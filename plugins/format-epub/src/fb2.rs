//! FictionBook 2 (`.fb2`): one XML file with the book's description, its
//! bodies of nested sections and its pictures as base64 `<binary>`
//! elements.
//!
//! - Metadata from `<title-info>`: title, authors, language, series
//!   (`<sequence>`), keywords as subjects, the annotation as description
//!   and the cover (`<coverpage>`).
//! - The text is converted like an EPUB chapter (see [`crate::convert`]),
//!   FictionBook's elements read as the HTML ones they look like
//!   (`<emphasis>` as `<i>`, `<epigraph>` as `<blockquote>`, …); section
//!   titles become headings by their depth and the table of contents.
//!   Notes (`<body name="notes">`) follow the text, so note links work as
//!   in EPUBs.
//! - Pictures are decoded from their `<binary>` after the text.
//! - UTF-8, Windows-1251 (common for Russian books) and Windows-1252 /
//!   ISO-8859-1, as the XML declaration says.
//!
//! What is read once (metadata, contents) is cached beside the converted
//! text, so opening a book again does not read the whole file. Zipped
//! books (`.fb2.zip`) are unpacked into the cache once.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cell::Cell;

use paperpaw_sdk::document::{
    Block, Document, DocumentError, ImageRef, Link, Locator, Metadata, TocEntry,
};
use paperpaw_sdk::image::Fit;
use paperpaw_sdk::xml::{Token, Tokenizer, decode, local_name};
use paperpaw_sdk::{Env, Storage};

use crate::PLUGIN;
use crate::cached;
use crate::convert::{self, CachePaths, INDEX, Source};

/// Changes whenever books are read differently, so they are read again.
const FB2_VERSION: u32 = 2;
/// Marks a book whose converted length is not known yet.
const UNKNOWN: u32 = u32::MAX;
/// Marks a book that could not be converted since it was opened (see
/// `document::FAILED`).
const FAILED: u32 = u32::MAX - 1;
/// Contents entries kept at most.
const MAX_TOC: u32 = 10_000;
/// Longest title of a contents entry kept.
const MAX_TITLE: usize = 200;
/// Anchor ids of section titles: this, then their number.
pub(crate) const TOC_ANCHOR: &str = "fb2-toc-";

/// How the file's bytes are text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Encoding {
    Utf8,
    /// A single-byte encoding: the characters of bytes 128 to 255.
    Single(&'static [char; 128]),
}

/// The encoding the XML declaration names (UTF-8 if none).
fn detect(head: &[u8]) -> Encoding {
    let head = String::from_utf8_lossy(&head[..head.len().min(200)]).to_ascii_lowercase();
    let name = head
        .split("encoding=")
        .nth(1)
        .and_then(|rest| {
            rest.trim_start_matches(['"', '\''])
                .split(['"', '\''])
                .next()
        })
        .unwrap_or("utf-8");
    match name {
        "windows-1251" | "cp1251" => Encoding::Single(&CP1251),
        "windows-1252" | "cp1252" | "iso-8859-1" | "latin1" => Encoding::Single(&CP1252),
        _ => Encoding::Utf8,
    }
}

/// Feeds the file at `path` to `tokenizer` as UTF-8.
pub(crate) fn feed(
    storage: &dyn Storage,
    path: &str,
    encoding: Encoding,
    tokenizer: &mut Tokenizer,
    emit: &mut dyn FnMut(Token<'_>),
) -> Result<(), DocumentError> {
    let mut chunk = [0u8; 1024];
    let mut utf8 = Vec::new();
    if matches!(encoding, Encoding::Single(_)) {
        utf8.try_reserve_exact(3 * chunk.len())
            .map_err(|_| DocumentError::OutOfMemory)?;
    }
    let mut at = 0u64;
    loop {
        let n = storage.read_at(path, at, &mut chunk)?;
        if n == 0 {
            break;
        }
        at += n as u64;
        let fed = match encoding {
            Encoding::Utf8 => tokenizer.try_feed(&chunk[..n], emit),
            Encoding::Single(table) => {
                utf8.clear();
                let mut buf = [0u8; 4];
                for &b in &chunk[..n] {
                    if b < 128 {
                        utf8.push(b);
                    } else {
                        let c = table[usize::from(b - 128)];
                        utf8.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
                    }
                }
                tokenizer.try_feed(&utf8, emit)
            }
        };
        fed.map_err(|_| DocumentError::OutOfMemory)?;
    }
    tokenizer.finish(emit);
    Ok(())
}

/// The value of an attribute by its name without prefix (`l:href`,
/// `xlink:href`).
pub(crate) fn attr(attrs: &paperpaw_sdk::xml::Attrs<'_>, name: &str) -> Option<String> {
    attrs
        .iter()
        .find(|(n, _)| local_name(n) == name)
        .map(|(_, v)| decode(v).into_owned())
}

/// What one look at the book finds. Its contents are written to the cache
/// as they are found (`toc.dat`, see `package`), each entry with the
/// anchor its title gets ([`TOC_ANCHOR`] and its number).
#[derive(Debug, Default)]
struct Scan {
    metadata: Metadata,
    /// Entries of the contents.
    toc_len: u32,
    cover: Option<String>,
}

/// Elements whose names a look at the book keeps open at most: a book's
/// description is never deep, its text may be.
const MAX_SCAN_DEPTH: usize = 64;
/// Element names are kept to this length.
const MAX_SCAN_NAME: usize = 32;
/// Text of a metadata element and the annotation read at most.
const MAX_SCAN_TEXT: usize = 4096;
/// Authors and keywords kept at most.
const MAX_AUTHORS: usize = 16;
const MAX_SUBJECTS: usize = 32;
/// Longest value of the metadata kept, but the annotation.
const MAX_VALUE: usize = 512;

/// A look at the book, element by element. Section titles are counted as
/// [`Fb2Dialect`] counts them when the book is converted, also those of
/// self-closing elements, so each entry of the contents leads to its
/// title.
struct Scanner<'a> {
    found: Scan,
    toc: crate::package::TocWriter<'a>,
    /// The names of the open elements, as far as kept, and how many more
    /// are open below them.
    stack: Vec<String>,
    overflow: u32,
    /// Where `<title-info>` is in `stack`, while it is open.
    info: Option<usize>,
    text: String,
    /// An author's name parts, a title being read and where sections are.
    name_parts: Vec<String>,
    nickname: String,
    title: Option<String>,
    /// Section titles met (as [`Fb2Dialect::titles`]).
    titles: u32,
    depth: u32,
    notes: bool,
    annotation: Option<String>,
}

