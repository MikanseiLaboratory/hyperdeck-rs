//! Connection state that does not perform I/O.

mod client;
mod server;

pub use client::{ClientEvent, ClientSession};
pub use server::{NotifyFlags, NotifyKind, ServerSession};
