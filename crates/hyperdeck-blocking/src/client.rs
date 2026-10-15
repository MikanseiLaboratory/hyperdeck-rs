//! Synchronous clients.
//!
//! [`IoClient`] owns the socket on the calling thread. [`BlockingClient`] moves
//! the read half onto a background thread so a Tauri command can send one
//! command at a time while asynchronous notices queue up.

use core::fmt::Write as _;

use embedded_io::{ErrorKind, Read, Write};
use heapless::{String, Vec};
use hyperdeck_core::ethernet::{ByteBuf, RawMessage, encode_command};
use hyperdeck_core::session::{ClientEvent, ClientSession};
use hyperdeck_core::types::CodeClass;

use crate::error::Error;

const CMD_CAP: usize = 512;

/// Single-threaded client over a blocking [`embedded_io`] socket.
pub struct IoClient<IO, const N: usize> {
    io: IO,
    session: ClientSession<N>,
    read_buf: [u8; 256],
    model: String<48>,
    protocol_version: String<16>,
}

impl<IO, const N: usize> IoClient<IO, N>
where
    IO: Read + Write,
{
    /// Reads the `500 connection info` preamble.
    ///
    /// A socket timeout surfaces as [`Error::Timeout`]. Arm a timeout on `io`
    /// before calling this if the peer might never speak.
    pub fn connect(io: IO) -> Result<Self, Error<IO::Error>> {
        let mut client = Self {
            io,
            session: ClientSession::new(),
            read_buf: [0; 256],
            model: String::new(),
            protocol_version: String::new(),
        };
        loop {
            let n = client.read_some()?;
            client.session.push(&client.read_buf[..n])?;
            while let Some(event) = client.session.poll::<N>()? {
                match event {
                    ClientEvent::ConnectionInfo(message) => {
                        client.remember_identity(&message)?;
                        return Ok(client);
                    }
                    ClientEvent::Notification(_) | ClientEvent::Response(_) => {}
                }
            }
        }
    }

    /// Model name from the preamble.
    pub fn model(&self) -> &str {
        &self.model
    }

    /// Protocol version from the preamble.
    pub fn protocol_version(&self) -> &str {
        &self.protocol_version
    }

    /// Sends `play`.
    pub fn play(&mut self) -> Result<(), Error<IO::Error>> {
        self.command("play", &[]).map(|_| ())
    }

    /// Sends `stop`.
    pub fn stop(&mut self) -> Result<(), Error<IO::Error>> {
        self.command("stop", &[]).map(|_| ())
    }

    /// Sends `record`, optionally naming the clip.
    pub fn record(&mut self, name: Option<&str>) -> Result<(), Error<IO::Error>> {
        match name {
            Some(name) => self.command("record", &[("name", name)]).map(|_| ()),
            None => self.command("record", &[]).map(|_| ()),
        }
    }

    /// Sends `ping`.
    pub fn ping(&mut self) -> Result<(), Error<IO::Error>> {
        self.command("ping", &[]).map(|_| ())
    }

    /// Sends a command that is already formatted, without a trailing newline.
    pub fn raw(&mut self, line: &str) -> Result<RawMessage<N>, Error<IO::Error>> {
        let mut buf = Vec::<u8, CMD_CAP>::new();
        let mut writer = ByteBuf::new(&mut buf);
        writer
            .write_str(line)
            .map_err(|_| Error::<IO::Error>::BufferFull)?;
        writer
            .write_str("\r\n")
            .map_err(|_| Error::<IO::Error>::BufferFull)?;
        self.roundtrip(buf.as_slice())
    }

    fn command(
        &mut self,
        name: &str,
        params: &[(&str, &str)],
    ) -> Result<RawMessage<N>, Error<IO::Error>> {
        let mut buf = Vec::<u8, CMD_CAP>::new();
        encode_command(&mut ByteBuf::new(&mut buf), name, params).map_err(|_| Error::BufferFull)?;
        self.roundtrip(buf.as_slice())
    }

    fn roundtrip(&mut self, bytes: &[u8]) -> Result<RawMessage<N>, Error<IO::Error>> {
        self.session.begin_command()?;
        if let Err(err) = self.io.write_all(bytes) {
            self.session.abandon();
            return Err(Error::Io(err));
        }
        loop {
            while let Some(event) = self.session.poll::<N>()? {
                match event {
                    ClientEvent::Response(message) => return accept_response(message),
                    ClientEvent::Notification(_) | ClientEvent::ConnectionInfo(_) => {}
                }
            }
            let n = match self.read_some() {
                Err(Error::Timeout) => {
                    self.session.abandon();
                    return Err(Error::Timeout);
                }
                other => other?,
            };
            self.session.push(&self.read_buf[..n])?;
        }
    }

    fn read_some(&mut self) -> Result<usize, Error<IO::Error>> {
        match self.io.read(&mut self.read_buf) {
            Ok(0) => Err(Error::Closed),
            Ok(n) => Ok(n),
            Err(err) if is_timeout(&err) => Err(Error::Timeout),
            Err(err) => Err(Error::Io(err)),
        }
    }

    fn remember_identity(&mut self, message: &RawMessage<N>) -> Result<(), Error<IO::Error>> {
        let response = message.response()?;
        if let Some(model) = response.get("model") {
            self.model.clear();
            push_trunc(&mut self.model, model);
        }
        if let Some(version) = response.get("protocol version") {
            self.protocol_version.clear();
            push_trunc(&mut self.protocol_version, version);
        }
        Ok(())
    }
}