impl Scanner<'_> {
    fn start(&mut self, name: &str, attrs: &paperpaw_sdk::xml::Attrs<'_>) {
        let tag = scan_name(name);
        let in_info = self.info.is_some();
        match tag.as_str() {
            "body" => self.notes = attrs.get("name").as_deref() == Some("notes"),
            "section" => self.depth += 1,
            "title" => {
                // As the dialect gives titles anchors.
                if !self.notes && self.depth > 0 {
                    self.title = Some(String::new());
                }
            }
            "sequence" if in_info && self.found.metadata.series.is_none() => {
                let m = &mut self.found.metadata;
                m.series = attr(attrs, "name")
                    .map(|n| value(&n))
                    .filter(|n| !n.is_empty());
                m.series_index = attr(attrs, "number")
                    .map(|n| value(&n))
                    .filter(|n| !n.is_empty() && n != "0");
            }
            "image" if in_info && self.stack.last().is_some_and(|t| t == "coverpage") => {
                self.found.cover = attr(attrs, "href")
                    .map(|h| value(h.trim_start_matches('#')))
                    .filter(|h| !h.is_empty());
            }
            "annotation" if in_info => self.annotation = Some(String::new()),
            "author" if in_info => {
                self.name_parts.clear();
                self.nickname.clear();
            }
            _ => {}
        }
        if self.stack.len() < MAX_SCAN_DEPTH {
            if tag == "title-info" && self.info.is_none() {
                self.info = Some(self.stack.len());
            }
            self.stack.push(tag);
        } else {
            self.overflow += 1;
        }
        self.text.clear();
    }

    fn end(&mut self, name: &str) {
        let tag = scan_name(name);
        let in_info = self.info.is_some();
        let value = value(&self.text);
        let m = &mut self.found.metadata;
        match tag.as_str() {
            "book-title" if in_info => m.title = Some(value),
            "lang" if in_info && !value.is_empty() => m.language = Some(value),
            "keywords" if in_info => {
                m.subjects = value
                    .split([',', ';'])
                    .map(str::trim)
                    .filter(|k| !k.is_empty())
                    .take(MAX_SUBJECTS)
                    .map(String::from)
                    .collect();
            }
            "first-name" | "middle-name" | "last-name" if in_info && !value.is_empty() => {
                self.name_parts.push(value);
            }
            "nickname" if in_info => self.nickname = value,
            "author" if in_info && m.authors.len() < MAX_AUTHORS => {
                let full = if self.name_parts.is_empty() {
                    core::mem::take(&mut self.nickname)
                } else {
                    self.name_parts.join(" ")
                };
                if !full.is_empty() {
                    m.authors.push(full);
                }
            }
            "annotation" if in_info => {
                m.description = self.annotation.take().map(|a| clean(&a));
            }
            "section" => self.depth = self.depth.saturating_sub(1),
            "title" => {
                if let Some(title) = self.title.take() {
                    let anchor = format!("{TOC_ANCHOR}{}", self.titles);
                    self.titles += 1;
                    let title = clean(&title);
                    // A title without words (a picture) is no entry.
                    if !title.is_empty() && self.found.toc_len < MAX_TOC {
                        let level = (self.depth.saturating_sub(1)).min(5) as u8;
                        self.toc
                            .push(level, 0, Some(&anchor), cut(&title, MAX_TITLE));
                        self.found.toc_len += 1;
                    }
                }
            }
            // Lines of a title or an annotation are parted.
            "p" => {
                for open in [self.title.as_mut(), self.annotation.as_mut()]
                    .into_iter()
                    .flatten()
                {
                    open.push(' ');
                }
            }
            _ => {}
        }
        if self.overflow > 0 {
            self.overflow -= 1;
        } else if let Some(at) = self.stack.iter().rposition(|t| *t == tag) {
            self.stack.truncate(at);
            if self.info.is_some_and(|info| info >= at) {
                self.info = None;
            }
        }
        self.text.clear();
    }

    fn characters(&mut self, text: &str) {
        // Pictures are long and of no use here.
        if self.stack.last().is_some_and(|t| t == "binary") {
            return;
        }
        let text = decode(text);
        for open in [
            Some(&mut self.text),
            self.title.as_mut(),
            self.annotation.as_mut(),
        ]
        .into_iter()
        .flatten()
        {
            if open.len() < MAX_SCAN_TEXT {
                open.push_str(&text);
            }
        }
    }
}

