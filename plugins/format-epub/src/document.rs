use alloc::borrow::Cow;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::{Cell, RefCell};

use paperpaw_sdk::digest::fnv1a64;
use paperpaw_sdk::document::{
    Block, Document, DocumentError, EmbeddedFont, ImageRef, Link, Locator, Metadata, TocEntry,
};
use paperpaw_sdk::image::Fit;
use paperpaw_sdk::zip::ZipArchive;
use paperpaw_sdk::{Env, Storage};

use crate::PLUGIN;
use crate::cached;
use crate::convert::{self, CachePaths, INDEX};
use crate::obfuscation;
use crate::package::{self, Chapter};

/// Largest embedded font read.
const MAX_FONT: u64 = 16 * 1024 * 1024;

/// Marks a chapter whose converted length is not known yet.
const UNKNOWN: u32 = u32::MAX;
/// Marks a chapter that could not be converted since the book was opened:
/// it is not tried again on every look at it, only when the book is
/// opened again.
const FAILED: u32 = u32::MAX - 1;

/// An open EPUB. The package is read once into the cache; chapters are
/// converted on first use and read from the cache afterwards. In memory:
/// 8 bytes per archive entry and 12 per chapter.
#[derive(Debug)]
pub struct EpubDocument {
    archive: ZipArchive,
    /// The spine: archive record and uncompressed size per chapter.
    spine: Vec<Chapter>,
    /// Length of each converted chapter's text, once known.
    lengths: Vec<Cell<u32>>,
    metadata: Metadata,
    toc_len: u32,
    cache: String,
    /// Where converted chapters are: `cache`, or its `plain` folder when
    /// the book's styles are off.
    chapters: String,
    /// Whether the book's CSS applies.
    styles: bool,
    /// Pictures in colour (ADR 0015).
    color: bool,
    /// Pictures decoded at most this large.
    image_fit: Fit,
    /// The cover decoded at most this large.
    cover_fit: Fit,
    /// Archive path of the cover image.
    cover: Option<String>,
    /// One's own stylesheets for this book (see [`user_css`]), applied
    /// after the book's.
    user_css: String,
    /// The printed pages of one section, resolved.
    pages: core::cell::RefCell<Option<SectionPages>>,
    /// Where an anchor was found last.
    anchors: cached::AnchorHint,
    /// The chapter read last, known: its paths, text length and number of
    /// blocks, and the block found last in it. A page is laid out a block
    /// at a time, the next one mostly right after the last.
    known: core::cell::RefCell<Option<Known>>,
}

/// A converted chapter, as [`EpubDocument::known`] keeps it.
#[derive(Debug)]
struct Known {
    section: u32,
    paths: CachePaths,
    length: u32,
    blocks: u32,
    last: Option<u32>,
}

/// The printed pages of a section: where each begins and where its line
/// is in `pages.txt`, for its label; the line of the last page before the
/// section.
#[derive(Debug)]
struct SectionPages {
    section: u32,
    before: Option<u64>,
    here: Vec<(u32, u64)>,
}

/// Printed pages resolved in a section at most.
const MAX_SECTION_PAGES: usize = 2048;

fn paths(cache: &str, section: u32) -> CachePaths {
    CachePaths::new(cache, &alloc::format!("{section}"))
}

/// What an XPointer points at: the section, the element path below
/// `<body>`, and the text node in it with the character, if it points
/// into text.
struct XPointer {
    section: u32,
    element: String,
    text: Option<(u32, u32)>,
}

impl XPointer {
    /// The text node as `<n>.xpt` names it, e.g. `div[1]/p[5]/text()[1]`.
    fn node(&self) -> Option<(String, u32)> {
        let (index, char) = self.text?;
        let node = if self.element.is_empty() {
            alloc::format!("text()[{index}]")
        } else {
            alloc::format!("{}/text()[{index}]", self.element)
        };
        Some((node, char))
    }
}

