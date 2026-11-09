//! Requests to a catalog through `core.net`: one place that builds them (the
//! catalog's credentials go only to the catalog itself: feeds may link to
//! covers, books or further feeds anywhere, and those get no password), sends
//! them and tells how they ended.
//!
//! Every answer is written to a file of the plugin's data folder (a feed, a
//! book, a cover), by name: one file for each kind of request, so a newer
//! request of a kind replaces the older one's file (the older ends as
//! replaced). The buffers of a request hold the password's header: they are
//! wiped as the call returns.
use alloc::string::String;
use paperpaw_sdk::{
    native::{
        Error, Host,
        net::{FileRoot, Header, HttpDone, Net, REPLY, REQUEST_INPUT, Reason, Request, RequestId},
    },
    web::USER_AGENT,
};
use paperpaw_ui_kit::{
    mem::{buffer, wipe},
    text::copy,
};

use crate::native_catalogs::Catalog;
use crate::native_dav::PROPFIND;
use crate::native_keep::Secret;
use crate::native_model::MAX_URL;

/// The plugin's own numbers of its requests, in their `http_done` events: one
/// for each kind, so all of a kind can be cancelled at once.
pub const FEED: u32 = 1;
pub const DESCRIPTION: u32 = 2;
pub const BOOK: u32 = 3;
pub const COVER: u32 = 4;
pub const SYNC_FEED: u32 = 5;
pub const SYNC_BOOK: u32 = 6;

/// The files the answers are written to, one for each kind of request.
pub const FEED_FILE: &str = "feed.tmp";
pub const DESCRIPTION_FILE: &str = "description.tmp";
pub const BOOK_FILE: &str = "book.tmp";
pub const COVER_FILE: &str = "cover.tmp";
pub const SYNC_FEED_FILE: &str = "sync-feed.tmp";
pub const SYNC_BOOK_FILE: &str = "sync-book.tmp";
/// Every file an answer may be in: removed when the plugin starts and stops.
pub const FILES: [&str; 6] = [
    FEED_FILE,
    DESCRIPTION_FILE,
    BOOK_FILE,
    COVER_FILE,
    SYNC_FEED_FILE,
    SYNC_BOOK_FILE,
];

/// What a feed, a description and a cover may be at most, and a book.
pub const SMALL_FILE: u64 = 1 << 20;
pub const COVER_BYTES: u64 = 512 << 10;
pub const BOOK_BYTES: u64 = 64 << 20;

/// What a request is for.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct Call<'a> {
    /// Its kind: one of the numbers above.
    pub token: u32,
    pub url: &'a str,
    /// The file its answer is written to, and the most it may be.
    pub file: &'a str,
    pub cap: u64,
    /// A list of a catalog (a WebDAV folder is asked for its listing).
    pub list: bool,
    /// `http_progress` events while a body comes.
    pub progress: bool,
}

impl<'a> Call<'a> {
    /// A request for `url` of the kind `token`, its answer in `file`.
    #[must_use]
    pub const fn new(token: u32, url: &'a str, file: &'a str, cap: u64) -> Self {
        Self {
            token,
            url,
            file,
            cap,
            list: false,
            progress: false,
        }
    }

    /// A list of the catalog.
    #[must_use]
    pub const fn list(mut self) -> Self {
        self.list = true;
        self
    }

    /// With progress events.
    #[must_use]
    pub const fn progress(mut self) -> Self {
        self.progress = true;
        self
    }
}

/// The `core.net` handle of the plugin.
#[derive(Debug, Default)]
pub struct Web {
    net: Option<Net>,
}

impl Web {
    /// No handle yet: it is opened by the first request.
    #[must_use]
    pub const fn new() -> Self {
        Self { net: None }
    }

    fn net(&mut self, host: &Host<'_>) -> Result<&Net, Error> {
        if self.net.is_none() {
            self.net = Some(Net::open(host)?);
        }
        self.net.as_ref().ok_or(Error::State)
    }

    /// Sends `call` to `catalog`, with `auth` (the `Authorization` header's
    /// value) where the address is the catalog's own.
    ///
    /// # Errors
    /// `NoMemory`; `Unsupported` or `Denied` where there is no network or no
    /// right to it; `Invalid` or `NoSpace` for a request that cannot be made;
    /// `Busy` at the plugin's 4 requests at once.
    pub fn send(
        &mut self,
        host: &Host<'_>,
        catalog: &Catalog,
        auth: Option<&Secret>,
        call: &Call<'_>,
    ) -> Result<RequestId, Error> {
        let (mut input, mut reply) = (buffer(REQUEST_INPUT)?, buffer(REPLY)?);
        let sent = self.send_in(host, catalog, auth, call, &mut input, &mut reply);
        wipe(&mut input);
        wipe(&mut reply);
        sent
    }

