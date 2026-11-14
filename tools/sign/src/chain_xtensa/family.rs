//! The compact `allow` lines of the S3 scan (todo 624): a category instead of
//! one line per site, for the plugins' own code that the scan roots
//! (`root paperpaw_sdk::native::binding::entry`).
//!
//! - `allow rom <callee> <why>`: any function may call the routine `callee`
//!   outside the image (the chip's ROM: `memcpy`, `memset`, the soft-float
//!   helpers). A call to code of the image with no symbol is not covered.
//! - `allow indirect-in <crate> <max> <why>`: the functions of the crate (a
//!   trailing `*` is a prefix: `paperpaw_plugin_*`) have at most `max` calls
//!   through a register between them that no `allow indirect` line of their
//!   own lists. The callees of such a call are not scanned (device test S4), as
//!   with the per-function line; what is lost is the exact count of each
//!   function, what stays is a ceiling for the family, and the line fails when
//!   no function of the family is reached.
//! - `include <file>`: the lines of a file beside the budget (both S3 devices
//!   share one for the linked plugins).

use super::{Allow, Budget, Function};
use std::collections::BTreeMap;

/// The crate a function belongs to: the first segment of its path, or, for a
/// method of a trait impl (whose path is the method's name), the crate of the
/// type in the symbol (`_ZN<n>_$LT$crate..module..Type$u20$as$u20$…`).
pub(super) fn crate_of(f: &Function) -> Option<String> {
    f.names().find_map(|(symbol, path)| {
        if !path.starts_with('<')
            && let Some((head, _)) = path.split_once("::")
        {
            return Some(head.to_string());
        }
        crate_of_symbol(symbol)
    })
}

pub(super) fn crate_of_symbol(symbol: &str) -> Option<String> {
    let rest = symbol.strip_prefix("_ZN")?;
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    let (length, rest) = rest.split_at(digits);
    let length: usize = length.parse().ok()?;
    let segment = rest.get(..length)?;
    let name = match segment.strip_prefix("_$LT$") {
        Some(inside) => inside.split("..").next()?,
        None => segment,
    };
    (!name.is_empty() && name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_'))
        .then(|| name.to_string())
}

/// Whether `krate` is named by `pattern` (equal, or `prefix*`).
pub(super) fn family_matches(pattern: &str, krate: &str) -> bool {
    match pattern.strip_suffix('*') {
        Some(prefix) => krate.starts_with(prefix),
        None => krate == pattern,
    }
}

/// The text of a budget with its `include <file>` lines replaced by the
/// file's lines (read beside `base`, one level: an included file includes
/// nothing).
pub fn expand_includes(text: &str, base: &std::path::Path) -> Result<String, String> {
    let mut out = String::new();
    for line in text.lines() {
        match line
            .split('#')
            .next()
            .unwrap_or("")
            .split_whitespace()
            .collect::<Vec<_>>()
            .as_slice()
        {
            ["include", file] => {
                let path = base
                    .parent()
                    .unwrap_or(std::path::Path::new("."))
                    .join(file);
                let body = std::fs::read_to_string(&path)
                    .map_err(|e| format!("include {}: {e}", path.display()))?;
                if body.lines().any(|l| l.trim_start().starts_with("include ")) {
                    return Err(format!(
                        "include {}: an included file includes nothing",
                        path.display()
                    ));
                }
                out.push_str(&body);
                out.push('\n');
            }
            _ => {
                out.push_str(line);
                out.push('\n');
            }
        }
    }
    Ok(out)
}

/// An `allow rom` or `allow indirect-in` line, with its reason.
pub(super) fn parse_line(words: &[&str]) -> Option<(Allow, String)> {
    match words {
        ["allow", "rom", callee, why @ ..] if !why.is_empty() => Some((
            Allow::Rom {
                callee: (*callee).to_string(),
            },
            why.join(" "),
        )),
        ["allow", "indirect-in", family, max, why @ ..] if !why.is_empty() => Some((
            Allow::IndirectIn {
                family: (*family).to_string(),
                max: max.parse().ok()?,
            },
            why.join(" "),
        )),
        _ => None,
    }
}

/// Counts `count` unfollowed calls of `f` in the first `allow indirect-in`
/// family that names its crate; whether one does.
pub(super) fn in_a_family(
    budget: &Budget,
    used: &mut [bool],
    sites: &mut BTreeMap<usize, u32>,
    f: &Function,
    count: u32,
) -> bool {
    let Some(krate) = crate_of(f) else {
        return false;
    };
    for (n, (allow, _)) in budget.allow.iter().enumerate() {
        if let Allow::IndirectIn { family, .. } = allow
            && family_matches(family, &krate)
        {
            used[n] = true;
            *sites.entry(n).or_default() += count;
            return true;
        }
    }
    false
}

/// The families whose calls through a register are more than their line allows.
pub(super) fn ceilings_exceeded(budget: &Budget, sites: &BTreeMap<usize, u32>) -> Vec<String> {
    let mut out = Vec::new();
    for (&n, &count) in sites {
        if let Allow::IndirectIn { family, max } = &budget.allow[n].0
            && count > *max
        {
            out.push(format!(
                "the functions of `{family}` have {count} call(s) through a register that no line of their own lists, above the {max} `allow indirect-in` allows"
            ));
        }
    }
    out
}
