//! What the native progress sync decides and writes without host calls:
//! the server address, the account key, the progress body sent, the server's
//! answers read, and what a fetched position means (offer it, hold back, or
//! nothing to do). Each part is the interface-6 code made fallible, with a
//! test that both give the same answers.
//!
//! The server's JSON is read with `paperpaw_sdk::json` until Kernel's
//! fallible JSON reader (#414) is on the integration branch: that reader
//! allocates without a refusal path, so the answer's text is bounded by the
//! network host's reply cap, not by this crate (`PORTING-7.md`).
use alloc::string::String;
use paperpaw_sdk::{digest::md5::Md5, json, native::Error};
use paperpaw_ui_kit::{
    mem::{wipe, wipe_text},
    text::{concat, copy, decimal},
};

pub use crate::declarations::DEFAULT_SERVER;

/// The most bytes of a server's answer to a progress fetch that are read: the
/// request's `max_body` (the answer is a few hundred bytes; the network host's
/// default would be 45,056 on the X4 and 1,048,576 elsewhere). A longer text is
/// refused with `NoSpace`, so the interim JSON reader (no refusal path) never
/// meets more than this.
pub const MAX_ANSWER: usize = 4096;

/// Another device counts as further from this much more progress (0..1).
pub const FURTHER: f64 = 0.002;

/// A server address as entered, as a base URL: spaces around and a `/` at
/// the end left out, `https://` put first unless a scheme is given, KOReader's
/// server if nothing is entered.
///
/// # Errors
/// `NoMemory` when the address cannot be kept.
pub fn normalize_server(text: &str) -> Result<String, Error> {
    let text = text.trim().trim_end_matches('/');
    let scheme = text.split_once("://").is_some_and(|(scheme, _)| {
        scheme.starts_with(|c: char| c.is_ascii_alphabetic())
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    });
    if text.is_empty() {
        copy(DEFAULT_SERVER)
    } else if scheme {
        copy(text)
    } else {
        concat(&["https://", text])
    }
}

/// A base URL as shown: without `https://` (but with `http://`, which is
/// worth knowing).
#[must_use]
pub fn server_label(url: &str) -> &str {
    url.strip_prefix("https://").unwrap_or(url)
}

/// The translation key telling why a request with HTTP status `status`
/// failed (0: no answer).
#[must_use]
pub const fn failure(status: u32) -> &'static str {
    match status {
        0 => "kosync.unreachable",
        401 | 403 => "kosync.wrong",
        _ => "kosync.server-error",
    }
}

/// The key the server is sent for a password: its MD5 as hex, as KOReader
/// does. The password itself is never sent or kept.
///
/// # Errors
/// `NoMemory` when the key cannot be kept.
pub fn password_key(password: &str) -> Result<Secret, Error> {
    let mut digits = [0; 32];
    key_with(password, &mut digits)
}

/// [`password_key`] with the hex digits kept in `digits`, which are zeroed
/// before it returns, so the key is left only in the returned text.
///
/// The MD5 state (it holds the password's last block) wipes itself when
/// the digest is dropped (`paperpaw_digest`'s `Drop`, #451).
fn key_with(password: &str, digits: &mut [u8; 32]) -> Result<Secret, Error> {
    let mut md5 = Md5::new();
    md5.update(password.as_bytes());
    let key = Secret::copy(md5.hex_into(digits));
    wipe(digits);
    key
}

/// A text that is a secret (the account's key, a typed text): zeroed when it
/// is dropped, also on an early return, and never shown by `Debug`.
#[derive(Default)]
pub struct Secret(String);

impl Secret {
    /// Takes `text` over.
    #[must_use]
    pub const fn new(text: String) -> Self {
        Self(text)
    }

    /// A copy of `text`.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn copy(text: &str) -> Result<Self, Error> {
        copy(text).map(Self)
    }

    /// The text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        wipe_text(core::mem::take(&mut self.0));
    }
}

