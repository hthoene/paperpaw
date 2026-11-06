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

use alloc::collections::TryReserveError;
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

/// A language's table as the kernel holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Entry {
    /// The plugin that names it.
    pub(crate) plugin: String,
    /// The language tag, in lowercase.
    pub(crate) tag: String,
    /// The table's path on the storage (the plugin's folder and the path).
    pub(crate) path: String,
}

impl Entry {
    /// How warnings name it: `<plugin>:<tag>`.
    pub(crate) fn name(&self) -> String {
        format!("{}:{}", self.plugin, self.tag)
    }
}

/// The hyphenation tables of data plugins. Every plugin's are kept; where
/// several plugins name one language, the plugin whose id is greatest
/// (byte-wise, in lowercase) has it, whatever order they load in (D4). So a
/// plugin taken out gives its languages back to the others at once.
#[derive(Debug, Default)]
pub(crate) struct Tables {
    entries: Vec<Entry>,
}

impl Tables {
    /// Adds the `tables` of `plugin` (in `folder`), all or none of them:
    /// room is made first, so nothing is added where it cannot be.
    pub(crate) fn add(
        &mut self,
        plugin: &str,
        folder: &str,
        tables: &[Table],
    ) -> Result<(), TryReserveError> {
        let mut new = Vec::new();
        new.try_reserve_exact(tables.len())?;
        for table in tables {
            new.push(Entry {
                plugin: copy(&[plugin])?,
                tag: copy(&[&table.tag])?,
                path: copy(&[folder, "/", &table.path])?,
            });
        }
        self.entries.try_reserve(new.len())?;
        self.entries.extend(new);
        Ok(())
    }

    /// The entries of `plugin` that another plugin's keeps out, and the
    /// entries of others it keeps out, each with the entry that has its
    /// language: for the warnings as it is added.
    pub(crate) fn collisions<'a>(
        &'a self,
        plugin: &'a str,
    ) -> impl Iterator<Item = (&'a Entry, &'a Entry)> + 'a {
        self.entries
            .iter()
            .filter(move |e| e.plugin != plugin)
            .filter_map(move |other| {
                let mine = self
                    .entries
                    .iter()
                    .find(|e| e.plugin == plugin && e.tag == other.tag)?;
                Some(if wins(&mine.plugin, &other.plugin) {
                    (other, mine)
                } else {
                    (mine, other)
                })
            })
    }

    /// Takes out what `plugin` named (switched off or removed).
    pub(crate) fn remove(&mut self, plugin: &str) {
        self.entries.retain(|e| e.plugin != plugin);
    }

    /// The table for a book's language `tag`: the tag in lowercase, with `_`
    /// read as `-`, exactly; then without its last subtag, again and again
    /// (`de-ch-1996`, `de-ch`, `de`). Never a longer tag, never another
    /// language.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "read by the card-backed reader, wired when the SDK's `Hyphenation` changes at the \
                  interface-7 raise (abi/interface-7/raise.md)"
        )
    )]
    pub(crate) fn find(&self, tag: &str) -> Option<&Entry> {
        let mut wanted = String::new();
        wanted.try_reserve_exact(tag.len()).ok()?;
        wanted.extend(tag.chars().map(|c| {
            if c == '_' {
                '-'
            } else {
                c.to_ascii_lowercase()
            }
        }));
        let mut tag = wanted.as_str();
        loop {
            let found = self
                .entries
                .iter()
                .filter(|e| e.tag == tag)
                .reduce(|a, b| if wins(&b.plugin, &a.plugin) { b } else { a });
            if found.is_some() {
                return found;
            }
            tag = &tag[..tag.rfind('-')?];
        }
    }
}

/// `parts` joined into a string made with room for them, or the allocation
/// error.
fn copy(parts: &[&str]) -> Result<String, TryReserveError> {
    let mut out = String::new();
    out.try_reserve_exact(parts.iter().map(|p| p.len()).sum())?;
    parts.iter().for_each(|p| out.push_str(p));
    Ok(out)
}

/// Whether plugin `a` has a language over plugin `b` (D4): the greater id,
/// compared byte-wise in lowercase.
fn wins(a: &str, b: &str) -> bool {
    a.bytes()
        .map(|c| c.to_ascii_lowercase())
        .cmp(b.bytes().map(|c| c.to_ascii_lowercase()))
        .is_gt()
}

#[cfg(test)]
mod tests;
