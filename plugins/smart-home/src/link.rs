//! How the plugin reaches the server: which addresses it accepts (the home
//! network unless the reader allows more, plain HTTP only there), the
//! addresses of Home Assistant's REST calls, the body of a service call, and
//! what a failed request is called in plain words ([`Problem`]).
use alloc::string::String;
use paperpaw_sdk::native::{Error, net::Reason};

/// The longest base address kept.
pub const BASE_MAX: usize = 200;

/// Why an address was not taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Refused {
    /// Nothing is typed.
    Empty,
    /// Not an address (a space, a user name, a query, a missing name).
    Malformed,
    /// A server outside the home network, which is not allowed (a setting).
    Outside,
    /// Plain `http://` to a server outside the home network: never.
    Insecure,
}

/// What a request that did not give an answer is, in plain words (catalog keys).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Problem {
    NoServer,
    NoToken,
    Address(Refused),
    NoNetwork,
    Offline,
    Off,
    Unreachable,
    NotFound,
    Tls,
    Denied,
    Status(u32),
    TooBig,
    Card,
    BadAnswer,
}

impl Problem {
    /// The catalog key of the words that say it.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::NoServer => "smart-home.problem.no-server",
            Self::NoToken => "smart-home.problem.no-token",
            Self::Address(Refused::Empty) => "smart-home.problem.no-server",
            Self::Address(Refused::Malformed) => "smart-home.problem.address",
            Self::Address(Refused::Outside) => "smart-home.problem.outside",
            Self::Address(Refused::Insecure) => "smart-home.problem.insecure",
            Self::NoNetwork => "smart-home.problem.no-network",
            Self::Offline => "smart-home.problem.offline",
            Self::Off => "smart-home.problem.off",
            Self::Unreachable => "smart-home.problem.unreachable",
            Self::NotFound => "smart-home.problem.not-found",
            Self::Tls => "smart-home.problem.tls",
            Self::Denied => "smart-home.problem.denied",
            Self::Status(_) => "smart-home.problem.status",
            Self::TooBig => "smart-home.problem.too-big",
            Self::Card => "smart-home.problem.card",
            Self::BadAnswer => "smart-home.problem.bad-answer",
        }
    }

    /// What a finished request was, if it failed: `None` for an answer with a
    /// 2xx status.
    #[must_use]
    pub const fn of(reason: Reason, status: u32) -> Option<Self> {
        match reason {
            Reason::Ok => match status {
                200..=299 => None,
                401 | 403 => Some(Self::Denied),
                404 => Some(Self::NotFound),
                other => Some(Self::Status(other)),
            },
            Reason::NoNetwork => Some(Self::NoNetwork),
            Reason::Offline | Reason::Saving => Some(Self::Offline),
            Reason::NetworkOff => Some(Self::Off),
            Reason::Tls => Some(Self::Tls),
            Reason::TooLarge => Some(Self::TooBig),
            Reason::Io | Reason::Full => Some(Self::Card),
            _ => Some(Self::Unreachable),
        }
    }
}

/// Whether `host` (without its port) is in the home network: a private,
/// loopback or link-local address, `localhost`, a name of one word, or one of
/// the endings a router or mDNS gives (`.local`, `.lan`, `.home.arpa`, `.fritz.box`).
#[must_use]
pub fn is_home(host: &str) -> bool {
    let lower = |suffix: &str| {
        host.len() > suffix.len()
            && host
                .get(host.len() - suffix.len()..)
                .is_some_and(|tail| tail.eq_ignore_ascii_case(suffix))
    };
    if let Some(inner) = host.strip_prefix('[').and_then(|h| h.strip_suffix(']')) {
        let inner = inner.to_ascii_lowercase();
        return inner == "::1"
            || inner.starts_with("fe8")
            || inner.starts_with("fe9")
            || inner.starts_with("fea")
            || inner.starts_with("feb")
            || inner.starts_with("fc")
            || inner.starts_with("fd");
    }
    let parts = host.split('.');
    if host.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
        let mut octets = [0u16; 4];
        let mut n = 0;
        for part in parts {
            let Some(slot) = octets.get_mut(n) else {
                return false;
            };
            let Ok(value) = part.parse::<u16>() else {
                return false;
            };
            if value > 255 || part.is_empty() {
                return false;
            }
            *slot = value;
            n += 1;
        }
        if n != 4 {
            return false;
        }
        let [a, b, ..] = octets;
        return a == 10
            || a == 127
            || (a == 172 && (16..=31).contains(&b))
            || (a == 192 && b == 168)
            || (a == 169 && b == 254);
    }
    !host.is_empty()
        && (!host.contains('.')
            || host.eq_ignore_ascii_case("localhost")
            || lower(".local")
            || lower(".lan")
            || lower(".home")
            || lower(".home.arpa")
            || lower(".fritz.box")
            || lower(".internal")
            || lower(".localdomain"))
}

