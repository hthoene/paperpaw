//! The headers of a message (RFC 5322) for the native plugin: folded lines
//! unfolded, their values as text, the addresses and the date in them, and the
//! Content-Type of a part. The interface-6 `header` made fallible, bounded
//! (a mail from anyone has at most [`MAX_FIELDS`] headers of [`MAX_VALUE`]
//! bytes, [`MAX_PARAMS`] parameters) and free of arithmetic and slicing
//! that could panic; a date is read only for the years 1 to 9999.

use alloc::string::String;
use alloc::vec::Vec;

use super::codec::{self, Charset};
use super::mem::{Error, copy, lossy, push, push_bytes, push_char, push_str, vec_with};

/// The most headers kept of a message or a part; the rest are not read.
pub const MAX_FIELDS: usize = 256;
/// The most bytes of one header's value (unfolded), cut at the end.
pub const MAX_VALUE: usize = 8 * 1024;
/// The most parameters kept of a Content-Type.
pub const MAX_PARAMS: usize = 32;

/// The headers of a message or a part: name (lower case) and value
/// (unfolded, encoded words decoded), in the order they came.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Headers {
    fields: Vec<(String, String)>,
}

impl Headers {
    /// The headers in `raw` (up to an empty line, if there is one). A
    /// line that is no header (no colon, a continuation without a header)
    /// is skipped.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn parse(raw: &[u8]) -> Result<Self, Error> {
        let mut fields: Vec<(String, Vec<u8>)> = Vec::new();
        for line in raw.split(|&b| b == b'\n') {
            let line = line.strip_suffix(b"\r").unwrap_or(line);
            let Some(&first) = line.first() else {
                break;
            };
            if matches!(first, b' ' | b'\t') {
                if let Some((_, value)) = fields.last_mut() {
                    let room = MAX_VALUE.saturating_sub(value.len());
                    let part = trim(line);
                    // A space, then as much of the line as fits.
                    if room > 1 {
                        push_bytes(value, b" ")?;
                        push_bytes(value, part.get(..part.len().min(room - 1)).unwrap_or(&[]))?;
                    }
                }
                continue;
            }
            let Some(colon) = line.iter().position(|&b| b == b':') else {
                continue;
            };
            let name = line.get(..colon).unwrap_or(&[]);
            if name.is_empty() || name.iter().any(|b| !(33..=126).contains(b)) {
                continue;
            }
            if fields.len() >= MAX_FIELDS {
                break;
            }
            let mut lower = String::new();
            lower
                .try_reserve_exact(name.len())
                .map_err(|_| Error::NoMemory)?;
            lower.extend(name.iter().map(|b| char::from(b.to_ascii_lowercase())));
            let value = trim(line.get(colon + 1..).unwrap_or(&[]));
            let value = value.get(..value.len().min(MAX_VALUE)).unwrap_or(&[]);
            let mut kept = vec_with(value.len())?;
            push_bytes(&mut kept, value)?;
            push(&mut fields, (lower, kept))?;
        }
        let mut out = vec_with(fields.len())?;
        for (name, value) in fields {
            let text = if name == "content-type" || name == "content-disposition" {
                // Parameters are parsed from the raw text (see
                // `ContentType`), not as encoded words.
                lossy(&value)?
            } else {
                codec::decode_words(&value)?
            };
            push(&mut out, (name, text))?;
        }
        Ok(Self { fields: out })
    }

    /// The first header `name` (lower case).
    pub fn get(&self, name: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    pub fn len(&self) -> usize {
        self.fields.len()
    }

    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }
}

fn trim(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|b| !matches!(b, b' ' | b'\t'))
        .unwrap_or(bytes.len());
    let end = bytes
        .iter()
        .rposition(|b| !matches!(b, b' ' | b'\t' | b'\r' | b'\n'))
        .map_or(start, |p| p + 1);
    bytes.get(start..end.max(start)).unwrap_or(&[])
}

/// A Content-Type (or Content-Disposition) value: the type, in lower
/// case, and its parameters.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContentType {
    /// `text/plain`, `multipart/alternative`; `text/plain` where the
    /// header is missing (RFC 2045).
    pub kind: String,
    params: Vec<(String, String)>,
}

