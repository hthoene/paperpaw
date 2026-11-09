//! The `[hyphenation]` section of a data plugin's `plugin.ini` (formats 5)
//! and the tables it names, by language (`docs/abi/proposals/
//! declarative-registration.md`, "The `[hyphenation]` data-plugin kind").
//!
//! ```text
//! [hyphenation]
//! nb = hyphenation/nb.pphp
//! no = hyphenation/nb.pphp
//! ```
//!
//! A line is a language tag and a file in the plugin's folder. What is read
//! here is only the section: the file itself is checked when its language is
//! first needed (it may be missing then, or replaced), not as the plugin
//! loads. A bad line is left out with one warning; the plugin stays.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// The formats that brought the `[hyphenation]` section.
pub(crate) const FORMATS_5: u32 = 5;
/// Most languages one plugin names.
pub(crate) const MAX_TABLES: usize = 8;
/// Longest language tag, in bytes (BCP 47's longest practical tag).
const MAX_TAG: usize = 35;
/// Longest path of a table, relative to the plugin's folder, in bytes.
const MAX_PATH: usize = 100;
/// Most segments of that path.
const MAX_SEGMENTS: usize = 3;

/// A language and its table, as a plugin names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Table {
    /// The language tag, in lowercase.
    pub(crate) tag: String,
    /// The table's path relative to the plugin's folder.
    pub(crate) path: String,
}

/// The tables `[hyphenation]` in `source` (a `plugin.ini`) names, of a
/// plugin made for `formats`; what is left out is told in `problems`,
/// which `path` (the plugin's folder) names.
pub(crate) fn tables(
    source: &str,
    path: &str,
    formats: u32,
    problems: &mut Vec<String>,
) -> Vec<Table> {
    let mut found: Vec<Table> = Vec::new();
    let entries = paperpaw_config::parse(source)
        .filter_map(Result::ok)
        .filter(|entry| entry.section == "hyphenation");
    let mut told_formats = false;
    for entry in entries {
        let at = format!("{path}/plugin.ini:{}", entry.line);
        if formats < FORMATS_5 {
            if !told_formats {
                problems.push(format!(
                    "{at}: [hyphenation] left out, the plugin says it is made for formats {formats}"
                ));
                told_formats = true;
            }
            continue;
        }
        let value = entry.value.as_str();
        let file = value.trim();
        let problem = if !valid_tag(entry.key) {
            Some("not a language tag")
        } else if !valid_path(file) {
            Some("not a file of the plugin's folder")
        } else if found.iter().any(|t| t.tag.eq_ignore_ascii_case(entry.key)) {
            Some("the language is named twice")
        } else if found.len() == MAX_TABLES {
            Some("more languages than a plugin may name")
        } else {
            None
        };
        if let Some(problem) = problem {
            problems.push(format!(
                "{at}: [hyphenation] {} left out, {problem}",
                entry.key
            ));
            continue;
        }
        found.push(Table {
            tag: entry.key.to_ascii_lowercase(),
            path: file.to_string(),
        });
    }
    found
}

/// Why the loader refuses a whole plugin ([`hyphenation_tables`]).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ManifestRefusal {
    /// The plugin is made for a later PaperPaw than this one reads
    /// (`made_for` is its formats; this one reads up to
    /// [`paperpaw_sdk::DATA_FORMATS`]). It may be a good plugin that a newer
    /// firmware wrote: not this one's to judge, or to replace.
    LaterFormats { made_for: u32 },
    /// Any other reason (a line that does not parse, an invalid id or
    /// version, ...), in words.
    Invalid(String),
}

/// The formats `source` says it is made for (the last `formats` of its
/// `[plugin]`, read as the loader reads it); lines that do not parse are
/// passed over here.
fn made_for(source: &str) -> Option<u32> {
    let mut found = None;
    for entry in paperpaw_config::parse(source).filter_map(Result::ok) {
        if entry.section == "plugin" && entry.key == "formats" {
            found = entry.value.as_str().trim().parse().ok();
        }
    }
    found
}

/// The tables the loader takes from the `plugin.ini` `source` of the plugin
/// folder `folder` (its name: the id of a plugin that names none): the paths
/// `[hyphenation]` names, relative to the folder, in the order written. `Err`
/// where the loader refuses the whole plugin (a line that does not parse, an
/// invalid id or version, formats from a later PaperPaw: told apart by
/// [`ManifestRefusal`]). Formats before 5 give no tables. It is the loader's
/// own code (`parse_manifest` and `tables`, and its formats limit), so a shell
/// that has to tell a usable hyphenation plugin from one that is not (the
/// default pack's seeding) cannot drift from it. Whether a table's file is
/// there, or holds, is not looked at here.
///
/// # Errors
/// Why the loader refuses the plugin.
pub fn hyphenation_tables(source: &str, folder: &str) -> Result<Vec<String>, ManifestRefusal> {
    let formats = match super::parse_manifest(source, folder) {
        Ok((_, formats)) => formats,
        Err(why) => {
            // A pack of a later PaperPaw is told as such, whatever else is in
            // it: it is kept toward the newer firmware, not judged.
            return Err(match made_for(source) {
                Some(made_for) if made_for > super::FORMATS => {
                    ManifestRefusal::LaterFormats { made_for }
                }
                _ => ManifestRefusal::Invalid(why),
            });
        }
    };
    let mut ignored = Vec::new();
    Ok(tables(source, folder, formats, &mut ignored)
        .into_iter()
        .map(|table| table.path)
        .collect())
}

/// Whether `tag` is a language tag a table may be named for: `[A-Za-z0-9-]`,
/// 1 to 35 bytes.
fn valid_tag(tag: &str) -> bool {
    (1..=MAX_TAG).contains(&tag.len())
        && tag.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// The reference rule for a file of the plugin's folder: relative,
/// `[A-Za-z0-9._-]` and `/`, at most 3 segments and 100 bytes, no empty,
/// `.`, `..` or dot-leading segment (so no leading or trailing `/`).
pub(crate) fn valid_path(path: &str) -> bool {
    path.len() <= MAX_PATH
        && path.split('/').count() <= MAX_SEGMENTS
        && path.split('/').all(|segment| {
            !segment.is_empty()
                && !segment.starts_with('.')
                && segment
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
        })
}

#[cfg(test)]
mod tests;
