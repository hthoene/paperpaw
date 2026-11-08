//! HTML files (web pages, saved or downloaded) as documents of one
//! section, converted like EPUB chapters (see [`crate::convert`]) and read
//! as browsers read HTML: only the reading matter, links made absolute
//! with the page's address (from its `<base href>`).

use alloc::string::String;
use alloc::vec::Vec;
use core::cell::Cell;

use paperpaw_sdk::Storage;
use paperpaw_sdk::document::{Block, Document, DocumentError, Link, Locator, Metadata};
use paperpaw_sdk::xml::{Token, Tokenizer, local_name, try_decode};

use crate::PLUGIN;
use crate::cached;
use crate::convert::{self, CachePaths, INDEX, Source};
use crate::fallible::{self, TryPush, TryPushStr};

/// Changes whenever pages are read differently (e.g. what counts as
/// page furniture), so converted pages are converted again.
const HTML_VERSION: u32 = 2;

/// Marks a page whose converted length is not known yet.
const UNKNOWN: u32 = u32::MAX;
/// Marks a page that could not be converted since it was opened (see
/// `document::FAILED`).
const FAILED: u32 = u32::MAX - 1;
/// Longest title kept.
const TITLE: usize = 300;

/// An open HTML file.
#[derive(Debug)]
pub struct HtmlDocument {
    path: String,
    paths: CachePaths,
    /// The block found last (see [`cached::block_at`]).
    last: core::cell::Cell<Option<u32>>,
    /// Where an anchor was found last.
    anchors: cached::AnchorHint,
    metadata: Metadata,
    /// The element holding the reading matter, if the page marks it.
    focus: Option<&'static str>,
    /// The page's address.
    base: Option<String>,
    styles: bool,
    /// Whether pictures beside the file are shown (made from Markdown).
    pictures: bool,
    /// Size of the file: the length until it is converted.
    size: u32,
    length: Cell<u32>,
    /// Its text length and number of blocks once converted: the cache
    /// is not looked at again while it is open.
    known: Cell<Option<(u32, u32)>>,
}

/// What a first look at a page finds.
#[derive(Debug, Default)]
struct Scan {
    title: Option<String>,
    language: Option<String>,
    focus: Option<&'static str>,
    base: Option<String>,
}

/// What [`scan`] holds while it reads.
#[derive(Default)]
struct Scanning {
    found: Scan,
    main: bool,
    article: bool,
    title: Option<String>,
}