/// An element's name as a look at the book keeps it.
fn scan_name(name: &str) -> String {
    String::from(cut(local_name(name), MAX_SCAN_NAME))
}

/// `text` cut to at most `max` bytes, where a character ends.
fn cut(text: &str, max: usize) -> &str {
    let mut end = text.len().min(max);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// A value of the metadata: whitespace collapsed, at most [`MAX_VALUE`]
/// bytes.
fn value(text: &str) -> String {
    String::from(cut(&clean(text), MAX_VALUE))
}

/// Looks at the book once: its metadata, and its contents written to the
/// cache folder `cache`.
fn scan(
    storage: &dyn Storage,
    path: &str,
    encoding: Encoding,
    cache: &str,
) -> Result<Scan, DocumentError> {
    let mut scanner = Scanner {
        found: Scan::default(),
        toc: crate::package::TocWriter::new(storage, cache)?,
        stack: Vec::new(),
        overflow: 0,
        info: None,
        text: String::new(),
        name_parts: Vec::new(),
        nickname: String::new(),
        title: None,
        titles: 0,
        depth: 0,
        notes: false,
        annotation: None,
    };
    let mut handle = |token: Token<'_>| match token {
        Token::Start {
            name,
            attrs,
            self_closing,
        } => {
            scanner.start(name, &attrs);
            if self_closing {
                scanner.end(name);
            }
        }
        Token::End { name } => scanner.end(name),
        Token::Text(text) => scanner.characters(&text),
        Token::CData(_) => {}
    };
    let mut tokenizer = Tokenizer::new();
    feed(storage, path, encoding, &mut tokenizer, &mut handle)?;
    let Scanner { mut found, toc, .. } = scanner;
    found.toc_len = toc.finish()?;
    Ok(found)
}

/// Whitespace collapsed.
fn clean(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Decodes base64 piece by piece; other characters are skipped.
#[derive(Debug, Default)]
struct Base64 {
    acc: u32,
    bits: u32,
}

impl Base64 {
    fn feed(&mut self, text: &str, out: &mut Vec<u8>) {
        for b in text.bytes() {
            let v = match b {
                b'A'..=b'Z' => b - b'A',
                b'a'..=b'z' => b - b'a' + 26,
                b'0'..=b'9' => b - b'0' + 52,
                b'+' | b'-' => 62,
                b'/' | b'_' => 63,
                _ => continue,
            };
            self.acc = (self.acc << 6) | u32::from(v);
            self.bits += 6;
            if self.bits >= 8 {
                self.bits -= 8;
                out.push((self.acc >> self.bits) as u8);
                self.acc &= (1 << self.bits) - 1;
            }
        }
    }
}

/// Decodes the pictures `wanted` (binary id, cache file, size)
/// from their `<binary>` elements: each into a temporary file, then into
/// the cache. One that cannot be decoded is left out.
pub(crate) fn decode_binaries(
    storage: &dyn Storage,
    path: &str,
    encoding: Encoding,
    wanted: &[(String, String, Fit)],
    dir: &str,
    color: bool,
) -> Result<(), DocumentError> {
    let wanted: Vec<&(String, String, Fit)> = wanted
        .iter()
        .filter(|(_, cache, _)| paperpaw_sdk::image::ImageFile::open(storage, cache).is_none())
        .collect();
    if wanted.is_empty() {
        return Ok(());
    }
    let temp = format!("{dir}/binary.tmp");
    let mut current: Option<usize> = None;
    let mut base64 = Base64::default();
    let mut bytes = Vec::new();
    let mut done: Vec<usize> = Vec::new();
    let mut failed = None;
    let mut handle = |token: Token<'_>| match token {
        Token::Start { name, attrs, .. } if local_name(name) == "binary" => {
            let id = attrs.get("id").unwrap_or_default();
            // The first binary of an id that decodes is the picture; a
            // later one of the same id is not decoded over it.
            current = wanted
                .iter()
                .position(|(w, ..)| *w == id)
                .filter(|at| !done.contains(at));
            if current.is_some() {
                base64 = Base64::default();
                failed = storage.write(&temp, &[]).err().or(failed.take());
            }
        }
        Token::Text(text) if current.is_some() => {
            bytes.clear();
            base64.feed(&text, &mut bytes);
            if failed.is_none() {
                failed = storage.append(&temp, &bytes).err();
            }
        }
        Token::End { name } if local_name(name) == "binary" => {
            if let Some(at) = current.take() {
                let (_, cache, fit) = wanted[at];
                if failed.is_none()
                    && convert::decode_file(storage, &temp, cache, *fit, color).is_ok()
                {
                    done.push(at);
                }
            }
        }
        _ => {}
    };
    let mut tokenizer = Tokenizer::new();
    feed(storage, path, encoding, &mut tokenizer, &mut handle)?;
    let _ = storage.remove(&temp);
    match failed {
        Some(e) => Err(DocumentError::Storage(e)),
        None => Ok(()),
    }
}