/// The parts of an XPointer such as
/// `/body/DocFragment[3]/body/div/p[5]/text().12`: indexes are filled in
/// (`div` is `div[1]`, `text()` is `text()[1]`).
fn parse_xpointer(xpointer: &str) -> Option<XPointer> {
    let rest = xpointer.trim().strip_prefix("/body/DocFragment")?;
    let (index, rest) = match rest.strip_prefix('[') {
        Some(r) => {
            let (n, r) = r.split_once(']')?;
            (n.parse::<u32>().ok()?, r)
        }
        None => (1, rest),
    };
    let rest = rest.strip_prefix("/body").unwrap_or(rest);
    let mut segments: Vec<String> = Vec::new();
    let mut text = None;
    for segment in rest.split('/').filter(|s| !s.is_empty()) {
        if let Some(node) = segment.strip_prefix("text()") {
            // `text()[2].5`, `text().5`
            let (index, char) = match node.strip_prefix('[') {
                Some(r) => {
                    let (n, r) = r.split_once(']')?;
                    (n.parse::<u32>().ok()?, r)
                }
                None => (1, node),
            };
            let char = char.strip_prefix('.').and_then(|c| c.parse::<u32>().ok());
            text = char.map(|char| (index, char));
            break;
        }
        // A position inside an element (`p[3].5`) counts as the element.
        let segment = segment.split('.').next().unwrap_or(segment);
        if segment.contains('[') {
            segments.push(String::from(segment));
        } else {
            segments.push(alloc::format!("{segment}[1]"));
        }
    }
    Some(XPointer {
        section: index.checked_sub(1)?,
        element: segments.join("/"),
        text,
    })
}

/// A line of `<n>.xpt`: offset, text node, character.
fn origin(line: &str) -> Option<(u32, &str, u32)> {
    let mut fields = line.split('\t');
    let offset = fields.next()?.parse().ok()?;
    let node = fields.next()?;
    let char = fields.next()?.parse().ok()?;
    Some((offset, node, char))
}

/// A line of `<n>.pos`: offset, element path.
fn position(line: &str) -> Option<(u32, &str)> {
    let (offset, path) = line.split_once('\t')?;
    Some((offset.parse().ok()?, path))
}

/// The most of a block's text read to count its characters, more than a
/// block holds: a position further from where its text node is listed
/// is past the chapter.
const MAX_SPAN: u32 = 16 * 1024;

/// The cache of a book.
fn book_cache(path: &str) -> String {
    alloc::format!(
        "{}/{}",
        Env::plugin_dir(PLUGIN.as_str()),
        paperpaw_sdk::library::key(path)
    )
}

/// The cache of a book's chapters converted with its styles or not, and
/// one's own stylesheets: chapters converted with them go into a cache of
/// their own, so a changed stylesheet applies at once.
fn chapters_cache(cache: &str, styles: bool, user_css: &str) -> String {
    if !styles {
        plain_cache(cache)
    } else if user_css.is_empty() {
        String::from(cache)
    } else {
        alloc::format!("{cache}/css-{:016x}", fnv1a64(user_css))
    }
}

/// One's own stylesheets for the book at `path` (see [`user_css`]), none
/// without the book's `styles`.
fn styles_css(storage: &dyn Storage, path: &str, styles: bool, plugin_css: &str) -> String {
    if styles {
        user_css(storage, path, plugin_css)
    } else {
        String::new()
    }
}

/// One's own stylesheets as [`EpubDocument::prepared_keeping`] read them,
/// for the opening of the book that follows at once: so their files are
/// looked for once per opening, not twice.
#[derive(Debug, Default)]
pub(crate) struct PreparedCss(RefCell<Option<KeptCss>>);

/// The book, the options' stylesheets and one's own for them.
#[derive(Debug)]
struct KeptCss {
    path: String,
    styles: bool,
    plugin_css: String,
    user_css: String,
}

impl PreparedCss {
    /// One's own stylesheets if kept for this opening; forgotten either
    /// way, so they are never older than one opening.
    fn take(&self, path: &str, styles: bool, plugin_css: &str) -> Option<String> {
        let kept = self.0.borrow_mut().take()?;
        (kept.path == path && kept.styles == styles && kept.plugin_css == plugin_css)
            .then_some(kept.user_css)
    }
}

