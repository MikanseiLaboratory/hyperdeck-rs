//! Blocking client for a process that does not run an async runtime.
//!
//! A Tauri command can call [`hyperdeck_blocking::BlockingClient`] the same way:
//! the handle is `Clone + Send + Sync`, and commands on one connection are
//! serialised.
//!
//! ```text
//! cargo run -p std-blocking-client -- 127.0.0.1:9993
//! ```

use hyperdeck_blocking::BlockingClient;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:9993".to_string());
    let client = BlockingClient::connect(addr)?;
    println!(
        "connected to {} (protocol {})",
        client.model(),
        client.protocol_version()
    );
    client.play()?;
    let info = client.transport_info()?;
    println!(
        "status {} speed {} timecode {} clip {}",
        info.status, info.speed, info.timecode, info.clip_id
    );
    Ok(())
}
