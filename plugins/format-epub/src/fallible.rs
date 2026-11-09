//! Allocation that may be refused: a copy, a push or a formatted string
//! that finds no room answers [`DocumentError::OutOfMemory`] instead of
//! aborting the device (`handle_alloc_error`). Every growth of a buffer on
//! the path of opening and reading a book goes through here, or through
//! `try_reserve` itself.

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::{self, Write};

use paperpaw_sdk::document::DocumentError;

/// The answer when no room is found.
pub(crate) const OOM: DocumentError = DocumentError::OutOfMemory;

/// Room for `more` bytes in `out`, or `OutOfMemory`.
pub(crate) fn reserve_str(out: &mut String, more: usize) -> Result<(), DocumentError> {
    out.try_reserve(more).map_err(|_| OOM)
}

/// `text` onto `out`.
pub(crate) fn push_str(out: &mut String, text: &str) -> Result<(), DocumentError> {
    out.try_reserve(text.len()).map_err(|_| OOM)?;
    out.push_str(text);
    Ok(())
}

/// `c` onto `out`.
pub(crate) fn push_char(out: &mut String, c: char) -> Result<(), DocumentError> {
    out.try_reserve(c.len_utf8()).map_err(|_| OOM)?;
    out.push(c);
    Ok(())
}

/// A copy of `text`.
pub(crate) fn string(text: &str) -> Result<String, DocumentError> {
    let mut out = String::new();
    push_str(&mut out, text)?;
    Ok(out)
}

/// An empty string with room for `capacity` bytes.
pub(crate) fn string_with(capacity: usize) -> Result<String, DocumentError> {
    let mut out = String::new();
    reserve_str(&mut out, capacity)?;
    Ok(out)
}

/// `item` onto `list`.
pub(crate) fn push<T>(list: &mut Vec<T>, item: T) -> Result<(), DocumentError> {
    list.try_reserve(1).map_err(|_| OOM)?;
    list.push(item);
    Ok(())
}

/// Room for `more` items in `list`.
pub(crate) fn reserve<T>(list: &mut Vec<T>, more: usize) -> Result<(), DocumentError> {
    list.try_reserve(more).map_err(|_| OOM)
}

/// Room for exactly `more` items in `list`.
pub(crate) fn reserve_exact<T>(list: &mut Vec<T>, more: usize) -> Result<(), DocumentError> {
    list.try_reserve_exact(more).map_err(|_| OOM)
}

/// `bytes` zeros.
pub(crate) fn zeroed(bytes: usize) -> Result<Vec<u8>, DocumentError> {
    let mut out = Vec::new();
    out.try_reserve_exact(bytes).map_err(|_| OOM)?;
    out.resize(bytes, 0);
    Ok(out)
}

/// A `fmt::Write` that reserves before it writes and remembers a refusal.
struct Room<'a> {
    out: &'a mut String,
    refused: bool,
}

impl Write for Room<'_> {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        if self.out.try_reserve(text.len()).is_err() {
            self.refused = true;
            return Err(fmt::Error);
        }
        self.out.push_str(text);
        Ok(())
    }
}

/// `args` written onto `out`.
pub(crate) fn write(out: &mut String, args: fmt::Arguments<'_>) -> Result<(), DocumentError> {
    let mut room = Room {
        out,
        refused: false,
    };
    match room.write_fmt(args) {
        Ok(()) => Ok(()),
        Err(_) if room.refused => Err(OOM),
        // A `Display` that failed by itself: nothing was refused here.
        Err(_) => Err(DocumentError::Other(0)),
    }
}

/// `args` as a new string.
pub(crate) fn format(args: fmt::Arguments<'_>) -> Result<String, DocumentError> {
    let mut out = String::new();
    write(&mut out, args)?;
    Ok(out)
}

/// `format!`, answering `OutOfMemory` where there is no room.
macro_rules! try_format {
    ($($arg:tt)*) => {
        $crate::fallible::format(format_args!($($arg)*))
    };
}
pub(crate) use try_format;

/// `write!` onto a string, answering `OutOfMemory` where there is no room.
macro_rules! try_write {
    ($out:expr, $($arg:tt)*) => {
        $crate::fallible::write($out, format_args!($($arg)*))
    };
}
pub(crate) use try_write;

/// `text` written onto `out` safe as XML or HTML text and attribute
/// values, as [`paperpaw_sdk::xml::escape`] does it.
pub(crate) fn escape(text: &str, out: &mut String) -> Result<(), DocumentError> {
    for c in text.chars() {
        match c {
            '&' => push_str(out, "&amp;")?,
            '<' => push_str(out, "&lt;")?,
            '>' => push_str(out, "&gt;")?,
            '"' => push_str(out, "&quot;")?,
            c => push_char(out, c)?,
        }
    }
    Ok(())
}

/// The first bytes of `path`, at most 4 KB, on the heap.
pub(crate) fn head(
    storage: &dyn paperpaw_sdk::Storage,
    path: &str,
) -> Result<Vec<u8>, DocumentError> {
    let mut head = zeroed(4096)?;
    let n = storage.read_at(path, 0, &mut head)?;
    head.truncate(n);
    Ok(head)
}