    fn send_in(
        &mut self,
        host: &Host<'_>,
        catalog: &Catalog,
        auth: Option<&Secret>,
        call: &Call<'_>,
        input: &mut [u8],
        reply: &mut [u8],
    ) -> Result<RequestId, Error> {
        let own = auth.filter(|_| same_origin(call.url, &catalog.url));
        let webdav = call.list && catalog.webdav;
        let user_agent = Header::new("User-Agent", USER_AGENT)?;
        let depth = Header::new("Depth", "1")?;
        let kind = Header::new("Content-Type", "application/xml; charset=utf-8")?;
        let mut headers = [user_agent; 4];
        let mut count = 1;
        if let Some(auth) = own {
            let header = Header::new("Authorization", auth.as_str())?;
            if let Some(slot) = headers.get_mut(count) {
                *slot = header;
                count += 1;
            }
        }
        if webdav {
            for header in [depth, kind] {
                if let Some(slot) = headers.get_mut(count) {
                    *slot = header;
                    count += 1;
                }
            }
        }
        let used = headers.get(..count).ok_or(Error::Invalid)?;
        let method = if webdav { "PROPFIND" } else { "GET" };
        let mut request = Request::new(call.token, method, call.url)?
            .headers(used)?
            .save_to(FileRoot::Data, call.file)?
            .max_file(call.cap)?;
        if webdav {
            request = request.body(PROPFIND.as_bytes())?;
        }
        if call.progress {
            request = request.progress();
        }
        self.net(host)?.request(host, &request, input, reply)
    }

    /// The address the answer of request `id` came from, after redirects;
    /// `None` where the answer says none (or one too long to keep). Only while
    /// its `http_done` event is delivered.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn final_url(&mut self, host: &Host<'_>, id: RequestId) -> Result<Option<String>, Error> {
        let (mut input, mut reply) = (buffer(REQUEST_INPUT)?, buffer(REPLY)?);
        let net = self.net(host)?;
        let found = match net.answer(host, id, &mut input, &mut reply) {
            Ok(answer) => {
                let url = answer.final_url();
                if url.is_empty() || url.len() > MAX_URL {
                    Ok(None)
                } else {
                    copy(url).map(Some)
                }
            }
            Err(Error::NoMemory) => Err(Error::NoMemory),
            Err(_) => Ok(None),
        };
        wipe(&mut input);
        wipe(&mut reply);
        found
    }

    /// Ends every request of the kind `token` (each ends as cancelled).
    pub fn cancel(&mut self, host: &Host<'_>, token: u32) {
        let (Some(net), Ok(mut input), Ok(mut reply)) =
            (self.net.as_ref(), buffer(REQUEST_INPUT), buffer(REPLY))
        else {
            return;
        };
        match net.cancel(host, token, &mut input, &mut reply) {
            Ok(()) | Err(_) => {}
        }
    }
}

/// How a request ended, for what is said of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Ended {
    /// A 2xx answer.
    Fine,
    /// 401 or 403: the catalog wants an account.
    Unauthorized,
    /// Any other status.
    Status(u32),
    /// No network, a network off or on hold.
    NoNetwork,
    /// It ended for a request of the same kind replacing it, or was
    /// cancelled: it is not told.
    Stopped,
    /// The host could not be reached, or the answer was too large.
    Unreached,
}

/// How `done` ended.
#[must_use]
pub fn ended(done: &HttpDone) -> Ended {
    match done.reason() {
        Reason::Ok => match done.status() {
            200..=299 => Ended::Fine,
            401 | 403 => Ended::Unauthorized,
            0 => Ended::Unreached,
            status => Ended::Status(status),
        },
        Reason::NoNetwork | Reason::Offline | Reason::Saving | Reason::NetworkOff => {
            Ended::NoNetwork
        }
        Reason::Replaced | Reason::Cancelled => Ended::Stopped,
        _ => Ended::Unreached,
    }
}

/// The scheme, host and port of `url`, in a form to compare: `None` where any
/// of them is missing or in doubt, such as an address with a user name
/// (`https://me@host/`). The port is filled in for `http` and `https`.
fn origin(url: &str) -> Option<(&str, &str, u16)> {
    let (scheme, rest) = url.split_once("://")?;
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = rest.get(..end)?;
    let doubtful = |c: char| matches!(c, '@' | '\\' | '%') || c.is_whitespace() || c.is_control();
    if authority.contains(doubtful) {
        return None;
    }
    // A colon inside `[…]` belongs to an IPv6 address, not the port.
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) if !port.contains(']') => (host, Some(port)),
        _ => (authority, None),
    };
    let port = match (port, scheme) {
        (Some(port), _) => port.parse().ok()?,
        (None, s) if s.eq_ignore_ascii_case("http") => 80,
        (None, s) if s.eq_ignore_ascii_case("https") => 443,
        (None, _) => return None,
    };
    (!host.is_empty()).then_some((scheme, host, port))
}

/// Whether `a` and `b` are on the same server: the same scheme, host and
/// port (letters in any case).
#[must_use]
pub fn same_origin(a: &str, b: &str) -> bool {
    match (origin(a), origin(b)) {
        (Some((s1, h1, p1)), Some((s2, h2, p2))) => {
            s1.eq_ignore_ascii_case(s2) && h1.eq_ignore_ascii_case(h2) && p1 == p2
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origins_are_scheme_host_and_port() {
        let catalog = "https://Books.example.org/opds";
        assert!(same_origin(catalog, "https://books.example.org:443/b.epub"));
        assert!(same_origin("http://[::1]:8080/a", "http://[::1]:8080/b"));
        for other in [
            "http://books.example.org/b.epub",
            "https://books.example.org:8443/b.epub",
            "https://covers.example.org/c.png",
            "https://books.example.org.evil.example/b.epub",
            "https://books.example.org@evil.example/b.epub",
            "https://evil.example\\@books.example.org/b.epub",
            "http://[::1]:8081/b",
            "ftp://books.example.org/b.epub",
            "/b.epub",
        ] {
            assert!(!same_origin(catalog, other), "{other}");
        }
    }
}
