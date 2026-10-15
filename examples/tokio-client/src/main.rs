//! Connects to a HyperDeck (or a mock) and plays the current timeline.
//!
//! ```text
//! cargo run -p tokio-client -- 127.0.0.1:9993
//! ```

use hyperdeck_tokio::connect_tcp;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:9993".to_string());
    let mut client = connect_tcp(&addr).await?;
    println!(
        "connected to {} (protocol {})",
        client.model(),
        client.protocol_version()
    );
    client.play().await?;
    let info = client.transport_info().await?;
    println!(
        "status {} speed {} timecode {} clip {}",
        info.status, info.speed, info.timecode, info.clip_id
    );
    Ok(())
}
