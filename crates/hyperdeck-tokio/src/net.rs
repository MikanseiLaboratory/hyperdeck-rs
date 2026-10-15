//! TCP helpers for the runtime-agnostic client and server.

use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embedded_io_adapters::tokio_1::FromTokio;
use hyperdeck::core::session::NotifyKind;
use hyperdeck::{
    Clock, DeckHandler, DeviceIdentity, Error, Hub, HyperDeckClient, ServeArgs, SharedSim,
    serve_connection,
};
use tokio::net::{TcpListener, TcpStream};

/// Tokio clock. Milliseconds are counted from construction.
#[derive(Clone)]
pub struct TokioClock {
    start: Instant,
}

impl TokioClock {
    /// Starts the clock at the current instant.
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
        }
    }
}

impl Default for TokioClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for TokioClock {
    fn now_ms(&self) -> u64 {
        u64::try_from(self.start.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    // `async fn` would drop the `Send` bound [`hyperdeck::Clock`] promises.
    #[allow(clippy::manual_async_fn)]
    fn sleep(&self, ms: u64) -> impl Future<Output = ()> + Send + '_ {
        async move {
            tokio::time::sleep(Duration::from_millis(ms)).await;
        }
    }
}

/// Connects to a HyperDeck at `addr` (port 9993 in normal use).
pub async fn connect_tcp(
    addr: impl tokio::net::ToSocketAddrs,
) -> Result<HyperDeckClient<FromTokio<TcpStream>, TokioClock, 2048>, Error<std::io::Error>> {
    let stream = TcpStream::connect(addr).await.map_err(Error::Io)?;
    stream.set_nodelay(true).map_err(Error::Io)?;
    HyperDeckClient::connect(FromTokio::new(stream), TokioClock::new(), 3_000).await
}

/// Accepts Ethernet clients until the listener closes.
///
/// `hub` must be the same hub the handler publishes to. [`spawn_mock`] builds
/// both from [`SharedSim`].
pub async fn serve<H>(
    listener: TcpListener,
    handler: Arc<H>,
    hub: Arc<Hub<CriticalSectionRawMutex, 4>>,
    identity: DeviceIdentity,
    clock: TokioClock,
) -> Result<(), Error<std::io::Error>>
where
    H: DeckHandler + Sync + Send + 'static,
{
    loop {
        let (stream, _peer) = listener.accept().await.map_err(Error::Io)?;
        stream.set_nodelay(true).map_err(Error::Io)?;
        let handler = Arc::clone(&handler);
        let hub = Arc::clone(&hub);
        let clock = clock.clone();
        tokio::spawn(async move {
            let mut io = FromTokio::new(stream);
            let Some(conn_id) = hub.acquire().await else {
                let _ = hyperdeck::reject_connection(&mut io).await;
                return;
            };
            let _ = serve_connection(
                &mut io,
                ServeArgs {
                    handler: handler.as_ref(),
                    hub: hub.as_ref(),
                    conn_id,
                    clock: &clock,
                    identity: &identity,
                    boot_ms: 0,
                },
            )
            .await;
        });
    }
}

/// A mock deck plus the hub playback notices are published on.
#[derive(Clone)]
pub struct MockDeck {
    /// In-memory transport and timeline.
    pub sim: Arc<SharedSim<CriticalSectionRawMutex>>,
    /// Notice fan-out. Capacity is four connections.
    pub hub: Arc<Hub<CriticalSectionRawMutex, 4>>,
    /// Clock shared with the server.
    pub clock: TokioClock,
}

impl MockDeck {
    /// Demo timeline, remote control enabled.
    pub fn demo() -> Self {
        Self {
            sim: Arc::new(SharedSim::demo()),
            hub: Arc::new(Hub::new(true)),
            clock: TokioClock::new(),
        }
    }

    /// Advances playback and publishes `508 transport info` when it changes.
    pub async fn tick(&self, frames: u32) {
        if let Some(bytes) = self.sim.tick(frames).await {
            self.hub.publish(hyperdeck::server::Notice {
                kind: NotifyKind::Transport,
                bytes,
            });
        }
    }
}

/// Binds `addr` and serves `deck` until the task is aborted.
pub async fn serve_mock(addr: SocketAddr, deck: MockDeck) -> Result<(), Error<std::io::Error>> {
    let listener = TcpListener::bind(addr).await.map_err(Error::Io)?;
    let MockDeck { sim, hub, clock } = deck;
    serve(listener, sim, hub, hyperdeck::DEMO_IDENTITY, clock).await
}

/// Local address of a freshly bound mock. The caller receives the deck and the bound port.
pub async fn bind_mock() -> std::io::Result<(MockDeck, SocketAddr, TcpListener)> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    Ok((MockDeck::demo(), addr, listener))
}
