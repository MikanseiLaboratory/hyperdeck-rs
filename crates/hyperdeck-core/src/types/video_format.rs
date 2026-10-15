//! Video format tokens shared by the Ethernet protocol and the REST API.
//!
//! Ethernet spells formats as `1080p2997` or `2160p59.94`. REST spells the same
//! raster as `1920x1080p29.97`. [`VideoFormat`] stores the raster once and
//! prints either spelling.

use core::fmt;

use crate::error::Error;

/// Progressive or interlaced scanning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Scan {
    /// Progressive (`p`).
    Progressive,
    /// Interlaced (`i`).
    Interlaced,
}

/// Frame rates that appear in the December 2024 HyperDeck manuals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FrameRate {
    /// 23.976 / 23.98 fps.
    F23_98,
    /// 24 fps.
    F24,
    /// 25 fps.
    F25,
    /// 29.97 fps.
    F29_97,
    /// 30 fps.
    F30,
    /// 50 fps.
    F50,
    /// 59.94 fps.
    F59_94,
    /// 60 fps.
    F60,
}

impl FrameRate {
    /// Integer frame count used to convert timecode to a frame index.
    ///
    /// Fractional rates use the rounded-up integer (`29.97` -> `30`), which is
    /// how timecode frame numbers are counted.
    pub const fn integer_fps(self) -> u32 {
        match self {
            Self::F23_98 | Self::F24 => 24,
            Self::F25 => 25,
            Self::F29_97 | Self::F30 => 30,
            Self::F50 => 50,
            Self::F59_94 | Self::F60 => 60,
        }
    }

    fn rest_token(self) -> &'static str {
        match self {
            Self::F23_98 => "23.98",
            Self::F24 => "24",
            Self::F25 => "25",
            Self::F29_97 => "29.97",
            Self::F30 => "30",
            Self::F50 => "50",
            Self::F59_94 => "59.94",
            Self::F60 => "60",
        }
    }
}

/// A playback or input video format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VideoFormat {
    /// Standard-definition formats that the protocol spells by name.
    Sd(SdSystem),
    /// A raster described by its dimensions, scan, and rate.
    Raster {
        /// Horizontal samples.
        width: u16,
        /// Vertical lines.
        height: u16,
        /// Progressive or interlaced.
        scan: Scan,
        /// Frame rate.
        rate: FrameRate,
    },
}

/// Named standard-definition systems.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SdSystem {
    /// Interlaced NTSC.
    Ntsc,
    /// Interlaced PAL.
    Pal,
    /// Progressive NTSC (`NTSCp`).
    NtscP,
    /// Progressive PAL (`PALp`).
    PalP,
}

impl VideoFormat {
    /// Parses an Ethernet token such as `1080p2997`, `2160p59.94`, or `NTSC`.
    pub fn parse_ethernet(input: &str) -> Result<Self, Error> {
        match input {
            "NTSC" => return Ok(Self::Sd(SdSystem::Ntsc)),
            "PAL" => return Ok(Self::Sd(SdSystem::Pal)),
            "NTSCp" => return Ok(Self::Sd(SdSystem::NtscP)),
            "PALp" => return Ok(Self::Sd(SdSystem::PalP)),
            _ => {}
        }
        parse_raster_token(input).ok_or(Error::InvalidValue)
    }

    /// Parses a REST name such as `1920x1080p29.97`.
    pub fn parse_rest(input: &str) -> Result<Self, Error> {
        let Some((dims, (scan, rate_token))) = split_scan(input) else {
            return Err(Error::InvalidValue);
        };
        let Some((width, height)) = dims.split_once('x') else {
            return Err(Error::InvalidValue);
        };
        let width: u16 = width.parse().map_err(|_| Error::InvalidValue)?;
        let height: u16 = height.parse().map_err(|_| Error::InvalidValue)?;
        let rate = parse_rate(rate_token).ok_or(Error::InvalidValue)?;
        Ok(Self::Raster {
            width,
            height,
            scan,
            rate,
        })
    }

    /// Writes the Ethernet token.
    pub fn write_ethernet(self, out: &mut impl fmt::Write) -> fmt::Result {
        match self {
            Self::Sd(SdSystem::Ntsc) => out.write_str("NTSC"),
            Self::Sd(SdSystem::Pal) => out.write_str("PAL"),
            Self::Sd(SdSystem::NtscP) => out.write_str("NTSCp"),
            Self::Sd(SdSystem::PalP) => out.write_str("PALp"),
            Self::Raster {
                width,
                height,
                scan,
                rate,
            } => write_ethernet_raster(out, width, height, scan, rate),
        }
    }

    /// Writes the REST name (`{width}x{height}{p|i}{rate}`).
    pub fn write_rest(self, out: &mut impl fmt::Write) -> fmt::Result {
        let (width, height, scan, rate) = match self {
            Self::Sd(SdSystem::Ntsc) => (720, 480, Scan::Interlaced, FrameRate::F29_97),
            Self::Sd(SdSystem::Pal) => (720, 576, Scan::Interlaced, FrameRate::F25),
            Self::Sd(SdSystem::NtscP) => (720, 480, Scan::Progressive, FrameRate::F59_94),
            Self::Sd(SdSystem::PalP) => (720, 576, Scan::Progressive, FrameRate::F50),
            Self::Raster {
                width,
                height,
                scan,
                rate,
            } => (width, height, scan, rate),
        };
        let scan_ch = match scan {
            Scan::Progressive => 'p',
            Scan::Interlaced => 'i',
        };
        write!(out, "{width}x{height}{scan_ch}{}", rate.rest_token())
    }
}

