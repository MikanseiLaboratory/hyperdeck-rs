//! Async HyperDeck client and server.
//!
//! The socket is an [`embedded_io_async`] reader and writer. tokio adapts its
//! `TcpStream` in `hyperdeck-tokio`; Embassy passes a `TcpSocket` directly.
//! Bytes are interpreted by [`hyperdeck_core`], which is sans-I/O and `no_std`.

#![no_std]
#![forbid(unsafe_code)]

#[cfg(feature = "client")]
mod client;
mod error;
#[cfg(feature = "server")]
pub mod server;
mod time;

#[cfg(feature = "client")]
pub use client::{ClientHub, HyperDeckClient, TransportSnapshot, drive_client};
pub use error::Error;
pub use hyperdeck_core as core;
#[cfg(feature = "server")]
pub use server::{
    DEMO_IDENTITY, DeckHandler, DeviceIdentity, Hub, Outcome, ServeArgs, SharedSim,
    reject_connection, serve_connection,
};
pub use time::{Clock, EmbassyClock};
