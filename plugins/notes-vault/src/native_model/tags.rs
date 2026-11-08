//! The tags of a note: those its front matter names and the `#tags` in
//! its first part, read once for the whole vault (see `index`).
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::Storage;
use paperpaw_sdk::native::Error;

use super::text::{buffer, copy, lossy};

/// How much of a note is read for its tags, in bytes.
pub const READ: usize = 8 * 1024;
/// Tags kept of one note.
pub const PER_NOTE: usize = 16;
/// The longest a tag is, in bytes.
pub const MAX_TAG: usize = 48;
/// Bytes read from the storage in one call.
const CHUNK: usize = 1024;

/// The tags in the beginning of the note at `path`, each once, in the
/// order they are met; none where the note cannot be read.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn of_file(storage: &dyn Storage, path: &str) -> Result<Vec<String>, Error> {
    let mut text = buffer(READ)?;
    let mut have = 0usize;
    while have < READ {
        let end = (have + CHUNK).min(READ);
        let Some(room) = text.get_mut(have..end) else {
            break;
        };
        match storage.read_at(path, have as u64, room) {
            Ok(0) | Err(_) => break,
            Ok(n) => have += n,
        }
    }
    of_text(&lossy(text.get(..have.min(READ)).unwrap_or(&[]))?)
}

/// The tags of `text`: those of its front matter (`tags: [a, b]`, or a
/// list under `tags:`), and the words in it that begin with `#` and a
/// letter, outside code.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn of_text(text: &str) -> Result<Vec<String>, Error> {
    let mut found: Vec<String> = Vec::new();
    found
        .try_reserve_exact(PER_NOTE)
        .map_err(|_| Error::NoMemory)?;
    let mut lines = text.lines().peekable();
    if lines.peek().is_some_and(|l| l.trim_end() == "---") {
        lines.next();
        // The front matter is looked at twice, not held: once to see that it
        // is closed, once for its tags.
        let closed = lines
            .clone()
            .any(|line| matches!(line.trim_end(), "---" | "..."));
        if !closed {
            // Never closed: no front matter, all of it text.
            inline_tags(text.lines(), &mut found)?;
            return Ok(found);
        }
        let mut listing = false;
        for line in lines.by_ref() {
            if matches!(line.trim_end(), "---" | "...") {
                break;
            }
            in_front_matter(line, &mut listing, &mut found)?;
        }
    }
    inline_tags(lines, &mut found)?;
    Ok(found)
}

fn in_front_matter(line: &str, listing: &mut bool, found: &mut Vec<String>) -> Result<(), Error> {
    if *listing && let Some(item) = line.trim_start().strip_prefix("- ") {
        return add(found, item);
    }
    *listing = false;
    let Some((key, value)) = line
        .split_once(':')
        .filter(|_| !line.starts_with([' ', '\t']))
    else {
        return Ok(());
    };
    let key = key.trim();
    if !(key.eq_ignore_ascii_case("tags") || key.eq_ignore_ascii_case("tag")) {
        return Ok(());
    }
    let value = value.trim().trim_start_matches('[').trim_end_matches(']');
    *listing = value.is_empty();
    for part in value.split([',', ' ']) {
        add(found, part)?;
    }
    Ok(())
}

/// The `#tags` of lines of text, not in code and not headings.
fn inline_tags<'a>(
    lines: impl Iterator<Item = &'a str>,
    found: &mut Vec<String>,
) -> Result<(), Error> {
    let mut code = false;
    for line in lines {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            code = !code;
            continue;
        }
        if code || found.len() >= PER_NOTE {
            continue;
        }
        let mut before = ' ';
        let mut ticks = false;
        for (at, c) in line.char_indices() {
            if c == '`' {
                ticks = !ticks;
            } else if c == '#' && !ticks && before.is_whitespace() {
                let rest = line.get(at + 1..).unwrap_or("");
                let end = rest
                    .find(|c: char| !(c.is_alphanumeric() || matches!(c, '-' | '_' | '/')))
                    .unwrap_or(rest.len());
                let word = rest.get(..end).unwrap_or("");
                // A tag has a letter in it: `#12` is a number, `# ` a
                // heading.
                if word.chars().any(char::is_alphabetic) {
                    add(found, word)?;
                }
            }
            before = c;
        }
    }
    Ok(())
}

/// Keeps `tag` if it is a tag not yet there: trimmed of quotes and `#`, only
/// letters, digits and `-_/` of it, at most [`MAX_TAG`] bytes, the first
/// [`PER_NOTE`] of a note.
fn add(found: &mut Vec<String>, tag: &str) -> Result<(), Error> {
    let tag = tag.trim().trim_matches(['"', '\'']).trim_start_matches('#');
    // The clean tag is made in a small buffer: a longer one is no tag.
    let mut clean = [0u8; MAX_TAG];
    let mut len = 0usize;
    for c in tag
        .chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '/'))
    {
        let mut utf8 = [0u8; 4];
        let bytes = c.encode_utf8(&mut utf8).as_bytes();
        let Some(room) = clean.get_mut(len..len + bytes.len()) else {
            return Ok(());
        };
        room.copy_from_slice(bytes);
        len += bytes.len();
    }
    let Some(clean) = clean.get(..len).and_then(|b| core::str::from_utf8(b).ok()) else {
        return Ok(());
    };
    if clean.is_empty()
        || found.len() >= PER_NOTE
        || found.iter().any(|t| t.eq_ignore_ascii_case(clean))
    {
        return Ok(());
    }
    // The room for `PER_NOTE` tags was reserved, and the count is checked above.
    found.push(copy(clean)?);
    Ok(())
}
