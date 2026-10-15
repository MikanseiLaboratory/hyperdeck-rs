//! Text codec for the HyperDeck Ethernet Protocol.
//!
//! Messages are line oriented. A line that ends with `:` starts a block that
//! runs until a blank line. Every other line is a complete message. Clients
//! may send LF or CRLF; this codec accepts both and writes CRLF.

mod command;
mod encode;
mod framer;
mod params;
mod response;

pub use command::Command;
pub use encode::{
    ByteBuf, COMMANDS_XML, HELP_TEXT, SliceWriter, encode_block, encode_command, encode_lines,
    encode_simple,
};
pub use framer::LineFramer;
pub use params::parse_inline_params;
pub use response::{RawMessage, Response};

use crate::error::Error;

/// Failure while writing into a fixed buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WriteError;

impl From<core::fmt::Error> for WriteError {
    fn from(_: core::fmt::Error) -> Self {
        Self
    }
}

impl From<WriteError> for Error {
    fn from(_: WriteError) -> Self {
        Error::BufferFull
    }
}