impl Scanning {
    /// One token of the page; a copy that finds no room is `OutOfMemory`.
    fn token(&mut self, token: Token<'_>) -> Result<(), DocumentError> {
        match token {
            Token::Start { name, attrs, .. } => {
                let local = local_name(name);
                let is = |tag: &str| local.eq_ignore_ascii_case(tag);
                if is("html") {
                    let language = attrs.get("lang");
                    self.found.language = match language
                        .as_deref()
                        .map(str::trim)
                        .filter(|l| l.len() <= MAX_LANGUAGE)
                    {
                        Some(language) => Some(fallible::string(language)?),
                        None => None,
                    };
                } else if is("title") && self.found.title.is_none() {
                    self.title = Some(String::new());
                } else if is("main") {
                    self.main = true;
                } else if is("article") {
                    self.article = true;
                } else if is("base") && self.found.base.is_none() {
                    self.found.base = match attrs.get("href").filter(|h| h.len() <= MAX_BASE) {
                        Some(href) => Some(fallible::string(&href)?),
                        None => None,
                    };
                }
            }
            Token::End { name } if local_name(name).eq_ignore_ascii_case("title") => {
                if let Some(text) = self.title.take() {
                    let text = fallible::words(&text)?;
                    self.found.title = (!text.is_empty()).then_some(text);
                }
            }
            Token::Text(text) => {
                if let Some(title) = self.title.as_mut()
                    && title.len() < TITLE
                {
                    let text = try_decode(&text).map_err(|_| fallible::OOM)?;
                    title.try_push_str(&text)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
}

/// Reads the title, language, address and reading matter of a page.
fn scan(storage: &dyn Storage, path: &str) -> Result<Scan, DocumentError> {
    let mut scanning = Scanning::default();
    let mut refused = None;
    let mut handle = |token: Token<'_>| {
        if refused.is_none()
            && let Err(error) = scanning.token(token)
        {
            refused = Some(error);
        }
    };
    let mut tokenizer = Tokenizer::html();
    let mut chunk = [0u8; 1024];
    let mut at = 0u64;
    loop {
        let n = storage.read_at(path, at, &mut chunk)?;
        if n == 0 {
            break;
        }
        tokenizer
            .try_feed(&chunk[..n], &mut handle)
            .map_err(|_| DocumentError::OutOfMemory)?;
        at += n as u64;
    }
    tokenizer.finish(&mut handle);
    if let Some(error) = refused {
        return Err(error);
    }
    let Scanning {
        mut found,
        main,
        article,
        ..
    } = scanning;
    found.focus = if main {
        Some("main")
    } else if article {
        Some("article")
    } else {
        None
    };
    found.language = found.language.filter(|l| !l.is_empty());
    Ok(found)
}

/// Longest address and language kept (no address read is longer).
const MAX_BASE: usize = 2000;
const MAX_LANGUAGE: usize = 64;

/// What [`scan`] found, as kept in the cache: `key<TAB>value` lines.
fn save_scan(found: &Scan) -> Result<String, DocumentError> {
    let mut out = String::new();
    for (key, value) in [
        ("title", found.title.as_deref()),
        ("language", found.language.as_deref()),
        ("focus", found.focus),
        ("base", found.base.as_deref()),
    ] {
        if let Some(value) = value {
            out.try_push_str(key)?;
            out.try_push('\t')?;
            for c in value.chars() {
                out.try_push(if matches!(c, '\t' | '\n' | '\r') {
                    ' '
                } else {
                    c
                })?;
            }
            out.try_push('\n')?;
        }
    }
    Ok(out)
}

/// What [`scan`] found, from the cache.
fn load_scan(storage: &dyn Storage, path: &str) -> Option<Scan> {
    // On the heap, not the stack: a book is opened deep in the call stack.
    const CACHE_MAX: usize = 4 * 1024;
    let mut data = fallible::zeroed(CACHE_MAX).ok()?;
    let mut filled = 0;
    while filled < data.len() {
        match storage
            .read_at(path, filled as u64, &mut data[filled..])
            .ok()?
        {
            0 => break,
            n => filled += n,
        }
    }
    if filled == data.len() {
        // Larger than the cache ever holds.
        return None;
    }
    let text = core::str::from_utf8(&data[..filled]).ok()?;
    let mut found = Scan::default();
    for line in text.lines() {
        let (key, value) = line.split_once('\t')?;
        match key {
            "title" => found.title = Some(fallible::string(value).ok()?),
            "language" => found.language = Some(fallible::string(value).ok()?),
            "focus" if value == "main" => found.focus = Some("main"),
            "focus" if value == "article" => found.focus = Some("article"),
            "base" => found.base = Some(fallible::string(value).ok()?),
            _ => {}
        }
    }
    Some(found)
}

impl HtmlDocument {
    /// Opens an HTML file; without `styles` its CSS is ignored.
    pub fn open(storage: &dyn Storage, path: &str, styles: bool) -> Result<Self, DocumentError> {
        let cache = fallible::kept_path(&fallible::plugin_dir(PLUGIN.as_str())?, "html-", path)?;
        Self::open_as(storage, path, styles, false, cache)
    }

    /// Opens an HTML file made from Markdown: as [`HtmlDocument::open`],
    /// the pictures it names on the storage shown, converted into `cache`
    /// (the Markdown file's, so it goes with it).
    pub fn open_with_pictures(
        storage: &dyn Storage,
        path: &str,
        styles: bool,
        cache: String,
    ) -> Result<Self, DocumentError> {
        Self::open_as(storage, path, styles, true, cache)
    }

    fn open_as(
        storage: &dyn Storage,
        path: &str,
        styles: bool,
        pictures: bool,
        mut cache: String,
    ) -> Result<Self, DocumentError> {
        let size = storage.size(path)?;
        if !styles {
            cache.try_push_str("/plain")?;
        }
        let paths = CachePaths::new(&cache, "0")?;
        // Read and converted again when the file or the cache format
        // changed, and then nothing made of it before stays; what a look
        // at the page finds is kept meanwhile.
        let stamp = fallible::try_format!(
            "{}.{HTML_VERSION} {}\n",
            crate::package::CACHE_VERSION,
            fallible::fingerprint(storage, path)?
        )?;
        let stamp_path = fallible::try_format!("{cache}/source")?;
        let page_path = fallible::try_format!("{cache}/page.txt")?;
        let fresh = fallible::file_is(storage, &stamp_path, stamp.as_bytes());
        let found = match fresh.then(|| load_scan(storage, &page_path)).flatten() {
            Some(found) => found,
            None => {
                let _ = storage.remove_dir_all(&cache);
                let found = scan(storage, path)?;
                storage.write(&page_path, save_scan(&found)?.as_bytes())?;
                storage.write(&stamp_path, stamp.as_bytes())?;
                found
            }
        };
        let name = path.rsplit('/').next().unwrap_or(path);
        let name = name.rsplit_once('.').map_or(name, |(stem, _)| stem);
        let title = match found.title {
            Some(title) => title,
            None => fallible::string(name)?,
        };
        let mut authors = Vec::new();
        authors.try_reserve(0).map_err(|_| fallible::OOM)?;
        let metadata = Metadata::default()
            .title(Some(title))
            .authors(authors)
            .language(found.language);
        Ok(Self {
            path: fallible::string(path)?,
            paths,
            last: core::cell::Cell::new(None),
            anchors: cached::AnchorHint::default(),
            metadata,
            focus: found.focus,
            base: found.base,
            styles,
            pictures,
            size: u32::try_from(size).unwrap_or(u32::MAX - 1),
            length: Cell::new(UNKNOWN),
            known: Cell::new(None),
        })
    }

    /// Converts the page if needed; returns its text length and number of
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
                let source = Source::Html {
                    path: &self.path,
                    focus: self.focus,
                    base: self.base.as_deref(),
                    pictures: self.pictures,
                };
                // Web pages' pictures are not fetched: no colour to ask for.
                let converted = convert::convert(
                    storage,
                    &source,
                    &self.paths,
                    Default::default(),
                    self.styles,
                    false,
                    convert::IMAGE_FIT,
                    convert::BookText::default(),
                );
                // Short of memory, it is tried again: with more free, it may
                // convert.
                converted.inspect_err(|e| {
                    if !matches!(e, DocumentError::OutOfMemory) {
                        self.length.set(FAILED);
                    }
                })?
            }
        };
        self.length.set(length);
        let blocks = (storage.size(&self.paths.index)? / u64::from(INDEX)) as u32;
        self.known.set(Some((length, blocks)));
        Ok((length, blocks))
    }
}

