use alloc::borrow::Cow;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::{Cell, OnceCell, RefCell};

use paperpaw_sdk::digest::fnv1a64;
use paperpaw_sdk::document::{
    Block, Document, DocumentError, EmbeddedFont, ImageRef, Link, Locator, Metadata, TocEntry,
};
use paperpaw_sdk::image::Fit;
use paperpaw_sdk::zip::ZipArchive;
use paperpaw_sdk::{Storage, StorageError};

use crate::PLUGIN;
use crate::cached;
use crate::convert::{self, CachePaths, INDEX};
use crate::fallible;
use crate::obfuscation;
use crate::package::{self, Chapter};

/// Largest embedded font read.
const MAX_FONT: u64 = 16 * 1024 * 1024;

/// The key of [`Document::extended`] that copies an embedded face to a file:
/// its parameters are the face's index (u32, little-endian) and the
/// destination path; the answer is a status byte (0 written, 1 not to be
/// streamed, 2 no room, 3 unreadable) and the size written (u32).
pub(crate) const FONT_COPY: &str = "format-epub.font-copy";

/// Marks a chapter whose converted length is not known yet.
const UNKNOWN: u32 = u32::MAX;
/// Marks a chapter that could not be converted since the book was opened:
/// it is not tried again on every look at it, only when the book is
/// opened again.
const FAILED: u32 = u32::MAX - 1;

/// What of a book is in its cache, as [`EpubDocument::state`] says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Held {
    /// Nothing yet: what was there was another book's, or left over, and
    /// went at the open.
    Nothing,
    /// A cover for the library (or its failure), with the book's stamp
    /// ([`package::stamp`]).
    Stamped,
    /// The package.
    Package,
    /// Another book's, or what was left, and it could not be removed (a
    /// damaged folder): nothing of the book is written there nor read
    /// from there.
    Uncleared(StorageError),
}

/// An open EPUB. The package is read once into the cache, when the book is
/// first read (its contents, stylesheets or a chapter wanted): opened only
/// for its details (the library's walk), nothing is written. Chapters are
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
    /// Entries of the contents, once the package is in the cache.
    toc_len: Cell<u32>,
    cache: String,
    /// The book's fingerprint, the package's in the cache; what of the
    /// book is there (see [`EpubDocument::built`]).
    book: String,
    state: Cell<Held>,
    /// The package could not be written (its contents or stylesheets are
    /// unreadable, or the card refused it, e.g. full): not tried again
    /// until the book is opened again, each try reading the package and
    /// writing to the card anew.
    broken: Cell<bool>,
    /// The book's path, and the stylesheets to apply after its own (e.g.
    /// the plugins'): for [`EpubDocument::styled`].
    path: String,
    plugin_css: String,
    /// Where converted chapters are and one's own stylesheets, once a
    /// chapter is converted or read.
    styled: OnceCell<Styled>,
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
    /// The printed pages of one section, resolved.
    pages: core::cell::RefCell<Option<SectionPages>>,
    /// Where an anchor was found last.
    anchors: cached::AnchorHint,
    /// The chapter read last, known: its paths, text length and number of
    /// blocks, and the block found last in it. A page is laid out a block
    /// at a time, the next one mostly right after the last.
    known: core::cell::RefCell<Option<Known>>,
}

/// Where a book's chapters are converted to, and one's own stylesheets
/// for it (see [`user_css`]), applied after the book's. Looked for when a
/// chapter is first needed, not when the book is opened: its details (for
/// the library, say) need none, and a stylesheet not there is looked for
/// through the whole of its folder.
#[derive(Debug)]
struct Styled {
    /// `cache`, its `plain` folder when the book's styles are off, or one
    /// for one's own stylesheets (see [`chapters_cache`]).
    chapters: String,
    user_css: String,
}

