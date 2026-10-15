# W5500-EVB-Pico

Firmware for the WIZnet W5500-EVB-Pico (RP2040 + W5500).

The W5500 is on SPI0:

| Signal | GPIO |
| --- | --- |
| MISO | 16 |
| CS | 17 |
| SCK | 18 |
| MOSI | 19 |
| Reset | 20 |
| Interrupt | 21 |

`mock-deck` obtains an address with DHCP and serves the HyperDeck Ethernet Protocol on port 9993. One client is accepted at a time.

`deck-client` obtains an address with DHCP and controls the recorder at `DECK` in `src/bin/deck_client.rs` (port 9993).

```text
cargo run --release --bin mock-deck
cargo run --release --bin deck-client
```

`probe-rs` is the runner (`probe-rs run --chip RP2040`). `cargo check --bins` is what CI runs; it does not flash a board.
