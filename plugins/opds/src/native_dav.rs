//! WebDAV folders (Nextcloud, ownCloud and the like) as catalogs: the listing
//! a `PROPFIND` of depth 1 answers, read as it streams into the same [`Feed`]
//! as OPDS: subfolders lead further, files are books named by their file
//! names. Folders first, then by name. The interface-6 reader made fallible:
//! a refused allocation ends the reading with what was read, the feed marked
//! `memory`; an address, a name or a type too long to keep is left out.
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::{
    native::Error,
    web::try_percent_decode,
    xml::{Token, Tokenizer, local_name, try_decode},
};
use paperpaw_ui_kit::{
    mem::{buffer, push},
    text::{cmp_lowercase, copy},
};

use crate::native_model::{
    Book, CHUNK, Entry, Feed, Limits, MAX_MIME, MAX_URL, Source, clean, join_url,
};

/// What a WebDAV folder is asked for: names, kinds and types.
pub const PROPFIND: &str = "<?xml version=\"1.0\"?><d:propfind xmlns:d=\"DAV:\"><d:prop><d:displayname/><d:resourcetype/><d:getcontenttype/></d:prop></d:propfind>";

/// An address in a listing is read up to this many bytes; a longer one leads
/// nowhere.
const MAX_LINK: usize = 4096;

/// One `<response>` of a WebDAV listing.
#[derive(Debug, Default)]
struct Response {
    href: String,
    name: String,
    kind: String,
    folder: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Href,
    Name,
    Kind,
}

#[derive(Debug)]
struct Reader {
    limits: Limits,
    base: String,
    /// The folder's own path, decoded, without a `/` at the end.
    own: String,
    feed: Feed,
    current: Option<Response>,
    field: Option<Field>,
    text: String,
    /// The field's text did not fit: an address cut short leads nowhere.
    cut: bool,
    kept: usize,
    failed: bool,
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

    fn start(&mut self, name: &str, self_closing: bool) {
        match name {
            "response" => {
                self.current =
                    (self.feed.entries.len() < self.limits.entries).then(Response::default);
            }
            "collection" => {
                if let Some(r) = &mut self.current {
                    r.folder = true;
                }
            }
            "href" | "displayname" | "getcontenttype" if !self_closing => {
                self.field = Some(match name {
                    "href" => Field::Href,
                    "displayname" => Field::Name,
                    _ => Field::Kind,
                });
                self.text.clear();
                self.cut = false;
            }
            _ => {}
        }
    }

    fn add(&mut self, piece: &str) -> Result<(), Error> {
        if self.field.is_none() {
            return Ok(());
        }
        // A piece that would pass the cap is not taken, whole: the text is cut.
        if self.text.len().saturating_add(piece.len()) > MAX_LINK {
            self.cut = true;
            return Ok(());
        }
        self.text
            .try_reserve(piece.len())
            .map_err(|_| Error::NoMemory)?;
        self.text.push_str(piece);
        Ok(())
    }

    fn handle(&mut self, token: Token<'_>) -> Result<(), Error> {
        match token {
            Token::Start {
                name, self_closing, ..
            } => self.start(local_name(name), self_closing),
            Token::Text(text) => {
                let text = try_decode(&text).map_err(|_| Error::NoMemory)?;
                self.add(&text)?;
            }
            Token::CData(text) => self.add(text)?,
            Token::End { name } => self.end(local_name(name))?,
        }
        Ok(())
    }

    fn end(&mut self, name: &str) -> Result<(), Error> {
        let field = match name {
            "href" => Some(Field::Href),
            "displayname" => Some(Field::Name),
            "getcontenttype" => Some(Field::Kind),
            _ => None,
        };
        if field.is_some() && self.field == field {
            if let Some(r) = &mut self.current {
                let value = self.text.trim();
                match self.field {
                    Some(Field::Href) if self.cut => r.href.clear(),
                    Some(Field::Href) => r.href = copy(value)?,
                    Some(Field::Name) => r.name = clean(value)?,
                    _ if value.len() > MAX_MIME => r.kind.clear(),
                    _ => r.kind = copy(value)?,
                }
            }
            self.field = None;
        }
        if name == "response"
            && let Some(r) = self.current.take()
            && self.feed.entries.len() < self.limits.entries
        {
            self.response(r)?;
        }
        Ok(())
    }

