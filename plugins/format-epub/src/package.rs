//! Reading an EPUB's package: container, OPF (metadata, manifest, spine)
//! and table of contents (EPUB 3 navigation document or EPUB 2 NCX).
//!
//! Every file is tokenized while it is inflated, never held in memory, and
//! the result is written to the book's cache on first open, so later opens
//! read only a few small files and memory does not grow with the book:
//!
//! - `spine.bin`: per chapter its ZIP directory record and uncompressed
//!   size (two u32).
//! - `toc.dat`: table of contents records (level u8, section u32, fragment
//!   length u16, title length u16, fragment, title); `toc.idx` holds the
//!   offset of each record as u32.
//! - `pages.txt`: the printed edition's pages (EPUB 3 `page-list`), as
//!   `section<TAB>fragment<TAB>label` lines in reading order.
//! - `styles.txt`: the rules of all the book's stylesheets (see `css`).
//! - `fonts.txt`: the embedded fonts: first `id<TAB>identifier`, then per
//!   font `family<TAB>bold<TAB>italic<TAB>obfuscation<TAB>path`.
//! - `package.ini`: cache version, the book's fingerprint (see
//!   `crate::fingerprint`), metadata and counts. Written last, so its
//!   presence marks a complete package cache.

use alloc::borrow::Cow;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::Storage;
use paperpaw_sdk::digest::fnv1a64;
use paperpaw_sdk::document::{DocumentError, Metadata, PageProgression};
use paperpaw_sdk::xml::{Token, Tokenizer, decode, local_name};
use paperpaw_sdk::zip::{ZipArchive, ZipError};
use paperpaw_sdk::{config, web};

use crate::css;
use crate::obfuscation::Method;

/// Bump when the cache format or the conversion changes.
pub(crate) const CACHE_VERSION: u32 = 27;

/// A chapter: where it is in the archive and its uncompressed size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Chapter {
    pub record: u32,
    pub size: u32,
}

#[derive(Debug, Clone)]
pub(crate) struct Package {
    pub metadata: Metadata,
    /// The spine in reading order.
    pub spine: Vec<Chapter>,
    pub toc_len: u32,
    /// Archive path of the cover image, if the package names one.
    pub cover: Option<String>,
}

/// A table of contents entry as stored in the cache.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RawToc {
    pub title: String,
    pub level: u8,
    pub section: u32,
    /// Fragment identifier (`#id`), if any.
    pub fragment: Option<String>,
}

/// What is read of a package at most: the text of a metadata element and
/// what is kept of it, authors, subjects, chapters and stylesheets. It
/// bounds memory, and `package.ini` stays well below [`MAX_INI`], so a
/// book's package is read once, not on every open.
const MAX_CAPTURE: usize = 8 * 1024;
const MAX_VALUE: usize = 512;
const MAX_AUTHORS: usize = 16;
const MAX_SUBJECTS: usize = 32;
const MAX_SPINE: usize = 4096;
const MAX_STYLESHEETS: usize = 64;
/// Longest `package.ini` read.
const MAX_INI: usize = 32 * 1024;

/// Size of a `toc.dat` record header.
const TOC_HEADER: usize = 9;
/// Cache files are appended in chunks of about this size.
const FLUSH: usize = 256;

fn path(cache: &str, name: &str) -> String {
    alloc::format!("{cache}/{name}")
}

pub(crate) fn error(e: paperpaw_sdk::zip::ZipError) -> DocumentError {
    match e {
        paperpaw_sdk::zip::ZipError::Storage(e) => DocumentError::Storage(e),
        paperpaw_sdk::zip::ZipError::OutOfMemory => DocumentError::OutOfMemory,
        other => DocumentError::Malformed(Cow::Owned(other.to_string())),
    }
}

fn malformed(message: &'static str) -> DocumentError {
    DocumentError::Malformed(Cow::Borrowed(message))
}

/// The chapters files are, by name: the last looked up is kept, as entries
/// of the contents mostly lead into the file of the one before.
struct Sections<'a> {
    storage: &'a dyn Storage,
    archive: &'a ZipArchive,
    spine: &'a [Chapter],
    last: core::cell::RefCell<Option<(String, Option<u32>)>>,
}

impl Sections<'_> {
    fn of(&self, file: &str) -> Option<u32> {
        let mut last = self.last.borrow_mut();
        if let Some((name, section)) = last.as_ref()
            && name == file
        {
            return *section;
        }
        let record = self.archive.entry(self.storage, file).ok()??.record();
        let section = self.spine.iter().position(|c| c.record == record);
        let section = section.and_then(|s| u32::try_from(s).ok());
        *last = Some((String::from(file), section));
        section
    }
}

/// Tokenizes an archive entry while it is inflated.
fn tokenize(
    storage: &dyn Storage,
    archive: &ZipArchive,
    name: &str,
    handle: &mut dyn FnMut(Token<'_>),
) -> Result<(), DocumentError> {
    let entry = archive
        .entry(storage, name)
        .map_err(error)?
        .ok_or_else(|| DocumentError::Malformed(Cow::Owned(alloc::format!("missing {name}"))))?;
    let mut tokenizer = Tokenizer::new();
    let mut out_of_memory = false;
    archive
        .stream(storage, &entry, &mut |chunk| {
            out_of_memory = tokenizer.try_feed(chunk, handle).is_err();
            !out_of_memory
        })
        .map_err(error)?;
    if out_of_memory {
        return Err(DocumentError::OutOfMemory);
    }
    tokenizer.finish(handle);
    Ok(())
}

/// Adds `more` to `text`, the text of an element being read, while it
/// holds less than [`MAX_CAPTURE`]; false where the heap has no room.
/// Grown by what it needs, not by doubling, so it stays near the most.
fn capture(text: &mut String, more: &str) -> bool {
    if text.len() >= MAX_CAPTURE {
        return true;
    }
    if text.try_reserve_exact(more.len()).is_err() {
        return false;
    }
    text.push_str(more);
    true
}

/// The directory part of an archive path, with trailing slash.
fn dir_of(path: &str) -> &str {
    path.rfind('/').map_or("", |i| &path[..=i])
}

/// Resolves `href` relative to the directory `base` into an archive path
/// and a fragment.
pub(crate) fn resolve(base: &str, href: &str) -> (String, Option<String>) {
    let (path, fragment) = match href.split_once('#') {
        Some((p, f)) => (p, Some(web::percent_decode(f))),
        None => (href, None),
    };
    let joined = if path.is_empty() {
        String::from(base.trim_end_matches('/'))
    } else {
        alloc::format!("{base}{}", web::percent_decode(path))
    };
    let mut parts: Vec<&str> = Vec::new();
    for part in joined.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            part => parts.push(part),
        }
    }
    (parts.join("/"), fragment)
}

