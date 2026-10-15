//! Writers that do not allocate.

use core::fmt;

use super::WriteError;

/// `core::fmt::Write` adapter over a byte slice.
pub struct SliceWriter<'a> {
    buf: &'a mut [u8],
    pos: usize,
}

impl<'a> SliceWriter<'a> {
    /// Writes into `buf`, starting at the beginning.
    pub fn new(buf: &'a mut [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    /// Bytes written so far.
    pub fn written(&self) -> usize {
        self.pos
    }

    /// The filled prefix.
    pub fn as_slice(&self) -> &[u8] {
        &self.buf[..self.pos]
    }
}

impl fmt::Write for SliceWriter<'_> {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        let bytes = text.as_bytes();
        let end = self.pos.checked_add(bytes.len()).ok_or(fmt::Error)?;
        if end > self.buf.len() {
            return Err(fmt::Error);
        }
        self.buf[self.pos..end].copy_from_slice(bytes);
        self.pos = end;
        Ok(())
    }
}

/// `core::fmt::Write` adapter over a [`heapless::Vec`].
pub struct ByteBuf<'a, const N: usize> {
    buf: &'a mut heapless::Vec<u8, N>,
}

impl<'a, const N: usize> ByteBuf<'a, N> {
    /// Wraps an existing vector.
    pub fn new(buf: &'a mut heapless::Vec<u8, N>) -> Self {
        Self { buf }
    }
}

impl<const N: usize> fmt::Write for ByteBuf<'_, N> {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        self.buf
            .extend_from_slice(text.as_bytes())
            .map_err(|_| fmt::Error)
    }
}

/// Writes `name[: k: v ...]\\r\\n`.
pub fn encode_command(
    out: &mut impl fmt::Write,
    name: &str,
    params: &[(&str, &str)],
) -> Result<(), WriteError> {
    out.write_str(name)?;
    if !params.is_empty() {
        out.write_char(':')?;
        for (key, value) in params {
            write!(out, " {key}: {value}")?;
        }
    }
    out.write_str("\r\n")?;
    Ok(())
}

/// Writes a single-line response such as `200 ok`.
pub fn encode_simple(out: &mut impl fmt::Write, code: u16, text: &str) -> Result<(), WriteError> {
    write!(out, "{code} {text}\r\n")?;
    Ok(())
}

/// Writes a block response. `body` is raw text already split into lines.
pub fn encode_block(
    out: &mut impl fmt::Write,
    code: u16,
    text: &str,
    body: &str,
) -> Result<(), WriteError> {
    write!(out, "{code} {text}:\r\n")?;
    out.write_str(body)?;
    if !body.is_empty() && !body.ends_with('\n') {
        out.write_str("\r\n")?;
    }
    out.write_str("\r\n")?;
    Ok(())
}

/// Writes `key: value` lines from pairs, then a blank line is left to the caller.
pub fn encode_lines(out: &mut impl fmt::Write, lines: &[(&str, &str)]) -> Result<(), WriteError> {
    for (key, value) in lines {
        write!(out, "{key}: {value}\r\n")?;
    }
    Ok(())
}

/// Help text returned by the built-in `help` command.
pub const HELP_TEXT: &str = "\
HyperDeck Ethernet Protocol\r\n\
commands: help, commands, device info, ping, quit, uptime,\r\n\
play, stop, record, goto, jog, shuttle, preview, transport info,\r\n\
playrange, playrange set, playrange clear, slot info, slot select,\r\n\
disk list, clips count, clips get, clips add, clips remove, clips clear,\r\n\
notify, remote, configuration, watchdog, identify\r\n";

/// A subset of the `commands` XML document.
///
/// Real HyperDeck firmware reports the commands that particular model
/// implements. This document lists the commands this crate understands.
pub const COMMANDS_XML: &str = "\
<commands>\r\n\
<command name=\"help\"/>\r\n\
<command name=\"commands\"/>\r\n\
<command name=\"device info\"/>\r\n\
<command name=\"ping\"/>\r\n\
<command name=\"quit\"/>\r\n\
<command name=\"play\"><parameter name=\"speed\"/><parameter name=\"loop\"/><parameter name=\"single clip\"/><parameter name=\"clip id\"/></command>\r\n\
<command name=\"stop\"/>\r\n\
<command name=\"record\"><parameter name=\"name\"/></command>\r\n\
<command name=\"goto\"><parameter name=\"clip id\"/><parameter name=\"clip\"/><parameter name=\"timeline\"/><parameter name=\"timecode\"/><parameter name=\"slot id\"/></command>\r\n\
<command name=\"transport info\"/>\r\n\
<command name=\"slot info\"><parameter name=\"slot id\"/><parameter name=\"device\"/></command>\r\n\
<command name=\"slot select\"><parameter name=\"slot id\"/><parameter name=\"video format\"/></command>\r\n\
<command name=\"disk list\"><parameter name=\"slot id\"/></command>\r\n\
<command name=\"clips get\"><parameter name=\"clip id\"/><parameter name=\"version\"/></command>\r\n\
<command name=\"clips add\"><parameter name=\"name\"/><parameter name=\"clip id\"/></command>\r\n\
<command name=\"notify\"><parameter name=\"transport\"/><parameter name=\"slot\"/><parameter name=\"remote\"/></command>\r\n\
<command name=\"remote\"><parameter name=\"enable\"/><parameter name=\"override\"/></command>\r\n\
<command name=\"configuration\"/>\r\n\
<command name=\"watchdog\"><parameter name=\"period\"/></command>\r\n\
</commands>\r\n";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ethernet::{Command, Response};

    #[test]
    fn command_round_trip() {
        let mut buf = [0u8; 128];
        let mut writer = SliceWriter::new(&mut buf);
        encode_command(
            &mut writer,
            "play",
            &[
                ("clip id", "3"),
                ("speed", "200"),
                ("loop", "true"),
                ("single clip", "true"),
            ],
        )
        .unwrap();
        let text = core::str::from_utf8(writer.as_slice()).unwrap();
        let cmd = Command::parse(text).unwrap();
        assert_eq!(cmd.get("clip id"), Some("3"));
        assert_eq!(cmd.get("speed"), Some("200"));
        assert_eq!(cmd.get("single clip"), Some("true"));
    }

    #[test]
    fn response_round_trip() {
        let mut buf = [0u8; 64];
        let mut writer = SliceWriter::new(&mut buf);
        encode_simple(&mut writer, 200, "ok").unwrap();
        let response = Response::parse(core::str::from_utf8(writer.as_slice()).unwrap()).unwrap();
        assert_eq!(response.code.as_u16(), 200);
    }
}
