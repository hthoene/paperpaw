//! OPDS 2 catalogs: JSON feeds (`application/opds+json`), read as they stream
//! from a file into the same [`Feed`] as OPDS 1, so a feed of any size needs
//! little memory. The interface-6 reader made fallible: the parser allocates
//! with `try_feed` and `try_finish`, what is kept is reserved first, and a
//! refusal ends the reading with what was read, the feed marked `memory`.
//!
//! What is read: the title (`metadata.title`); `navigation` (entries that lead
//! to further feeds); `publications` (books: title, authors, description, cover
//! from `images`, files from acquisition `links`); the same inside `groups`;
//! and the feed's own `links`: `next` and `search` (a URI template such as
//! `…/search{?query}`). A link over [`MAX_URL`] bytes or a file type over
//! [`MAX_MIME`] is not kept, as in OPDS 1; the entries and their text stay
//! within the same [`Limits`].
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::{
    json::stream::{Event, Parser},
    native::Error,
    xml::try_decode,
};
use paperpaw_ui_kit::{mem::push, text::copy};

use crate::native_model::{
    Book, CHUNK, Entry, Feed, Limits, MAX_AUTHORS, MAX_BOOKS, MAX_MIME, MAX_URL, Search, Source,
    clean, join_url,
};

/// Relations kept of one link.
const MAX_RELS: usize = 8;

/// A link being read.
#[derive(Debug, Default)]
struct Link {
    rel: Vec<String>,
    href: String,
    kind: String,
    title: String,
    templated: bool,
    /// The address or the type was too long to keep.
    long: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Publication,
    Navigation,
}

/// Builds the feed from the parser's events.
#[derive(Debug, Default)]
struct Builder {
    limits: Limits,
    base: String,
    feed: Feed,
    /// The entry being read, what it is, and how deep it starts.
    entry: Option<(Entry, Kind, usize)>,
    authors: Vec<String>,
    link: Option<Link>,
    /// Bytes of text held by the entries kept so far.
    kept: usize,
    /// The page is full or an allocation was refused: nothing more is kept.
    failed: bool,
}

/// `path` without the leading `groups/[]` (entries of groups count as the
/// feed's own).
fn ungrouped(path: &[String]) -> &[String] {
    match path {
        [first, second, rest @ ..] if first == "groups" && second == "[]" => rest,
        _ => path,
    }
}

fn is(path: &[String], parts: &[&str]) -> bool {
    path.len() == parts.len() && path.iter().zip(parts).all(|(a, b)| a == b)
}

fn starts(path: &[String]) -> Option<&str> {
    path.first().map(String::as_str)
}

impl Builder {
    fn event(&mut self, event: Event<'_>) {
        if self.failed {
            return;
        }
        let done = match event {
            Event::Open(path) => self.open(path),
            Event::Close(path) => self.close(path),
            Event::Value(path, value) => self.value(path, value),
        };
        if done.is_err() {
            self.failed = true;
            self.feed.memory = true;
        }
    }

    fn open(&mut self, path: &[String]) -> Result<(), Error> {
        if let Some((_, _, depth)) = &self.entry {
            if is(path.get(*depth..).unwrap_or_default(), &["links", "[]"]) {
                self.link = Some(Link::default());
            }
            return Ok(());
        }
        let local = ungrouped(path);
        let entry = is(local, &["publications", "[]"]) || is(local, &["navigation", "[]"]);
        // Past the entries kept, nothing of them is built.
        if entry && self.feed.entries.len() < self.limits.entries {
            let kind = if starts(local) == Some("publications") {
                Kind::Publication
            } else {
                Kind::Navigation
            };
            self.entry = Some((Entry::default(), kind, path.len()));
            self.authors.clear();
        } else if is(path, &["links", "[]"]) {
            self.link = Some(Link::default());
        }
        Ok(())
    }

    fn close(&mut self, path: &[String]) -> Result<(), Error> {
        let entry_depth = self.entry.as_ref().map(|(_, _, d)| *d);
        if let Some(depth) = entry_depth {
            if is(path.get(depth..).unwrap_or_default(), &["links", "[]"]) {
                if let (Some(link), Some((entry, kind, _))) = (self.link.take(), &mut self.entry) {
                    add_entry_link(&self.base, entry, *kind, link)?;
                }
                return Ok(());
            }
            if path.len() == depth
                && let Some((mut entry, _, _)) = self.entry.take()
            {
                entry.authors = join(&self.authors)?;
                if !entry.title.is_empty() && self.feed.entries.len() < self.limits.entries {
                    let kept = self.kept.saturating_add(entry.weight());
                    if kept > self.limits.page {
                        // The page is full: this and the rest are not kept.
                        self.failed = true;
                        self.feed.memory = true;
                        return Ok(());
                    }
                    push(&mut self.feed.entries, entry)?;
                    self.kept = kept;
                }
            }
            return Ok(());
        }
        if is(path, &["links", "[]"])
            && let Some(link) = self.link.take()
        {
            self.feed_link(link)?;
        }
        Ok(())
    }

