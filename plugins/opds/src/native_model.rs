//! OPDS 1 catalogs for the native plugin, without host calls: Atom feeds of
//! books and of further feeds, read as they stream in pieces, so a feed of any
//! size needs little memory. It is the interface-6 `feed` reader made
//! fallible (a refused allocation ends the reading with what was read, flagged
//! as `memory`, never an abort), tested to read the same feeds the same way.
//!
//! What is kept of a feed: its title, its entries (at most [`MAX_ENTRIES`])
//! with titles, authors and links, the next page and where to search.
//! Everything kept has a limit, so a feed made to fill the memory cannot.
//!
//! The XML tokenizer decodes entities and attributes into owned text where
//! one occurs (`Cow`): that allocation has no refusal path and is bounded by
//! the tokenizer's own limits on a token (`PORTING-7.md`).
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    document::is_external,
    native::Error,
    xml::{Token, Tokenizer, decode, local_name},
};
use paperpaw_ui_kit::{
    mem::push,
    text::{concat, copy},
};

/// Entries kept of one page; catalogs page longer lists with *next*.
pub const MAX_ENTRIES: usize = 100;
/// Titles and notes are cut at this many characters.
pub const MAX_TEXT: usize = 300;
/// Files kept of one entry: catalogs offer a book in a few formats.
pub const MAX_BOOKS: usize = 16;
/// Authors kept of one entry.
pub const MAX_AUTHORS: usize = 10;
/// Feeds are read in pieces of this size.
pub const CHUNK: usize = 2048;

/// A feed's kept parts.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Feed {
    pub title: String,
    pub entries: Vec<Entry>,
    /// The next page of the list.
    pub next: Option<String>,
    pub search: Option<Search>,
    /// The heap had no room to read all of it: what is here is the part
    /// before.
    pub memory: bool,
}

/// Where a catalog is searched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Search {
    /// An OpenSearch description, which names the address to use.
    Description(String),
    /// An address with `{searchTerms}` in it.
    Template(String),
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Entry {
    pub id: String,
    pub title: String,
    /// Its authors, separated by ` & ` (names may hold commas: `Austen, Jane`).
    pub authors: String,
    /// A short plain text about it, if it has one.
    pub summary: String,
    /// A feed it leads to.
    pub feed: Option<String>,
    /// A small picture of it (a cover), if the catalog has one.
    pub thumbnail: Option<String>,
    pub books: Vec<Book>,
}

/// A file of a book to download.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Book {
    /// Its content type, e.g. `application/epub+zip`.
    pub mime: String,
    pub url: String,
    /// How the catalog names this file, e.g. `EPUB (no images)`.
    pub title: String,
}

/// Where a feed's bytes come from: a file the network host saved.
pub trait Source {
    /// Reads at `offset` into `buf`; 0 at the end.
    ///
    /// # Errors
    /// The host's refusal.
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<usize, Error>;
}

/// The extensions of the formats a file may have, by name.
const NAMED: [&str; 11] = [
    "epub", "txt", "pdf", "mobi", "azw3", "fb2", "cbz", "rtf", "odt", "html", "htm",
];

impl Book {
    /// The file extension for its content type, if it is a known one, else
    /// from the file's name in its address.
    #[must_use]
    pub fn extension(&self) -> Option<&'static str> {
        let mime = self.mime.split(';').next().unwrap_or("").trim();
        if let Some(ext) = by_type(mime) {
            return Some(ext);
        }
        let path = self.url.split(['?', '#']).next().unwrap_or("");
        let name = path.rsplit('/').next().unwrap_or("");
        NAMED.into_iter().find(|ext| {
            name.len() > ext.len()
                && name.is_char_boundary(name.len() - ext.len())
                && name.get(name.len() - ext.len() - 1..).is_some_and(|tail| {
                    tail.starts_with('.')
                        && tail.get(1..).is_some_and(|t| t.eq_ignore_ascii_case(ext))
                })
        })
    }
}

