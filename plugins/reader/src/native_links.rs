//! The links of a page: read from the document service for the span the page
//! shows, matched to the page's words, chosen with the buttons or touched, and
//! followed (a place in the book, a note over the page, the web). The trail back
//! (a followed link is returned from with *Back*) and the text of a note are
//! here too. The interface-6 screen (`screen/linking.rs`, `footnote.rs`) is the
//! reference. What is host-free is tested without a host.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    NoStorage,
    document::{BlockKind, Document, Locator},
    native::{
        Error, Host,
        document::{DocCursor, DocId, DocPosition, DocSpan, Documents, LinkKind},
    },
    reading::WordBox,
};
use paperpaw_ui_kit::mem::buffer;

use crate::native_document::HostDocument;

/// Most links kept for one page.
pub const MAX_LINKS: usize = 64;
/// Most places a followed link is returned to, the oldest forgotten.
pub const RETURNS: usize = 32;

/// One link of the page: where its text is in the book and where it leads.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct PageLink {
    /// The section its text is in.
    pub section: u32,
    /// The source offsets of its text.
    pub start: u32,
    pub end: u32,
    /// What it leads to.
    pub kind: LinkKind,
    /// Where it leads (at most 1,024 bytes).
    pub href: String,
}

/// The document a page is of, as the links need it.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct DocRef<'a> {
    pub docs: &'a Documents,
    pub id: DocId,
    /// How many sections the book has.
    pub sections: u32,
    /// The book is whole pages (comics): nothing on them to follow.
    pub whole_pages: bool,
}

impl<'a> DocRef<'a> {
    /// The document `id` of `sections` sections.
    #[must_use]
    pub const fn new(docs: &'a Documents, id: DocId, sections: u32, whole_pages: bool) -> Self {
        Self {
            docs,
            id,
            sections,
            whole_pages,
        }
    }
}

/// The source range each section of the page's words covers: `(section, from,
/// to)`, in the order the sections come on the page.
fn spans(words: &[WordBox]) -> Result<Vec<(u32, u32, u32)>, Error> {
    let mut spans: Vec<(u32, u32, u32)> = Vec::new();
    for word in words {
        let section = word.start.section;
        match spans.iter_mut().find(|s| s.0 == section) {
            Some(span) => {
                span.1 = span.1.min(word.start.offset);
                span.2 = span.2.max(word.end.offset);
            }
            None => {
                spans.try_reserve(1).map_err(|_| Error::NoMemory)?;
                spans.push((section, word.start.offset, word.end.offset));
            }
        }
    }
    Ok(spans)
}

/// The links of the page whose words are `words`: those of each section it
/// shows that overlap the page's span, at most [`MAX_LINKS`]. A book with none,
/// or a document that cannot list them now (preparing), has none.
///
/// # Errors
/// `NoMemory` and the host's refusals other than "not now".
pub fn read_links(
    host: &Host<'_>,
    doc: &DocRef<'_>,
    words: &[WordBox],
) -> Result<Vec<PageLink>, Error> {
    let mut found: Vec<PageLink> = Vec::new();
    if doc.whole_pages || words.is_empty() {
        return Ok(found);
    }
    let (mut input, mut out) = (buffer(256)?, buffer(4096)?);
    for (section, from, to) in spans(words)? {
        let mut cursor = DocCursor::start();
        loop {
            let page = match doc.docs.links(
                host,
                doc.id,
                DocSpan::new(section, from..to),
                cursor,
                &mut input,
                &mut out,
            ) {
                Ok(page) => page,
                // Preparing, or a section past the end: no links, not an error.
                Err(Error::Busy | Error::NotFound) => break,
                // The list changed while it was read: begin again, once.
                Err(Error::State) if cursor != DocCursor::start() => {
                    cursor = DocCursor::start();
                    found.retain(|l| l.section != section);
                    continue;
                }
                Err(error) => return Err(error),
            };
            for link in page.rows() {
                if found.len() >= MAX_LINKS {
                    return Ok(found);
                }
                let mut href = String::new();
                href.try_reserve(link.href().len())
                    .map_err(|_| Error::NoMemory)?;
                href.push_str(link.href());
                found.try_reserve(1).map_err(|_| Error::NoMemory)?;
                found.push(PageLink {
                    section,
                    start: link.start(),
                    end: link.end(),
                    kind: link.kind(),
                    href,
                });
            }
            match page.end().next() {
                Some(next) => cursor = next,
                None => break,
            }
        }
    }
    Ok(found)
}

/// The link `word` is part of, if any: the first of the page's links its source
/// range overlaps.
#[must_use]
pub fn link_of<'a>(links: &'a [PageLink], word: &WordBox) -> Option<&'a PageLink> {
    links.iter().find(|l| {
        word.start.section == l.section && word.end.offset > l.start && word.start.offset < l.end
    })
}

/// The place in `links` of the one a word is in (as [`link_of`]).
#[must_use]
pub fn link_index_of(links: &[PageLink], word: &WordBox) -> Option<usize> {
    links.iter().position(|l| {
        word.start.section == l.section && word.end.offset > l.start && word.start.offset < l.end
    })
}