/// The most of a description kept, in bytes.
const MAX_DESCRIPTION: usize = 2000;

/// A description as plain text: Calibre and others write HTML into it
/// (escaped), so tags go and entities are decoded; cut at
/// [`MAX_DESCRIPTION`].
fn plain_text(html: &str) -> String {
    let mut text = String::new();
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => {
                in_tag = true;
                // Paragraphs and line breaks part words.
                text.push(' ');
            }
            '>' if in_tag => in_tag = false,
            c if !in_tag => text.push(c),
            _ => {}
        }
    }
    let mut plain = clean(&decode(&text));
    if plain.len() > MAX_DESCRIPTION {
        let mut end = MAX_DESCRIPTION;
        while !plain.is_char_boundary(end) {
            end -= 1;
        }
        plain.truncate(end);
        plain.push('…');
    }
    plain
}

/// A value of the package: its whitespace collapsed, at most
/// [`MAX_VALUE`] bytes.
fn value(text: &str) -> String {
    let mut value = clean(text);
    value.truncate(truncate(&value, MAX_VALUE).len());
    value
}

/// Collapses whitespace in a title.
fn clean(text: &str) -> String {
    let mut out = String::new();
    for word in text.split_whitespace() {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    out
}

fn read_u32(storage: &dyn Storage, path: &str, offset: u64) -> Option<u32> {
    let mut buf = [0u8; 4];
    (storage.read_at(path, offset, &mut buf).ok()? == 4).then(|| u32::from_le_bytes(buf))
}

/// A file of the cache, at most `limit` bytes long: `None` where it is
/// not there or cannot be read (it is made anew), an error where the heap
/// has no room for it now (what is there stays).
fn read_cache(
    storage: &dyn Storage,
    path: &str,
    limit: usize,
) -> Result<Option<Vec<u8>>, DocumentError> {
    match paperpaw_sdk::read_file(storage, path, limit) {
        Ok(data) => Ok(Some(data)),
        // Not too long for the limit: too long for the heap.
        Err(paperpaw_sdk::StorageError::TooLarge)
            if storage.size(path).is_ok_and(|size| size <= limit as u64) =>
        {
            Err(DocumentError::OutOfMemory)
        }
        Err(_) => Ok(None),
    }
}

/// The package from the cache, if it is complete and matches the book;
/// an error only where the heap has no room to read it.
pub(crate) fn load(
    storage: &dyn Storage,
    cache: &str,
    book: &str,
) -> Result<Option<Package>, DocumentError> {
    let Some(data) = read_cache(storage, &path(cache, "package.ini"), MAX_INI)? else {
        return Ok(None);
    };
    let parsed = core::str::from_utf8(&data)
        .ok()
        .and_then(|source| parse_ini(source, book));
    drop(data);
    let Some((metadata, sections, toc_len, cover)) = parsed else {
        return Ok(None);
    };
    let Some(spine) = load_spine(storage, cache, sections)? else {
        return Ok(None);
    };
    Ok(Some(Package {
        metadata,
        spine,
        toc_len,
        cover,
    }))
}

/// What `package.ini` says of the book: its metadata, its number of
/// chapters and of entries of the contents, and its cover. `None` where
/// it is damaged or of another book or cache version.
fn parse_ini(source: &str, book: &str) -> Option<(Metadata, usize, u32, Option<String>)> {
    let mut metadata = Metadata::default();
    let (mut version, mut stamp, mut sections, mut toc_len) = (None, None, None, None);
    let mut cover = None;
    for entry in config::parse(source) {
        let entry = entry.ok()?;
        match (entry.section, entry.key) {
            ("cache", "version") => version = entry.value.as_u32(),
            ("cache", "source") => stamp = Some(entry.value.as_str().into_owned()),
            ("book", "title") => metadata.title = Some(entry.value.as_str().into_owned()),
            ("book", key) if key.starts_with("author.") => {
                metadata.authors.push(entry.value.as_str().into_owned());
            }
            ("book", "language") => metadata.language = Some(entry.value.as_str().into_owned()),
            ("book", "series") => metadata.series = Some(entry.value.as_str().into_owned()),
            ("book", "series-index") => {
                metadata.series_index = Some(entry.value.as_str().into_owned());
            }
            ("book", key) if key.starts_with("subject.") => {
                metadata.subjects.push(entry.value.as_str().into_owned());
            }
            ("book", "description") => {
                metadata.description = Some(entry.value.as_str().into_owned());
            }
            ("book", "direction") if entry.value.as_str() == "rtl" => {
                metadata.direction = PageProgression::RightToLeft;
            }
            ("book", "sections") => sections = entry.value.as_u32(),
            ("book", "toc") => toc_len = entry.value.as_u32(),
            ("book", "cover") => cover = Some(entry.value.as_str().into_owned()),
            _ => {}
        }
    }
    if version != Some(CACHE_VERSION) || stamp.as_deref() != Some(book) {
        return None;
    }
    Some((metadata, sections? as usize, toc_len?, cover))
}

/// The spine of `sections` chapters from `spine.bin`, read a few records
/// at a time into a list made once: `None` where the file is not whole.
fn load_spine(
    storage: &dyn Storage,
    cache: &str,
    sections: usize,
) -> Result<Option<Vec<Chapter>>, DocumentError> {
    let file = path(cache, "spine.bin");
    if sections == 0 || storage.size(&file).ok() != Some(sections as u64 * 8) {
        return Ok(None);
    }
    let mut spine = Vec::new();
    spine
        .try_reserve_exact(sections)
        .map_err(|_| DocumentError::OutOfMemory)?;
    let mut buf = [0u8; FLUSH];
    while spine.len() < sections {
        let want = ((sections - spine.len()) * 8).min(buf.len());
        let at = spine.len() as u64 * 8;
        let mut filled = 0;
        while filled < want {
            match storage.read_at(&file, at + filled as u64, &mut buf[filled..want]) {
                Ok(n) if n > 0 => filled += n,
                _ => return Ok(None),
            }
        }
        spine.extend(buf[..want].as_chunks::<8>().0.iter().map(|c| Chapter {
            record: u32::from_le_bytes([c[0], c[1], c[2], c[3]]),
            size: u32::from_le_bytes([c[4], c[5], c[6], c[7]]),
        }));
    }
    Ok(Some(spine))
}

/// Writes `fonts.txt`: the fonts the stylesheets embed and how
/// `META-INF/encryption.xml` says they are obfuscated.
fn write_fonts(
    storage: &dyn Storage,
    archive: &ZipArchive,
    cache: &str,
    faces: &[css::FontFace],
    identifier: &str,
) -> Result<(), DocumentError> {
    let mut methods: Vec<(String, Method)> = Vec::new();
    if !faces.is_empty() {
        let mut method = Method::None;
        // A missing encryption.xml is fine: nothing is obfuscated.
        let read = tokenize(storage, archive, "META-INF/encryption.xml", &mut |token| {
            if let Token::Start { name, attrs, .. } = token {
                match local_name(name) {
                    "EncryptionMethod" => {
                        method =
                            Method::from_algorithm(&attrs.get("Algorithm").unwrap_or_default());
                    }
                    "CipherReference" => {
                        if let Some(uri) = attrs.get("URI") {
                            methods.push((resolve("", &uri).0, method));
                        }
                    }
                    _ => {}
                }
            }
        });
        only_out_of_memory(read)?;
    }
    let mut out = alloc::format!("id\t{}\n", identifier.replace(['\t', '\n'], " "));
    for face in faces {
        let method = methods
            .iter()
            .find(|(p, _)| *p == face.path)
            .map_or(Method::None, |(_, m)| *m);
        out.push_str(&alloc::format!(
            "{}\t{}\t{}\t{}\t{}\n",
            face.family,
            u8::from(face.bold),
            u8::from(face.italic),
            method.code(),
            face.path
        ));
    }
    storage.write(&path(cache, "fonts.txt"), out.as_bytes())?;
    Ok(())
}

/// An embedded font as listed in `fonts.txt`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FontEntry {
    pub family: u8,
    pub bold: bool,
    pub italic: bool,
    pub method: Method,
    pub path: String,
}

