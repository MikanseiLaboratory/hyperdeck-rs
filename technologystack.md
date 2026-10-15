# Technology stack

## Language

- Rust 1.99 (`rust-toolchain.toml`)
- Edition 2024
- `unsafe_code` is forbidden in every workspace crate

## Crates

`hyperdeck-core` is `no_std` and does not allocate unless the `alloc` or `rest` feature is on. Framing uses fixed `heapless` buffers. REST models use `serde` and `alloc::string::String`.

`hyperdeck` is `no_std`. The socket is `embedded-io-async::{Read, Write}`. Timeouts and the watchdog go through the `Clock` trait, so the crate does not depend on tokio or on `embassy-time` unless that feature is enabled. Shared state uses `embassy-sync` (`Mutex`, `Channel`) with a caller-chosen raw mutex. On the host that mutex is `CriticalSectionRawMutex` with the `std` critical-section implementation. On RP2040 it is the same mutex with `embassy-rp`'s critical-section implementation.

`hyperdeck-tokio` adapts `tokio::net::TcpStream` with `embedded-io-adapters`. REST uses `reqwest` (rustls) and `axum`. The notification socket uses `tokio-tungstenite`.

`hyperdeck-blocking` uses synchronous `embedded-io` for `IoClient`. With `std`, `BlockingClient` owns a `TcpStream` and a reader thread, and `BlockingServer` accepts on `std::net::TcpListener` with one thread per connection.

## Targets

| Target | What runs |
| --- | --- |
| host | unit tests, tokio examples, blocking examples |
| `thumbv6m-none-eabi` | `hyperdeck-core`, `hyperdeck`, `hyperdeck-blocking` (`client` only), the W5500-EVB-Pico firmware |

The Pico example (its own workspace, so the HAL stays out of host builds) uses Embassy 0.10 / `embassy-net` 0.9 / `embassy-net-wiznet` 0.3. SPI0 talks to the W5500 on GPIO16 (MISO), GPIO17 (CS), GPIO18 (SCK), GPIO19 (MOSI), GPIO20 (reset), and GPIO21 (interrupt).

## Protocol references

The Ethernet codec follows the HyperDeck Ethernet Protocol as documented for protocol version 1.13: response classes `1xx` (failure), `2xx` (success), and `5xx` (asynchronous). The REST models follow the HyperDeck Control REST API (December 2024), including the requirement that `/` in a NAS URL path segment is encoded as `%2F`.