impl core::fmt::Debug for Secret {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("Secret(..)")
    }
}

/// A progress as text, as `{progress}` writes it, for the key of a position
/// offered or dismissed. A value whose plain text would be longer than 40 bytes
/// (tiny numbers) is written in scientific form, which is short.
///
/// # Errors
/// `NoMemory`.
pub fn progress_text(progress: f64) -> Result<String, Error> {
    use core::fmt::Write;
    struct Bounded {
        bytes: [u8; 40],
        len: usize,
    }
    impl Write for Bounded {
        fn write_str(&mut self, text: &str) -> core::fmt::Result {
            let end = self.len + text.len();
            self.bytes
                .get_mut(self.len..end)
                .ok_or(core::fmt::Error)?
                .copy_from_slice(text.as_bytes());
            self.len = end;
            Ok(())
        }
    }
    let mut out = Bounded {
        bytes: [0; 40],
        len: 0,
    };
    if write!(out, "{progress}").is_err() {
        out = Bounded {
            bytes: [0; 40],
            len: 0,
        };
        // Cannot fail: the scientific form of an `f64` is at most 24 bytes.
        write!(out, "{progress:e}").ok();
    }
    let text = out
        .bytes
        .get(..out.len)
        .and_then(|b| core::str::from_utf8(b).ok())
        .unwrap_or("0");
    copy(text)
}

/// A progress (0..1) as a whole percent, as `{:.0}` of the progress times 100
/// writes it (a tie goes to the even number), for the offer's text.
///
/// # Errors
/// `NoMemory`.
pub fn percent_text(progress: f64) -> Result<String, Error> {
    let x = if progress.is_finite() {
        progress * 100.0
    } else {
        0.0
    };
    let x = x.clamp(0.0, 999_999.0);
    let floor = x - x % 1.0;
    let rest = x - floor;
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped above"
    )]
    let floor = floor as u32;
    let whole = if rest > 0.5 || (rest == 0.5 && floor % 2 == 1) {
        floor + 1
    } else {
        floor
    };
    let mut digits = [0; 20];
    copy(decimal(whole as usize, &mut digits))
}

/// `text` as a JSON string literal.
///
/// # Errors
/// `NoMemory` when the literal cannot be kept.
pub fn json_string(text: &str) -> Result<String, Error> {
    let mut out = String::new();
    write_json_string(&mut out, text)?;
    Ok(out)
}

/// The bytes `c` takes as part of a JSON string literal.
const fn json_len(c: char) -> usize {
    match c {
        '"' | '\\' | '\n' | '\r' | '\t' => 2,
        c if (c as u32) < 0x20 => 6,
        c => c.len_utf8(),
    }
}

fn write_json_string(out: &mut String, text: &str) -> Result<(), Error> {
    // All of it is reserved at once: the string never grows, so no earlier,
    // smaller copy of a secret is left behind in the heap by a move.
    let total = text
        .chars()
        .fold(2usize, |total, c| total.saturating_add(json_len(c)));
    out.try_reserve_exact(total).map_err(|_| Error::NoMemory)?;
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                const HEX: &[u8; 16] = b"0123456789abcdef";
                out.push_str("\\u00");
                out.push(char::from(HEX[(c as usize >> 4) & 15]));
                out.push(char::from(HEX[c as usize & 15]));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    Ok(())
}

