//! Non-drop-frame timecode (`HH:MM:SS:FF`) and relative offsets.

use core::fmt;

use crate::error::Error;

/// A non-drop-frame timecode.
///
/// The HyperDeck Ethernet Protocol writes timecode as `HH:MM:SS:FF`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Timecode {
    /// Hours, `0..=99`.
    pub hours: u8,
    /// Minutes, `0..=59`.
    pub minutes: u8,
    /// Seconds, `0..=59`.
    pub seconds: u8,
    /// Frames. The valid range depends on the video format.
    pub frames: u8,
}

impl Timecode {
    /// `00:00:00:00`.
    pub const ZERO: Self = Self {
        hours: 0,
        minutes: 0,
        seconds: 0,
        frames: 0,
    };

    /// Builds a timecode, rejecting out-of-range components.
    pub const fn new(hours: u8, minutes: u8, seconds: u8, frames: u8) -> Result<Self, Error> {
        if hours > 99 || minutes > 59 || seconds > 59 || frames > 99 {
            return Err(Error::InvalidValue);
        }
        Ok(Self {
            hours,
            minutes,
            seconds,
            frames,
        })
    }

    /// Parses `HH:MM:SS:FF`.
    pub fn parse(input: &str) -> Result<Self, Error> {
        let mut parts = input.split(':');
        let hours = parse_part(parts.next())?;
        let minutes = parse_part(parts.next())?;
        let seconds = parse_part(parts.next())?;
        let frames = parse_part(parts.next())?;
        if parts.next().is_some() {
            return Err(Error::Syntax);
        }
        Self::new(hours, minutes, seconds, frames)
    }

    /// Total frames at `fps`, which must be the integer frame rate (for example 30).
    pub fn to_frames(self, fps: u32) -> u32 {
        let seconds =
            u32::from(self.hours) * 3600 + u32::from(self.minutes) * 60 + u32::from(self.seconds);
        seconds * fps + u32::from(self.frames)
    }

    /// Inverse of [`Self::to_frames`]. Frame counts that overflow 99 hours saturate.
    pub fn from_frames(mut frames: u32, fps: u32) -> Self {
        let fps = fps.max(1);
        let ff = frames % fps;
        frames /= fps;
        let seconds = frames % 60;
        frames /= 60;
        let minutes = frames % 60;
        let hours = (frames / 60).min(99);
        Self {
            hours: hours as u8,
            minutes: minutes as u8,
            seconds: seconds as u8,
            frames: ff as u8,
        }
    }

    /// Writes `HH:MM:SS:FF`.
    pub fn write(self, out: &mut impl fmt::Write) -> fmt::Result {
        write!(
            out,
            "{:02}:{:02}:{:02}:{:02}",
            self.hours, self.minutes, self.seconds, self.frames
        )
    }
}

impl fmt::Display for Timecode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.write(f)
    }
}

/// An absolute timecode or a signed offset (`+HH:MM:SS:FF` / `-HH:MM:SS:FF`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TimecodeOffset {
    /// An absolute timeline or clip timecode.
    Absolute(Timecode),
    /// A duration added to or subtracted from the current position.
    Relative {
        /// `true` for `+`, `false` for `-`.
        forward: bool,
        /// The magnitude.
        timecode: Timecode,
    },
}

impl TimecodeOffset {
    /// Parses an absolute or relative timecode.
    pub fn parse(input: &str) -> Result<Self, Error> {
        if let Some(rest) = input.strip_prefix('+') {
            return Ok(Self::Relative {
                forward: true,
                timecode: Timecode::parse(rest)?,
            });
        }
        if let Some(rest) = input.strip_prefix('-') {
            return Ok(Self::Relative {
                forward: false,
                timecode: Timecode::parse(rest)?,
            });
        }
        Ok(Self::Absolute(Timecode::parse(input)?))
    }

    /// Writes the protocol spelling.
    pub fn write(self, out: &mut impl fmt::Write) -> fmt::Result {
        match self {
            Self::Absolute(tc) => tc.write(out),
            Self::Relative {
                forward: true,
                timecode,
            } => {
                out.write_char('+')?;
                timecode.write(out)
            }
            Self::Relative {
                forward: false,
                timecode,
            } => {
                out.write_char('-')?;
                timecode.write(out)
            }
        }
    }
}

fn parse_part(part: Option<&str>) -> Result<u8, Error> {
    let part = part.ok_or(Error::Syntax)?;
    if part.len() != 2 || !part.bytes().all(|b| b.is_ascii_digit()) {
        return Err(Error::Syntax);
    }
    part.parse().map_err(|_| Error::InvalidValue)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_formats() {
        let tc = Timecode::parse("01:02:03:04").unwrap();
        assert_eq!(tc.hours, 1);
        assert_eq!(tc.frames, 4);
        let mut s = heapless::String::<16>::new();
        tc.write(&mut s).unwrap();
        assert_eq!(s.as_str(), "01:02:03:04");
    }

    #[test]
    fn relative_offset() {
        let offset = TimecodeOffset::parse("+00:00:01:00").unwrap();
        assert_eq!(
            offset,
            TimecodeOffset::Relative {
                forward: true,
                timecode: Timecode::new(0, 0, 1, 0).unwrap(),
            }
        );
    }

    #[test]
    fn frame_round_trip() {
        let tc = Timecode::new(0, 1, 2, 3).unwrap();
        assert_eq!(Timecode::from_frames(tc.to_frames(30), 30), tc);
    }
}
