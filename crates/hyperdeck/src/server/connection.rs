//! One TCP connection.

use core::fmt::Write;

use embassy_futures::select::{Either, select};
use embassy_sync::blocking_mutex::raw::RawMutex;
use embedded_io_async::{Read, Write as IoWrite};
use heapless::{String, Vec};
use hyperdeck_core::ethernet::{
    ByteBuf, COMMANDS_XML, Command, HELP_TEXT, encode_block, encode_simple,
};
use hyperdeck_core::session::{NotifyFlags, NotifyKind, ServerSession};
use hyperdeck_core::types::ResponseCode;

use super::handler::{Outcome, ReplyWriter};
use super::hub::{Hub, Notice};
use crate::error::Error;
use crate::time::Clock;

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

/// Borrowed pieces [`serve_connection`] needs besides the socket.
pub struct ServeArgs<'a, H, C, M: RawMutex, const CONNS: usize> {
    /// Deck behaviour.
    pub handler: &'a H,
    /// Notice fan-out and the remote-enable flag. The slot `conn_id` must already be reserved.
    pub hub: &'a Hub<M, CONNS>,
    /// Slot reserved with [`Hub::acquire`].
    pub conn_id: usize,
    /// Timeouts and the watchdog clock.
    pub clock: &'a C,
    /// Text returned by `device info`.
    pub identity: &'a DeviceIdentity,
    /// [`Clock::now_ms`] value when the deck booted, used by `uptime`.
    pub boot_ms: u64,
}

/// Serves one client until it quits, the watchdog fires, or the socket closes.
///
/// The caller reserves `args.conn_id` first. When every slot is taken, call
/// [`reject_connection`] instead.
pub async fn serve_connection<IO, H, C, M, const CONNS: usize>(
    io: &mut IO,
    args: ServeArgs<'_, H, C, M, CONNS>,
) -> Result<(), Error<IO::Error>>
where
    IO: Read + IoWrite,
    H: super::DeckHandler,
    C: Clock,
    M: RawMutex,
{
    let result = connection_loop(io, &args).await;
    args.hub.release(args.conn_id).await;
    result
}

/// Writes `120 connection failed` and returns. Used when [`Hub::acquire`] fails.
pub async fn reject_connection<IO>(io: &mut IO) -> Result<(), Error<IO::Error>>
where
    IO: IoWrite,
{
    let mut buf = Vec::<u8, 64>::new();
    encode_simple(
        &mut ByteBuf::new(&mut buf),
        ResponseCode::CONNECTION_FAILED.as_u16(),
        "connection failed",
    )
    .map_err(|_| Error::BufferFull)?;
    io.write_all(buf.as_slice()).await.map_err(Error::Io)?;
    Ok(())
}

async fn connection_loop<IO, H, C, M, const CONNS: usize>(
    io: &mut IO,
    args: &ServeArgs<'_, H, C, M, CONNS>,
) -> Result<(), Error<IO::Error>>
where
    IO: Read + IoWrite,
    H: super::DeckHandler,
    C: Clock,
    M: RawMutex,
{
    write_preamble(io, args.identity).await?;
    let mut session = ServerSession::<1024>::new(args.clock.now_ms());
    let mut read_buf = [0u8; 256];
    loop {
        if session.watchdog_expired(args.clock.now_ms()) {
            return Ok(());
        }
        flush_notices(io, args.hub, args.conn_id, &session).await?;
        let read = io.read(&mut read_buf);
        let pause = args.clock.sleep(50);
        match select(read, pause).await {
            Either::Second(()) => continue,
            Either::First(Ok(0)) => return Ok(()),
            Either::First(Err(err)) => return Err(Error::Io(err)),
            Either::First(Ok(n)) => {
                session.note_activity(args.clock.now_ms());
                session.push(&read_buf[..n])?;
                while let Some(raw) = session.pop_raw::<1024>()? {
                    let text = core::str::from_utf8(raw.as_slice())
                        .map_err(|_| Error::Protocol(hyperdeck_core::Error::InvalidUtf8))?;
                    let cmd = Command::parse(text)?;
                    if handle_builtin(io, &mut session, args, &cmd).await? {
                        return Ok(());
                    }
                }
            }
        }
    }
}

