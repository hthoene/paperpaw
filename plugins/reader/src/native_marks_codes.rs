//! The look of a mark as `reader.decorations@1` carries it (`decorate.look`):
//! fixed codes, never the order of `Tone::ALL` or `LineKind::ALL`, so that a
//! later SDK may add tones without changing what a code means. The table is
//! the stage's contract (`PORTING-7.md`, the stage `reader.decorations@1`);
//! a subscriber writes the same codes.
use paperpaw_sdk::decorations::{LineKind, Tone};

/// Every tone with its code; 0 is no tone (no background, the text's own ink).
const TONES: [(u32, Tone); 11] = [
    (1, Tone::LightGray),
    (2, Tone::Gray),
    (3, Tone::DarkGray),
    (4, Tone::Black),
    (5, Tone::White),
    (6, Tone::Yellow),
    (7, Tone::Green),
    (8, Tone::Blue),
    (9, Tone::Pink),
    (10, Tone::Orange),
    (11, Tone::Red),
];

/// Every kind of line with its code; 0 is no line.
const LINES: [(u32, LineKind); 5] = [
    (1, LineKind::Solid),
    (2, LineKind::Dotted),
    (3, LineKind::Dashed),
    (4, LineKind::Wavy),
    (5, LineKind::Double),
];

/// The most a line or frame is thick, in tenths of a millimetre.
pub const MAX_THICKNESS: u32 = 20;
/// `flags` bit 0: a frame around the text.
pub const FRAME: u32 = 1;
/// `flags` bit 1: a line through the text.
pub const STRUCK: u32 = 1 << 1;
/// `flags` bit 2: the subscriber takes taps on the mark.
pub const TAPS: u32 = 1 << 2;

/// The tone of `code`: `None` for 0; a code a reader does not know draws in
/// gray, as `Tone::Other` does.
#[must_use]
pub fn tone(code: u32) -> Option<Tone> {
    if code == 0 {
        return None;
    }
    Some(
        TONES
            .iter()
            .find(|(c, _)| *c == code)
            .map_or(Tone::Gray, |(_, tone)| *tone),
    )
}

/// The code of `tone` (`None`: 0); a tone of a later build is gray's.
#[must_use]
pub fn tone_code(tone: Option<Tone>) -> u32 {
    tone.map_or(0, |tone| {
        TONES
            .iter()
            .find(|(_, t)| *t == tone)
            .map_or(2, |(code, _)| *code)
    })
}

/// The kind of line of `code`: `None` for 0; a code a reader does not know
/// draws solid.
#[must_use]
pub fn line(code: u32) -> Option<LineKind> {
    if code == 0 {
        return None;
    }
    Some(
        LINES
            .iter()
            .find(|(c, _)| *c == code)
            .map_or(LineKind::Solid, |(_, kind)| *kind),
    )
}

/// The code of `kind` (`None`: 0); a kind of a later build is solid's.
#[must_use]
pub fn line_code(kind: Option<LineKind>) -> u32 {
    kind.map_or(0, |kind| {
        LINES
            .iter()
            .find(|(_, k)| *k == kind)
            .map_or(1, |(code, _)| *code)
    })
}

/// Whether `role` may be the mark of subscriber `plugin`: `<plugin>.<name>`,
/// a name of at least one byte. Any other role is dropped (no plugin styles
/// or takes the taps of another's marks).
#[must_use]
pub fn role_of(plugin: &str, role: &str) -> bool {
    role.strip_prefix(plugin)
        .and_then(|rest| rest.strip_prefix('.'))
        .is_some_and(|name| !name.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_codes_are_fixed_and_go_both_ways() {
        assert_eq!(tone(0), None);
        assert_eq!(tone(1), Some(Tone::LightGray));
        assert_eq!(tone(6), Some(Tone::Yellow));
        assert_eq!(tone(11), Some(Tone::Red));
        // Unknown: gray, as a tone of a later build.
        assert_eq!(tone(99), Some(Tone::Gray));
        for (code, t) in TONES {
            assert_eq!(tone_code(Some(t)), code);
            assert_eq!(tone(code), Some(t));
        }
        assert_eq!(tone_code(None), 0);
        assert_eq!(line(0), None);
        assert_eq!(line(4), Some(LineKind::Wavy));
        assert_eq!(line(77), Some(LineKind::Solid));
        for (code, k) in LINES {
            assert_eq!(line_code(Some(k)), code);
        }
        assert_eq!(line_code(None), 0);
    }

    #[test]
    fn a_role_is_the_subscribers_own() {
        assert!(role_of("vocabulary", "vocabulary.learning"));
        assert!(!role_of("vocabulary", "vocabulary."));
        assert!(!role_of("vocabulary", "vocabulary"));
        assert!(!role_of("vocabulary", "vocabularyx.learning"));
        assert!(!role_of("aids", "vocabulary.learning"));
        assert!(!role_of("vocabulary", "core.highlight"));
    }
}
