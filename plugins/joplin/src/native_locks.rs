//! Joplin's sync locks, as `locks.rs` reads them in interface 6: a client
//! upgrading a sync target to a new version of its format holds an exclusive
//! lock on it meanwhile, and nothing is to be written then. PaperPaw looks for
//! one before it writes, and waits while one is held.
//!
//! Locks are kept as files `locks/<type>_<client type>_<client id>.json` in a
//! WebDAV folder (type 2 is exclusive), and listed at `api/locks` by Joplin
//! Server, each with when it was renewed (`updatedTime`); one not renewed for
//! 3 minutes has run out (`defaultLockTtl` in Joplin's `LockHandler`). The
//! times are the server's, compared with this device's clock. The answer is a
//! file in the data folder, read in pieces here, with every allocation
//! fallible; a lock list that cannot be read is an error, so nothing is
//! written then either. PaperPaw takes no shared lock of its own (see
//! interface 6's `locks.rs` for why).
use alloc::string::String;

use paperpaw_sdk::files::has_extension;
use paperpaw_sdk::json::stream::{Event as Json, Parser};
use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::files::Volume;
use paperpaw_xml::{Token, Tokenizer, local_name, try_decode};

use crate::native_joplin::Service;

/// How long a lock holds unless renewed, in ms.
pub const TTL: i64 = 3 * 60 * 1000;
/// The type of an exclusive lock.
const EXCLUSIVE: &str = "2";
/// Longest `<href>` or `<getlastmodified>` kept from a WebDAV listing, in
/// bytes; the rest of a longer one is left out (no lock has such a name).
const MAX_FIELD: usize = 1024;
/// How much of the list is read at a time.
const PIECE: usize = 512;
/// What a WebDAV folder is asked for: when each file changed.
pub const PROPFIND: &str = "<?xml version=\"1.0\"?><d:propfind xmlns:d=\"DAV:\">\
    <d:prop><d:getlastmodified/></d:prop></d:propfind>";

/// Whether the list of locks in the file `path` (the answer to the request for
/// them) holds an exclusive lock still held at `now` (ms since 1970): a client
/// upgrading the target.
///
/// # Errors
/// `NoMemory` where the heap had no room to read the list (not known: nothing
/// is written then either); what the card says.
pub fn upgrading(service: Service, v: &dyn Volume, path: &str, now: i64) -> Result<bool, Error> {
    let held = |renewed: Option<i64>| renewed.is_some_and(|at| now - at < TTL);
    match service {
        Service::WebDav => webdav_upgrading(v, path, &held),
        _ => server_upgrading(v, path, &held),
    }
}

/// The file `path` in pieces, each handed to `feed`.
fn pieces(
    v: &dyn Volume,
    path: &str,
    feed: &mut dyn FnMut(&[u8]) -> Result<(), Error>,
) -> Result<(), Error> {
    let mut piece = [0u8; PIECE];
    let mut at = 0u64;
    loop {
        let n = v.read_at(path, at, &mut piece)?;
        if n == 0 {
            return Ok(());
        }
        feed(piece.get(..n).unwrap_or(&[]))?;
        at += n as u64;
    }
}

/// Joplin Server's list: `{"items": [{"type": 2, "updatedTime": …}]}`.
fn server_upgrading(
    v: &dyn Volume,
    path: &str,
    held: &dyn Fn(Option<i64>) -> bool,
) -> Result<bool, Error> {
    let (mut found, mut exclusive, mut renewed) = (false, false, None);
    let mut parser = Parser::default();
    let mut emit = |event: Json<'_>| match event {
        Json::Open([items, item]) if items == "items" && item == "[]" => {
            (exclusive, renewed) = (false, None);
        }
        Json::Value([items, item, key], value) if items == "items" && item == "[]" => {
            match key.as_str() {
                "type" => exclusive = value == EXCLUSIVE,
                "updatedTime" => renewed = value.parse().ok(),
                _ => {}
            }
        }
        Json::Close([items, item]) if items == "items" && item == "[]" => {
            found |= exclusive && held(renewed);
        }
        _ => {}
    };
    pieces(v, path, &mut |piece| {
        parser
            .try_feed(piece, &mut emit)
            .map_err(|_| Error::NoMemory)
    })?;
    parser.try_finish(&mut emit).map_err(|_| Error::NoMemory)?;
    Ok(found)
}

/// Adds `text` to `field`, as much as fits in [`MAX_FIELD`] bytes.
fn push_bounded(field: &mut String, text: &str) -> Result<(), Error> {
    let room = MAX_FIELD.saturating_sub(field.len());
    let mut end = text.len().min(room);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let part = text.get(..end).unwrap_or_default();
    field.try_reserve(part.len()).map_err(|_| Error::NoMemory)?;
    field.push_str(part);
    Ok(())
}

/// A WebDAV listing of the folder `locks`: a `<response>` per file, its
/// `<href>` and `<getlastmodified>`.
fn webdav_upgrading(
    v: &dyn Volume,
    path: &str,
    held: &dyn Fn(Option<i64>) -> bool,
) -> Result<bool, Error> {
    let (mut found, mut href, mut modified) = (false, String::new(), String::new());
    let mut field: Option<bool> = None;
    let mut failed = None;
    let mut tokenizer = Tokenizer::new();
    let mut emit = |token: Token<'_>| {
        if failed.is_some() {
            return;
        }
        match token {
            Token::Start {
                name, self_closing, ..
            } => match local_name(name) {
                "response" => {
                    href.clear();
                    modified.clear();
                }
                "href" if !self_closing => field = Some(true),
                "getlastmodified" if !self_closing => field = Some(false),
                _ => {}
            },
            Token::Text(text) => {
                let target = match field {
                    Some(true) => &mut href,
                    Some(false) => &mut modified,
                    None => return,
                };
                let pushed = try_decode(&text)
                    .map_err(|_| Error::NoMemory)
                    .and_then(|decoded| push_bounded(target, &decoded));
                if let Err(error) = pushed {
                    failed = Some(error);
                }
            }
            Token::End { name } => match local_name(name) {
                "href" | "getlastmodified" => field = None,
                "response" => {
                    let file = href.trim().trim_end_matches('/');
                    let file = file.rsplit('/').next().unwrap_or_default();
                    let exclusive = file.starts_with("2_") && has_extension(file, "json");
                    let renewed = paperpaw_sdk::time::UnixTime::parse_http_date(&modified)
                        .map(|t| t.millis());
                    found |= exclusive && held(renewed);
                }
                _ => {}
            },
            _ => {}
        }
    };
    pieces(v, path, &mut |piece| {
        tokenizer
            .try_feed(piece, &mut emit)
            .map_err(|_| Error::NoMemory)
    })?;
    tokenizer.finish(&mut emit);
    drop(emit);
    failed.map_or(Ok(found), Err)
}
