//! Blocking HyperDeck client and server.
//!
//! [`IoClient`] speaks the Ethernet protocol through synchronous
//! [`embedded_io`] traits, so it does not need an async runtime and builds for
//! `no_std`. The `std` feature adds [`BlockingClient`], a `Clone + Send + Sync`
//! handle aimed at Tauri commands, and a thread-per-connection server.

#![cfg_attr(not(feature = "std"), no_std)]
#![forbid(unsafe_code)]

#[cfg(feature = "client")]
mod client;
mod error;
#[cfg(feature = "server")]
mod server;

#[cfg(feature = "client")]
pub use client::IoClient;
#[cfg(all(feature = "client", feature = "std"))]
pub use client::{BlockingClient, TransportSnapshot};
pub use error::Error;
pub use hyperdeck_core as core;
#[cfg(feature = "server")]
pub use server::{BlockingServer, DEMO_IDENTITY, DeviceIdentity, SharedDeck};