/// An open FictionBook.
#[derive(Debug)]
pub struct Fb2Document {
    path: String,
    paths: CachePaths,
    /// The block found last (see [`cached::block_at`]).
    last: core::cell::Cell<Option<u32>>,
    /// Where an anchor was found last.
    anchors: cached::AnchorHint,
    encoding: Encoding,
    metadata: Metadata,
    /// Entries of the contents, kept in the cache (see [`Scan`]).
    toc_len: u32,
    cover: Option<String>,
    styles: bool,
    /// Pictures in colour (ADR 0015).
    color: bool,
    /// Pictures decoded at most this large.
    image_fit: Fit,
    /// The cover decoded at most this large.
    cover_fit: Fit,
    /// Size of the file: the length until it is converted.
    size: u32,
    length: Cell<u32>,
    /// Its text length and number of blocks once converted: the cache
    /// is not looked at again while it is open.
    known: Cell<Option<(u32, u32)>>,
}

impl Fb2Document {
    /// Opens a FictionBook; without `styles` it is converted into a cache
    /// of its own (FictionBook has no style sheets, but the setting keeps
    /// caches apart).
    pub fn open(storage: &dyn Storage, path: &str, styles: bool) -> Result<Self, DocumentError> {
        let options = paperpaw_sdk::OpenOptions::default().publisher_styles(styles);
        Self::open_options(storage, path, &options)
    }

    /// Opens a FictionBook as `options` say (styles, colour pictures).
    pub fn open_options(
        storage: &dyn Storage,
        path: &str,
        options: &paperpaw_sdk::OpenOptions,
    ) -> Result<Self, DocumentError> {
        let (styles, color) = (options.publisher_styles, options.color);
        let (image_fit, cover_fit) = (options.image_fit, options.cover_fit);
        let size = storage.size(path)?;
        let book = crate::fingerprint(storage, path)?;
        let mut cache = format!(
            "{}/fb2-{}",
            Env::plugin_dir(PLUGIN.as_str()),
            paperpaw_sdk::library::key(path)
        );
        if !styles {
            cache.push_str("/plain");
        }
        let paths = CachePaths::new(&cache, "0");
        // Read again when the file or the way it is read changed, and then
        // nothing made of it before stays (its pictures).
        let stamp = format!("{}.{FB2_VERSION} {book}\n", crate::package::CACHE_VERSION);
        let stamp_path = format!("{cache}/source");
        let info_path = format!("{cache}/book.txt");
        let fresh = paperpaw_sdk::read_file(storage, &stamp_path, 256).ok()
            == Some(stamp.clone().into_bytes());
        if !fresh {
            let _ = storage.remove_dir_all(&cache);
        }
        // A zipped book is read from its unpacked copy.
        let zipped = path.to_ascii_lowercase().ends_with(".zip");
        let file = if zipped {
            let unpacked = format!("{cache}/book.fb2");
            if !fresh || storage.size(&unpacked).is_err() {
                let _ = storage.create_dir(&cache);
                unpack(storage, path, &unpacked)?;
            }
            unpacked
        } else {
            path.to_string()
        };
        let encoding = detect(&crate::head(storage, &file)?);
        let found = match fresh.then(|| load_info(storage, &info_path)).flatten() {
            Some(found) => found,
            None => {
                let _ = storage.remove(&paths.end);
                let _ = storage.create_dir(&cache);
                let found = scan(storage, &file, encoding, &cache)?;
                storage.write(&info_path, save_info(&found).as_bytes())?;
                storage.write(&stamp_path, stamp.as_bytes())?;
                found
            }
        };
        let mut metadata = found.metadata;
        if metadata.title.is_none() {
            let name = path.rsplit('/').next().unwrap_or(path);
            metadata.title = Some(name.rsplit_once('.').map_or(name, |(s, _)| s).to_string());
        }
        Ok(Self {
            path: file,
            paths,
            last: core::cell::Cell::new(None),
            anchors: cached::AnchorHint::default(),
            encoding,
            metadata,
            toc_len: found.toc_len,
            cover: found.cover,
            styles,
            color,
            image_fit,
            cover_fit,
            size: u32::try_from(size).unwrap_or(u32::MAX - 1),
            length: Cell::new(UNKNOWN),
            known: Cell::new(None),
        })
    }

    /// Converts the book if needed; returns its text length and number of
    /// blocks.
    fn converted(&self, storage: &dyn Storage) -> Result<(u32, u32), DocumentError> {
        if let Some(known) = self.known.get() {
            return Ok(known);
        }
        let length = match cached::converted_length(storage, &self.paths) {
            Some(length) => length,
            None if self.length.get() == FAILED => {
                return Err(DocumentError::Malformed(alloc::borrow::Cow::Borrowed(
                    "cannot be read",
                )));
            }
            None => {
                self.anchors.reset();
                let source = Source::Fb2 {
                    path: &self.path,
                    encoding: self.encoding,
                };
                let converted = convert::convert(
                    storage,
                    &source,
                    &self.paths,
                    Default::default(),
                    self.styles,
                    self.color,
                    self.image_fit,
                    convert::BookText::of(self.metadata.language.as_deref()),
                );
                converted.inspect_err(|_| self.length.set(FAILED))?
            }
        };
        self.length.set(length);
        let blocks = (storage.size(&self.paths.index)? / u64::from(INDEX)) as u32;
        self.known.set(Some((length, blocks)));
        Ok((length, blocks))
    }
}

