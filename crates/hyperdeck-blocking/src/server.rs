//! Thread-per-connection HyperDeck server.
//!
//! The deck state is [`hyperdeck_core::DeckSim`]. Each accepted socket is moved
//! onto its own thread, which is the shape a small desktop mock (and a Tauri
//! sidecar) wants. Connection slots are fixed at four.

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use hyperdeck_core::ethernet::{COMMANDS_XML, Command, HELP_TEXT};
use hyperdeck_core::session::{NotifyKind, ServerSession};
use hyperdeck_core::types::ResponseCode;
use hyperdeck_core::{ApplyResult, DeckSim};

use crate::error::Error;

const SLOTS: usize = 4;

/// Static description reported by `device info` and the connection preamble.
#[derive(Debug, Clone, Copy)]
pub struct DeviceIdentity {
    /// `protocol version`.
    pub protocol_version: &'static str,
    /// `model`.
    pub model: &'static str,
    /// `unique id`.
    pub unique_id: &'static str,
    /// `slot count`.
    pub slot_count: u8,
    /// `software version`.
    pub software_version: &'static str,
    /// `name`.
    pub name: &'static str,
}

/// Identity used by the mock deck examples.
pub const DEMO_IDENTITY: DeviceIdentity = DeviceIdentity {
    protocol_version: "1.13",
    model: "HyperDeck Rust",
    unique_id: "hyperdeck-rs",
    slot_count: 2,
    software_version: "0.1.0",
    name: "Mock HyperDeck",
};

/// In-memory deck shared by every connection and by the caller's tick loop.
pub struct SharedDeck {
    sim: Mutex<DeckSim>,
    remote: AtomicBool,
    slots: Mutex<[bool; SLOTS]>,
    notices: [Mutex<VecDeque<Vec<u8>>>; SLOTS],
    identity: DeviceIdentity,
    boot: Instant,
}

impl SharedDeck {
    /// Wraps `sim`. The remote-control switch starts at `sim.remote_enabled()`.
    pub fn new(sim: DeckSim) -> Self {
        let remote = sim.remote_enabled();
        Self {
            sim: Mutex::new(sim),
            remote: AtomicBool::new(remote),
            slots: Mutex::new([false; SLOTS]),
            notices: [const { Mutex::new(VecDeque::new()) }; SLOTS],
            identity: DEMO_IDENTITY,
            boot: Instant::now(),
        }
    }

    /// The demo timeline from [`DeckSim::demo`].
    pub fn demo() -> Self {
        Self::new(DeckSim::demo())
    }

    /// Replaces the text returned by `device info`.
    pub fn with_identity(mut self, identity: DeviceIdentity) -> Self {
        self.identity = identity;
        self
    }

    /// Advances playback and queues a `508 transport info` notice when it changes.
    pub fn tick(&self, frames: u32) {
        let mut sim = self.lock_sim();
        if !sim.tick(frames) {
            return;
        }
        let mut text = String::new();
        if sim.write_transport_notice(&mut text).is_err() {
            return;
        }
        drop(sim);
        self.publish(text.as_bytes());
    }

    /// Deck-wide remote-control switch.
    pub fn remote_enabled(&self) -> bool {
        self.remote.load(Ordering::Relaxed)
    }

    /// Sets the deck-wide remote-control switch.
    pub fn set_remote_enabled(&self, enabled: bool) {
        self.remote.store(enabled, Ordering::Relaxed);
    }

