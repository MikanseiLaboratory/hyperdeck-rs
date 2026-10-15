//! Per-connection server state: notification mask, remote override, watchdog.

use crate::error::Error;
use crate::ethernet::LineFramer;

/// Which asynchronous messages a connection asked to receive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotifyFlags {
    /// `notify: remote`.
    pub remote: bool,
    /// `notify: transport`.
    pub transport: bool,
    /// `notify: slot`.
    pub slot: bool,
    /// `notify: configuration`.
    pub configuration: bool,
    /// `notify: dropped frames`.
    pub dropped_frames: bool,
    /// `notify: display timecode`.
    pub display_timecode: bool,
    /// `notify: timeline position`.
    pub timeline_position: bool,
    /// `notify: playrange`.
    pub playrange: bool,
    /// `notify: cache`.
    pub cache: bool,
    /// `notify: dynamic range`.
    pub dynamic_range: bool,
    /// `notify: slate`.
    pub slate: bool,
    /// `notify: clips`.
    pub clips: bool,
    /// `notify: disk`.
    pub disk: bool,
    /// `notify: device info`.
    pub device_info: bool,
    /// `notify: nas`.
    pub nas: bool,
}

impl NotifyFlags {
    /// Every subscription disabled, which is the deck's default.
    pub const fn none() -> Self {
        Self {
            remote: false,
            transport: false,
            slot: false,
            configuration: false,
            dropped_frames: false,
            display_timecode: false,
            timeline_position: false,
            playrange: false,
            cache: false,
            dynamic_range: false,
            slate: false,
            clips: false,
            disk: false,
            device_info: false,
            nas: false,
        }
    }

    /// Sets one flag by its protocol parameter name.
    ///
    /// Returns `false` when `name` is not a known subscription.
    pub fn set(&mut self, name: &str, enabled: bool) -> bool {
        let slot = match name {
            "remote" => &mut self.remote,
            "transport" => &mut self.transport,
            "slot" => &mut self.slot,
            "configuration" => &mut self.configuration,
            "dropped frames" => &mut self.dropped_frames,
            "display timecode" => &mut self.display_timecode,
            "timeline position" => &mut self.timeline_position,
            "playrange" => &mut self.playrange,
            "cache" => &mut self.cache,
            "dynamic range" => &mut self.dynamic_range,
            "slate" => &mut self.slate,
            "clips" => &mut self.clips,
            "disk" => &mut self.disk,
            "device info" => &mut self.device_info,
            "nas" => &mut self.nas,
            _ => return false,
        };
        *slot = enabled;
        true
    }

    /// The flag that corresponds to `kind`.
    pub fn get(self, kind: NotifyKind) -> bool {
        match kind {
            NotifyKind::Remote => self.remote,
            NotifyKind::Transport => self.transport,
            NotifyKind::Slot => self.slot,
            NotifyKind::Configuration => self.configuration,
            NotifyKind::DroppedFrames => self.dropped_frames,
            NotifyKind::DisplayTimecode => self.display_timecode,
            NotifyKind::TimelinePosition => self.timeline_position,
            NotifyKind::Playrange => self.playrange,
            NotifyKind::Cache => self.cache,
            NotifyKind::DynamicRange => self.dynamic_range,
            NotifyKind::Slate => self.slate,
            NotifyKind::Clips => self.clips,
            NotifyKind::Disk => self.disk,
            NotifyKind::DeviceInfo => self.device_info,
            NotifyKind::Nas => self.nas,
        }
    }
}

/// A class of asynchronous notice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotifyKind {
    /// `510 remote info`.
    Remote,
    /// `508 transport info`.
    Transport,
    /// `502 slot info`.
    Slot,
    /// `511 configuration`.
    Configuration,
    /// Dropped-frame counter.
    DroppedFrames,
    /// Front-panel timecode.
    DisplayTimecode,
    /// Playback position.
    TimelinePosition,
    /// Play-range changes.
    Playrange,
    /// Cache state.
    Cache,
    /// Dynamic-range settings.
    DynamicRange,
    /// Slate metadata.
    Slate,
    /// Timeline clip list.
    Clips,
    /// Disk clip list.
    Disk,
    /// Device identity.
    DeviceInfo,
    /// NAS bookmarks and mounts.
    Nas,
}