/// Unpacks the first `.fb2` of the archive at `zip` into `to`.
fn unpack(storage: &dyn Storage, zip: &str, to: &str) -> Result<(), DocumentError> {
    use paperpaw_sdk::zip::ZipArchive;
    let bad = |_| DocumentError::Malformed(alloc::borrow::Cow::Borrowed("zip"));
    let archive = ZipArchive::open(storage, zip).map_err(bad)?;
    let entry = archive
        .entries(storage)
        .filter_map(Result::ok)
        .find(|e| e.name.to_ascii_lowercase().ends_with(".fb2"))
        .ok_or(DocumentError::Malformed(alloc::borrow::Cow::Borrowed(
            "no FictionBook in the archive",
        )))?;
    storage.write(to, &[])?;
    let mut failed = None;
    archive
        .stream(storage, &entry, &mut |chunk| {
            failed = storage.append(to, chunk).err();
            failed.is_none()
        })
        .map_err(bad)?;
    match failed {
        Some(e) => Err(DocumentError::Storage(e)),
        None => Ok(()),
    }
}

/// The cached metadata and the number of entries of the contents:
/// `key<TAB>value` lines.
fn save_info(found: &Scan) -> String {
    let one = |s: &str| s.replace(['\t', '\n', '\r'], " ");
    let m = &found.metadata;
    let mut out = format!("toc\t{}\n", found.toc_len);
    let mut line = |key: &str, value: &str| {
        out.push_str(&format!("{key}\t{}\n", one(value)));
    };
    for (key, value) in [
        ("title", &m.title),
        ("language", &m.language),
        ("series", &m.series),
        ("series-index", &m.series_index),
        ("description", &m.description),
        ("cover", &found.cover),
    ] {
        if let Some(value) = value {
            line(key, value);
        }
    }
    for author in &m.authors {
        line("author", author);
    }
    for subject in &m.subjects {
        line("subject", subject);
    }
    out
}

/// Longest `book.txt` read: what [`Scan`] keeps is far less.
const MAX_INFO: usize = 32 * 1024;

fn load_info(storage: &dyn Storage, path: &str) -> Option<Scan> {
    let data = paperpaw_sdk::read_file(storage, path, MAX_INFO).ok()?;
    let text = core::str::from_utf8(&data).ok()?;
    let mut found = Scan::default();
    for line in text.lines() {
        let (key, value) = line.split_once('\t')?;
        let value = value.to_string();
        let m = &mut found.metadata;
        match key {
            "title" => m.title = Some(value),
            "language" => m.language = Some(value),
            "series" => m.series = Some(value),
            "series-index" => m.series_index = Some(value),
            "description" => m.description = Some(value),
            "cover" => found.cover = Some(value),
            "author" => m.authors.push(value),
            "subject" => m.subjects.push(value),
            "toc" => found.toc_len = value.parse().ok()?,
            _ => {}
        }
    }
    Some(found)
}

impl Document for Fb2Document {
    fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    fn sections(&self) -> u32 {
        1
    }

    fn section_len(&self, _: u32) -> u32 {
        match self.length.get() {
            UNKNOWN | FAILED => self.size,
            length => length,
        }
    }

    fn block_at(&self, storage: &dyn Storage, at: Locator) -> Result<Option<Block>, DocumentError> {
        if at.section != 0 {
            return Ok(None);
        }
        let (length, blocks) = self.converted(storage)?;
        let mut last = self.last.get();
        let block = cached::block_at(
            storage,
            &self.paths,
            length,
            blocks,
            at.offset,
            self.color,
            &mut last,
        );
        self.last.set(last);
        block
    }

    fn toc_len(&self, _: &dyn Storage) -> u32 {
        self.toc_len
    }

    fn toc_entry(&self, storage: &dyn Storage, index: u32) -> Option<TocEntry> {
        let entry = crate::package::toc_entry(storage, &self.paths.dir, index)?;
        Some(TocEntry {
            title: entry.title,
            level: entry.level,
            at: self.toc_target(storage, index).unwrap_or(Locator::START),
        })
    }

    fn toc_target(&self, storage: &dyn Storage, index: u32) -> Option<Locator> {
        let entry = crate::package::toc_entry(storage, &self.paths.dir, index)?;
        self.converted(storage).ok()?;
        let id = entry.fragment?;
        cached::anchor(storage, &self.paths, 0, &id, &self.anchors)
            .map(|offset| Locator::new(0, offset))
    }