    fn now_ms(&self) -> u64 {
        u64::try_from(self.boot.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    fn lock_sim(&self) -> std::sync::MutexGuard<'_, DeckSim> {
        self.sim.lock().unwrap_or_else(|poison| poison.into_inner())
    }

    fn acquire(&self) -> Option<usize> {
        let mut slots = self
            .slots
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let id = slots.iter().position(|slot| !*slot)?;
        slots[id] = true;
        drop(slots);
        self.notices[id]
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .clear();
        Some(id)
    }

    fn release(&self, id: usize) {
        if let Some(slot) = self
            .slots
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .get_mut(id)
        {
            *slot = false;
        }
    }

    fn publish(&self, bytes: &[u8]) {
        for queue in &self.notices {
            let mut queue = queue.lock().unwrap_or_else(|poison| poison.into_inner());
            if queue.len() == 8 {
                queue.pop_front();
            }
            queue.push_back(bytes.to_vec());
        }
    }

    fn poll_notice(&self, id: usize) -> Option<Vec<u8>> {
        self.notices
            .get(id)?
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .pop_front()
    }

    fn apply(&self, cmd: &Command<'_>, body: &mut String) -> (ApplyResult, String) {
        let mut sim = self.lock_sim();
        let mut notify = String::new();
        let result = sim.apply(cmd, body, &mut notify);
        (result, notify)
    }
}

/// Accepts Ethernet clients on a blocking listener.
pub struct BlockingServer {
    listener: TcpListener,
}

impl BlockingServer {
    /// Binds `addr` and switches the listener to non-blocking accept.
    pub fn bind(addr: impl ToSocketAddrs) -> Result<Self, Error<std::io::Error>> {
        let listener = TcpListener::bind(addr).map_err(Error::Io)?;
        listener.set_nonblocking(true).map_err(Error::Io)?;
        Ok(Self { listener })
    }

    /// The address the listener actually bound.
    pub fn local_addr(&self) -> Result<SocketAddr, Error<std::io::Error>> {
        self.listener.local_addr().map_err(Error::Io)
    }

    /// Accepts until the process ends. Each connection runs on its own thread.
    pub fn serve(self, deck: Arc<SharedDeck>) -> Result<(), Error<std::io::Error>> {
        self.serve_until(deck, &AtomicBool::new(false))
    }

    /// Accepts until `shutdown` is set. Connections already running finish on their own.
    pub fn serve_until(
        self,
        deck: Arc<SharedDeck>,
        shutdown: &AtomicBool,
    ) -> Result<(), Error<std::io::Error>> {
        while !shutdown.load(Ordering::Relaxed) {
            match self.listener.accept() {
                Ok((stream, _)) => {
                    let deck = Arc::clone(&deck);
                    thread::Builder::new()
                        .name("hyperdeck-conn".into())
                        .spawn(move || {
                            let _ = connection(stream, &deck);
                        })
                        .map_err(Error::Io)?;
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(20));
                }
                Err(err) if err.kind() == std::io::ErrorKind::Interrupted => {}
                Err(err) => return Err(Error::Io(err)),
            }
        }
        Ok(())
    }
}

fn connection(mut stream: TcpStream, deck: &SharedDeck) -> Result<(), Error<std::io::Error>> {
    stream.set_nodelay(true).map_err(Error::Io)?;
    stream
        .set_read_timeout(Some(Duration::from_millis(50)))
        .map_err(Error::Io)?;
    let Some(conn_id) = deck.acquire() else {
        write_simple(
            &mut stream,
            ResponseCode::CONNECTION_FAILED.as_u16(),
            "connection failed",
        )?;
        return Err(Error::TooManyConnections);
    };
    let result = connection_loop(&mut stream, deck, conn_id);
    deck.release(conn_id);
    result
}

fn connection_loop(
    stream: &mut TcpStream,
    deck: &SharedDeck,
    conn_id: usize,
) -> Result<(), Error<std::io::Error>> {
    write_preamble(stream, &deck.identity)?;
    let mut session = ServerSession::<1024>::new(deck.now_ms());
    let mut buf = [0u8; 256];
    loop {
        if session.watchdog_expired(deck.now_ms()) {
            return Ok(());
        }
        flush_notices(stream, deck, conn_id, &session)?;
        match stream.read(&mut buf) {
            Ok(0) => return Ok(()),
            Err(err)
                if err.kind() == std::io::ErrorKind::TimedOut
                    || err.kind() == std::io::ErrorKind::WouldBlock
                    || err.kind() == std::io::ErrorKind::Interrupted =>
            {
                continue;
            }
            Err(err) => return Err(Error::Io(err)),
            Ok(n) => {
                session.note_activity(deck.now_ms());
                session.push(&buf[..n])?;
                while let Some(raw) = session.pop_raw::<1024>()? {
                    let text = core::str::from_utf8(raw.as_slice())
                        .map_err(|_| Error::Protocol(hyperdeck_core::Error::InvalidUtf8))?;
                    let cmd = Command::parse(text)?;
                    if handle_builtin(stream, &mut session, deck, &cmd)? {
                        return Ok(());
                    }
                }
            }
        }
    }
}