fn is_timeout<E: embedded_io::Error>(err: &E) -> bool {
    matches!(err.kind(), ErrorKind::TimedOut)
}

fn accept_response<E, const N: usize>(message: RawMessage<N>) -> Result<RawMessage<N>, Error<E>> {
    let response = message.response()?;
    if response.code.class() == CodeClass::Failure {
        let mut text = String::new();
        push_trunc(&mut text, response.text);
        return Err(Error::Device {
            code: response.code.as_u16(),
            text,
        });
    }
    Ok(message)
}

fn push_trunc<const N: usize>(out: &mut String<N>, text: &str) {
    for ch in text.chars() {
        if out.push(ch).is_err() {
            break;
        }
    }
}

#[cfg(feature = "std")]
mod threaded {
    use std::collections::VecDeque;
    use std::io::{Read, Write};
    use std::net::{Shutdown, SocketAddr, TcpStream, ToSocketAddrs};
    use std::sync::{Arc, Condvar, Mutex};
    use std::time::{Duration, Instant};

    use core::fmt::Write as _;
    use heapless::Vec;
    use hyperdeck_core::ethernet::{ByteBuf, encode_command};
    use hyperdeck_core::session::{ClientEvent, ClientSession};
    use hyperdeck_core::types::CodeClass;

    use super::push_trunc;
    use crate::error::Error;

    const N: usize = 2048;

    type CommandResult = Result<String, Error<std::io::Error>>;

    struct Flight {
        epoch: u64,
        open: bool,
    }

    struct Inner {
        write: Mutex<TcpStream>,
        flight: Mutex<Flight>,
        response: Mutex<Option<(u64, CommandResult)>>,
        cv: Condvar,
        notes: Mutex<VecDeque<String>>,
        notes_cv: Condvar,
        alive: Mutex<bool>,
        model: String,
        protocol_version: String,
    }

    impl Drop for Inner {
        fn drop(&mut self) {
            if let Ok(stream) = self.write.lock() {
                let _ = stream.shutdown(Shutdown::Both);
            }
        }
    }

    /// Cloneable handle for one TCP connection.
    ///
    /// Clones share the socket. Commands are serialised, which matches the
    /// protocol rule of one in-flight command. Notifications that arrive
    /// between commands are queued and read with [`BlockingClient::try_notification`].
    #[derive(Clone)]
    pub struct BlockingClient {
        inner: Arc<Inner>,
        timeout: Duration,
    }

