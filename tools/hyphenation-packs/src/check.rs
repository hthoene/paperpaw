//! Looking at a built folder: layout, names, caps and the table's header.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use crate::{FORMATS, MAX_FILE, MAX_LANGUAGES, MAX_MANIFEST, MAX_PATH, MAX_TABLE, MAX_TAG};

/// Something wrong with a folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem(pub String);

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The lines `key = value` of `section` of an ini text, in order.
pub(crate) fn section<'a>(ini: &'a str, name: &str) -> Vec<(&'a str, &'a str)> {
    let mut inside = false;
    let mut out = Vec::new();
    for line in ini.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if let Some(head) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            inside = head == name;
        } else if inside && let Some((k, v)) = line.split_once('=') {
            out.push((k.trim(), v.trim()));
        }
    }
    out
}

fn value<'a>(ini: &'a str, section_name: &str, key: &str) -> Option<&'a str> {
    section(ini, section_name)
        .into_iter()
        .find_map(|(k, v)| (k == key).then_some(v))
}

/// Every file below `dir`, as `/`-separated paths from it. A symbolic link is
/// never followed: it is listed in `links` and not read.
fn files(dir: &Path, prefix: &str, out: &mut Vec<String>, links: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        match entry.file_type() {
            Ok(kind) if kind.is_symlink() => links.push(path),
            Ok(kind) if kind.is_dir() => files(&entry.path(), &path, out, links),
            _ => out.push(path),
        }
    }
}

fn is_plugin_id(id: &str) -> bool {
    (1..=40).contains(&id.len())
        && !id.starts_with('-')
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// The reference rule for a path named in `[hyphenation]`.
pub(crate) fn path_problem(path: &str) -> Option<&'static str> {
    if path.len() > MAX_PATH {
        return Some("longer than 100 bytes");
    }
    if path.starts_with('/') || path.ends_with('/') || path.is_empty() {
        return Some("empty, or a leading or trailing /");
    }
    let segments: Vec<&str> = path.split('/').collect();
    if segments.len() > 3 {
        return Some("more than 3 segments");
    }
    if segments.iter().any(|s| s.is_empty() || s.starts_with('.')) {
        return Some("an empty or dot-leading segment");
    }
    let plain = |b: u8| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'/');
    (!path.bytes().all(plain)).then_some("a character outside A-Z a-z 0-9 . _ - /")
}

fn tag_problem(tag: &str) -> Option<&'static str> {
    if tag.is_empty() || tag.len() > MAX_TAG {
        return Some("empty or longer than 35 bytes");
    }
    (!tag.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')).then_some("not [A-Za-z0-9-]")
}

/// The expected files only.
fn layout(files: &[String], id: &str, out: &mut Vec<Problem>) {
    let allowed = |f: &str| {
        f == "plugin.ini"
            || (f.starts_with("i18n/") && f.ends_with(".ini"))
            || (f.starts_with("how/") && f.ends_with(".txt"))
            || (f.starts_with("hyphenation/")
                && (f.ends_with(".pphp") || f == "hyphenation/README.md"))
    };
    for f in files {
        if f.len() > MAX_PATH {
            out.push(Problem(format!(
                "{id}: {f}: path longer than {MAX_PATH} bytes"
            )));
        } else if path_problem(f).is_some() {
            out.push(Problem(format!("{id}: {f}: not a plain file name")));
        } else if !allowed(f) {
            out.push(Problem(format!(
                "{id}: {f}: a file that PaperPaw does not read here"
            )));
        }
    }
    for needed in [
        "plugin.ini",
        "i18n/en.ini",
        "i18n/de.ini",
        "how/en.txt",
        "how/de.txt",
    ] {
        if !files.iter().any(|f| f == needed) {
            out.push(Problem(format!("{id}: {needed} is missing")));
        }
    }
}

fn text(dir: &Path, file: &str, cap: u64, out: &mut Vec<Problem>) -> Option<String> {
    let path = dir.join(file);
    let meta = fs::symlink_metadata(&path).ok()?;
    if !meta.is_file() {
        return None;
    }
    let size = meta.len();
    if size > cap {
        out.push(Problem(format!("{file}: {size} bytes, at most {cap}")));
        return None;
    }
    fs::read_to_string(&path).ok()
}

fn dashes(file: &str, content: &str, out: &mut Vec<Problem>) {
    if content.contains('\u{2014}') || content.contains('\u{2013}') {
        out.push(Problem(format!("{file}: em or en dash")));
    }
}

