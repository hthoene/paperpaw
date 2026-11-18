//! What the news plugin decides without the host: the feeds it keeps and how
//! they are written, a feed's short name, the mark of its address, the
//! newest headlines it keeps, and the dates feeds give. The same rules as the
//! interface-6 plugin (`lib.rs`, `convert.rs`), with fallible allocation and
//! the text given by the caller instead of read from a storage. Fetching, the
//! conversion of a feed to a page, the files, the list screen and the
//! headlines widget are the native plugin's later (see `PORTING-7.md`).
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::native::Error;
use paperpaw_sdk::time::days_in_month;
use paperpaw_sdk::web::host;

/// The most `feeds.txt` is read to change, as in interface 6 (64 KiB).
pub const FEEDS_CAP: usize = 64 * 1024;
/// The most `headlines.txt` is read to be added to, as in interface 6 (32
/// KiB); a larger file is left out and written over.
pub const HEADLINES_CAP: usize = 32 * 1024;
/// Where the newest headlines are kept (`title<TAB>book` lines), in the data folder.
pub const HEADLINES_FILE: &str = "headlines.txt";
/// The most headlines kept in all.
pub const HEADLINES_KEPT: usize = 40;
/// The largest feed taken, as in interface 6 (4 MiB): the answer is read as
/// it streams, but a larger one is not taken.
pub const MAX_FEED_FILE: u64 = 4 * 1024 * 1024;
/// Text kept of an article: little on small devices.
pub const MAX_TEXT_SMALL: usize = 8 * 1024;
pub const MAX_TEXT: usize = 64 * 1024;
/// Titles, dates and links are cut at this many bytes.
pub const MAX_FIELD: usize = 400;
/// The headlines of a feed that the page names at its top.
pub const HEADLINES_SHOWN: usize = 5;
/// Articles a feed keeps: the setting is clamped to this.
pub const ARTICLES: core::ops::RangeInclusive<i64> = 1..=200;

fn join(parts: &[&str]) -> Result<String, Error> {
    let mut out = String::new();
    out.try_reserve_exact(parts.iter().map(|p| p.len()).sum())
        .map_err(|_| Error::NoMemory)?;
    for part in parts {
        out.push_str(part);
    }
    Ok(out)
}

fn copy(text: &str) -> Result<String, Error> {
    join(&[text])
}

fn push<T>(list: &mut Vec<T>, item: T) -> Result<(), Error> {
    list.try_reserve(1).map_err(|_| Error::NoMemory)?;
    list.push(item);
    Ok(())
}

/// A short mark of a feed's address, six hex digits (24 bits of its FNV-1a
/// hash): feeds of the same name keep books of their own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mark([u8; 6]);

impl Mark {
    pub fn of(url: &str) -> Self {
        let hash = url.bytes().fold(0x811c_9dc5_u32, |hash, b| {
            (hash ^ u32::from(b)).wrapping_mul(0x0100_0193)
        });
        let mut digits = [0u8; 6];
        for (i, d) in digits.iter_mut().enumerate() {
            let nibble = (hash >> (20 - 4 * i)) & 0xf;
            *d = b"0123456789abcdef"[nibble as usize];
        }
        Self(digits)
    }

    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.0).unwrap_or("000000")
    }
}

/// A book's name without the mark of its feed's address (`Tagesschau
/// (3fa9c2)` is "Tagesschau").
pub fn without_mark(name: &str) -> &str {
    match name.rsplit_once(" (") {
        Some((name, rest))
            if rest.len() == 7
                && rest.ends_with(')')
                && rest[..6].bytes().all(|b| b.is_ascii_hexdigit()) =>
        {
            name
        }
        _ => name,
    }
}

/// A feed's title without its tagline ("Site - all the news" gives "Site").
///
/// # Errors
/// `NoMemory` when the title cannot be kept.
pub fn short_title(title: &str) -> Result<String, Error> {
    for separator in [" - ", " | ", " \u{2013} ", " \u{2014} ", ": "] {
        if let Some((name, _)) = title.split_once(separator)
            && name.trim().chars().count() >= 3
        {
            return copy(name.trim());
        }
    }
    copy(title.trim())
}

