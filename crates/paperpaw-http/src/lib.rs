//! HTTP/1.1, without I/O: for a shell that has its own
//! network stack (the X4's, on smoltcp) and passes bytes in and out.
//!
//! - [`Url::parse`] splits an address; [`request`] builds what to send;
//! - a [`Parser`] takes the response as it comes, in pieces of any size,
//!   and passes the body on in pieces (by `Content-Length`, chunked, or up
//!   to the connection's end), so a large one never needs to fit in
//!   memory;
//! - [`redirect`] tells where a redirect leads, [`follow`] whether and
//!   how a request goes there (what of it goes along);
//! - [`server`]: a request's head read, an answer's head made, for a
//!   shell that serves plugins.
//!
//! One request per connection (`Connection: close`).

#![no_std]

extern crate alloc;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

pub mod server;

/// What went wrong with a response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// Not HTTP as expected (or a head longer than [`MAX_HEAD`]).
    Malformed,
    /// The connection ended before the whole body came.
    Cut,
}

/// The longest response head taken.
pub const MAX_HEAD: usize = 16 * 1024;

/// An address split up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Url<'a> {
    pub https: bool,
    pub host: &'a str,
    pub port: u16,
    /// From the first `/` on, with the query (`/` if none).
    pub path: &'a str,
}

impl<'a> Url<'a> {
    /// Splits `http://host[:port]/path` or `https://…` (an IPv6 host in
    /// brackets, as in `http://[::1]:8080/`, without them in `host`);
    /// `None` for anything else, and for an address with control
    /// characters, which could end the request's line.
    pub fn parse(url: &'a str) -> Option<Self> {
        if url.bytes().any(|b| b.is_ascii_control()) {
            return None;
        }
        let (https, rest) = if let Some(rest) = strip_prefix_ignore_case(url, "http://") {
            (false, rest)
        } else {
            (true, strip_prefix_ignore_case(url, "https://")?)
        };
        let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
        let (authority, path) = rest.split_at(end);
        // Credentials in the address are not sent.
        let authority = authority.rsplit('@').next().unwrap_or(authority);
        let path = path.split('#').next().unwrap_or("");
        let path = if path.is_empty() { "/" } else { path };
        let (host, port) = match authority.strip_prefix('[') {
            Some(inner) => {
                let (host, after) = inner.split_once(']')?;
                let port = match after {
                    "" => None,
                    after => Some(after.strip_prefix(':')?),
                };
                let ipv6 = host.contains(':')
                    && host
                        .bytes()
                        .all(|b| b.is_ascii_hexdigit() || b == b':' || b == b'.');
                (ipv6.then_some(host)?, port)
            }
            None => {
                let (host, port) = match authority.split_once(':') {
                    Some((host, port)) => (host, Some(port)),
                    None => (authority, None),
                };
                let name = host
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~'));
                (name.then_some(host)?, port)
            }
        };
        let port = match port {
            None | Some("") => default_port(https),
            Some(port) if port.bytes().all(|b| b.is_ascii_digit()) => {
                port.parse().ok().filter(|&port| port != 0)?
            }
            Some(_) => return None,
        };
        (!host.is_empty()).then_some(Self {
            https,
            host,
            port,
            path,
        })
    }

    /// The `Host` header's value: the host (an IPv6 one in brackets), with
    /// the port if not the scheme's own.
    pub fn host_header(&self) -> String {
        let host = if self.host.contains(':') {
            format!("[{}]", self.host)
        } else {
            self.host.to_string()
        };
        if self.port == default_port(self.https) {
            host
        } else {
            format!("{host}:{}", self.port)
        }
    }

    fn scheme(&self) -> &'static str {
        if self.https { "https" } else { "http" }
    }

    /// Whether `other` is the same server: scheme, host and port.
    fn same_origin(&self, other: &Url<'_>) -> bool {
        self.https == other.https
            && self.port == other.port
            && self.host.eq_ignore_ascii_case(other.host)
    }
}

fn default_port(https: bool) -> u16 {
    if https { 443 } else { 80 }
}

fn strip_prefix_ignore_case<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let head = text.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &text[prefix.len()..])
}

