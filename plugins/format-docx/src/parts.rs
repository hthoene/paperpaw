//! What the small parts of a Word document say: styles, lists, the
//! relationships (where pictures and links lead) and the properties (title,
//! author, language); and small helpers for its XML.

use alloc::borrow::Cow;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::Cell;

use paperpaw_sdk::Storage;
use paperpaw_sdk::document::DocumentError;
use paperpaw_sdk::xml::{Attrs, Token, Tokenizer, local_name, try_decode};
use paperpaw_sdk::zip::{Entry, ZipArchive};

use crate::mem::{self, Refused};
use crate::{
    BOLD, ITALIC, MAX_ADDRESS, MAX_LISTS, MAX_NAME, MAX_REL_BYTES, MAX_RELS, MAX_SMALL, MAX_STYLES,
    MAX_TITLE,
};

/// The error for a damaged or failing archive (its reason is a fixed text:
/// naming it takes no allocation).
pub(crate) fn error(e: paperpaw_sdk::zip::ZipError) -> DocumentError {
    use paperpaw_sdk::zip::ZipError;
    match e {
        ZipError::Storage(e) => DocumentError::Storage(e),
        ZipError::OutOfMemory => DocumentError::OutOfMemory,
        ZipError::Invalid(what) | ZipError::Unsupported(what) => {
            DocumentError::Malformed(Cow::Borrowed(what))
        }
        ZipError::TooLarge => malformed("the archive holds a part that is too large"),
        _ => malformed("the archive cannot be read"),
    }
}

pub(crate) fn malformed(message: &'static str) -> DocumentError {
    DocumentError::Malformed(Cow::Borrowed(message))
}

/// The value of attribute `name` without its prefix (`w:val` as `val`). A
/// refused allocation is latched in `oom` and reads as no attribute.
pub(crate) fn attr<'a>(attrs: &Attrs<'a>, name: &str, oom: &Cell<bool>) -> Option<Cow<'a, str>> {
    let (_, value) = attrs.iter().find(|(n, _)| local_name(n) == name)?;
    match try_decode(value) {
        Ok(value) => Some(value),
        Err(_) => {
            oom.set(true);
            None
        }
    }
}

/// `value` added to `list`; a refused allocation is latched in `oom`.
pub(crate) fn keep<T>(list: &mut Vec<T>, value: T, oom: &Cell<bool>) {
    if oom.get() {
        return;
    }
    match mem::push(list, value) {
        Ok(()) => {}
        Err(Refused) => oom.set(true),
    }
}

/// A copy of `text`; a refused allocation is latched in `oom`.
pub(crate) fn copy(text: &str, oom: &Cell<bool>) -> String {
    mem::string(text).unwrap_or_else(|_| {
        oom.set(true);
        String::new()
    })
}