    /// A link of the feed itself: the next page, the search.
    fn feed_link(&mut self, link: Link) -> Result<(), Error> {
        if link.long || link.href.is_empty() {
            return Ok(());
        }
        let next = link.rel.iter().any(|r| r == "next");
        let search = link.rel.iter().any(|r| r == "search");
        if !(next && self.feed.next.is_none() || search && self.feed.search.is_none()) {
            return Ok(());
        }
        let href = self.template(&link)?;
        if href.len() > MAX_URL {
            return Ok(());
        }
        if next && self.feed.next.is_none() {
            self.feed.next = Some(href);
        } else if search && self.feed.search.is_none() {
            self.feed.search = Some(if link.templated {
                Search::Template(href)
            } else {
                Search::Description(href)
            });
        }
        Ok(())
    }

    /// A link's address, with an OPDS 2 search template (`{?query}`) in the
    /// form searches use (`{searchTerms}`).
    fn template(&self, link: &Link) -> Result<String, Error> {
        let href = join_url(&self.base, &link.href)?;
        if !link.templated {
            return Ok(href);
        }
        let href = replace(&href, "{?query}", "?query={searchTerms}")?;
        let href = replace(&href, "{&query}", "&query={searchTerms}")?;
        replace(&href, "{query}", "{searchTerms}")
    }

    fn value(&mut self, path: &[String], value: &str) -> Result<(), Error> {
        if let Some((entry, kind, depth)) = &mut self.entry {
            let rel = path.get(*depth..).unwrap_or_default();
            if let Some(link) = &mut self.link
                && let [first, _, field, ..] = rel
                && first == "links"
            {
                return link_field(link, field, value, true);
            }
            let parts: Vec<&str> = {
                let mut parts = Vec::new();
                for part in rel {
                    push(&mut parts, part.as_str())?;
                }
                parts
            };
            match *kind {
                Kind::Navigation => match parts.as_slice() {
                    ["title"] => entry.title = clean(value)?,
                    ["href"] => {
                        if value.len() <= MAX_URL {
                            let url = join_url(&self.base, value)?;
                            if url.len() <= MAX_URL {
                                entry.feed = Some(url);
                            }
                        }
                    }
                    _ => {}
                },
                Kind::Publication => match parts.as_slice() {
                    ["metadata", "title"] => entry.title = clean(value)?,
                    ["metadata", "identifier"] => entry.id = clean(value)?,
                    ["metadata", "description"] => entry.summary = plain(value)?,
                    ["metadata", "author"]
                    | ["metadata", "author", "[]"]
                    | ["metadata", "author", "name"]
                    | ["metadata", "author", "[]", "name"]
                        if self.authors.len() < MAX_AUTHORS =>
                    {
                        let author = clean(value)?;
                        push(&mut self.authors, author)?;
                    }
                    ["images", "[]", "href"] if entry.thumbnail.is_none() => {
                        if value.len() <= MAX_URL {
                            let url = join_url(&self.base, value)?;
                            if url.len() <= MAX_URL {
                                entry.thumbnail = Some(url);
                            }
                        }
                    }
                    _ => {}
                },
            }
            return Ok(());
        }
        if let Some(link) = &mut self.link
            && let [first, _, field, ..] = path
            && first == "links"
        {
            return link_field(link, field, value, false);
        }
        if is(path, &["metadata", "title"]) {
            self.feed.title = clean(value)?;
        }
        Ok(())
    }
}

/// One field of a link being read.
fn link_field(link: &mut Link, field: &str, value: &str, in_entry: bool) -> Result<(), Error> {
    match field {
        "rel" if link.rel.len() < MAX_RELS => push(&mut link.rel, copy(value)?)?,
        "href" if value.len() > MAX_URL => link.long = true,
        "href" => link.href = copy(value)?,
        "type" if value.len() > MAX_MIME => link.long = true,
        "type" => link.kind = copy(value)?,
        "title" if in_entry => link.title = clean(value)?,
        "templated" if !in_entry => link.templated = value == "true",
        _ => {}
    }
    Ok(())
}