/// The book's identifier and embedded fonts from the cache.
pub(crate) fn fonts(storage: &dyn Storage, cache: &str) -> (String, Vec<FontEntry>) {
    let Ok(data) = paperpaw_sdk::read_file(storage, &path(cache, "fonts.txt"), 64 * 1024) else {
        return (String::new(), Vec::new());
    };
    let text = String::from_utf8_lossy(&data);
    let mut identifier = String::new();
    let mut entries = Vec::new();
    for line in text.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        match fields.as_slice() {
            ["id", id] => identifier = String::from(*id),
            [family, bold, italic, method, path] => {
                let Ok(family) = family.parse() else {
                    continue;
                };
                entries.push(FontEntry {
                    family,
                    bold: *bold == "1",
                    italic: *italic == "1",
                    method: Method::from_code(method.parse().unwrap_or(0)),
                    path: String::from(*path),
                });
            }
            _ => {}
        }
    }
    (identifier, entries)
}

/// The book's stylesheet rules from the cache; an error where the heap
/// has no room for them, as a chapter converted without them would be
/// kept so.
pub(crate) fn styles(storage: &dyn Storage, cache: &str) -> Result<css::Rules, DocumentError> {
    let Some(data) = read_cache(storage, &path(cache, "styles.txt"), 64 * 1024)? else {
        return Ok(css::Rules::default());
    };
    match String::from_utf8(data) {
        Ok(text) => css::Rules::try_decode(&text).map_err(|_| DocumentError::OutOfMemory),
        Err(_) => Ok(css::Rules::default()),
    }
}

/// Reads table of contents entry `index` from the cache.
pub(crate) fn toc_entry(storage: &dyn Storage, cache: &str, index: u32) -> Option<RawToc> {
    let data = path(cache, "toc.dat");
    let offset = u64::from(read_u32(
        storage,
        &path(cache, "toc.idx"),
        u64::from(index) * 4,
    )?);
    let mut header = [0u8; TOC_HEADER];
    if storage.read_at(&data, offset, &mut header).ok()? != TOC_HEADER {
        return None;
    }
    let section = u32::from_le_bytes([header[1], header[2], header[3], header[4]]);
    let fragment_len = usize::from(u16::from_le_bytes([header[5], header[6]]));
    let title_len = usize::from(u16::from_le_bytes([header[7], header[8]]));
    let mut text = alloc::vec![0u8; fragment_len + title_len];
    let mut filled = 0;
    while filled < text.len() {
        let n = storage
            .read_at(
                &data,
                offset + (TOC_HEADER + filled) as u64,
                &mut text[filled..],
            )
            .ok()?;
        if n == 0 {
            return None;
        }
        filled += n;
    }
    let (fragment, title) = text.split_at(fragment_len);
    Some(RawToc {
        title: String::from_utf8_lossy(title).into_owned(),
        level: header[0],
        section,
        fragment: (fragment_len > 0).then(|| String::from_utf8_lossy(fragment).into_owned()),
    })
}

