//! Converting a chapter as a whole: reading it, writing what is pending,
//! and taking in its pictures after it.

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::Storage;
use paperpaw_sdk::document::{BlockKind, DocumentError, TextStyle};
use paperpaw_sdk::image::Fit;
use paperpaw_sdk::xml::{Token, Tokenizer, local_name};

use super::decode::{decode_image, decode_range, image_size};
use super::elements::is_void;
use super::{
    BookText, CachePaths, Converter, FLUSH, FLUSH_SMALL, HEADER, Html, LIMIT, NodePaths, Source,
    error, image_path, tables,
};
use crate::css::{self, Rules};
use crate::fallible::{self, TryPush};

/// The most bytes written for a chapter of `size` bytes: what it says
/// grows a few times when its places are listed (`<n>.pos`, `<n>.xpt`),
/// never by far more, and never past 64 MiB.
pub(super) fn budget(size: u64) -> u64 {
    size.saturating_mul(16)
        .saturating_add(1 << 20)
        .min(64 << 20)
}

/// Converts a chapter; returns the length of its text file.
#[allow(
    clippy::too_many_arguments,
    reason = "what a chapter is converted with, each of it different; a struct would be built for one call"
)]
// On the S3 the HTML provider's `converted` took this inline, and its frame
// came to the 2,048 B the packer refuses (#263). Apart, the epub plugin's
// frame stays at 1,984 B. The X4's code is as it was.
#[cfg_attr(target_arch = "xtensa", inline(never))]
pub(crate) fn convert(
    storage: &dyn Storage,
    source: &Source<'_>,
    paths: &CachePaths,
    rules: Rules,
    styles: bool,
    color: bool,
    fit: Fit,
    book: BookText,
) -> Result<u32, DocumentError> {
    let mut converter = Converter::new(storage, source, paths, rules, styles, book)?;
    converter.read(source)?;
    converter.write_all();
    if let Some(e) = converter.failed {
        return Err(e);
    }
    // Rules of the chapter's own `<style>` were dropped. Kept so, the
    // chapter would look different for good.
    if converter.rules.out_of_memory {
        return Err(DocumentError::OutOfMemory);
    }
    let length = converter.length;
    drop(converter);
    let complete = take_pictures(storage, source, paths, color, fit)?;
    let _ = storage.remove(&paths.pictures);
    // Last: its presence marks a complete conversion. It is not written
    // where a picture found no room to be decoded. That picture is its
    // alternative text for now, and the next opening converts the chapter
    // again.
    if complete {
        paths.complete(storage, length)?;
    }
    Ok(length)
}

impl<'a> Converter<'a> {
    /// A converter for the chapter in `source`, with the buffers it needs.
    fn new(
        storage: &'a dyn Storage,
        source: &Source<'_>,
        paths: &'a CachePaths,
        rules: Rules,
        styles: bool,
        book: BookText,
    ) -> Result<Self, DocumentError> {
        let name = match source {
            Source::Zip { entry, .. } => entry.name.as_str(),
            Source::Html { path, .. } | Source::Fb2 { path, .. } => path,
        };
        let base = fallible::string(name.rfind('/').map_or("", |i| &name[..=i]))?;
        let html = match source {
            Source::Zip { .. } | Source::Fb2 { .. } => None,
            Source::Html {
                focus,
                base,
                pictures,
                ..
            } => Some(Html {
                focus: focus.map(fallible::string).transpose()?,
                base: base.map(fallible::string).transpose()?,
                pictures: *pictures,
            }),
        };
        let dialect: Option<crate::fb2::Fb2Dialect> = match source {
            Source::Fb2 { .. } => Some(crate::fb2::Fb2Dialect::default()),
            _ => None,
        };
        let size = match source {
            Source::Zip { entry, .. } => u64::from(entry.size),
            Source::Html { path, .. } | Source::Fb2 { path, .. } => storage.size(path)?,
        };
        paths.begin(storage)?;
        // The buffers that take most are asked for so that a heap too short
        // for them is an error, not a stop.
        let oom = |_| DocumentError::OutOfMemory;
        let mut out = Vec::new();
        out.try_reserve_exact(FLUSH + HEADER as usize)
            .map_err(oom)?;
        let mut text = String::new();
        text.try_reserve_exact(LIMIT + 4).map_err(oom)?;
        let mut index = Vec::new();
        index.try_reserve_exact(FLUSH_SMALL + 4).map_err(oom)?;
        Ok(Converter {
            storage,
            paths,
            length: 0,
            out,
            index,
            anchors: String::new(),
            links: String::new(),
            foreign: String::new(),
            block_foreign: false,
            chapter: fallible::string(name)?,
            positions: String::new(),
            xpointers: String::new(),
            node: None,
            origins: Vec::new(),
            origin_bytes: 0,
            since_origin: 0,
            origin: None,
            space_origin: None,
            node_paths: NodePaths::default(),
            path: String::new(),
            body: None,
            block_path: String::new(),
            // Blocks are split before they outgrow this, so it never grows.
            text,
            runs: Vec::new(),
            align: None,
            book,
            dir: None,
            lang: None,
            inline_langs: Vec::new(),
            drop_cap: (0, None),
            marker: None,
            table: None,
            row: None,
            quote: tables::Quoted::None,
            indent: None,
            size: None,
            margins: (0, 0),
            line_height: None,
            margin: None,
            padding: None,
            stack: Vec::new(),
            overflow: 0,
            rules,
            styles,
            css: css::Parser::new(&base)?,
            base,
            pictures: String::new(),
            seen: Vec::new(),
            written: core::cell::Cell::new(0),
            budget: budget(size),
            full: false,
            kind: BlockKind::Paragraph,
            continues: false,
            // A page with reading matter shows nothing outside it.
            hidden: u32::from(html.as_ref().is_some_and(|h| h.focus.is_some())),
            html,
            dialect,
            pre: 0,
            space: false,
            space_style: TextStyle::REGULAR,
            failed: None,
            math: None,
        })
    }

