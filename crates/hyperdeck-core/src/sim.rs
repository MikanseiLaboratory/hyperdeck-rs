//! In-memory HyperDeck used by the mock servers.
//!
//! [`DeckSim`] accepts parsed commands and writes response bodies. It performs
//! no I/O, so the same deck runs inside tokio, a blocking thread, and Embassy.

use core::fmt::{self, Write};

use heapless::{String, Vec};

use crate::ethernet::Command;
use crate::session::NotifyKind;
use crate::types::{Timecode, VideoFormat};

/// One media file on the simulated disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimClip {
    /// File name relative to the disk root.
    pub name: String<48>,
    /// Duration in frames.
    pub frames: u32,
    /// Ethernet file-format token.
    pub file_format: String<32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TimelineEntry {
    clip_index: u16,
    in_frame: u32,
    frames: u32,
}

/// What [`DeckSim::apply`] decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyResult {
    /// Response code.
    pub code: u16,
    /// Reason phrase, without a trailing colon.
    pub text: String<40>,
    /// Set when `notify` was filled with a complete `5xx` message.
    pub notify: Option<NotifyKind>,
}

impl ApplyResult {
    fn new(code: u16, text: &str, notify: Option<NotifyKind>) -> Self {
        let mut reason = String::new();
        let _ = reason.push_str(text);
        Self {
            code,
            text: reason,
            notify,
        }
    }

    fn ok() -> Self {
        Self::new(200, "ok", None)
    }

    fn fail(code: u16) -> Self {
        let text = crate::types::ResponseCode::new(code)
            .canonical_text()
            .unwrap_or("error");
        Self::new(code, text, None)
    }
}

/// A small deck with two slots, a timeline, and a transport.
#[derive(Debug, Clone)]
pub struct DeckSim {
    status: String<16>,
    speed: i32,
    loop_playback: bool,
    single_clip: bool,
    slot_id: u8,
    timeline_frame: u32,
    fps: u32,
    video_format: String<24>,
    input_format: String<24>,
    video_input: String<16>,
    audio_input: String<16>,
    file_format: String<32>,
    record_prefix: String<32>,
    clips: Vec<SimClip, 16>,
    timeline: Vec<TimelineEntry, 16>,
    /// Inclusive start frame and exclusive end frame.
    playrange: Option<(u32, u32)>,
    recording_frames: u32,
    next_record: u32,
    remote_enabled: bool,
}

impl DeckSim {
    /// Two clips on slot 1, parked at the start of the timeline.
    pub fn demo() -> Self {
        let mut clips = Vec::new();
        let _ = clips.push(clip("A.mov", 300));
        let _ = clips.push(clip("B.mov", 150));
        let mut timeline = Vec::new();
        let _ = timeline.push(TimelineEntry {
            clip_index: 0,
            in_frame: 0,
            frames: 300,
        });
        let _ = timeline.push(TimelineEntry {
            clip_index: 1,
            in_frame: 0,
            frames: 150,
        });
        Self {
            status: str_buf("stopped"),
            speed: 0,
            loop_playback: false,
            single_clip: false,
            slot_id: 1,
            timeline_frame: 0,
            fps: 30,
            video_format: str_buf("1080p2997"),
            input_format: str_buf("1080p2997"),
            video_input: str_buf("SDI"),
            audio_input: str_buf("embedded"),
            file_format: str_buf("QuickTimeProRes"),
            record_prefix: str_buf("HyperDeck"),
            clips,
            timeline,
            playrange: None,
            recording_frames: 0,
            next_record: 1,
            remote_enabled: true,
        }
    }

    /// Transport status token (`stopped`, `play`, `record`, ...).
    pub fn transport_status(&self) -> &str {
        &self.status
    }

    /// Playback speed in percent.
    pub fn speed(&self) -> i32 {
        self.speed
    }

    /// Deck-wide remote-control switch exposed to the server.
    pub fn remote_enabled(&self) -> bool {
        self.remote_enabled
    }