impl ContentType {
    /// The value of a Content-Type header (`None`: the default).
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn parse(value: Option<&str>) -> Result<Self, Error> {
        let Some(value) = value else {
            return Ok(Self {
                kind: copy("text/plain")?,
                params: Vec::new(),
            });
        };
        let mut parts = Params::new(value);
        let mut kind = String::new();
        let first = parts.next().unwrap_or("").trim();
        kind.try_reserve_exact(first.len())
            .map_err(|_| Error::NoMemory)?;
        kind.extend(first.chars().map(|c| c.to_ascii_lowercase()));
        if !kind.contains('/') {
            kind = copy("text/plain")?;
        }
        let mut params: Vec<(String, String)> = Vec::new();
        for part in parts {
            let Some((name, value)) = part.split_once('=') else {
                continue;
            };
            let name = name.trim();
            let value = unquote(value.trim())?;
            // RFC 2231: `name*=utf-8''%C3%A4` and pieces `name*0=…`.
            if let Some((base, rest)) = name.split_once('*') {
                let extended = rest.ends_with('*') || rest.is_empty();
                let value = if extended {
                    percent_decoded(&value)?
                } else {
                    value
                };
                let mut base = copy(base)?;
                base.make_ascii_lowercase();
                // The part after `*` is a number or `number*`, in any case.
                match params.iter_mut().find(|(n, _)| *n == base) {
                    Some((_, existing)) if !rest.is_empty() && !rest.starts_with('0') => {
                        push_str(existing, &value)?;
                    }
                    Some(_) => {}
                    None => {
                        if params.len() < MAX_PARAMS {
                            push(&mut params, (base, value))?;
                        }
                    }
                }
                continue;
            }
            if params.len() < MAX_PARAMS {
                let mut name = copy(name)?;
                name.make_ascii_lowercase();
                push(&mut params, (name, value))?;
            }
        }
        Ok(Self { kind, params })
    }

    /// A parameter, e.g. `boundary` or `charset`.
    pub fn param(&self, name: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    /// Whether it is `multipart/…`.
    pub fn is_multipart(&self) -> bool {
        self.kind.starts_with("multipart/")
    }
}

/// `value` cut at the `;` outside quotes, one piece after the other.
struct Params<'a> {
    rest: Option<&'a str>,
}

impl<'a> Params<'a> {
    fn new(value: &'a str) -> Self {
        Self { rest: Some(value) }
    }
}

impl<'a> Iterator for Params<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<&'a str> {
        let value = self.rest?;
        let mut quoted = false;
        for (i, c) in value.char_indices() {
            match c {
                '"' => quoted = !quoted,
                ';' if !quoted => {
                    self.rest = value.get(i + 1..);
                    return value.get(..i);
                }
                _ => {}
            }
        }
        self.rest = None;
        Some(value)
    }
}

fn unquote(value: &str) -> Result<String, Error> {
    let Some(inner) = value.strip_prefix('"') else {
        return copy(value);
    };
    let mut out = String::new();
    out.try_reserve_exact(inner.len())
        .map_err(|_| Error::NoMemory)?;
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => break,
            '\\' => {
                if let Some(next) = chars.next() {
                    push_char(&mut out, next)?;
                }
            }
            c => push_char(&mut out, c)?,
        }
    }
    Ok(out)
}

/// RFC 2231's `utf-8''%C3%A4`: the text with its percent signs undone.
fn percent_decoded(value: &str) -> Result<String, Error> {
    let (charset, rest) = match value.split_once('\'') {
        Some((charset, rest)) => (
            charset,
            rest.split_once('\'').map_or(rest, |(_, text)| text),
        ),
        None => ("", value),
    };
    let raw = rest.as_bytes();
    let mut bytes = vec_with(raw.len())?;
    let mut i = 0;
    while let Some(&byte) = raw.get(i) {
        let digit = |at: usize| raw.get(at).and_then(|b| char::from(*b).to_digit(16));
        if byte == b'%'
            && let (Some(h), Some(l)) = (digit(i + 1), digit(i + 2))
        {
            bytes.push((h * 16 + l) as u8);
            i += 3;
        } else {
            bytes.push(byte);
            i += 1;
        }
    }
    codec::text_of(Charset::of(charset).unwrap_or(Charset::Utf8), &bytes)
}

/// An address as a header gives it: `Ada <ada@example.org>`,
/// `"Lovelace, Ada" <ada@example.org>`, `ada@example.org`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Address {
    /// The name shown, empty where there is none.
    pub name: String,
    /// `ada@example.org`.
    pub address: String,
}

impl Address {
    /// The name, else the address.
    pub fn shown(&self) -> &str {
        if self.name.is_empty() {
            &self.address
        } else {
            &self.name
        }
    }
}