/// A feed: its address and its name (the feed's own title once fetched).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Feed {
    pub url: String,
    pub name: String,
}

impl Feed {
    /// What the list shows: its name, or its host without `www.`.
    pub fn shown(&self) -> &str {
        if self.name.is_empty() {
            host(&self.url).trim_start_matches("www.")
        } else {
            &self.name
        }
    }
}

/// `text`, the file as read, without a UTF-8 byte order mark; `NoSpace` over
/// `cap` bytes. A list too large is not an empty one: the caller must not
/// write the file then (what it holds would be lost), as in interface 6.
fn within(text: &str, cap: usize) -> Result<&str, Error> {
    if text.len() > cap {
        return Err(Error::NoSpace);
    }
    Ok(text.strip_prefix('\u{feff}').unwrap_or(text))
}

/// The feeds in `feeds.txt` (`address<TAB>name` lines, a line with no address
/// left out). `text` is the file as read, at most [`FEEDS_CAP`] bytes.
///
/// # Errors
/// `NoSpace` over [`FEEDS_CAP`]; `NoMemory` when the list cannot be kept.
pub fn feeds(text: &str) -> Result<Vec<Feed>, Error> {
    let mut list = Vec::new();
    for line in within(text, FEEDS_CAP)?.lines() {
        let (url, name) = line.split_once('\t').unwrap_or((line, ""));
        if !url.trim().is_empty() {
            let feed = Feed {
                url: copy(url.trim())?,
                name: copy(name.trim())?,
            };
            push(&mut list, feed)?;
        }
    }
    Ok(list)
}

/// The feeds as they are kept: `address<TAB>name` lines, tabs and line
/// breaks in a name made spaces.
///
/// # Errors
/// `NoMemory` when the text cannot be kept.
pub fn feeds_text(list: &[Feed]) -> Result<String, Error> {
    let size = list.iter().map(|f| f.url.len() + f.name.len() + 2).sum();
    let mut out = String::new();
    out.try_reserve_exact(size).map_err(|_| Error::NoMemory)?;
    for f in list {
        out.push_str(&f.url);
        out.push('\t');
        out.extend(
            f.name
                .chars()
                .map(|c| if matches!(c, '\t' | '\n') { ' ' } else { c }),
        );
        out.push('\n');
    }
    Ok(out)
}

/// `bytes` as text, an invalid sequence replaced by U+FFFD as
/// `String::from_utf8_lossy` does, in memory that may be refused.
fn lossy(bytes: &[u8]) -> Result<String, Error> {
    let size = bytes
        .utf8_chunks()
        .map(|c| c.valid().len() + 3 * usize::from(!c.invalid().is_empty()))
        .sum();
    let mut out = String::new();
    out.try_reserve_exact(size).map_err(|_| Error::NoMemory)?;
    for chunk in bytes.utf8_chunks() {
        out.push_str(chunk.valid());
        if !chunk.invalid().is_empty() {
            out.push('\u{fffd}');
        }
    }
    Ok(out)
}

/// The headlines file once `titles` are kept as the headlines of `book`,
/// before the others': `title<TAB>book` lines joined by line breaks, at most
/// [`HEADLINES_KEPT`]. `old` is the file as it was, if it could be read: one
/// larger than [`HEADLINES_CAP`] counts as none (it is written over, as in
/// interface 6), and the old lines of `book` itself are dropped.
///
/// # Errors
/// `NoMemory` when the text cannot be kept.
pub fn headlines(titles: &[String], book: &str, old: Option<&[u8]>) -> Result<String, Error> {
    let mut lines: Vec<String> = Vec::new();
    for title in titles {
        let mut line = String::new();
        line.try_reserve_exact(title.len() + 1 + book.len())
            .map_err(|_| Error::NoMemory)?;
        line.extend(
            title
                .chars()
                .map(|c| if matches!(c, '\t' | '\n') { ' ' } else { c }),
        );
        line.push('\t');
        line.push_str(book);
        push(&mut lines, line)?;
    }
    if let Some(old) = old.filter(|old| old.len() <= HEADLINES_CAP) {
        let old = lossy(old)?;
        for line in old.lines() {
            let mine = line
                .strip_suffix(book)
                .is_some_and(|rest| rest.ends_with('\t'));
            if !mine && lines.len() < HEADLINES_KEPT {
                push(&mut lines, copy(line)?)?;
            }
        }
    }
    let size = lines.iter().map(String::len).sum::<usize>() + lines.len().saturating_sub(1);
    let mut out = String::new();
    out.try_reserve_exact(size).map_err(|_| Error::NoMemory)?;
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(line);
    }
    Ok(out)
}