    fn response(&mut self, r: Response) -> Result<(), Error> {
        let decoded = try_percent_decode(&r.href).map_err(|_| Error::NoMemory)?;
        let target = decoded.trim_end_matches('/');
        if target == self.own || r.href.is_empty() || r.href.len() > MAX_URL {
            return Ok(());
        }
        let url = join_url(&self.base, &r.href)?;
        if url.len() > MAX_URL {
            return Ok(());
        }
        let file = target.rsplit('/').next().unwrap_or("");
        let title = if r.name.is_empty() {
            clean(file)?
        } else {
            copy(&r.name)?
        };
        let mut entry = Entry {
            id: copy(&url)?,
            ..Entry::default()
        };
        if r.folder {
            entry.title = title;
            entry.feed = Some(url);
        } else {
            entry.title = copy(
                title
                    .rsplit_once('.')
                    .map_or(title.as_str(), |(stem, _)| stem),
            )?;
            push(
                &mut entry.books,
                Book {
                    mime: r.kind,
                    url,
                    title: String::new(),
                },
            )?;
        }
        let kept = self.kept.saturating_add(entry.weight());
        if kept > self.limits.page {
            self.failed = true;
            self.feed.memory = true;
            return Ok(());
        }
        push(&mut self.feed.entries, entry)?;
        self.kept = kept;
        Ok(())
    }
}

/// Reads the WebDAV listing (`PROPFIND`, depth 1) of the folder `base` that
/// `source` holds: folders first, then by name.
///
/// # Errors
/// `NoMemory` when not even the reader can be made; the source's error when
/// it cannot be read.
pub fn read(source: &mut impl Source, base: &str, limits: Limits) -> Result<Feed, Error> {
    let path = base.split("://").nth(1).map_or(base, |r| {
        r.get(r.find('/').unwrap_or(r.len())..).unwrap_or("")
    });
    let decoded = try_percent_decode(path).map_err(|_| Error::NoMemory)?;
    let own = copy(decoded.trim_end_matches('/'))?;
    let mut reader = Reader {
        limits,
        base: copy(base)?,
        feed: Feed {
            title: copy(own.rsplit('/').next().unwrap_or(""))?,
            ..Feed::default()
        },
        own,
        current: None,
        field: None,
        text: String::new(),
        cut: false,
        kept: 0,
        failed: false,
    };
    let mut tokenizer = Tokenizer::new();
    let mut buf = buffer(CHUNK)?;
    let mut offset = 0;
    loop {
        let n = source.read_at(offset, &mut buf)?;
        if n == 0 {
            break;
        }
        let piece = buf.get(..n).ok_or(Error::Invalid)?;
        if tokenizer
            .try_feed(piece, &mut |token| reader.token(token))
            .is_err()
        {
            reader.feed.memory = true;
            break;
        }
        offset += n as u64;
    }
    if !reader.feed.memory {
        tokenizer.finish(&mut |token| reader.token(token));
    }
    let mut entries: Vec<Entry> = core::mem::take(&mut reader.feed.entries);
    // The shared sorter: the std sort keeps a 32-entry scratch on the stack, a frame the X4 refuses.
    paperpaw_sdk::sort::sort_by(&mut entries, |a, b| {
        a.feed
            .is_none()
            .cmp(&b.feed.is_none())
            .then_with(|| cmp_lowercase(&a.title, &b.title))
            .then_with(|| a.id.cmp(&b.id))
    });
    reader.feed.entries = entries;
    Ok(reader.feed)
}
