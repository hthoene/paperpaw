//! Input rules of `core.net` (`docs/abi/network.md` 2.1, 2.2, 5.1, 5.3): the
//! records of `request` and `respond` in their order, and every check of
//! their fields that needs no device state. Nothing here allocates: the
//! records borrow the call's batch.
use super::super::registrations::{self as wire, Record};
use paperpaw_native::host_glue::{ERR_DENIED, ERR_INVALID, ERR_NO_SPACE, ERR_UNSUPPORTED};

/// The most headers on a `request` / on a `respond`.
pub(super) const REQUEST_HEADERS: usize = 16;
pub(super) const RESPONSE_HEADERS: usize = 8;
/// `net_file.role`: a file sent as the body / the answer saved to a file.
pub(super) const ROLE_SEND: u32 = 0;
pub(super) const ROLE_SAVE: u32 = 1;
/// `net_request.flags` bit 0: send `event.http_progress`.
pub(super) const FLAG_PROGRESS: u32 = 1;
/// The largest file a request saves (2.1).
pub(super) const MAX_FILE: u64 = 64 * 1024 * 1024;
/// A server's default and largest request body (5.1).
pub(super) const SERVE_BODY_DEFAULT: u64 = 4 * 1024 * 1024;
pub(super) const SERVE_BODY_MAX: u64 = 64 * 1024 * 1024;
/// Shortest and longest `timeout_ms` (2.1).
const TIMEOUT_MIN: u32 = 1_000;
const TIMEOUT_MAX: u32 = 600_000;

/// A `request` input, borrowed from its batch.
pub(crate) struct Request<'a> {
    pub(crate) head: wire::ServiceNetRequest<'a>,
    pub(crate) headers: [Option<wire::ServiceNetHeader<'a>>; REQUEST_HEADERS],
    pub(crate) header_count: usize,
    pub(crate) send: Option<wire::ServiceNetFile<'a>>,
    pub(crate) save: Option<wire::ServiceNetFile<'a>>,
    pub(crate) body: Option<&'a [u8]>,
}

impl<'a> Request<'a> {
    /// The headers the plugin sent, in order.
    pub(crate) fn headers(&self) -> impl Iterator<Item = &wire::ServiceNetHeader<'a>> {
        self.headers[..self.header_count].iter().flatten()
    }
}

/// `request`: `net_request`, then `net_header` (at most 16), then at most
/// one `net_file` per role, then at most one `net_body`; any other order, a
/// repeated role or a second request is `ERR_INVALID`, and so is a file to
/// send together with a body.
///
/// # Errors
/// `ERR_INVALID` for the shape and every field rule; `ERR_UNSUPPORTED` for an
/// unknown flag bit. Statuses of the codec for a malformed batch.
pub(crate) fn parse_request(input: &[u8]) -> Result<Request<'_>, i32> {
    let mut head = None;
    let mut out = Request {
        head: wire::ServiceNetRequest {
            token: 0,
            method: "",
            url: "",
            flags: 0,
            timeout_ms: 0,
            max_body: 0,
            max_file: 0,
        },
        headers: [None; REQUEST_HEADERS],
        header_count: 0,
        send: None,
        save: None,
        body: None,
    };
    // 0 request, 1 headers, 2 files, 3 body: only forward.
    let mut stage = 0_u8;
    wire::host_records_visit(input, 20, &mut |record| {
        match (record, stage) {
            (Record::ServiceNetRequest(r), 0) if head.is_none() => {
                head = Some(r);
                stage = 1;
            }
            (Record::ServiceNetHeader(h), 1) if out.header_count < REQUEST_HEADERS => {
                header_ok(&h, &[])?;
                out.headers[out.header_count] = Some(h);
                out.header_count += 1;
            }
            (Record::ServiceNetFile(f), 1 | 2) => {
                stage = 2;
                let slot = match f.role {
                    ROLE_SEND => &mut out.send,
                    ROLE_SAVE => &mut out.save,
                    _ => return Err(ERR_INVALID),
                };
                if slot.replace(f).is_some() {
                    return Err(ERR_INVALID);
                }
            }
            (Record::ServiceNetBody(b), 1..=2) => {
                stage = 3;
                out.body = Some(b.data);
            }
            _ => return Err(ERR_INVALID),
        }
        Ok(())
    })?;
    out.head = head.ok_or(ERR_INVALID)?;
    if out.send.is_some() && out.body.is_some() {
        return Err(ERR_INVALID);
    }
    check_head(&out.head)?;
    if let Some(file) = &out.send {
        path_ok(file.path, false)?;
    }
    if let Some(file) = &out.save {
        path_ok(file.path, true)?;
    }
    Ok(out)
}

