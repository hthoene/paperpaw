//! Web addresses and HTTP helpers shared by plugins that fetch things:
//! resolving links, file names for downloads, query encoding and `Basic`
//! authorisation. HTTP dates are [`UnixTime`](crate::time::UnixTime)'s.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::document::is_external;

/// What PaperPaw calls itself to servers, with the version of the SDK
/// this is built with: the `User-Agent` header's value, e.g.
/// `PaperPaw/0.6.0-beta.2 (e-reader)`. A plugin sends [`user_agent_of`], with
/// the version of the PaperPaw it runs in.
pub const USER_AGENT: &str = concat!("PaperPaw/", env!("CARGO_PKG_VERSION"), " (e-reader)");

/// The `User-Agent` header of [`USER_AGENT`], the SDK this is built with;
/// [`user_agent_of`] says the PaperPaw that runs.
pub fn user_agent() -> (String, String) {
    (String::from("User-Agent"), String::from(USER_AGENT))
}

/// The `User-Agent` header, to push onto a request's headers: with the
/// version of the PaperPaw that runs ([`Plugins::kernel`](crate::Plugins::kernel)),
/// not that of the SDK the plugin was built with ([`USER_AGENT`] where
/// the kernel is not listed).
pub fn user_agent_of(env: &crate::Env<'_>) -> (String, String) {
    let value = match env.plugins().kernel() {
        Some(kernel) => format!("PaperPaw/{} (e-reader)", kernel.manifest.version),
        None => String::from(USER_AGENT),
    };
    (String::from("User-Agent"), value)
}

/// A server's address as entered in a setting, as a base URL to put paths
/// after: spaces around and `/` at the end left out, `https://` put first
/// unless it names a scheme (`http://`, `webdavs://`); empty if nothing
/// is entered.
pub fn base_url(text: &str) -> String {
    let text = text.trim().trim_end_matches('/');
    let scheme = text.split_once("://").is_some_and(|(scheme, _)| {
        scheme.starts_with(|c: char| c.is_ascii_alphabetic())
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    });
    if text.is_empty() || scheme {
        text.to_string()
    } else {
        format!("https://{text}")
    }
}

/// `href` made absolute with `base`, a page's address: as browsers
/// resolve links (`/a`, `../b`, `?q`, `//host/c`).
pub fn join_url(base: &str, href: &str) -> String {
    if is_external(href) {
        return href.to_string();
    }
    let Some((scheme, rest)) = base.split_once("://") else {
        return href.to_string();
    };
    if let Some(net) = href.strip_prefix("//") {
        return format!("{scheme}://{net}");
    }
    let host_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let host = &rest[..host_end];
    let after = &rest[host_end..];
    let path = after.split(['?', '#']).next().unwrap_or("");
    let path = if path.is_empty() { "/" } else { path };
    let joined = if href.starts_with('/') {
        href.to_string()
    } else if href.starts_with('?') {
        format!("{path}{href}")
    } else if href.starts_with('#') {
        let without = after.split('#').next().unwrap_or("");
        format!("{}{href}", if without.is_empty() { "/" } else { without })
    } else {
        let dir = &path[..path.rfind('/').map_or(0, |i| i + 1)];
        format!("{dir}{href}")
    };
    format!("{scheme}://{host}{}", normalize(&joined))
}

/// Resolves `.` and `..` in the path of `target`, keeping its query and
/// fragment.
fn normalize(target: &str) -> String {
    let split = target.find(['?', '#']).unwrap_or(target.len());
    let (path, tail) = target.split_at(split);
    let mut parts: Vec<&str> = Vec::new();
    let segments: Vec<&str> = path.split('/').collect();
    for (i, segment) in segments.iter().enumerate() {
        let last = i + 1 == segments.len();
        match *segment {
            "." => {
                if last {
                    parts.push("");
                }
            }
            ".." => {
                if parts.len() > 1 {
                    parts.pop();
                }
                if last {
                    parts.push("");
                }
            }
            segment => parts.push(segment),
        }
    }
    let mut out = parts.join("/");
    if !out.starts_with('/') {
        out.insert(0, '/');
    }
    out.push_str(tail);
    out
}