    fn cover(&self, storage: &dyn Storage) -> Result<Option<ImageRef>, DocumentError> {
        let Some(id) = &self.cover else {
            return Ok(None);
        };
        let base = format!("{}/cover-{}", self.paths.dir, self.cover_fit.long);
        if crate::pictures::failed(storage, &base) {
            return Ok(None);
        }
        let cache = crate::pictures::file(&base, self.color);
        decode_binaries(
            storage,
            &self.path,
            self.encoding,
            &[(id.clone(), cache, self.cover_fit)],
            &self.paths.dir,
            self.color,
        )?;
        let cover = crate::pictures::open(storage, &base, self.color);
        if cover.is_none() {
            // Not in the book, or not decodable: the whole book is not read
            // through again for it each time it is listed.
            let missing = DocumentError::Malformed(alloc::borrow::Cow::Borrowed("cover"));
            crate::pictures::note_failure(storage, &base, &missing);
        }
        Ok(cover)
    }

    fn links(&self, storage: &dyn Storage, section: u32, from: u32, to: u32) -> Vec<Link> {
        if section != 0 || self.converted(storage).is_err() {
            return Vec::new();
        }
        cached::links(storage, &self.paths, from, to)
    }

    fn link_target(&self, storage: &dyn Storage, href: &str) -> Option<Locator> {
        let id = href.rsplit_once('#').map_or(href, |(_, id)| id);
        self.converted(storage).ok()?;
        cached::anchor(storage, &self.paths, 0, id, &self.anchors)
            .map(|offset| Locator::new(0, offset))
    }
}

/// Windows-1251 (Cyrillic), bytes 128 to 255.
static CP1251: [char; 128] = [
    'Ђ', 'Ѓ', '‚', 'ѓ', '„', '…', '†', '‡', '€', '‰', 'Љ', '‹', 'Њ', 'Ќ', 'Ћ', 'Џ', //
    'ђ', '‘', '’', '“', '”', '•', '–', '—', '\u{fffd}', '™', 'љ', '›', 'њ', 'ќ', 'ћ', 'џ', //
    '\u{a0}', 'Ў', 'ў', 'Ј', '¤', 'Ґ', '¦', '§', 'Ё', '©', 'Є', '«', '¬', '\u{ad}', '®',
    'Ї', //
    '°', '±', 'І', 'і', 'ґ', 'µ', '¶', '·', 'ё', '№', 'є', '»', 'ј', 'Ѕ', 'ѕ', 'ї', //
    'А', 'Б', 'В', 'Г', 'Д', 'Е', 'Ж', 'З', 'И', 'Й', 'К', 'Л', 'М', 'Н', 'О', 'П', //
    'Р', 'С', 'Т', 'У', 'Ф', 'Х', 'Ц', 'Ч', 'Ш', 'Щ', 'Ъ', 'Ы', 'Ь', 'Э', 'Ю', 'Я', //
    'а', 'б', 'в', 'г', 'д', 'е', 'ж', 'з', 'и', 'й', 'к', 'л', 'м', 'н', 'о', 'п', //
    'р', 'с', 'т', 'у', 'ф', 'х', 'ц', 'ч', 'ш', 'щ', 'ъ', 'ы', 'ь', 'э', 'ю', 'я', //
];

/// Windows-1252 (Western), bytes 128 to 255 (ISO-8859-1 where it has
/// characters).
static CP1252: [char; 128] = [
    '€', '\u{fffd}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{fffd}', 'Ž',
    '\u{fffd}', '\u{fffd}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{fffd}',
    'ž', 'Ÿ', '\u{a0}', '¡', '¢', '£', '¤', '¥', '¦', '§', '¨', '©', 'ª', '«', '¬', '\u{ad}', '®',
    '¯', '°', '±', '²', '³', '´', 'µ', '¶', '·', '¸', '¹', 'º', '»', '¼', '½', '¾', '¿', 'À', 'Á',
    'Â', 'Ã', 'Ä', 'Å', 'Æ', 'Ç', 'È', 'É', 'Ê', 'Ë', 'Ì', 'Í', 'Î', 'Ï', 'Ð', 'Ñ', 'Ò', 'Ó', 'Ô',
    'Õ', 'Ö', '×', 'Ø', 'Ù', 'Ú', 'Û', 'Ü', 'Ý', 'Þ', 'ß', 'à', 'á', 'â', 'ã', 'ä', 'å', 'æ', 'ç',
    'è', 'é', 'ê', 'ë', 'ì', 'í', 'î', 'ï', 'ð', 'ñ', 'ò', 'ó', 'ô', 'õ', 'ö', '÷', 'ø', 'ù', 'ú',
    'û', 'ü', 'ý', 'þ', 'ÿ',
];

/// A FictionBook read as HTML: sections are headings by their depth,
/// their titles get anchors for the contents ([`TOC_ANCHOR`]), and
/// pictures are the book's `<binary>` elements by their ids.
#[derive(Debug, Default)]
pub(crate) struct Fb2Dialect {
    /// Depth of nested sections.
    depth: u32,
    /// Inside a `<title>`.
    in_title: bool,
    /// Inside the notes (`<body name="notes">`).
    notes: bool,
    /// Section titles met so far, for their anchors.
    titles: u32,
}

