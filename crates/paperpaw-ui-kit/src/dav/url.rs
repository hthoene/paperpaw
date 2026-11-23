//! Addresses of a WebDAV server: its parts, a listing's `href` made whole, and
//! the folder address a setting holds.
use alloc::string::String;

use paperpaw_sdk::native::Error;

use crate::text::{concat, copy};

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

/// The scheme, host and port of `url`, to compare: `None` where any of them
/// is missing or in doubt, such as an address with a user name
/// (`https://me@host/`). The port is filled in for `http` and `https`.
fn server(url: &str) -> Option<(&str, &str, u16)> {
    let (scheme, authority) = origin(url).split_once("://")?;
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
/// port (letters in any case). `https` and `http` are never the same.
#[must_use]
pub fn same_origin(a: &str, b: &str) -> bool {
    match (server(a), server(b)) {
        (Some((s1, h1, p1)), Some((s2, h2, p2))) => {
            s1.eq_ignore_ascii_case(s2) && h1.eq_ignore_ascii_case(h2) && p1 == p2
        }
        _ => false,
    }
}

/// The address a listing's `href` means for the folder at `base`: a whole
/// address on the folder's server as it is, a path on the folder's server,
/// else beside the folder.
///
/// # Errors
/// [`Error::NoMemory`]; [`Error::Denied`] for a whole address on another
/// server (or without the folder's `https`): the account's password is not
/// sent there.
pub fn resolve(base: &str, href: &str) -> Result<String, Error> {
    if href.contains("://") {
        if !same_origin(base, href) {
            return Err(Error::Denied);
        }
        return copy(href);
    }
    let origin = origin(base);
    if href.starts_with('/') {
        return concat(&[origin, href]);
    }
    let folder = path(base);
    concat(&[origin, folder.trim_end_matches('/'), "/", href])
}

/// `name` as one part of a path: letters, digits and `-_.~` as they are,
/// everything else percent-encoded (a space is `%20`).
///
/// # Errors
/// [`Error::NoMemory`].
pub fn segment(name: &str) -> Result<String, Error> {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::new();
    out.try_reserve(name.len() * 3)
        .map_err(|_| Error::NoMemory)?;
    for byte in name.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.~".contains(&byte) {
            out.push(char::from(byte));
        } else {
            out.push('%');
            out.push(char::from(HEX[usize::from(byte >> 4)]));
            out.push(char::from(HEX[usize::from(byte & 15)]));
        }
    }
    Ok(out)
}

/// The text of an address with a `/` at its end (a folder's), `None` for no
/// address or one that does not start with `http://` or `https://`.
#[must_use]
pub fn folder_url(setting: &str) -> Option<String> {
    let url = setting.trim();
    let lower_ok = ["http://", "https://"].iter().any(|scheme| {
        url.len() > scheme.len()
            && url
                .get(..scheme.len())
                .is_some_and(|s| s.eq_ignore_ascii_case(scheme))
    });
    if !lower_ok || url.len() > 800 {
        return None;
    }
    let mut out = String::new();
    out.try_reserve_exact(url.len() + 1).ok()?;
    out.push_str(url);
    if !out.ends_with('/') {
        out.push('/');
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "test assertions")]
    use super::*;

    const BASE: &str = "https://cloud.example.org/remote.php/dav/files/me/Photos/";

    #[test]
    fn the_parts_of_an_address_are_told_apart() {
        assert_eq!(origin(BASE), "https://cloud.example.org");
        assert_eq!(origin("http://10.0.0.2:8080/dav/"), "http://10.0.0.2:8080");
        assert_eq!(origin("nonsense"), "");
        assert_eq!(path(BASE), "/remote.php/dav/files/me/Photos/");
        assert_eq!(path("http://host"), "/");
        assert_eq!(path("http://host/a?b=c#d"), "/a");
        assert_eq!(
            resolve(BASE, "/remote.php/dav/files/me/Photos/a.jpg").unwrap(),
            "https://cloud.example.org/remote.php/dav/files/me/Photos/a.jpg"
        );
        assert_eq!(
            resolve(BASE, "b.jpg").unwrap(),
            "https://cloud.example.org/remote.php/dav/files/me/Photos/b.jpg"
        );
        assert_eq!(
            resolve(BASE, "https://Cloud.example.org:443/d.jpg").unwrap(),
            "https://Cloud.example.org:443/d.jpg"
        );
    }

    #[test]
    fn an_href_on_another_server_or_without_https_is_refused() {
        for href in [
            "http://other/c.jpg",
            "http://cloud.example.org/c.jpg",
            "https://cloud.example.org:8443/c.jpg",
            "https://me@cloud.example.org/c.jpg",
            "https://cloud.example.org.evil/c.jpg",
        ] {
            assert_eq!(resolve(BASE, href), Err(Error::Denied), "{href}");
        }
    }

    #[test]
    fn a_name_is_one_part_of_a_path() {
        assert_eq!(segment("a b/ü.md").unwrap(), "a%20b%2F%C3%BC.md");
        assert_eq!(segment("Note-1_x~.md").unwrap(), "Note-1_x~.md");
    }

    #[test]
    fn a_folder_address_ends_in_a_slash_and_is_http_or_https() {
        assert_eq!(folder_url(" https://h/dav ").unwrap(), "https://h/dav/");
        assert_eq!(folder_url("HTTP://h/").unwrap(), "HTTP://h/");
        assert!(folder_url("ftp://h/").is_none());
        assert!(folder_url("https://").is_none());
        assert!(folder_url("").is_none());
    }
}