/// The bytes of a request's head (the body, if any, goes after them).
/// `headers` may override `User-Agent`; `Host`, `Connection` and
/// `Content-Length` are set here, and headers that are not well formed
/// are left out. Spaces and other bytes a request line cannot carry are
/// percent-encoded in the path.
pub fn request(
    method: &str,
    url: &Url<'_>,
    headers: &[(String, String)],
    body_len: Option<usize>,
) -> Vec<u8> {
    request_keeping(method, url, headers, body_len, false)
}

/// [`request`], asking the server to keep the connection open
/// (`keep_alive`) for the next request instead of closing it.
pub fn request_keeping(
    method: &str,
    url: &Url<'_>,
    headers: &[(String, String)],
    body_len: Option<usize>,
    keep_alive: bool,
) -> Vec<u8> {
    let mut head = format!("{method} ");
    for &byte in url.path.as_bytes() {
        if byte <= b' ' || byte >= 0x7F || matches!(byte, b'"' | b'<' | b'>' | b'`' | b'{' | b'}') {
            head.push_str(&format!("%{byte:02X}"));
        } else {
            head.push(char::from(byte));
        }
    }
    head.push_str(&format!(
        " HTTP/1.1\r\nHost: {}\r\nConnection: {}\r\n",
        url.host_header(),
        if keep_alive { "keep-alive" } else { "close" }
    ));
    let ours = ["host", "connection", "content-length"];
    for (name, value) in headers {
        if ours.iter().any(|o| name.eq_ignore_ascii_case(o)) || !is_header(name, value) {
            continue;
        }
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    if !headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("user-agent"))
    {
        head.push_str("User-Agent: PaperPaw\r\n");
    }
    if let Some(len) = body_len {
        head.push_str(&format!("Content-Length: {len}\r\n"));
    }
    head.push_str("\r\n");
    head.into_bytes()
}

/// Whether `name: value` is a header as HTTP has them: a name of token
/// characters, a value without control characters (but tabs), so that
/// neither can end the header or start another.
pub(crate) fn is_header(name: &str, value: &str) -> bool {
    let token = |b: u8| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b);
    !name.is_empty()
        && name.bytes().all(token)
        && value.bytes().all(|b| b == b'\t' || !b.is_ascii_control())
}

/// Where a redirect from `base` to `location` leads (absolute, or relative
/// to `base` as RFC 3986 resolves it); `None` where that is no `http` or
/// `https` address (another scheme, control characters).
pub fn redirect(base: &str, location: &str) -> Option<String> {
    let location = location.trim();
    let location = location.split('#').next().unwrap_or("");
    let url = Url::parse(base)?;
    let resolved = if has_scheme(location) {
        String::from(location)
    } else if let Some(rest) = location.strip_prefix("//") {
        format!("{}://{rest}", url.scheme())
    } else {
        let (path, query) = match location.split_once('?') {
            Some((path, query)) => (path, Some(query)),
            None => (location, None),
        };
        let (base_path, base_query) = match url.path.split_once('?') {
            Some((path, query)) => (path, Some(query)),
            None => (url.path, None),
        };
        // A base without a path (`http://h.org?x`) is at the root.
        let base_path = if base_path.is_empty() { "/" } else { base_path };
        let (path, query) = if path.is_empty() {
            (String::from(base_path), query.or(base_query))
        } else if path.starts_with('/') {
            (without_dots(path), query)
        } else {
            let dir = &base_path[..base_path.rfind('/').map_or(0, |at| at + 1)];
            (without_dots(&format!("{dir}{path}")), query)
        };
        let query = query.map_or_else(String::new, |query| format!("?{query}"));
        format!("{}://{}{path}{query}", url.scheme(), url.host_header())
    };
    Url::parse(&resolved)?;
    Some(resolved)
}

/// Whether `reference` starts with a scheme (`name:`), as an absolute
/// address does.
fn has_scheme(reference: &str) -> bool {
    let end = reference.find(['/', '?', '#']).unwrap_or(reference.len());
    reference[..end].split_once(':').is_some_and(|(scheme, _)| {
        scheme.starts_with(|c: char| c.is_ascii_alphabetic())
            && scheme
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.'))
    })
}

/// `path` with its `.` and `..` segments resolved (RFC 3986, 5.2.4).
fn without_dots(path: &str) -> String {
    let mut kept: Vec<&str> = Vec::new();
    for segment in path.split('/').skip(1) {
        match segment {
            "." => {}
            ".." => {
                kept.pop();
            }
            segment => kept.push(segment),
        }
    }
    let mut out = String::new();
    for segment in &kept {
        out.push('/');
        out.push_str(segment);
    }
    if out.is_empty() || path.ends_with("/.") || path.ends_with("/..") {
        out.push('/');
    }
    out
}

