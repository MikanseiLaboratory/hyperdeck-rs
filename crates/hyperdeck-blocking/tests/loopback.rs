//! Blocking client against the blocking mock deck.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use hyperdeck_blocking::{BlockingClient, BlockingServer, SharedDeck};

#[test]
fn play_stop_and_remote_disable() {
    let deck = Arc::new(SharedDeck::demo());
    let server = BlockingServer::bind("127.0.0.1:0").unwrap();
    let addr = server.local_addr().unwrap();
    let shutdown = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&shutdown);
    thread::spawn(move || {
        server.serve_until(deck, &flag).unwrap();
    });

    let client = BlockingClient::connect(addr).unwrap();
    assert_eq!(client.model(), "HyperDeck Rust");
    assert_eq!(client.protocol_version(), "1.13");
    client.play().unwrap();
    let info = client.transport_info().unwrap();
    assert_eq!(info.status, "play");
    assert_eq!(info.speed, 100);
    client.set_notify("transport", true).unwrap();
    client.stop().unwrap();
    let notice = client
        .recv_notification_timeout(Duration::from_secs(1))
        .expect("transport notice");
    assert!(notice.starts_with("508 transport info"), "{notice}");
    client.raw("remote: enable: false").unwrap();
    let err = client.play().unwrap_err();
    let message = err.to_string();
    assert!(message.contains("111"), "{message}");
    client.quit().unwrap();
    shutdown.store(true, Ordering::Relaxed);
}