/// The most addresses taken from one header.
pub const MAX_ADDRESSES: usize = 32;
/// The most bytes kept of a name, an address, a subject, an id, the ids a
/// message refers to: a mail from anyone must not make the lists, the
/// index and the letter grow without end.
pub const MAX_NAME: usize = 128;
pub const MAX_ADDRESS: usize = 128;
pub const MAX_SUBJECT: usize = 256;
pub const MAX_ID: usize = 256;
pub const MAX_REFERENCES: usize = 2048;
/// An attachment's name, and a list of addresses written out.
pub const MAX_LABEL: usize = 128;
pub const MAX_LIST: usize = 2048;

/// `text` up to `max` bytes, cut at a character boundary.
pub fn cut(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut end = max;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text.get(..end).unwrap_or("")
}

/// `text` as an owned string of up to `max` bytes.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn capped(text: &str, max: usize) -> Result<String, Error> {
    copy(cut(text, max))
}

/// The ids of a References header, the last ones that fit `max` bytes (the
/// ones a reply adds are at its end).
///
/// # Errors
/// [`Error::NoMemory`].
pub fn last_ids(text: &str, max: usize) -> Result<String, Error> {
    let text = text.trim();
    if text.len() <= max {
        return copy(text);
    }
    let mut start = text.len() - max;
    while start < text.len() && !text.is_char_boundary(start) {
        start += 1;
    }
    // From the next whole id.
    let tail = text.get(start..).unwrap_or("");
    let tail = tail
        .find(char::is_whitespace)
        .map_or("", |i| tail.get(i..).unwrap_or("").trim_start());
    copy(tail)
}

/// The addresses of a header's value, comma apart (a comma in a quoted
/// name or a group does not part them). Comments in parentheses go.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn addresses(value: &str) -> Result<Vec<Address>, Error> {
    let mut out = Vec::new();
    let (mut start, mut quoted, mut angle, mut comment) = (0, false, false, 0u32);
    let mut pieces = 0;
    for (i, c) in value.char_indices() {
        match c {
            '"' if comment == 0 => quoted = !quoted,
            '<' if !quoted && comment == 0 => angle = true,
            '>' if !quoted && comment == 0 => angle = false,
            '(' if !quoted => comment += 1,
            ')' if !quoted && comment > 0 => comment -= 1,
            ',' | ';' if !quoted && !angle && comment == 0 => {
                add_address(&mut out, value.get(start..i).unwrap_or(""))?;
                pieces += 1;
                start = i + 1;
                if pieces >= MAX_ADDRESSES {
                    return Ok(out);
                }
            }
            _ => {}
        }
    }
    add_address(&mut out, value.get(start..).unwrap_or(""))?;
    Ok(out)
}

fn add_address(out: &mut Vec<Address>, piece: &str) -> Result<(), Error> {
    if let Some(mut address) = one_address(piece)? {
        address.name.truncate(cut(&address.name, MAX_NAME).len());
        address
            .address
            .truncate(cut(&address.address, MAX_ADDRESS).len());
        push(out, address)?;
    }
    Ok(())
}

fn one_address(piece: &str) -> Result<Option<Address>, Error> {
    let piece = piece.trim();
    // `Group: a@b` leaves the group's name out.
    let piece = match piece.split_once(':') {
        Some((_, rest)) if !piece.contains('<') || piece.find(':') < piece.find('<') => rest.trim(),
        _ => piece,
    };
    if let Some(open) = piece.rfind('<') {
        let after = piece.get(open..).unwrap_or("");
        let close = after.find('>').map_or(piece.len(), |c| open + c);
        let address = piece.get(open + 1..close).unwrap_or("").trim();
        if !address.contains('@') {
            return Ok(None);
        }
        let name = unquote(piece.get(..open).unwrap_or("").trim())?;
        return Ok(Some(Address {
            name: copy(name.trim())?,
            address: copy(address)?,
        }));
    }
    // Without brackets: the address, maybe with a comment `(Ada)`.
    let mut bare = String::new();
    bare.try_reserve_exact(piece.len())
        .map_err(|_| Error::NoMemory)?;
    let mut comment: Option<String> = None;
    let mut depth = 0u32;
    let mut note = String::new();
    for c in piece.chars() {
        match c {
            '(' => depth += 1,
            ')' if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    comment = Some(copy(&note)?);
                }
            }
            c if depth > 0 => push_char(&mut note, c)?,
            c => push_char(&mut bare, c)?,
        }
    }
    let bare = bare.trim();
    if !bare.contains('@') || bare.contains(char::is_whitespace) {
        return Ok(None);
    }
    let name = match comment {
        Some(note) => copy(note.trim())?,
        None => String::new(),
    };
    Ok(Some(Address {
        name,
        address: copy(bare)?,
    }))
}

