//! Tokio integration and the HyperDeck REST API.
//!
//! The Ethernet client and server live in [`hyperdeck`] and only need an
//! [`embedded_io_async`] socket. This crate adapts `tokio::net::TcpStream`,
//! provides a tokio [`hyperdeck::Clock`], and implements the JSON REST API.

#![forbid(unsafe_code)]

mod net;
mod rest_client;
mod rest_server;
mod ws;

pub use hyperdeck;
pub use hyperdeck_core as core;
pub use net::{MockDeck, TokioClock, bind_mock, connect_tcp, serve, serve_mock};
pub use rest_client::{RestClient, encode_nas_url};
pub use rest_server::{AppState, DeckModel, router, serve_rest};
pub use ws::{EventClient, WsMessage};
