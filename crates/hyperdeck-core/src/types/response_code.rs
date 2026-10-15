//! Three-digit status codes from the Ethernet protocol.

use core::fmt;

/// A protocol response code.
///
/// Known codes from the December 2024 specification have associated constants.
/// Firmware that adds a code is represented by [`ResponseCode::new`] and
/// classified by its numeric range.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResponseCode(u16);

impl ResponseCode {
    /// `100 syntax error`
    pub const SYNTAX: Self = Self(100);
    /// `101 unsupported parameter`
    pub const UNSUPPORTED_PARAMETER: Self = Self(101);
    /// `102 invalid value`
    pub const INVALID_VALUE: Self = Self(102);
    /// `103 unsupported`
    pub const UNSUPPORTED: Self = Self(103);
    /// `104 disk full`
    pub const DISK_FULL: Self = Self(104);
    /// `105 no disk`
    pub const NO_DISK: Self = Self(105);
    /// `106 disk error`
    pub const DISK_ERROR: Self = Self(106);
    /// `107 timeline empty`
    pub const TIMELINE_EMPTY: Self = Self(107);
    /// `108 internal error`
    pub const INTERNAL: Self = Self(108);
    /// `109 out of range`
    pub const OUT_OF_RANGE: Self = Self(109);
    /// `110 no input`
    pub const NO_INPUT: Self = Self(110);
    /// `111 remote control disabled`
    pub const REMOTE_DISABLED: Self = Self(111);
    /// `112 clip not found`
    pub const CLIP_NOT_FOUND: Self = Self(112);
    /// `120 connection failed`
    pub const CONNECTION_FAILED: Self = Self(120);
    /// `121 authentication failed`
    pub const AUTHENTICATION_FAILED: Self = Self(121);
    /// `122 authentication required`
    pub const AUTHENTICATION_REQUIRED: Self = Self(122);
    /// `150 invalid state`
    pub const INVALID_STATE: Self = Self(150);
    /// `151 invalid codec`
    pub const INVALID_CODEC: Self = Self(151);
    /// `160 invalid format`
    pub const INVALID_FORMAT: Self = Self(160);
    /// `161 invalid token`
    pub const INVALID_TOKEN: Self = Self(161);
    /// `162 format not prepared`
    pub const FORMAT_NOT_PREPARED: Self = Self(162);
    /// `163 parameterized single line command not supported`
    pub const SINGLE_LINE_UNSUPPORTED: Self = Self(163);

    /// `200 ok`
    pub const OK: Self = Self(200);
    /// `201 help`
    pub const HELP: Self = Self(201);
    /// `202 slot info`
    pub const SLOT_INFO: Self = Self(202);
    /// `204 device info`
    pub const DEVICE_INFO: Self = Self(204);
    /// `205 clips info`
    pub const CLIPS_INFO: Self = Self(205);
    /// `206 disk list`
    pub const DISK_LIST: Self = Self(206);
    /// `208 transport info`
    pub const TRANSPORT_INFO: Self = Self(208);
    /// `209 notify`
    pub const NOTIFY: Self = Self(209);
    /// `210 remote info`
    pub const REMOTE_INFO: Self = Self(210);
    /// `211 configuration`
    pub const CONFIGURATION: Self = Self(211);
    /// `212 commands`
    pub const COMMANDS: Self = Self(212);
    /// `213 deck rebooting`
    pub const DECK_REBOOTING: Self = Self(213);
    /// `214 clips count`
    pub const CLIPS_COUNT: Self = Self(214);
    /// `225 nas host info`
    pub const NAS_HOST_INFO: Self = Self(225);
    /// `226 external drive info`
    pub const EXTERNAL_DRIVE_INFO: Self = Self(226);

    /// `500 connection info`
    pub const CONNECTION_INFO: Self = Self(500);
    /// `502 slot info`
    pub const ASYNC_SLOT_INFO: Self = Self(502);
    /// `508 transport info`
    pub const ASYNC_TRANSPORT_INFO: Self = Self(508);
    /// `510 remote info`
    pub const ASYNC_REMOTE_INFO: Self = Self(510);
    /// `511 configuration`
    pub const ASYNC_CONFIGURATION: Self = Self(511);

    /// Wraps any three-digit code.
    pub const fn new(code: u16) -> Self {
        Self(code)
    }

    /// The numeric code.
    pub const fn as_u16(self) -> u16 {
        self.0
    }

    /// Failure, success, or asynchronous class derived from the number.
    pub const fn class(self) -> CodeClass {
        match self.0 {
            100..=199 => CodeClass::Failure,
            200..=299 => CodeClass::Success,
            500..=599 => CodeClass::Async,
            _ => CodeClass::Unknown,
        }
    }

    /// Canonical reason text from the December 2024 specification, when known.
    pub const fn canonical_text(self) -> Option<&'static str> {
        Some(match self.0 {
            100 => "syntax error",
            101 => "unsupported parameter",
            102 => "invalid value",
            103 => "unsupported",
            104 => "disk full",
            105 => "no disk",
            106 => "disk error",
            107 => "timeline empty",
            108 => "internal error",
            109 => "out of range",
            110 => "no input",
            111 => "remote control disabled",
            112 => "clip not found",
            120 => "connection failed",
            121 => "authentication failed",
            122 => "authentication required",
            150 => "invalid state",
            151 => "invalid codec",
            160 => "invalid format",
            161 => "invalid token",
            162 => "format not prepared",
            163 => "parameterized single line command not supported",
            200 => "ok",
            201 => "help",
            202 => "slot info",
            204 => "device info",
            205 => "clips info",
            206 => "disk list",
            208 => "transport info",
            209 => "notify",
            210 => "remote info",
            211 => "configuration",
            212 => "commands",
            213 => "deck rebooting",
            214 => "clips count",
            225 => "nas host info",
            226 => "external drive info",
            500 => "connection info",
            502 => "slot info",
            508 => "transport info",
            510 => "remote info",
            511 => "configuration",
            _ => return None,
        })
    }
}

impl fmt::Debug for ResponseCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ResponseCode({})", self.0)
    }
}

impl fmt::Display for ResponseCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<u16> for ResponseCode {
    fn from(value: u16) -> Self {
        Self(value)
    }
}

/// The three ranges the specification assigns to response codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CodeClass {
    /// `100..=199`, a failed command.
    Failure,
    /// `200..=299`, a completed command.
    Success,
    /// `500..=599`, an unsolicited notification.
    Async,
    /// A code outside the ranges defined in the specification.
    Unknown,
}
