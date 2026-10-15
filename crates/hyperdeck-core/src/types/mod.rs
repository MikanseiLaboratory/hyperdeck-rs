//! Shared values used by both the Ethernet protocol and the REST API.

mod response_code;
mod timecode;
mod video_format;

pub use response_code::{CodeClass, ResponseCode};
pub use timecode::{Timecode, TimecodeOffset};
pub use video_format::{FrameRate, Scan, VideoFormat};
