//! Handler trait. Built-in commands never reach it.

use core::fmt;

use hyperdeck_core::ethernet::Command;
use hyperdeck_core::session::NotifyKind;

use crate::error::Error;

/// Result of a command the handler understood.
pub struct Outcome {
    /// Response code. `200` is `ok`.
    pub code: u16,
    /// Reason phrase.
    pub text: heapless::String<40>,
    /// When set, `notify_message` is a complete `5xx` block to fan out.
    pub notify: Option<NotifyKind>,
    /// Encoded notice, including the header and the trailing blank line.
    pub notify_message: heapless::Vec<u8, 768>,
}

impl Outcome {
    /// `200 ok` and no notice.
    pub fn ok() -> Self {
        Self::status(200, "ok")
    }

    /// A status with no body and no notice.
    pub fn status(code: u16, text: &str) -> Self {
        let mut reason = heapless::String::new();
        push_trunc(&mut reason, text);
        Self {
            code,
            text: reason,
            notify: None,
            notify_message: heapless::Vec::new(),
        }
    }

    /// `103 unsupported`.
    pub fn unsupported() -> Self {
        Self::status(103, "unsupported")
    }

    fn from_apply(
        result: hyperdeck_core::ApplyResult,
        notify_message: heapless::Vec<u8, 768>,
    ) -> Self {
        Self {
            code: result.code,
            text: result.text,
            notify: result.notify,
            notify_message,
        }
    }
}

/// Fixed buffer a handler fills with `key: value` lines.
pub struct ReplyWriter<'a> {
    buf: &'a mut [u8],
    len: usize,
}

impl<'a> ReplyWriter<'a> {
    /// Writes into `buf`.
    pub fn new(buf: &'a mut [u8]) -> Self {
        Self { buf, len: 0 }
    }

    /// Bytes written.
    pub fn as_bytes(&self) -> &[u8] {
        &self.buf[..self.len]
    }

    /// UTF-8 view of the body. Non-UTF-8 input yields an empty string.
    pub fn as_str(&self) -> &str {
        core::str::from_utf8(self.as_bytes()).unwrap_or("")
    }
}

impl fmt::Write for ReplyWriter<'_> {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        let bytes = text.as_bytes();
        let end = self.len.checked_add(bytes.len()).ok_or(fmt::Error)?;
        if end > self.buf.len() {
            return Err(fmt::Error);
        }
        self.buf[self.len..end].copy_from_slice(bytes);
        self.len = end;
        Ok(())
    }
}

fn push_trunc<const N: usize>(out: &mut heapless::String<N>, text: &str) {
    for ch in text.chars() {
        if out.push(ch).is_err() {
            break;
        }
    }
}