/// `progress` (0..1) as the body writes it: four decimals, a tie rounding to
/// the even digit as `{:.4}` does. Out of range or not a number is cut to 0..1.
#[must_use]
pub fn four_decimals(progress: f64, digits: &mut [u8; 20]) -> (u32, &str) {
    let progress = if progress.is_nan() {
        0.0
    } else {
        progress.clamp(0.0, 1.0)
    };
    let value = if progress < 0.00004 {
        // Below a hundredth of the last digit: 0, however it is rounded.
        0
    } else {
        // Exact: the value is `mantissa / 2^shift` (at least 4e-5, so the shift
        // is at most 67 and `mantissa * 10_000` fits 128 bits); no multiplying
        // in floating point, which would turn 0.12345 into an exact tie.
        let bits = progress.to_bits();
        let mantissa = u128::from((bits & ((1 << 52) - 1)) | (1 << 52));
        let shift = 1075 - u32::try_from(bits >> 52).unwrap_or(1075);
        let scaled = mantissa * 10_000;
        let floor = scaled >> shift;
        let rest = scaled & ((1u128 << shift) - 1);
        let half = 1u128 << (shift - 1);
        let up = rest > half || (rest == half && floor & 1 == 1);
        // 0 ..= 10_000 by the cut above.
        u32::try_from(floor + u128::from(up)).unwrap_or(10_000)
    };
    (value / 10_000, decimal((value % 10_000) as usize, digits))
}

/// The body of a progress update: the book's digest, the position as
/// KOReader's XPointer, the progress, this device's name and id.
///
/// # Errors
/// `NoMemory` when the body cannot be kept.
pub fn progress_body(
    digest: &str,
    xpointer: &str,
    progress: f64,
    device: &str,
    device_id: &str,
) -> Result<String, Error> {
    let (digest, xpointer) = (json_string(digest)?, json_string(xpointer)?);
    let (device, device_id) = (json_string(device)?, json_string(device_id)?);
    let mut digits = [0; 20];
    let (whole, fraction) = four_decimals(progress, &mut digits);
    let whole = if whole == 0 { "0" } else { "1" };
    let pad = "0000"
        .get(..4usize.saturating_sub(fraction.len()))
        .unwrap_or_default();
    concat(&[
        "{\"document\":",
        &digest,
        ",\"progress\":",
        &xpointer,
        ",\"percentage\":",
        whole,
        ".",
        pad,
        fraction,
        ",\"device\":",
        &device,
        ",\"device_id\":",
        &device_id,
        "}",
    ])
}

/// What a server's answer to a progress fetch says.
#[derive(Debug, Clone, PartialEq)]
pub struct Remote {
    /// Progress from 0 to 1; 0 for a book the server has none of.
    pub progress: f64,
    /// The device that sent it, if the answer names one.
    pub device_id: Option<String>,
    /// That device's name.
    pub device: Option<String>,
    /// The position as KOReader's XPointer (may be empty).
    pub xpointer: Option<String>,
}

/// The progress (0..1) in a sync server's answer: 0 for a book it has none
/// of (an object without one, e.g. `{}`); `None` if the answer is no object
/// or its progress no number from 0 to 1.
///
/// # Errors
/// `NoSpace` for an answer longer than [`MAX_ANSWER`].
pub fn remote_progress(text: &str) -> Result<Option<f64>, Error> {
    if text.len() > MAX_ANSWER {
        return Err(Error::NoSpace);
    }
    Ok(read_remote(text, &json::object(text)))
}

fn read_remote(text: &str, body: &[(String, json::Value)]) -> Option<f64> {
    if body.is_empty() {
        let inner = text
            .trim()
            .strip_prefix('{')
            .and_then(|t| t.strip_suffix('}'));
        return inner.is_some_and(|i| i.trim().is_empty()).then_some(0.0);
    }
    if !body.iter().any(|(key, _)| key == "percentage") {
        return Some(0.0);
    }
    let progress = json::number(body, "percentage")
        .or_else(|| json::text(body, "percentage").and_then(|p| p.trim().parse().ok()))?;
    (0.0..=1.0).contains(&progress).then_some(progress)
}