    /// Reads the chapter in `source` up to its end, or until the budget is
    /// spent or room is refused. Such a refusal is kept in `failed`.
    // Its own frame: the tokenizer and the handler are not in `convert`'s,
    // which stays under the firmware's frame limit (#263).
    #[inline(never)]
    fn read(&mut self, source: &Source<'_>) -> Result<(), DocumentError> {
        let storage = self.storage;
        let mut tokenizer = match source {
            Source::Zip { .. } | Source::Fb2 { .. } => Tokenizer::new(),
            Source::Html { .. } => Tokenizer::html(),
        };
        let full = core::cell::Cell::new(false);
        let mut handle = |token: Token<'_>| {
            // A refusal for room stops the reading, and ends it as such.
            let result = match token {
                Token::Start {
                    name,
                    attrs,
                    self_closing,
                } => self.start(name, attrs).and_then(|()| {
                    // Empty elements, also as HTML writes them (`<br>`).
                    if self_closing || is_void(local_name(name)) {
                        self.end(name)
                    } else {
                        Ok(())
                    }
                }),
                Token::End { name } => self.end(name),
                Token::Text(text) => self.characters(&text, false),
                Token::CData(text) => self.characters(text, true),
            };
            if let Err(error) = result {
                self.failed.get_or_insert(error);
                self.full = true;
            }
            full.set(self.full);
        };
        match source {
            // An archive entry is no longer inflated once the budget is spent.
            Source::Zip { archive, entry } => {
                let mut out_of_memory = false;
                archive
                    .stream(storage, entry, &mut |chunk| {
                        out_of_memory = tokenizer.try_feed(chunk, &mut handle).is_err();
                        !full.get() && !out_of_memory
                    })
                    .map_err(error)?;
                if out_of_memory {
                    return Err(DocumentError::OutOfMemory);
                }
            }
            Source::Html { path, .. } => {
                feed_html(storage, path, &full, &mut tokenizer, &mut handle)?;
            }
            Source::Fb2 { path, encoding } => {
                crate::fb2::feed(storage, path, *encoding, &full, &mut tokenizer, &mut handle)?;
            }
        }
        tokenizer.finish(&mut handle);
        Ok(())
    }

    /// Writes out the last block and everything still pending.
    fn write_all(&mut self) {
        if let Err(error) = self.flush() {
            self.failed.get_or_insert(error);
        }
        self.write_out();
        self.write_index();
        self.write_anchors();
        self.write_links();
        self.write_foreign();
        self.write_positions();
        self.write_xpointers();
        self.write_pictures();
    }
}