/// Deck behaviour for one process.
///
/// The default [`DeckHandler::handle_command`] dispatches to the per-command
/// methods, which answer `103 unsupported` until overridden.
/// [`super::SharedSim`] replaces `handle_command` with the in-memory deck.
///
/// Every method returns a `Send` future. That is what tokio's multi-thread
/// runtime and Embassy's thread-mode executor both require, and it matches a
/// handler that only captures `&self`.
pub trait DeckHandler: Sync {
    /// Handles one command that the connection loop did not answer itself.
    ///
    /// Override this to take full control, or implement the `on_*` methods and
    /// keep this default, which calls [`dispatch_commands`].
    fn handle_command<'a>(
        &'a self,
        cmd: &'a Command<'_>,
        body: &'a mut ReplyWriter<'_>,
    ) -> impl core::future::Future<Output = Outcome> + Send + 'a {
        async move { dispatch_commands(self, cmd, body).await }
    }

    /// `play`.
    fn on_play<'a>(
        &'a self,
        _cmd: &'a Command<'_>,
        _body: &'a mut ReplyWriter<'_>,
    ) -> impl core::future::Future<Output = Outcome> + Send + 'a {
        async { Outcome::unsupported() }
    }

    /// `stop`.
    fn on_stop<'a>(
        &'a self,
        _cmd: &'a Command<'_>,
        _body: &'a mut ReplyWriter<'_>,
    ) -> impl core::future::Future<Output = Outcome> + Send + 'a {
        async { Outcome::unsupported() }
    }

    /// `record`.
    fn on_record<'a>(
        &'a self,
        _cmd: &'a Command<'_>,
        _body: &'a mut ReplyWriter<'_>,
    ) -> impl core::future::Future<Output = Outcome> + Send + 'a {
        async { Outcome::unsupported() }
    }

    /// `goto`.
    fn on_goto<'a>(
        &'a self,
        _cmd: &'a Command<'_>,
        _body: &'a mut ReplyWriter<'_>,
    ) -> impl core::future::Future<Output = Outcome> + Send + 'a {
        async { Outcome::unsupported() }
    }

    /// `jog`.
    fn on_jog<'a>(
        &'a self,
        _cmd: &'a Command<'_>,
        _body: &'a mut ReplyWriter<'_>,
    ) -> impl core::future::Future<Output = Outcome> + Send + 'a {
        async { Outcome::unsupported() }
    }

    /// `shuttle`.
    fn on_shuttle<'a>(
        &'a self,
        _cmd: &'a Command<'_>,
        _body: &'a mut ReplyWriter<'_>,
    ) -> impl core::future::Future<Output = Outcome> + Send + 'a {
        async { Outcome::unsupported() }
    }

    /// `preview`.
    fn on_preview<'a>(
        &'a self,
        _cmd: &'a Command<'_>,
        _body: &'a mut ReplyWriter<'_>,
    ) -> impl core::future::Future<Output = Outcome> + Send + 'a {
        async { Outcome::unsupported() }
    }

    /// `transport info`.
    fn on_transport_info<'a>(
        &'a self,
        _cmd: &'a Command<'_>,
        _body: &'a mut ReplyWriter<'_>,
    ) -> impl core::future::Future<Output = Outcome> + Send + 'a {
        async { Outcome::unsupported() }
    }

    /// `slot info`.
    fn on_slot_info<'a>(
        &'a self,
        _cmd: &'a Command<'_>,
        _body: &'a mut ReplyWriter<'_>,
    ) -> impl core::future::Future<Output = Outcome> + Send + 'a {
        async { Outcome::unsupported() }
    }

    /// `slot select`.
    fn on_slot_select<'a>(
        &'a self,
        _cmd: &'a Command<'_>,
        _body: &'a mut ReplyWriter<'_>,
    ) -> impl core::future::Future<Output = Outcome> + Send + 'a {
        async { Outcome::unsupported() }
    }

    /// `disk list`.
    fn on_disk_list<'a>(
        &'a self,
        _cmd: &'a Command<'_>,
        _body: &'a mut ReplyWriter<'_>,
    ) -> impl core::future::Future<Output = Outcome> + Send + 'a {
        async { Outcome::unsupported() }
    }

    /// `clips count`.
    fn on_clips_count<'a>(
        &'a self,
        _cmd: &'a Command<'_>,
        _body: &'a mut ReplyWriter<'_>,
    ) -> impl core::future::Future<Output = Outcome> + Send + 'a {
        async { Outcome::unsupported() }
    }

    /// `clips get`.
    fn on_clips_get<'a>(
        &'a self,
        _cmd: &'a Command<'_>,
        _body: &'a mut ReplyWriter<'_>,
    ) -> impl core::future::Future<Output = Outcome> + Send + 'a {
        async { Outcome::unsupported() }
    }

    /// `clips add`.
    fn on_clips_add<'a>(
        &'a self,
        _cmd: &'a Command<'_>,
        _body: &'a mut ReplyWriter<'_>,
    ) -> impl core::future::Future<Output = Outcome> + Send + 'a {
        async { Outcome::unsupported() }
    }

    /// `clips remove`.
    fn on_clips_remove<'a>(
        &'a self,
        _cmd: &'a Command<'_>,
        _body: &'a mut ReplyWriter<'_>,
    ) -> impl core::future::Future<Output = Outcome> + Send + 'a {
        async { Outcome::unsupported() }
    }

    /// `clips clear`.
    fn on_clips_clear<'a>(
        &'a self,
        _cmd: &'a Command<'_>,
        _body: &'a mut ReplyWriter<'_>,
    ) -> impl core::future::Future<Output = Outcome> + Send + 'a {
        async { Outcome::unsupported() }
    }

    /// `playrange`, `playrange set`, and `playrange clear`.
    fn on_playrange<'a>(
        &'a self,
        _cmd: &'a Command<'_>,
        _body: &'a mut ReplyWriter<'_>,
    ) -> impl core::future::Future<Output = Outcome> + Send + 'a {
        async { Outcome::unsupported() }
    }

    /// `configuration`.
    fn on_configuration<'a>(
        &'a self,
        _cmd: &'a Command<'_>,
        _body: &'a mut ReplyWriter<'_>,
    ) -> impl core::future::Future<Output = Outcome> + Send + 'a {
        async { Outcome::unsupported() }
    }

    /// `identify`.
    fn on_identify<'a>(
        &'a self,
        _cmd: &'a Command<'_>,
        _body: &'a mut ReplyWriter<'_>,
    ) -> impl core::future::Future<Output = Outcome> + Send + 'a {
        async { Outcome::unsupported() }
    }

    /// A command this crate does not dispatch, such as `slate` or `nas`.
    fn on_unknown<'a>(
        &'a self,
        _cmd: &'a Command<'_>,
        _body: &'a mut ReplyWriter<'_>,
    ) -> impl core::future::Future<Output = Outcome> + Send + 'a {
        async { Outcome::unsupported() }
    }
}