/// Removes the folders of chapters converted with stylesheets of one's
/// own in `cache`, but `keep`.
fn remove_other_css(storage: &dyn Storage, cache: &str, keep: &str) {
    let mut gone = Vec::new();
    let _ = storage.list(cache, &mut |entry| {
        if entry.is_dir && entry.name.starts_with("css-") {
            gone.push(alloc::format!("{cache}/{}", entry.name));
        }
    });
    for dir in gone.iter().filter(|dir| *dir != keep) {
        let _ = storage.remove_dir_all(dir);
    }
}

/// Where chapters converted without the book's styles are kept.
fn plain_cache(cache: &str) -> String {
    alloc::format!("{cache}/plain")
}

impl EpubDocument {
    pub fn open(storage: &dyn Storage, path: &str) -> Result<Self, DocumentError> {
        Self::open_with(storage, path, true)
    }

    /// Opens a book; without `styles` its CSS is ignored (chapters are
    /// then converted into a cache of their own).
    pub fn open_with(
        storage: &dyn Storage,
        path: &str,
        styles: bool,
    ) -> Result<Self, DocumentError> {
        Self::open_styled(
            storage,
            path,
            styles,
            styles_css(storage, path, styles, ""),
            false,
            (convert::IMAGE_FIT, paperpaw_sdk::COVER_FIT),
        )
    }

    /// Opens a book as `options` say: with its styles or not, and with
    /// stylesheets to apply after its own (e.g. the plugins').
    pub fn open_options(
        storage: &dyn Storage,
        path: &str,
        options: &paperpaw_sdk::OpenOptions,
    ) -> Result<Self, DocumentError> {
        Self::open_prepared(storage, path, options, &PreparedCss::default())
    }

    /// As [`EpubDocument::open_options`], with one's own stylesheets as
    /// `prepared` kept them for this book, if it did.
    pub(crate) fn open_prepared(
        storage: &dyn Storage,
        path: &str,
        options: &paperpaw_sdk::OpenOptions,
        prepared: &PreparedCss,
    ) -> Result<Self, DocumentError> {
        let styles = options.publisher_styles;
        let user_css = prepared
            .take(path, styles, &options.css)
            .unwrap_or_else(|| styles_css(storage, path, styles, &options.css));
        Self::open_styled(
            storage,
            path,
            styles,
            user_css,
            options.color,
            (options.image_fit, options.cover_fit),
        )
    }

    /// Whether the book opens at once as `options` say: its first chapter
    /// is converted. Converting it (and decoding its pictures, often a
    /// large cover) takes a while the first time.
    pub fn prepared(
        storage: &dyn Storage,
        path: &str,
        options: &paperpaw_sdk::OpenOptions,
    ) -> bool {
        Self::prepared_keeping(storage, path, options, &PreparedCss::default())
    }

    /// As [`EpubDocument::prepared`]; where the book opens at once, one's
    /// own stylesheets are kept in `kept` for that opening.
    pub(crate) fn prepared_keeping(
        storage: &dyn Storage,
        path: &str,
        options: &paperpaw_sdk::OpenOptions,
        kept: &PreparedCss,
    ) -> bool {
        let styles = options.publisher_styles;
        let user_css = styles_css(storage, path, styles, &options.css);
        let chapters = chapters_cache(&book_cache(path), styles, &user_css);
        let prepared = storage.exists(&paths(&chapters, 0).end);
        *kept.0.borrow_mut() = prepared.then(|| KeptCss {
            path: String::from(path),
            styles,
            plugin_css: options.css.clone(),
            user_css,
        });
        prepared
    }

