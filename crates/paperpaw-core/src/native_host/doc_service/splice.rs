//! Foreign elements in a block (`docs/abi/document.md` 4.1, #429): a format
//! that does not convert an element itself (a `<math>` in an EPUB) writes it
//! as a `doc_ruby` of the kind [`FOREIGN`] over the text it says for it
//! without help, its markup the ruby's text. Here, at the call level of the
//! format's answer (no deeper), the host asks the plugins that subscribe to
//! `core.convert` for the text of each and puts the first answer where the
//! format's text was: the block's text, runs, maps, cells and annotations
//! after it move. The records are never passed on: where there is no hook,
//! the hook fails, says nothing or the block does not fit after it, the
//! format's own text stays.
//!
//! Only a block that is whole in one fragment is changed (the format sees to it
//! that a block with a foreign element is small; where it is not, the
//! element is left to the format's text). Nothing is kept between calls.
use super::{DocHost, check, wire};
use crate::native_host::codec;
use crate::native_host::registrations::{RawRecord, Record};
use alloc::{string::String, vec::Vec};
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NO_MEMORY, ERR_NO_SPACE};

/// `doc_ruby.kind` of a foreign element: `3 +` the annotation kind number
/// `Annotation::FOREIGN` of the document crate.
pub(super) const FOREIGN: u32 = 3 + 0xFFF0;
/// The most elements of one block that are asked for.
const MOST: usize = 8;
/// The stage payload: a fragment is never larger.
const FRAGMENT_MAX: usize = 3_840;
/// A text part holds at most this (`doc_text_part.text`).
const PART_MAX: usize = 3_072;

/// The list being built.
struct Writer {
    bytes: Vec<u8>,
    used: usize,
}

impl Writer {
    /// Adds a record; `false` where the fragment has no room for it.
    fn put(&mut self, record: &Record<'_>) -> Result<bool, i32> {
        match codec::encode_one(record, &mut self.bytes[self.used..]) {
            Ok(written) => {
                self.used += written;
                Ok(true)
            }
            Err(ERR_NO_SPACE) => Ok(false),
            Err(error) => Err(error),
        }
    }

    /// Adds a record as it is; `false` where there is no room.
    fn copy(&mut self, frame: &[u8]) -> bool {
        match self.bytes.get_mut(self.used..self.used + frame.len()) {
            Some(room) => {
                room.copy_from_slice(frame);
                self.used += frame.len();
                true
            }
            None => false,
        }
    }
}

/// One element to replace: the range of the block's text and what stands there.
struct Hit {
    start: u32,
    end: u32,
    text: String,
}

fn vec<T: Clone>(length: usize, fill: T) -> Result<Vec<T>, i32> {
    let mut vec = Vec::new();
    vec.try_reserve_exact(length).map_err(|_| ERR_NO_MEMORY)?;
    vec.resize(length, fill);
    Ok(vec)
}

/// The name of the element a markup starts with (`<math display="block">`: `math`).
fn element(markup: &str) -> &str {
    let rest = markup.strip_prefix('<').unwrap_or("");
    let end = rest
        .find(|c: char| c.is_whitespace() || c == '>' || c == '/')
        .unwrap_or(rest.len());
    &rest[..end]
}

/// The block fragment in `out[..length]` (checked) without its foreign
/// elements: with the hooks' texts where `whole` (the one fragment of a block
/// that goes no further) and a hook answered. Returns the new length.
///
/// # Errors
/// `ERR_NO_MEMORY` where a buffer cannot be had; a block that does not pass
/// its own check afterwards is passed on as the format wrote it, minus the
/// foreign elements.
pub(super) fn foreign(
    host: &dyn DocHost,
    whole: bool,
    out: &mut [u8],
    length: usize,
) -> Result<usize, i32> {
    if !has_foreign(&out[..length])? {
        return Ok(length);
    }
    let mut hits = Vec::new();
    let mut at = 0;
    while at < length && whole {
        let (raw, used) = RawRecord::prefix(&out[at..length])?;
        at += used;
        if raw.tag() != wire::SERVICE_DOC_RUBY_TAG {
            continue;
        }
        let ruby: wire::ServiceDocRuby<'_> = raw.decode()?;
        if ruby.kind != FOREIGN || hits.len() >= MOST || ruby.start >= ruby.end {
            continue;
        }
        // A hook that fails or has nothing to say leaves the format's text.
        if let Ok(Some(text)) = host.convert(element(ruby.text), ruby.text)
            && !text.is_empty()
        {
            hits.try_reserve(1).map_err(|_| ERR_NO_MEMORY)?;
            hits.push(Hit {
                start: ruby.start,
                end: ruby.end,
                text,
            });
        }
    }
    let stripped = strip(out, length)?;
    if hits.is_empty() {
        return Ok(stripped);
    }
    if let Some((done, size)) = replace(&out[..stripped], &hits)?
        && let Some(room) = out.get_mut(..size)
        && check::block(&done[..size], 0, 0, None, |_, _| Ok(())).is_ok()
    {
        room.copy_from_slice(&done[..size]);
        return Ok(size);
    }
    // The format's own text, as it wrote it.
    Ok(stripped)
}

