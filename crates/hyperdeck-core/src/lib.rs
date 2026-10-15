//! Sans-I/O core for the Blackmagic HyperDeck protocols.
//!
//! This crate turns bytes into typed commands and responses, and tracks the
//! small amount of connection state the text protocol requires (one in-flight
//! command, notification subscriptions, watchdog deadline). It does not open
//! sockets, allocate, or depend on an async runtime, so the same types run
//! under tokio, a blocking `std` thread, or Embassy on `thumbv6m`.
//!
//! # Layers
//!
//! - [`types`] — timecode, video format, response codes
//! - [`ethernet`] — framing, parsing, encoding
//! - [`session`] — client and server state machines
//! - [`sim`] — an in-memory deck used by the mock servers
//!
//! Enable the `rest` feature for the JSON models of the HyperDeck REST API.

#![no_std]
#![forbid(unsafe_code)]

#[cfg(feature = "alloc")]
extern crate alloc;

pub mod error;
pub mod ethernet;
pub mod session;
pub mod sim;
pub mod types;

#[cfg(feature = "rest")]
pub mod rest;

pub use error::Error;
pub use ethernet::{ByteBuf, Command, LineFramer, RawMessage, Response, SliceWriter, WriteError};
pub use session::{ClientEvent, ClientSession, NotifyFlags, NotifyKind, ServerSession};
pub use sim::{ApplyResult, DeckSim, SimClip};
pub use types::{FrameRate, ResponseCode, Scan, Timecode, TimecodeOffset, VideoFormat};