/// Reads a server's answer to a progress fetch.
///
/// # Errors
/// `NoSpace` for an answer longer than [`MAX_ANSWER`], `NoMemory` when a
/// text cannot be kept.
pub fn read_answer(text: &str) -> Result<Option<Remote>, Error> {
    if text.len() > MAX_ANSWER {
        return Err(Error::NoSpace);
    }
    let body = json::object(text);
    let Some(progress) = read_remote(text, &body) else {
        return Ok(None);
    };
    let take = |key: &str| json::text(&body, key).map(copy).transpose();
    Ok(Some(Remote {
        progress,
        device_id: take("device_id")?,
        device: take("device")?,
        xpointer: take("progress")?,
    }))
}

/// What a fetched position means for the book open here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Another device is further: offer to go there, send nothing until the
    /// user chose.
    Offer,
    /// Further on another device and the user said *not now*: keep sending
    /// held back, without asking.
    HoldQuietly,
    /// Nothing further elsewhere: this device's position may be sent.
    UpToDate,
}

/// What the answer to a fetch means.
#[derive(Debug, Clone, Copy)]
pub struct Seen {
    /// The server's progress (0..1).
    pub remote: f64,
    /// This device's progress (0..1).
    pub local: f64,
    /// The server's position was sent by this device.
    pub sent_from_here: bool,
    /// The user declined this very position before.
    pub declined: bool,
    /// The user said *not now* to this very position.
    pub dismissed: bool,
    /// The user asked for the sync (*Sync progress*).
    pub manual: bool,
}

/// Decides what to do with a fetched position.
#[must_use]
pub fn judge(seen: Seen) -> Verdict {
    // Sent from here, unless the book starts here anew: copied again, say,
    // its position lost.
    let from_here = seen.sent_from_here && seen.local > FURTHER;
    let further = !from_here && seen.remote > seen.local + FURTHER;
    if further && seen.dismissed && !seen.manual {
        Verdict::HoldQuietly
    } else if further && (seen.manual || !seen.declined) {
        Verdict::Offer
    } else {
        Verdict::UpToDate
    }
}

