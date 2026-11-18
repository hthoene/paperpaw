//! The listing a WebDAV `PROPFIND` of depth 1 answers, read as it streams from
//! the file it was saved to (nothing larger than a piece of it is in memory).
//! Every file and folder of it becomes an [`Item`] with its address, name, size
//! and a number that changes when the server's file does (a hash of its
//! `ETag`, else of its last-modified time); the folder itself is left out. A
//! refused allocation ends the reading with what was read (and says so), and
//! so does an entry whose address is too long to read: a listing that is not
//! all there must never be taken for the folder. An answer that is no whole
//! listing (a page of a proxy, a body cut off) is an error.
use alloc::string::String;

use paperpaw_sdk::{
    native::Error,
    web::try_percent_decode,
    xml::{Token, Tokenizer, local_name, try_decode},
};

use super::url::{path, resolve};
use crate::{files::Volume, mem::buffer, text::copy};

/// What a WebDAV folder is asked for: the kind, size, `ETag` and time of its
/// files.
pub const PROPFIND: &str = "<?xml version=\"1.0\"?><d:propfind xmlns:d=\"DAV:\"><d:prop><d:resourcetype/><d:getcontentlength/><d:getetag/><d:getlastmodified/></d:prop></d:propfind>";

/// The file the listing is saved to (in the plugin's data folder).
pub const LISTING: &str = "listing.xml";

/// A piece of the listing read at a time.
const CHUNK: usize = 2048;
/// An address or a number in a listing is read up to this many bytes.
const MAX_TEXT: usize = 1500;

/// A file or folder of the listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// The whole address.
    pub url: String,
    /// The last part of the path, decoded.
    pub name: String,
    pub folder: bool,
    /// A file's bytes; 0 where the server did not say.
    pub size: u64,
    /// Changes when the server's file does; 0 where the server told nothing.
    pub version: u64,
}