/// The text of `link` on the page: the parts of its words inside it (a note mark
/// is often part of a word, as in `source¹`).
///
/// # Errors
/// `NoMemory`.
pub fn label_of(words: &[WordBox], link: &PageLink) -> Result<String, Error> {
    let mut label = String::new();
    for word in words.iter().filter(|w| {
        w.start.section == link.section && w.end.offset > link.start && w.start.offset < link.end
    }) {
        let from = link.start.saturating_sub(word.start.offset) as usize;
        let to = link
            .end
            .min(word.end.offset)
            .saturating_sub(word.start.offset) as usize;
        let part = word.text.get(from..to).unwrap_or(&word.text);
        label
            .try_reserve(part.len() + 1)
            .map_err(|_| Error::NoMemory)?;
        if !label.is_empty() {
            label.push(' ');
        }
        label.push_str(part);
    }
    Ok(label)
}

/// Whether a link's text looks like a note mark: short, or a number in
/// brackets, or a symbol such as `*` or `†`.
#[must_use]
pub fn note_like(label: &str) -> bool {
    let t = label.trim();
    !t.is_empty()
        && (t.chars().count() <= 3
            || t.chars().count() <= 6
                && t.chars()
                    .all(|c| c.is_ascii_digit() || "[]()*†‡§¹²³⁴⁵⁶⁷⁸⁹⁰".contains(c)))
}

/// Whether `href` leads outside the book (an address with a scheme).
#[must_use]
pub fn is_external(href: &str) -> bool {
    paperpaw_sdk::document::is_external(href)
}

/// The place an internal link leads to, if the document knows it.
///
/// # Errors
/// `NoMemory` and the host's refusals other than "no such place".
pub fn target_of(
    host: &Host<'_>,
    doc: &DocRef<'_>,
    href: &str,
) -> Result<Option<DocPosition>, Error> {
    let (mut input, mut out) = (buffer(2048)?, buffer(4096)?);
    match doc
        .docs
        .link_target(host, doc.id, href, &mut input, &mut out)
    {
        Ok(position) => Ok(Some(position)),
        Err(Error::NotFound | Error::Busy | Error::Invalid) => Ok(None),
        Err(error) => Err(error),
    }
}

/// The text at the target of link `href`, as a note shows it: the block there and
/// the ones after it, up to a heading or a few hundred characters, on one line;
/// none where the link leads nowhere in the book or to no text.
///
/// # Errors
/// `NoMemory` and the host's refusals other than "no such place".
pub fn note_text(host: &Host<'_>, doc: &DocRef<'_>, href: &str) -> Result<Option<String>, Error> {
    let Some(target) = target_of(host, doc, href)? else {
        return Ok(None);
    };
    let book = HostDocument::new(host, doc.docs, doc.id, doc.sections, doc.whole_pages)?;
    let mut at = Locator::new(target.section, target.offset);
    let mut text = String::new();
    for _ in 0..4 {
        let Ok(Some(block)) = book.block_at(&NoStorage, at) else {
            break;
        };
        if matches!(block.kind, BlockKind::Heading(_)) && !text.is_empty() {
            break;
        }
        let part = block.text.trim();
        if !part.is_empty() {
            text.try_reserve(part.len() + 1)
                .map_err(|_| Error::NoMemory)?;
            if !text.is_empty() {
                text.push(' ');
            }
            text.push_str(part);
        }
        // A note that is only its number goes on in the next block.
        if text.chars().count() > 300 || text.chars().count() > 12 && block.end > block.start {
            break;
        }
        at = Locator::new(at.section, block.end);
    }
    let mut one = String::new();
    one.try_reserve(text.len()).map_err(|_| Error::NoMemory)?;
    for word in text.split_whitespace() {
        if !one.is_empty() {
            one.push(' ');
        }
        one.push_str(word);
    }
    Ok((!one.is_empty()).then_some(one))
}

/// The places links were followed from, to return to with *Back*.
#[derive(Debug, Default)]
pub struct Returns {
    places: Vec<Locator>,
    /// Where the last link landed: the way back is offered there only.
    landed: Option<Locator>,
}

impl Returns {
    /// Nothing followed yet.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            places: Vec::new(),
            landed: None,
        }
    }

    /// A link was followed from the page starting at `from`.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn followed(&mut self, from: Locator) -> Result<(), Error> {
        self.places.try_reserve(1).map_err(|_| Error::NoMemory)?;
        if self.places.len() >= RETURNS {
            self.places.remove(0);
        }
        self.places.push(from);
        Ok(())
    }

    /// The link landed on the page starting at `at`.
    pub fn landed_on(&mut self, at: Locator) {
        self.landed = Some(at);
    }

    /// The place to return to, forgetting it.
    pub fn back(&mut self) -> Option<Locator> {
        self.places.pop()
    }

    /// Whether there is a place to return to.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.places.is_empty()
    }

    /// Whether the way back is shown on the page starting at `page`.
    #[must_use]
    pub fn offered_on(&self, page: Locator) -> bool {
        !self.places.is_empty() && self.landed == Some(page)
    }
}

#[cfg(test)]
mod tests;
