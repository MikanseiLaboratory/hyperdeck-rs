# hyperdeck-rs

Rust implementation of the Blackmagic HyperDeck Ethernet Protocol (TCP 9993) and the HyperDeck Control REST API, for controlling a deck and for building a deck.

The text protocol and the session state live in a sans-I/O core that builds for `thumbv6m` without an allocator. Async and blocking drivers sit on top of that core, so the same deck behaviour runs under tokio, a plain `std` thread (Tauri), or Embassy.

| Crate | Role |
| --- | --- |
| `hyperdeck-core` | `no_std` framing, commands, responses, timecode, an in-memory deck, REST models |
| `hyperdeck` | Async client and server over `embedded-io-async` |
| `hyperdeck-tokio` | Tokio TCP, reqwest REST client, axum REST server, WebSocket events |
| `hyperdeck-blocking` | Blocking client and thread-per-connection server |

Rust 1.99, edition 2024.

## Ethernet protocol

Messages are lines. A line that ends with `:` opens a block that runs until a blank line. Every other line is a complete message. The codec accepts LF and CRLF and writes CRLF.

A client may have one command in flight. `5xx` messages that arrive while that command is outstanding are notifications, not the response. The server side tracks the notification mask, the per-connection remote override, and the watchdog deadline. Bytes and the clock stay outside the core: callers pass them in.

```rust
use hyperdeck_tokio::connect_tcp;

let mut deck = connect_tcp("192.168.1.10:9993").await?;
deck.play().await?;
let transport = deck.transport_info().await?;
println!("{} {}", transport.status, transport.timecode);
```

`BlockingClient` is `Clone + Send + Sync`. Clones share one connection and take turns sending, which is the shape a Tauri command wants:

```rust
let deck = hyperdeck_blocking::BlockingClient::connect("192.168.1.10:9993")?;
deck.play()?;
```

A custom deck implements `hyperdeck::DeckHandler`. `SharedSim` is the built-in deck: two clips, playback, record, slots, and the configuration fields a controller usually reads. Commands the handler does not implement answer `103 unsupported`. Built-in connection commands (`ping`, `quit`, `help`, `commands`, `notify`, `remote`, `watchdog`, `device info`, `uptime`) never reach the handler.

## REST API

Enable `hyperdeck-core`'s `rest` feature for the December 2024 JSON models (`camelCase`, unknown fields ignored). `hyperdeck-tokio::RestClient` calls a deck under `/control/api/v1` unless the base URL already points at the API root. `hyperdeck-tokio::router` serves the same routes for a mock, and `serve_rest` binds that router. `EventClient` speaks the notification WebSocket (`subscribe`, `unsubscribe`, `listProperties`).

The mock router covers transport, playback, record, timecode, product, codec, video format, the media working set, clips, the timeline, record audio, the event list, and a fixed display list. NAS bookmark routes are on the client; the mock does not store them.

## Examples

```text
cargo run -p tokio-mock-deck -- 0.0.0.0:9993
cargo run -p tokio-client -- 127.0.0.1:9993

cargo run -p std-blocking-mock-deck -- 0.0.0.0:9993
cargo run -p std-blocking-client -- 127.0.0.1:9993
```

`examples/embassy-rp2040-w5500` is a separate package for the WIZnet W5500-EVB-Pico (RP2040, W5500 on SPI0, GPIO16–21). `mock-deck` listens on port 9993. `deck-client` connects to the address in `DECK`. It is excluded from the main workspace so host builds do not pull the HAL.

```text
cd examples/embassy-rp2040-w5500
cargo run --release --bin mock-deck
```

## Building for `no_std`

```text
cargo check -p hyperdeck-core --target thumbv6m-none-eabi --no-default-features
cargo check -p hyperdeck --target thumbv6m-none-eabi --no-default-features --features client,server,embassy-time
cargo check -p hyperdeck-blocking --target thumbv6m-none-eabi --no-default-features --features client
```

`hyperdeck-blocking` without `std` exposes `IoClient`, a single-threaded client over `embedded-io`. The `std` feature adds `BlockingClient` and the server.

## Tests

```text
cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

`cargo test --all-features` is not a host target: the `defmt` feature needs a device logger at link time. Clippy still type-checks that feature. GitHub Actions runs fmt, clippy, the workspace tests, the `thumbv6m` checks, and the Pico example.