/// Dispatches `cmd` to the `on_*` methods of `handler`.
pub async fn dispatch_commands<H: DeckHandler + ?Sized>(
    handler: &H,
    cmd: &Command<'_>,
    body: &mut ReplyWriter<'_>,
) -> Outcome {
    match cmd.name {
        "play" => handler.on_play(cmd, body).await,
        "stop" => handler.on_stop(cmd, body).await,
        "record" => handler.on_record(cmd, body).await,
        "goto" => handler.on_goto(cmd, body).await,
        "jog" => handler.on_jog(cmd, body).await,
        "shuttle" => handler.on_shuttle(cmd, body).await,
        "preview" => handler.on_preview(cmd, body).await,
        "transport info" => handler.on_transport_info(cmd, body).await,
        "slot info" => handler.on_slot_info(cmd, body).await,
        "slot select" => handler.on_slot_select(cmd, body).await,
        "disk list" => handler.on_disk_list(cmd, body).await,
        "clips count" => handler.on_clips_count(cmd, body).await,
        "clips get" => handler.on_clips_get(cmd, body).await,
        "clips add" => handler.on_clips_add(cmd, body).await,
        "clips remove" => handler.on_clips_remove(cmd, body).await,
        "clips clear" => handler.on_clips_clear(cmd, body).await,
        "playrange" | "playrange set" | "playrange clear" => handler.on_playrange(cmd, body).await,
        "configuration" => handler.on_configuration(cmd, body).await,
        "identify" => handler.on_identify(cmd, body).await,
        _ => handler.on_unknown(cmd, body).await,
    }
}

pub(crate) fn outcome_from_sim(
    result: hyperdeck_core::ApplyResult,
    notify: &str,
) -> Result<Outcome, Error<()>> {
    let mut bytes = heapless::Vec::new();
    bytes
        .extend_from_slice(notify.as_bytes())
        .map_err(|_| Error::BufferFull)?;
    Ok(Outcome::from_apply(result, bytes))
}