fn handle_builtin(
    stream: &mut TcpStream,
    session: &mut ServerSession<1024>,
    deck: &SharedDeck,
    cmd: &Command<'_>,
) -> Result<bool, Error<std::io::Error>> {
    match cmd.name {
        "ping" => {
            write_simple(stream, 200, "ok")?;
            Ok(false)
        }
        "quit" => {
            write_simple(stream, 200, "ok")?;
            Ok(true)
        }
        "help" | "?" => {
            write_block(stream, 201, "help", HELP_TEXT)?;
            Ok(false)
        }
        "commands" => {
            write_block(stream, 212, "commands", COMMANDS_XML)?;
            Ok(false)
        }
        "notify" => {
            write_notify(stream, session, cmd)?;
            Ok(false)
        }
        "remote" => {
            write_remote(stream, session, deck, cmd)?;
            Ok(false)
        }
        "watchdog" => {
            let period = cmd.int("period")?.unwrap_or(0);
            session.set_watchdog(period, deck.now_ms());
            write_simple(stream, 200, "ok")?;
            Ok(false)
        }
        "device info" => {
            write_device_info(stream, &deck.identity)?;
            Ok(false)
        }
        "uptime" => {
            let seconds = deck.now_ms() / 1000;
            write_block(stream, 200, "ok", &format!("uptime: {seconds}\r\n"))?;
            Ok(false)
        }
        _ => {
            if is_mutation(cmd) && !deck.remote_enabled() && !session.remote_override() {
                write_simple(
                    stream,
                    ResponseCode::REMOTE_DISABLED.as_u16(),
                    "remote control disabled",
                )?;
                return Ok(false);
            }
            dispatch(stream, deck, cmd)?;
            Ok(false)
        }
    }
}

fn dispatch(
    stream: &mut TcpStream,
    deck: &SharedDeck,
    cmd: &Command<'_>,
) -> Result<(), Error<std::io::Error>> {
    let mut body = String::new();
    let (result, notify) = deck.apply(cmd, &mut body);
    if body.is_empty() {
        write_simple(stream, result.code, result.text.as_str())?;
    } else {
        write_block(stream, result.code, result.text.as_str(), &body)?;
    }
    if result.notify.is_some() && !notify.is_empty() {
        deck.publish(notify.as_bytes());
    }
    Ok(())
}

fn is_mutation(cmd: &Command<'_>) -> bool {
    match cmd.name {
        "transport info" | "slot info" | "disk list" | "clips count" | "clips get"
        | "playrange" => false,
        "configuration" => !cmd.params.is_empty(),
        _ => true,
    }
}

fn write_preamble(
    stream: &mut TcpStream,
    identity: &DeviceIdentity,
) -> Result<(), Error<std::io::Error>> {
    let body = format!(
        "protocol version: {}\r\nmodel: {}\r\n",
        identity.protocol_version, identity.model
    );
    write_block(stream, 500, "connection info", &body)
}

fn write_device_info(
    stream: &mut TcpStream,
    identity: &DeviceIdentity,
) -> Result<(), Error<std::io::Error>> {
    let body = format!(
        "protocol version: {}\r\nmodel: {}\r\nunique id: {}\r\nslot count: {}\r\nsoftware version: {}\r\nname: {}\r\n",
        identity.protocol_version,
        identity.model,
        identity.unique_id,
        identity.slot_count,
        identity.software_version,
        identity.name
    );
    write_block(stream, 204, "device info", &body)
}