/// `text` for a query: letters, digits and `-_.~` as they are, spaces as
/// `+`, everything else percent-encoded.
pub fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        match byte {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(char::from(byte));
            }
            b' ' => out.push('+'),
            byte => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// `text` with `%XX` sequences decoded (invalid UTF-8 replaced).
pub fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let hex = |b: u8| char::from(b).to_digit(16).map(|d| d as u8);
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let (Some(high), Some(low)) = (
                bytes.get(i + 1).copied().and_then(hex),
                bytes.get(i + 2).copied().and_then(hex),
            )
        {
            out.push(high << 4 | low);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The host of an address, for messages: `de.wikipedia.org` (with its
/// port, if it names one; without a user and password before it).
pub fn host(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..end];
    authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host)
}

/// A file name for what `url` points at: its last path part, decoded,
/// without characters file systems refuse; `download` if it has none.
pub fn file_name(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let path = rest.split(['?', '#']).next().unwrap_or("");
    let last = path.split_once('/').map_or("", |(_, path)| path);
    let last = last.rsplit('/').find(|part| !part.is_empty()).unwrap_or("");
    clean_name(&percent_decode(last), 200).unwrap_or_else(|| String::from("download"))
}

/// `name` as a file name every storage takes: without characters file
/// systems refuse, not hidden, no dot or space at the end, not a name
/// Windows keeps for a device (`CON.txt` becomes `_CON.txt`), at most
/// `max` UTF-16 units as FAT counts them (255 at most there); `None` if
/// nothing is left.
pub fn clean_name(name: &str, max: usize) -> Option<String> {
    let name: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => ' ',
            c => c,
        })
        .filter(|c| !c.is_control())
        .collect();
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut name = String::from(name.trim_start_matches(|c: char| c == '.' || c.is_whitespace()));
    if paperpaw_platform::storage::is_reserved_name(&name) {
        name.insert(0, '_');
    }
    let mut units = 0;
    let end = name
        .char_indices()
        .find(|(_, c)| {
            units += c.len_utf16();
            units > max
        })
        .map_or(name.len(), |(at, _)| at);
    name.truncate(end);
    let name = name.trim_end_matches(|c: char| c == '.' || c.is_whitespace());
    (!name.is_empty()).then(|| name.to_string())
}

/// The `Authorization` header's value for `user` and `password`.
pub fn basic_auth(user: &str, password: &str) -> String {
    format!("Basic {}", base64(format!("{user}:{password}").as_bytes()))
}

/// The seconds since 1970 (UTC) of an HTTP date
/// ([`UnixTime::parse_http_date`](crate::time::UnixTime)).
pub(crate) fn http_date_seconds(text: &str) -> Option<i64> {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let (_, rest) = text.trim().split_once(", ")?;
    let mut parts = rest.split_ascii_whitespace();
    let day = parts.next()?.parse().ok()?;
    let month = parts.next()?;
    let month = MONTHS.iter().position(|m| *m == month)? as u8 + 1;
    let year = parts.next()?.parse().ok()?;
    let mut clock = parts.next()?.splitn(3, ':').map(str::parse::<u8>);
    let (hour, minute, second) = (
        clock.next()?.ok()?,
        clock.next()?.ok()?,
        clock.next()?.ok()?,
    );
    if parts.next()? != "GMT" || hour > 23 || minute > 59 || second > 60 || day > 31 {
        return None;
    }
    let time = crate::time::LocalTime::new(year, month, day, hour, minute, second);
    Some(time.to_seconds())
}