/// A number made of a text (FNV-1a); 0 for none.
#[must_use]
pub fn hash(text: &str) -> u64 {
    if text.is_empty() {
        return 0;
    }
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.bytes() {
        h = (h ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
    }
    h.max(1)
}

/// A number made of an `ETag` as a server tells it in a listing or in an
/// answer: the same for `"abc"`, `W/"abc"` and `abc`; 0 for none.
#[must_use]
pub fn etag_hash(etag: &str) -> u64 {
    let etag = etag.trim();
    let etag = etag.strip_prefix("W/").unwrap_or(etag);
    hash(etag.trim_matches('"'))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Href,
    Size,
    Etag,
    Modified,
}

impl Field {
    fn of(name: &str) -> Option<Self> {
        match name {
            "href" => Some(Self::Href),
            "getcontentlength" => Some(Self::Size),
            "getetag" => Some(Self::Etag),
            "getlastmodified" => Some(Self::Modified),
            _ => None,
        }
    }
}

/// One `<response>` of the listing.
#[derive(Debug, Default)]
struct Response {
    href: String,
    size: u64,
    version: u64,
    modified: u64,
    folder: bool,
}

struct Reader<'a, 'f> {
    base: &'a str,
    /// The folder's own path, decoded, without a `/` at the end.
    own: String,
    each: &'f mut dyn FnMut(Item) -> Result<(), Error>,
    current: Option<Response>,
    field: Option<Field>,
    text: String,
    cut: bool,
    /// Memory refused, or an entry was left out: said at the end.
    more: bool,
    /// The listing's last tag came: the answer is a whole listing.
    whole: bool,
    failed: bool,
    /// What the callback said where it failed.
    error: Option<Error>,
}

impl Reader<'_, '_> {
    fn token(&mut self, token: Token<'_>) {
        if !self.failed
            && let Err(error) = self.handle(token)
        {
            self.failed = true;
            self.more = true;
            self.error = Some(error);
        }
    }

    fn handle(&mut self, token: Token<'_>) -> Result<(), Error> {
        match token {
            Token::Start {
                name, self_closing, ..
            } => match local_name(name) {
                "response" => self.current = Some(Response::default()),
                "collection" => {
                    if let Some(r) = &mut self.current {
                        r.folder = true;
                    }
                }
                other if !self_closing => {
                    if let Some(field) = Field::of(other) {
                        self.field = Some(field);
                        self.text.clear();
                        self.cut = false;
                    }
                }
                _ => {}
            },
            Token::Text(text) => {
                let text = try_decode(&text).map_err(|_| Error::NoMemory)?;
                self.add(&text)?;
            }
            Token::CData(text) => self.add(text)?,
            Token::End { name } => self.end(local_name(name))?,
        }
        Ok(())
    }

    fn add(&mut self, piece: &str) -> Result<(), Error> {
        if self.field.is_none() {
            return Ok(());
        }
        if self.text.len().saturating_add(piece.len()) > MAX_TEXT {
            self.cut = true;
            return Ok(());
        }
        self.text
            .try_reserve(piece.len())
            .map_err(|_| Error::NoMemory)?;
        self.text.push_str(piece);
        Ok(())
    }

    fn end(&mut self, name: &str) -> Result<(), Error> {
        let field = Field::of(name);
        if field.is_some() && self.field == field {
            if let Some(r) = &mut self.current {
                let value = self.text.trim();
                match self.field {
                    Some(Field::Href) if self.cut => {
                        // Its name is not known: the listing is not all there.
                        r.href.clear();
                        self.more = true;
                    }
                    Some(Field::Href) => r.href = copy(value)?,
                    Some(Field::Size) => r.size = value.parse().unwrap_or(0),
                    Some(Field::Etag) => r.version = etag_hash(value),
                    _ => r.modified = hash(value),
                }
            }
            self.field = None;
        }
        if name == "response"
            && let Some(r) = self.current.take()
        {
            self.response(r)?;
        }
        if name == "multistatus" {
            self.whole = true;
        }
        Ok(())
    }

    fn response(&mut self, r: Response) -> Result<(), Error> {
        if r.href.is_empty() {
            return Ok(());
        }
        let decoded = try_percent_decode(&r.href).map_err(|_| Error::NoMemory)?;
        let target = decoded.trim_end_matches('/');
        if target == self.own {
            return Ok(());
        }
        let name = target.rsplit('/').next().unwrap_or("");
        if name.is_empty() {
            return Ok(());
        }
        let url = resolve(self.base, &r.href)?;
        if url.bytes().any(|b| b.is_ascii_control()) {
            return Ok(());
        }
        (self.each)(Item {
            url,
            name: copy(name)?,
            folder: r.folder,
            size: r.size,
            version: if r.version != 0 {
                r.version
            } else {
                r.modified
            },
        })
    }
}

/// Calls `each` with every file and folder of the listing saved as `name` on
/// `volume`, for the folder at `base`, in the order of the listing. The flag
/// says the listing is not all there: memory ran short (the reading stopped)
/// or an entry's address was too long to read. An error of `each` ends the
/// reading and is the result.
///
/// # Errors
/// What reading the file says; [`Error::NoMemory`]; what `each` says;
/// [`Error::Invalid`] for a file that is no whole listing (it never closed
/// its `multistatus`).
pub fn read_each(
    volume: &dyn Volume,
    name: &str,
    base: &str,
    each: &mut dyn FnMut(Item) -> Result<(), Error>,
) -> Result<bool, Error> {
    let own = try_percent_decode(path(base)).map_err(|_| Error::NoMemory)?;
    let mut reader = Reader {
        base,
        own: copy(own.trim_end_matches('/'))?,
        each,
        current: None,
        field: None,
        text: String::new(),
        cut: false,
        more: false,
        whole: false,
        failed: false,
        error: None,
    };
    let mut tokenizer = Tokenizer::new();
    let mut buf = buffer(CHUNK)?;
    let mut offset = 0u64;
    let mut stopped = false;
    loop {
        let n = volume.read_at(name, offset, &mut buf)?;
        if n == 0 {
            break;
        }
        let piece = buf.get(..n).ok_or(Error::Invalid)?;
        if tokenizer
            .try_feed(piece, &mut |token| reader.token(token))
            .is_err()
        {
            stopped = true;
            break;
        }
        if reader.failed {
            break;
        }
        offset += n as u64;
    }
    if !reader.failed && !stopped {
        tokenizer.finish(&mut |token| reader.token(token));
    }
    match reader.error {
        // The callback's own refusal of an item is not memory running short.
        Some(error) if error != Error::NoMemory => Err(error),
        _ if !reader.failed && !stopped && !reader.whole => Err(Error::Invalid),
        _ => Ok(reader.more || stopped),
    }
}