/// `text` cut to at most `max` bytes, at a character boundary.
fn truncate(text: &str, max: usize) -> &str {
    let mut cut = text.len().min(max);
    while !text.is_char_boundary(cut) {
        cut -= 1;
    }
    &text[..cut]
}

/// Writes table of contents records to the cache in small chunks.
pub(crate) struct TocWriter<'a> {
    storage: &'a dyn Storage,
    data_path: String,
    index_path: String,
    data: Vec<u8>,
    index: Vec<u8>,
    written: u32,
    count: u32,
    failed: Option<DocumentError>,
}

impl<'a> TocWriter<'a> {
    pub(crate) fn new(storage: &'a dyn Storage, cache: &str) -> Result<Self, DocumentError> {
        let writer = Self {
            storage,
            data_path: path(cache, "toc.dat"),
            index_path: path(cache, "toc.idx"),
            data: Vec::new(),
            index: Vec::new(),
            written: 0,
            count: 0,
            failed: None,
        };
        storage.write(&writer.data_path, &[])?;
        storage.write(&writer.index_path, &[])?;
        Ok(writer)
    }

    pub(crate) fn push(&mut self, level: u8, section: u32, fragment: Option<&str>, title: &str) {
        // Longer ids cannot match an anchor; titles are shown on one line.
        let fragment = truncate(fragment.unwrap_or_default(), 255);
        let title = truncate(title, 1024);
        self.index.extend_from_slice(&self.written.to_le_bytes());
        self.data.push(level);
        self.data.extend_from_slice(&section.to_le_bytes());
        self.data
            .extend_from_slice(&(fragment.len() as u16).to_le_bytes());
        self.data
            .extend_from_slice(&(title.len() as u16).to_le_bytes());
        self.data.extend_from_slice(fragment.as_bytes());
        self.data.extend_from_slice(title.as_bytes());
        self.written += (TOC_HEADER + fragment.len() + title.len()) as u32;
        self.count += 1;
        if self.data.len() >= FLUSH || self.index.len() >= FLUSH {
            self.flush();
        }
    }

    fn flush(&mut self) {
        for (path, buffer) in [
            (&self.data_path, &mut self.data),
            (&self.index_path, &mut self.index),
        ] {
            if !buffer.is_empty()
                && self.failed.is_none()
                && let Err(e) = self.storage.append(path, buffer)
            {
                self.failed = Some(DocumentError::Storage(e));
            }
            buffer.clear();
        }
    }

    pub(crate) fn finish(mut self) -> Result<u32, DocumentError> {
        self.flush();
        match self.failed {
            Some(e) => Err(e),
            None => Ok(self.count),
        }
    }
}

/// Reads the package from the archive and writes it to the cache, for
/// the book of fingerprint `book`.
pub(crate) fn build(
    storage: &dyn Storage,
    archive: &ZipArchive,
    cache: &str,
    book: &str,
) -> Result<Package, DocumentError> {
    let opf_path = package_path(storage, archive)?;
    let opf = read_metadata(storage, archive, &opf_path)?;
    let items = read_manifest(storage, archive, &opf_path, &opf)?;
    // The spine's ids are done with.
    let Opf {
        metadata,
        identifier,
        ..
    } = opf;
    let toc_len = write_toc(
        storage,
        archive,
        cache,
        &items.spine,
        items.nav.as_deref(),
        items.ncx.as_deref(),
    )?;
    write_spine(storage, cache, &items.spine)?;
    write_styles(storage, archive, cache, &items.stylesheets, &identifier)?;
    let sections = items.spine.len();
    let cover = items.cover.as_deref();
    write_ini(storage, cache, &metadata, sections, toc_len, cover, book)?;
    Ok(Package {
        metadata,
        spine: items.spine,
        toc_len,
        cover: items.cover,
    })
}

/// Where the package document is (`META-INF/container.xml` says).
fn package_path(storage: &dyn Storage, archive: &ZipArchive) -> Result<String, DocumentError> {
    // container.xml → path of the OPF package document.
    let mut opf_path = None;
    tokenize(storage, archive, "META-INF/container.xml", &mut |token| {
        if let Token::Start { name, attrs, .. } = token
            && local_name(name) == "rootfile"
            && opf_path.is_none()
        {
            opf_path = attrs.get("full-path").map(|p| p.into_owned());
        }
    })?;
    let opf_path = opf_path.ok_or(malformed("no package document"))?;
    Ok(opf_path)
}

/// What the first pass over the package document finds.
struct Opf {
    metadata: Metadata,
    /// The spine's ids (hashed: 8 bytes each), sorted, with their places
    /// in it.
    spine_ids: Vec<(u64, u32)>,
    /// The NCX's id, as the spine names it.
    ncx_id: Option<u64>,
    /// EPUB 2's cover, as `<meta name="cover">` names it.
    cover_id: Option<u64>,
    /// The unique identifier, the key of obfuscated fonts.
    identifier: String,
}

