//! Pages a plugin serves to a phone or computer on the local network
//! ([`Command::Serve`](crate::Command::Serve)): who is answered, and what
//! the requests and answers need.
//!
//! The address shown on the device (as a QR code, see
//! `ui::draw_address`) carries a key drawn
//! anew each time from the platform's random source ([`key`]); every
//! request must carry it (`?k=…`). A [`Guard`] checks it and counts wrong
//! keys: after a few of them no request is answered for a while, so the
//! key cannot be found by trying. Serve only while the screen that shows
//! the address is open.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::Instant;
use crate::random::Random;

/// The port of the device's cable link (USB): a server asked for it
/// ([`Command::serve`](crate::Command::serve)) is reached by the computer
/// at the other end of the cable, where the device has such a link, with
/// the same requests as over the network, and needs no network. Nobody
/// shows a key there, and who is at that end is not known: ask the reader
/// on the device before anything is written. PaperPaw opens the screen
/// bound to [`slots::CABLE`](crate::builtin::slots::CABLE) when a computer
/// speaks over the cable while nothing serves there.
pub const CABLE_PORT: u16 = 0;

/// Letters of a key: at least 64 bits of letters a person reads off the
/// screen and types without taking one for another
/// ([`Random::readable`]).
pub const KEY_LETTERS: usize = crate::random::readable_letters(64);
/// Wrong keys taken before requests are turned away.
pub const WRONG_KEYS: u32 = 5;
/// How long requests are turned away then, in milliseconds.
pub const LOCKED_FOR: u64 = 60_000;

/// A key for one time of serving from `random`, the platform's source;
/// `None` where it has none (no weaker key in its place).
pub fn key(random: &Random) -> Option<String> {
    random.readable(KEY_LETTERS)
}

/// Whether a request is answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Access {
    Granted,
    /// Without the key, or with a wrong one.
    Denied,
    /// Too many wrong keys were tried: no request is answered for now.
    Locked,
}

/// The key of a time of serving, and the wrong keys tried.
#[derive(Debug)]
pub struct Guard {
    key: String,
    wrong: u32,
    locked_until: Option<Instant>,
}

impl Guard {
    pub fn new(key: String) -> Self {
        Self {
            key,
            wrong: 0,
            locked_until: None,
        }
    }

    /// Whether a request carrying `given` (`None`: no key at all) is
    /// answered at `now`. After [`WRONG_KEYS`] wrong keys, none is for
    /// [`LOCKED_FOR`], not even one with the right key. Requests without
    /// a key (a browser asking for an icon) are turned away uncounted:
    /// they guess nothing.
    pub fn check(&mut self, given: Option<&str>, now: Instant) -> Access {
        if let Some(until) = self.locked_until {
            if now < until {
                return Access::Locked;
            }
            self.locked_until = None;
            self.wrong = 0;
        }
        match given {
            Some(given) if same(given, &self.key) => Access::Granted,
            Some(_) => {
                self.wrong += 1;
                if self.wrong >= WRONG_KEYS {
                    self.locked_until = Some(now.add_millis(LOCKED_FOR));
                }
                Access::Denied
            }
            None => Access::Denied,
        }
    }

    /// Whether the request for `target` (its path and query) is answered
    /// at `now`, by its key `k`.
    pub fn check_target(&mut self, target: &str, now: Instant) -> Access {
        let query = target.split_once('?').map_or("", |(_, query)| query);
        self.check(param(query, "k").as_deref(), now)
    }
}

/// Whether `a` and `b` are equal, compared in a time that does not tell
/// how much of them is.
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |differ, (x, y)| differ | (x ^ y))
            == 0
}

/// Parameter `name` of a query (`a=1&b=x%20y`), decoded.
pub fn param(query: &str, name: &str) -> Option<String> {
    query
        .split('&')
        .filter_map(|pair| pair.split_once('=').or(Some((pair, ""))))
        .find(|(key, _)| *key == name)
        .map(|(_, value)| decode(value))
}

/// Decodes `%XX` and `+` of a query value.
fn decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let hex = |b: u8| char::from(b).to_digit(16).map(|d| d as u8);
    let mut i = 0;
    while i < bytes.len() {
        let escaped = (bytes[i] == b'%')
            .then(|| Some((hex(*bytes.get(i + 1)?)?, hex(*bytes.get(i + 2)?)?)))
            .flatten();
        match (bytes[i], escaped) {
            (_, Some((high, low))) => {
                out.push(high << 4 | low);
                i += 3;
                continue;
            }
            (b'+', _) => out.push(b' '),
            (byte, _) => out.push(byte),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `text` as a JSON string, safe inside a page's `<script>` too.
pub fn json_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '<' => out.push_str("\\u003c"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_random_and_long() {
        // From the platform's source, 5 bits a letter.
        let random = Random::new(|buf| {
            for (i, byte) in buf.iter_mut().enumerate() {
                *byte = (i as u8).wrapping_mul(97).wrapping_add(13);
            }
            true
        });
        let key = key(&random).unwrap();
        assert_eq!(key.len(), KEY_LETTERS);
        assert!(
            key.bytes()
                .all(|b| b.is_ascii_digit() || b.is_ascii_lowercase())
        );
        // None where there is no source: no weaker key in its place.
        assert_eq!(super::key(&Random::NONE), None);
        assert_eq!(super::key(&Random::new(|_| false)), None);
    }

    #[test]
    fn wrong_keys_lock_for_a_while() {
        let mut guard = Guard::new("abc".into());
        assert_eq!(guard.check(None, Instant(0)), Access::Denied);
        assert_eq!(guard.check_target("/x?k=abc", Instant(0)), Access::Granted);
        for _ in 0..WRONG_KEYS {
            assert_eq!(guard.check(Some("guess"), Instant(1)), Access::Denied);
        }
        assert_eq!(guard.check(Some("abc"), Instant(2)), Access::Locked);
        let later = Instant(1 + LOCKED_FOR);
        assert_eq!(guard.check(Some("abc"), later), Access::Granted);
    }

    #[test]
    fn queries_are_decoded() {
        let query = "k=1&path=a%2Fb+c%C3%A4";
        assert_eq!(param(query, "path").as_deref(), Some("a/b cä"));
        assert_eq!(param("k", "k").as_deref(), Some(""));
        assert_eq!(param("path=%zz%4", "path").as_deref(), Some("%zz%4"));
        assert_eq!(param("k=1", "path"), None);
    }

    #[test]
    fn json_is_escaped() {
        // `<` escaped too, so that `</script>` cannot end the page's script.
        let escaped = |hex: &str| ["\\", "u", hex].concat();
        let expected = [
            "\"a\\\"b\\\\c",
            &escaped("003c"),
            "/",
            &escaped("000a"),
            "\"",
        ];
        assert_eq!(json_string("a\"b\\c</\n"), expected.concat());
    }
}