/// Standard base64 with padding.
pub fn base64(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn servers_as_base_urls() {
        assert_eq!(base_url(" sync.example.org/ "), "https://sync.example.org");
        assert_eq!(base_url("http://nas.local:8080//"), "http://nas.local:8080");
        assert_eq!(base_url("webdavs://nas/dav/"), "webdavs://nas/dav");
        assert_eq!(base_url("nas/x?to=http://y"), "https://nas/x?to=http://y");
        assert_eq!(base_url("  "), "");
        assert!(USER_AGENT.starts_with(concat!("PaperPaw/", env!("CARGO_PKG_VERSION"))));
    }

    #[test]
    fn http_dates() {
        assert_eq!(
            http_date_seconds("Tue, 20 Oct 2026 08:00:00 GMT"),
            Some(1_792_483_200)
        );
        assert_eq!(http_date_seconds("Thu, 01 Jan 1970 00:00:00 GMT"), Some(0));
        for wrong in [
            "",
            "20 Oct 2026 08:00:00 GMT",
            "Tue, 20 Okt 2026 08:00:00 GMT",
            "Tue, 20 Oct 2026 08:00:00 CET",
            "Tue, 20 Oct 2026 25:00:00 GMT",
            "Tuesday, 20-Oct-26 08:00:00 GMT",
        ] {
            assert_eq!(http_date_seconds(wrong), None, "{wrong}");
        }
    }

    #[test]
    fn links_resolve_as_in_browsers() {
        let base = "https://example.org/blog/2024/post.html?x=1#top";
        let cases = [
            ("other.html", "https://example.org/blog/2024/other.html"),
            ("../index.html", "https://example.org/blog/index.html"),
            ("../../../../a", "https://example.org/a"),
            ("/about", "https://example.org/about"),
            ("?page=2", "https://example.org/blog/2024/post.html?page=2"),
            ("#c3", "https://example.org/blog/2024/post.html?x=1#c3"),
            ("//cdn.example.net/f.epub", "https://cdn.example.net/f.epub"),
            ("mailto:a@b.c", "mailto:a@b.c"),
            ("./", "https://example.org/blog/2024/"),
        ];
        for (href, expected) in cases {
            assert_eq!(join_url(base, href), expected, "{href}");
        }
        assert_eq!(
            join_url("https://example.org", "a"),
            "https://example.org/a"
        );
    }

    #[test]
    fn names_and_hosts() {
        assert_eq!(
            file_name("https://a.org/books/Die%20Verwandlung.epub?x=1"),
            "Die Verwandlung.epub"
        );
        assert_eq!(file_name("https://a.org/"), "download");
        assert_eq!(file_name("https://a.org"), "download");
        assert_eq!(file_name("https://a.org/f/..%2F.hidden"), "hidden");
        assert_eq!(
            clean_name(" Kafka: Die  Verwandlung? ", 100).as_deref(),
            Some("Kafka Die Verwandlung")
        );
        assert_eq!(clean_name("...", 100), None);
        // Names FAT and Windows take for another, or for a device.
        assert_eq!(clean_name("Chapter 1.", 100).as_deref(), Some("Chapter 1"));
        assert_eq!(clean_name("con.txt", 100).as_deref(), Some("_con.txt"));
        assert_eq!(clean_name("abcdef. x", 6).as_deref(), Some("abcdef"));
        // Counted in UTF-16 units: a character beyond the BMP takes two.
        let emoji = "\u{1f4da}".repeat(3);
        assert_eq!(clean_name(&emoji, 5).as_deref(), Some("\u{1f4da}\u{1f4da}"));
        assert_eq!(host("https://de.wikipedia.org/wiki/X"), "de.wikipedia.org");
        assert_eq!(
            host("https://me:secret@dav.example.org:8443/x"),
            "dav.example.org:8443"
        );
        assert_eq!(encode("Größe a"), "Gr%C3%B6%C3%9Fe+a");
    }

    #[test]
    fn credentials() {
        assert_eq!(base64(b"user:pass"), "dXNlcjpwYXNz");
        assert_eq!(base64(b"a"), "YQ==");
        assert_eq!(base64(b"ab"), "YWI=");
        assert_eq!(
            basic_auth("Aladdin", "open sesame"),
            "Basic QWxhZGRpbjpvcGVuIHNlc2FtZQ=="
        );
    }
}
