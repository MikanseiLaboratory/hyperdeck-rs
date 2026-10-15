//! Ethernet and REST loopback, including the blocking client.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use hyperdeck_blocking::{BlockingClient, BlockingServer, SharedDeck};
use hyperdeck_tokio::hyperdeck::DEMO_IDENTITY;
use hyperdeck_tokio::{AppState, RestClient, bind_mock, connect_tcp, serve, serve_rest};

#[tokio::test]
async fn ethernet_play_and_stop() {
    let (deck, addr, listener) = bind_mock().await.unwrap();
    let ticking = deck.clone();
    tokio::spawn(async move {
        loop {
            ticking.tick(1).await;
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    });
    let sim = deck.sim.clone();
    let hub = deck.hub.clone();
    let clock = deck.clock.clone();
    tokio::spawn(async move {
        let _ = serve(listener, sim, hub, DEMO_IDENTITY, clock).await;
    });

    let mut client = connect_tcp(addr).await.unwrap();
    assert_eq!(client.model(), "HyperDeck Rust");
    client.play().await.unwrap();
    let info = client.transport_info().await.unwrap();
    assert_eq!(info.status.as_str(), "play");
    assert_eq!(info.speed, 100);
    client.stop().await.unwrap();
    let stopped = client.transport_info().await.unwrap();
    assert_eq!(stopped.status.as_str(), "stopped");
    client.quit().await.unwrap();
}

#[tokio::test]
async fn blocking_client_talks_to_tokio_server() {
    let (deck, addr, listener) = bind_mock().await.unwrap();
    let sim = deck.sim.clone();
    let hub = deck.hub.clone();
    let clock = deck.clock.clone();
    tokio::spawn(async move {
        let _ = serve(listener, sim, hub, DEMO_IDENTITY, clock).await;
    });

    let info = tokio::task::spawn_blocking(move || {
        let client = BlockingClient::connect(addr).unwrap();
        client.play().unwrap();
        client.transport_info().unwrap()
    })
    .await
    .unwrap();
    assert_eq!(info.status, "play");
}

#[test]
fn tokio_client_talks_to_blocking_server() {
    let deck = Arc::new(SharedDeck::demo());
    let server = BlockingServer::bind("127.0.0.1:0").unwrap();
    let addr = server.local_addr().unwrap();
    let shutdown = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&shutdown);
    std::thread::spawn(move || {
        server.serve_until(deck, &flag).unwrap();
    });

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async move {
        let mut client = connect_tcp(addr).await.unwrap();
        client.record(Some("Take")).await.unwrap();
        let info = client.transport_info().await.unwrap();
        assert_eq!(info.status.as_str(), "record");
        client.quit().await.unwrap();
    });
    shutdown.store(true, Ordering::Relaxed);
}

#[tokio::test]
async fn rest_transport_and_clips() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        serve_rest(listener, AppState::demo()).await.unwrap();
    });

    let client = RestClient::new(&format!("http://{addr}"));
    let product = client.product().await.unwrap();
    assert_eq!(product.device_name, "Mock HyperDeck");
    client.play().await.unwrap();
    let playback = client.playback().await.unwrap();
    assert_eq!(playback.speed, 1.0);
    assert_eq!(client.clips().await.unwrap().len(), 2);
    client.record(Some("new.mov")).await.unwrap();
    let clips = client.clips().await.unwrap();
    assert_eq!(clips.len(), 3);
    assert_eq!(clips[2].file_path, "new.mov");
    client.clear_timeline().await.unwrap();
    assert!(client.timeline().await.unwrap().clips.is_empty());
    let events = client.events().await.unwrap();
    assert!(events.iter().any(|event| event == "/transports/0"));
}