/// The fields of `net_request` that need no device: method, URL, flags and
/// `timeout_ms`. The caps `max_body` / `max_file` are the device's (profile).
fn check_head(head: &wire::ServiceNetRequest<'_>) -> Result<(), i32> {
    if head.flags & !FLAG_PROGRESS != 0 {
        return Err(ERR_UNSUPPORTED);
    }
    if !is_token(head.method, 16) || !url_ok(head.url) {
        return Err(ERR_INVALID);
    }
    if head.timeout_ms != 0 && !(TIMEOUT_MIN..=TIMEOUT_MAX).contains(&head.timeout_ms) {
        return Err(ERR_INVALID);
    }
    if head.max_file > MAX_FILE {
        return Err(ERR_NO_SPACE);
    }
    Ok(())
}

/// A `net_file` path: the storage grammar, and for a saved file not a
/// `.part` name (the host's own temporary, in any case).
fn path_ok(path: &str, save: bool) -> Result<(), i32> {
    if !paperpaw_platform::storage::is_valid_path(path) {
        return Err(ERR_INVALID);
    }
    if save && path.len() >= 5 && path.as_bytes()[path.len() - 5..].eq_ignore_ascii_case(b".part") {
        return Err(ERR_INVALID);
    }
    Ok(())
}

/// The root of a file: 0, 1 or 2; a saved file never on root 2.
///
/// # Errors
/// `ERR_INVALID` for another root, `ERR_DENIED` for a save on root 2.
pub(super) fn root_ok(file: &wire::ServiceNetFile<'_>) -> Result<(), i32> {
    match (file.root, file.role) {
        (0 | 1, _) | (2, ROLE_SEND) => Ok(()),
        (2, _) => Err(ERR_DENIED),
        _ => Err(ERR_INVALID),
    }
}

/// An HTTP token (RFC 9110): 1 to `max` bytes of letters, digits and
/// ``!#$%&'*+-.^_`|~``.
pub(super) fn is_token(text: &str, max: usize) -> bool {
    !text.is_empty()
        && text.len() <= max
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
}

/// A header: a token name, a value without control bytes, and none of the
/// `refused` names (compared without case) the host sends itself.
pub(super) fn header_ok(
    header: &wire::ServiceNetHeader<'_>,
    extra_refused: &[&str],
) -> Result<(), i32> {
    const OWN: [&str; 3] = ["host", "connection", "content-length"];
    if !is_token(header.name, 64)
        || header.value.bytes().any(|b| b < 0x20 || b == 0x7f)
        || OWN
            .iter()
            .chain(extra_refused)
            .any(|own| header.name.eq_ignore_ascii_case(own))
    {
        return Err(ERR_INVALID);
    }
    Ok(())
}

