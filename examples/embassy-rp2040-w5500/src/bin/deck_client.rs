//! HyperDeck client on a WIZnet W5500-EVB-Pico.
//!
//! After DHCP the board connects to [`DECK`] port 9993, enables transport
//! notifications, and plays. GPIO16–21 belong to the onboard W5500.
//!
//! ```text
//! cargo run --release --bin deck-client
//! ```

#![no_std]
#![no_main]

use defmt::info;
use defmt_rtt as _;
use embassy_executor::Spawner;
use embassy_net::tcp::TcpSocket;
use embassy_net::{Config, IpAddress, IpEndpoint, Ipv4Address, Stack, StackResources};
use embassy_net_wiznet::chip::W5500;
use embassy_net_wiznet::{Device, Runner, State};
use embassy_rp::clocks::RoscRng;
use embassy_rp::gpio::{Input, Level, Output, Pull};
use embassy_rp::peripherals::{DMA_CH0, DMA_CH1, SPI0};
use embassy_rp::spi::{Async, Config as SpiConfig, Spi};
use embassy_rp::{bind_interrupts, dma};
use embassy_time::{Delay, Duration, Timer};
use embedded_hal_bus::spi::ExclusiveDevice;
use hyperdeck::{EmbassyClock, HyperDeckClient};
use panic_probe as _;
use static_cell::StaticCell;

/// Address of the HyperDeck this client controls.
const DECK: [u8; 4] = [192, 168, 1, 50];

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
async fn control(stack: Stack<'static>) -> ! {
    let remote = IpEndpoint::new(
        IpAddress::Ipv4(Ipv4Address::new(DECK[0], DECK[1], DECK[2], DECK[3])),
        9993,
    );
    let mut rx = [0u8; 2048];
    let mut tx = [0u8; 2048];
    loop {
        let mut socket = TcpSocket::new(stack, &mut rx, &mut tx);
        if socket.connect(remote).await.is_err() {
            Timer::after(Duration::from_secs(1)).await;
            continue;
        }
        let Ok(mut client) =
            HyperDeckClient::<_, _, 2048>::connect(socket, EmbassyClock, 3_000).await
        else {
            Timer::after(Duration::from_secs(1)).await;
            continue;
        };
        info!("connected");
        if client.set_notify("transport", true).await.is_err() || client.play().await.is_err() {
            continue;
        }
        loop {
            match client.transport_info().await {
                Ok(snapshot) => info!("timeline frame {}", snapshot.timeline),
                Err(_) => break,
            }
            Timer::after(Duration::from_secs(1)).await;
        }
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
        [0x02, 0x00, 0x00, 0x00, 0x00, 0x02],
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

    stack.wait_config_up().await;
    info!("controlling the deck on port 9993");
    spawner.spawn(control(stack).unwrap());
}