/// Where a redirect leads, and how the request changes on the way there
/// (see [`follow`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Redirect {
    /// The address it leads to.
    pub url: String,
    /// The request goes on as a `GET` without its body: after a 303, or
    /// a 301 or 302 of a `POST`.
    pub to_get: bool,
    /// It leads to another server: another scheme, host or port (from
    /// `http` to `https` on the same host and the schemes' own ports is
    /// the same server, now over TLS). Only the headers
    /// [`Redirect::keep`] keeps go along.
    pub elsewhere: bool,
}

impl Redirect {
    /// Keeps of `headers` those that may go where the redirect leads: all
    /// to the same server; elsewhere only those that never carry
    /// credentials (`Accept`, `Range`, `User-Agent`, …), so that neither
    /// `Authorization`, cookies nor a service's own key headers reach
    /// another host.
    pub fn keep(&self, headers: &mut Vec<(String, String)>) {
        if self.elsewhere {
            headers.retain(|(name, _)| {
                HARMLESS_HEADERS
                    .iter()
                    .any(|harmless| name.eq_ignore_ascii_case(harmless))
            });
        }
    }
}

/// The headers that go along to another server on a redirect.
const HARMLESS_HEADERS: [&str; 12] = [
    "accept",
    "accept-charset",
    "accept-language",
    "cache-control",
    "content-type",
    "depth",
    "if-match",
    "if-modified-since",
    "if-none-match",
    "if-unmodified-since",
    "range",
    "user-agent",
];

/// Why a redirect is not followed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// Its `Location` is no `http` or `https` address.
    NotAnAddress,
    /// It leads from `https` to plain `http`.
    Insecure,
    /// It would take the request's body to another server.
    BodyElsewhere,
}

impl Refusal {
    /// Why, in a few words (for the plugin that asked).
    pub fn reason(self) -> &'static str {
        match self {
            Refusal::NotAnAddress => "redirected to no address",
            Refusal::Insecure => "redirected from https to http",
            Refusal::BodyElsewhere => "redirected to another server with the request's body",
        }
    }
}

/// How a request to `from` with `method` (and a body, if `body`) follows
/// a redirect with `status` to `location`, the same in every shell: never
/// from `https` to `http`, never with credentials or a body to another
/// server.
pub fn follow(
    from: &str,
    status: u16,
    location: &str,
    method: &str,
    body: bool,
) -> Result<Redirect, Refusal> {
    let url = redirect(from, location).ok_or(Refusal::NotAnAddress)?;
    let (Some(old), Some(new)) = (Url::parse(from), Url::parse(&url)) else {
        return Err(Refusal::NotAnAddress);
    };
    if old.https && !new.https {
        return Err(Refusal::Insecure);
    }
    let upgrade = !old.https
        && new.https
        && old.port == 80
        && new.port == 443
        && old.host.eq_ignore_ascii_case(new.host);
    let elsewhere = !old.same_origin(&new) && !upgrade;
    let to_get = !method.eq_ignore_ascii_case("HEAD")
        && (status == 303 || (matches!(status, 301 | 302) && method.eq_ignore_ascii_case("POST")));
    if elsewhere && body && !to_get {
        return Err(Refusal::BodyElsewhere);
    }
    Ok(Redirect {
        url,
        to_get,
        elsewhere,
    })
}

/// What the head of a response said.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Head {
    pub status: u16,
    pub content_type: Option<String>,
    pub etag: Option<String>,
    pub location: Option<String>,
    /// `Content-Length`, if given.
    pub length: Option<u64>,
    pub chunked: bool,
    /// The server keeps the connection open after the answer (HTTP/1.1
    /// without `Connection: close`, or HTTP/1.0 with `keep-alive`).
    pub keep_alive: bool,
    /// The headers asked for ([`Parser::wanting`]) that came, their names
    /// in lower case.
    pub headers: Vec<(String, String)>,
}