/// `dir/name`, or `name` at the top, as `paperpaw_sdk::files::join` joins
/// them.
pub(crate) fn join(dir: &str, name: &str) -> Result<String, DocumentError> {
    if dir.is_empty() {
        string(name)
    } else {
        try_format!("{dir}/{name}")
    }
}

/// What tells the book at `path` apart, as
/// [`fingerprint`](crate::fingerprint) says it, where a refusal is
/// `OutOfMemory`.
pub(crate) fn fingerprint(
    storage: &dyn paperpaw_sdk::Storage,
    path: &str,
) -> Result<String, DocumentError> {
    crate::fingerprint_with(storage, path)
}

/// Where a plugin keeps what it keeps for the book at `book`, as
/// [`paperpaw_sdk::library::kept_path`] says it.
pub(crate) fn kept_path(folder: &str, prefix: &str, book: &str) -> Result<String, DocumentError> {
    let key = try_format!("{:016x}", paperpaw_sdk::digest::fnv1a64(book))?;
    let folder = folder.trim_end_matches('/');
    try_format!("{folder}/{}/{prefix}{key}", key.get(..2).unwrap_or("00"))
}

/// The directory where plugin `plugin` keeps its data, as
/// [`paperpaw_sdk::Env::plugin_dir`] says it.
pub(crate) fn plugin_dir(plugin: &str) -> Result<String, DocumentError> {
    try_format!("{}/plugins/{plugin}", paperpaw_sdk::data::DIR)
}

/// `push_str` and `push` that answer `OutOfMemory` where there is no room.
pub(crate) trait TryPush<T> {
    /// `item` onto the end.
    fn try_push(&mut self, item: T) -> Result<(), DocumentError>;
}

impl TryPush<char> for String {
    fn try_push(&mut self, item: char) -> Result<(), DocumentError> {
        push_char(self, item)
    }
}

impl<T> TryPush<T> for Vec<T> {
    fn try_push(&mut self, item: T) -> Result<(), DocumentError> {
        push(self, item)
    }
}

/// `push_str` for strings.
pub(crate) trait TryPushStr {
    /// `text` onto the end.
    fn try_push_str(&mut self, text: &str) -> Result<(), DocumentError>;
}

impl TryPushStr for String {
    fn try_push_str(&mut self, text: &str) -> Result<(), DocumentError> {
        push_str(self, text)
    }
}

/// `extend_from_slice` for vectors.
pub(crate) trait TryExtend<T: Clone> {
    /// `items` onto the end.
    fn try_extend_from_slice(&mut self, items: &[T]) -> Result<(), DocumentError>;
}

impl<T: Clone> TryExtend<T> for Vec<T> {
    fn try_extend_from_slice(&mut self, items: &[T]) -> Result<(), DocumentError> {
        reserve(self, items.len())?;
        self.extend_from_slice(items);
        Ok(())
    }
}

/// `bytes` as text, invalid sequences replaced as `String::from_utf8_lossy`
/// does it; a copy only where something was replaced.
pub(crate) fn lossy(bytes: &[u8]) -> Result<alloc::borrow::Cow<'_, str>, DocumentError> {
    match core::str::from_utf8(bytes) {
        Ok(text) => Ok(alloc::borrow::Cow::Borrowed(text)),
        Err(_) => {
            let mut out = String::new();
            for chunk in bytes.utf8_chunks() {
                push_str(&mut out, chunk.valid())?;
                if !chunk.invalid().is_empty() {
                    push_char(&mut out, char::REPLACEMENT_CHARACTER)?;
                }
            }
            Ok(alloc::borrow::Cow::Owned(out))
        }
    }
}

/// A copy of `text`, if there is one.
pub(crate) fn opt_string(text: Option<&String>) -> Result<Option<String>, DocumentError> {
    text.map(|text| string(text)).transpose()
}

/// A copy of `texts`.
pub(crate) fn strings(texts: &[String]) -> Result<Vec<String>, DocumentError> {
    let mut out = Vec::new();
    reserve(&mut out, texts.len())?;
    for text in texts {
        push(&mut out, string(text)?)?;
    }
    Ok(out)
}

/// A copy of `metadata`.
pub(crate) fn metadata(
    metadata: &paperpaw_sdk::document::Metadata,
) -> Result<paperpaw_sdk::document::Metadata, DocumentError> {
    Ok(paperpaw_sdk::document::Metadata::default()
        .title(opt_string(metadata.title.as_ref())?)
        .authors(strings(&metadata.authors)?)
        .language(opt_string(metadata.language.as_ref())?)
        .series(opt_string(metadata.series.as_ref())?)
        .series_index(opt_string(metadata.series_index.as_ref())?)
        .subjects(strings(&metadata.subjects)?)
        .description(opt_string(metadata.description.as_ref())?)
        .direction(metadata.direction)
        .writing_mode(metadata.writing_mode))
}

