//! `href` resolved against a page's address as browsers do, as
//! `paperpaw_sdk::web::join_url` does, with fallible allocation (a test
//! pins that both give the same).
use alloc::string::String;
use paperpaw_sdk::{document::is_external, native::Error};
use paperpaw_ui_kit::text::{concat, copy};

/// `href` made absolute with `base`, a page's address (`/a`, `../b`, `?q`,
/// `//host/c`).
///
/// # Errors
/// [`Error::NoMemory`].
pub fn join_url(base: &str, href: &str) -> Result<String, Error> {
    if is_external(href) {
        return copy(href);
    }
    let Some((scheme, rest)) = base.split_once("://") else {
        return copy(href);
    };
    if let Some(net) = href.strip_prefix("//") {
        return concat(&[scheme, "://", net]);
    }
    let host_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let host = &rest[..host_end];
    let after = &rest[host_end..];
    let path = after.split(['?', '#']).next().unwrap_or("");
    let path = if path.is_empty() { "/" } else { path };
    let joined = if href.starts_with('/') {
        copy(href)?
    } else if href.starts_with('?') {
        concat(&[path, href])?
    } else if href.starts_with('#') {
        let without = after.split('#').next().unwrap_or("");
        concat(&[if without.is_empty() { "/" } else { without }, href])?
    } else {
        let dir = &path[..path.rfind('/').map_or(0, |i| i + 1)];
        concat(&[dir, href])?
    };
    let path = normalize(&joined)?;
    concat(&[scheme, "://", host, &path])
}

/// Resolves `.` and `..` in the path of `target`, keeping its query and
/// fragment.
fn normalize(target: &str) -> Result<String, Error> {
    let split = target.find(['?', '#']).unwrap_or(target.len());
    let (path, tail) = target.split_at(split);
    let mut out = String::new();
    out.try_reserve_exact(target.len() + 1)
        .map_err(|_| Error::NoMemory)?;
    // The parts of the path joined by `/`: `parts` of them.
    let mut parts = 0usize;
    let mut segments = path.split('/').peekable();
    while let Some(segment) = segments.next() {
        let last = segments.peek().is_none();
        match segment {
            "." => {
                if last {
                    add(&mut out, &mut parts, "");
                }
            }
            ".." => {
                if parts > 1 {
                    out.truncate(out.rfind('/').unwrap_or(0));
                    parts -= 1;
                }
                if last {
                    add(&mut out, &mut parts, "");
                }
            }
            segment => add(&mut out, &mut parts, segment),
        }
    }
    if !out.starts_with('/') {
        out.insert(0, '/');
    }
    out.push_str(tail);
    Ok(out)
}

fn add(out: &mut String, parts: &mut usize, part: &str) {
    if *parts > 0 {
        out.push('/');
    }
    out.push_str(part);
    *parts += 1;
}