fn write_notify(
    stream: &mut TcpStream,
    session: &mut ServerSession<1024>,
    cmd: &Command<'_>,
) -> Result<(), Error<std::io::Error>> {
    if cmd.params.is_empty() {
        let flags = session.notify_flags();
        let mut body = String::new();
        for (name, enabled) in flag_pairs(&flags) {
            let value = if enabled { "true" } else { "false" };
            body.push_str(&format!("{name}: {value}\r\n"));
        }
        return write_block(stream, 209, "notify", &body);
    }
    for (key, value) in cmd.params.iter() {
        let enabled = match *value {
            "true" => true,
            "false" => false,
            _ => {
                write_simple(stream, 102, "invalid value")?;
                return Ok(());
            }
        };
        if !session.set_notify(key, enabled) {
            write_simple(stream, 101, "unsupported parameter")?;
            return Ok(());
        }
    }
    write_simple(stream, 200, "ok")
}

fn flag_pairs(flags: &hyperdeck_core::session::NotifyFlags) -> [(&'static str, bool); 15] {
    [
        ("transport", flags.transport),
        ("slot", flags.slot),
        ("remote", flags.remote),
        ("configuration", flags.configuration),
        ("dropped frames", flags.dropped_frames),
        ("display timecode", flags.display_timecode),
        ("timeline position", flags.timeline_position),
        ("playrange", flags.playrange),
        ("cache", flags.cache),
        ("dynamic range", flags.dynamic_range),
        ("slate", flags.slate),
        ("clips", flags.clips),
        ("disk", flags.disk),
        ("device info", flags.device_info),
        ("nas", flags.nas),
    ]
}

fn write_remote(
    stream: &mut TcpStream,
    session: &mut ServerSession<1024>,
    deck: &SharedDeck,
    cmd: &Command<'_>,
) -> Result<(), Error<std::io::Error>> {
    if let Some(value) = cmd.flag("override")? {
        session.set_remote_override(value);
    }
    if let Some(value) = cmd.flag("enable")? {
        deck.set_remote_enabled(value);
    }
    if cmd.params.is_empty() {
        let enabled = if deck.remote_enabled() {
            "true"
        } else {
            "false"
        };
        let override_flag = if session.remote_override() {
            "true"
        } else {
            "false"
        };
        write_block(
            stream,
            210,
            "remote info",
            &format!("enabled: {enabled}\r\noverride: {override_flag}\r\n"),
        )
    } else {
        write_simple(stream, 200, "ok")
    }
}

fn flush_notices(
    stream: &mut TcpStream,
    deck: &SharedDeck,
    conn_id: usize,
    session: &ServerSession<1024>,
) -> Result<(), Error<std::io::Error>> {
    while let Some(notice) = deck.poll_notice(conn_id) {
        let text = core::str::from_utf8(&notice)
            .map_err(|_| Error::Protocol(hyperdeck_core::Error::InvalidUtf8))?;
        let kind = notice_kind(text);
        if kind.is_none_or(|kind| session.should_notify(kind)) {
            stream.write_all(&notice).map_err(Error::Io)?;
        }
    }
    Ok(())
}

fn notice_kind(text: &str) -> Option<NotifyKind> {
    let code: u16 = text.get(..3)?.parse().ok()?;
    Some(match code {
        502 => NotifyKind::Slot,
        508 => NotifyKind::Transport,
        510 => NotifyKind::Remote,
        511 => NotifyKind::Configuration,
        _ => return None,
    })
}

fn write_simple(
    stream: &mut TcpStream,
    code: u16,
    text: &str,
) -> Result<(), Error<std::io::Error>> {
    write!(stream, "{code} {text}\r\n").map_err(Error::Io)
}

fn write_block(
    stream: &mut TcpStream,
    code: u16,
    text: &str,
    body: &str,
) -> Result<(), Error<std::io::Error>> {
    let mut message = format!("{code} {text}:\r\n{body}");
    if !body.is_empty() && !body.ends_with('\n') {
        message.push_str("\r\n");
    }
    message.push_str("\r\n");
    stream.write_all(message.as_bytes()).map_err(Error::Io)
}