/// Streams the XML part `entry` through `handle`, which says whether to go on:
/// once it does not, the rest of the part is not inflated.
pub(crate) fn xml_part(
    storage: &dyn Storage,
    archive: &ZipArchive,
    entry: &Entry,
    handle: &mut dyn FnMut(Token<'_>) -> bool,
) -> Result<(), DocumentError> {
    let mut tokenizer = Tokenizer::new();
    let mut failed = None;
    let mut going = true;
    archive
        .stream(storage, entry, &mut |chunk| {
            let fed = tokenizer.try_feed(chunk, &mut |token| {
                // What follows a stop in this chunk is dropped.
                going = going && handle(token);
            });
            match fed {
                Ok(()) => going,
                Err(_) => {
                    failed = Some(DocumentError::OutOfMemory);
                    false
                }
            }
        })
        .map_err(error)?;
    if going {
        tokenizer.finish(&mut |token| {
            handle(token);
        });
    }
    failed.map_or(Ok(()), Err)
}

/// The small part `name`, if there and not too large, streamed through
/// `handle`; a part that is missing or damaged is as if it was not there
/// (what was read of it is kept), but memory or the card failing is an error:
/// the document would be made without it, and kept so.
pub(crate) fn small_part(
    storage: &dyn Storage,
    archive: &ZipArchive,
    name: &str,
    handle: &mut dyn FnMut(Token<'_>),
) -> Result<(), DocumentError> {
    let entry = match archive.entry(storage, name) {
        Ok(Some(entry)) => entry,
        Ok(None) => return Ok(()),
        Err(e) => return skipped(e),
    };
    if entry.size > MAX_SMALL {
        return Ok(());
    }
    let mut all = |token: Token<'_>| {
        handle(token);
        true
    };
    match xml_part(storage, archive, &entry, &mut all) {
        Err(error @ (DocumentError::OutOfMemory | DocumentError::Storage(_))) => Err(error),
        _ => Ok(()),
    }
}

/// What a failure to find a small part says: only memory and the card are
/// the document's trouble.
fn skipped(e: paperpaw_sdk::zip::ZipError) -> Result<(), DocumentError> {
    match error(e) {
        error @ (DocumentError::OutOfMemory | DocumentError::Storage(_)) => Err(error),
        _ => Ok(()),
    }
}

/// What a style says: how a paragraph in it is shown.
#[derive(Debug, Default)]
pub(crate) struct Style {
    pub(crate) id: String,
    /// The heading level (1 to 6), or 0.
    pub(crate) level: u8,
    /// Its `BOLD` and `ITALIC`.
    pub(crate) format: u8,
    /// The list its paragraphs are in (a list's number and level).
    pub(crate) list: Option<(u32, u8)>,
}

/// A relationship: where `id` points.
#[derive(Debug)]
pub(crate) struct Rel {
    pub(crate) id: String,
    pub(crate) target: String,
    pub(crate) external: bool,
    pub(crate) image: bool,
}

/// What the small parts say.
#[derive(Debug, Default)]
pub(crate) struct Parts {
    pub(crate) styles: Vec<Style>,
    /// The abstract list each list is of.
    pub(crate) numbers: Vec<(u32, u32)>,
    /// Whether the level of an abstract list is numbered: (list, level,
    /// numbered).
    pub(crate) levels: Vec<(u32, u8, bool)>,
    pub(crate) rels: Vec<Rel>,
    pub(crate) rel_bytes: usize,
    pub(crate) title: String,
    pub(crate) author: String,
    pub(crate) language: String,
}

impl Parts {
    pub(crate) fn style(&self, id: &str) -> Option<&Style> {
        self.styles.iter().find(|s| s.id == id)
    }

    pub(crate) fn rel(&self, id: &str) -> Option<&Rel> {
        self.rels.iter().find(|r| r.id == id)
    }

    /// Whether list `number` at `level` is numbered (not bulleted).
    pub(crate) fn numbered(&self, number: u32, level: u8) -> bool {
        let abstract_list = self.numbers.iter().find(|(n, _)| *n == number);
        abstract_list
            .and_then(|(_, a)| {
                self.levels
                    .iter()
                    .find(|(list, l, _)| list == a && *l == level)
            })
            .is_some_and(|(_, _, numbered)| *numbered)
    }
}

/// The heading level a style's name stands for (`heading 2`, `Title`).
pub(crate) fn heading_of(name: &str) -> u8 {
    if name.eq_ignore_ascii_case("title") {
        return 1;
    }
    // `heading ` in any case, then the level.
    let Some((word, number)) = name.split_at_checked(8) else {
        return 0;
    };
    if !word.eq_ignore_ascii_case("heading ") {
        return 0;
    }
    number
        .trim()
        .parse::<u8>()
        .ok()
        .filter(|n| (1..=9).contains(n))
        .map_or(0, |n| n.min(6))
}

pub(crate) fn flag(attrs: &Attrs<'_>, oom: &Cell<bool>) -> bool {
    !attr(attrs, "val", oom).is_some_and(|v| matches!(v.as_ref(), "0" | "false" | "off" | "none"))
}

pub(crate) fn read_styles(
    storage: &dyn Storage,
    archive: &ZipArchive,
    parts: &mut Parts,
) -> Result<(), DocumentError> {
    let oom = Cell::new(false);
    let mut current = false;
    let (mut in_ppr, mut in_rpr) = (false, false);
    let read = small_part(
        storage,
        archive,
        "word/styles.xml",
        &mut |token| match token {
            Token::Start { name, attrs, .. } => match local_name(name) {
                "style" if parts.styles.len() < MAX_STYLES => {
                    let id = attr(&attrs, "styleId", &oom).unwrap_or_default();
                    current = id.len() <= MAX_NAME && !id.is_empty();
                    if current {
                        let id = copy(&id, &oom);
                        keep(
                            &mut parts.styles,
                            Style {
                                id,
                                ..Style::default()
                            },
                            &oom,
                        );
                        // A style that was not kept has no one to give to.
                        current = !oom.get();
                    }
                }
                "name" if current => {
                    if let Some(style) = parts.styles.last_mut() {
                        let name = attr(&attrs, "val", &oom).unwrap_or_default();
                        style.level = style.level.max(heading_of(&name));
                        if name.eq_ignore_ascii_case("strong") {
                            style.format |= BOLD;
                        } else if name.eq_ignore_ascii_case("emphasis") {
                            style.format |= ITALIC;
                        }
                    }
                }
                "outlineLvl" if current => {
                    let level = attr(&attrs, "val", &oom).and_then(|v| v.parse::<u8>().ok());
                    if let (Some(style), Some(level)) = (parts.styles.last_mut(), level)
                        && level < 6
                        && style.level == 0
                    {
                        style.level = level + 1;
                    }
                }
                "numId" if current && in_ppr => {
                    if let (Some(style), Some(n)) = (
                        parts.styles.last_mut(),
                        attr(&attrs, "val", &oom).and_then(|v| v.parse::<u32>().ok()),
                    ) {
                        let level = style.list.map_or(0, |(_, l)| l);
                        style.list = Some((n, level));
                    }
                }
                "ilvl" if current && in_ppr => {
                    if let (Some(style), Some(l)) = (
                        parts.styles.last_mut(),
                        attr(&attrs, "val", &oom).and_then(|v| v.parse::<u8>().ok()),
                    ) {
                        let number = style.list.map_or(0, |(n, _)| n);
                        style.list = Some((number, l));
                    }
                }
                "pPr" => in_ppr = true,
                "rPr" => in_rpr = true,
                "b" | "i" if current && in_rpr && !in_ppr && flag(&attrs, &oom) => {
                    if let Some(style) = parts.styles.last_mut() {
                        style.format |= if local_name(name) == "b" {
                            BOLD
                        } else {
                            ITALIC
                        };
                    }
                }
                _ => {}
            },
            Token::End { name } => match local_name(name) {
                "style" => current = false,
                "pPr" => in_ppr = false,
                "rPr" => in_rpr = false,
                _ => {}
            },
            _ => {}
        },
    );
    refused(read, &oom)
}

/// The result of reading a part, or `OutOfMemory` where an allocation inside
/// the handler was refused.
fn refused(read: Result<(), DocumentError>, oom: &Cell<bool>) -> Result<(), DocumentError> {
    read?;
    if oom.get() {
        return Err(DocumentError::OutOfMemory);
    }
    Ok(())
}

pub(crate) fn read_numbering(
    storage: &dyn Storage,
    archive: &ZipArchive,
    parts: &mut Parts,
) -> Result<(), DocumentError> {
    let oom = Cell::new(false);
    let (mut abstract_id, mut level, mut number): (Option<u32>, Option<u8>, Option<u32>) =
        (None, None, None);
    let read = small_part(
        storage,
        archive,
        "word/numbering.xml",
        &mut |token| match token {
            Token::Start { name, attrs, .. } => {
                let value = || attr(&attrs, "val", &oom);
                match local_name(name) {
                    "abstractNum" => {
                        abstract_id =
                            attr(&attrs, "abstractNumId", &oom).and_then(|v| v.parse().ok());
                    }
                    "lvl" => level = attr(&attrs, "ilvl", &oom).and_then(|v| v.parse().ok()),
                    "numFmt" => {
                        if let (Some(a), Some(l), Some(v)) = (abstract_id, level, value())
                            && parts.levels.len() < MAX_LISTS
                            && !parts.levels.iter().any(|e| e.0 == a && e.1 == l)
                        {
                            keep(
                                &mut parts.levels,
                                (a, l, !matches!(v.as_ref(), "bullet" | "none")),
                                &oom,
                            );
                        }
                    }
                    "num" => number = attr(&attrs, "numId", &oom).and_then(|v| v.parse().ok()),
                    "abstractNumId" if abstract_id.is_none() || number.is_some() => {
                        if let (Some(n), Some(a)) = (number, value().and_then(|v| v.parse().ok()))
                            && parts.numbers.len() < MAX_LISTS
                        {
                            keep(&mut parts.numbers, (n, a), &oom);
                        }
                    }
                    _ => {}
                }
            }
            Token::End { name } => match local_name(name) {
                "abstractNum" => (abstract_id, level) = (None, None),
                "lvl" => level = None,
                "num" => number = None,
                _ => {}
            },
            _ => {}
        },
    );
    refused(read, &oom)
}

pub(crate) fn read_rels(
    storage: &dyn Storage,
    archive: &ZipArchive,
    parts: &mut Parts,
) -> Result<(), DocumentError> {
    let oom = Cell::new(false);
    let read = small_part(
        storage,
        archive,
        "word/_rels/document.xml.rels",
        &mut |token| {
            if let Token::Start { name, attrs, .. } = token
                && local_name(name) == "Relationship"
                && parts.rels.len() < MAX_RELS
                && let (Some(id), Some(target)) =
                    (attr(&attrs, "Id", &oom), attr(&attrs, "Target", &oom))
                && id.len() <= MAX_NAME
                && target.len() <= MAX_ADDRESS
                && parts.rel_bytes + id.len() + target.len() <= MAX_REL_BYTES
            {
                parts.rel_bytes += id.len() + target.len();
                let kind = attr(&attrs, "Type", &oom).unwrap_or_default();
                let rel = Rel {
                    id: copy(&id, &oom),
                    target: copy(&target, &oom),
                    external: attr(&attrs, "TargetMode", &oom).is_some_and(|m| m == "External"),
                    image: kind.ends_with("/image"),
                };
                keep(&mut parts.rels, rel, &oom);
            }
        },
    );
    refused(read, &oom)
}

pub(crate) fn read_properties(
    storage: &dyn Storage,
    archive: &ZipArchive,
    parts: &mut Parts,
) -> Result<(), DocumentError> {
    let oom = Cell::new(false);
    let mut field: Option<u8> = None;
    let read = small_part(
        storage,
        archive,
        "docProps/core.xml",
        &mut |token| match token {
            Token::Start { name, .. } => {
                field = match local_name(name) {
                    "title" => Some(0),
                    "creator" => Some(1),
                    "language" => Some(2),
                    _ => None,
                };
            }
            Token::End { .. } => field = None,
            Token::Text(text) => {
                let into = match field {
                    Some(0) => &mut parts.title,
                    Some(1) => &mut parts.author,
                    Some(2) => &mut parts.language,
                    _ => return,
                };
                if into.len() < MAX_TITLE {
                    match try_decode(&text) {
                        Ok(text) => {
                            if mem::push_str(into, &text).is_err() {
                                oom.set(true);
                            }
                        }
                        Err(_) => oom.set(true),
                    }
                }
            }
            _ => {}
        },
    );
    refused(read, &oom)
}

/// Where the path `target` of a relationship of the document is in the
/// archive (`media/a.png` beside `document.xml`, `/word/x` from the root,
/// `../x` up). Its pieces are at most `MAX_ADDRESS / 2 + 1`.
pub(crate) fn archive_path(target: &str) -> Result<String, Refused> {
    let mut parts: Vec<&str> = Vec::new();
    parts
        .try_reserve(target.len() / 2 + 3)
        .map_err(|_| Refused)?;
    let rooted = target.starts_with('/');
    if !rooted {
        parts.push("word");
    }
    for piece in target.split('/') {
        match piece {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            piece => parts.push(piece),
        }
    }
    let mut out = String::new();
    out.try_reserve_exact(parts.iter().map(|p| p.len() + 1).sum())
        .map_err(|_| Refused)?;
    for (i, piece) in parts.iter().enumerate() {
        if i > 0 {
            out.push('/');
        }
        out.push_str(piece);
    }
    Ok(out)
}

/// The picture kind a file name says (the ones that can be shown).
pub(crate) fn picture_kind(target: &str) -> Option<&'static str> {
    let (_, ext) = target.rsplit_once('.')?;
    if ext.eq_ignore_ascii_case("png") {
        Some("png")
    } else if ext.eq_ignore_ascii_case("jpg") || ext.eq_ignore_ascii_case("jpeg") {
        Some("jpg")
    } else if ext.eq_ignore_ascii_case("gif") {
        Some("gif")
    } else {
        None
    }
}