/// Feeds the HTML file at `path` to the tokenizer, a piece at a time,
/// until `full` says the budget is spent.
// Its own frame, as a FictionBook's (`fb2::feed`): the piece read is not
// in `convert`'s, which stays under the firmware's frame limit.
#[inline(never)]
fn feed_html(
    storage: &dyn Storage,
    path: &str,
    full: &core::cell::Cell<bool>,
    tokenizer: &mut Tokenizer,
    emit: &mut dyn FnMut(Token<'_>),
) -> Result<(), DocumentError> {
    let mut chunk = [0u8; 1024];
    let mut at = 0u64;
    while !full.get() {
        let n = storage.read_at(path, at, &mut chunk)?;
        if n == 0 {
            break;
        }
        tokenizer
            .try_feed(&chunk[..n], emit)
            .map_err(|_| DocumentError::OutOfMemory)?;
        at += n as u64;
    }
    Ok(())
}

/// Pictures noted or decoded together, so the list of noted ones (or a
/// FictionBook) is read once for each so many.
const PICTURE_BATCH: usize = 32;

/// Takes in the pictures listed in `<n>.pic`, after the text, so only one
/// archive entry is inflated at a time. An EPUB's are decoded when a page
/// shows them; their sizes now, from their first bytes, for the pages to
/// be laid out (a big picture takes seconds to decode on small devices).
/// Those whose size the first bytes do not say are decoded now, as are
/// the pictures beside a file made from Markdown and a FictionBook's. One
/// that cannot be decoded is shown as its alternative text. So is one
/// that found no room to be decoded, but then the answer is `false`: the
/// conversion is not complete, to be made again when there is room.
fn take_pictures(
    storage: &dyn Storage,
    source: &Source<'_>,
    paths: &CachePaths,
    color: bool,
    fit: Fit,
) -> Result<bool, DocumentError> {
    let short = core::cell::Cell::new(false);
    let decoded = |result: Result<(), DocumentError>| {
        if matches!(result, Err(DocumentError::OutOfMemory)) {
            short.set(true);
        }
    };
    let mut notes: Vec<crate::pictures::Note> = Vec::new();
    let mut wanted: Vec<(String, String, Fit)> = Vec::new();
    let mut refused = None;
    let decode_binaries = |wanted: &mut Vec<(String, String, Fit)>| {
        let result = match source {
            Source::Fb2 { path, encoding } => {
                crate::fb2::decode_binaries(storage, path, *encoding, wanted, &paths.dir, color)
            }
            _ => Ok(()),
        };
        wanted.clear();
        decoded(result);
    };
    let mut each = |path: &str| -> Result<(), DocumentError> {
        let base = image_path(&paths.dir, path)?;
        if path.is_empty() || crate::pictures::has(storage, &base, color)? {
            return Ok(());
        }
        match source {
            Source::Zip { archive, .. } => match image_size(storage, archive, path) {
                Some((width, height)) if width > 0 && height > 0 => {
                    notes.try_push(crate::pictures::Note {
                        base,
                        size: fit.size(width, height),
                        source: fallible::string(path)?,
                    })?;
                    if notes.len() >= PICTURE_BATCH {
                        crate::pictures::try_note_all(storage, &mut notes)?;
                    }
                }
                _ => {
                    let file = crate::pictures::try_file(&base, color)?;
                    decoded(decode_image(
                        storage, archive, path, &file, &paths.dir, fit, color,
                    ));
                }
            },
            Source::Html { pictures, .. } => {
                if let (true, Ok(size)) = (*pictures, storage.size(path)) {
                    let file = crate::pictures::try_file(&base, color)?;
                    decoded(decode_range(storage, path, 0, size, &file, fit, color));
                }
            }
            Source::Fb2 { .. } => {
                wanted.try_push((
                    fallible::string(path)?,
                    crate::pictures::try_file(&base, color)?,
                    fit,
                ))?;
                if wanted.len() >= PICTURE_BATCH {
                    decode_binaries(&mut wanted);
                }
            }
        }
        Ok(())
    };
    let read = crate::cached::lines(storage, &paths.pictures, |path| match each(path) {
        Ok(()) => true,
        Err(error) => {
            refused = Some(error);
            false
        }
    });
    if let Some(error) = refused {
        return Err(error);
    }
    read?;
    crate::pictures::try_note_all(storage, &mut notes)?;
    if !wanted.is_empty() {
        decode_binaries(&mut wanted);
    }
    Ok(!short.get())
}
