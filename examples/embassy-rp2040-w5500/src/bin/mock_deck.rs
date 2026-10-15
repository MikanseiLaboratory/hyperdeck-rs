//! Mock HyperDeck on a WIZnet W5500-EVB-Pico.
//!
//! Listens on TCP port 9993. One client is served at a time. GPIO16–21 are
//! wired to the onboard W5500 and are not available for anything else.
//!
//! ```text
//! cargo run --release --bin mock-deck
//! ```

#![no_std]
#![no_main]

use defmt::info;
use defmt_rtt as _;
use embassy_executor::Spawner;
use embassy_net::tcp::TcpSocket;
use embassy_net::{Config, Stack, StackResources};
use embassy_net_wiznet::chip::W5500;
use embassy_net_wiznet::{Device, Runner, State};
use embassy_rp::clocks::RoscRng;
use embassy_rp::gpio::{Input, Level, Output, Pull};
use embassy_rp::peripherals::{DMA_CH0, DMA_CH1, SPI0};
use embassy_rp::spi::{Async, Config as SpiConfig, Spi};
use embassy_rp::{bind_interrupts, dma};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_time::{Delay, Duration, Timer};
use embedded_hal_bus::spi::ExclusiveDevice;
use hyperdeck::core::session::NotifyKind;
use hyperdeck::server::Notice;
use hyperdeck::{DEMO_IDENTITY, EmbassyClock, Hub, ServeArgs, SharedSim, serve_connection};
use panic_probe as _;
use static_cell::StaticCell;

bind_interrupts!(struct Irqs {
    DMA_IRQ_0 => dma::InterruptHandler<DMA_CH0>, dma::InterruptHandler<DMA_CH1>;
});

type SpiBus = Spi<'static, SPI0, Async>;
type SpiDev = ExclusiveDevice<SpiBus, Output<'static>, Delay>;

#[embassy_executor::task]
async fn ethernet_task(
    runner: Runner<'static, W5500, SpiDev, Input<'static>, Output<'static>>,
) -> ! {
    runner.run().await
}

#[embassy_executor::task]
async fn net_task(mut runner: embassy_net::Runner<'static, Device<'static>>) -> ! {
    runner.run().await
}

#[embassy_executor::task]
async fn tick(
    sim: &'static SharedSim<CriticalSectionRawMutex>,
    hub: &'static Hub<CriticalSectionRawMutex, 4>,
) -> ! {
    loop {
        if let Some(bytes) = sim.tick(1).await {
            hub.publish(Notice {
                kind: NotifyKind::Transport,
                bytes,
            });
        }
        Timer::after(Duration::from_millis(33)).await;
    }
}

#[embassy_executor::task]
async fn deck_task(
    stack: Stack<'static>,
    sim: &'static SharedSim<CriticalSectionRawMutex>,
    hub: &'static Hub<CriticalSectionRawMutex, 4>,
) -> ! {
    let mut rx = [0u8; 2048];
    let mut tx = [0u8; 2048];
    let clock = EmbassyClock;
    loop {
        let mut socket = TcpSocket::new(stack, &mut rx, &mut tx);
        if socket.accept(9993).await.is_err() {
            continue;
        }
        let Some(conn_id) = hub.acquire().await else {
            let _ = hyperdeck::reject_connection(&mut socket).await;
            continue;
        };
        info!("client connected");
        let _ = serve_connection(
            &mut socket,
            ServeArgs {
                handler: sim,
                hub,
                conn_id,
                clock: &clock,
                identity: &DEMO_IDENTITY,
                boot_ms: 0,
            },
        )
        .await;
    }
}

#[embassy_executor::main(
    executor = "embassy_rp::executor::Executor",
    entry = "cortex_m_rt::entry"
)]
async fn main(spawner: Spawner) {
    let p = embassy_rp::init(Default::default());
    let mut rng = RoscRng;

    let mut spi_cfg = SpiConfig::default();
    spi_cfg.frequency = 50_000_000;
    let spi = Spi::new(
        p.SPI0, p.PIN_18, p.PIN_19, p.PIN_16, p.DMA_CH0, p.DMA_CH1, Irqs, spi_cfg,
    );
    let cs = Output::new(p.PIN_17, Level::High);
    let w5500_int = Input::new(p.PIN_21, Pull::Up);
    let w5500_reset = Output::new(p.PIN_20, Level::High);

    static STATE: StaticCell<State<2, 2>> = StaticCell::new();
    let (device, runner) = embassy_net_wiznet::new(
        [0x02, 0x00, 0x00, 0x00, 0x00, 0x01],
        STATE.init(State::<2, 2>::new()),
        ExclusiveDevice::new(spi, cs, Delay),
        w5500_int,
        w5500_reset,
    )
    .await
    .unwrap();
    spawner.spawn(ethernet_task(runner).unwrap());

    static RESOURCES: StaticCell<StackResources<3>> = StaticCell::new();
    let (stack, net_runner) = embassy_net::new(
        device,
        Config::dhcpv4(Default::default()),
        RESOURCES.init(StackResources::new()),
        rng.next_u64(),
    );
    spawner.spawn(net_task(net_runner).unwrap());

    info!("waiting for DHCP");
    stack.wait_config_up().await;
    info!("HyperDeck Ethernet on port 9993");

    static SIM: StaticCell<SharedSim<CriticalSectionRawMutex>> = StaticCell::new();
    static HUB: StaticCell<Hub<CriticalSectionRawMutex, 4>> = StaticCell::new();
    let sim = SIM.init(SharedSim::demo());
    let hub = HUB.init(Hub::new(true));
    spawner.spawn(tick(sim, hub).unwrap());
    spawner.spawn(deck_task(stack, sim, hub).unwrap());
}