impl Head {
    /// A redirect that says where to.
    pub fn is_redirect(&self) -> bool {
        matches!(self.status, 301 | 302 | 303 | 307 | 308) && self.location.is_some()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Chunk {
    /// Reading a chunk's size line.
    Size,
    /// Bytes of the chunk left.
    Data(u64),
    /// The line break after a chunk's data.
    DataEnd,
    /// The lines after the last chunk, until an empty one.
    Trailer,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum State {
    /// Gathering the head.
    Head,
    /// Bytes of the body left (by `Content-Length`).
    Length(u64),
    Chunked(Chunk),
    /// Up to the connection's end.
    ToClose,
    Done,
}

/// Takes a response as it comes.
#[derive(Debug)]
pub struct Parser {
    state: State,
    /// The head so far, or a chunked body's current line.
    line: Vec<u8>,
    head: Option<Head>,
    /// The request was `HEAD`: no body follows.
    no_body: bool,
    /// Headers to keep, in lower case ([`Head::headers`]).
    wanted: Vec<String>,
}

impl Parser {
    /// A parser for the answer to a request with `method`.
    pub fn new(method: &str) -> Self {
        Self {
            state: State::Head,
            line: Vec::new(),
            head: None,
            no_body: method.eq_ignore_ascii_case("HEAD"),
            wanted: Vec::new(),
        }
    }

    /// Keeps these headers of the answer too ([`Head::headers`]).
    pub fn wanting(mut self, names: &[String]) -> Self {
        self.wanted = names.iter().map(|n| n.to_ascii_lowercase()).collect();
        self
    }

    /// The head, once it came.
    pub fn head(&self) -> Option<&Head> {
        self.head.as_ref()
    }

    /// The response's end is told by its own length or chunks (or it has
    /// no body), not by the connection's end: the connection can carry
    /// another request after it.
    pub fn delimited(&self) -> bool {
        self.no_body
            || self
                .head
                .as_ref()
                .is_some_and(|head| head.length.is_some() || head.chunked)
    }

    /// The whole response came.
    pub fn done(&self) -> bool {
        self.state == State::Done
    }

    /// Takes the next bytes received; the body's go to `body`, in pieces.
    /// Bytes after the end are ignored.
    pub fn feed(&mut self, mut data: &[u8], body: &mut dyn FnMut(&[u8])) -> Result<(), Error> {
        while !data.is_empty() {
            match self.state.clone() {
                State::Head => data = self.take_head(data)?,
                State::Length(left) => {
                    let n = data.len().min(usize::try_from(left).unwrap_or(usize::MAX));
                    body(&data[..n]);
                    data = &data[n..];
                    let left = left - n as u64;
                    self.state = if left == 0 {
                        State::Done
                    } else {
                        State::Length(left)
                    };
                }
                State::ToClose => {
                    body(data);
                    data = &[];
                }
                State::Chunked(chunk) => data = self.take_chunked(chunk, data, body)?,
                State::Done => data = &[],
            }
        }
        Ok(())
    }

    /// The connection ended: fine if the response was whole, or one that
    /// ends with the connection.
    pub fn close(&mut self) -> Result<(), Error> {
        match self.state {
            State::Done => Ok(()),
            State::ToClose => {
                self.state = State::Done;
                Ok(())
            }
            _ => Err(Error::Cut),
        }
    }

    fn take_head<'d>(&mut self, data: &'d [u8]) -> Result<&'d [u8], Error> {
        let from = self.line.len().saturating_sub(3);
        self.line.extend_from_slice(data);
        let Some(end) = find(&self.line[from..], b"\r\n\r\n").map(|at| from + at) else {
            if self.line.len() > MAX_HEAD {
                return Err(Error::Malformed);
            }
            return Ok(&[]);
        };
        // However it came: in pieces or at once.
        if end > MAX_HEAD {
            return Err(Error::Malformed);
        }
        let rest_len = self.line.len() - (end + 4);
        let head = parse_head(&self.line[..end], &self.wanted).ok_or(Error::Malformed)?;
        self.line.clear();
        let rest = &data[data.len() - rest_len..];
        // An interim answer (e.g. 100 Continue): the real one follows.
        if (100..200).contains(&head.status) {
            return Ok(rest);
        }
        self.state = if self.no_body || head.status == 204 || head.status == 304 {
            State::Done
        } else if head.chunked {
            State::Chunked(Chunk::Size)
        } else {
            match head.length {
                Some(0) => State::Done,
                Some(len) => State::Length(len),
                None => State::ToClose,
            }
        };
        self.head = Some(head);
        Ok(rest)
    }

    fn take_chunked<'d>(
        &mut self,
        chunk: Chunk,
        data: &'d [u8],
        body: &mut dyn FnMut(&[u8]),
    ) -> Result<&'d [u8], Error> {
        match chunk {
            Chunk::Data(left) => {
                let n = data.len().min(usize::try_from(left).unwrap_or(usize::MAX));
                body(&data[..n]);
                let left = left - n as u64;
                self.state = State::Chunked(if left == 0 {
                    Chunk::DataEnd
                } else {
                    Chunk::Data(left)
                });
                Ok(&data[n..])
            }
            Chunk::Size | Chunk::DataEnd | Chunk::Trailer => {
                let Some(at) = data.iter().position(|&b| b == b'\n') else {
                    self.line.extend_from_slice(data);
                    if self.line.len() > 1024 {
                        return Err(Error::Malformed);
                    }
                    return Ok(&[]);
                };
                self.line.extend_from_slice(&data[..at]);
                let line = core::mem::take(&mut self.line);
                let line = line.strip_suffix(b"\r").unwrap_or(&line);
                self.state = match chunk {
                    Chunk::DataEnd if line.is_empty() => State::Chunked(Chunk::Size),
                    Chunk::DataEnd => return Err(Error::Malformed),
                    Chunk::Trailer if line.is_empty() => State::Done,
                    Chunk::Trailer => State::Chunked(Chunk::Trailer),
                    _ => {
                        let size = core::str::from_utf8(line)
                            .ok()
                            .map(|l| l.split(';').next().unwrap_or("").trim())
                            .and_then(|hex| u64::from_str_radix(hex, 16).ok())
                            .ok_or(Error::Malformed)?;
                        State::Chunked(if size == 0 {
                            Chunk::Trailer
                        } else {
                            Chunk::Data(size)
                        })
                    }
                };
                Ok(&data[at + 1..])
            }
        }
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn parse_head(bytes: &[u8], wanted: &[String]) -> Option<Head> {
    let text = core::str::from_utf8(bytes).ok()?;
    let mut lines = text.split("\r\n");
    let status_line = lines.next()?;
    let mut words = status_line.split_ascii_whitespace();
    let version = words.next()?;
    if !version.starts_with("HTTP/") {
        return None;
    }
    let mut head = Head {
        status: words.next()?.parse().ok()?,
        ..Head::default()
    };
    let mut keeps = version.eq_ignore_ascii_case("HTTP/1.1");
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        let name = name.trim().to_ascii_lowercase();
        if wanted.contains(&name) {
            head.headers.push((name.clone(), value.to_string()));
        }
        match name.as_str() {
            "content-type" => head.content_type = Some(value.to_string()),
            "etag" => head.etag = Some(value.to_string()),
            "location" => head.location = Some(value.to_string()),
            "content-length" => head.length = Some(value.parse().ok()?),
            "connection" => {
                for token in value.split(',').map(str::trim) {
                    if token.eq_ignore_ascii_case("close") {
                        keeps = false;
                        break;
                    }
                    if token.eq_ignore_ascii_case("keep-alive") {
                        keeps = true;
                    }
                }
            }
            "transfer-encoding" => {
                head.chunked = value
                    .rsplit(',')
                    .next()
                    .is_some_and(|last| last.trim().eq_ignore_ascii_case("chunked"));
            }
            _ => {}
        }
    }
    head.keep_alive = keeps;
    Some(head)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Feeds `response` in pieces of `step` bytes; the head and the body.
    fn run(method: &str, response: &[u8], step: usize, close: bool) -> (Head, Vec<u8>, bool) {
        let mut parser = Parser::new(method);
        let mut body = Vec::new();
        for piece in response.chunks(step) {
            parser
                .feed(piece, &mut |b| body.extend_from_slice(b))
                .unwrap();
        }
        let ok = if close {
            parser.close().is_ok()
        } else {
            parser.done()
        };
        (parser.head().cloned().unwrap(), body, ok)
    }