fn by_type(mime: &str) -> Option<&'static str> {
    Some(match mime {
        "application/epub+zip" => "epub",
        "text/plain" => "txt",
        "application/pdf" => "pdf",
        "application/x-mobipocket-ebook" => "mobi",
        "application/vnd.amazon.ebook" => "azw3",
        "application/x-fictionbook+xml" | "application/fb2+xml" | "text/fb2+xml" => "fb2",
        "application/fb2+zip" | "application/x-zip-compressed-fb2" => "fb2.zip",
        "application/x-cbz" | "application/vnd.comicbook+zip" => "cbz",
        "application/rtf" | "text/rtf" => "rtf",
        "application/vnd.oasis.opendocument.text" => "odt",
        _ => return None,
    })
}

impl Entry {
    /// What to show below its title: its authors, else its summary.
    #[must_use]
    pub fn note(&self) -> &str {
        if self.authors.is_empty() {
            &self.summary
        } else {
            &self.authors
        }
    }

    /// The first of its files whose format `readable` accepts (by its
    /// extension), with that extension.
    #[must_use]
    pub fn readable_book(&self, readable: impl Fn(&str) -> bool) -> Option<(&Book, &'static str)> {
        self.books
            .iter()
            .filter_map(|book| book.extension().map(|ext| (book, ext)))
            .find(|(_, ext)| readable(ext))
    }
}

/// The text being collected, and what it is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    FeedTitle,
    Title,
    Id,
    Author,
    Content,
}

/// The parser's state between tokens.
#[derive(Debug, Default)]
struct Reader {
    base: String,
    feed: Feed,
    entry: Option<Entry>,
    in_author: bool,
    field: Option<Field>,
    text: String,
    /// The plain content or summary of the entry.
    content: String,
    authors: Vec<String>,
    /// In an entry past [`MAX_ENTRIES`]: read over, nothing kept.
    skipping: bool,
    /// An allocation was refused: nothing more is kept.
    failed: bool,
}

/// `text` joined to `out` if the heap allows.
fn append(out: &mut String, text: &str) -> Result<(), Error> {
    out.try_reserve(text.len()).map_err(|_| Error::NoMemory)?;
    out.push_str(text);
    Ok(())
}

impl Reader {
    fn token(&mut self, token: Token<'_>) {
        if self.failed {
            return;
        }
        if self.handle(token).is_err() {
            self.failed = true;
            self.feed.memory = true;
        }
    }

