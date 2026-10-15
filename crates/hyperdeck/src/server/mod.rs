//! Async HyperDeck server.
//!
//! [`serve_connection`] speaks the Ethernet protocol on one socket. Deck
//! behaviour comes from [`DeckHandler`]. A reference implementation backed by
//! [`hyperdeck_core::DeckSim`] is [`SharedSim`].

mod connection;
mod handler;
mod hub;
mod sim_handler;

pub use connection::{
    DEMO_IDENTITY, DeviceIdentity, ServeArgs, reject_connection, serve_connection,
};
pub use handler::{DeckHandler, Outcome, ReplyWriter, dispatch_commands};
pub use hub::{Hub, Notice};
pub use sim_handler::SharedSim;