/// The base address of the server as it is used: `scheme://host[:port][/path]`
/// without a closing slash. A missing scheme is `http://` (the usual home
/// address is `homeassistant.local:8123`); a trailing `/api` is dropped.
///
/// `anywhere` allows a server outside the home network, but only over `https://`.
///
/// # Errors
/// [`Refused`] says why the text is no address the plugin will use; `NoMemory`
/// as [`Error`] is folded into `Malformed` (a base is short).
pub fn base(text: &str, anywhere: bool) -> Result<String, Refused> {
    let text = text.trim();
    if text.is_empty() {
        return Err(Refused::Empty);
    }
    if text.len() > BASE_MAX
        || text
            .bytes()
            .any(|b| b <= b' ' || b == 0x7f || matches!(b, b'?' | b'#' | b'@' | b'\\'))
    {
        return Err(Refused::Malformed);
    }
    let (secure, rest) = match text.split_once("://") {
        Some((scheme, rest)) if scheme.eq_ignore_ascii_case("https") => (true, rest),
        Some((scheme, rest)) if scheme.eq_ignore_ascii_case("http") => (false, rest),
        Some(_) => return Err(Refused::Malformed),
        None => (false, text),
    };
    let rest = rest.trim_end_matches('/');
    let rest = rest.strip_suffix("/api").unwrap_or(rest);
    let (authority, path) = rest.split_once('/').map_or((rest, ""), |(a, p)| (a, p));
    let host = if authority.starts_with('[') {
        // An IPv6 literal: `[...]` and then nothing or a port.
        let Some((inner, tail)) = authority.split_once(']') else {
            return Err(Refused::Malformed);
        };
        if !(tail.is_empty() || tail.strip_prefix(':').is_some_and(port_ok)) {
            return Err(Refused::Malformed);
        }
        authority.get(..=inner.len()).ok_or(Refused::Malformed)?
    } else {
        let (host, port) = authority
            .split_once(':')
            .map_or((authority, None), |(h, p)| (h, Some(p)));
        if port.is_some_and(|p| !port_ok(p)) {
            return Err(Refused::Malformed);
        }
        host
    };
    let name_ok = !host.is_empty()
        && host.bytes().all(|b| {
            b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b'[' | b']' | b':')
        })
        && !host.starts_with('.')
        && !host.ends_with('.');
    if !name_ok || path.split('/').any(|part| part == "..") {
        return Err(Refused::Malformed);
    }
    if !is_home(host) {
        if !secure {
            return Err(Refused::Insecure);
        }
        if !anywhere {
            return Err(Refused::Outside);
        }
    }
    let mut out = String::new();
    out.try_reserve_exact(text.len() + 8)
        .map_err(|_| Refused::Malformed)?;
    out.push_str(if secure { "https://" } else { "http://" });
    out.push_str(authority);
    if !path.is_empty() {
        out.push('/');
        out.push_str(path);
    }
    Ok(out)
}

fn port_ok(port: &str) -> bool {
    !port.is_empty() && port.len() <= 5 && port.bytes().all(|b| b.is_ascii_digit())
}

/// `base` and `tail` as one address.
///
/// # Errors
/// `NoMemory`, `NoSpace` for an address longer than a request may have.
pub fn url(base: &str, tail: &[&str]) -> Result<String, Error> {
    let mut out = String::new();
    let len: usize = base.len() + tail.iter().map(|t| t.len()).sum::<usize>();
    if len > paperpaw_sdk::native::net::MAX_URL {
        return Err(Error::NoSpace);
    }
    out.try_reserve_exact(len).map_err(|_| Error::NoMemory)?;
    out.push_str(base);
    for part in tail {
        out.push_str(part);
    }
    Ok(out)
}

/// The address of all states.
///
/// # Errors
/// As [`url`].
pub fn states_url(base: &str) -> Result<String, Error> {
    url(base, &["/api/states"])
}

/// The address of one entity's state; `id` is a checked entity id.
///
/// # Errors
/// As [`url`].
pub fn state_url(base: &str, id: &str) -> Result<String, Error> {
    url(base, &["/api/states/", id])
}

/// The address of a service call.
///
/// # Errors
/// As [`url`].
pub fn service_url(base: &str, domain: &str, service: &str) -> Result<String, Error> {
    url(base, &["/api/services/", domain, "/", service])
}