    #[allow(
        clippy::too_many_lines,
        reason = "the interface-6 reader's shape, kept for parity"
    )]
    fn handle(&mut self, token: Token<'_>) -> Result<(), Error> {
        match token {
            Token::Start {
                name,
                attrs,
                self_closing,
            } => {
                let name = local_name(name);
                if self.skipping {
                    return Ok(());
                }
                match name {
                    "entry" if self.feed.entries.len() >= MAX_ENTRIES => {
                        self.skipping = !self_closing;
                        return Ok(());
                    }
                    "entry" => {
                        self.entry = Some(Entry::default());
                        self.authors.clear();
                        self.content.clear();
                    }
                    "author" => self.in_author = true,
                    "link" => self.link(
                        attrs.get("rel").as_deref(),
                        attrs.get("type").as_deref().unwrap_or(""),
                        attrs.get("href").as_deref().unwrap_or(""),
                        attrs.get("title").as_deref().unwrap_or(""),
                    )?,
                    _ => {}
                }
                if self_closing {
                    return Ok(());
                }
                let in_entry = self.entry.is_some();
                let field = match name {
                    "title" if in_entry => Some(Field::Title),
                    "title" if !self.in_author && self.feed.title.is_empty() => {
                        Some(Field::FeedTitle)
                    }
                    "id" if in_entry && !self.in_author => Some(Field::Id),
                    "name" if in_entry && self.in_author => Some(Field::Author),
                    "content" | "summary" if in_entry => {
                        let kind = attrs.get("type");
                        matches!(kind.as_deref(), None | Some("text")).then_some(Field::Content)
                    }
                    _ => None,
                };
                if field.is_some() {
                    self.field = field;
                    self.text.clear();
                }
            }
            Token::Text(text) => {
                if self.field.is_some() && self.text.chars().count() < MAX_TEXT {
                    append(&mut self.text, &decode(&text))?;
                }
            }
            Token::CData(text) => {
                if self.field.is_some() && self.text.chars().count() < MAX_TEXT {
                    append(&mut self.text, text)?;
                }
            }
            Token::End { name } if self.skipping => {
                self.skipping = local_name(name) != "entry";
            }
            Token::End { name } => self.end(local_name(name))?,
        }
        Ok(())
    }

    fn end(&mut self, name: &str) -> Result<(), Error> {
        let ends_field = match self.field {
            Some(Field::FeedTitle | Field::Title) => name == "title",
            Some(Field::Id) => name == "id",
            Some(Field::Author) => name == "name",
            Some(Field::Content) => name == "content" || name == "summary",
            None => false,
        };
        if ends_field {
            let text = clean(&self.text)?;
            match (self.field.take(), self.entry.as_mut()) {
                (Some(Field::FeedTitle), _) => self.feed.title = text,
                (Some(Field::Title), Some(entry)) => entry.title = text,
                (Some(Field::Id), Some(entry)) => entry.id = text,
                (Some(Field::Author), Some(_))
                    if !text.is_empty() && self.authors.len() < MAX_AUTHORS =>
                {
                    push(&mut self.authors, text)?;
                }
                (Some(Field::Content), Some(_)) if self.content.is_empty() => self.content = text,
                _ => {}
            }
        }
        match name {
            "author" => self.in_author = false,
            "entry" => {
                if let Some(mut entry) = self.entry.take() {
                    entry.authors = join_authors(&self.authors)?;
                    entry.summary = core::mem::take(&mut self.content);
                    let useful = entry.feed.is_some() || !entry.books.is_empty();
                    if useful && self.feed.entries.len() < MAX_ENTRIES {
                        push(&mut self.feed.entries, entry)?;
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn link(
        &mut self,
        rel: Option<&str>,
        kind: &str,
        href: &str,
        title: &str,
    ) -> Result<(), Error> {
        if href.is_empty() || href.starts_with("data:") {
            return Ok(());
        }
        let url = join_url(&self.base, href)?;
        let atom = kind.contains("atom+xml");
        let Some(entry) = self.entry.as_mut() else {
            match rel {
                Some("next") if atom || kind.is_empty() => self.feed.next = Some(url),
                Some("search") if kind.contains("opensearchdescription") => {
                    self.feed.search = Some(Search::Description(url));
                }
                Some("search") if href.contains("{searchTerms}") => {
                    // `{` and `}` survive joining as they are.
                    self.feed.search = Some(Search::Template(url));
                }
                _ => {}
            }
            return Ok(());
        };
        let rel = rel.unwrap_or("");
        if rel == "http://opds-spec.org/image/thumbnail" || rel == "http://opds-spec.org/thumbnail"
        {
            entry.thumbnail = Some(url);
            return Ok(());
        }
        if rel == "http://opds-spec.org/image" || rel == "http://opds-spec.org/cover" {
            // The full picture only where there is no small one.
            entry.thumbnail.get_or_insert(url);
            return Ok(());
        }
        if rel == "http://opds-spec.org/acquisition"
            || rel == "http://opds-spec.org/acquisition/open-access"
        {
            if entry.books.len() >= MAX_BOOKS {
                return Ok(());
            }
            let book = Book {
                mime: copy(kind)?,
                url,
                title: clean(title)?,
            };
            push(&mut entry.books, book)?;
        } else if atom
            && entry.feed.is_none()
            && (matches!(rel, "" | "subsection" | "alternate" | "start")
                || rel.starts_with("http://opds-spec.org/sort/")
                || rel.starts_with("http://opds-spec.org/featured")
                || rel.starts_with("http://opds-spec.org/recommended"))
        {
            entry.feed = Some(url);
        }
        Ok(())
    }
}

/// The authors joined with ` & `.
fn join_authors(authors: &[String]) -> Result<String, Error> {
    let mut out = String::new();
    for (at, author) in authors.iter().enumerate() {
        if at > 0 {
            append(&mut out, " & ")?;
        }
        append(&mut out, author)?;
    }
    Ok(out)
}

/// Whitespace runs as single spaces, at most [`MAX_TEXT`] characters.
///
/// # Errors
/// `NoMemory` when the text cannot be kept.
pub fn clean(text: &str) -> Result<String, Error> {
    let mut out = String::new();
    for word in text.split_whitespace() {
        if !out.is_empty() {
            append(&mut out, " ")?;
        }
        append(&mut out, word)?;
    }
    if let Some((cut, _)) = out.char_indices().nth(MAX_TEXT) {
        out.truncate(cut);
        append(&mut out, "…")?;
    }
    Ok(out)
}

/// `href` made absolute with `base`, a page's address: as browsers resolve
/// links (`/a`, `../b`, `?q`, `//host/c`).
///
/// # Errors
/// `NoMemory` when the address cannot be kept.
pub fn join_url(base: &str, href: &str) -> Result<String, Error> {
    if is_external(href) {
        return copy(href);
    }
    let Some((scheme, rest)) = base.split_once("://") else {
        return copy(href);
    };
    if let Some(net) = href.strip_prefix("//") {
        return concat(&[scheme, "://", net]);
    }
    let host_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let host = rest.get(..host_end).unwrap_or_default();
    let after = rest.get(host_end..).unwrap_or_default();
    let path = after.split(['?', '#']).next().unwrap_or("");
    let path = if path.is_empty() { "/" } else { path };
    let joined = if href.starts_with('/') {
        copy(href)?
    } else if href.starts_with('?') {
        concat(&[path, href])?
    } else if href.starts_with('#') {
        let without = after.split('#').next().unwrap_or("");
        concat(&[if without.is_empty() { "/" } else { without }, href])?
    } else {
        let dir = path
            .get(..path.rfind('/').map_or(0, |i| i + 1))
            .unwrap_or_default();
        concat(&[dir, href])?
    };
    let target = normalize(&joined)?;
    concat(&[scheme, "://", host, &target])
}

/// Resolves `.` and `..` in the path of `target`, keeping its query and
/// fragment.
fn normalize(target: &str) -> Result<String, Error> {
    let split = target.find(['?', '#']).unwrap_or(target.len());
    let (path, tail) = target.split_at(split);
    let mut parts: Vec<&str> = Vec::new();
    let mut segments = path.split('/').peekable();
    while let Some(segment) = segments.next() {
        let last = segments.peek().is_none();
        match segment {
            "." => {
                if last {
                    push(&mut parts, "")?;
                }
            }
            ".." => {
                if parts.len() > 1 {
                    parts.pop();
                }
                if last {
                    push(&mut parts, "")?;
                }
            }
            segment => push(&mut parts, segment)?,
        }
    }
    let mut out = String::new();
    let mut joined = String::new();
    for (at, part) in parts.iter().enumerate() {
        if at > 0 {
            append(&mut joined, "/")?;
        }
        append(&mut joined, part)?;
    }
    if !joined.starts_with('/') {
        append(&mut out, "/")?;
    }
    append(&mut out, &joined)?;
    append(&mut out, tail)?;
    Ok(out)
}

/// Reads the feed `source` holds, which came from `base` (links are resolved
/// against it). A refused allocation ends the reading with what was read, the
/// feed marked `memory`.
///
/// # Errors
/// `NoMemory` when not even the reader can be made.
pub fn read(source: &mut impl Source, base: &str) -> Result<Feed, Error> {
    let mut reader = Reader {
        base: copy(base)?,
        ..Reader::default()
    };
    let mut tokenizer = Tokenizer::new();
    let mut buf = [0u8; CHUNK];
    let mut offset = 0;
    while let Ok(n) = source.read_at(offset, &mut buf) {
        if n == 0 {
            break;
        }
        let Some(piece) = buf.get(..n) else { break };
        if tokenizer
            .try_feed(piece, &mut |token| reader.token(token))
            .is_err()
        {
            reader.feed.memory = true;
            return Ok(reader.feed);
        }
        offset += n as u64;
    }
    tokenizer.finish(&mut |token| reader.token(token));
    Ok(reader.feed)
}