    impl BlockingClient {
        /// Connects and reads the preamble, timing out after three seconds.
        pub fn connect(addr: impl ToSocketAddrs) -> Result<Self, Error<std::io::Error>> {
            Self::connect_timeout(addr, Duration::from_secs(3))
        }

        /// Connects and reads the preamble within `timeout`.
        pub fn connect_timeout(
            addr: impl ToSocketAddrs,
            timeout: Duration,
        ) -> Result<Self, Error<std::io::Error>> {
            let mut last = None;
            for addr in addr.to_socket_addrs().map_err(Error::Io)? {
                match connect_one(addr, timeout) {
                    Ok(client) => return Ok(client),
                    Err(err) => last = Some(err),
                }
            }
            Err(last.unwrap_or(Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "the address list was empty",
            ))))
        }

        /// Model name from the preamble.
        pub fn model(&self) -> &str {
            &self.inner.model
        }

        /// Protocol version from the preamble.
        pub fn protocol_version(&self) -> &str {
            &self.inner.protocol_version
        }

        /// Replaces the per-command deadline. The default is three seconds.
        pub fn set_timeout(&mut self, timeout: Duration) {
            self.timeout = timeout;
        }

        /// Sends `play`.
        pub fn play(&self) -> Result<(), Error<std::io::Error>> {
            self.command("play", &[]).map(|_| ())
        }

        /// Sends `play` with an optional speed, loop flag, and single-clip flag.
        pub fn play_with(
            &self,
            speed: Option<i32>,
            loop_playback: Option<bool>,
            single_clip: Option<bool>,
        ) -> Result<(), Error<std::io::Error>> {
            let speed_buf = speed.map(|value| value.to_string());
            let mut params = Vec::<(&str, &str), 4>::new();
            if let Some(text) = speed_buf.as_deref() {
                params
                    .push(("speed", text))
                    .map_err(|_| Error::BufferFull)?;
            }
            if let Some(text) = format_bool(loop_playback) {
                params.push(("loop", text)).map_err(|_| Error::BufferFull)?;
            }
            if let Some(text) = format_bool(single_clip) {
                params
                    .push(("single clip", text))
                    .map_err(|_| Error::BufferFull)?;
            }
            self.command("play", params.as_slice()).map(|_| ())
        }

        /// Sends `stop`.
        pub fn stop(&self) -> Result<(), Error<std::io::Error>> {
            self.command("stop", &[]).map(|_| ())
        }

        /// Sends `record`, optionally naming the clip.
        pub fn record(&self, name: Option<&str>) -> Result<(), Error<std::io::Error>> {
            match name {
                Some(name) => self.command("record", &[("name", name)]).map(|_| ()),
                None => self.command("record", &[]).map(|_| ()),
            }
        }

        /// Sends `goto` with one parameter, for example `("clip id", "2")`.
        pub fn goto(&self, key: &str, value: &str) -> Result<(), Error<std::io::Error>> {
            self.command("goto", &[(key, value)]).map(|_| ())
        }

        /// Sends `transport info` and parses the numeric fields we use most.
        pub fn transport_info(&self) -> Result<TransportSnapshot, Error<std::io::Error>> {
            let message = self.command("transport info", &[])?;
            TransportSnapshot::parse(&message)
        }

        /// Enables or disables one notification by its protocol name.
        pub fn set_notify(&self, name: &str, enabled: bool) -> Result<(), Error<std::io::Error>> {
            let value = if enabled { "true" } else { "false" };
            self.command("notify", &[(name, value)]).map(|_| ())
        }

        /// Sends `ping`.
        pub fn ping(&self) -> Result<(), Error<std::io::Error>> {
            self.command("ping", &[]).map(|_| ())
        }

        /// Sends `quit`. The background reader exits after the peer closes.
        pub fn quit(&self) -> Result<(), Error<std::io::Error>> {
            self.command("quit", &[]).map(|_| ())
        }

        /// Sends a command that is already formatted, without a trailing newline.
        pub fn raw(&self, line: &str) -> Result<String, Error<std::io::Error>> {
            let mut buf = heapless::Vec::<u8, 512>::new();
            let mut writer = ByteBuf::new(&mut buf);
            writer
                .write_str(line)
                .map_err(|_| Error::<std::io::Error>::BufferFull)?;
            writer
                .write_str("\r\n")
                .map_err(|_| Error::<std::io::Error>::BufferFull)?;
            self.roundtrip(buf.as_slice())
        }

        /// Pops one queued notice, if the reader has stored one.
        pub fn try_notification(&self) -> Option<String> {
            self.inner
                .notes
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .pop_front()
        }

        /// Waits up to `timeout` for a notice.
        pub fn recv_notification_timeout(&self, timeout: Duration) -> Option<String> {
            let deadline = Instant::now() + timeout;
            let mut notes = self
                .inner
                .notes
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            loop {
                if let Some(message) = notes.pop_front() {
                    return Some(message);
                }
                let now = Instant::now();
                if now >= deadline {
                    return None;
                }
                let (guard, _) = self
                    .inner
                    .notes_cv
                    .wait_timeout(notes, deadline - now)
                    .unwrap_or_else(|poison| poison.into_inner());
                notes = guard;
            }
        }

        fn command(
            &self,
            name: &str,
            params: &[(&str, &str)],
        ) -> Result<String, Error<std::io::Error>> {
            let mut buf = heapless::Vec::<u8, 512>::new();
            encode_command(&mut ByteBuf::new(&mut buf), name, params)
                .map_err(|_| Error::BufferFull)?;
            self.roundtrip(buf.as_slice())
        }

        fn roundtrip(&self, bytes: &[u8]) -> Result<String, Error<std::io::Error>> {
            let epoch = {
                let mut flight = self
                    .inner
                    .flight
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner());
                flight.epoch = flight.epoch.wrapping_add(1);
                flight.open = true;
                *self
                    .inner
                    .response
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner()) = None;
                flight.epoch
            };
            self.inner
                .write
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .write_all(bytes)
                .map_err(Error::Io)?;

            let deadline = Instant::now() + self.timeout;
            let mut flight = self
                .inner
                .flight
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            loop {
                if let Some(result) = take_response(&self.inner, epoch) {
                    return result;
                }
                if !*self
                    .inner
                    .alive
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner())
                {
                    flight.open = false;
                    return Err(Error::Closed);
                }
                let now = Instant::now();
                if now >= deadline {
                    if flight.epoch == epoch {
                        flight.open = false;
                    }
                    if let Some(result) = take_response(&self.inner, epoch) {
                        return result;
                    }
                    return Err(Error::Timeout);
                }
                let (guard, _) = self
                    .inner
                    .cv
                    .wait_timeout(flight, deadline - now)
                    .unwrap_or_else(|poison| poison.into_inner());
                flight = guard;
            }
        }
    }

    fn take_response(inner: &Inner, epoch: u64) -> Option<Result<String, Error<std::io::Error>>> {
        let mut slot = inner
            .response
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        match slot.as_ref() {
            Some((got, _)) if *got == epoch => slot.take().map(|(_, result)| result),
            _ => None,
        }
    }

    fn connect_one(
        addr: SocketAddr,
        timeout: Duration,
    ) -> Result<BlockingClient, Error<std::io::Error>> {
        let mut stream = TcpStream::connect_timeout(&addr, timeout).map_err(Error::Io)?;
        stream.set_nodelay(true).map_err(Error::Io)?;
        stream
            .set_read_timeout(Some(Duration::from_millis(200)))
            .map_err(Error::Io)?;
        let mut session = ClientSession::<N>::new();
        let mut buf = [0u8; 512];
        let deadline = Instant::now() + timeout;
        loop {
            if Instant::now() >= deadline {
                return Err(Error::Timeout);
            }
            match stream.read(&mut buf) {
                Ok(0) => return Err(Error::Closed),
                Ok(n) => {
                    session.push(&buf[..n])?;
                    while let Some(event) = session.poll::<N>()? {
                        if let ClientEvent::ConnectionInfo(message) = event {
                            let response = message.response()?;
                            let model = response.get("model").unwrap_or("").to_string();
                            let protocol_version =
                                response.get("protocol version").unwrap_or("").to_string();
                            return spawn_reader(stream, session, model, protocol_version, timeout);
                        }
                    }
                }
                Err(err)
                    if err.kind() == std::io::ErrorKind::TimedOut
                        || err.kind() == std::io::ErrorKind::WouldBlock
                        || err.kind() == std::io::ErrorKind::Interrupted => {}
                Err(err) => return Err(Error::Io(err)),
            }
        }
    }

    fn spawn_reader(
        stream: TcpStream,
        session: ClientSession<N>,
        model: String,
        protocol_version: String,
        timeout: Duration,
    ) -> Result<BlockingClient, Error<std::io::Error>> {
        let reader = stream.try_clone().map_err(Error::Io)?;
        let inner = Arc::new(Inner {
            write: Mutex::new(stream),
            flight: Mutex::new(Flight {
                epoch: 0,
                open: false,
            }),
            response: Mutex::new(None),
            cv: Condvar::new(),
            notes: Mutex::new(VecDeque::new()),
            notes_cv: Condvar::new(),
            alive: Mutex::new(true),
            model,
            protocol_version,
        });
        let thread_inner = Arc::clone(&inner);
        std::thread::Builder::new()
            .name("hyperdeck-rx".into())
            .spawn(move || read_loop(reader, session, thread_inner))
            .map_err(Error::Io)?;
        Ok(BlockingClient { inner, timeout })
    }

    fn read_loop(mut reader: TcpStream, mut session: ClientSession<N>, inner: Arc<Inner>) {
        let mut buf = [0u8; 512];
        loop {
            if !drain_session(&mut session, &inner) {
                break;
            }
            match reader.read(&mut buf) {
                Ok(0) => {
                    fail(&inner, Error::Closed);
                    break;
                }
                Ok(n) => {
                    if session.push(&buf[..n]).is_err() {
                        fail(&inner, Error::BufferFull);
                        break;
                    }
                }
                Err(err)
                    if err.kind() == std::io::ErrorKind::TimedOut
                        || err.kind() == std::io::ErrorKind::WouldBlock
                        || err.kind() == std::io::ErrorKind::Interrupted => {}
                Err(err) => {
                    fail(&inner, Error::Io(err));
                    break;
                }
            }
        }
    }

    fn drain_session(session: &mut ClientSession<N>, inner: &Inner) -> bool {
        loop {
            let event = match session.poll::<N>() {
                Ok(Some(event)) => event,
                Ok(None) => return true,
                Err(_) => {
                    fail(inner, Error::Protocol(hyperdeck_core::Error::Syntax));
                    return false;
                }
            };
            match event {
                ClientEvent::Response(message) => {
                    deliver_response(inner, message_to_result(&message));
                }
                ClientEvent::Notification(message) | ClientEvent::ConnectionInfo(message) => {
                    if let Ok(text) = message.as_str() {
                        push_note(inner, text.to_string());
                    }
                }
            }
        }
    }

    fn message_to_result(
        message: &hyperdeck_core::RawMessage<N>,
    ) -> Result<String, Error<std::io::Error>> {
        let response = message.response()?;
        let text = message
            .as_str()
            .map_err(Error::<std::io::Error>::from)?
            .to_string();
        if response.code.class() == CodeClass::Failure {
            let mut reason = heapless::String::new();
            push_trunc(&mut reason, response.text);
            Err(Error::Device {
                code: response.code.as_u16(),
                text: reason,
            })
        } else {
            Ok(text)
        }
    }

    fn deliver_response(inner: &Inner, result: Result<String, Error<std::io::Error>>) {
        let mut flight = inner
            .flight
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if !flight.open {
            return;
        }
        let epoch = flight.epoch;
        flight.open = false;
        *inner
            .response
            .lock()
            .unwrap_or_else(|poison| poison.into_inner()) = Some((epoch, result));
        inner.cv.notify_one();
    }

    fn push_note(inner: &Inner, text: String) {
        let mut notes = inner
            .notes
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if notes.len() == 32 {
            notes.pop_front();
        }
        notes.push_back(text);
        inner.notes_cv.notify_one();
    }

    fn fail(inner: &Inner, err: Error<std::io::Error>) {
        *inner
            .alive
            .lock()
            .unwrap_or_else(|poison| poison.into_inner()) = false;
        deliver_response(inner, Err(err));
        inner.cv.notify_one();
        inner.notes_cv.notify_one();
    }

    fn format_bool(value: Option<bool>) -> Option<&'static str> {
        value.map(|value| if value { "true" } else { "false" })
    }

    /// Fields from `208 transport info`.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct TransportSnapshot {
        /// `status` token.
        pub status: String,
        /// Playback speed in percent.
        pub speed: i32,
        /// Timeline position in frames.
        pub timeline: u32,
        /// `loop`.
        pub loop_playback: bool,
        /// `single clip`.
        pub single_clip: bool,
        /// `clip id`, or `none`.
        pub clip_id: String,
        /// `timecode`.
        pub timecode: String,
        /// Ethernet video-format token.
        pub video_format: String,
    }

    impl TransportSnapshot {
        fn parse(message: &str) -> Result<Self, Error<std::io::Error>> {
            let response = hyperdeck_core::Response::parse(message)?;
            let number = |key: &str| -> Result<i32, Error<std::io::Error>> {
                response
                    .get(key)
                    .unwrap_or("0")
                    .parse()
                    .map_err(|_| Error::Protocol(hyperdeck_core::Error::InvalidValue))
            };
            Ok(Self {
                status: response.get("status").unwrap_or("").to_string(),
                speed: number("speed")?,
                timeline: number("timeline")? as u32,
                loop_playback: response.get("loop") == Some("true"),
                single_clip: response.get("single clip") == Some("true"),
                clip_id: response.get("clip id").unwrap_or("none").to_string(),
                timecode: response.get("timecode").unwrap_or("").to_string(),
                video_format: response.get("video format").unwrap_or("").to_string(),
            })
        }
    }
}