/// First pass: metadata and the spine.
#[expect(
    clippy::too_many_lines,
    reason = "longer than the limit from before it was set; split it when it is next changed"
)]
fn read_metadata(
    storage: &dyn Storage,
    archive: &ZipArchive,
    opf_path: &str,
) -> Result<Opf, DocumentError> {
    // First pass: metadata and the spine's ids (hashed: 8 bytes each).
    let mut metadata = Metadata::default();
    let mut spine_ids: Vec<(u64, u32)> = Vec::new();
    let mut ncx_id: Option<u64> = None;
    // EPUB 2 names the cover's manifest item in `<meta name="cover">`.
    let mut cover_id: Option<u64> = None;
    let mut capture: Option<&'static str> = None;
    let mut text = String::new();
    // The unique identifier, the key of obfuscated fonts.
    let mut unique_id: Option<String> = None;
    let mut identifier = String::new();
    // EPUB 3 series: `<meta property="belongs-to-collection" id="c">`, its
    // number in `<meta refines="#c" property="group-position">`.
    let mut collection_id: Option<String> = None;
    let mut out_of_memory = false;
    tokenize(storage, archive, opf_path, &mut |token| match token {
        Token::Start {
            name,
            attrs,
            self_closing,
        } => match local_name(name) {
            "package" => unique_id = attrs.get("unique-identifier").map(|v| v.into_owned()),
            "identifier"
                if !self_closing
                    && identifier.is_empty()
                    && attrs
                        .get("id")
                        .is_some_and(|id| unique_id.as_deref() == Some(&*id)) =>
            {
                capture = Some("identifier");
                text.clear();
            }
            "title" | "creator" | "language" | "subject" | "description" if !self_closing => {
                capture = match local_name(name) {
                    "title" => Some("title"),
                    "creator" => Some("creator"),
                    "subject" => Some("subject"),
                    "description" => Some("description"),
                    _ => Some("language"),
                };
                text.clear();
            }
            "spine" => {
                ncx_id = attrs.get("toc").map(|v| fnv1a64(v.as_bytes()));
                if attrs.get("page-progression-direction").as_deref() == Some("rtl") {
                    metadata.direction = PageProgression::RightToLeft;
                }
            }
            "meta" if attrs.get("name").as_deref() == Some("cover") => {
                cover_id = attrs.get("content").map(|v| fnv1a64(v.as_bytes()));
            }
            // Calibre's series.
            "meta" if attrs.get("name").as_deref() == Some("calibre:series") => {
                if let Some(series) = attrs.get("content").map(|v| value(&v)) {
                    metadata.series.get_or_insert(series);
                }
            }
            "meta" if attrs.get("name").as_deref() == Some("calibre:series_index") => {
                if let Some(index) = attrs.get("content").map(|v| value(&v)) {
                    metadata.series_index.get_or_insert(index);
                }
            }
            "meta" if !self_closing => {
                let property = attrs.get("property");
                if property.as_deref() == Some("belongs-to-collection") {
                    collection_id = attrs.get("id").map(|v| v.into_owned());
                    capture = Some("collection");
                    text.clear();
                } else if property.as_deref() == Some("group-position")
                    && attrs.get("refines").is_some_and(|r| {
                        collection_id
                            .as_deref()
                            .is_some_and(|id| r.trim_start_matches('#') == id)
                    })
                {
                    capture = Some("position");
                    text.clear();
                }
            }
            "itemref" if spine_ids.len() < MAX_SPINE => {
                if let Some(id) = attrs.get("idref") {
                    let position = spine_ids.len() as u32;
                    match spine_ids.try_reserve(1) {
                        Ok(()) => spine_ids.push((fnv1a64(id.as_bytes()), position)),
                        Err(_) => out_of_memory = true,
                    }
                }
            }
            _ => {}
        },
        Token::Text(t) if capture.is_some() => {
            out_of_memory |= !self::capture(&mut text, &decode(&t));
        }
        Token::CData(t) if capture.is_some() => {
            out_of_memory |= !self::capture(&mut text, t);
        }
        Token::End { name } => {
            // Series are captured from `<meta>` elements.
            let element = |field: &'static str| -> &'static str {
                match field {
                    "collection" | "position" => "meta",
                    field => field,
                }
            };
            if let Some(field) = capture
                && local_name(name) == element(field)
            {
                let value = value(&text);
                match field {
                    "title" if metadata.title.is_none() && !value.is_empty() => {
                        metadata.title = Some(value);
                    }
                    "creator" if !value.is_empty() && metadata.authors.len() < MAX_AUTHORS => {
                        metadata.authors.push(value);
                    }
                    "subject"
                        if !value.is_empty()
                            && metadata.subjects.len() < MAX_SUBJECTS
                            && !metadata.subjects.contains(&value) =>
                    {
                        metadata.subjects.push(value);
                    }
                    "description" if metadata.description.is_none() => {
                        let plain = plain_text(&text);
                        metadata.description = (!plain.is_empty()).then_some(plain);
                    }
                    "language" if metadata.language.is_none() && !value.is_empty() => {
                        metadata.language = Some(value);
                    }
                    "identifier" => identifier = value,
                    "collection" if !value.is_empty() => {
                        metadata.series.get_or_insert(value);
                    }
                    "position" if !value.is_empty() => {
                        metadata.series_index.get_or_insert(value);
                    }
                    _ => {}
                }
                capture = None;
            }
        }
        _ => {}
    })?;
    if out_of_memory {
        return Err(DocumentError::OutOfMemory);
    }
    spine_ids.sort_unstable();
    Ok(Opf {
        metadata,
        spine_ids,
        ncx_id,
        cover_id,
        identifier,
    })
}

/// A chapter the spine names that is not in the archive. No record is
/// there: it would be past the end of an archive of 4 GiB.
const MISSING: Chapter = Chapter {
    record: u32::MAX,
    size: 0,
};

/// What the second pass keeps of the manifest.
struct Items {
    /// The chapters, in reading order.
    spine: Vec<Chapter>,
    /// The EPUB 3 navigation document.
    nav: Option<String>,
    /// The EPUB 2 NCX.
    ncx: Option<String>,
    stylesheets: Vec<String>,
    cover: Option<String>,
}