/// Whether the list holds a foreign element.
fn has_foreign(bytes: &[u8]) -> Result<bool, i32> {
    let mut at = 0;
    while at < bytes.len() {
        let (raw, used) = RawRecord::prefix(&bytes[at..])?;
        at += used;
        if raw.tag() == wire::SERVICE_DOC_RUBY_TAG {
            let ruby: wire::ServiceDocRuby<'_> = raw.decode()?;
            if ruby.kind == FOREIGN {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// The list without its foreign elements, in place; the new length.
fn strip(out: &mut [u8], mut length: usize) -> Result<usize, i32> {
    let mut at = 0;
    while at < length {
        let (raw, used) = RawRecord::prefix(&out[at..length])?;
        let foreign = raw.tag() == wire::SERVICE_DOC_RUBY_TAG
            && raw.decode::<wire::ServiceDocRuby<'_>>()?.kind == FOREIGN;
        if foreign {
            out.copy_within(at + used..length, at);
            length -= used;
        } else {
            at += used;
        }
    }
    Ok(length)
}

/// Where `position` of the old text is in the new one: what lies after a
/// replaced range moves with it, a place inside one goes to its start (or, for
/// the end of a range, to the end of the new text).
fn shift(position: u32, end: bool, hits: &[Hit]) -> u32 {
    let mut moved: i64 = 0;
    for hit in hits {
        let new = i64::try_from(hit.text.len()).unwrap_or(i64::MAX);
        let old = i64::from(hit.end - hit.start);
        if position <= hit.start {
            break;
        }
        if position >= hit.end {
            moved += new - old;
            continue;
        }
        let inside = i64::from(hit.start) + moved + if end { new } else { 0 };
        return u32::try_from(inside).unwrap_or(u32::MAX);
    }
    u32::try_from(i64::from(position) + moved).unwrap_or(u32::MAX)
}

/// The text with each hit's range replaced.
fn composed(old: &str, hits: &[Hit]) -> Result<Vec<u8>, i32> {
    let mut text = Vec::new();
    let extra: usize = hits.iter().map(|hit| hit.text.len()).sum();
    text.try_reserve_exact(old.len() + extra)
        .map_err(|_| ERR_NO_MEMORY)?;
    let mut from = 0;
    for hit in hits {
        text.extend_from_slice(&old.as_bytes()[from..hit.start as usize]);
        text.extend_from_slice(hit.text.as_bytes());
        from = hit.end as usize;
    }
    text.extend_from_slice(&old.as_bytes()[from..]);
    Ok(text)
}

/// The block's text, its text parts joined.
fn joined(bytes: &[u8]) -> Result<Vec<u8>, i32> {
    let mut old = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        let (raw, used) = RawRecord::prefix(&bytes[at..])?;
        at += used;
        if raw.tag() == wire::SERVICE_DOC_TEXT_PART_TAG {
            let part: wire::ServiceDocTextPart<'_> = raw.decode()?;
            old.try_reserve(part.text.len())
                .map_err(|_| ERR_NO_MEMORY)?;
            old.extend_from_slice(part.text.as_bytes());
        }
    }
    Ok(old)
}

/// The record `raw` as the longer or shorter text leaves it, if it has places
/// in the text or the text's length (the header, runs, maps, cells and
/// annotations); `None` for the others, which stay as they are.
fn moved<'a>(raw: RawRecord<'a>, new_len: u32, hits: &[Hit]) -> Result<Option<Record<'a>>, i32> {
    Ok(Some(match raw.tag() {
        wire::SERVICE_DOC_BLOCK_TAG => {
            let block: wire::ServiceDocBlock<'_> = raw.decode()?;
            Record::ServiceDocBlock(wire::ServiceDocBlock {
                text_bytes: new_len,
                ..block
            })
        }
        wire::SERVICE_DOC_RUN_TAG => {
            let run: wire::ServiceDocRun = raw.decode()?;
            let start = shift(run.start, false, hits);
            Record::ServiceDocRun(wire::ServiceDocRun {
                start,
                end: shift(run.end, true, hits).max(start),
                ..run
            })
        }
        wire::SERVICE_DOC_MAP_TAG => {
            let map: wire::ServiceDocMap = raw.decode()?;
            Record::ServiceDocMap(wire::ServiceDocMap {
                text: shift(map.text, false, hits),
                ..map
            })
        }
        wire::SERVICE_DOC_CELL_TAG => {
            let cell: wire::ServiceDocCell = raw.decode()?;
            Record::ServiceDocCell(wire::ServiceDocCell {
                end: shift(cell.end, true, hits),
                ..cell
            })
        }
        wire::SERVICE_DOC_RUBY_TAG => {
            let ruby: wire::ServiceDocRuby<'_> = raw.decode()?;
            let start = shift(ruby.start, false, hits);
            Record::ServiceDocRuby(wire::ServiceDocRuby {
                start,
                end: shift(ruby.end, true, hits).max(start),
                ..ruby
            })
        }
        _ => return Ok(None),
    }))
}

/// The text in parts of at most [`PART_MAX`] bytes, cut at characters.
fn put_text(done: &mut Writer, text: &[u8]) -> Result<bool, i32> {
    let text = core::str::from_utf8(text).map_err(|_| ERR_INVALID)?;
    let mut from = 0;
    while from < text.len() {
        let mut to = (from + PART_MAX).min(text.len());
        while !text.is_char_boundary(to) {
            to -= 1;
        }
        let part = Record::ServiceDocTextPart(wire::ServiceDocTextPart {
            text: &text[from..to],
        });
        if !done.put(&part)? {
            return Ok(false);
        }
        from = to;
    }
    Ok(true)
}

/// The block's list with the hits' texts in place of its text's ranges, in a
/// buffer of its own and its length. `None` where the hits do not fit the
/// block (unsorted, overlapping, past the text, inside a character) or the
/// block would not fit a fragment.
fn replace(bytes: &[u8], hits: &[Hit]) -> Result<Option<(Vec<u8>, usize)>, i32> {
    let old = joined(bytes)?;
    let Ok(old) = core::str::from_utf8(&old) else {
        return Ok(None);
    };
    let mut floor = 0;
    for hit in hits {
        let (start, end) = (hit.start as usize, hit.end as usize);
        if start < floor
            || end > old.len()
            || !old.is_char_boundary(start)
            || !old.is_char_boundary(end)
        {
            return Ok(None);
        }
        floor = end;
    }
    let text = composed(old, hits)?;
    let Ok(new_len) = u32::try_from(text.len()) else {
        return Ok(None);
    };
    let mut done = Writer {
        bytes: vec(FRAGMENT_MAX, 0_u8)?,
        used: 0,
    };
    let mut parts_done = false;
    let mut at = 0;
    while at < bytes.len() {
        let (raw, size) = RawRecord::prefix(&bytes[at..])?;
        let frame = &bytes[at..at + size];
        at += size;
        let fits = if raw.tag() == wire::SERVICE_DOC_TEXT_PART_TAG {
            // The new text stands where the first part was.
            let first = !core::mem::replace(&mut parts_done, true);
            !first || put_text(&mut done, &text)?
        } else if let Some(record) = moved(raw, new_len, hits)? {
            done.put(&record)?
        } else {
            // Columns, the picture, the languages and the end: as they are.
            done.copy(frame)
        };
        if !fits {
            return Ok(None);
        }
    }
    let used = done.used;
    Ok(Some((done.bytes, used)))
}