    /// Sets the deck-wide remote-control switch.
    pub fn set_remote_enabled(&mut self, enabled: bool) {
        self.remote_enabled = enabled;
    }

    /// Current timeline frame.
    pub fn timeline_frame(&self) -> u32 {
        self.timeline_frame
    }

    /// Advances playback or recording by `frames` frame-periods at 1x.
    ///
    /// Returns `true` when the transport state changed and a notification
    /// should be sent. The caller writes that notification with
    /// [`DeckSim::write_transport_notice`].
    pub fn tick(&mut self, frames: u32) -> bool {
        if self.status.as_str() == "record" {
            self.recording_frames = self.recording_frames.saturating_add(frames);
            return true;
        }
        if self.status.as_str() != "play" && self.status.as_str() != "shuttle" {
            return false;
        }
        if self.speed == 0 || frames == 0 {
            return false;
        }
        let step = (self.speed.unsigned_abs() * frames) / 100;
        let step = step.max(1);
        if self.speed > 0 {
            self.timeline_frame = self.timeline_frame.saturating_add(step);
        } else {
            self.timeline_frame = self.timeline_frame.saturating_sub(step);
        }
        self.clamp_playback();
        true
    }

    /// Writes a `508 transport info` message for the current state.
    pub fn write_transport_notice(&self, out: &mut impl fmt::Write) -> fmt::Result {
        out.write_str("508 transport info:\r\n")?;
        self.write_transport(out)?;
        out.write_str("\r\n")?;
        Ok(())
    }

    /// Handles one command.
    ///
    /// Parameter lines of a block response are written to `body`. When
    /// [`ApplyResult::notify`] is set, `notify` holds a complete asynchronous
    /// message.
    pub fn apply(
        &mut self,
        cmd: &Command<'_>,
        body: &mut impl fmt::Write,
        notify: &mut impl fmt::Write,
    ) -> ApplyResult {
        match cmd.name {
            "play" => self.play(cmd, notify),
            "stop" => self.stop(notify),
            "record" => self.record(cmd, notify),
            "goto" => self.goto(cmd, notify),
            "jog" => self.jog(cmd, notify),
            "shuttle" => self.shuttle(cmd, notify),
            "preview" => self.preview(cmd, notify),
            "transport info" => self.query_transport(body),
            "slot info" => self.query_slot(cmd, body),
            "slot select" => self.select_slot(cmd, notify),
            "disk list" => self.disk_list(body),
            "clips count" => self.clips_count(body),
            "clips get" => self.clips_get(body),
            "clips add" => self.clips_add(cmd),
            "clips remove" => self.clips_remove(cmd),
            "clips clear" => self.clips_clear(),
            "playrange" => self.playrange_query(body),
            "playrange set" => self.playrange_set(cmd),
            "playrange clear" => self.playrange_clear(),
            "configuration" => self.configuration(cmd, body, notify),
            "identify" => ApplyResult::ok(),
            _ => ApplyResult::fail(103),
        }
    }

    fn play(&mut self, cmd: &Command<'_>, notify: &mut impl fmt::Write) -> ApplyResult {
        if self.timeline.is_empty() {
            return ApplyResult::fail(107);
        }
        if let Some(speed) = int_param(cmd, "speed") {
            let Ok(speed) = speed else {
                return ApplyResult::fail(102);
            };
            if !(-5000..=5000).contains(&speed) {
                return ApplyResult::fail(109);
            }
            self.speed = speed;
        } else {
            self.speed = 100;
        }
        if let Some(value) = flag_param(cmd, "loop") {
            let Ok(value) = value else {
                return ApplyResult::fail(102);
            };
            self.loop_playback = value;
        }
        if let Some(value) = flag_param(cmd, "single clip") {
            let Ok(value) = value else {
                return ApplyResult::fail(102);
            };
            self.single_clip = value;
        }
        if let Some(clip_id) = cmd.get("clip id")
            && let Err(code) = self.seek_clip_id(clip_id)
        {
            return ApplyResult::fail(code);
        }
        self.status.clear();
        let _ = self
            .status
            .push_str(if self.speed < 0 { "rewind" } else { "play" });
        self.emit_transport(notify);
        ApplyResult::new(200, "ok", Some(NotifyKind::Transport))
    }