/// Second pass: the manifest's items the first pass asks for.
fn read_manifest(
    storage: &dyn Storage,
    archive: &ZipArchive,
    opf_path: &str,
    opf: &Opf,
) -> Result<Items, DocumentError> {
    let opf_dir = String::from(dir_of(opf_path));
    let (spine_ids, ncx_id, cover_id) = (&opf.spine_ids, opf.ncx_id, opf.cover_id);
    // Second pass: the manifest items the spine refers to, the navigation
    // document and the NCX. Nothing else of the manifest is kept.
    // Chapters not found keep `MISSING` and go at the end: no list
    // besides this one.
    let mut chapters: Vec<Chapter> = Vec::new();
    chapters
        .try_reserve_exact(spine_ids.len())
        .map_err(|_| DocumentError::OutOfMemory)?;
    chapters.resize(spine_ids.len(), MISSING);
    let mut nav: Option<String> = None;
    let mut ncx: Option<String> = None;
    let mut ncx_by_type: Option<String> = None;
    let mut stylesheets: Vec<String> = Vec::new();
    // EPUB 3 `cover-image` wins over EPUB 2's `<meta name="cover">`.
    let mut cover: Option<(bool, String)> = None;
    let mut failed = None;
    tokenize(storage, archive, opf_path, &mut |token| {
        let Token::Start { name, attrs, .. } = token else {
            return;
        };
        if local_name(name) != "item" {
            return;
        }
        let get = |n| attrs.get(n).unwrap_or_default();
        let id = fnv1a64(get("id").as_bytes());
        let first = spine_ids.partition_point(|&(h, _)| h < id);
        let positions = spine_ids[first..].iter().take_while(|&&(h, _)| h == id);
        let is_nav = get("properties").split_whitespace().any(|p| p == "nav");
        let is_ncx = ncx_id == Some(id);
        let media_type = get("media-type");
        let is_ncx_type = media_type == "application/x-dtbncx+xml";
        let is_css = media_type == "text/css";
        let is_cover_image = get("properties")
            .split_whitespace()
            .any(|p| p == "cover-image");
        if (is_cover_image || cover_id == Some(id))
            && media_type.starts_with("image/")
            && !cover.as_ref().is_some_and(|(declared, _)| *declared)
        {
            cover = Some((is_cover_image, resolve(&opf_dir, &get("href")).0));
        }
        let mut positions = positions.peekable();
        if positions.peek().is_none() && !is_nav && !is_ncx && !is_ncx_type && !is_css {
            return;
        }
        let (path, _) = resolve(&opf_dir, &get("href"));
        if is_css && stylesheets.len() < MAX_STYLESHEETS {
            stylesheets.push(path.clone());
        }
        for &(_, position) in positions {
            match archive.entry(storage, &path) {
                Ok(Some(entry)) => {
                    chapters[position as usize] = Chapter {
                        record: entry.record(),
                        size: entry.size,
                    };
                }
                Ok(None) => {}
                Err(e) => failed = Some(error(e)),
            }
        }
        if is_nav && nav.is_none() {
            nav = Some(path.clone());
        }
        if is_ncx {
            ncx = Some(path.clone());
        }
        if is_ncx_type && ncx_by_type.is_none() {
            ncx_by_type = Some(path);
        }
    })?;
    if let Some(e) = failed {
        return Err(e);
    }
    let mut spine = chapters;
    spine.retain(|chapter| *chapter != MISSING);
    if spine.is_empty() {
        return Err(malformed("empty spine"));
    }
    Ok(Items {
        spine,
        nav,
        ncx: ncx.or(ncx_by_type),
        stylesheets,
        cover: cover.map(|(_, path)| path),
    })
}

/// Writes the table of contents and the printed pages; how many entries
/// the contents have.
fn write_toc(
    storage: &dyn Storage,
    archive: &ZipArchive,
    cache: &str,
    spine: &[Chapter],
    nav: Option<&str>,
    ncx: Option<&str>,
) -> Result<u32, DocumentError> {
    // Table of contents: EPUB 3 navigation document, else (or if it has no
    // entries) the EPUB 2 NCX.
    let mut toc = TocWriter::new(storage, cache)?;
    let sections = Sections {
        storage,
        archive,
        spine,
        last: core::cell::RefCell::new(None),
    };
    let add =
        |toc: &mut TocWriter<'_>, title: String, level: u8, file: &str, fragment: Option<&str>| {
            if let Some(section) = sections.of(file) {
                toc.push(level, section, fragment, &title);
            }
        };
    let pages_path = path(cache, "pages.txt");
    storage.write(&pages_path, &[])?;
    if let Some(nav) = nav {
        let mut pages = String::new();
        let mut add_page = |label: String, file: &str, fragment: Option<&str>| {
            let Some(fragment) = fragment else {
                return;
            };
            if let Some(section) = sections.of(file) {
                let one = |s: &str| s.replace(['\t', '\n'], " ");
                pages.push_str(&alloc::format!(
                    "{section}\t{}\t{}\n",
                    one(fragment),
                    one(&label)
                ));
                if pages.len() >= 1024 {
                    let _ = storage.append(&pages_path, pages.as_bytes());
                    pages.clear();
                }
            }
        };
        let read = read_nav(
            storage,
            archive,
            nav,
            &mut |t, l, f, fr| add(&mut toc, t, l, f, fr),
            &mut add_page,
        );
        let _ = storage.append(&pages_path, pages.as_bytes());
        only_out_of_memory(read)?;
    }
    if let Some(ncx) = ncx
        && toc.count == 0
    {
        only_out_of_memory(read_ncx(storage, archive, ncx, &mut |t, l, f, fr| {
            add(&mut toc, t, l, f, fr)
        }))?;
    }
    toc.finish()
}

