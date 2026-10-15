//! JSON models for the HyperDeck Control REST API.
//!
//! These types are the December 2024 document, with `camelCase` field names.
//! Unknown firmware fields are ignored (`deny_unknown_fields` is not set) so a
//! newer deck does not break deserialization.

use alloc::string::String;
use alloc::vec::Vec;

use serde::{Deserialize, Serialize};

/// `GET /transports/0`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransportMode {
    /// `InputPreview`, `InputRecord`, or `Output`.
    pub mode: String,
}

/// `GET /transports/0/playback`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackState {
    /// `Play`, `Jog`, `Shuttle`, or `Var`.
    #[serde(rename = "type")]
    pub kind: String,
    /// Loop the whole timeline.
    #[serde(rename = "loop")]
    pub loop_playback: bool,
    /// Loop the current clip.
    pub single_clip: bool,
    /// `1.0` is normal forward playback.
    pub speed: f64,
    /// Timeline position in frames.
    pub position: i64,
}

/// `GET /transports/0/record`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordState {
    /// `true` while the transport is in `InputRecord`.
    pub recording: bool,
}

/// `GET /transports/0/timecode`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimecodePair {
    /// Front-panel timecode.
    pub display: String,
    /// Timeline timecode.
    pub timeline: String,
}

/// `GET /system/product`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductInfo {
    /// Name shown in the setup UI.
    pub device_name: String,
    /// Product name.
    pub product_name: String,
    /// Software version.
    pub software_version: String,
}

/// Codec selection used by `/system/codecFormat` and clip metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodecFormat {
    /// Codec token such as `ProRes:HQ`.
    pub codec: String,
    /// Container such as `MOV`. Optional when setting a codec family.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub container: Option<String>,
}

/// Video format object used throughout the REST API.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoFormatInfo {
    /// Serialised name, for example `1920x1080p29.97`.
    pub name: String,
    /// Frame rate token.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame_rate: Option<String>,
    /// Height in lines.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    /// Width in samples.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    /// `true` for interlaced formats.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interlaced: Option<bool>,
}

/// One entry in `GET /media/workingset`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkingSetItem {
    /// Index in the working set.
    pub index: u32,
    /// This item is the active disk.
    pub active_disk: bool,
    /// Volume label.
    #[serde(default)]
    pub volume: String,
    /// Device name such as `ssd1`.
    pub device_name: String,
    /// Remaining record time in seconds.
    #[serde(default)]
    pub remaining_record_time: u64,
    /// Total capacity in bytes.
    #[serde(default)]
    pub total_space: u64,
    /// Free space in bytes.
    #[serde(default)]
    pub remaining_space: u64,
    /// Number of clips on the device.
    #[serde(default)]
    pub clip_count: u32,
}

/// `GET /media/workingset`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkingSet {
    /// Fixed size of the working set.
    pub size: u32,
    /// Slots. `null` entries are empty slots.
    pub workingset: Vec<Option<WorkingSetItem>>,
}

/// `GET /media/active`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveMedia {
    /// Working-set index.
    pub workingset_index: u32,
    /// Device name.
    pub device_name: String,
}

/// `GET /media/devices/{name}`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceState {
    /// `None`, `Scanning`, `Mounted`, `Uninitialised`, `Formatting`, or `RaidComponent`.
    pub state: String,
}

/// A clip as returned by `GET /clips`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Clip {
    /// Stable id on this disk.
    pub clip_unique_id: u32,
    /// Path relative to the mount.
    #[serde(default)]
    pub file_path: String,
    /// Size in bytes.
    #[serde(default)]
    pub file_size: u64,
    /// Codec, when the deck reported one.
    #[serde(default)]
    pub codec_format: Option<CodecFormat>,
    /// Video format.
    #[serde(default)]
    pub video_format: Option<VideoFormatInfo>,
    /// Start timecode.
    #[serde(default)]
    pub start_timecode: String,
    /// Duration as timecode.
    #[serde(default)]
    pub duration_timecode: String,
    /// Duration in frames.
    #[serde(default)]
    pub frame_count: u64,
}

/// One clip on `GET /timelines/0`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineClip {
    /// Disk clip id. Repeated when the same file is on the timeline twice.
    pub clip_unique_id: u32,
    /// Frames of this timeline entry.
    pub frame_count: u64,
    /// Duration as timecode.
    #[serde(default)]
    pub duration_timecode: String,
    /// In-point as a frame offset in the file.
    #[serde(default)]
    pub clip_in: String,
    /// Timecode of the in-point.
    #[serde(default)]
    pub in_timecode: String,
    /// Timeline frame of the in-point.
    #[serde(default)]
    pub timeline_in: String,
    /// Timeline timecode of the in-point.
    #[serde(default)]
    pub timeline_in_timecode: String,
}

/// `GET /timelines/0`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Timeline {
    /// Timeline entries in order.
    pub clips: Vec<TimelineClip>,
}

/// `GET /audio/recordFormat`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioFormat {
    /// `PCM` or `AAC`.
    pub codec: String,
    /// Channel count. The manual lists 2, 4, 8, 16, 32, and 64.
    pub num_channels: u32,
}

/// `GET /media/nas/discovered`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NasHost {
    /// mDNS host name.
    #[serde(default)]
    pub host_name: String,
    /// Friendly name.
    #[serde(default)]
    pub friendly_name: String,
    /// IP address.
    #[serde(default)]
    pub ip: String,
}

/// A NAS bookmark.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bookmark {
    /// Share URL, for example `smb://host/Share`.
    pub url: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playback_uses_protocol_field_names() {
        let state = PlaybackState {
            kind: "Play".into(),
            loop_playback: false,
            single_clip: true,
            speed: 1.0,
            position: 10,
        };
        let json = serde_json::to_value(&state).unwrap();
        assert_eq!(json["type"], "Play");
        assert_eq!(json["singleClip"], true);
        assert_eq!(json["loop"], false);
        let back: PlaybackState = serde_json::from_value(json).unwrap();
        assert_eq!(back, state);
    }
}