    fn stop(&mut self, notify: &mut impl fmt::Write) -> ApplyResult {
        self.finish_recording();
        self.speed = 0;
        self.status.clear();
        let _ = self.status.push_str("stopped");
        self.emit_transport(notify);
        ApplyResult::new(200, "ok", Some(NotifyKind::Transport))
    }

    fn record(&mut self, cmd: &Command<'_>, notify: &mut impl fmt::Write) -> ApplyResult {
        if let Some(name) = cmd.get("name") {
            self.record_prefix.clear();
            push_trunc(&mut self.record_prefix, name);
        }
        self.recording_frames = 0;
        self.speed = 100;
        self.status.clear();
        let _ = self.status.push_str("record");
        self.emit_transport(notify);
        ApplyResult::new(200, "ok", Some(NotifyKind::Transport))
    }

    fn goto(&mut self, cmd: &Command<'_>, notify: &mut impl fmt::Write) -> ApplyResult {
        if let Some(slot) = cmd.get("slot id") {
            let Ok(slot) = slot.parse::<u8>() else {
                return ApplyResult::fail(102);
            };
            if slot == 0 || slot > 2 {
                return ApplyResult::fail(109);
            }
            self.slot_id = slot;
        }
        if let Some(target) = cmd.get("clip id") {
            if let Err(code) = self.seek_clip_id(target) {
                return ApplyResult::fail(code);
            }
        } else if let Some(target) = cmd.get("timeline") {
            if let Err(code) = self.seek_timeline(target) {
                return ApplyResult::fail(code);
            }
        } else if let Some(target) = cmd.get("timecode") {
            let Ok(tc) = crate::types::TimecodeOffset::parse(target) else {
                return ApplyResult::fail(102);
            };
            let frames = match tc {
                crate::types::TimecodeOffset::Absolute(tc) => tc.to_frames(self.fps),
                crate::types::TimecodeOffset::Relative { forward, timecode } => {
                    let delta = timecode.to_frames(self.fps);
                    if forward {
                        self.timeline_frame.saturating_add(delta)
                    } else {
                        self.timeline_frame.saturating_sub(delta)
                    }
                }
            };
            self.timeline_frame = frames.min(self.timeline_len());
        } else if let Some(target) = cmd.get("clip")
            && let Err(code) = self.seek_inside_clip(target)
        {
            return ApplyResult::fail(code);
        }
        self.finish_recording();
        self.speed = 0;
        self.status.clear();
        let _ = self.status.push_str("stopped");
        self.emit_transport(notify);
        ApplyResult::new(200, "ok", Some(NotifyKind::Transport))
    }

    fn jog(&mut self, cmd: &Command<'_>, notify: &mut impl fmt::Write) -> ApplyResult {
        let Some(target) = cmd.get("timecode") else {
            return ApplyResult::fail(100);
        };
        let Ok(offset) = crate::types::TimecodeOffset::parse(target) else {
            return ApplyResult::fail(102);
        };
        if let crate::types::TimecodeOffset::Relative { forward, timecode } = offset {
            let delta = timecode.to_frames(self.fps).max(1);
            if forward {
                self.timeline_frame = self.timeline_frame.saturating_add(delta);
            } else {
                self.timeline_frame = self.timeline_frame.saturating_sub(delta);
            }
        }
        self.speed = 0;
        self.status.clear();
        let _ = self.status.push_str("jog");
        self.emit_transport(notify);
        ApplyResult::new(200, "ok", Some(NotifyKind::Transport))
    }