impl Styled {
    fn new(
        storage: &dyn Storage,
        cache: &str,
        styles: bool,
        user_css: String,
    ) -> Result<Self, DocumentError> {
        let chapters = chapters_cache(cache, styles, &user_css)?;
        if chapters.contains("/css-")
            && paths(&chapters, 0).is_ok_and(|paths| !storage.exists(&paths.end))
        {
            // Stylesheets of one's own changed: the chapters converted
            // with those before go.
            remove_other_css(storage, cache, &chapters);
        }
        Ok(Styled { chapters, user_css })
    }
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

fn paths(cache: &str, section: u32) -> Result<CachePaths, DocumentError> {
    CachePaths::new(cache, &crate::fallible::try_format!("{section}")?)
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
            fallible::try_format!("text()[{index}]").ok()?
        } else {
            fallible::try_format!("{}/text()[{index}]", self.element).ok()?
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
            fallible::push(&mut segments, fallible::string(segment).ok()?).ok()?;
        } else {
            fallible::push(&mut segments, fallible::try_format!("{segment}[1]").ok()?).ok()?;
        }
    }
    Some(XPointer {
        section: index.checked_sub(1)?,
        element: {
            let mut element = String::new();
            for (i, segment) in segments.iter().enumerate() {
                if i > 0 {
                    fallible::push_char(&mut element, '/').ok()?;
                }
                fallible::push_str(&mut element, segment).ok()?;
            }
            element
        },
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

/// Whether anything may be in folder `dir`: something listed, or it
/// cannot be listed (but is there).
fn left_in(storage: &dyn Storage, dir: &str) -> bool {
    let mut any = false;
    match storage.list(dir, &mut |_| any = true) {
        Ok(()) | Err(StorageError::NotFound) => any,
        Err(_) => true,
    }
}

/// The cache of a book.
fn book_cache(path: &str) -> Result<String, DocumentError> {
    fallible::kept_path(&fallible::plugin_dir(PLUGIN.as_str())?, "", path)
}

/// The cache of a book's chapters converted with its styles or not, and
/// one's own stylesheets: chapters converted with them go into a cache of
/// their own, so a changed stylesheet applies at once.
fn chapters_cache(cache: &str, styles: bool, user_css: &str) -> Result<String, DocumentError> {
    if !styles {
        plain_cache(cache)
    } else if user_css.is_empty() {
        fallible::string(cache)
    } else {
        fallible::try_format!("{cache}/css-{:016x}", fnv1a64(user_css))
    }
}

/// One's own stylesheets for the book at `path` (see [`user_css`]), none
/// without the book's `styles`.
fn styles_css(
    storage: &dyn Storage,
    path: &str,
    styles: bool,
    plugin_css: &str,
) -> Result<String, DocumentError> {
    if styles {
        user_css(storage, path, plugin_css)
    } else {
        Ok(String::new())
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
fn plain_cache(cache: &str) -> Result<String, DocumentError> {
    fallible::try_format!("{cache}/plain")
}

impl EpubDocument {
    /// Copies the face named by `params` (see [`FONT_COPY`]) to its file in
    /// chunks as it is inflated, through a temporary name that is renamed at
    /// the end and removed on any error: no more than a chunk is held. `None`
    /// for a face that is not a plain font file (a WOFF is converted whole).
    fn copy_font(
        &self,
        storage: &dyn Storage,
        params: &[u8],
    ) -> Result<Option<u32>, DocumentError> {
        let bad = || DocumentError::Malformed(Cow::Borrowed("font copy"));
        let index = params
            .get(..4)
            .and_then(|b| b.try_into().ok())
            .map(u32::from_le_bytes)
            .ok_or_else(bad)? as usize;
        let dest = core::str::from_utf8(params.get(4..).ok_or_else(bad)?).map_err(|_| bad())?;
        self.built(storage)?;
        let (identifier, fonts) = package::fonts(storage, &self.cache)?;
        let font = fonts
            .get(index)
            .ok_or(DocumentError::Malformed(Cow::Borrowed("no such font")))?;
        let entry = self
            .archive
            .entry(storage, &font.path)
            .map_err(package::error)?
            .ok_or(DocumentError::Malformed(Cow::Borrowed("font file missing")))?;
        // A file too large is the book's fault, not a short heap.
        if u64::from(entry.size) > MAX_FONT {
            return Err(bad());
        }
        let temp = crate::fallible::try_format!("{dest}.tmp")?;
        storage.write(&temp, &[])?;
        let mut written = 0usize;
        let mut woff = false;
        let mut failure: Option<DocumentError> = None;
        let streamed = self.archive.stream(storage, &entry, &mut |chunk| {
            // The chunk is the zip reader's own window: revealed in a small
            // buffer, a part at a time.
            let mut part = [0u8; 512];
            for piece in chunk.chunks(part.len()) {
                let buf = &mut part[..piece.len()];
                buf.copy_from_slice(piece);
                obfuscation::reveal_at(buf, written, font.method, &identifier);
                if written == 0 && buf.starts_with(crate::woff::SIGNATURE) {
                    woff = true;
                    return false;
                }
                if let Err(e) = storage.append(&temp, buf) {
                    failure = Some(e.into());
                    return false;
                }
                written += buf.len();
            }
            true
        });
        let outcome = match (streamed, failure) {
            (_, Some(error)) => Err(error),
            (Err(error), None) => Err(package::error(error)),
            (Ok(()), None) if woff => Ok(None),
            (Ok(()), None) if written == 0 => Err(bad()),
            (Ok(()), None) => storage
                .rename(&temp, dest)
                .map(|()| u32::try_from(written).ok())
                .map_err(Into::into),
        };
        if !matches!(outcome, Ok(Some(_))) {
            let _ = storage.remove(&temp);
        }
        outcome
    }

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
            (styles, "", None),
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
        let user_css = prepared.take(path, styles, &options.css);
        Self::open_styled(
            storage,
            path,
            (styles, &options.css, user_css),
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
        // No room to tell finds the book not prepared (it opens as any).
        Self::try_prepared_keeping(storage, path, options, kept).unwrap_or(false)
    }

    fn try_prepared_keeping(
        storage: &dyn Storage,
        path: &str,
        options: &paperpaw_sdk::OpenOptions,
        kept: &PreparedCss,
    ) -> Result<bool, DocumentError> {
        let styles = options.publisher_styles;
        let user_css = styles_css(storage, path, styles, &options.css)?;
        let chapters = chapters_cache(&book_cache(path)?, styles, &user_css)?;
        let prepared = paths(&chapters, 0).is_ok_and(|paths| storage.exists(&paths.end));
        let keep = if prepared {
            Some(KeptCss {
                path: fallible::string(path)?,
                styles,
                plugin_css: fallible::string(&options.css)?,
                user_css,
            })
        } else {
            None
        };
        *kept.0.borrow_mut() = keep;
        Ok(prepared)
    }

    /// Opens a book with its `styles` or not, the stylesheets to apply
    /// after its own and one's own (looked for later where not given).
    fn open_styled(
        storage: &dyn Storage,
        path: &str,
        (styles, plugin_css, user_css): (bool, &str, Option<String>),
        color: bool,
        (image_fit, cover_fit): (Fit, Fit),
    ) -> Result<Self, DocumentError> {
        let archive = ZipArchive::open(storage, path).map_err(package::error)?;
        let cache = book_cache(path)?;
        let book = fallible::fingerprint(storage, path)?;
        let (package, state) = match package::load(storage, &cache, &book)? {
            Some(package) => (package, Held::Package),
            // A cover made for the library before the book was read stays
            // (the library's details point at it).
            None if !package::cached(storage, &cache)
                && package::stamped(storage, &cache, &book) =>
            {
                (package::read(storage, &archive)?, Held::Stamped)
            }
            None => {
                // Another book, file version or cache format, or what a
                // removal cut off left: start over, also with its pictures
                // and its chapters converted with stylesheets or without.
                let state = match storage.remove_dir_all(&cache) {
                    Ok(()) => Held::Nothing,
                    // Not there, or empty (a storage that removes no
                    // folders): nothing to read from. One that cannot be
                    // listed may hold anything.
                    Err(e) if left_in(storage, &cache) => Held::Uncleared(e),
                    Err(_) => Held::Nothing,
                };
                (package::read(storage, &archive)?, state)
            }
        };
        let styled = OnceCell::new();
        if let Some(user_css) = user_css {
            let _ = styled.set(Styled::new(storage, &cache, styles, user_css)?);
        }
        let mut metadata = package.metadata;
        if metadata.title.is_none() {
            let name = path.rsplit('/').next().unwrap_or(path);
            metadata.title = Some(fallible::string(
                name.rsplit_once('.').map_or(name, |(s, _)| s),
            )?);
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
            toc_len: Cell::new(package.toc_len),
            cover: package.cover,
            archive,
            cache,
            book,
            state: Cell::new(state),
            broken: Cell::new(false),
            path: fallible::string(path)?,
            plugin_css: fallible::string(plugin_css)?,
            styled,
            styles,
            color,
            image_fit,
            cover_fit,
            pages: core::cell::RefCell::new(None),
            anchors: cached::AnchorHint::default(),
            known: core::cell::RefCell::new(None),
        })
    }

    /// The package in the cache, written the first time the book is read
    /// (see [`EpubDocument`]); an error where it cannot be.
    fn built(&self, storage: &dyn Storage) -> Result<(), DocumentError> {
        match self.state.get() {
            Held::Package => return Ok(()),
            Held::Uncleared(e) => return Err(DocumentError::Storage(e)),
            Held::Nothing | Held::Stamped => {}
        }
        if self.broken.get() {
            return Err(DocumentError::Malformed(Cow::Borrowed(
                "contents cannot be read",
            )));
        }
        match package::build(storage, &self.archive, &self.cache, &self.book) {
            Ok(package) => {
                self.toc_len.set(package.toc_len);
                self.state.set(Held::Package);
                Ok(())
            }
            // Perhaps with more room later.
            Err(e @ DocumentError::OutOfMemory) => Err(e),
            Err(e) => {
                self.broken.set(true);
                Err(e)
            }
        }
    }

    /// The cache marked as this book's before anything of it is written
    /// without the package (see [`package::stamp`]).
    fn stamped(&self, storage: &dyn Storage) -> Result<(), DocumentError> {
        match self.state.get() {
            Held::Nothing => {
                package::stamp(storage, &self.cache, &self.book)?;
                self.state.set(Held::Stamped);
            }
            Held::Uncleared(e) => return Err(DocumentError::Storage(e)),
            Held::Stamped | Held::Package => {}
        }
        Ok(())
    }

    /// Where its chapters are converted to and one's own stylesheets:
    /// looked for the first time they are needed.
    fn styled(&self, storage: &dyn Storage) -> Result<&Styled, DocumentError> {
        if let Some(styled) = self.styled.get() {
            return Ok(styled);
        }
        let user_css = styles_css(storage, &self.path, self.styles, &self.plugin_css)?;
        let styled = Styled::new(storage, &self.cache, self.styles, user_css)?;
        Ok(self.styled.get_or_init(|| styled))
    }

    fn paths(&self, storage: &dyn Storage, section: u32) -> Result<CachePaths, DocumentError> {
        paths(&self.styled(storage)?.chapters, section)
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
        self.built(storage)?;
        let paths = self.paths(storage, section)?;
        let length = match cached::converted_length(storage, &paths) {
            Some(length) => length,
            // Converted by this very book, but not marked complete (a
            // picture found no room to be decoded): not again each time.
            None if slot.get() != UNKNOWN
                && slot.get() != FAILED
                && storage.size(&paths.text).ok() == Some(u64::from(slot.get())) =>
            {
                slot.get()
            }
            None if slot.get() == FAILED => {
                return Err(DocumentError::Malformed(Cow::Borrowed(
                    "chapter cannot be read",
                )));
            }
            None => {
                self.anchors.reset();
                // What this book once converted is not matched against the
                // files of a conversion begun now (and refused midway).
                slot.set(UNKNOWN);
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
                let own = &self.styled(storage)?.user_css;
                if !own.is_empty() {
                    rules.add_own(own);
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
pub(crate) fn user_css(
    storage: &dyn Storage,
    path: &str,
    plugins: &str,
) -> Result<String, DocumentError> {
    let folder = paperpaw_sdk::files::parent(path);
    let name = paperpaw_sdk::files::name(path);
    let stem = name.rsplit_once('.').map_or(name, |(s, _)| s);
    let mut css = fallible::string(plugins)?;
    if !css.is_empty() {
        fallible::push_char(&mut css, '\n')?;
    }
    for file in [
        fallible::string("styles.css")?,
        fallible::try_format!("{stem}.css")?,
    ] {
        let file = fallible::join(folder, &file)?;
        if let Ok(data) = paperpaw_sdk::read_file(storage, &file, MAX_USER_CSS) {
            fallible::push_str(&mut css, &fallible::lossy(&data)?)?;
            fallible::push_char(&mut css, '\n')?;
        }
    }
    Ok(css)
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
        let Some(path) = self.pages_path(storage) else {
            return pages;
        };
        let mut at = 0;
        // No room for a line leaves the pages found so far.
        let _ = cached::lines_from(storage, &path, 0, |line, next| {
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
        paperpaw_sdk::sort::sort_by_cached_key(&mut pages.here, |(offset, _)| *offset);
        pages
    }

    /// Where the printed pages are listed, with the contents: none where
    /// they cannot be.
    fn pages_path(&self, storage: &dyn Storage) -> Option<String> {
        self.built(storage).ok()?;
        fallible::try_format!("{}/pages.txt", self.cache).ok()
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
        // No room for the line or its copy finds no label.
        let _ = cached::lines_from(storage, &self.pages_path(storage)?, line, |line, _| {
            label = line
                .splitn(3, '\t')
                .nth(2)
                .and_then(|text| crate::fallible::string(text).ok());
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

    fn prepare_image_larger(
        &self,
        storage: &dyn Storage,
        image: &ImageRef,
        long: u32,
    ) -> Option<ImageRef> {
        let base = image.path.rsplit_once('.')?.0;
        let (dir, _) = base.rsplit_once('/')?;
        let (_, source) = crate::pictures::noted(storage, base)?;
        // Named by its size, beside the one for the page; no larger than
        // pictures are decoded at most.
        let fit = Fit::side(long).min(convert::IMAGE_FIT);
        let larger = alloc::format!("{base}-{}", fit.long);
        if crate::pictures::failed(storage, &larger) {
            return None;
        }
        if let Some(decoded) = crate::pictures::open(storage, &larger, self.color) {
            return Some(decoded);
        }
        let file = crate::pictures::file(&larger, self.color);
        convert::decode_image(storage, &self.archive, &source, &file, dir, fit, self.color)
            .inspect_err(|e| crate::pictures::note_failure(storage, &larger, e))
            .ok()?;
        crate::pictures::open(storage, &larger, self.color)
    }

    fn cover(&self, storage: &dyn Storage) -> Result<Option<ImageRef>, DocumentError> {
        let Some(source) = &self.cover else {
            return Ok(None);
        };
        // Named by its size: decoded anew for a screen that shows it larger.
        // What is in the cache is this book's (see `open_styled`).
        let base = fallible::try_format!("{}/cover-{}", self.cache, self.cover_fit.long)?;
        if let Held::Uncleared(e) = self.state.get() {
            return Err(DocumentError::Storage(e));
        }
        if crate::pictures::failed(storage, &base) {
            return Ok(None);
        }
        if !crate::pictures::has(storage, &base, self.color)? {
            self.stamped(storage)?;
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
        let mut no_room = false;
        let _ = cached::lines(storage, &paths.positions, |line| {
            let Some((offset, element)) = position(line) else {
                return true;
            };
            if offset > at.offset {
                return false;
            }
            block = offset;
            path.clear();
            no_room = fallible::push_str(&mut path, element).is_err();
            !no_room
        });
        let fragment = fallible::try_format!("/body/DocFragment[{}]/body", at.section + 1).ok()?;
        // The text node the block's text at `at` comes from.
        let mut found: Option<(u32, String, u32)> = None;
        let _ = cached::lines(storage, &paths.xpointers, |line| {
            let Some((offset, node, char)) = origin(line) else {
                return true;
            };
            if offset > at.offset {
                return false;
            }
            match fallible::string(node) {
                Ok(node) => found = Some((offset, node, char)),
                Err(_) => no_room = true,
            }
            !no_room
        });
        if no_room {
            return None;
        }
        if let Some((offset, node, char)) =
            found.filter(|(offset, ..)| *offset >= block && at.offset - *offset <= MAX_SPAN)
        {
            let mut text = fallible::zeroed((at.offset - offset) as usize).ok()?;
            let n = storage
                .read_at(&paths.text, u64::from(offset), &mut text)
                .ok()?;
            let chars = fallible::lossy(&text[..n]).ok()?.chars().count() as u32;
            return fallible::try_format!("{fragment}/{node}.{}", char + chars).ok();
        }
        if path.is_empty() {
            Some(fragment)
        } else {
            fallible::try_format!("{fragment}/{path}").ok()
        }
    }

    fn locate_xpointer(&self, storage: &dyn Storage, xpointer: &str) -> Option<Locator> {
        let pointer = parse_xpointer(xpointer)?;
        let section = pointer.section;
        let (paths, ..) = self.chapter(storage, section).ok()?;
        // Exactly, where it points into text PaperPaw shows.
        if let Some((node, char)) = pointer.node() {
            let mut found: Option<(u32, u32)> = None;
            let _ = cached::lines(storage, &paths.xpointers, |line| {
                if let Some((offset, at, from)) = origin(line)
                    && at == node
                    && from <= char
                {
                    found = Some((offset, char - from));
                }
                true
            });
            if let Some((offset, chars)) = found {
                let mut text =
                    fallible::zeroed((chars as usize * 4).min(MAX_SPAN as usize)).ok()?;
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
        let _ = cached::lines(storage, &paths.positions, |line| {
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
        if !self.styles || self.built(storage).is_err() {
            return Vec::new();
        }
        // None where the heap has no room for the list.
        let Ok((_, fonts)) = package::fonts(storage, &self.cache) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        if out.try_reserve_exact(fonts.len()).is_err() {
            return Vec::new();
        }
        out.extend(fonts.into_iter().map(|f| EmbeddedFont {
            family: f.family,
            bold: f.bold,
            italic: f.italic,
        }));
        out
    }

    fn font_data(&self, storage: &dyn Storage, index: usize) -> Result<Vec<u8>, DocumentError> {
        self.built(storage)?;
        let (identifier, fonts) = package::fonts(storage, &self.cache)?;
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

    /// `FONT_COPY`: copies the face of the book to a file in bounded chunks.
    fn extended(&self, storage: &dyn Storage, key: &str, params: &[u8]) -> Option<Vec<u8>> {
        if key != FONT_COPY {
            return None;
        }
        let mut answer = Vec::new();
        answer.try_reserve_exact(5).ok()?;
        let (code, size) = match self.copy_font(storage, params) {
            Ok(Some(size)) => (0, size),
            Ok(None) => (1, 0),
            Err(DocumentError::OutOfMemory) => (2, 0),
            Err(_) => (3, 0),
        };
        answer.push(code);
        answer.extend_from_slice(&size.to_le_bytes());
        Some(answer)
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

    fn toc_len(&self, storage: &dyn Storage) -> u32 {
        match self.built(storage) {
            Ok(()) => self.toc_len.get(),
            Err(_) => 0,
        }
    }

    /// Points at the start of the entry's chapter; finding an anchor needs
    /// the chapter converted, which [`Document::toc_target`] does.
    fn toc_entry(&self, storage: &dyn Storage, index: u32) -> Option<TocEntry> {
        self.built(storage).ok()?;
        let entry = package::toc_entry(storage, &self.cache, index)?;
        Some(TocEntry {
            title: entry.title,
            level: entry.level,
            at: Locator::new(entry.section, 0),
        })
    }

    fn toc_target(&self, storage: &dyn Storage, index: u32) -> Option<Locator> {
        self.built(storage).ok()?;
        let entry = package::toc_entry(storage, &self.cache, index)?;
        let offset = entry
            .fragment
            .as_deref()
            .and_then(|id| self.anchor(storage, entry.section, id))
            .unwrap_or(0);
        Some(Locator::new(entry.section, offset))
    }
}