fn write_ethernet_raster(
    out: &mut impl fmt::Write,
    width: u16,
    height: u16,
    scan: Scan,
    rate: FrameRate,
) -> fmt::Result {
    let scan_ch = match scan {
        Scan::Progressive => 'p',
        Scan::Interlaced => 'i',
    };
    // DCI 4K/8K use the `4K` / `8K` prefix. UHD uses the vertical size.
    // 1080/720 and the DCI prefixes spell fractional rates without a dot
    // (`2997`). UHD (`2160p`, `4320p`) spells them with a dot (`29.97`).
    let dci = match (width, height) {
        (4096, 2160) => Some("4K"),
        (8192, 4320) => Some("8K"),
        _ => None,
    };
    let dotted = dci.is_none() && matches!(height, 2160 | 4320);
    let rate_token = ethernet_rate(rate, dotted);
    if let Some(prefix) = dci {
        write!(out, "{prefix}{scan_ch}{rate_token}")
    } else {
        write!(out, "{height}{scan_ch}{rate_token}")
    }
}

fn ethernet_rate(rate: FrameRate, dotted: bool) -> &'static str {
    match (rate, dotted) {
        (FrameRate::F23_98, false) => "23976",
        (FrameRate::F23_98, true) => "23.98",
        (FrameRate::F24, _) => "24",
        (FrameRate::F25, _) => "25",
        (FrameRate::F29_97, false) => "2997",
        (FrameRate::F29_97, true) => "29.97",
        (FrameRate::F30, _) => "30",
        (FrameRate::F50, _) => "50",
        (FrameRate::F59_94, false) => "5994",
        (FrameRate::F59_94, true) => "59.94",
        (FrameRate::F60, _) => "60",
    }
}

fn parse_raster_token(input: &str) -> Option<VideoFormat> {
    let (prefix, rest) = if let Some(rest) = input.strip_prefix("4K") {
        ("4K", rest)
    } else if let Some(rest) = input.strip_prefix("8K") {
        ("8K", rest)
    } else {
        ("", input)
    };
    let (_digits, (scan, rate_src)) = split_scan(rest)?;
    let rate = parse_rate(rate_src)?;
    let (width, height) = match prefix {
        "4K" => (4096, 2160),
        "8K" => (8192, 4320),
        _ => {
            let height: u16 = prefix_height(rest)?;
            let width = match height {
                720 => 1280,
                1080 => 1920,
                2160 => 3840,
                4320 => 7680,
                _ => return None,
            };
            (width, height)
        }
    };
    Some(VideoFormat::Raster {
        width,
        height,
        scan,
        rate,
    })
}

fn prefix_height(token: &str) -> Option<u16> {
    let mut buf = [0u8; 8];
    let mut len = 0;
    for byte in token.bytes() {
        if !byte.is_ascii_digit() {
            break;
        }
        if len >= buf.len() {
            return None;
        }
        buf[len] = byte;
        len += 1;
    }
    if len == 0 {
        return None;
    }
    core::str::from_utf8(&buf[..len]).ok()?.parse().ok()
}

fn split_scan(input: &str) -> Option<(&str, (Scan, &str))> {
    if let Some((head, tail)) = input.split_once('p') {
        return Some((head, (Scan::Progressive, tail)));
    }
    if let Some((head, tail)) = input.split_once('i') {
        return Some((head, (Scan::Interlaced, tail)));
    }
    None
}

fn parse_rate(token: &str) -> Option<FrameRate> {
    Some(match token {
        "23976" | "23.98" | "23.976" => FrameRate::F23_98,
        "24" | "24.00" => FrameRate::F24,
        "25" | "25.00" => FrameRate::F25,
        "2997" | "29.97" => FrameRate::F29_97,
        "30" | "30.00" => FrameRate::F30,
        "50" | "50.00" => FrameRate::F50,
        "5994" | "59.94" => FrameRate::F59_94,
        "60" | "60.00" => FrameRate::F60,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eth(format: VideoFormat) -> heapless::String<32> {
        let mut out = heapless::String::new();
        format.write_ethernet(&mut out).unwrap();
        out
    }

    fn rest(format: VideoFormat) -> heapless::String<32> {
        let mut out = heapless::String::new();
        format.write_rest(&mut out).unwrap();
        out
    }

    #[test]
    fn ethernet_and_rest_round_trip() {
        let format = VideoFormat::parse_ethernet("1080p2997").unwrap();
        assert_eq!(eth(format).as_str(), "1080p2997");
        assert_eq!(rest(format).as_str(), "1920x1080p29.97");
        assert_eq!(VideoFormat::parse_rest("1920x1080p29.97").unwrap(), format);

        let uhd = VideoFormat::parse_ethernet("2160p59.94").unwrap();
        assert_eq!(rest(uhd).as_str(), "3840x2160p59.94");
        assert_eq!(eth(uhd).as_str(), "2160p59.94");

        let dci = VideoFormat::parse_ethernet("4Kp5994").unwrap();
        assert_eq!(rest(dci).as_str(), "4096x2160p59.94");
        assert_eq!(eth(dci).as_str(), "4Kp5994");

        let interlaced = VideoFormat::parse_ethernet("1080i50").unwrap();
        assert_eq!(rest(interlaced).as_str(), "1920x1080i50");
    }

    #[test]
    fn named_sd_formats() {
        assert_eq!(
            VideoFormat::parse_ethernet("NTSC").unwrap(),
            VideoFormat::Sd(SdSystem::Ntsc)
        );
        assert_eq!(rest(VideoFormat::Sd(SdSystem::Pal)).as_str(), "720x576i25");
    }
}
