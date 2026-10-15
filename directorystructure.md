# Directory structure

```text
.
├── Cargo.toml                  workspace (resolver 3, edition 2024)
├── rust-toolchain.toml         Rust 1.99, rustfmt, clippy, thumbv6m-none-eabi
├── rustfmt.toml
├── crates/
│   ├── hyperdeck-core/         sans-I/O protocol, deck simulator, REST models
│   │   └── src/
│   │       ├── ethernet/       framer, command, response, parameter codec
│   │       ├── session/        client and server state machines
│   │       ├── types/          timecode, video format, response codes
│   │       ├── sim.rs          in-memory deck
│   │       └── rest.rs         serde models (`rest` feature)
│   ├── hyperdeck/              async client, server, Clock
│   │   └── src/
│   │       ├── client.rs
│   │       └── server/         connection loop, DeckHandler, Hub, SharedSim
│   ├── hyperdeck-tokio/        TCP, REST client, axum router, WebSocket
│   └── hyperdeck-blocking/     IoClient, BlockingClient, BlockingServer
├── examples/
│   ├── tokio-client/
│   ├── tokio-mock-deck/
│   ├── std-blocking-client/
│   ├── std-blocking-mock-deck/
│   └── embassy-rp2040-w5500/   separate workspace, W5500-EVB-Pico
│       └── src/bin/
│           ├── mock_deck.rs
│           └── deck_client.rs
└── .github/workflows/ci.yml
```

`examples/embassy-rp2040-w5500` is listed in the root workspace `exclude` array. Building it with `--manifest-path` uses its own `[workspace]` table, its `memory.x`, and `.cargo/config.toml` (the `thumbv6m-none-eabi` target and the single-core atomic cfg).
