//! `name: value` pairs, including values that contain spaces or colons.

use heapless::Vec;

/// Parameter names understood by the December 2024 specification.
///
/// Unknown names are still accepted: the scanner falls back to the text that
/// precedes the next colon. Longer names are matched in preference to shorter
/// ones that share a starting position (`timeline in` wins over `in`).
const KEYS: &[&str] = &[
    "genlock input resync",
    "audio input channels",
    "timecode preference",
    "connection protocol",
    "camera operator",
    "response version",
    "append timestamp",
    "reference source",
    "dropped frames",
    "display timecode",
    "timeline position",
    "dynamic range",
    "default standard",
    "record trigger",
    "record prefix",
    "timecode input",
    "timecode output",
    "timecode preset",
    "video format",
    "video input",
    "audio input",
    "audio codec",
    "file format",
    "single clip",
    "stop mode",
    "timeline in",
    "timeline out",
    "take scenario",
    "take auto inc",
    "project name",
    "focal length",
    "xlr input id",
    "xlr mapping",
    "rca mapping",
    "audio mapping",
    "device info",
    "record cache",
    "usb spill",
    "playrange",
    "configuration",
    "frame in",
    "frame out",
    "shot type",
    "good take",
    "scene id",
    "day night",
    "lens type",
    "slot id",
    "clip id",
    "username",
    "password",
    "environment",
    "override",
    "prepare",
    "confirm",
    "period",
    "enable",
    "device",
    "remote",
    "transport",
    "version",
    "timeline",
    "timecode",
    "preview",
    "format",
    "count",
    "speed",
    "loop",
    "name",
    "slot",
    "clip",
    "cache",
    "slate",
    "clips",
    "disk",
    "nas",
    "url",
    "reel",
    "take",
    "iris",
    "filter",
    "camera",
    "director",
    "distance",
    "in",
    "out",
];

/// Parses inline `key: value` pairs from the text that follows a command name.
pub fn parse_inline_params<const N: usize>(
    input: &str,
) -> Result<Vec<(&str, &str), N>, crate::Error> {
    let mut out = Vec::new();
    let mut rest = input.trim();
    while !rest.is_empty() {
        let Some((idx, key_end)) = find_key(rest) else {
            break;
        };
        if idx != 0 {
            // Text before the first key is not a parameter.
            break;
        }
        let key = &rest[..key_end];
        let after_colon = key_end + 1;
        let value_start = if rest.as_bytes().get(after_colon) == Some(&b' ') {
            after_colon + 1
        } else {
            after_colon
        };
        let value_end = find_terminator(&rest[value_start..])
            .map(|(next, _)| value_start + next)
            .unwrap_or(rest.len());
        let mut end = value_end;
        if end > value_start && rest.as_bytes().get(end - 1) == Some(&b' ') {
            end -= 1;
        }
        let value = rest[value_start..end].trim();
        out.push((key, value))
            .map_err(|_| crate::Error::BufferFull)?;
        rest = rest[value_end..].trim_start();
    }
    Ok(out)
}

/// Parses one `key: value` line. The value may contain colons (timecode).
pub fn parse_param_line(line: &str) -> Option<(&str, &str)> {
    let (key, value) = line.split_once(':')?;
    let key = key.trim();
    if key.is_empty() {
        return None;
    }
    Some((key, value.trim()))
}

/// Returns the byte range of the earliest parameter name.
///
/// Known names win over a shorter name at the same position. A single unknown
/// token ending in `:` is accepted so firmware can add parameters without
/// breaking the parser.
fn find_known(input: &str) -> Option<(usize, usize)> {
    let mut best: Option<(usize, usize)> = None;
    for key in KEYS {
        if let Some(idx) = locate(input, key) {
            let end = idx + key.len();
            if better((idx, end), best) {
                best = Some((idx, end));
            }
        }
    }
    best
}

fn find_key(input: &str) -> Option<(usize, usize)> {
    let mut best = find_known(input);
    if let Some(unknown) = find_unknown_key(input)
        && better(unknown, best)
    {
        best = Some(unknown);
    }
    best
}

/// End of a parameter value.
///
/// Unknown keys at index 0 are ignored so values such as `smb://...` and
/// `01:02:03:04` stay intact. A later `custom:` token still ends the value.
fn find_terminator(input: &str) -> Option<(usize, usize)> {
    let known = find_known(input);
    match find_unknown_key(input) {
        Some((idx, end)) if idx > 0 && better((idx, end), known) => Some((idx, end)),
        _ => known,
    }
}

/// A firmware-added parameter: a single alphabetic token followed by `:`.
///
/// Numeric tokens are ignored so a timecode value (`01:02:03:04`) is not split
/// into parameters.
fn find_unknown_key(input: &str) -> Option<(usize, usize)> {
    let bytes = input.as_bytes();
    let mut best: Option<(usize, usize)> = None;
    for (index, byte) in bytes.iter().enumerate() {
        if *byte != b':' {
            continue;
        }
        let token_start = input[..index].rfind(' ').map(|pos| pos + 1).unwrap_or(0);
        let token = &input[token_start..index];
        if !plausible_key(token) {
            continue;
        }
        let candidate = (token_start, index);
        if better(candidate, best) {
            best = Some(candidate);
        }
    }
    best
}

fn plausible_key(token: &str) -> bool {
    !token.is_empty()
        && !token.contains(' ')
        && token.bytes().any(|byte| byte.is_ascii_alphabetic())
}

fn better(candidate: (usize, usize), best: Option<(usize, usize)>) -> bool {
    match best {
        None => true,
        Some((best_idx, best_end)) => {
            let (idx, end) = candidate;
            idx < best_idx || (idx == best_idx && end - idx > best_end - best_idx)
        }
    }
}

fn locate(input: &str, key: &str) -> Option<usize> {
    let mut start = 0;
    while let Some(rel) = input[start..].find(key) {
        let idx = start + rel;
        let boundary = idx == 0 || input.as_bytes()[idx - 1] == b' ';
        let after = idx + key.len();
        if boundary && input.as_bytes().get(after) == Some(&b':') {
            return Some(idx);
        }
        start = idx + key.len();
        if start > input.len() {
            break;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combined_play_parameters() {
        let pairs: Vec<(&str, &str), 8> =
            parse_inline_params("clip id: 3 speed: 200 loop: true single clip: true").unwrap();
        assert_eq!(
            pairs.as_slice(),
            &[
                ("clip id", "3"),
                ("speed", "200"),
                ("loop", "true"),
                ("single clip", "true"),
            ]
        );
    }

    #[test]
    fn timecode_value_keeps_colons() {
        let pairs: Vec<(&str, &str), 4> =
            parse_inline_params("in: 01:02:03:04 out: 01:02:13:04").unwrap();
        assert_eq!(pairs[0], ("in", "01:02:03:04"));
        assert_eq!(pairs[1], ("out", "01:02:13:04"));
    }

    #[test]
    fn filename_may_contain_spaces() {
        let pairs: Vec<(&str, &str), 2> = parse_inline_params("name: My Clip 01.mov").unwrap();
        assert_eq!(pairs[0], ("name", "My Clip 01.mov"));
    }

    #[test]
    fn url_value_keeps_scheme() {
        let pairs: Vec<(&str, &str), 4> =
            parse_inline_params("url: smb://192.168.1.1/Main username: user1234").unwrap();
        assert_eq!(pairs[0], ("url", "smb://192.168.1.1/Main"));
        assert_eq!(pairs[1], ("username", "user1234"));
    }
}