/// Whether `target` is an address to follow from a link.
pub(crate) fn is_address(target: &str) -> bool {
    ["http://", "https://", "mailto:"].iter().any(|scheme| {
        target
            .get(..scheme.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(scheme))
    })
}

/// A bookmark's name as the name its links use: letters, digits, `_`, `-`
/// and `.`.
pub(crate) fn clean_id(name: &str) -> Result<String, Refused> {
    let mut out = String::new();
    out.try_reserve_exact(name.len().min(crate::MAX_NAME))
        .map_err(|_| Refused)?;
    for c in name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
        .take(crate::MAX_NAME)
    {
        out.push(c);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::xml_part;
    use alloc::vec::Vec;
    use core::cell::Cell;
    use paperpaw_sdk::zip::ZipArchive;
    use paperpaw_sdk::{DirEntry, MemoryStorage, Storage, StorageError};
    use paperpaw_zip::testing::ZipBuilder;

    /// Counts the bytes read from the archive.
    struct Counted(MemoryStorage, Cell<u64>);

    impl Storage for Counted {
        fn read_at(&self, path: &str, at: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
            let n = self.0.read_at(path, at, buf)?;
            self.1.set(self.1.get() + n as u64);
            Ok(n)
        }
        fn size(&self, path: &str) -> Result<u64, StorageError> {
            self.0.size(path)
        }
        fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
            self.0.write(path, data)
        }
        fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
            self.0.append(path, data)
        }
        fn list(&self, dir: &str, v: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
            self.0.list(dir, v)
        }
        fn remove(&self, path: &str) -> Result<(), StorageError> {
            self.0.remove(path)
        }
    }

    #[test]
    fn a_part_that_is_stopped_is_not_read_to_its_end() {
        let xml: Vec<u8> = core::iter::repeat_n(*b"<a/>", 200_000).flatten().collect();
        let zip = ZipBuilder::new().stored("p.xml", xml.clone()).build();
        let io = Counted(MemoryStorage::new().with("z", zip), Cell::new(0));
        let archive = ZipArchive::open(&io, "z").unwrap();
        let entry = archive.entry(&io, "p.xml").unwrap().unwrap();

        let (mut seen, before) = (0, io.1.get());
        xml_part(&io, &archive, &entry, &mut |_| {
            seen += 1;
            seen < 3
        })
        .unwrap();
        assert_eq!(seen, 3, "what follows the stop is not handed on");
        assert!(io.1.get() - before < 100_000, "{}", io.1.get() - before);

        // A part that is not stopped is read whole.
        let (mut all, before) = (0, io.1.get());
        xml_part(&io, &archive, &entry, &mut |_| {
            all += 1;
            true
        })
        .unwrap();
        assert_eq!(all, 200_000);
        assert!(io.1.get() - before >= xml.len() as u64);
    }
}