    fn shuttle(&mut self, cmd: &Command<'_>, notify: &mut impl fmt::Write) -> ApplyResult {
        let Some(speed) = cmd.get("speed") else {
            return ApplyResult::fail(100);
        };
        let Ok(speed) = speed.parse::<i32>() else {
            return ApplyResult::fail(102);
        };
        if !(-5000..=5000).contains(&speed) {
            return ApplyResult::fail(109);
        }
        self.speed = speed;
        self.status.clear();
        let _ = self.status.push_str("shuttle");
        self.emit_transport(notify);
        ApplyResult::new(200, "ok", Some(NotifyKind::Transport))
    }

    fn preview(&mut self, cmd: &Command<'_>, notify: &mut impl fmt::Write) -> ApplyResult {
        let Some(flag) = flag_param(cmd, "enable") else {
            return ApplyResult::fail(100);
        };
        let Ok(enable) = flag else {
            return ApplyResult::fail(102);
        };
        self.finish_recording();
        self.speed = 0;
        self.status.clear();
        let _ = self
            .status
            .push_str(if enable { "preview" } else { "stopped" });
        self.emit_transport(notify);
        ApplyResult::new(200, "ok", Some(NotifyKind::Transport))
    }

    fn query_transport(&self, body: &mut impl fmt::Write) -> ApplyResult {
        if self.write_transport(body).is_err() {
            return ApplyResult::fail(108);
        }
        ApplyResult::new(208, "transport info", None)
    }

    fn query_slot(&self, cmd: &Command<'_>, body: &mut impl fmt::Write) -> ApplyResult {
        let slot = if let Some(value) = cmd.get("slot id") {
            match value.parse::<u8>() {
                Ok(slot) if (1..=2).contains(&slot) => slot,
                Ok(_) => return ApplyResult::fail(109),
                Err(_) => return ApplyResult::fail(102),
            }
        } else {
            self.slot_id
        };
        let mounted = slot == 1;
        let status = if mounted { "mounted" } else { "empty" };
        let _ = write!(
            body,
            "slot id: {slot}\r\nslot name: ssd{slot}\r\ndevice name: ssd{slot}\r\nstatus: {status}\r\n"
        );
        if mounted {
            let _ = write!(
                body,
                "volume name: Media\r\nrecording time: 3600\r\nvideo format: {}\r\nblocked: false\r\nremaining size: 1000000000\r\ntotal size: 2000000000\r\n",
                self.video_format
            );
        }
        ApplyResult::new(202, "slot info", None)
    }

    fn select_slot(&mut self, cmd: &Command<'_>, notify: &mut impl fmt::Write) -> ApplyResult {
        if let Some(slot) = cmd.get("slot id") {
            let Ok(slot) = slot.parse::<u8>() else {
                return ApplyResult::fail(102);
            };
            if !(1..=2).contains(&slot) {
                return ApplyResult::fail(109);
            }
            self.slot_id = slot;
        }
        if let Some(format) = cmd.get("video format") {
            if VideoFormat::parse_ethernet(format).is_err() {
                return ApplyResult::fail(102);
            }
            self.video_format.clear();
            push_trunc(&mut self.video_format, format);
        }
        self.emit_transport(notify);
        ApplyResult::new(200, "ok", Some(NotifyKind::Transport))
    }

    fn disk_list(&self, body: &mut impl fmt::Write) -> ApplyResult {
        let _ = write!(body, "slot id: {}\r\n", self.slot_id);
        for (index, clip) in self.clips.iter().enumerate() {
            let tc = Timecode::from_frames(clip.frames, self.fps);
            let _ = write!(
                body,
                "{}: {} {} {} {}\r\n",
                index + 1,
                clip.name,
                clip.file_format,
                self.video_format,
                tc
            );
        }
        ApplyResult::new(206, "disk list", None)
    }