/// A link of an entry: a file of a book, or the feed it leads to.
fn add_entry_link(base: &str, entry: &mut Entry, kind: Kind, link: Link) -> Result<(), Error> {
    if link.long || link.href.is_empty() || link.href.starts_with("data:") {
        return Ok(());
    }
    let url = join_url(base, &link.href)?;
    if url.len() > MAX_URL {
        return Ok(());
    }
    let acquisition = link.rel.iter().any(|r| r.contains("acquisition"));
    let feed = link.kind.contains("opds") || link.kind.contains("atom");
    if kind == Kind::Publication && (acquisition || (!feed && link.rel.is_empty())) {
        if entry.books.len() >= MAX_BOOKS {
            return Ok(());
        }
        push(
            &mut entry.books,
            Book {
                mime: link.kind,
                url,
                title: link.title,
            },
        )?;
    } else if feed && entry.feed.is_none() {
        entry.feed = Some(url);
    }
    Ok(())
}

/// The authors joined with ` & `.
fn join(authors: &[String]) -> Result<String, Error> {
    let mut out = String::new();
    for (at, author) in authors.iter().enumerate() {
        let more = author.len() + if at > 0 { 3 } else { 0 };
        out.try_reserve(more).map_err(|_| Error::NoMemory)?;
        if at > 0 {
            out.push_str(" & ");
        }
        out.push_str(author);
    }
    Ok(out)
}

/// `text` with every `from` replaced by `to`.
fn replace(text: &str, from: &str, to: &str) -> Result<String, Error> {
    let mut out = String::new();
    out.try_reserve(text.len()).map_err(|_| Error::NoMemory)?;
    let mut rest = text;
    while let Some(at) = rest.find(from) {
        let (head, tail) = rest.split_at(at);
        out.try_reserve(head.len() + to.len())
            .map_err(|_| Error::NoMemory)?;
        out.push_str(head);
        out.push_str(to);
        rest = tail.get(from.len()..).unwrap_or_default();
    }
    out.try_reserve(rest.len()).map_err(|_| Error::NoMemory)?;
    out.push_str(rest);
    Ok(out)
}

/// A description as plain text: tags dropped, entities decoded, whitespace
/// collapsed.
fn plain(html: &str) -> Result<String, Error> {
    let mut text = String::new();
    let mut in_tag = false;
    for c in html.chars() {
        let kept = match c {
            '<' => {
                in_tag = true;
                None
            }
            '>' if in_tag => {
                in_tag = false;
                Some(' ')
            }
            c if !in_tag => Some(c),
            _ => None,
        };
        if let Some(c) = kept {
            text.try_reserve(c.len_utf8())
                .map_err(|_| Error::NoMemory)?;
            text.push(c);
        }
    }
    clean(&try_decode(&text).map_err(|_| Error::NoMemory)?)
}

/// Whether the file `source` reads is JSON (its first character `{`).
///
/// # Errors
/// The source's refusal.
pub fn is_json(source: &mut impl Source) -> Result<bool, Error> {
    let mut head = [0u8; 64];
    let n = source.read_at(0, &mut head)?;
    Ok(head
        .get(..n)
        .unwrap_or_default()
        .iter()
        .find(|b| !b.is_ascii_whitespace() && !matches!(**b, 0xef | 0xbb | 0xbf))
        == Some(&b'{'))
}

/// Reads the OPDS 2 feed `source` holds, which came from `base`. A refused
/// allocation ends the reading with what was read, the feed marked `memory`.
///
/// # Errors
/// `NoMemory` when not even the reader can be made; the source's error when
/// it cannot be read.
pub fn read(source: &mut impl Source, base: &str, limits: Limits) -> Result<Feed, Error> {
    let mut builder = Builder {
        limits,
        base: copy(base)?,
        ..Builder::default()
    };
    let mut parser = Parser::default();
    let mut buf = [0u8; CHUNK];
    let mut offset = 0;
    loop {
        let n = source.read_at(offset, &mut buf)?;
        if n == 0 {
            break;
        }
        let piece = buf.get(..n).ok_or(Error::Invalid)?;
        if parser
            .try_feed(piece, &mut |event| builder.event(event))
            .is_err()
        {
            builder.feed.memory = true;
            return Ok(builder.feed);
        }
        if builder.failed {
            return Ok(builder.feed);
        }
        offset += n as u64;
    }
    if parser
        .try_finish(&mut |event| builder.event(event))
        .is_err()
    {
        builder.feed.memory = true;
    }
    Ok(builder.feed)
}