impl Fb2Dialect {
    /// The HTML element a FictionBook element is read as.
    fn name(&self, tag: &str) -> String {
        let name = match tag {
            "description" | "binary" | "stylesheet" => "head",
            "title" => return format!("h{}", (self.depth + 1).min(6)),
            "p" if self.in_title => "span",
            "subtitle" => "h5",
            "epigraph" | "cite" => "blockquote",
            "poem" | "stanza" => "div",
            "v" | "text-author" | "empty-line" => "p",
            "emphasis" => "i",
            "strong" => "b",
            "strikethrough" => "s",
            // A named style of a few words, not a stylesheet.
            "style" => "span",
            tag => tag,
        };
        name.to_string()
    }
}

impl crate::convert::Dialect for Fb2Dialect {
    fn enter(
        &mut self,
        tag: &str,
        attrs: &paperpaw_sdk::xml::Attrs<'_>,
    ) -> crate::convert::Entered {
        let mut entered = crate::convert::Entered::default();
        match tag {
            "body" => self.notes = attrs.get("name").as_deref() == Some("notes"),
            "section" => self.depth += 1,
            "title" => {
                self.in_title = true;
                if !self.notes && self.depth > 0 {
                    // Where the contents entry leads.
                    entered.anchor = Some(format!("{TOC_ANCHOR}{}", self.titles));
                    self.titles += 1;
                }
            }
            // The lines of a title.
            "p" if self.in_title => entered.line_break = true,
            _ => {}
        }
        entered.name = self.name(tag);
        entered
    }

    fn leave(&mut self, tag: &str) -> String {
        let name = self.name(tag);
        match tag {
            "section" => self.depth = self.depth.saturating_sub(1),
            "title" => self.in_title = false,
            _ => {}
        }
        name
    }