/// Only running out of memory from reading a table of contents: without
/// one the book still opens, but contents cut short would be kept.
fn only_out_of_memory(read: Result<(), DocumentError>) -> Result<(), DocumentError> {
    match read {
        Err(DocumentError::OutOfMemory) => Err(DocumentError::OutOfMemory),
        _ => Ok(()),
    }
}

/// Writes where the chapters are in the archive, a few at a time.
fn write_spine(storage: &dyn Storage, cache: &str, spine: &[Chapter]) -> Result<(), DocumentError> {
    let file = path(cache, "spine.bin");
    storage.write(&file, &[])?;
    let mut buf = [0u8; FLUSH];
    for chapters in spine.chunks(FLUSH / 8) {
        for (record, chapter) in buf.as_chunks_mut::<8>().0.iter_mut().zip(chapters) {
            record[..4].copy_from_slice(&chapter.record.to_le_bytes());
            record[4..].copy_from_slice(&chapter.size.to_le_bytes());
        }
        storage.append(&file, &buf[..chapters.len() * 8])?;
    }
    Ok(())
}

/// Writes the book's styles and its fonts.
fn write_styles(
    storage: &dyn Storage,
    archive: &ZipArchive,
    cache: &str,
    stylesheets: &[String],
    identifier: &str,
) -> Result<(), DocumentError> {
    // Stylesheets, one at a time; a damaged one is skipped.
    let mut rules = css::Rules::default();
    for path in stylesheets {
        let mut parser = css::Parser::new(dir_of(path));
        let Ok(Some(entry)) = archive.entry(storage, path) else {
            continue;
        };
        let mut pending: Vec<u8> = Vec::new();
        let mut out_of_memory = false;
        let streamed = archive.stream(storage, &entry, &mut |chunk| {
            out_of_memory = pending.try_reserve(chunk.len()).is_err();
            if out_of_memory {
                return false;
            }
            pending.extend_from_slice(chunk);
            feed_utf8(&mut pending, &mut |text| parser.feed(text, &mut rules));
            !rules.out_of_memory
        });
        // A damaged stylesheet is passed over; one the heap had no room
        // for would be missed in the cache.
        if out_of_memory || rules.out_of_memory || streamed == Err(ZipError::OutOfMemory) {
            return Err(DocumentError::OutOfMemory);
        }
    }
    write_rules(storage, &path(cache, "styles.txt"), &rules)?;
    write_fonts(storage, archive, cache, &rules.faces, identifier)?;
    drop(rules);
    Ok(())
}

/// Writes `rules` to `file` in their cache form, a few lines at a time.
fn write_rules(storage: &dyn Storage, file: &str, rules: &css::Rules) -> Result<(), DocumentError> {
    storage.write(file, &[])?;
    let mut lines = String::new();
    let mut failed = None;
    rules.encode_lines(&mut |line| {
        lines.push_str(line);
        if lines.len() >= FLUSH {
            if failed.is_none() {
                failed = storage.append(file, lines.as_bytes()).err();
            }
            lines.clear();
        }
    });
    match failed {
        Some(e) => Err(DocumentError::Storage(e)),
        None => Ok(storage.append(file, lines.as_bytes())?),
    }
}

/// Feeds the text at the start of `pending` to `feed` and removes it: all
/// but a sequence cut off at the end, kept for the next chunk. Bytes that
/// are not UTF-8 (a Latin-1 `©` in a stylesheet's comment) are fed as
/// U+FFFD, and the text after them goes on.
fn feed_utf8(pending: &mut Vec<u8>, feed: &mut dyn FnMut(&str)) {
    let mut start = 0;
    while start < pending.len() {
        let (valid, skip) = match core::str::from_utf8(&pending[start..]) {
            Ok(text) => (text.len(), None),
            Err(e) => (e.valid_up_to(), e.error_len()),
        };
        if let Ok(text) = core::str::from_utf8(&pending[start..start + valid]) {
            feed(text);
        }
        start += valid;
        match skip {
            Some(bad) => {
                feed("\u{fffd}");
                start += bad;
            }
            // Whole, or cut off at the end.
            None => break,
        }
    }
    pending.drain(..start);
}

/// Writes what is known of the book, for opening it again quickly.
fn write_ini(
    storage: &dyn Storage,
    cache: &str,
    metadata: &Metadata,
    sections: usize,
    toc_len: u32,
    cover: Option<&str>,
    book: &str,
) -> Result<(), DocumentError> {
    let mut entries: Vec<(&str, String, String)> = Vec::new();
    let mut set = |section: &'static str, key: &str, value: &str| {
        entries.push((section, String::from(key), String::from(value)));
    };
    set("cache", "version", &CACHE_VERSION.to_string());
    set("cache", "source", book);
    if let Some(title) = &metadata.title {
        set("book", "title", title);
    }
    if let Some(language) = &metadata.language {
        set("book", "language", language);
    }
    if let Some(series) = &metadata.series {
        set("book", "series", series);
    }
    if let Some(index) = &metadata.series_index {
        set("book", "series-index", index);
    }
    set("book", "sections", &sections.to_string());
    set("book", "toc", &toc_len.to_string());
    for (i, author) in metadata.authors.iter().enumerate() {
        set("book", &alloc::format!("author.{}", i + 1), author);
    }
    for (i, subject) in metadata.subjects.iter().enumerate() {
        set("book", &alloc::format!("subject.{}", i + 1), subject);
    }
    if let Some(description) = &metadata.description {
        set("book", "description", description);
    }
    if metadata.direction == PageProgression::RightToLeft {
        set("book", "direction", "rtl");
    }
    if let Some(cover) = cover {
        set("book", "cover", cover);
    }
    let ini = config::set_all("", entries);
    storage.write(&path(cache, "package.ini"), ini.as_bytes())?;
    Ok(())
}

type AddToc<'a> = dyn FnMut(String, u8, &str, Option<&str>) + 'a;