/// `true` when the connection should close (`quit`).
async fn handle_builtin<IO, H, C, M, const CONNS: usize>(
    io: &mut IO,
    session: &mut ServerSession<1024>,
    args: &ServeArgs<'_, H, C, M, CONNS>,
    cmd: &Command<'_>,
) -> Result<bool, Error<IO::Error>>
where
    IO: IoWrite,
    H: super::DeckHandler,
    C: Clock,
    M: RawMutex,
{
    match cmd.name {
        "ping" => {
            write_ok(io).await?;
            Ok(false)
        }
        "quit" => {
            write_ok(io).await?;
            Ok(true)
        }
        "help" | "?" => {
            write_block(io, 201, "help", HELP_TEXT).await?;
            Ok(false)
        }
        "commands" => {
            write_block(io, 212, "commands", COMMANDS_XML).await?;
            Ok(false)
        }
        "notify" => {
            write_notify(io, session, cmd).await?;
            Ok(false)
        }
        "remote" => {
            write_remote(io, session, args, cmd).await?;
            Ok(false)
        }
        "watchdog" => {
            let period = cmd.int("period")?.unwrap_or(0);
            session.set_watchdog(period, args.clock.now_ms());
            write_ok(io).await?;
            Ok(false)
        }
        "device info" => {
            write_device_info(io, args.identity).await?;
            Ok(false)
        }
        "uptime" => {
            let seconds = args.clock.now_ms().saturating_sub(args.boot_ms) / 1000;
            let mut body = String::<32>::new();
            let _ = write!(body, "uptime: {seconds}\r\n");
            write_block(io, 200, "ok", &body).await?;
            Ok(false)
        }
        _ => {
            if is_mutation(cmd) && !args.hub.remote_enabled().await && !session.remote_override() {
                write_simple(
                    io,
                    ResponseCode::REMOTE_DISABLED.as_u16(),
                    "remote control disabled",
                )
                .await?;
                return Ok(false);
            }
            dispatch(io, args, cmd).await?;
            Ok(false)
        }
    }
}