    fn picture(&self, href: &str) -> Option<String> {
        Some(href.trim_start_matches('#').to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_sdk::MemoryStorage;
    use paperpaw_sdk::document::BlockKind;

    const BOOK: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<FictionBook xmlns="http://www.gribuser.ru/xml/fictionbook/2.0" xmlns:l="http://www.w3.org/1999/xlink">
<description><title-info>
  <genre>sf</genre>
  <author><first-name>Ann</first-name><last-name>Example</last-name></author>
  <book-title>A   Test</book-title>
  <annotation><p>What it is</p><p>about.</p></annotation>
  <keywords>space, ships</keywords>
  <lang>en</lang>
  <coverpage><image l:href="#cover.png"/></coverpage>
  <sequence name="Tests" number="2"/>
</title-info></description>
<body>
  <title><p>A Test</p></title>
  <section><title><p>Chapter 1</p><p>The start</p></title>
    <epigraph><p>Words.</p><text-author>Someone</text-author></epigraph>
    <p>Some <emphasis>text</emphasis> and a note<a l:href="#n1" type="note">1</a>.</p>
    <empty-line/>
    <poem><stanza><v>Line one</v><v>Line two</v></stanza></poem>
    <section><title><p>Part 1.1</p></title><p>Inner.</p></section>
  </section>
</body>
<body name="notes"><title><p>Notes</p></title>
  <section id="n1"><title><p>1</p></title><p>The note.</p></section>
</body>
<binary id="cover.png" content-type="image/png">iVBORw0KGgo=</binary>
</FictionBook>"##;

    fn texts(doc: &Fb2Document, storage: &dyn Storage) -> Vec<(BlockKind, String)> {
        let mut out = Vec::new();
        let mut at = Locator::START;
        while let Some(block) = doc.block_at(storage, at).unwrap() {
            out.push((block.kind, block.text.clone()));
            at = Locator::new(0, block.end);
        }
        out
    }

    #[test]
    fn reads_metadata_text_contents_and_notes() {
        let storage = MemoryStorage::new().with("b/test.fb2", BOOK.as_bytes().to_vec());
        let doc = Fb2Document::open(&storage, "b/test.fb2", true).unwrap();
        let m = doc.metadata();
        assert_eq!(m.title.as_deref(), Some("A Test"));
        assert_eq!(m.authors, ["Ann Example"]);
        assert_eq!(m.language.as_deref(), Some("en"));
        assert_eq!(m.series.as_deref(), Some("Tests"));
        assert_eq!(m.series_index.as_deref(), Some("2"));
        assert_eq!(m.subjects, ["space", "ships"]);
        assert_eq!(m.description.as_deref(), Some("What it is about."));
        let blocks = texts(&doc, &storage);
        let only: Vec<&str> = blocks.iter().map(|(_, t)| t.as_str()).collect();
        assert_eq!(
            only,
            [
                "A Test",
                "Chapter 1\nThe start",
                "Words.",
                "Someone",
                "Some text and a note1.",
                "Line one",
                "Line two",
                "Part 1.1",
                "Inner.",
                "Notes",
                "1",
                "The note."
            ]
        );
        assert_eq!(blocks[0].0, BlockKind::Heading(1));
        assert_eq!(blocks[1].0, BlockKind::Heading(2));
        assert_eq!(blocks[7].0, BlockKind::Heading(3));
        // The contents: the sections of the text, not the notes.
        assert_eq!(doc.toc_len(&storage), 2);
        let first = doc.toc_entry(&storage, 0).unwrap();
        assert_eq!(
            (first.title.as_str(), first.level),
            ("Chapter 1 The start", 0)
        );
        let second = doc.toc_entry(&storage, 1).unwrap();
        assert_eq!(second.level, 1);
        let block = doc.block_at(&storage, second.at).unwrap().unwrap();
        assert_eq!(block.text, "Part 1.1");
        // The note link leads to the note.
        let links = doc.links(&storage, 0, 0, u32::MAX);
        assert_eq!(links.len(), 1);
        let note = doc.link_target(&storage, &links[0].href).unwrap();
        let block = doc.block_at(&storage, note).unwrap().unwrap();
        assert_eq!(block.text, "1");
        // Opened again: from the cache.
        let again = Fb2Document::open(&storage, "b/test.fb2", true).unwrap();
        assert_eq!(again.metadata().authors, ["Ann Example"]);
        assert_eq!(again.toc_len(&storage), 2);
    }

    #[test]
    fn reads_windows_1251() {
        let mut book = Vec::new();
        book.extend_from_slice(
            b"<?xml version=\"1.0\" encoding=\"windows-1251\"?><FictionBook><description>\
              <title-info><book-title>",
        );
        // "Мир" in Windows-1251.
        book.extend_from_slice(&[0xCC, 0xE8, 0xF0]);
        book.extend_from_slice(b"</book-title></title-info></description><body><section><p>");
        book.extend_from_slice(&[0xCC, 0xE8, 0xF0]);
        book.extend_from_slice(b"</p></section></body></FictionBook>");
        let storage = MemoryStorage::new().with("r.fb2", book);
        let doc = Fb2Document::open(&storage, "r.fb2", true).unwrap();
        assert_eq!(doc.metadata().title.as_deref(), Some("Мир"));
        assert_eq!(texts(&doc, &storage)[0].1, "Мир");
    }

    #[test]
    fn zipped_books_open_too() {
        use paperpaw_zip::testing::ZipBuilder;
        let zip = ZipBuilder::new()
            .deflated("book.fb2", BOOK.as_bytes())
            .build();
        let storage = MemoryStorage::new().with("b/test.fb2.zip", zip);
        let doc = Fb2Document::open(&storage, "b/test.fb2.zip", true).unwrap();
        assert_eq!(doc.metadata().title.as_deref(), Some("A Test"));
        assert_eq!(texts(&doc, &storage)[4].1, "Some text and a note1.");
    }

    /// Pictures' long multibyte alternative texts: cut where a character
    /// ends, not inside one, wherever the limit falls in them.
    #[test]
    fn long_alternative_texts_do_not_break_the_book() {
        let mut images = String::new();
        for shift in 0..3 {
            let alt = alloc::format!("{}{}", "a".repeat(shift), "語".repeat(1340));
            images.push_str(&alloc::format!("<image l:href=\"#none\" alt=\"{alt}\"/>"));
        }
        let book = alloc::format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?><FictionBook \
             xmlns:l=\"http://www.w3.org/1999/xlink\"><body><section>\
             {images}<p>After</p></section></body></FictionBook>"
        );
        let storage = MemoryStorage::new().with("a.fb2", book.into_bytes());
        let doc = Fb2Document::open(&storage, "a.fb2", true).unwrap();
        let blocks = texts(&doc, &storage);
        assert_eq!(blocks.len(), 4);
        for (_, text) in &blocks[..3] {
            assert!(text.len() > 3000 && text.len() < 4000, "{}", text.len());
            assert!(text.chars().all(|c| c == 'a' || c == '語'));
        }
        assert_eq!(blocks[3].1, "After");
    }

    #[test]
    fn named_styles_keep_their_text() {
        let book = "<?xml version=\"1.0\"?><FictionBook><body><section>\
            <p>Before <style name=\"bold\">styled</style> after.</p></section></body></FictionBook>";
        let storage = MemoryStorage::new().with("s.fb2", book.as_bytes().to_vec());
        let doc = Fb2Document::open(&storage, "s.fb2", true).unwrap();
        assert_eq!(texts(&doc, &storage)[0].1, "Before styled after.");
    }

    /// Self-closing sections and titles, and a title of the body: each
    /// entry of the contents leads to its own title.
    #[test]
    fn contents_lead_to_their_titles_past_self_closing_elements() {
        let book = "<?xml version=\"1.0\"?><FictionBook><body><section/>\
            <title><p>The book</p></title>\
            <section><title/><p>Untitled.</p></section>\
            <section><title><p>One</p></title><p>First.</p></section>\
            <section><title><p>Two</p></title><p>Second.</p></section></body></FictionBook>";
        let storage = MemoryStorage::new().with("c.fb2", book.as_bytes().to_vec());
        let doc = Fb2Document::open(&storage, "c.fb2", true).unwrap();
        let entries: Vec<(String, u8, String)> = (0..doc.toc_len(&storage))
            .map(|i| {
                let entry = doc.toc_entry(&storage, i).unwrap();
                let block = doc.block_at(&storage, entry.at).unwrap().unwrap();
                (entry.title, entry.level, block.text)
            })
            .collect();
        let row = |t: &str, l, b: &str| (String::from(t), l, String::from(b));
        assert_eq!(entries, [row("One", 0, "One"), row("Two", 0, "Two")]);
    }

    #[test]
    fn base64_is_decoded_in_pieces() {
        let mut out = Vec::new();
        let mut b = Base64::default();
        b.feed("aGVsb", &mut out);
        b.feed("G8=\n", &mut out);
        assert_eq!(out, b"hello");
    }
}