    fn open_styled(
        storage: &dyn Storage,
        path: &str,
        styles: bool,
        user_css: String,
        color: bool,
        (image_fit, cover_fit): (Fit, Fit),
    ) -> Result<Self, DocumentError> {
        let archive = ZipArchive::open(storage, path).map_err(package::error)?;
        let cache = book_cache(path);
        let book = crate::fingerprint(storage, path)?;
        let package = match package::load(storage, &cache, &book)? {
            Some(package) => package,
            None => {
                // Another book, file version or cache format: start over,
                // also with its pictures and its chapters converted with
                // stylesheets or without.
                let _ = storage.remove_dir_all(&cache);
                package::build(storage, &archive, &cache, &book)?
            }
        };
        let chapters = chapters_cache(&cache, styles, &user_css);
        if chapters.contains("/css-") && !storage.exists(&paths(&chapters, 0).end) {
            // Stylesheets of one's own changed: the chapters converted
            // with those before go.
            remove_other_css(storage, &cache, &chapters);
        }
        let mut metadata = package.metadata;
        if metadata.title.is_none() {
            let name = path.rsplit('/').next().unwrap_or(path);
            metadata.title = Some(String::from(name.rsplit_once('.').map_or(name, |(s, _)| s)));
        }
        let mut lengths = Vec::new();
        lengths
            .try_reserve_exact(package.spine.len())
            .map_err(|_| DocumentError::OutOfMemory)?;
        lengths.resize_with(package.spine.len(), || Cell::new(UNKNOWN));
        Ok(EpubDocument {
            lengths,
            spine: package.spine,
            metadata,
            toc_len: package.toc_len,
            cover: package.cover,
            archive,
            cache,
            chapters,
            styles,
            color,
            image_fit,
            cover_fit,
            user_css,
            pages: core::cell::RefCell::new(None),
            anchors: cached::AnchorHint::default(),
            known: core::cell::RefCell::new(None),
        })
    }

    fn paths(&self, section: u32) -> CachePaths {
        paths(&self.chapters, section)
    }

    fn length(&self, section: u32) -> Option<u32> {
        let length = self.lengths.get(section as usize)?.get();
        (length != UNKNOWN && length != FAILED).then_some(length)
    }

    /// Converts a chapter if needed; returns its paths, text length and
    /// number of blocks.
    fn chapter(
        &self,
        storage: &dyn Storage,
        section: u32,
    ) -> Result<(CachePaths, u32, u32), DocumentError> {
        let (Some(chapter), Some(slot)) = (
            self.spine.get(section as usize),
            self.lengths.get(section as usize),
        ) else {
            return Err(DocumentError::Malformed(Cow::Borrowed("no such chapter")));
        };
        let paths = self.paths(section);
        let length = match cached::converted_length(storage, &paths) {
            Some(length) => length,
            None if slot.get() == FAILED => {
                return Err(DocumentError::Malformed(Cow::Borrowed(
                    "chapter cannot be read",
                )));
            }
            None => {
                self.anchors.reset();
                let entry = self
                    .archive
                    .entry_by_record(storage, chapter.record)
                    .map_err(package::error)?;
                let mut rules = if self.styles {
                    package::styles(storage, &self.cache)?
                } else {
                    Default::default()
                };
                // One's own after the book's: they win.
                if !self.user_css.is_empty() {
                    rules.add_own(&self.user_css);
                }
                let source = convert::Source::Zip {
                    archive: &self.archive,
                    entry: &entry,
                };
                let converted = convert::convert(
                    storage,
                    &source,
                    &paths,
                    rules,
                    self.styles,
                    self.color,
                    self.image_fit,
                    convert::BookText::of(self.metadata.language.as_deref()),
                );
                // Short of memory, it is tried again: with more free, it
                // may convert.
                converted.inspect_err(|e| {
                    if !matches!(e, DocumentError::OutOfMemory) {
                        slot.set(FAILED);
                    }
                })?
            }
        };
        slot.set(length);
        let blocks = (storage.size(&paths.index)? / u64::from(INDEX)) as u32;
        Ok((paths, length, blocks))
    }

    /// The offset of anchor `id` in a chapter.
    fn anchor(&self, storage: &dyn Storage, section: u32, id: &str) -> Option<u32> {
        let (paths, ..) = self.chapter(storage, section).ok()?;
        cached::anchor(storage, &paths, section, id, &self.anchors)
    }
}

/// Longest stylesheet of one's own that is read.
const MAX_USER_CSS: usize = 64 * 1024;

/// The stylesheets for the book at `path` after its own: `plugins'`
/// (e.g. a theme's), then one's own: `styles.css` in its folder (for all
/// books there), then the one named like the book (`Alice.css` for
/// `Alice.epub`). The later win.
pub(crate) fn user_css(storage: &dyn Storage, path: &str, plugins: &str) -> String {
    let folder = paperpaw_sdk::files::parent(path);
    let name = paperpaw_sdk::files::name(path);
    let stem = name.rsplit_once('.').map_or(name, |(s, _)| s);
    let mut css = String::from(plugins);
    if !css.is_empty() {
        css.push('\n');
    }
    for file in [String::from("styles.css"), alloc::format!("{stem}.css")] {
        let file = paperpaw_sdk::files::join(folder, &file);
        if let Ok(data) = paperpaw_sdk::read_file(storage, &file, MAX_USER_CSS) {
            css.push_str(&String::from_utf8_lossy(&data));
            css.push('\n');
        }
    }
    css
}

