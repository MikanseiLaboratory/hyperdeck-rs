//! Borrowed commands.

use heapless::Vec;

use super::params::{parse_inline_params, parse_param_line};
use crate::error::Error;

/// One client command.
///
/// `name` is the text before the first colon (`"play"`, `"playrange set"`,
/// `"device info"`). Parameters keep the order they were received. Values borrow
/// the original message, so a file name with spaces needs no extra buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command<'a> {
    /// Command name, without the trailing colon.
    pub name: &'a str,
    /// Parameter pairs. At most 16, which covers a combined configuration line.
    pub params: Vec<(&'a str, &'a str), 16>,
    /// `true` when the command used the multiline syntax.
    pub multiline: bool,
}

impl<'a> Command<'a> {
    /// Parses one framed message, including its trailing newline.
    pub fn parse(message: &'a str) -> Result<Self, Error> {
        let text = strip_terminator(message);
        let mut lines = text.split('\n');
        let first = lines.next().unwrap_or("").trim_end_matches('\r').trim();
        if first.is_empty() {
            return Err(Error::Syntax);
        }
        let (name, inline) = match first.split_once(':') {
            Some((name, rest)) => (name.trim(), rest),
            None => (first, ""),
        };
        if name.is_empty() {
            return Err(Error::Syntax);
        }
        let mut params = parse_inline_params(inline)?;
        let mut multiline = false;
        for line in lines {
            let line = line.trim_end_matches('\r').trim();
            if line.is_empty() {
                continue;
            }
            multiline = true;
            let Some((key, value)) = parse_param_line(line) else {
                return Err(Error::Syntax);
            };
            params.push((key, value)).map_err(|_| Error::BufferFull)?;
        }
        Ok(Self {
            name,
            params,
            multiline,
        })
    }

    /// Returns the value of `key`, if the command carried it.
    pub fn get(&self, key: &str) -> Option<&'a str> {
        self.params
            .iter()
            .find(|(name, _)| *name == key)
            .map(|(_, value)| *value)
    }

    /// Parses `true` or `false`.
    pub fn flag(&self, key: &str) -> Result<Option<bool>, Error> {
        match self.get(key) {
            None => Ok(None),
            Some("true") => Ok(Some(true)),
            Some("false") => Ok(Some(false)),
            Some(_) => Err(Error::InvalidValue),
        }
    }

    /// Parses a signed integer parameter.
    pub fn int(&self, key: &str) -> Result<Option<i32>, Error> {
        match self.get(key) {
            None => Ok(None),
            Some(value) => value.parse().map(Some).map_err(|_| Error::InvalidValue),
        }
    }
}

fn strip_terminator(message: &str) -> &str {
    let mut text = message.trim_end_matches(['\r', '\n']);
    // A block ends with a blank line. Drop trailing empty lines, keep content.
    while let Some(stripped) = text
        .strip_suffix("\r\n\r\n")
        .or_else(|| text.strip_suffix("\n\n"))
    {
        text = stripped.trim_end_matches(['\r', '\n']);
    }
    // The framer includes the terminating newline of a single-line message and
    // the blank line of a block. `trim_end` already removed the last newline.
    // If the block still ends with an empty line inside `text`, the split loop
    // ignores it.
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playrange_from_the_manual() {
        let cmd = Command::parse("playrange set: clip id: 5 count: 7\r\n").unwrap();
        assert_eq!(cmd.name, "playrange set");
        assert_eq!(cmd.get("clip id"), Some("5"));
        assert_eq!(cmd.get("count"), Some("7"));
        assert!(!cmd.multiline);
    }

    #[test]
    fn combined_configuration() {
        let cmd = Command::parse("configuration: video input: SDI audio input: XLR\n").unwrap();
        assert_eq!(cmd.get("video input"), Some("SDI"));
        assert_eq!(cmd.get("audio input"), Some("XLR"));
    }

    #[test]
    fn multiline_nas_add() {
        let raw = "nas add:\r\nurl: smb://192.168.1.1/Main\r\nusername: user1234\r\n\r\n";
        let cmd = Command::parse(raw).unwrap();
        assert_eq!(cmd.name, "nas add");
        assert!(cmd.multiline);
        assert_eq!(cmd.get("url"), Some("smb://192.168.1.1/Main"));
        assert_eq!(cmd.get("username"), Some("user1234"));
    }
}
