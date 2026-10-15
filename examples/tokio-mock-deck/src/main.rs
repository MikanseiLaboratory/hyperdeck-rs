//! Mock HyperDeck Ethernet server on port 9993.
//!
//! Playback advances about once per frame-period so `notify: transport: true`
//! clients see `508 transport info`.
//!
//! ```text
//! cargo run -p tokio-mock-deck -- 0.0.0.0:9993
//! ```

use std::time::Duration;

use hyperdeck_tokio::hyperdeck::DEMO_IDENTITY;
use hyperdeck_tokio::{MockDeck, serve};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let bind = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "0.0.0.0:9993".to_string());
    let listener = TcpListener::bind(&bind).await?;
    let deck = MockDeck::demo();
    let ticking = deck.clone();
    tokio::spawn(async move {
        loop {
            ticking.tick(1).await;
            tokio::time::sleep(Duration::from_millis(33)).await;
        }
    });
    eprintln!("HyperDeck Ethernet listening on {bind}");
    serve(listener, deck.sim, deck.hub, DEMO_IDENTITY, deck.clock).await?;
    Ok(())
}