impl EpubDocument {
    /// The printed pages listed for `section`. The list is read a line at
    /// a time, the labels when they are shown.
    fn section_pages(&self, storage: &dyn Storage, section: u32) -> SectionPages {
        let mut pages = SectionPages {
            section,
            before: None,
            here: Vec::new(),
        };
        let mut at = 0;
        cached::lines_from(storage, &self.pages_path(), 0, |line, next| {
            let line_at = core::mem::replace(&mut at, next);
            let mut fields = line.splitn(3, '\t');
            let (Some(Ok(s)), Some(fragment)) =
                (fields.next().map(str::parse::<u32>), fields.next())
            else {
                return true;
            };
            if s < section {
                pages.before = Some(line_at);
            } else if s == section
                && pages.here.len() < MAX_SECTION_PAGES
                && let Some(offset) = self.anchor(storage, section, fragment)
            {
                pages.here.push((offset, line_at));
            }
            true
        });
        pages.here.sort_by_cached_key(|(offset, _)| *offset);
        pages
    }

    fn pages_path(&self) -> String {
        alloc::format!("{}/pages.txt", self.cache)
    }
}

impl Document for EpubDocument {
    fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    fn page_label(&self, storage: &dyn Storage, at: Locator) -> Option<String> {
        if at.section >= self.sections() {
            return None;
        }
        // One section's pages are resolved once.
        let mut pages = self.pages.borrow_mut();
        if pages.as_ref().is_none_or(|p| p.section != at.section) {
            *pages = Some(self.section_pages(storage, at.section));
        }
        let pages = pages.as_ref()?;
        let line = pages
            .here
            .iter()
            .take_while(|(offset, _)| *offset <= at.offset)
            .last()
            .map(|(_, line)| *line)
            .or(pages.before)?;
        let mut label = None;
        cached::lines_from(storage, &self.pages_path(), line, |line, _| {
            label = line.splitn(3, '\t').nth(2).map(String::from);
            false
        });
        label
    }

    fn prepare_image(&self, storage: &dyn Storage, image: &ImageRef) -> Option<ImageRef> {
        if paperpaw_sdk::image::ImageFile::open(storage, &image.path).is_some() {
            return None;
        }
        let base = image.path.rsplit_once('.')?.0;
        let (dir, _) = base.rsplit_once('/')?;
        if crate::pictures::failed(storage, base) {
            return None;
        }
        let (_, source) = crate::pictures::noted(storage, base)?;
        let file = crate::pictures::file(base, self.color);
        convert::decode_image(
            storage,
            &self.archive,
            &source,
            &file,
            dir,
            self.image_fit,
            self.color,
        )
        .inspect_err(|e| crate::pictures::note_failure(storage, base, e))
        .ok()?;
        crate::pictures::open(storage, base, self.color)
    }

    fn cover(&self, storage: &dyn Storage) -> Result<Option<ImageRef>, DocumentError> {
        let Some(source) = &self.cover else {
            return Ok(None);
        };
        // Named by its size: decoded anew for a screen that shows it larger.
        let base = alloc::format!("{}/cover-{}", self.cache, self.cover_fit.long);
        if crate::pictures::failed(storage, &base) {
            return Ok(None);
        }
        if !crate::pictures::has(storage, &base, self.color) {
            convert::decode_image(
                storage,
                &self.archive,
                source,
                &crate::pictures::file(&base, self.color),
                &self.cache,
                self.cover_fit,
                self.color,
            )
            .inspect_err(|e| crate::pictures::note_failure(storage, &base, e))?;
        }
        Ok(crate::pictures::open(storage, &base, self.color))
    }

    fn sections(&self) -> u32 {
        self.spine.len() as u32
    }

    fn section_len(&self, section: u32) -> u32 {
        self.length(section)
            .unwrap_or_else(|| self.spine.get(section as usize).map_or(0, |c| c.size))
    }