impl Document for HtmlDocument {
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
            false,
            &mut last,
        );
        self.last.set(last);
        block
    }

    fn links(&self, storage: &dyn Storage, section: u32, from: u32, to: u32) -> Vec<Link> {
        if section != 0 || self.converted(storage).is_err() {
            return Vec::new();
        }
        cached::links(storage, &self.paths, from, to)
    }

    fn link_target(&self, storage: &dyn Storage, href: &str) -> Option<Locator> {
        let id = href.strip_prefix('#')?;
        self.converted(storage).ok()?;
        cached::anchor(storage, &self.paths, 0, id, &self.anchors)
            .map(|offset| Locator::new(0, offset))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_sdk::MemoryStorage;
    use paperpaw_sdk::document::BlockKind;

    const PAGE: &str = r##"<!DOCTYPE html>
<html lang="de"><head><meta charset="utf-8"><title>Ein   Artikel</title>
<base href="https://example.org/news/item.html">
<script>if (a<b) { document.write("</div>"); }</script>
<style>.hidden { display: none }</style></head>
<body><header><nav><a href="/">Start</a> <a href="/news">News</a></nav></header>
<aside>Werbung</aside>
<main><article><header><h1>Überschrift</h1></header>
<p>Erster Absatz mit <a href="../more.html">Link</a> und <a href="#n1">Fußnote</a>
<p>Zweiter Absatz<br>neue Zeile
<ul><li>Eins<li>Zwei</ul>
<p class="hidden">Versteckt</p>
<form><input name="q"><button>Suchen</button></form>
<p id="n1">Die Fußnote.</p></article></main>
<footer>Impressum</footer></body></html>"##;

    #[test]
    fn web_pages_show_their_reading_matter() {
        let storage = MemoryStorage::new().with("web/page.html", PAGE.as_bytes().to_vec());
        let doc = HtmlDocument::open(&storage, "web/page.html", true).unwrap();
        assert_eq!(doc.metadata().title.as_deref(), Some("Ein Artikel"));
        assert_eq!(doc.metadata().language.as_deref(), Some("de"));
        let mut texts = Vec::new();
        let mut at = Locator::START;
        while let Some(block) = doc.block_at(&storage, at).unwrap() {
            texts.push((block.kind, block.text.clone()));
            at = Locator::new(0, block.end);
        }
        let only: Vec<&str> = texts.iter().map(|(_, t)| t.as_str()).collect();
        assert_eq!(
            only,
            [
                "Überschrift",
                "Erster Absatz mit Link und Fußnote",
                "Zweiter Absatz\nneue Zeile",
                "•\u{a0}Eins",
                "•\u{a0}Zwei",
                "Die Fußnote."
            ]
        );
        assert_eq!(texts[0].0, BlockKind::Heading(1));
        let links = doc.links(&storage, 0, 0, u32::MAX);
        let hrefs: Vec<&str> = links.iter().map(|l| l.href.as_str()).collect();
        assert_eq!(hrefs, ["https://example.org/more.html", "#n1"]);
        assert_eq!(doc.link_target(&storage, &links[0].href), None);
        let note = doc.link_target(&storage, "#n1").unwrap();
        let block = doc.block_at(&storage, note).unwrap().unwrap();
        assert_eq!(block.text, "Die Fußnote.");
        // Opened again: from the cache.
        let doc = HtmlDocument::open(&storage, "web/page.html", true).unwrap();
        assert_eq!(doc.links(&storage, 0, 0, u32::MAX).len(), 2);
    }

    #[test]
    fn small_caps_reach_the_runs() {
        let page = "<html><head><style>.sc { font-variant: small-caps }</style></head><body>\
            <p>At <span class=\"sc\">noon</span> and <b style=\"font-variant-caps: \
            all-small-caps\">later</b>.</p></body></html>";
        let storage = MemoryStorage::new().with("p.html", page.as_bytes().to_vec());
        let doc = HtmlDocument::open(&storage, "p.html", true).unwrap();
        let block = doc.block_at(&storage, Locator::START).unwrap().unwrap();
        assert_eq!(block.text, "At noon and later.");
        let caps: Vec<(&str, bool)> = block
            .runs
            .iter()
            .filter(|r| r.style.small_caps())
            .map(|r| {
                (
                    &block.text[r.start as usize..r.end as usize],
                    r.style.bold(),
                )
            })
            .collect();
        assert_eq!(caps, [("noon", false), ("later", true)]);
    }

    #[test]
    fn pages_without_marked_reading_matter_lose_their_furniture() {
        let page = "<html><body><header>Seite</header><nav>Menü</nav>\
            <div><p>Text</p></div><footer>Fuß</footer></body></html>";
        let storage = MemoryStorage::new().with("p.htm", page.as_bytes().to_vec());
        let doc = HtmlDocument::open(&storage, "p.htm", true).unwrap();
        assert_eq!(doc.metadata().title.as_deref(), Some("p"));
        let block = doc.block_at(&storage, Locator::START).unwrap().unwrap();
        assert_eq!(block.text, "Text");
        assert!(
            doc.block_at(&storage, Locator::new(0, block.end))
                .unwrap()
                .is_none()
        );
    }
}