    #[test]
    fn urls() {
        let url = Url::parse("http://Example.org:8080/a/b?c=1#top").unwrap();
        assert_eq!(
            url,
            Url {
                https: false,
                host: "Example.org",
                port: 8080,
                path: "/a/b?c=1"
            }
        );
        assert_eq!(url.host_header(), "Example.org:8080");
        let url = Url::parse("HTTPS://user:pw@host?q").unwrap();
        assert_eq!(
            (url.https, url.host, url.port, url.path),
            (true, "host", 443, "?q")
        );
        assert_eq!(Url::parse("http://host").unwrap().path, "/");
        assert_eq!(Url::parse("ftp://host"), None);
        assert_eq!(Url::parse("http://"), None);
        assert_eq!(Url::parse("http://host:x/"), None);
    }

    #[test]
    fn keep_alive_is_what_the_server_says() {
        let head = |text: &str| parse_head(text.as_bytes(), &[]).unwrap();
        assert!(head("HTTP/1.1 200 OK\r\nContent-Length: 1").keep_alive);
        assert!(!head("HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 1").keep_alive);
        assert!(!head("HTTP/1.1 200 OK\r\nconnection: Keep-Alive, Close").keep_alive);
        assert!(!head("HTTP/1.0 200 OK\r\nContent-Length: 1").keep_alive);
        assert!(head("HTTP/1.0 200 OK\r\nConnection: keep-alive").keep_alive);
    }