/// The body of a service call for `id` (a checked entity id: no escaping).
///
/// # Errors
/// `NoMemory`.
pub fn service_body(id: &str) -> Result<String, Error> {
    let mut out = String::new();
    out.try_reserve_exact(id.len() + 16)
        .map_err(|_| Error::NoMemory)?;
    out.push_str("{\"entity_id\":\"");
    out.push_str(id);
    out.push_str("\"}");
    Ok(out)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "test assertions")]
    use super::*;

    #[test]
    fn home_addresses() {
        for host in [
            "192.168.1.20",
            "10.0.0.5",
            "172.16.0.1",
            "172.31.255.1",
            "127.0.0.1",
            "169.254.1.1",
            "homeassistant",
            "homeassistant.local",
            "ha.fritz.box",
            "localhost",
            "nas.lan",
            "[::1]",
            "[fd12:3456::1]",
            "[fe80::1]",
        ] {
            assert!(is_home(host), "{host}");
        }
        for host in [
            "8.8.8.8",
            "172.32.0.1",
            "172.15.0.1",
            "192.169.0.1",
            "ha.example.com",
            "myhome.duckdns.org",
            "1.2.3",
            "1.2.3.4.5",
            "999.1.1.1",
            "[2001:db8::1]",
            "",
        ] {
            assert!(!is_home(host), "{host}");
        }
    }

    #[test]
    fn the_base_address() {
        assert_eq!(
            base("homeassistant.local:8123", false).unwrap(),
            "http://homeassistant.local:8123"
        );
        assert_eq!(
            base("  http://192.168.1.2:8123/ ", false).unwrap(),
            "http://192.168.1.2:8123"
        );
        assert_eq!(
            base("http://192.168.1.2:8123/api/", false).unwrap(),
            "http://192.168.1.2:8123"
        );
        assert_eq!(
            base("https://ha.local/prefix", false).unwrap(),
            "https://ha.local/prefix"
        );
        assert_eq!(
            base("HTTP://[::1]:8123", false).unwrap(),
            "http://[::1]:8123"
        );
        assert_eq!(base("", false), Err(Refused::Empty));
        assert_eq!(base("ftp://ha.local", false), Err(Refused::Malformed));
        assert_eq!(
            base("http://user:pw@ha.local", false),
            Err(Refused::Malformed)
        );
        assert_eq!(base("http://ha.local:80a", false), Err(Refused::Malformed));
        assert_eq!(base("http://ha local", false), Err(Refused::Malformed));
        assert_eq!(base("http://ha.local/../x", false), Err(Refused::Malformed));
        assert_eq!(base("http://ha.local?x=1", false), Err(Refused::Malformed));
        assert_eq!(base("http://", false), Err(Refused::Malformed));
        // Outside the home network: https and the setting, never plain http.
        assert_eq!(base("http://ha.example.com", true), Err(Refused::Insecure));
        assert_eq!(base("https://ha.example.com", false), Err(Refused::Outside));
        assert_eq!(
            base("https://ha.example.com", true).unwrap(),
            "https://ha.example.com"
        );
        assert_eq!(base("ha.example.com", true), Err(Refused::Insecure));
    }

    #[test]
    fn the_calls() {
        let b = "http://ha.local:8123";
        assert_eq!(states_url(b).unwrap(), "http://ha.local:8123/api/states");
        assert_eq!(
            state_url(b, "light.desk").unwrap(),
            "http://ha.local:8123/api/states/light.desk"
        );
        assert_eq!(
            service_url(b, "light", "toggle").unwrap(),
            "http://ha.local:8123/api/services/light/toggle"
        );
        assert_eq!(
            service_body("light.desk").unwrap(),
            r#"{"entity_id":"light.desk"}"#
        );
    }

    #[test]
    fn failures_are_told_in_words() {
        assert_eq!(Problem::of(Reason::Ok, 200), None);
        assert_eq!(Problem::of(Reason::Ok, 201), None);
        assert_eq!(Problem::of(Reason::Ok, 401), Some(Problem::Denied));
        assert_eq!(Problem::of(Reason::Ok, 404), Some(Problem::NotFound));
        assert_eq!(Problem::of(Reason::Ok, 500), Some(Problem::Status(500)));
        assert_eq!(Problem::of(Reason::Dns, 0), Some(Problem::Unreachable));
        assert_eq!(Problem::of(Reason::Tls, 0), Some(Problem::Tls));
        assert_eq!(Problem::of(Reason::NoNetwork, 0), Some(Problem::NoNetwork));
        assert_eq!(Problem::of(Reason::Offline, 0), Some(Problem::Offline));
    }
}