    /// Down to the character in its text node, as KOReader counts them,
    /// where it is in text; else the element of its block. The lists of
    /// places are read a line at a time, never whole.
    fn xpointer(&self, storage: &dyn Storage, at: Locator) -> Option<String> {
        let (paths, ..) = self.chapter(storage, at.section).ok()?;
        // The block: the last listed at or before `at`.
        let (mut block, mut path) = (0, String::new());
        cached::lines(storage, &paths.positions, |line| {
            let Some((offset, element)) = position(line) else {
                return true;
            };
            if offset > at.offset {
                return false;
            }
            block = offset;
            path.clear();
            path.push_str(element);
            true
        });
        let fragment = alloc::format!("/body/DocFragment[{}]/body", at.section + 1);
        // The text node the block's text at `at` comes from.
        let mut found: Option<(u32, String, u32)> = None;
        cached::lines(storage, &paths.xpointers, |line| {
            let Some((offset, node, char)) = origin(line) else {
                return true;
            };
            if offset > at.offset {
                return false;
            }
            found = Some((offset, String::from(node), char));
            true
        });
        if let Some((offset, node, char)) =
            found.filter(|(offset, ..)| *offset >= block && at.offset - *offset <= MAX_SPAN)
        {
            let mut text = alloc::vec![0u8; (at.offset - offset) as usize];
            let n = storage
                .read_at(&paths.text, u64::from(offset), &mut text)
                .ok()?;
            let chars = String::from_utf8_lossy(&text[..n]).chars().count() as u32;
            return Some(alloc::format!("{fragment}/{node}.{}", char + chars));
        }
        if path.is_empty() {
            Some(fragment)
        } else {
            Some(alloc::format!("{fragment}/{path}"))
        }
    }

    fn locate_xpointer(&self, storage: &dyn Storage, xpointer: &str) -> Option<Locator> {
        let pointer = parse_xpointer(xpointer)?;
        let section = pointer.section;
        let (paths, ..) = self.chapter(storage, section).ok()?;
        // Exactly, where it points into text PaperPaw shows.
        if let Some((node, char)) = pointer.node() {
            let mut found: Option<(u32, u32)> = None;
            cached::lines(storage, &paths.xpointers, |line| {
                if let Some((offset, at, from)) = origin(line)
                    && at == node
                    && from <= char
                {
                    found = Some((offset, char - from));
                }
                true
            });
            if let Some((offset, chars)) = found {
                let mut text = alloc::vec![0u8; (chars as usize * 4).min(MAX_SPAN as usize)];
                let n = storage
                    .read_at(&paths.text, u64::from(offset), &mut text)
                    .ok()?;
                let text = String::from_utf8_lossy(&text[..n]);
                let bytes: usize = text.chars().take(chars as usize).map(char::len_utf8).sum();
                return Some(Locator::new(section, offset + bytes as u32));
            }
        }
        let target = pointer.element;
        let inside = |path: &str, of: &str| {
            of.is_empty() || path == of || path.strip_prefix(of).is_some_and(|r| r.starts_with('/'))
        };
        // The element's first block, else the last block it is part of.
        let (mut first, mut within) = (None, None);
        cached::lines(storage, &paths.positions, |line| {
            let Some((offset, path)) = position(line) else {
                return true;
            };
            if inside(path, &target) {
                first = Some(offset);
                return false;
            }
            if inside(&target, path) {
                within = Some(offset);
            }
            true
        });
        Some(Locator::new(section, first.or(within).unwrap_or(0)))
    }

    fn fonts(&self, storage: &dyn Storage) -> Vec<EmbeddedFont> {
        if !self.styles {
            return Vec::new();
        }
        package::fonts(storage, &self.cache)
            .1
            .into_iter()
            .map(|f| EmbeddedFont {
                family: f.family,
                bold: f.bold,
                italic: f.italic,
            })
            .collect()
    }