/// Per-connection server state machine.
#[derive(Debug)]
pub struct ServerSession<const N: usize> {
    framer: LineFramer<N>,
    notify: NotifyFlags,
    remote_override: bool,
    watchdog_ms: Option<u64>,
    last_activity_ms: u64,
}

impl<const N: usize> ServerSession<N> {
    /// A new connection. `now_ms` starts the watchdog activity clock.
    pub const fn new(now_ms: u64) -> Self {
        Self {
            framer: LineFramer::new(),
            notify: NotifyFlags::none(),
            remote_override: false,
            watchdog_ms: None,
            last_activity_ms: now_ms,
        }
    }

    /// Records that the client sent something at `now_ms`.
    pub fn note_activity(&mut self, now_ms: u64) {
        self.last_activity_ms = now_ms;
    }

    /// Applies `watchdog: period: {seconds}`.
    ///
    /// A period of zero or less disables monitoring, matching the specification.
    pub fn set_watchdog(&mut self, period_seconds: i32, now_ms: u64) {
        self.note_activity(now_ms);
        self.watchdog_ms = if period_seconds > 0 {
            Some(u64::from(period_seconds as u32) * 1000)
        } else {
            None
        };
    }

    /// The absolute deadline in caller-supplied milliseconds, if the watchdog is armed.
    pub fn next_deadline(&self) -> Option<u64> {
        self.watchdog_ms
            .map(|period| self.last_activity_ms.saturating_add(period))
    }

    /// `true` when the client has been silent longer than the configured period.
    pub fn watchdog_expired(&self, now_ms: u64) -> bool {
        self.next_deadline()
            .is_some_and(|deadline| now_ms >= deadline)
    }

    /// Current notification mask.
    pub fn notify_flags(&self) -> NotifyFlags {
        self.notify
    }

    /// Updates one subscription. Returns `false` for an unknown name.
    pub fn set_notify(&mut self, name: &str, enabled: bool) -> bool {
        self.notify.set(name, enabled)
    }

    /// Whether this connection should receive `kind`.
    pub fn should_notify(&self, kind: NotifyKind) -> bool {
        self.notify.get(kind)
    }

    /// Session-scoped remote override. This does not change the deck-wide switch.
    pub fn set_remote_override(&mut self, enabled: bool) {
        self.remote_override = enabled;
    }

    /// The override flag for this connection.
    pub fn remote_override(&self) -> bool {
        self.remote_override
    }

    /// Appends bytes received from the client.
    pub fn push(&mut self, data: &[u8]) -> Result<(), Error> {
        self.framer.push(data)
    }

    /// Pops the next command as raw bytes, including the terminator.
    pub fn pop_raw<const M: usize>(&mut self) -> Result<Option<heapless::Vec<u8, M>>, Error> {
        self.framer.pop()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watchdog_deadline() {
        let mut session = ServerSession::<32>::new(1_000);
        assert!(session.next_deadline().is_none());
        session.set_watchdog(2, 1_500);
        assert_eq!(session.next_deadline(), Some(3_500));
        assert!(!session.watchdog_expired(3_499));
        assert!(session.watchdog_expired(3_500));
        session.set_watchdog(0, 4_000);
        assert!(!session.watchdog_expired(99_000));
    }

    #[test]
    fn notify_mask() {
        let mut session = ServerSession::<32>::new(0);
        assert!(session.set_notify("transport", true));
        assert!(session.should_notify(NotifyKind::Transport));
        assert!(!session.should_notify(NotifyKind::Slot));
        assert!(!session.set_notify("not-a-real-flag", true));
    }
}
