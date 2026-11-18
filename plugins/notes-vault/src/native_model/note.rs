//! What a note is: a Markdown file. Its title is its file name.
use alloc::string::String;

use paperpaw_sdk::Storage;
use paperpaw_sdk::native::Error;

use super::text::{buffer, lossy, push, push_char};

/// Whether `name` is a note's file name (`.md`, `.markdown`).
#[must_use]
pub fn is_note(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(stem, ending)| {
        !stem.is_empty()
            && (ending.eq_ignore_ascii_case("md") || ending.eq_ignore_ascii_case("markdown"))
    })
}

/// Whether a folder of the vault is left out: those of a program's own
/// (`.obsidian`, `.trash`, `.git`), as any that begins with a dot.
#[must_use]
pub fn is_hidden(name: &str) -> bool {
    name.starts_with('.')
}

/// The title of the note at `path`: its file name without the ending.
#[must_use]
pub fn title(path: &str) -> &str {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.rsplit_once('.').map_or(name, |(stem, _)| stem)
}

/// The folder of `path`, within the vault `root`, as written for
/// showing: empty for a note in the vault's own folder.
#[must_use]
pub fn folder_in<'a>(path: &'a str, root: &str) -> &'a str {
    let dir = path.rsplit_once('/').map_or("", |(dir, _)| dir);
    let root = root.trim_matches('/');
    let below = dir
        .strip_prefix(root)
        .filter(|rest| rest.is_empty() || rest.starts_with('/'))
        .unwrap_or(dir);
    below.trim_start_matches('/')
}

/// Most of a note's beginning is shown, in characters.
pub const EXCERPT: usize = 140;
/// Bytes of a note read for its excerpt.
pub const EXCERPT_READ: usize = 2048;

/// The text of `text` past its front matter (`---` … `---` or `...`): all of
/// it where the front matter is never closed (a `---` line is then only a
/// line).
pub(crate) fn past_front_matter(mut text: &str) -> &str {
    loop {
        let Some(first) = text.split_inclusive('\n').next() else {
            return text;
        };
        if first.trim_end() != "---" {
            return text;
        }
        let rest = text.get(first.len()..).unwrap_or("");
        let mut taken = 0usize;
        for line in rest.split_inclusive('\n') {
            taken += line.len();
            if matches!(line.trim_end(), "---" | "...") {
                return rest.get(taken..).unwrap_or("");
            }
        }
        // Never closed: the first line is a line, and the rest is read the same.
        text = rest;
    }
}

/// The beginning of a note's text, as a line of plain words: past the
/// front matter, the headings and what is no text (rules, code, tables),
/// the first paragraph with its markup taken off.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn excerpt(text: &str) -> Result<String, Error> {
    let mut out = String::new();
    let mut chars = 0usize;
    let mut code = false;
    for line in past_front_matter(text).lines() {
        let line = line.trim();
        if line.starts_with("```") || line.starts_with("~~~") {
            code = !code;
            continue;
        }
        let skip = code
            || line.is_empty()
            || line.starts_with('#')
                && line
                    .get(1..)
                    .is_some_and(|rest| rest.starts_with([' ', '#']))
            || line.starts_with('|')
            || line.chars().all(|c| matches!(c, '-' | '*' | '_' | ' '));
        if skip {
            // A paragraph ends at a blank line.
            if line.is_empty() && !out.is_empty() {
                break;
            }
            continue;
        }
        if !out.is_empty() {
            push_char(&mut out, ' ')?;
            chars += 1;
        }
        plain_into(&mut out, &mut chars, line)?;
        if chars >= EXCERPT {
            break;
        }
    }
    if chars > EXCERPT {
        let cut = out
            .char_indices()
            .nth(EXCERPT)
            .map_or(out.len(), |(at, _)| at);
        out.truncate(cut);
        push_char(&mut out, '…')?;
    }
    Ok(out)
}

/// Appends the words of a line of Markdown without its signs: links as their
/// labels, emphasis and code marks and list and quote signs gone. Stops once
/// `chars` is past [`EXCERPT`] (the rest is cut anyway).
fn plain_into(out: &mut String, chars: &mut usize, line: &str) -> Result<(), Error> {
    let line = line
        .trim_start_matches(['>', ' '])
        .trim_start_matches("- ")
        .trim_start_matches("* ");
    let mut rest = line;
    fn add(out: &mut String, chars: &mut usize, text: &str) -> Result<(), Error> {
        push(out, text)?;
        *chars += text.chars().count();
        Ok(())
    }
    while let Some(c) = rest.chars().next() {
        if *chars > EXCERPT {
            break;
        }
        if let Some(inner) = rest.strip_prefix("[[")
            && let Some(end) = inner.find("]]")
        {
            let link = inner.get(..end).unwrap_or("");
            let shown = link.rsplit_once('|').map_or(link, |(_, label)| label);
            add(out, chars, shown.split('#').next().unwrap_or(shown))?;
            rest = inner.get(end + 2..).unwrap_or("");
        } else if c == '['
            && let Some((label, after)) = rest.get(1..).and_then(|r| r.split_once("]("))
            && let Some((_, tail)) = after.split_once(')')
        {
            add(out, chars, label)?;
            rest = tail;
        } else {
            if !matches!(c, '*' | '_' | '`' | '~' | '=') {
                let mut utf8 = [0u8; 4];
                add(out, chars, c.encode_utf8(&mut utf8))?;
            }
            rest = rest.get(c.len_utf8()..).unwrap_or("");
        }
    }
    Ok(())
}

/// The beginning of the note at `path`, as [`excerpt`] makes it; empty where
/// the note cannot be read.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn excerpt_of_file(storage: &dyn Storage, path: &str) -> Result<String, Error> {
    let mut bytes = buffer(EXCERPT_READ)?;
    match storage.read_at(path, 0, &mut bytes) {
        Ok(n) if n > 0 => excerpt(&lossy(bytes.get(..n).unwrap_or(&[]))?),
        _ => Ok(String::new()),
    }
}