/// `http://` or `https://` (any case), an authority of a bracketed IPv6 or a
/// name of letters, digits, `-`, `.`, `_`, `~` (an IPv4 is one), a port 1-65535
/// if given; `user:password@` is skipped; no control bytes anywhere. The
/// fragment is dropped by the host, so `#...` is accepted.
pub(super) fn url_ok(url: &str) -> bool {
    if url.is_empty() || url.len() > 1024 || url.bytes().any(|b| b < 0x20 || b == 0x7f) {
        return false;
    }
    let Some(rest) = strip_scheme(url) else {
        return false;
    };
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..end];
    // Credentials end at the last '@'.
    let host_port = authority
        .rsplit_once('@')
        .map_or(authority, |(_, after)| after);
    let (host_ok, port) = if let Some(inner) = host_port.strip_prefix('[') {
        let Some((v6, after)) = inner.split_once(']') else {
            return false;
        };
        let v6_ok = v6.parse::<core::net::Ipv6Addr>().is_ok();
        let port = if after.is_empty() {
            None
        } else if let Some(port) = after.strip_prefix(':') {
            Some(port)
        } else {
            return false;
        };
        (v6_ok, port)
    } else {
        let (host, port) = match host_port.split_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (host_port, None),
        };
        let name_ok = !host.is_empty()
            && host
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-._~".contains(&b));
        (name_ok, port)
    };
    host_ok && port.is_none_or(port_ok)
}

/// A port of 1 to 5 digits, 1-65535.
fn port_ok(port: &str) -> bool {
    port.len() <= 5
        && !port.is_empty()
        && port.bytes().all(|b| b.is_ascii_digit())
        && port.parse::<u32>().is_ok_and(|p| (1..=65_535).contains(&p))
}

fn strip_scheme(url: &str) -> Option<&str> {
    let (scheme, rest) = url.split_once("://")?;
    (scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")).then_some(rest)
}

/// A `respond` input, borrowed from its batch.
pub(crate) struct Response<'a> {
    pub(crate) head: wire::ServiceNetResponse<'a>,
    pub(crate) headers: [Option<wire::ServiceNetHeader<'a>>; RESPONSE_HEADERS],
    pub(crate) header_count: usize,
    pub(crate) send: Option<wire::ServiceNetFile<'a>>,
    pub(crate) body: Option<&'a [u8]>,
}

impl<'a> Response<'a> {
    pub(crate) fn headers(&self) -> impl Iterator<Item = &wire::ServiceNetHeader<'a>> {
        self.headers[..self.header_count].iter().flatten()
    }
}

/// `respond`: `net_response`, at most 8 `net_header`, then at most one of a
/// `net_file` (role `send`) and a `net_body`.
///
/// # Errors
/// `ERR_INVALID` for the shape, a status outside 100-599 and every header
/// rule.
pub(crate) fn parse_response(input: &[u8]) -> Result<Response<'_>, i32> {
    let mut head = None;
    let mut out = Response {
        head: wire::ServiceNetResponse {
            request: 0,
            status: 0,
            content_type: "",
        },
        headers: [None; RESPONSE_HEADERS],
        header_count: 0,
        send: None,
        body: None,
    };
    let mut stage = 0_u8;
    wire::host_records_visit(input, 11, &mut |record| {
        match (record, stage) {
            (Record::ServiceNetResponse(r), 0) if head.is_none() => {
                head = Some(r);
                stage = 1;
            }
            (Record::ServiceNetHeader(h), 1) if out.header_count < RESPONSE_HEADERS => {
                header_ok(&h, &["content-type"])?;
                out.headers[out.header_count] = Some(h);
                out.header_count += 1;
            }
            (Record::ServiceNetFile(f), 1) if f.role == ROLE_SEND => {
                stage = 2;
                out.send = Some(f);
            }
            (Record::ServiceNetBody(b), 1) => {
                stage = 3;
                out.body = Some(b.data);
            }
            _ => return Err(ERR_INVALID),
        }
        Ok(())
    })?;
    out.head = head.ok_or(ERR_INVALID)?;
    if !(100..=599).contains(&out.head.status) {
        return Err(ERR_INVALID);
    }
    if let Some(file) = &out.send {
        path_ok(file.path, false)?;
        root_ok(file)?;
    }
    Ok(out)
}