/// `plugin.ini`'s `[plugin]` and the catalogs it repeats.
fn identity(dir: &Path, id: &str, out: &mut Vec<Problem>) {
    let Some(ini) = text(dir, "plugin.ini", MAX_MANIFEST, out) else {
        return;
    };
    dashes("plugin.ini", &ini, out);
    if value(&ini, "plugin", "id") != Some(id) {
        out.push(Problem(format!("plugin.ini: id is not {id}")));
    }
    let version = value(&ini, "plugin", "version").unwrap_or("");
    let parts: Vec<&str> = version.split('.').collect();
    if parts.len() != 3
        || parts
            .iter()
            .any(|p| p.is_empty() || p.parse::<u32>().is_err())
    {
        out.push(Problem(format!(
            "plugin.ini: version {version:?} is not x.y.z"
        )));
    }
    if value(&ini, "plugin", "formats").and_then(|v| v.parse::<u32>().ok()) != Some(FORMATS) {
        out.push(Problem(format!(
            "plugin.ini: formats must be {FORMATS} for [hyphenation]"
        )));
    }
    for what in ["name", "description"] {
        let ini_value = value(&ini, "plugin", what).unwrap_or("");
        let key = format!("{id}.plugin.{what}");
        let english = text(dir, "i18n/en.ini", MAX_FILE, out).unwrap_or_default();
        let german = text(dir, "i18n/de.ini", MAX_FILE, out).unwrap_or_default();
        if ini_value.is_empty() || value(&english, "messages", &key) != Some(ini_value) {
            out.push(Problem(format!(
                "plugin.ini: {what} differs from {key} in i18n/en.ini"
            )));
        }
        if value(&german, "messages", &key).is_none_or(str::is_empty) {
            out.push(Problem(format!("i18n/de.ini: {key} is missing")));
        }
    }
    for file in ["i18n/en.ini", "i18n/de.ini", "how/en.txt", "how/de.txt"] {
        if let Some(content) = text(dir, file, MAX_FILE, out) {
            dashes(file, &content, out);
        }
    }
}

/// One `[hyphenation]` line: tag, path, then the table.
fn table(dir: &Path, tag: &str, path: &str, out: &mut Vec<Problem>) {
    if let Some(why) = tag_problem(tag) {
        out.push(Problem(format!("[hyphenation] {tag}: tag {why}")));
    }
    if let Some(why) = path_problem(path) {
        out.push(Problem(format!("[hyphenation] {tag}: path {path}: {why}")));
        return;
    }
    let file: PathBuf = dir.join(path);
    let Ok(meta) = fs::symlink_metadata(&file) else {
        out.push(Problem(format!(
            "[hyphenation] {tag}: {path} does not exist"
        )));
        return;
    };
    if !meta.is_file() {
        out.push(Problem(format!(
            "[hyphenation] {tag}: {path} is not a plain file"
        )));
        return;
    }
    if meta.len() > MAX_TABLE {
        out.push(Problem(format!(
            "[hyphenation] {tag}: {path} is {} bytes, at most {MAX_TABLE}",
            meta.len()
        )));
        return;
    }
    let Ok(bytes) = fs::read(&file) else {
        out.push(Problem(format!(
            "[hyphenation] {tag}: {path} cannot be read"
        )));
        return;
    };
    if let Err(why) = paperpaw_hyphen::check_table(&bytes) {
        out.push(Problem(format!(
            "[hyphenation] {tag}: {path}: table check failed, {why}"
        )));
    }
}

fn languages(dir: &Path, files: &[String], out: &mut Vec<Problem>) {
    let ini = fs::read_to_string(dir.join("plugin.ini")).unwrap_or_default();
    let lines = section(&ini, "hyphenation");
    if lines.is_empty() {
        out.push(Problem("plugin.ini: [hyphenation] has no language".into()));
    }
    if lines.len() > MAX_LANGUAGES {
        out.push(Problem(format!(
            "plugin.ini: {} languages, at most {MAX_LANGUAGES}",
            lines.len()
        )));
    }
    let mut seen: Vec<String> = Vec::new();
    for &(tag, path) in &lines {
        let lower = tag.to_ascii_lowercase();
        if seen.contains(&lower) {
            out.push(Problem(format!("[hyphenation] {tag}: the language twice")));
        }
        seen.push(lower);
        table(dir, tag, path, out);
    }
    for f in files.iter().filter(|f| f.ends_with(".pphp")) {
        if !lines.iter().any(|&(_, path)| path == f) {
            out.push(Problem(format!("{f}: no language names this table")));
        }
    }
}

/// Everything wrong with the plugin folder `dir` (none: it is as it should be).
pub fn check(dir: &Path) -> Vec<Problem> {
    let id = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut out = Vec::new();
    if !is_plugin_id(&id) {
        out.push(Problem(format!("{id}: not a plugin id")));
    }
    let (mut found, mut links) = (Vec::new(), Vec::new());
    files(dir, "", &mut found, &mut links);
    for link in &links {
        out.push(Problem(format!(
            "{id}: {link}: a symbolic link, which is never followed"
        )));
    }
    layout(&found, &id, &mut out);
    identity(dir, &id, &mut out);
    languages(dir, &found, &mut out);
    out
}
