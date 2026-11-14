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

use super::{Allow, Around, Budget, Function, Image, read_function};
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

/// The path of the type a trait-impl method is for, read from its symbol
/// (`_ZN<n>_$LT$a..b..Type$LT$..$GT$$u20$as$u20$Trait$GT$<n>method`): the
/// method's path is its name only, so the type is the one thing that says
/// where the impl is. `None` for an impl on a reference-less type with no
/// path (`T`, `[T]`, a tuple, `dyn`), which no family names.
fn type_path(symbol: &str) -> Option<String> {
    let rest = symbol.strip_prefix("_ZN")?;
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    let (length, rest) = rest.split_at(digits);
    let segment = rest.get(..length.parse().ok()?)?;
    let mut inside = segment.strip_prefix("_$LT$")?;
    // `&T`, `&mut T`: the impl is where `T` is.
    while let Some(deeper) = inside
        .strip_prefix("$RF$")
        .map(|s| s.strip_prefix("mut$u20$").unwrap_or(s))
    {
        inside = deeper;
    }
    let end = inside
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '.'))
        .unwrap_or(inside.len());
    let path = inside[..end].trim_end_matches('.').replace("..", "::");
    path.contains("::").then_some(path)
}

/// Whether `f` is in `family`: a path of a crate or module (`a::b`) holds the
/// functions under it (a trait-impl method by the type of its impl), a crate
/// name or `prefix*` the crates it names.
fn in_family(f: &Function, family: &str) -> bool {
    if family.contains("::") {
        let under = |p: &str| p == family || p.starts_with(&format!("{family}::"));
        return f
            .names()
            .any(|(symbol, path)| under(path) || type_path(symbol).is_some_and(|p| under(&p)));
    }
    crate_of(f).is_some_and(|krate| family_matches(family, &krate))
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

/// An `allow frame <function> <max> <why>` line: a function known to take a
/// frame of the limit or more, held at `max` (the owner's todo is in the why).
pub(super) fn parse_frame(words: &[&str]) -> Option<(String, u32, String)> {
    match words {
        ["allow", "frame", name, max, why @ ..] if !why.is_empty() => {
            Some(((*name).to_string(), max.parse().ok()?, why.join(" ")))
        }
        _ => None,
    }
}

/// Whether a frame of `frame` bytes in `f` is a named exception of the budget.
pub(super) fn frame_excused(budget: &Budget, f: &Function, frame: u32) -> bool {
    budget
        .frame_exceptions
        .iter()
        .any(|(name, max, _)| f.is(name) && frame <= *max)
}

fn frame_of(image: &Image<'_>, around: &Around<'_>, f: &Function) -> Option<u32> {
    let (k, from) = f.code;
    let data = image.exec[k].1;
    let to = (from + f.size as usize).min(data.len());
    read_function(&data[from..to], f.start, around).frame
}

/// The functions of a `frames-in` family with a frame of the limit or more
/// (but the named exceptions), the functions with no crate (impls on `&T`,
/// `[T]`, tuples, `dyn`) held with them, a line that matches no function, and
/// an exception line that matches no function or whose function is under the
/// limit now (remove it: the release check is that none is left).
pub(super) fn frames_in_problems(
    image: &Image<'_>,
    around: &Around<'_>,
    budget: &Budget,
) -> Vec<String> {
    let mut out = Vec::new();
    let over = |f: &Function, out: &mut Vec<String>| {
        if let Some(frame) = frame_of(image, around, f)
            && frame >= budget.limit
            && !frame_excused(budget, f, frame)
        {
            // As `judge` words it: a function reached says it once.
            out.push(format!(
                "`{}` takes a frame of {frame} bytes (limit {})",
                f.shown, budget.limit
            ));
        }
    };
    for (family, _) in &budget.frames_in {
        let mut any = false;
        for f in image
            .funcs
            .iter()
            .filter(|f| f.size > 0 && in_family(f, family))
        {
            any = true;
            over(f, &mut out);
        }
        if !any {
            out.push(format!(
                "the `frames-in` line `{family}` matches no function of the image: renamed? (update the budget)"
            ));
        }
    }
    if !budget.frames_in.is_empty() {
        for f in image
            .funcs
            .iter()
            .filter(|f| f.size > 0 && crate_of(f).is_none())
        {
            over(f, &mut out);
        }
    }
    for (name, max, _) in &budget.frame_exceptions {
        let frames: Vec<u32> = image
            .funcs
            .iter()
            .filter(|f| f.size > 0 && f.is(name))
            .filter_map(|f| frame_of(image, around, f))
            .collect();
        if frames.is_empty() {
            out.push(format!(
                "an `allow frame` line matches no function: `{name}` (remove it)"
            ));
        } else if frames.iter().all(|&frame| frame < budget.limit) {
            out.push(format!(
                "`{name}` is under the frame limit now ({max} allowed): remove its `allow frame` line"
            ));
        }
    }
    out
}