    fn clips_count(&self, body: &mut impl fmt::Write) -> ApplyResult {
        let _ = write!(body, "clip count: {}\r\n", self.timeline.len());
        ApplyResult::new(214, "clips count", None)
    }

    fn clips_get(&self, body: &mut impl fmt::Write) -> ApplyResult {
        let _ = write!(body, "clip count: {}\r\n", self.timeline.len());
        let mut cursor = 0u32;
        for (index, entry) in self.timeline.iter().enumerate() {
            let Some(clip) = self.clips.get(entry.clip_index as usize) else {
                continue;
            };
            let start = Timecode::from_frames(cursor, self.fps);
            let duration = Timecode::from_frames(entry.frames, self.fps);
            let _ = write!(
                body,
                "{}: {} {} {}\r\n",
                index + 1,
                clip.name,
                start,
                duration
            );
            cursor = cursor.saturating_add(entry.frames);
        }
        ApplyResult::new(205, "clips info", None)
    }

    fn clips_add(&mut self, cmd: &Command<'_>) -> ApplyResult {
        let Some(name) = cmd.get("name") else {
            return ApplyResult::fail(100);
        };
        let Some(index) = self.find_clip(name) else {
            return ApplyResult::fail(112);
        };
        let frames = self.clips[index].frames;
        let entry = TimelineEntry {
            clip_index: index as u16,
            in_frame: 0,
            frames,
        };
        if let Some(before) = cmd.get("clip id") {
            let Ok(before) = before.parse::<usize>() else {
                return ApplyResult::fail(102);
            };
            if before == 0 || before > self.timeline.len() + 1 {
                return ApplyResult::fail(109);
            }
            // heapless::Vec has no insert. Rebuild.
            let mut next = Vec::new();
            for (idx, existing) in self.timeline.iter().cloned().enumerate() {
                if idx + 1 == before {
                    let _ = next.push(entry.clone());
                }
                let _ = next.push(existing);
            }
            if before == self.timeline.len() + 1 {
                let _ = next.push(entry);
            }
            self.timeline = next;
        } else if self.timeline.push(entry).is_err() {
            return ApplyResult::fail(109);
        }
        ApplyResult::ok()
    }

    fn clips_remove(&mut self, cmd: &Command<'_>) -> ApplyResult {
        let Some(id) = cmd.get("clip id") else {
            return ApplyResult::fail(100);
        };
        let Ok(id) = id.parse::<usize>() else {
            return ApplyResult::fail(102);
        };
        if id == 0 || id > self.timeline.len() {
            return ApplyResult::fail(109);
        }
        let mut next = Vec::new();
        for (idx, entry) in self.timeline.iter().cloned().enumerate() {
            if idx + 1 != id {
                let _ = next.push(entry);
            }
        }
        self.timeline = next;
        if self.timeline_frame > self.timeline_len() {
            self.timeline_frame = self.timeline_len();
        }
        ApplyResult::ok()
    }

    fn clips_clear(&mut self) -> ApplyResult {
        self.timeline.clear();
        self.timeline_frame = 0;
        self.playrange = None;
        ApplyResult::ok()
    }

    fn playrange_query(&self, body: &mut impl fmt::Write) -> ApplyResult {
        if let Some((start, end)) = self.playrange {
            let _ = write!(body, "timeline in: {start}\r\ntimeline out: {end}\r\n");
        }
        ApplyResult::new(200, "ok", None)
    }