/// The date of a Date header (RFC 5322: `Tue, 3 Nov 2026 09:15:00 +0100`)
/// as seconds since 1970; `None` where it cannot be read, or where the year
/// is not one of 1 to 9999.
pub fn date_seconds(value: &str) -> Option<i64> {
    let value = value.trim();
    // Drop a weekday and a comment `(CET)`.
    let value = value.split_once(',').map_or(value, |(_, rest)| rest);
    let value = value.split('(').next().unwrap_or(value);
    let mut words = value.split_whitespace();
    let day: i64 = words.next()?.parse().ok()?;
    let month = month_of(words.next()?)?;
    let mut year: i64 = words.next()?.parse().ok()?;
    // Two digits: RFC 5322's rule.
    if year < 50 {
        year += 2000;
    } else if year < 100 {
        year += 1900;
    }
    if !(1..=9999).contains(&year) {
        return None;
    }
    let time = words.next().unwrap_or("00:00:00");
    let mut clock = time.split(':');
    let hour: i64 = clock.next()?.parse().ok()?;
    let minute: i64 = clock.next().unwrap_or("0").parse().ok()?;
    let second: i64 = clock.next().unwrap_or("0").parse().ok()?;
    let zone = words.next().unwrap_or("+0000");
    let offset = zone_seconds(zone);
    if !(1..=31).contains(&day) || !(0..=23).contains(&hour) || !(0..=59).contains(&minute) {
        return None;
    }
    if !(0..=60).contains(&second) {
        return None;
    }
    Some(days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60 + second - offset)
}

fn month_of(name: &str) -> Option<i64> {
    const MONTHS: [&str; 12] = [
        "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
    ];
    // The first three characters, compared as they are (no copy).
    let mut chars = name.chars();
    let mut key = [0u8; 3];
    for slot in &mut key {
        let c = chars.next()?;
        *slot = u8::try_from(c.to_ascii_lowercase()).ok()?;
    }
    MONTHS
        .iter()
        .position(|m| m.as_bytes() == key)
        .map(|i| i as i64 + 1)
}

fn zone_seconds(zone: &str) -> i64 {
    let named = match zone {
        "GMT" | "UT" | "UTC" | "Z" => Some(0),
        "EST" => Some(-5),
        "EDT" => Some(-4),
        "CST" => Some(-6),
        "CDT" => Some(-5),
        "MST" => Some(-7),
        "MDT" => Some(-6),
        "PST" => Some(-8),
        "PDT" => Some(-7),
        "CET" => Some(1),
        "CEST" => Some(2),
        _ => None,
    };
    if let Some(hours) = named {
        return hours * 3600;
    }
    let (sign, digits) = match zone.as_bytes().first() {
        Some(b'+') => (1, zone.get(1..).unwrap_or("")),
        Some(b'-') => (-1, zone.get(1..).unwrap_or("")),
        _ => return 0,
    };
    if digits.len() != 4 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return 0;
    }
    let number: i64 = digits.parse().unwrap_or(0);
    sign * ((number / 100) * 3600 + (number % 100) * 60)
}

/// Days from 1970-01-01 to the civil date (proleptic Gregorian). The year is
/// held to 1 to 9999 (what a Date header has), and the arithmetic saturates,
/// so no input overflows into a panic.
pub fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = year.clamp(1, 9999);
    let y = if month <= 2 {
        year.saturating_sub(1)
    } else {
        year
    };
    let era = y.div_euclid(400);
    let yoe = y.saturating_sub(era.saturating_mul(400));
    let month_part = if month > 2 {
        month.saturating_sub(3)
    } else {
        month.saturating_add(9)
    };
    let doy = month_part
        .saturating_mul(153)
        .saturating_add(2)
        .saturating_div(5)
        .saturating_add(day)
        .saturating_sub(1);
    let doe = yoe
        .saturating_mul(365)
        .saturating_add(yoe / 4)
        .saturating_sub(yoe / 100)
        .saturating_add(doy);
    era.saturating_mul(146_097)
        .saturating_add(doe)
        .saturating_sub(719_468)
}

/// The civil date (year, month, day) of `days` since 1970-01-01; a day count
/// no date has comes saturated, not as a panic.
pub fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days.saturating_add(719_468);
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe.saturating_add(era.saturating_mul(400));
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y.saturating_add(1) } else { y }, m, d)
}

/// `subject` for a reply: `Re: ` once, whatever case a former one had.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn reply_subject(subject: &str) -> Result<String, Error> {
    let subject = subject.trim();
    if super::mem::starts_with_ignore_case(subject, "re:") {
        return copy(subject);
    }
    let mut out = super::mem::string_with(subject.len() + 4)?;
    push_str(&mut out, "Re: ")?;
    push_str(&mut out, subject)?;
    Ok(out)
}