/// Whether the file at `path` holds exactly `expected` (at most 256 bytes),
/// read into the stack.
pub(crate) fn file_is(storage: &dyn paperpaw_sdk::Storage, path: &str, expected: &[u8]) -> bool {
    let mut buf = [0u8; 257];
    if expected.len() >= buf.len() {
        return false;
    }
    let mut filled = 0;
    while filled < buf.len() {
        match storage.read_at(path, filled as u64, &mut buf[filled..]) {
            Ok(0) | Err(_) => break,
            Ok(n) => filled += n,
        }
    }
    &buf[..filled] == expected
}

/// `text`'s words, one space between them.
pub(crate) fn words(text: &str) -> Result<String, DocumentError> {
    let mut out = String::new();
    for word in text.split_whitespace() {
        if !out.is_empty() {
            push_char(&mut out, ' ')?;
        }
        push_str(&mut out, word)?;
    }
    Ok(out)
}

/// `text` in ASCII lower case.
pub(crate) fn lowercase(text: &str) -> Result<String, DocumentError> {
    let mut out = string_with(text.len())?;
    out.extend(text.chars().map(|c| c.to_ascii_lowercase()));
    Ok(out)
}

/// `text` with `%XX` sequences decoded.
pub(crate) fn percent_decode(text: &str) -> Result<String, DocumentError> {
    paperpaw_sdk::web::try_percent_decode(text).map_err(|_| OOM)
}

/// `href` made absolute with `base`, a page's address, as
/// [`paperpaw_sdk::web::join_url`] does it.
pub(crate) fn join_url(base: &str, href: &str) -> Result<String, DocumentError> {
    if paperpaw_sdk::document::is_external(href) {
        return string(href);
    }
    let Some((scheme, rest)) = base.split_once("://") else {
        return string(href);
    };
    if let Some(net) = href.strip_prefix("//") {
        return try_format!("{scheme}://{net}");
    }
    let host_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let host = &rest[..host_end];
    let after = &rest[host_end..];
    let path = after.split(['?', '#']).next().unwrap_or("");
    let path = if path.is_empty() { "/" } else { path };
    let joined = if href.starts_with('/') {
        string(href)?
    } else if href.starts_with('?') {
        try_format!("{path}{href}")?
    } else if href.starts_with('#') {
        let without = after.split('#').next().unwrap_or("");
        try_format!("{}{href}", if without.is_empty() { "/" } else { without })?
    } else {
        let dir = &path[..path.rfind('/').map_or(0, |i| i + 1)];
        try_format!("{dir}{href}")?
    };
    let path = normalize(&joined)?;
    try_format!("{scheme}://{host}{path}")
}

/// The path of `target` with `.` and `..` resolved, its query and fragment
/// kept.
fn normalize(target: &str) -> Result<String, DocumentError> {
    let split = target.find(['?', '#']).unwrap_or(target.len());
    let (path, tail) = target.split_at(split);
    let mut out = string_with(target.len() + 1)?;
    // The segments kept are those of `out` after its first `/`: popping one
    // is cutting back to the `/` before it.
    let segments = path.split('/').count();
    let mut kept = 0;
    for (i, segment) in path.split('/').enumerate() {
        let last = i + 1 == segments;
        match segment {
            "." => {
                if last {
                    push_part(&mut out, &mut kept, "")?;
                }
            }
            ".." => {
                if kept > 1 {
                    let cut = out.rfind('/').unwrap_or(0);
                    out.truncate(cut);
                    kept -= 1;
                }
                if last {
                    push_part(&mut out, &mut kept, "")?;
                }
            }
            segment => push_part(&mut out, &mut kept, segment)?,
        }
    }
    if !out.starts_with('/') {
        reserve_str(&mut out, 1)?;
        out.insert(0, '/');
    }
    push_str(&mut out, tail)?;
    Ok(out)
}

/// Adds `part` as one more `/`-separated segment of `out`.
fn push_part(out: &mut String, kept: &mut usize, part: &str) -> Result<(), DocumentError> {
    if *kept > 0 {
        push_char(out, '/')?;
    }
    push_str(out, part)?;
    *kept += 1;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `join_url` here and the SDK's join the same address the same way.
    #[test]
    fn join_url_is_the_sdks() {
        let bases = [
            "https://example.org",
            "https://example.org/",
            "https://example.org/a/b/c.html",
            "https://example.org/a/b/?q=1#top",
            "http://h:8080/x/y#f",
            "not an address",
            "",
        ];
        let hrefs = [
            "",
            "x",
            "../x",
            "../../../../x",
            "./x/./y",
            "/abs",
            "?q=2",
            "#frag",
            "//other.org/p",
            "mailto:a@b.c",
            "https://elsewhere.net/z",
            "d/e/../f?x#y",
        ];
        for base in bases {
            for href in hrefs {
                assert_eq!(
                    join_url(base, href).as_deref().ok(),
                    Some(paperpaw_sdk::web::join_url(base, href).as_str()),
                    "base {base:?} href {href:?}"
                );
            }
        }
    }
}
