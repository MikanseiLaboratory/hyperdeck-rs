//! Errors produced by the sans-I/O core.

use core::fmt;

/// Failure while framing, parsing, or stepping a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// The input buffer cannot hold the current message.
    Overflow,
    /// A completed message was not valid UTF-8.
    InvalidUtf8,
    /// The message does not match the protocol grammar.
    Syntax,
    /// A numeric field was out of range or not a number.
    InvalidValue,
    /// A command was started while another response is still outstanding.
    Busy,
    /// The output buffer ran out of room.
    BufferFull,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Self::Overflow => "message exceeded the framing buffer",
            Self::InvalidUtf8 => "message was not valid UTF-8",
            Self::Syntax => "message did not match the HyperDeck grammar",
            Self::InvalidValue => "a field had an invalid value",
            Self::Busy => "a command is already waiting for a response",
            Self::BufferFull => "the output buffer is full",
        };
        f.write_str(text)
    }
}