    fn font_data(&self, storage: &dyn Storage, index: usize) -> Result<Vec<u8>, DocumentError> {
        let (identifier, fonts) = package::fonts(storage, &self.cache);
        let font = fonts
            .get(index)
            .ok_or(DocumentError::Malformed(Cow::Borrowed("no such font")))?;
        let entry = self
            .archive
            .entry(storage, &font.path)
            .map_err(package::error)?
            .ok_or(DocumentError::Malformed(Cow::Borrowed("font file missing")))?;
        if u64::from(entry.size) > MAX_FONT {
            return Err(DocumentError::OutOfMemory);
        }
        let mut data = Vec::new();
        data.try_reserve_exact(entry.size as usize)
            .map_err(|_| DocumentError::OutOfMemory)?;
        self.archive
            .stream(storage, &entry, &mut |chunk| {
                data.extend_from_slice(chunk);
                true
            })
            .map_err(package::error)?;
        obfuscation::reveal(&mut data, font.method, &identifier);
        if data.starts_with(crate::woff::SIGNATURE) {
            return crate::woff::to_font(&data, MAX_FONT as usize)
                .ok_or(DocumentError::Malformed(Cow::Borrowed("WOFF font")));
        }
        Ok(data)
    }

    fn progress(&self, at: Locator) -> f32 {
        let total: u64 = self.spine.iter().map(|c| u64::from(c.size)).sum();
        if total == 0 {
            return 0.0;
        }
        let section = (at.section as usize).min(self.spine.len());
        let before: u64 = self.spine[..section]
            .iter()
            .map(|c| u64::from(c.size))
            .sum();
        let within = match (self.spine.get(section), self.length(section as u32)) {
            (Some(chapter), Some(length)) if length > 0 => {
                u64::from(chapter.size) * u64::from(at.offset.min(length)) / u64::from(length)
            }
            _ => 0,
        };
        ((before + within) as f32 / total as f32).min(1.0)
    }

    fn block_at(&self, storage: &dyn Storage, at: Locator) -> Result<Option<Block>, DocumentError> {
        if at.section as usize >= self.spine.len() {
            return Ok(None);
        }
        let mut known = self.known.borrow_mut();
        if known.as_ref().is_none_or(|k| k.section != at.section) {
            let (paths, length, blocks) = self.chapter(storage, at.section)?;
            *known = Some(Known {
                section: at.section,
                paths,
                length,
                blocks,
                last: None,
            });
        }
        let Some(k) = known.as_mut() else {
            return Ok(None);
        };
        cached::block_at(
            storage,
            &k.paths,
            k.length,
            k.blocks,
            at.offset,
            self.color,
            &mut k.last,
        )
    }

    fn links(&self, storage: &dyn Storage, section: u32, from: u32, to: u32) -> Vec<Link> {
        match self.chapter(storage, section) {
            Ok((paths, ..)) => cached::links(storage, &paths, from, to),
            Err(_) => Vec::new(),
        }
    }

    fn link_target(&self, storage: &dyn Storage, href: &str) -> Option<Locator> {
        if paperpaw_sdk::document::is_external(href) {
            return None;
        }
        let (path, fragment) = match href.split_once('#') {
            Some((path, fragment)) => (path, Some(fragment)),
            None => (href, None),
        };
        let entry = self.archive.entry(storage, path).ok()??;
        let section = self.spine.iter().position(|c| c.record == entry.record())? as u32;
        let offset = fragment
            .and_then(|id| self.anchor(storage, section, id))
            .unwrap_or(0);
        Some(Locator::new(section, offset))
    }

    fn toc_len(&self, _: &dyn Storage) -> u32 {
        self.toc_len
    }

    /// Points at the start of the entry's chapter; finding an anchor needs
    /// the chapter converted, which [`Document::toc_target`] does.
    fn toc_entry(&self, storage: &dyn Storage, index: u32) -> Option<TocEntry> {
        let entry = package::toc_entry(storage, &self.cache, index)?;
        Some(TocEntry {
            title: entry.title,
            level: entry.level,
            at: Locator::new(entry.section, 0),
        })
    }

    fn toc_target(&self, storage: &dyn Storage, index: u32) -> Option<Locator> {
        let entry = package::toc_entry(storage, &self.cache, index)?;
        let offset = entry
            .fragment
            .as_deref()
            .and_then(|id| self.anchor(storage, entry.section, id))
            .unwrap_or(0);
        Some(Locator::new(entry.section, offset))
    }
}
