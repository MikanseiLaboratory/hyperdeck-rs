//! Client session: one in-flight command, with asynchronous notices peeled off.

use crate::error::Error;
use crate::ethernet::{LineFramer, RawMessage, Response};
use crate::types::CodeClass;

/// Something the server sent.
#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum ClientEvent<const N: usize> {
    /// The `500 connection info` preamble.
    ConnectionInfo(RawMessage<N>),
    /// The response to the command currently in flight.
    Response(RawMessage<N>),
    /// A `5xx` notice other than the connection preamble.
    Notification(RawMessage<N>),
}

/// Sans-I/O client.
///
/// Call [`ClientSession::begin_command`] before writing a command, feed every
/// received byte to [`ClientSession::push`], then drain [`ClientSession::poll`]
/// until it yields a [`ClientEvent::Response`]. Notices that arrive while a
/// command is in flight are returned first and do not complete the command.
#[derive(Debug)]
pub struct ClientSession<const N: usize> {
    framer: LineFramer<N>,
    waiting: bool,
    /// After a timeout, drop the next non-notification so a late response
    /// cannot be paired with a later command.
    discard_response: bool,
}

impl<const N: usize> ClientSession<N> {
    /// A session that is not waiting for a response.
    pub const fn new() -> Self {
        Self {
            framer: LineFramer::new(),
            waiting: false,
            discard_response: false,
        }
    }

    /// Marks a command as in flight.
    ///
    /// Returns [`Error::Busy`] when a response is already outstanding.
    pub fn begin_command(&mut self) -> Result<(), Error> {
        if self.waiting {
            return Err(Error::Busy);
        }
        self.waiting = true;
        Ok(())
    }

    /// `true` while a command is waiting for its response.
    pub fn is_waiting(&self) -> bool {
        self.waiting
    }

    /// Forgets the in-flight command after a timeout or a cancelled write.
    pub fn abandon(&mut self) {
        self.waiting = false;
        self.discard_response = true;
    }

    /// Appends bytes received from the socket.
    pub fn push(&mut self, data: &[u8]) -> Result<(), Error> {
        self.framer.push(data)
    }

    /// Pops the next decoded event.
    ///
    /// `M` is the capacity of the returned message and must be at least `N`
    /// for every message the framer can produce. Use `M = N`.
    pub fn poll<const M: usize>(&mut self) -> Result<Option<ClientEvent<M>>, Error> {
        let Some(bytes) = self.framer.pop::<M>()? else {
            return Ok(None);
        };
        let message = RawMessage::from_slice(bytes.as_slice())?;
        let response = message.response()?;
        let event = classify(&response);
        match event {
            Kind::Response if self.discard_response => {
                self.discard_response = false;
                // Look for the next message in the same buffer.
                self.poll()
            }
            Kind::Response => {
                self.waiting = false;
                Ok(Some(ClientEvent::Response(message)))
            }
            Kind::Connection => Ok(Some(ClientEvent::ConnectionInfo(message))),
            Kind::Notification => Ok(Some(ClientEvent::Notification(message))),
        }
    }
}

impl<const N: usize> Default for ClientSession<N> {
    fn default() -> Self {
        Self::new()
    }
}

enum Kind {
    Connection,
    Notification,
    Response,
}

fn classify(response: &Response<'_>) -> Kind {
    if response.code == crate::types::ResponseCode::CONNECTION_INFO {
        return Kind::Connection;
    }
    match response.code.class() {
        CodeClass::Async => Kind::Notification,
        _ => Kind::Response,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notification_does_not_complete_a_command() {
        let mut session = ClientSession::<256>::new();
        session.begin_command().unwrap();
        session
            .push(b"508 transport info:\r\nstatus: play\r\n\r\n200 ok\r\n")
            .unwrap();
        match session.poll::<256>().unwrap().unwrap() {
            ClientEvent::Notification(_) => {}
            other => panic!("expected notification, got {other:?}"),
        }
        assert!(session.is_waiting());
        match session.poll::<256>().unwrap().unwrap() {
            ClientEvent::Response(message) => {
                assert_eq!(message.response().unwrap().code.as_u16(), 200);
            }
            other => panic!("expected response, got {other:?}"),
        }
        assert!(!session.is_waiting());
    }

    #[test]
    fn second_command_is_busy() {
        let mut session = ClientSession::<64>::new();
        session.begin_command().unwrap();
        assert_eq!(session.begin_command(), Err(Error::Busy));
    }
}