async fn dispatch<IO, H, C, M, const CONNS: usize>(
    io: &mut IO,
    args: &ServeArgs<'_, H, C, M, CONNS>,
    cmd: &Command<'_>,
) -> Result<(), Error<IO::Error>>
where
    IO: IoWrite,
    H: super::DeckHandler,
    M: RawMutex,
{
    let mut body_buf = [0u8; 1024];
    let mut writer = ReplyWriter::new(&mut body_buf);
    let outcome = args.handler.handle_command(cmd, &mut writer).await;
    write_outcome(io, &outcome, writer.as_str()).await?;
    if outcome.notify.is_some() && !outcome.notify_message.is_empty() {
        let kind = outcome.notify.unwrap_or(NotifyKind::Transport);
        args.hub.publish(Notice {
            kind,
            bytes: outcome.notify_message,
        });
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

async fn write_preamble<IO: IoWrite>(
    io: &mut IO,
    identity: &DeviceIdentity,
) -> Result<(), Error<IO::Error>> {
    let mut body = String::<96>::new();
    let _ = write!(
        body,
        "protocol version: {}\r\nmodel: {}\r\n",
        identity.protocol_version, identity.model
    );
    write_block(io, 500, "connection info", &body).await
}

async fn write_device_info<IO: IoWrite>(
    io: &mut IO,
    identity: &DeviceIdentity,
) -> Result<(), Error<IO::Error>> {
    let mut body = String::<192>::new();
    let _ = write!(
        body,
        "protocol version: {}\r\nmodel: {}\r\nunique id: {}\r\nslot count: {}\r\nsoftware version: {}\r\nname: {}\r\n",
        identity.protocol_version,
        identity.model,
        identity.unique_id,
        identity.slot_count,
        identity.software_version,
        identity.name
    );
    write_block(io, 204, "device info", &body).await
}

async fn write_notify<IO: IoWrite>(
    io: &mut IO,
    session: &mut ServerSession<1024>,
    cmd: &Command<'_>,
) -> Result<(), Error<IO::Error>> {
    if cmd.params.is_empty() {
        let flags = session.notify_flags();
        let mut body = String::<512>::new();
        write_flags(&mut body, &flags);
        return write_block(io, 209, "notify", &body).await;
    }
    for (key, value) in cmd.params.iter() {
        let enabled = match *value {
            "true" => true,
            "false" => false,
            _ => {
                write_simple(io, 102, "invalid value").await?;
                return Ok(());
            }
        };
        if !session.set_notify(key, enabled) {
            write_simple(io, 101, "unsupported parameter").await?;
            return Ok(());
        }
    }
    write_ok(io).await
}

fn write_flags(out: &mut impl Write, flags: &NotifyFlags) {
    let pairs = [
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
    ];
    for (name, enabled) in pairs {
        let value = if enabled { "true" } else { "false" };
        let _ = write!(out, "{name}: {value}\r\n");
    }
}

async fn write_remote<IO, H, C, M, const CONNS: usize>(
    io: &mut IO,
    session: &mut ServerSession<1024>,
    args: &ServeArgs<'_, H, C, M, CONNS>,
    cmd: &Command<'_>,
) -> Result<(), Error<IO::Error>>
where
    IO: IoWrite,
    M: RawMutex,
{
    if let Some(value) = cmd.flag("override")? {
        session.set_remote_override(value);
    }
    if let Some(value) = cmd.flag("enable")? {
        args.hub.set_remote_enabled(value).await;
    }
    if cmd.get("enable").is_some() || cmd.get("override").is_some() {
        // The query form has no parameters. Setting still returns the new state
        // only for the query; a set is `200 ok`, matching `notify`.
        if cmd
            .params
            .iter()
            .any(|(key, _)| *key == "enable" || *key == "override")
            && cmd.params.len()
                == cmd
                    .params
                    .iter()
                    .filter(|(key, _)| *key == "enable" || *key == "override")
                    .count()
        {
            // Fall through to the explicit branches below.
        }
    }
    if cmd.params.is_empty() {
        let enabled = if args.hub.remote_enabled().await {
            "true"
        } else {
            "false"
        };
        let override_flag = if session.remote_override() {
            "true"
        } else {
            "false"
        };
        let mut body = String::<64>::new();
        let _ = write!(body, "enabled: {enabled}\r\noverride: {override_flag}\r\n");
        write_block(io, 210, "remote info", &body).await
    } else {
        write_ok(io).await
    }
}

async fn flush_notices<IO, M, const CONNS: usize>(
    io: &mut IO,
    hub: &Hub<M, CONNS>,
    conn_id: usize,
    session: &ServerSession<1024>,
) -> Result<(), Error<IO::Error>>
where
    IO: IoWrite,
    M: RawMutex,
{
    while let Some(notice) = hub.poll(conn_id) {
        if session.should_notify(notice.kind) {
            io.write_all(notice.bytes.as_slice())
                .await
                .map_err(Error::Io)?;
        }
    }
    Ok(())
}

async fn write_outcome<IO: IoWrite>(
    io: &mut IO,
    outcome: &Outcome,
    body: &str,
) -> Result<(), Error<IO::Error>> {
    if body.is_empty() {
        write_simple(io, outcome.code, outcome.text.as_str()).await
    } else {
        write_block(io, outcome.code, outcome.text.as_str(), body).await
    }
}

async fn write_ok<IO: IoWrite>(io: &mut IO) -> Result<(), Error<IO::Error>> {
    write_simple(io, 200, "ok").await
}

async fn write_simple<IO: IoWrite>(
    io: &mut IO,
    code: u16,
    text: &str,
) -> Result<(), Error<IO::Error>> {
    let mut buf = Vec::<u8, 96>::new();
    encode_simple(&mut ByteBuf::new(&mut buf), code, text).map_err(|_| Error::BufferFull)?;
    io.write_all(buf.as_slice()).await.map_err(Error::Io)
}

async fn write_block<IO: IoWrite>(
    io: &mut IO,
    code: u16,
    text: &str,
    body: &str,
) -> Result<(), Error<IO::Error>> {
    let mut buf = Vec::<u8, 1200>::new();
    encode_block(&mut ByteBuf::new(&mut buf), code, text, body).map_err(|_| Error::BufferFull)?;
    io.write_all(buf.as_slice()).await.map_err(Error::Io)
}