/// Whether the position held back for a book left meanwhile is sent: the
/// server's answer was 200 with a progress, and it is not further than ours
/// (or it came from here and ours has moved).
#[must_use]
pub fn send_left_behind(
    status: u32,
    remote: Option<f64>,
    sent_from_here: bool,
    progress: f64,
) -> bool {
    status == 200
        && remote.is_some_and(|remote| {
            (sent_from_here && progress > FURTHER) || remote <= progress + FURTHER
        })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "fixtures")]
    use super::*;
    use alloc::format;

    #[test]
    fn server_addresses_are_made_base_urls() {
        for (text, want) in [
            ("", DEFAULT_SERVER),
            ("  ", DEFAULT_SERVER),
            ("sync.example.org", "https://sync.example.org"),
            ("sync.example.org/", "https://sync.example.org"),
            ("  https://sync.example.org// ", "https://sync.example.org"),
            ("http://192.168.1.5:8080/", "http://192.168.1.5:8080"),
            ("webdavs://x/y", "webdavs://x/y"),
            ("ftp", "https://ftp"),
            ("1ab://x", "https://1ab://x"),
            ("a b", "https://a b"),
            ("://x", "https://://x"),
        ] {
            assert_eq!(normalize_server(text).unwrap(), want, "{text:?}");
        }
        assert_eq!(server_label("https://a.b"), "a.b");
        assert_eq!(server_label("http://a.b"), "http://a.b");
    }

    #[test]
    fn failures_name_the_texts_shown() {
        for (status, want) in [
            (0u32, "kosync.unreachable"),
            (200, "kosync.server-error"),
            (400, "kosync.server-error"),
            (401, "kosync.wrong"),
            (403, "kosync.wrong"),
            (404, "kosync.server-error"),
            (500, "kosync.server-error"),
            (502, "kosync.server-error"),
        ] {
            assert_eq!(failure(status), want, "{status}");
        }
    }

    #[test]
    fn a_secret_is_never_shown() {
        let key = Secret::copy("900150983cd24fb0d6963f7d28e17f72").unwrap();
        assert_eq!(key.as_str(), "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(format!("{key:?}"), "Secret(..)");
        let held = Some(password_key("abc").unwrap());
        assert!(!format!("{held:?}").contains("9001"));
    }

    #[test]
    fn the_hex_digits_are_zeroed_once_the_key_is_made() {
        let mut digits = [0xAA; 32];
        let key = key_with("abc", &mut digits).unwrap();
        assert_eq!(key.as_str(), "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(digits, [0; 32]);
    }

    #[test]
    fn percent_is_written_as_the_format_macro_rounds_it() {
        for progress in [
            0.0, 0.004, 0.005, 0.0125, 0.125, 0.135, 0.5, 0.505, 0.994, 0.995, 1.0, 0.3333, 0.665,
        ] {
            assert_eq!(
                percent_text(progress).unwrap(),
                format!("{:.0}", progress * 100.0),
                "{progress}"
            );
        }
        assert_eq!(percent_text(f64::NAN).unwrap(), "0");
    }

    #[test]
    fn the_password_key_is_its_md5() {
        for text in ["", "secret", "pässwörd", "a very long password indeed"] {
            assert_eq!(
                password_key(text).unwrap().as_str(),
                paperpaw_sdk::digest::md5::of(text)
            );
        }
    }

    #[test]
    fn json_strings_are_written_as_before() {
        for text in [
            "",
            "plain",
            "q\"uote",
            "back\\slash",
            "a\nb\rc\td",
            "\u{1}\u{1f}",
            "ünï 😀",
        ] {
            assert_eq!(json_string(text).unwrap(), json::string(text), "{text:?}");
        }
    }

    #[test]
    fn the_body_is_what_interface_6_wrote() {
        for progress in [0.0, 0.0001, 0.03125, 0.5, 0.12345, 0.99995, 1.0] {
            let old = format!(
                "{{\"document\":{},\"progress\":{},\"percentage\":{:.4},\"device\":{},\"device_id\":{}}}",
                json::string("d41d8cd9"),
                json::string("/body/DocFragment[3]/text().10"),
                progress,
                json::string("My \"Reader\""),
                json::string("ID-1"),
            );
            let new = progress_body(
                "d41d8cd9",
                "/body/DocFragment[3]/text().10",
                progress,
                "My \"Reader\"",
                "ID-1",
            )
            .unwrap();
            assert_eq!(new, old, "{progress}");
        }
    }

    #[test]
    fn four_decimals_match_the_formatter_on_many_values_and_ties() {
        let mut digits = [0; 20];
        let mut check = |p: f64| {
            let (whole, fraction) = four_decimals(p, &mut digits);
            let text = format!("{whole}.{fraction:0>4}");
            assert_eq!(text, format!("{p:.4}"), "{p:?}");
        };
        // A spread of values by a simple generator, and the ties and their neighbours.
        let mut x: u64 = 0x2545_f491_4f6c_dd1d;
        for _ in 0..20_000 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            check((x >> 11) as f64 / (1u64 << 53) as f64);
        }
        for n in 0..10_000u32 {
            let tie = (f64::from(n) + 0.5) / 10_000.0;
            check(tie);
            check(f64::from_bits(tie.to_bits() + 1));
            check(f64::from_bits(tie.to_bits() - 1));
        }
        for p in [0.0, 1e-9, 0.00004, 0.00005, 0.03125, 0.99995, 1.0] {
            check(p);
        }
    }

    #[test]
    fn progress_out_of_range_is_cut() {
        let body = |p| progress_body("d", "x", p, "n", "i").unwrap();
        assert!(body(-3.0).contains("\"percentage\":0.0000,"));
        assert!(body(7.0).contains("\"percentage\":1.0000,"));
        assert!(body(f64::NAN).contains("\"percentage\":0.0000,"));
    }

    #[test]
    fn answers_are_read() {
        for (text, want) in [
            ("{}", Some(0.0)),
            ("  { }  ", Some(0.0)),
            ("", None),
            ("[]", None),
            ("nonsense", None),
            ("{\"percentage\":0.5}", Some(0.5)),
            ("{\"percentage\":\"0.25\"}", Some(0.25)),
            ("{\"percentage\":1.5}", None),
            ("{\"percentage\":-0.1}", None),
            ("{\"percentage\":\"x\"}", None),
            ("{\"device\":\"Boox\"}", Some(0.0)),
            (
                "{\"percentage\":0.75,\"device_id\":\"A\",\"device\":\"Boox\",\"progress\":\"/body\"}",
                Some(0.75),
            ),
        ] {
            assert_eq!(remote_progress(text).unwrap(), want, "{text:?}");
        }
        let remote = read_answer(
            "{\"percentage\":0.75,\"device_id\":\"A\",\"device\":\"Boox\",\"progress\":\"/body\"}",
        )
        .unwrap()
        .unwrap();
        assert_eq!(remote.progress, 0.75);
        assert_eq!(remote.device_id.as_deref(), Some("A"));
        assert_eq!(remote.device.as_deref(), Some("Boox"));
        assert_eq!(remote.xpointer.as_deref(), Some("/body"));
        assert_eq!(read_answer("[]").unwrap(), None);
        // An answer above the cap is refused, not read.
        let long = alloc::format!("{{\"device\":\"{}\"}}", "x".repeat(MAX_ANSWER));
        assert_eq!(read_answer(&long).err(), Some(Error::NoSpace));
        assert_eq!(remote_progress(&long).err(), Some(Error::NoSpace));
    }

    fn seen(remote: f64, local: f64) -> Seen {
        Seen {
            remote,
            local,
            sent_from_here: false,
            declined: false,
            dismissed: false,
            manual: false,
        }
    }

    #[test]
    fn a_further_position_is_offered_unless_declined_or_put_off() {
        assert_eq!(judge(seen(0.5, 0.1)), Verdict::Offer);
        assert_eq!(judge(seen(0.1, 0.5)), Verdict::UpToDate);
        // Within the margin is not further.
        assert_eq!(judge(seen(0.101, 0.1)), Verdict::UpToDate);
        assert_eq!(judge(seen(0.1021, 0.1)), Verdict::Offer);
        // Declined: not asked again, unless the user asks.
        let declined = Seen {
            declined: true,
            ..seen(0.5, 0.1)
        };
        assert_eq!(judge(declined), Verdict::UpToDate);
        assert_eq!(
            judge(Seen {
                manual: true,
                ..declined
            }),
            Verdict::Offer
        );
        // Put off: held quietly, unless the user asks.
        let dismissed = Seen {
            dismissed: true,
            ..seen(0.5, 0.1)
        };
        assert_eq!(judge(dismissed), Verdict::HoldQuietly);
        assert_eq!(
            judge(Seen {
                manual: true,
                ..dismissed
            }),
            Verdict::Offer
        );
    }

    #[test]
    fn a_position_sent_from_here_is_not_offered_unless_the_book_starts_anew() {
        let here = Seen {
            sent_from_here: true,
            ..seen(0.5, 0.4)
        };
        assert_eq!(judge(here), Verdict::UpToDate);
        // Local progress lost (a copy of the book): the server's is offered.
        let anew = Seen {
            sent_from_here: true,
            ..seen(0.5, 0.0)
        };
        assert_eq!(judge(anew), Verdict::Offer);
    }

    #[test]
    fn a_book_left_behind_sends_only_what_does_not_overwrite_a_further_position() {
        assert!(send_left_behind(200, Some(0.2), false, 0.3));
        assert!(!send_left_behind(200, Some(0.9), false, 0.3));
        assert!(send_left_behind(200, Some(0.9), true, 0.3));
        assert!(!send_left_behind(200, Some(0.9), true, 0.0));
        assert!(!send_left_behind(500, Some(0.0), false, 0.3));
        assert!(!send_left_behind(200, None, false, 0.3));
    }
}
