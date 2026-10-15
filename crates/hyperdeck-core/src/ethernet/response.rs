//! Borrowed responses and an owned message buffer.

use core::fmt;
use core::str;

use heapless::Vec;

use super::params::parse_param_line;
use crate::error::Error;
use crate::types::ResponseCode;

/// One server message.
///
/// `text` is the reason phrase. When the header line ends with `:`, `lines`
/// holds the following parameter lines (already without the trailing blank line).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response<'a> {
    /// Numeric status.
    pub code: ResponseCode,
    /// Reason phrase, without a trailing colon.
    pub text: &'a str,
    /// Raw body between the header and the terminating blank line.
    pub body: &'a str,
}

impl<'a> Response<'a> {
    /// Parses one framed message.
    pub fn parse(message: &'a str) -> Result<Self, Error> {
        let text = message.trim_end_matches(['\r', '\n']);
        let Some((header, rest)) = split_first_line(text) else {
            return Err(Error::Syntax);
        };
        let header = header.trim();
        let mut parts = header.splitn(2, ' ');
        let code_text = parts.next().ok_or(Error::Syntax)?;
        if code_text.len() != 3 || !code_text.bytes().all(|b| b.is_ascii_digit()) {
            return Err(Error::Syntax);
        }
        let code = ResponseCode::new(code_text.parse().map_err(|_| Error::Syntax)?);
        let mut reason = parts.next().unwrap_or("").trim();
        if let Some(stripped) = reason.strip_suffix(':') {
            reason = stripped.trim();
        }
        Ok(Self {
            code,
            text: reason,
            body: rest.trim_matches(['\r', '\n']),
        })
    }

    /// Iterates `key: value` body lines.
    ///
    /// Lines that do not contain a colon (clip rows, help text, XML) are skipped.
    /// Use [`Self::body`] when the body is not a parameter list.
    pub fn params(&self) -> impl Iterator<Item = (&'a str, &'a str)> {
        self.body.lines().filter_map(|line| {
            let line = line.trim_end_matches('\r').trim();
            if line.is_empty() {
                None
            } else {
                parse_param_line(line)
            }
        })
    }

    /// Looks up one body parameter.
    pub fn get(&self, key: &str) -> Option<&'a str> {
        self.params().find(|(name, _)| *name == key).map(|(_, v)| v)
    }
}

fn split_first_line(message: &str) -> Option<(&str, &str)> {
    if let Some((head, tail)) = message.split_once("\r\n") {
        return Some((head, tail));
    }
    if let Some((head, tail)) = message.split_once('\n') {
        return Some((head, tail));
    }
    // A single line with the newline already trimmed.
    if message.is_empty() {
        None
    } else {
        Some((message, ""))
    }
}

/// A message stored in a fixed buffer so it can outlive the framer.
#[derive(Clone, PartialEq, Eq)]
pub struct RawMessage<const N: usize> {
    data: Vec<u8, N>,
}

impl<const N: usize> RawMessage<N> {
    /// Copies `bytes` into a new message.
    pub fn from_slice(bytes: &[u8]) -> Result<Self, Error> {
        let mut data = Vec::new();
        data.extend_from_slice(bytes)
            .map_err(|_| Error::BufferFull)?;
        Ok(Self { data })
    }

    /// Borrows the message as UTF-8.
    pub fn as_str(&self) -> Result<&str, Error> {
        str::from_utf8(&self.data).map_err(|_| Error::InvalidUtf8)
    }

    /// Parses the message as a server response.
    pub fn response(&self) -> Result<Response<'_>, Error> {
        Response::parse(self.as_str()?)
    }

    /// The raw bytes, including the terminating newline.
    pub fn as_bytes(&self) -> &[u8] {
        &self.data
    }
}

impl<const N: usize> fmt::Debug for RawMessage<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.as_str() {
            Ok(text) => write!(f, "RawMessage({text:?})"),
            Err(_) => write!(f, "RawMessage({:?})", self.data.as_slice()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_ok() {
        let response = Response::parse("200 ok\r\n").unwrap();
        assert_eq!(response.code, ResponseCode::OK);
        assert_eq!(response.text, "ok");
        assert!(response.body.is_empty());
    }

    #[test]
    fn transport_block() {
        let raw = "\
208 transport info:\r\n\
status: play\r\n\
speed: 100\r\n\
loop: true\r\n\
\r\n";
        let response = Response::parse(raw).unwrap();
        assert_eq!(response.code, ResponseCode::TRANSPORT_INFO);
        assert_eq!(response.get("status"), Some("play"));
        assert_eq!(response.get("speed"), Some("100"));
        assert_eq!(response.get("loop"), Some("true"));
    }
}