    fn playrange_set(&mut self, cmd: &Command<'_>) -> ApplyResult {
        if let (Some(start), Some(end)) = (cmd.get("timeline in"), cmd.get("timeline out")) {
            let Ok(start) = start.parse::<u32>() else {
                return ApplyResult::fail(102);
            };
            let Ok(end) = end.parse::<u32>() else {
                return ApplyResult::fail(102);
            };
            if end < start {
                return ApplyResult::fail(109);
            }
            self.playrange = Some((start, end));
            return ApplyResult::ok();
        }
        if let Some(id) = cmd.get("clip id") {
            let Ok(id) = id.parse::<usize>() else {
                return ApplyResult::fail(102);
            };
            if id == 0 || id > self.timeline.len() {
                return ApplyResult::fail(109);
            }
            let count = cmd
                .get("count")
                .map(|value| value.parse::<usize>())
                .transpose()
                .map_err(|_| ())
                .ok()
                .flatten()
                .unwrap_or(1);
            let start = self.clip_start(id - 1);
            let mut end = start;
            for entry in self.timeline.iter().skip(id - 1).take(count) {
                end = end.saturating_add(entry.frames);
            }
            self.playrange = Some((start, end));
            return ApplyResult::ok();
        }
        if let (Some(start), Some(end)) = (cmd.get("in"), cmd.get("out")) {
            let Ok(start) = Timecode::parse(start) else {
                return ApplyResult::fail(102);
            };
            let Ok(end) = Timecode::parse(end) else {
                return ApplyResult::fail(102);
            };
            self.playrange = Some((start.to_frames(self.fps), end.to_frames(self.fps)));
            return ApplyResult::ok();
        }
        ApplyResult::fail(100)
    }

    fn playrange_clear(&mut self) -> ApplyResult {
        self.playrange = None;
        ApplyResult::ok()
    }

    fn configuration(
        &mut self,
        cmd: &Command<'_>,
        body: &mut impl fmt::Write,
        notify: &mut impl fmt::Write,
    ) -> ApplyResult {
        if cmd.params.is_empty() {
            let _ = write!(
                body,
                "audio input: {}\r\nvideo input: {}\r\nfile format: {}\r\ntimecode input: embedded\r\ntimecode output: timeline\r\nrecord prefix: {}\r\n",
                self.audio_input, self.video_input, self.file_format, self.record_prefix
            );
            return ApplyResult::new(211, "configuration", None);
        }
        if let Some(value) = cmd.get("video input") {
            self.video_input.clear();
            push_trunc(&mut self.video_input, value);
        }
        if let Some(value) = cmd.get("audio input") {
            self.audio_input.clear();
            push_trunc(&mut self.audio_input, value);
        }
        if let Some(value) = cmd.get("file format") {
            self.file_format.clear();
            push_trunc(&mut self.file_format, value);
        }
        if let Some(value) = cmd.get("record prefix") {
            self.record_prefix.clear();
            push_trunc(&mut self.record_prefix, value);
        }
        let _ = write!(notify, "511 configuration:\r\n");
        let _ = write!(
            notify,
            "audio input: {}\r\nvideo input: {}\r\nfile format: {}\r\nrecord prefix: {}\r\n\r\n",
            self.audio_input, self.video_input, self.file_format, self.record_prefix
        );
        ApplyResult::new(200, "ok", Some(NotifyKind::Configuration))
    }

    fn write_transport(&self, out: &mut impl fmt::Write) -> fmt::Result {
        let tc = Timecode::from_frames(self.timeline_frame, self.fps);
        let clip_id = self.current_clip_id();
        let clip_token = match clip_id {
            Some(id) => {
                let mut buf = String::<8>::new();
                let _ = write!(buf, "{id}");
                buf
            }
            None => str_buf("none"),
        };
        write!(
            out,
            "status: {}\r\nspeed: {}\r\nslot id: {}\r\nslot name: ssd{}\r\ndevice name: ssd{}\r\nclip id: {}\r\nsingle clip: {}\r\ndisplay timecode: {tc}\r\ntimecode: {tc}\r\nvideo format: {}\r\nloop: {}\r\ntimeline: {}\r\ninput video format: {}\r\ndynamic range: none\r\nreference locked: false\r\n",
            self.status,
            self.speed,
            self.slot_id,
            self.slot_id,
            self.slot_id,
            clip_token,
            bool_token(self.single_clip),
            self.video_format,
            bool_token(self.loop_playback),
            self.timeline_frame,
            self.input_format,
        )
    }

