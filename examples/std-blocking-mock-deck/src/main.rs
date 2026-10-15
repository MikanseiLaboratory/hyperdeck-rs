//! Blocking mock HyperDeck. No async runtime.
//!
//! ```text
//! cargo run -p std-blocking-mock-deck -- 0.0.0.0:9993
//! ```

use std::sync::Arc;
use std::thread;
use std::time::Duration;

use hyperdeck_blocking::{BlockingServer, SharedDeck};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let bind = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "0.0.0.0:9993".to_string());
    let deck = Arc::new(SharedDeck::demo());
    let ticking = Arc::clone(&deck);
    thread::spawn(move || {
        loop {
            ticking.tick(1);
            thread::sleep(Duration::from_millis(33));
        }
    });
    let server = BlockingServer::bind(bind)?;
    eprintln!("HyperDeck Ethernet listening on {}", server.local_addr()?);
    server.serve(deck)?;
    Ok(())
}
