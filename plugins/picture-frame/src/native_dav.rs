//! A WebDAV folder as a list of photos: the listing a `PROPFIND` of depth 1
//! answers, read as it streams from the file it was saved to (nothing larger
//! than a piece of it is in memory). Files with a photo's name become
//! [`Remote`]s with the address to fetch them from and their size; folders and
//! the folder itself are left out. A refused allocation ends the reading with
//! what was read; an address too long to keep is left out.
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::{
    native::Error,
    web::try_percent_decode,
    xml::{Token, Tokenizer, local_name, try_decode},
};
use paperpaw_ui_kit::{
    files::Volume,
    mem::{buffer, push},
    text::{concat, copy},
};

use crate::native_cache::{HREF_MAX, MOST, Remote};
use crate::native_photos::{by_name, extension};

/// What a WebDAV folder is asked for: the kind and the size of its files.
pub const PROPFIND: &str = "<?xml version=\"1.0\"?><d:propfind xmlns:d=\"DAV:\"><d:prop><d:resourcetype/><d:getcontentlength/></d:prop></d:propfind>";

/// The file the listing is saved to (in the plugin's data folder).
pub const LISTING: &str = "listing.xml";

/// A piece of the listing read at a time.
const CHUNK: usize = 2048;
/// An address or a number in a listing is read up to this many bytes.
const MAX_TEXT: usize = 1500;

/// `scheme://host[:port]` of an address; empty where it has none.
#[must_use]
pub fn origin(url: &str) -> &str {
    let Some((scheme, rest)) = url.split_once("://") else {
        return "";
    };
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    url.get(..scheme.len() + 3 + end).unwrap_or("")
}

/// The path of an address, without a query or fragment, `/` where it has none.
#[must_use]
pub fn path(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let from = rest.find('/').unwrap_or(rest.len());
    let path = rest.get(from..).unwrap_or("");
    let path = path.split(['?', '#']).next().unwrap_or("");
    if path.is_empty() { "/" } else { path }
}

/// The address a listing's `href` means for the folder at `base`: a whole
/// address as it is, a path on the folder's server, else beside the folder.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn resolve(base: &str, href: &str) -> Result<String, Error> {
    if href.contains("://") {
        return copy(href);
    }
    let origin = origin(base);
    if href.starts_with('/') {
        return concat(&[origin, href]);
    }
    let folder = path(base);
    concat(&[origin, folder.trim_end_matches('/'), "/", href])
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Href,
    Size,
}

/// One `<response>` of the listing.
#[derive(Debug, Default)]
struct Response {
    href: String,
    size: u64,
    folder: bool,
}

#[derive(Debug)]
struct Reader<'a> {
    base: &'a str,
    /// The folder's own path, decoded, without a `/` at the end.
    own: String,
    photos: Vec<Remote>,
    current: Option<Response>,
    field: Option<Field>,
    text: String,
    cut: bool,
    /// More photos than are kept, or memory refused: said at the end.
    more: bool,
    failed: bool,
}

impl Reader<'_> {
    fn token(&mut self, token: Token<'_>) {
        if !self.failed && self.handle(token).is_err() {
            self.failed = true;
            self.more = true;
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
                "href" | "getcontentlength" if !self_closing => {
                    self.field = Some(if local_name(name) == "href" {
                        Field::Href
                    } else {
                        Field::Size
                    });
                    self.text.clear();
                    self.cut = false;
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
        let field = match name {
            "href" => Some(Field::Href),
            "getcontentlength" => Some(Field::Size),
            _ => None,
        };
        if field.is_some() && self.field == field {
            if let Some(r) = &mut self.current {
                let value = self.text.trim();
                match self.field {
                    Some(Field::Href) if self.cut => r.href.clear(),
                    Some(Field::Href) => r.href = copy(value)?,
                    _ => r.size = value.parse().unwrap_or(0),
                }
            }
            self.field = None;
        }
        if name == "response"
            && let Some(r) = self.current.take()
        {
            self.response(r)?;
        }
        Ok(())
    }

    fn response(&mut self, r: Response) -> Result<(), Error> {
        if r.folder || r.href.is_empty() {
            return Ok(());
        }
        let decoded = try_percent_decode(&r.href).map_err(|_| Error::NoMemory)?;
        let target = decoded.trim_end_matches('/');
        if target == self.own {
            return Ok(());
        }
        let file = target.rsplit('/').next().unwrap_or("");
        if extension(file).is_none() || file.starts_with('.') {
            return Ok(());
        }
        if self.photos.len() >= MOST {
            self.more = true;
            return Ok(());
        }
        let url = resolve(self.base, &r.href)?;
        if url.len() > HREF_MAX || url.bytes().any(|b| b.is_ascii_control()) {
            return Ok(());
        }
        push(
            &mut self.photos,
            Remote {
                href: url,
                size: r.size,
            },
        )
    }
}

/// The photos of the listing saved as `name` on `volume`, for the folder at
/// `base`, sorted by address. At most [`MOST`]; the flag says there were more
/// or memory ran short.
///
/// # Errors
/// What reading the file says; [`Error::NoMemory`].
pub fn read(volume: &dyn Volume, name: &str, base: &str) -> Result<(Vec<Remote>, bool), Error> {
    let own = try_percent_decode(path(base)).map_err(|_| Error::NoMemory)?;
    let mut reader = Reader {
        base,
        own: copy(own.trim_end_matches('/'))?,
        photos: Vec::new(),
        current: None,
        field: None,
        text: String::new(),
        cut: false,
        more: false,
        failed: false,
    };
    let mut tokenizer = Tokenizer::new();
    let mut buf = buffer(CHUNK)?;
    let mut offset = 0u64;
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
            reader.more = true;
            break;
        }
        offset += n as u64;
    }
    if !reader.more {
        tokenizer.finish(&mut |token| reader.token(token));
    }
    let more = reader.more;
    let mut photos = core::mem::take(&mut reader.photos);
    // The shared sorter: the std sort keeps a scratch the X4 refuses on the stack.
    paperpaw_sdk::sort::sort_by(&mut photos, |a, b| by_name(&a.href, &b.href));
    Ok((photos, more))
}

#[cfg(test)]
#[path = "native_dav_tests.rs"]
mod tests;
