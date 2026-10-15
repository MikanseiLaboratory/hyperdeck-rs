//! Fixed-buffer message framer.

use heapless::Vec;

use crate::error::Error;

/// Accumulates bytes and yields complete protocol messages.
///
/// A message is one line, unless that line ends with `:` — then the message
/// continues until a blank line. LF and CRLF are both accepted. If a message
/// would exceed `N`, the framer discards bytes until the next blank line and
/// reports [`Error::Overflow`] from [`LineFramer::push`].
#[derive(Debug)]
pub struct LineFramer<const N: usize> {
    buf: [u8; N],
    len: usize,
    discarding: bool,
    /// While discarding, `true` once the current line has no payload yet.
    line_empty: bool,
}

impl<const N: usize> LineFramer<N> {
    /// An empty framer.
    pub const fn new() -> Self {
        Self {
            buf: [0; N],
            len: 0,
            discarding: false,
            line_empty: true,
        }
    }

    /// Appends bytes.
    pub fn push(&mut self, data: &[u8]) -> Result<(), Error> {
        if self.discarding {
            return self.consume_discard(data);
        }
        if self.len + data.len() > N {
            self.len = 0;
            self.discarding = true;
            self.line_empty = false;
            let _ = self.consume_discard(data);
            return Err(Error::Overflow);
        }
        self.buf[self.len..self.len + data.len()].copy_from_slice(data);
        self.len += data.len();
        Ok(())
    }

    /// Removes and returns the next complete message, including its terminator.
    pub fn pop<const M: usize>(&mut self) -> Result<Option<Vec<u8, M>>, Error> {
        let Some(end) = find_message_end(&self.buf[..self.len]) else {
            return Ok(None);
        };
        let mut out = Vec::new();
        out.extend_from_slice(&self.buf[..end])
            .map_err(|_| Error::BufferFull)?;
        self.buf.copy_within(end..self.len, 0);
        self.len -= end;
        Ok(Some(out))
    }

    /// Drops any partial input.
    pub fn clear(&mut self) {
        self.len = 0;
        self.discarding = false;
        self.line_empty = true;
    }

    fn consume_discard(&mut self, data: &[u8]) -> Result<(), Error> {
        let mut index = 0;
        while index < data.len() {
            let byte = data[index];
            index += 1;
            if byte == b'\n' {
                if self.line_empty {
                    self.discarding = false;
                    self.line_empty = true;
                    return self.push(&data[index..]);
                }
                self.line_empty = true;
            } else if byte != b'\r' {
                self.line_empty = false;
            }
        }
        Ok(())
    }
}

impl<const N: usize> Default for LineFramer<N> {
    fn default() -> Self {
        Self::new()
    }
}

fn find_message_end(data: &[u8]) -> Option<usize> {
    let mut line_start = 0;
    let mut index = 0;
    let mut first = true;
    let mut in_block = false;
    while index < data.len() {
        if data[index] == b'\n' {
            let mut line_end = index;
            if line_end > line_start && data[line_end - 1] == b'\r' {
                line_end -= 1;
            }
            let line = &data[line_start..line_end];
            if first {
                first = false;
                if line.last() == Some(&b':') {
                    in_block = true;
                } else {
                    return Some(index + 1);
                }
            } else if in_block && line.is_empty() {
                return Some(index + 1);
            }
            line_start = index + 1;
        }
        index += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_line_lf_and_crlf() {
        let mut framer = LineFramer::<64>::new();
        framer.push(b"ping\n").unwrap();
        let msg: Vec<u8, 64> = framer.pop().unwrap().unwrap();
        assert_eq!(msg.as_slice(), b"ping\n");

        framer.push(b"200 ok\r\n").unwrap();
        let msg: Vec<u8, 64> = framer.pop().unwrap().unwrap();
        assert_eq!(msg.as_slice(), b"200 ok\r\n");
    }

    #[test]
    fn block_and_following_message() {
        let mut framer = LineFramer::<128>::new();
        let raw = b"208 transport info:\r\nstatus: play\r\n\r\n200 ok\r\n";
        framer.push(raw).unwrap();
        let first: Vec<u8, 128> = framer.pop().unwrap().unwrap();
        assert!(first.as_slice().starts_with(b"208 transport info:"));
        let second: Vec<u8, 128> = framer.pop().unwrap().unwrap();
        assert_eq!(second.as_slice(), b"200 ok\r\n");
    }

    #[test]
    fn partial_then_remainder() {
        let mut framer = LineFramer::<32>::new();
        framer.push(b"200 ").unwrap();
        let none: Option<Vec<u8, 32>> = framer.pop().unwrap();
        assert!(none.is_none());
        framer.push(b"ok\r\n").unwrap();
        let msg: Vec<u8, 32> = framer.pop().unwrap().unwrap();
        assert_eq!(msg.as_slice(), b"200 ok\r\n");
    }

    #[test]
    fn overflow_resyncs_on_blank_line() {
        let mut framer = LineFramer::<8>::new();
        let err = framer.push(b"0123456789\r\n\r\nping\n").unwrap_err();
        assert_eq!(err, Error::Overflow);
        let msg: Vec<u8, 8> = framer.pop().unwrap().unwrap();
        assert_eq!(msg.as_slice(), b"ping\n");
    }
}