    fn emit_transport(&self, notify: &mut impl fmt::Write) {
        let _ = self.write_transport_notice(notify);
    }

    fn finish_recording(&mut self) {
        if self.status.as_str() != "record" {
            return;
        }
        let mut name = String::new();
        let _ = write!(name, "{}_{:04}.mov", self.record_prefix, self.next_record);
        self.next_record = self.next_record.saturating_add(1);
        let frames = self.recording_frames.max(1);
        if self
            .clips
            .push(SimClip {
                name,
                frames,
                file_format: self.file_format.clone(),
            })
            .is_ok()
        {
            let index = self.clips.len() - 1;
            let _ = self.timeline.push(TimelineEntry {
                clip_index: index as u16,
                in_frame: 0,
                frames,
            });
        }
        self.recording_frames = 0;
    }

    fn seek_clip_id(&mut self, target: &str) -> Result<(), u16> {
        if self.timeline.is_empty() {
            return Err(107);
        }
        let index = if target == "start" {
            0
        } else if target == "end" {
            self.timeline.len() - 1
        } else if let Some(rest) = target.strip_prefix('+') {
            let step = rest.parse::<usize>().map_err(|_| 102u16)?;
            self.current_clip_index()
                .saturating_add(step)
                .min(self.timeline.len() - 1)
        } else if let Some(rest) = target.strip_prefix('-') {
            let step = rest.parse::<usize>().map_err(|_| 102u16)?;
            self.current_clip_index().saturating_sub(step)
        } else {
            let id = target.parse::<usize>().map_err(|_| 102u16)?;
            if id == 0 || id > self.timeline.len() {
                return Err(109);
            }
            id - 1
        };
        self.timeline_frame = self.clip_start(index);
        Ok(())
    }

    fn seek_timeline(&mut self, target: &str) -> Result<(), u16> {
        let len = self.timeline_len();
        self.timeline_frame = if target == "start" {
            0
        } else if target == "end" {
            len
        } else if let Some(rest) = target.strip_prefix('+') {
            let step = rest.parse::<u32>().map_err(|_| 102u16)?;
            self.timeline_frame.saturating_add(step).min(len)
        } else if let Some(rest) = target.strip_prefix('-') {
            let step = rest.parse::<u32>().map_err(|_| 102u16)?;
            self.timeline_frame.saturating_sub(step)
        } else {
            let frame = target.parse::<u32>().map_err(|_| 102u16)?;
            if frame > len {
                return Err(109);
            }
            frame
        };
        Ok(())
    }

    fn seek_inside_clip(&mut self, target: &str) -> Result<(), u16> {
        let index = self.current_clip_index();
        let Some(entry) = self.timeline.get(index) else {
            return Err(107);
        };
        let start = self.clip_start(index);
        let local = self.timeline_frame.saturating_sub(start);
        let next = if target == "start" {
            0
        } else if target == "end" {
            entry.frames
        } else if let Some(rest) = target.strip_prefix('+') {
            let step = rest.parse::<u32>().map_err(|_| 102u16)?;
            local.saturating_add(step).min(entry.frames)
        } else if let Some(rest) = target.strip_prefix('-') {
            let step = rest.parse::<u32>().map_err(|_| 102u16)?;
            local.saturating_sub(step)
        } else {
            let frame = target.parse::<u32>().map_err(|_| 102u16)?;
            if frame > entry.frames {
                return Err(109);
            }
            frame
        };
        self.timeline_frame = start.saturating_add(next);
        Ok(())
    }

