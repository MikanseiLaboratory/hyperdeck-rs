//! Async client over [`embedded_io_async`].

use core::fmt::Write;

use embassy_futures::select::{Either, select};
use embassy_sync::blocking_mutex::raw::RawMutex;
use embassy_sync::channel::Channel;
use embedded_io_async::{Read, Write as IoWrite};
use heapless::{String, Vec};
use hyperdeck_core::ethernet::{ByteBuf, RawMessage, Response, encode_command};
use hyperdeck_core::session::{ClientEvent, ClientSession};
use hyperdeck_core::types::CodeClass;

use crate::error::Error;
use crate::time::Clock;

const CMD_CAP: usize = 512;

/// Single-task client. One future owns the socket, the session, and the notice queue.
pub struct HyperDeckClient<IO, C, const N: usize> {
    io: IO,
    clock: C,
    session: ClientSession<N>,
    read_buf: [u8; 256],
    timeout_ms: u64,
    notifications: heapless::Deque<RawMessage<N>, 4>,
    model: String<48>,
    protocol_version: String<16>,
}

impl<IO, C, const N: usize> HyperDeckClient<IO, C, N>
where
    IO: Read + IoWrite,
    C: Clock,
{
    /// Reads the `500 connection info` preamble and returns a connected client.
    pub async fn connect(io: IO, clock: C, timeout_ms: u64) -> Result<Self, Error<IO::Error>> {
        let mut client = Self {
            io,
            clock,
            session: ClientSession::new(),
            read_buf: [0; 256],
            timeout_ms,
            notifications: heapless::Deque::new(),
            model: String::new(),
            protocol_version: String::new(),
        };
        loop {
            let n = client.read_some().await?;
            client.session.push(&client.read_buf[..n])?;
            while let Some(event) = client.session.poll::<N>()? {
                match event {
                    ClientEvent::ConnectionInfo(message) => {
                        client.remember_identity(&message)?;
                        return Ok(client);
                    }
                    ClientEvent::Notification(message) | ClientEvent::Response(message) => {
                        client.push_notice(message);
                    }
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

    /// Pops a notice that arrived earlier, if the queue is non-empty.
    pub fn pop_notification(&mut self) -> Option<RawMessage<N>> {
        self.notifications.pop_front()
    }

    /// Sends `play`.
    pub async fn play(&mut self) -> Result<(), Error<IO::Error>> {
        self.command("play", &[]).await.map(|_| ())
    }

    /// Sends `play` with an optional speed, loop flag, and single-clip flag.
    pub async fn play_with(
        &mut self,
        speed: Option<i32>,
        loop_playback: Option<bool>,
        single_clip: Option<bool>,
    ) -> Result<(), Error<IO::Error>> {
        let speed_buf = format_i32(speed);
        let mut params = Vec::<(&str, &str), 4>::new();
        if let Some(text) = speed_buf.as_ref() {
            let _ = params.push(("speed", text.as_str()));
        }
        if let Some(text) = format_bool(loop_playback) {
            let _ = params.push(("loop", text));
        }
        if let Some(text) = format_bool(single_clip) {
            let _ = params.push(("single clip", text));
        }
        self.command("play", params.as_slice()).await.map(|_| ())
    }

    /// Sends `stop`.
    pub async fn stop(&mut self) -> Result<(), Error<IO::Error>> {
        self.command("stop", &[]).await.map(|_| ())
    }

    /// Sends `record`, optionally naming the clip.
    pub async fn record(&mut self, name: Option<&str>) -> Result<(), Error<IO::Error>> {
        match name {
            Some(name) => self.command("record", &[("name", name)]).await.map(|_| ()),
            None => self.command("record", &[]).await.map(|_| ()),
        }
    }

    /// Sends `goto` with one parameter, for example `("clip id", "2")`.
    pub async fn goto(&mut self, key: &str, value: &str) -> Result<(), Error<IO::Error>> {
        self.command("goto", &[(key, value)]).await.map(|_| ())
    }

    /// Sends `transport info` and parses the numeric fields we use most.
    pub async fn transport_info(&mut self) -> Result<TransportSnapshot, Error<IO::Error>> {
        let message = self.command("transport info", &[]).await?;
        TransportSnapshot::from_response(&message.response()?)
    }

    /// Enables or disables one notification by its protocol name.
    pub async fn set_notify(&mut self, name: &str, enabled: bool) -> Result<(), Error<IO::Error>> {
        let value = if enabled { "true" } else { "false" };
        self.command("notify", &[(name, value)]).await.map(|_| ())
    }

    /// Sends `ping`.
    pub async fn ping(&mut self) -> Result<(), Error<IO::Error>> {
        self.command("ping", &[]).await.map(|_| ())
    }

    /// Sends `quit`. The socket is still open when this returns.
    pub async fn quit(&mut self) -> Result<(), Error<IO::Error>> {
        self.command("quit", &[]).await.map(|_| ())
    }

    /// Sends a command that is already formatted, without a trailing newline.
    pub async fn raw(&mut self, line: &str) -> Result<RawMessage<N>, Error<IO::Error>> {
        let mut buf = Vec::<u8, CMD_CAP>::new();
        let mut writer = ByteBuf::new(&mut buf);
        writer
            .write_str(line)
            .map_err(|_| Error::<IO::Error>::BufferFull)?;
        writer
            .write_str("\r\n")
            .map_err(|_| Error::<IO::Error>::BufferFull)?;
        self.roundtrip(buf.as_slice()).await
    }

    async fn command(
        &mut self,
        name: &str,
        params: &[(&str, &str)],
    ) -> Result<RawMessage<N>, Error<IO::Error>> {
        let mut buf = Vec::<u8, CMD_CAP>::new();
        encode_command(&mut ByteBuf::new(&mut buf), name, params).map_err(|_| Error::BufferFull)?;
        self.roundtrip(buf.as_slice()).await
    }

    async fn roundtrip(&mut self, bytes: &[u8]) -> Result<RawMessage<N>, Error<IO::Error>> {
        self.session.begin_command()?;
        if let Err(err) = self.io.write_all(bytes).await {
            self.session.abandon();
            return Err(Error::Io(err));
        }
        loop {
            while let Some(event) = self.session.poll::<N>()? {
                match event {
                    ClientEvent::Response(message) => return accept_response(message),
                    ClientEvent::Notification(message) | ClientEvent::ConnectionInfo(message) => {
                        self.push_notice(message);
                    }
                }
            }
            let read = self.read_some().await;
            let n = match read {
                Err(Error::Timeout) => {
                    self.session.abandon();
                    return Err(Error::Timeout);
                }
                other => other?,
            };
            self.session.push(&self.read_buf[..n])?;
        }
    }

    async fn read_some(&mut self) -> Result<usize, Error<IO::Error>> {
        let read = self.io.read(&mut self.read_buf);
        let sleep = self.clock.sleep(self.timeout_ms);
        match select(read, sleep).await {
            Either::First(Ok(0)) => Err(Error::Closed),
            Either::First(Ok(n)) => Ok(n),
            Either::First(Err(err)) => Err(Error::Io(err)),
            Either::Second(()) => Err(Error::Timeout),
        }
    }

    fn push_notice(&mut self, message: RawMessage<N>) {
        if let Err(message) = self.notifications.push_back(message) {
            let _ = self.notifications.pop_front();
            let _ = self.notifications.push_back(message);
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

/// Fields from `208 transport info` that the high-level API returns by value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportSnapshot {
    /// `status` token.
    pub status: String<16>,
    /// Playback speed in percent.
    pub speed: i32,
    /// Timeline position in frames.
    pub timeline: u32,
    /// `loop`.
    pub loop_playback: bool,
    /// `single clip`.
    pub single_clip: bool,
    /// `clip id`, or `none`.
    pub clip_id: String<8>,
    /// `timecode`.
    pub timecode: String<16>,
    /// Ethernet video-format token.
    pub video_format: String<24>,
}

impl TransportSnapshot {
    fn from_response<E>(response: &Response<'_>) -> Result<Self, Error<E>> {
        Ok(Self {
            status: copy_field(response.get("status").unwrap_or("")),
            speed: response
                .get("speed")
                .unwrap_or("0")
                .parse()
                .map_err(|_| Error::Protocol(hyperdeck_core::Error::InvalidValue))?,
            timeline: response
                .get("timeline")
                .unwrap_or("0")
                .parse()
                .map_err(|_| Error::Protocol(hyperdeck_core::Error::InvalidValue))?,
            loop_playback: response.get("loop") == Some("true"),
            single_clip: response.get("single clip") == Some("true"),
            clip_id: copy_field(response.get("clip id").unwrap_or("none")),
            timecode: copy_field(response.get("timecode").unwrap_or("")),
            video_format: copy_field(response.get("video format").unwrap_or("")),
        })
    }
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

fn copy_field<const N: usize>(text: &str) -> String<N> {
    let mut out = String::new();
    push_trunc(&mut out, text);
    out
}

fn push_trunc<const N: usize>(out: &mut String<N>, text: &str) {
    for ch in text.chars() {
        if out.push(ch).is_err() {
            break;
        }
    }
}

fn format_i32(value: Option<i32>) -> Option<String<8>> {
    value.map(|value| {
        let mut out = String::new();
        let _ = write!(out, "{value}");
        out
    })
}

fn format_bool(value: Option<bool>) -> Option<&'static str> {
    value.map(|value| if value { "true" } else { "false" })
}

/// Queues that let one task own the socket while other tasks send commands.
///
/// This is the shape Embassy firmware wants: a `#[embassy_executor::task]`
/// runs [`drive_client`], and a button task calls [`ClientHub::call`].
pub struct ClientHub<M: RawMutex, const N: usize> {
    requests: Channel<M, Vec<u8, CMD_CAP>, 1>,
    responses: Channel<M, RawMessage<N>, 1>,
    notifications: Channel<M, RawMessage<N>, 4>,
}

impl<M: RawMutex, const N: usize> ClientHub<M, N> {
    /// Empty queues.
    pub const fn new() -> Self {
        Self {
            requests: Channel::new(),
            responses: Channel::new(),
            notifications: Channel::new(),
        }
    }

    /// Sends one already-encoded command, including the trailing newline.
    pub async fn call(&self, bytes: &[u8]) -> Result<RawMessage<N>, Error<()>> {
        let mut buf = Vec::new();
        buf.extend_from_slice(bytes)
            .map_err(|_| Error::BufferFull)?;
        self.requests.send(buf).await;
        Ok(self.responses.receive().await)
    }

    /// Waits for the next asynchronous notice.
    pub async fn recv_notification(&self) -> RawMessage<N> {
        self.notifications.receive().await
    }

    /// Polls for a notice without waiting.
    pub fn try_notification(&self) -> Option<RawMessage<N>> {
        self.notifications.try_receive().ok()
    }
}

impl<M: RawMutex, const N: usize> Default for ClientHub<M, N> {
    fn default() -> Self {
        Self::new()
    }
}

/// Drives `io` until the peer closes it or a protocol error occurs.
///
/// The `500 connection info` preamble is delivered on the notification channel.
pub async fn drive_client<IO, C, M, const N: usize>(
    io: &mut IO,
    clock: &C,
    hub: &ClientHub<M, N>,
    timeout_ms: u64,
) -> Result<(), Error<IO::Error>>
where
    IO: Read + IoWrite,
    C: Clock,
    M: RawMutex,
{
    let mut session = ClientSession::<N>::new();
    let mut read_buf = [0u8; 256];
    loop {
        let incoming = select(io.read(&mut read_buf), hub.requests.receive()).await;
        match incoming {
            Either::First(Ok(0)) => return Err(Error::Closed),
            Either::First(Ok(n)) => {
                session.push(&read_buf[..n])?;
                dispatch_events(&mut session, hub)?;
            }
            Either::First(Err(err)) => return Err(Error::Io(err)),
            Either::Second(bytes) => {
                if session.begin_command().is_err() {
                    // A command is already in flight; this hub only allows one.
                    continue;
                }
                if let Err(err) = io.write_all(bytes.as_slice()).await {
                    session.abandon();
                    return Err(Error::Io(err));
                }
                let _ = timeout_ms;
                let _ = clock;
            }
        }
    }
}

fn dispatch_events<M: RawMutex, const N: usize>(
    session: &mut ClientSession<N>,
    hub: &ClientHub<M, N>,
) -> Result<(), hyperdeck_core::Error> {
    while let Some(event) = session.poll::<N>()? {
        match event {
            ClientEvent::Response(message) => {
                let _ = hub.responses.try_send(message);
            }
            ClientEvent::Notification(message) | ClientEvent::ConnectionInfo(message) => {
                if let Err(embassy_sync::channel::TrySendError::Full(message)) =
                    hub.notifications.try_send(message)
                {
                    let _ = hub.notifications.try_receive();
                    let _ = hub.notifications.try_send(message);
                }
            }
        }
    }
    Ok(())
}