/// EPUB 3: `<nav epub:type="toc">` with nested `<ol><li><a href>`, and
/// the printed pages of `<nav epub:type="page-list">` for `page`.
fn read_nav(
    storage: &dyn Storage,
    archive: &ZipArchive,
    path: &str,
    add: &mut AddToc<'_>,
    page: &mut dyn FnMut(String, &str, Option<&str>),
) -> Result<(), DocumentError> {
    let base = String::from(dir_of(path));
    let mut in_toc = false;
    let mut in_pages = false;
    let mut page_link: Option<(String, Option<String>)> = None;
    let mut ol_depth = 0u8;
    let mut link: Option<(String, Option<String>)> = None;
    let mut text = String::new();
    let mut out_of_memory = false;
    tokenize(storage, archive, path, &mut |token| match token {
        Token::Start {
            name,
            attrs,
            self_closing,
        } => match local_name(name) {
            "nav" => {
                let kind = attrs
                    .iter()
                    .find(|(n, _)| local_name(n) == "type")
                    .map(|(_, v)| v);
                if kind.is_some_and(|k| k.split_whitespace().any(|t| t == "toc")) {
                    in_toc = true;
                    ol_depth = 0;
                }
                in_pages = kind.is_some_and(|k| k.split_whitespace().any(|t| t == "page-list"));
            }
            "a" if in_pages && !self_closing => {
                let href = attrs.get("href").unwrap_or_default();
                page_link = Some(resolve(&base, &href));
                text.clear();
            }
            "ol" if in_toc => ol_depth = ol_depth.saturating_add(1),
            "a" if in_toc && !self_closing => {
                let href = attrs.get("href").unwrap_or_default();
                link = Some(resolve(&base, &href));
                text.clear();
            }
            _ => {}
        },
        Token::Text(t) if link.is_some() || page_link.is_some() => {
            out_of_memory |= !capture(&mut text, &decode(&t));
        }
        Token::End { name } => match local_name(name) {
            "nav" => {
                in_toc = false;
                in_pages = false;
            }
            "a" if page_link.is_some() => {
                if let Some((file, fragment)) = page_link.take() {
                    let label = clean(&text);
                    if !label.is_empty() && !file.is_empty() {
                        page(label, &file, fragment.as_deref());
                    }
                }
            }
            "ol" if in_toc => ol_depth = ol_depth.saturating_sub(1),
            "a" => {
                if let Some((file, fragment)) = link.take() {
                    let title = clean(&text);
                    if !title.is_empty() && !file.is_empty() {
                        add(
                            title,
                            ol_depth.saturating_sub(1),
                            &file,
                            fragment.as_deref(),
                        );
                    }
                }
            }
            _ => {}
        },
        _ => {}
    })?;
    if out_of_memory {
        return Err(DocumentError::OutOfMemory);
    }
    Ok(())
}

/// EPUB 2: nested `<navPoint>` with `<navLabel><text>` and `<content src>`
/// inside `<navMap>` (page lists and other lists are not chapters).
fn read_ncx(
    storage: &dyn Storage,
    archive: &ZipArchive,
    path: &str,
    add: &mut AddToc<'_>,
) -> Result<(), DocumentError> {
    let base = String::from(dir_of(path));
    let mut depth = 0u8;
    let mut in_map = false;
    let mut in_label = false;
    let mut text = String::new();
    let mut pending: Option<(String, u8)> = None;
    let mut out_of_memory = false;
    tokenize(storage, archive, path, &mut |token| match token {
        Token::Start { name, attrs, .. } => match local_name(name) {
            "navMap" => in_map = true,
            _ if !in_map => {}
            "navPoint" => depth = depth.saturating_add(1),
            "navLabel" => {
                in_label = true;
                text.clear();
            }
            "content" => {
                if let (Some((title, level)), Some(src)) = (pending.take(), attrs.get("src")) {
                    let (file, fragment) = resolve(&base, &src);
                    add(title, level, &file, fragment.as_deref());
                }
            }
            _ => {}
        },
        Token::Text(t) if in_label => out_of_memory |= !capture(&mut text, &decode(&t)),
        Token::End { name } => match local_name(name) {
            "navMap" => in_map = false,
            "navPoint" => depth = depth.saturating_sub(1),
            "navLabel" if in_map => {
                in_label = false;
                let title = clean(&text);
                if !title.is_empty() {
                    pending = Some((title, depth.saturating_sub(1)));
                }
            }
            _ => {}
        },
        _ => {}
    })?;
    if out_of_memory {
        return Err(DocumentError::OutOfMemory);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_relative_paths() {
        assert_eq!(
            resolve("OEBPS/", "text/ch1.xhtml"),
            ("OEBPS/text/ch1.xhtml".into(), None)
        );
        assert_eq!(
            resolve("OEBPS/text/", "../Text/ch%202.xhtml#sec-1"),
            ("OEBPS/Text/ch 2.xhtml".into(), Some("sec-1".into()))
        );
        assert_eq!(resolve("", "./a.xhtml"), ("a.xhtml".into(), None));
        // Only two hex digits are an escape, no sign.
        assert_eq!(resolve("", "a%+1.xhtml"), ("a%+1.xhtml".into(), None));
    }

    #[test]
    fn stylesheets_go_on_after_bytes_that_are_not_utf8() {
        let mut out = String::new();
        let mut pending = Vec::new();
        // A Latin-1 copyright sign, and a dash cut between two chunks.
        for piece in [&b"/* \xa9 2010 */ p { a: \xe2"[..], b"\x80\x94 }"] {
            pending.extend_from_slice(piece);
            feed_utf8(&mut pending, &mut |text| out.push_str(text));
        }
        assert_eq!(out, "/* \u{fffd} 2010 */ p { a: \u{2014} }");
        assert!(pending.is_empty());
    }
}