    fn clamp_playback(&mut self) {
        let (start, end) = self.playrange.unwrap_or((0, self.timeline_len()));
        let end = end.max(start);
        if self.speed >= 0 && self.timeline_frame >= end {
            if self.loop_playback {
                self.timeline_frame = start;
            } else {
                self.timeline_frame = end;
                self.speed = 0;
                self.status.clear();
                let _ = self.status.push_str("stopped");
            }
        } else if self.speed < 0 && self.timeline_frame <= start {
            if self.loop_playback {
                self.timeline_frame = end;
            } else {
                self.timeline_frame = start;
                self.speed = 0;
                self.status.clear();
                let _ = self.status.push_str("stopped");
            }
        }
    }

    fn timeline_len(&self) -> u32 {
        self.timeline.iter().map(|entry| entry.frames).sum()
    }

    fn clip_start(&self, index: usize) -> u32 {
        self.timeline
            .iter()
            .take(index)
            .map(|entry| entry.frames)
            .sum()
    }

    fn current_clip_index(&self) -> usize {
        let mut cursor = 0u32;
        for (index, entry) in self.timeline.iter().enumerate() {
            let next = cursor.saturating_add(entry.frames);
            if self.timeline_frame < next || index + 1 == self.timeline.len() {
                return index;
            }
            cursor = next;
        }
        0
    }

    fn current_clip_id(&self) -> Option<u32> {
        if self.timeline.is_empty() {
            None
        } else {
            Some(self.current_clip_index() as u32 + 1)
        }
    }

    fn find_clip(&self, name: &str) -> Option<usize> {
        self.clips
            .iter()
            .position(|clip| clip.name.as_str() == name)
    }
}

fn clip(name: &str, frames: u32) -> SimClip {
    SimClip {
        name: str_buf(name),
        frames,
        file_format: str_buf("QuickTimeProRes"),
    }
}

fn str_buf<const N: usize>(text: &str) -> String<N> {
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

fn bool_token(value: bool) -> &'static str {
    if value { "true" } else { "false" }
}

fn flag_param(cmd: &Command<'_>, key: &str) -> Option<Result<bool, ()>> {
    cmd.get(key).map(|value| match value {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(()),
    })
}

fn int_param(cmd: &Command<'_>, key: &str) -> Option<Result<i32, ()>> {
    cmd.get(key).map(|value| value.parse().map_err(|_| ()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ethernet::Response;
    use core::fmt::Write;

    fn run(sim: &mut DeckSim, line: &str) -> (ApplyResult, String<1024>, String<1024>) {
        let cmd = Command::parse(line).unwrap();
        let mut body = String::new();
        let mut notify = String::new();
        let result = sim.apply(&cmd, &mut body, &mut notify);
        (result, body, notify)
    }

    #[test]
    fn play_then_query_transport() {
        let mut sim = DeckSim::demo();
        let (result, _, notify) = run(&mut sim, "play: speed: 200 loop: true\r\n");
        assert_eq!(result.code, 200);
        assert!(notify.contains("status: play"));
        assert_eq!(sim.speed(), 200);
        let (result, body, _) = run(&mut sim, "transport info\r\n");
        assert_eq!(result.code, 208);
        let mut framed = String::<1200>::new();
        let _ = write!(framed, "208 transport info:\r\n{body}\r\n");
        let response = Response::parse(&framed).unwrap();
        assert_eq!(response.get("speed"), Some("200"));
        assert_eq!(response.get("loop"), Some("true"));
    }

    #[test]
    fn playrange_from_the_manual() {
        let mut sim = DeckSim::demo();
        let (result, _, _) = run(&mut sim, "playrange set: clip id: 1 count: 1\r\n");
        assert_eq!(result.code, 200);
        let (result, body, _) = run(&mut sim, "playrange\r\n");
        assert_eq!(result.code, 200);
        assert!(body.contains("timeline in: 0"));
    }

    #[test]
    fn disk_list_round_trip() {
        let mut sim = DeckSim::demo();
        let (result, body, _) = run(&mut sim, "disk list\r\n");
        assert_eq!(result.code, 206);
        assert!(body.contains("1: A.mov QuickTimeProRes 1080p2997 00:00:10:00"));
    }
}