/// The day of a date as a feed writes it: `2026-10-24T14:32:44-04:00` (Atom,
/// ISO 8601) or `Sat, 24 Oct 2026 14:32:44 GMT` (RSS, RFC 822, the weekday
/// and the year in two digits optional). The day as it is written, not moved
/// to the reader's time zone. `None` where it is neither.
pub fn read_date(text: &str) -> Option<(i32, u8, u8)> {
    let text = text.trim();
    let (year, month, day) = iso_date(text).or_else(|| rfc822_date(text))?;
    let real = (1..=12).contains(&month) && (1..=days_in_month(year, month)).contains(&day);
    real.then_some((year, month, day))
}

fn iso_date(text: &str) -> Option<(i32, u8, u8)> {
    let date = text.get(..10)?;
    let mut parts = date.split('-');
    let (year, month, day) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() || year.len() != 4 || month.len() != 2 || day.len() != 2 {
        return None;
    }
    Some((year.parse().ok()?, month.parse().ok()?, day.parse().ok()?))
}

fn rfc822_date(text: &str) -> Option<(i32, u8, u8)> {
    const MONTHS: [&str; 12] = [
        "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
    ];
    let rest = text.split_once(',').map_or(text, |(_, rest)| rest);
    let mut words = rest.split_whitespace();
    let day = words.next()?.parse().ok()?;
    let name = words.next()?.get(..3)?;
    let month = MONTHS.iter().position(|m| m.eq_ignore_ascii_case(name))? as u8 + 1;
    let year: i32 = words.next()?.parse().ok()?;
    // Two digits, as RFC 822 has them: 70 to 99 are 19xx.
    let year = match year {
        0..=69 => year + 2000,
        70..=99 => year + 1900,
        _ => year,
    };
    Some((year, month, day))
}

/// `text` cut to at most `max` bytes, at a character boundary.
///
/// # Errors
/// `NoMemory` when the text cannot be kept.
pub fn cut(text: &str, max: usize) -> Result<String, Error> {
    let mut end = max.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    copy(&text[..end])
}

/// Whitespace collapsed to single spaces.
///
/// # Errors
/// `NoMemory` when the text cannot be kept.
pub fn one_line(text: &str) -> Result<String, Error> {
    let mut out = String::new();
    out.try_reserve_exact(text.len())
        .map_err(|_| Error::NoMemory)?;
    for (i, word) in text.split_whitespace().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        out.push_str(word);
    }
    Ok(out)
}

/// The size of `text` once it is safe in HTML (see [`escape`]).
#[must_use]
pub fn escape_len(text: &str) -> usize {
    text.chars()
        .map(|c| match c {
            '&' => 5,
            '"' => 6,
            '<' | '>' => 4,
            c => c.len_utf8(),
        })
        .sum()
}

/// `text` safe in HTML: `&`, `<`, `>` and `"` as entities.
///
/// # Errors
/// `NoMemory` when the text cannot be kept.
pub fn escape(text: &str) -> Result<String, Error> {
    let size = escape_len(text);
    let mut out = String::new();
    out.try_reserve_exact(size).map_err(|_| Error::NoMemory)?;
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
    Ok(out)
}

/// Elements that start a new paragraph.
pub fn is_block(name: &str) -> bool {
    matches!(
        name,
        "p" | "div"
            | "br"
            | "li"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "blockquote"
            | "tr"
            | "section"
            | "article"
            | "ul"
            | "ol"
            | "pre"
            | "figure"
            | "figcaption"
            | "hr"
            | "table"
            | "dd"
            | "dt"
    )
}
