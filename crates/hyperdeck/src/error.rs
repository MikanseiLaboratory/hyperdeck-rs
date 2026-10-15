//! Errors from the async client and server.

use core::fmt;

/// I/O and protocol failures.
///
/// `E` is the [`embedded_io_async::ErrorType::Error`] of the socket.
#[derive(Debug)]
pub enum Error<E> {
    /// The socket returned an error.
    Io(E),
    /// Framing or parsing failed.
    Protocol(hyperdeck_core::Error),
    /// The deck rejected the command with a `1xx` code.
    Device {
        /// Numeric code.
        code: u16,
        /// Reason phrase.
        text: heapless::String<64>,
    },
    /// No bytes arrived before the deadline.
    Timeout,
    /// The peer closed the connection.
    Closed,
    /// A fixed buffer could not hold the message.
    BufferFull,
    /// The server has no free connection slot.
    TooManyConnections,
}

impl<E: fmt::Debug> fmt::Display for Error<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "I/O error: {err:?}"),
            Self::Protocol(err) => write!(f, "{err}"),
            Self::Device { code, text } => write!(f, "deck error {code} {text}"),
            Self::Timeout => f.write_str("timed out waiting for the deck"),
            Self::Closed => f.write_str("connection closed"),
            Self::BufferFull => f.write_str("buffer full"),
            Self::TooManyConnections => f.write_str("too many connections"),
        }
    }
}

impl<E: fmt::Debug> core::error::Error for Error<E> {}

impl<E> From<hyperdeck_core::Error> for Error<E> {
    fn from(value: hyperdeck_core::Error) -> Self {
        match value {
            hyperdeck_core::Error::BufferFull | hyperdeck_core::Error::Overflow => Self::BufferFull,
            other => Self::Protocol(other),
        }
    }
}