    #[test]
    fn a_kept_request_asks_for_it_and_a_plain_one_closes() {
        let url = Url::parse("https://h.org/a").unwrap();
        let kept = request_keeping("GET", &url, &[], None, true);
        assert!(
            core::str::from_utf8(&kept)
                .unwrap()
                .contains("Connection: keep-alive\r\n")
        );
        let plain = request("GET", &url, &[], None);
        assert!(
            core::str::from_utf8(&plain)
                .unwrap()
                .contains("Connection: close\r\n")
        );
    }

    #[test]
    fn only_a_counted_or_chunked_answer_ends_without_the_connection() {
        let mut counted = Parser::new("GET");
        counted
            .feed(
                b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nab",
                &mut |_| {},
            )
            .unwrap();
        assert!(counted.done() && counted.delimited());
        let mut open = Parser::new("GET");
        open.feed(b"HTTP/1.1 200 OK\r\n\r\nab", &mut |_| {})
            .unwrap();
        assert!(!open.delimited());
    }

    #[test]
    fn requests() {
        let url = Url::parse("http://h.org/p?q").unwrap();
        let headers = [
            (String::from("Accept"), String::from("*/*")),
            (String::from("Host"), String::from("evil")),
            (String::from("X-Bad"), String::from("a\r\nInjected: 1")),
        ];
        let head = String::from_utf8(request("POST", &url, &headers, Some(3))).unwrap();
        assert_eq!(
            head,
            "POST /p?q HTTP/1.1\r\nHost: h.org\r\nConnection: close\r\nAccept: */*\r\n\
             User-Agent: PaperPaw\r\nContent-Length: 3\r\n\r\n"
        );
    }