#[cfg(feature = "std")]
pub use threaded::{BlockingClient, TransportSnapshot};

#[cfg(test)]
mod tests {
    use super::*;
    use embedded_io::ErrorType;

    struct Script {
        incoming: &'static [u8],
        pos: usize,
        written: Vec<u8, 128>,
    }

    impl ErrorType for Script {
        type Error = ErrorKind;
    }

    impl Read for Script {
        fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
            if self.pos >= self.incoming.len() {
                return Ok(0);
            }
            let n = buf.len().min(self.incoming.len() - self.pos);
            buf[..n].copy_from_slice(&self.incoming[self.pos..self.pos + n]);
            self.pos += n;
            Ok(n)
        }
    }

    impl Write for Script {
        fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
            self.written
                .extend_from_slice(buf)
                .map_err(|_| ErrorKind::OutOfMemory)?;
            Ok(buf.len())
        }

        fn flush(&mut self) -> Result<(), Self::Error> {
            Ok(())
        }
    }

    #[test]
    fn scripted_play_roundtrip() {
        let incoming = b"500 connection info:\r\nprotocol version: 1.13\r\nmodel: Script Deck\r\n\r\n200 ok\r\n";
        let mut client = IoClient::<_, 256>::connect(Script {
            incoming,
            pos: 0,
            written: Vec::new(),
        })
        .unwrap();
        assert_eq!(client.model(), "Script Deck");
        assert_eq!(client.protocol_version(), "1.13");
        client.play().unwrap();
        assert_eq!(client.io.written.as_slice(), b"play\r\n");
    }
}