    #[test]
    fn ipv6_hosts() {
        let url = Url::parse("http://[::1]:8080/a").unwrap();
        assert_eq!((url.host, url.port, url.path), ("::1", 8080, "/a"));
        assert_eq!(url.host_header(), "[::1]:8080");
        let url = Url::parse("https://[fe80::1]").unwrap();
        assert_eq!((url.host, url.port), ("fe80::1", 443));
        assert_eq!(url.host_header(), "[fe80::1]");
        assert_eq!(Url::parse("http://h.org:/x").unwrap().port, 80);
        for bad in [
            "http://[::1/",
            "http://[::1]x/",
            "http://[h.org]/",
            "http://a:b:c/",
            "http://h.org:+80/",
            "http://h.org:0/",
            "http://h.org:99999/",
        ] {
            assert_eq!(Url::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn addresses_that_could_break_the_request() {
        for bad in [
            "http://h.org/a\r\nX-Injected: 1",
            "http://h.org/a\nb",
            "http://h.org/\0",
            "http://h .org/",
            "http://h.org\t/",
            "http://ev<il/",
        ] {
            assert_eq!(Url::parse(bad), None, "{bad:?}");
        }
        // Spaces and other bytes a request line cannot carry are encoded.
        let url = Url::parse("http://h.org/a b/\"ü\"?q={x}").unwrap();
        let head = String::from_utf8(request("GET", &url, &[], None)).unwrap();
        assert!(
            head.starts_with("GET /a%20b/%22%C3%BC%22?q=%7Bx%7D HTTP/1.1\r\n"),
            "{head}"
        );
        let headers = [
            (String::from("X Bad"), String::from("1")),
            (String::from("X-Bad:"), String::from("1")),
            (String::from("X-Nul"), String::from("a\0b")),
            (String::from("X-Tab"), String::from("a\tb")),
        ];
        let head = String::from_utf8(request("GET", &url, &headers, None)).unwrap();
        assert!(head.contains("\r\nX-Tab: a\tb\r\n"));
        assert!(!head.contains("X Bad") && !head.contains("X-Bad") && !head.contains("X-Nul"));
    }

    #[test]
    fn redirects() {
        let base = "http://h.org/a/b?x";
        assert_eq!(redirect(base, "https://o.org/").unwrap(), "https://o.org/");
        assert_eq!(redirect(base, "/c").unwrap(), "http://h.org/c");
        assert_eq!(redirect(base, "c?y").unwrap(), "http://h.org/a/c?y");
        assert_eq!(redirect(base, "//o.org/d").unwrap(), "http://o.org/d");
        assert_eq!(redirect(base, "?y").unwrap(), "http://h.org/a/b?y");
        assert_eq!(redirect(base, "#top").unwrap(), "http://h.org/a/b?x");
        assert_eq!(redirect(base, "../c/./d").unwrap(), "http://h.org/c/d");
        assert_eq!(redirect(base, "/x/../../y").unwrap(), "http://h.org/y");
        assert_eq!(redirect(base, "..").unwrap(), "http://h.org/");
        assert_eq!(
            redirect("http://[::1]:8080/a", "b").unwrap(),
            "http://[::1]:8080/b"
        );
        // Other schemes, and line breaks that would start another header.
        assert_eq!(redirect(base, "ftp://o.org/"), None);
        assert_eq!(redirect(base, "javascript:alert(1)"), None);
        assert_eq!(redirect(base, "/c\nX-Injected: 1"), None);
        assert_eq!(redirect(base, "https://o.org/\rx"), None);
    }

    fn headers(names: &[&str]) -> Vec<(String, String)> {
        names
            .iter()
            .map(|name| (String::from(*name), String::from("v")))
            .collect()
    }

    fn names(headers: &[(String, String)]) -> Vec<&str> {
        headers.iter().map(|(name, _)| name.as_str()).collect()
    }

    #[test]
    fn redirects_keep_credentials_on_the_same_server_only() {
        let sent = headers(&["Authorization", "x-auth-key", "Cookie", "Accept", "Range"]);
        // The same server: everything goes along.
        let same = follow("https://h.org/a", 302, "/b", "GET", false).unwrap();
        assert_eq!(
            (same.url.as_str(), same.to_get, same.elsewhere),
            ("https://h.org/b", false, false)
        );
        let mut kept = sent.clone();
        same.keep(&mut kept);
        assert_eq!(kept, sent);
        // Another host, port or scheme: only harmless headers.
        for (from, to) in [
            ("https://h.org/a", "https://evil.org/"),
            ("https://h.org/a", "https://h.org:8443/"),
            ("http://h.org/a", "http://evil.org/"),
            ("http://h.org:8080/a", "https://h.org/"),
            ("http://h.org/a", "https://h.org:8443/"),
        ] {
            let redirect = follow(from, 302, to, "GET", false).unwrap();
            assert!(redirect.elsewhere, "{from} -> {to}");
            let mut kept = sent.clone();
            redirect.keep(&mut kept);
            assert_eq!(names(&kept), ["Accept", "Range"], "{from} -> {to}");
        }
        // Up to TLS on the same host: the same server.
        let upgrade = follow("http://H.org/a", 301, "https://h.org/a", "GET", false).unwrap();
        assert!(!upgrade.elsewhere);
    }

    #[test]
    fn redirects_never_lead_down_to_plain_http() {
        for to in ["http://h.org/", "HTTP://evil.org/x"] {
            let followed = follow("https://h.org/a", 302, to, "GET", false);
            assert_eq!(followed, Err(Refusal::Insecure), "{to}");
        }
        // A scheme-relative one stays on TLS.
        let same_scheme = follow("https://h.org/a", 302, "//o.org/x", "GET", false).unwrap();
        assert_eq!(same_scheme.url, "https://o.org/x");
        assert_eq!(
            follow("https://h.org/", 302, "ftp://h.org/", "GET", false),
            Err(Refusal::NotAnAddress)
        );
    }

    #[test]
    fn redirects_change_the_method_or_keep_the_body_home() {
        let post = |status, to| follow("https://h.org/a", status, to, "POST", true);
        for status in [301, 302, 303] {
            let redirect = post(status, "https://o.org/").unwrap();
            assert!(redirect.to_get && redirect.elsewhere, "{status}");
        }
        for status in [307, 308] {
            assert!(!post(status, "/b").unwrap().to_get);
            assert_eq!(post(status, "https://o.org/"), Err(Refusal::BodyElsewhere));
        }
        let head = follow("https://h.org/", 303, "/b", "HEAD", false).unwrap();
        assert!(!head.to_get);
        let put = follow("https://h.org/", 302, "https://o.org/", "PUT", true);
        assert_eq!(put, Err(Refusal::BodyElsewhere));
    }

    #[test]
    fn bodies_by_length_in_any_pieces() {
        let response = b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nETag: \"e\"\r\n\
                         Content-Length: 5\r\n\r\nhelloEXTRA";
        for step in [1, 2, 7, 100] {
            let (head, body, done) = run("GET", response, step, false);
            assert_eq!(head.status, 200);
            assert_eq!(head.content_type.as_deref(), Some("text/plain"));
            assert_eq!(head.etag.as_deref(), Some("\"e\""));
            assert_eq!(body, b"hello");
            assert!(done);
        }
    }

    #[test]
    fn chunked_bodies() {
        let response = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: gzip, chunked\r\n\r\n\
                         4;ext=1\r\nWiki\r\n5\r\npedia\r\n0\r\nTrailer: x\r\n\r\n";
        for step in [1, 3, 11, 1000] {
            let (head, body, done) = run("GET", response, step, false);
            assert!(head.chunked);
            assert_eq!(body, b"Wikipedia");
            assert!(done);
        }
    }

    #[test]
    fn bodies_to_the_end_and_cut_ones() {
        let (_, body, ok) = run("GET", b"HTTP/1.0 200 OK\r\n\r\nall of it", 4, true);
        assert_eq!(body, b"all of it");
        assert!(ok);
        let (_, _, ok) = run(
            "GET",
            b"HTTP/1.1 200 OK\r\nContent-Length: 9\r\n\r\npart",
            4,
            true,
        );
        assert!(!ok);
    }

    #[test]
    fn no_bodies() {
        let (head, body, done) = run(
            "HEAD",
            b"HTTP/1.1 200 OK\r\nContent-Length: 9\r\n\r\n",
            5,
            false,
        );
        assert_eq!((head.status, body.len(), done), (200, 0, true));
        let (head, _, done) = run("GET", b"HTTP/1.1 304 Not Modified\r\n\r\n", 5, false);
        assert_eq!((head.status, done), (304, true));
        let (head, body, done) = run(
            "GET",
            b"HTTP/1.1 100 Continue\r\n\r\nHTTP/1.1 302 Found\r\nLocation: /x\r\nContent-Length: 0\r\n\r\n",
            3,
            false,
        );
        assert!(head.is_redirect());
        assert_eq!(
            (head.location.as_deref(), body.len(), done),
            (Some("/x"), 0, true)
        );
    }

    #[test]
    fn wanted_headers_are_kept_in_lower_case() {
        let mut parser = Parser::new("GET").wanting(&[String::from("Last-Modified")]);
        let answer: &[u8] =
            b"HTTP/1.1 200 OK\r\nlast-modified: Tue\r\nX-Other: 1\r\nContent-Length: 0\r\n\r\n";
        parser.feed(answer, &mut |_| {}).unwrap();
        let head = parser.head().unwrap();
        assert_eq!(
            head.headers,
            [(String::from("last-modified"), String::from("Tue"))]
        );
    }

    #[test]
    fn malformed_heads() {
        let mut parser = Parser::new("GET");
        assert_eq!(
            parser.feed(b"SSH-2.0 hello\r\n\r\n", &mut |_| {}),
            Err(Error::Malformed)
        );
        let mut parser = Parser::new("GET");
        let long = alloc::vec![b'a'; MAX_HEAD + 10];
        assert_eq!(parser.feed(&long, &mut |_| {}), Err(Error::Malformed));
    }
}
