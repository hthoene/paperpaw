//! A plugin's repository as it is, before a release: what the site will
//! read from the release it makes.
//!
//! The plugin's folder is the release workflow's `PLUGIN_DIR` (`plugin`
//! in the template), else `plugin/` where it has a `plugin.ini`, else the
//! repository's top. Its `plugin.ini` and `store/store.ini` give the store
//! entry; `store/` also holds the screenshots; everything else in the
//! folder is what readers install. A plugin with code has a `Cargo.toml`
//! at the repository's top.

use std::path::{Path, PathBuf};

use crate::Problem;
use crate::ini::{self, Line};
use crate::rules::{Rules, is_version, valid_name};

/// What the release workflow template is called.
pub const WORKFLOW: &str = ".github/workflows/paperpaw-release.yml";

/// The devices the store knows, by their names.
const DEVICES: [&str; 5] = ["android", "x4", "x4-pro", "x4-classic", "x3"];

/// What a system adds that is no part of a plugin: the site leaves it out
/// with a warning.
const LITTER: [&str; 4] = [".DS_Store", "__MACOSX", "Thumbs.db", "desktop.ini"];

/// The plugin a repository holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plugin {
    pub id: String,
    pub name: String,
    pub version: String,
    /// It has code (a `Cargo.toml` at the repository's top).
    pub code: bool,
    /// Its folder.
    pub folder: PathBuf,
}

/// What a check found: the plugin, where it could be read, and the
/// problems.
#[derive(Debug, Default)]
pub struct Found {
    pub plugin: Option<Plugin>,
    pub problems: Vec<Problem>,
}

/// The release workflow's switches (`CODE`, `PLUGIN_DIR`, ...), as its
/// `env` names them.
fn switches(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            let (key, value) = line.split_once(':')?;
            let key = key.trim();
            let upper = !key.is_empty()
                && key
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_');
            upper.then(|| {
                let value = value.split(" #").next().unwrap_or(value).trim();
                (key.to_string(), value.trim_matches('"').to_string())
            })
        })
        .collect()
}

/// Checks the repository at `dir` by `rules`.
pub fn check(dir: &Path, rules: &Rules) -> Found {
    let mut found = Found::default();
    let mut add = |p: Problem| found.problems.push(p);
    if !dir.is_dir() {
        add(Problem::error(
            "repo",
            dir.display().to_string(),
            "no such folder: name the plugin's repository",
        ));
        return found;
    }
    let (folder_name, code) = workflow(dir, &mut add);
    let folder = dir.join(&folder_name);
    let at = |path: &str| -> String {
        if folder_name == "." {
            path.to_string()
        } else {
            format!("{folder_name}/{path}")
        }
    };
    // plugin.ini: the id, the name, the version, the summary.
    let Ok(manifest) = std::fs::read_to_string(folder.join("plugin.ini")) else {
        add(Problem::error(
            "manifest",
            at("plugin.ini"),
            format!(
                "no plugin.ini in {}: the plugin's folder needs one with [plugin] id, name,                  description and version (set PLUGIN_DIR in the workflow to the folder that has                  it)",
                if folder_name == "." {
                    "the repository's top"
                } else {
                    &folder_name
                }
            ),
        ));
        return found;
    };
    let place = at("plugin.ini");
    let (id, name, version, description) = manifest_of(&manifest, &place, rules, &mut add);
    // store/store.ini: what only the store reads.
    let store_place = at("store/store.ini");
    let store = match std::fs::read_to_string(folder.join("store/store.ini")) {
        Ok(text) => ini::read(&text),
        Err(_) => {
            add(Problem::error(
                "metadata",
                store_place.clone(),
                "no store/store.ini: it names the plugin's group (and its tags, licence and \
                 screenshots), which the site needs",
            ));
            Vec::new()
        }
    };
    let long = store_entry(
        dir,
        &store,
        &store_place,
        &place,
        &description,
        rules,
        &mut add,
    );
    translations(&store, &store_place, rules, &mut add);
    screenshots(&folder, &at, &store, rules, &mut add);
    files(&folder, &at, rules, &mut add);
    if code {
        code_checks(dir, &folder, &at, &store, &long, rules, &mut add);
    }
    around(dir, rules, &mut add);
    found.plugin = (!id.is_empty() && !version.is_empty()).then_some(Plugin {
        id,
        name,
        version,
        code,
        folder,
    });
    found
}

/// The release workflow, and what it says: the plugin's folder, and
/// whether the repository has code (and the workflow's `CODE` agrees).
fn workflow(dir: &Path, add: &mut dyn FnMut(Problem)) -> (String, bool) {
    let workflow = std::fs::read_to_string(dir.join(WORKFLOW)).ok();
    let switch = |key: &str| {
        workflow
            .as_deref()
            .and_then(|w| switches(w).into_iter().find(|(k, _)| k == key))
            .map(|(_, v)| v)
    };
    if workflow.is_none() {
        add(Problem::warning(
            "repo.workflow",
            WORKFLOW,
            "no release workflow: copy paperpaw-release.yml from PaperPaw's tools/dev/templates \
             (or make the repository with `paperpaw-dev new --github`), so that a tag makes a \
             release with the files the site reads",
        ));
    }
    let folder_name = match switch("PLUGIN_DIR") {
        Some(dir) => dir,
        None if dir.join("plugin/plugin.ini").is_file() => "plugin".into(),
        None => ".".into(),
    };
    let code = dir.join("Cargo.toml").is_file();
    match switch("CODE").as_deref() {
        Some("true") if !code => add(Problem::error(
            "repo.workflow",
            WORKFLOW,
            "CODE is \"true\", but the repository has no Cargo.toml: set CODE: \"false\" for a \
             plugin without code",
        )),
        Some("false") if code => add(Problem::error(
            "repo.workflow",
            WORKFLOW,
            "CODE is \"false\", but the repository has a Cargo.toml: set CODE: \"true\", so that \
             the release has the signed builds a plugin with code needs",
        )),
        _ => {}
    }
    (folder_name, code)
}

/// `plugin.ini`'s id, name, version and description, checked.
fn manifest_of(
    text: &str,
    place: &str,
    rules: &Rules,
    add: &mut dyn FnMut(Problem),
) -> (String, String, String, String) {
    let place = place.to_string();
    let manifest = ini::read(text);
    let value = |key: &str| {
        ini::get(&manifest, "plugin", key)
            .map(|l| l.value.clone())
            .unwrap_or_default()
    };
    let (id, name, version, description) = (
        value("id"),
        value("name"),
        value("version"),
        value("description"),
    );
    if id.is_empty() || name.is_empty() || version.is_empty() {
        add(Problem::error(
            "manifest",
            place.clone(),
            "[plugin] needs an id, a name and a version",
        ));
    }
    if !id.is_empty() && !rules.valid_id(&id) {
        add(Problem::error(
            "metadata",
            place.clone(),
            format!(
                "the id `{id}`: lower-case letters, digits and hyphens, not at its ends, at most \
                 {} characters, such as night-owl",
                rules.max_id
            ),
        ));
    }
    if rules.retired.contains(&id) {
        add(Problem::error(
            "id",
            place.clone(),
            format!(
                "the id `{id}` was one of PaperPaw's own plugins and is never used again: choose another"
            ),
        ));
    }
    if name.chars().count() > rules.max_name {
        add(Problem::error(
            "metadata",
            place.clone(),
            format!("the name is at most {} characters", rules.max_name),
        ));
    }
    if !version.is_empty() && !is_version(&version) {
        add(Problem::error(
            "version",
            place.clone(),
            format!(
                "the version `{version}` is no SemVer version: write it as 1.0.0 (or 1.0.0-beta.1)"
            ),
        ));
    }
    (id, name, version, description)
}

/// `store/store.ini`'s `[store]`, checked with `plugin.ini`'s
/// `description`: the description the store shows.
fn store_entry(
    dir: &Path,
    store: &[Line],
    store_place: &str,
    place: &str,
    description: &str,
    rules: &Rules,
    add: &mut dyn FnMut(Problem),
) -> String {
    let (store_place, place, description) = (
        store_place.to_string(),
        place.to_string(),
        description.to_string(),
    );
    let said = |key: &str| ini::get(store, "store", key);
    let text_of = |key: &str| said(key).map(|l| l.value.clone()).unwrap_or_default();
    let line_place = |line: Option<&Line>| match line {
        Some(l) => format!("{store_place}:{}", l.line),
        None => store_place.clone(),
    };
    // The summary: store.ini's, or plugin.ini's description where it is
    // one sentence.
    let summary = match text_of("summary") {
        s if !s.is_empty() => s,
        _ if description.chars().count() <= rules.max_summary => description.clone(),
        _ => String::new(),
    };
    if summary.trim().is_empty() {
        add(Problem::error(
            "metadata",
            place.clone(),
            format!(
                "no summary: give [plugin] a description of one sentence (at most {} \
                 characters), or `summary` in store/store.ini",
                rules.max_summary
            ),
        ));
    } else if summary.chars().count() > rules.max_summary {
        add(Problem::error(
            "metadata",
            line_place(said("summary")),
            format!(
                "the summary is one sentence, at most {} characters",
                rules.max_summary
            ),
        ));
    }
    let long = match text_of("description") {
        d if !d.is_empty() => d,
        _ if description.chars().count() > rules.max_summary => description.clone(),
        _ => String::new(),
    };
    if long.chars().count() > rules.max_description {
        add(Problem::error(
            "metadata",
            line_place(said("description")),
            format!(
                "the description is at most {} characters",
                rules.max_description
            ),
        ));
    }
    store_lists(dir, store, &store_place, rules, add);
    long
}

/// `[store]`'s group, tags, licence, source and devices.
fn store_lists(
    dir: &Path,
    store: &[Line],
    store_place: &str,
    rules: &Rules,
    add: &mut dyn FnMut(Problem),
) {
    let store_place = store_place.to_string();
    let said = |key: &str| ini::get(store, "store", key);
    let text_of = |key: &str| said(key).map(|l| l.value.clone()).unwrap_or_default();
    let line_place = |line: Option<&Line>| match line {
        Some(l) => format!("{store_place}:{}", l.line),
        None => store_place.clone(),
    };

    let group = text_of("group");
    if !store.is_empty() && !rules.groups.contains(&group) {
        add(Problem::error(
            "metadata",
            line_place(said("group")),
            if group.is_empty() {
                format!(
                    "no group: add `group = ` with one of {}",
                    rules.groups.join(", ")
                )
            } else {
                format!(
                    "the group `{group}` is none of the store's: use one of {}",
                    rules.groups.join(", ")
                )
            },
        ));
    }
    tags(&text_of("tags"), &line_place(said("tags")), rules, add);
    let licence = text_of("licence");
    let licence_file = ["LICENSE", "LICENSE.md", "LICENSE.txt", "COPYING"]
        .iter()
        .any(|f| dir.join(f).is_file());
    if licence.is_empty() && !store.is_empty() {
        let problem = if licence_file {
            Problem::warning
        } else {
            Problem::error
        };
        add(problem(
            "metadata",
            store_place.clone(),
            "no licence: add `licence = ` with an SPDX identifier such as MIT (the site takes \
             the repository's from GitHub only where GitHub recognised a LICENSE file)",
        ));
    }
    if licence.chars().count() > rules.max_licence {
        add(Problem::error(
            "metadata",
            line_place(said("licence")),
            "the licence is an SPDX identifier such as MIT",
        ));
    }
    if let Some(source) = said("source")
        && !source.value.starts_with("https://")
    {
        add(Problem::error(
            "metadata",
            line_place(Some(source)),
            "source is an https address, such as the repository's",
        ));
    }
    for device in text_of("devices")
        .split(',')
        .map(str::trim)
        .filter(|d| !d.is_empty())
    {
        if !DEVICES.contains(&device) {
            add(Problem::error(
                "metadata",
                line_place(said("devices")),
                format!(
                    "no device `{device}`: the store knows {}",
                    DEVICES.join(", ")
                ),
            ));
        }
    }
}

/// `[store]`'s tags, `text` as written, at `place`.
fn tags(text: &str, place: &str, rules: &Rules, add: &mut dyn FnMut(Problem)) {
    let tags: Vec<String> = text
        .split(',')
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect();
    if tags.len() > rules.max_tags {
        add(Problem::error(
            "metadata",
            place.to_string(),
            format!(
                "{} tags: at most {}, separated by commas",
                tags.len(),
                rules.max_tags
            ),
        ));
    }
    for (i, tag) in tags.iter().enumerate() {
        if !rules.valid_tag(tag) {
            add(Problem::error(
                "metadata",
                place.to_string(),
                format!(
                    "the tag `{tag}`: lower-case letters, digits and hyphens, at most {}",
                    rules.max_tag
                ),
            ));
        }
        if tags[..i].contains(tag) {
            add(Problem::error(
                "metadata",
                place.to_string(),
                format!("the tag `{tag}` twice"),
            ));
        }
    }
}

/// The translations of the name and summary (`[i18n.<lang>]`).
fn translations(store: &[Line], store_place: &str, rules: &Rules, add: &mut dyn FnMut(Problem)) {
    let langs: std::collections::BTreeSet<&str> = store
        .iter()
        .filter_map(|l| l.section.strip_prefix("i18n."))
        .collect();
    for lang in langs {
        let section = format!("i18n.{lang}");
        let get = |key: &str| {
            ini::get(store, &section, key)
                .map(|l| l.value.clone())
                .unwrap_or_default()
        };
        let where_ = format!("{store_place} [{section}]");
        if !((2..=3).contains(&lang.len()) && lang.bytes().all(|b| b.is_ascii_lowercase()))
            || lang == "en"
        {
            add(Problem::error(
                "metadata",
                where_.clone(),
                "a language is two or three lower-case letters such as de, and not en (English \
                 is the name and summary themselves)",
            ));
        }
        if get("name").is_empty() || get("summary").is_empty() {
            add(Problem::error(
                "metadata",
                where_.clone(),
                "a translation needs a name and a summary",
            ));
        }
        if get("summary").chars().count() > rules.max_summary {
            add(Problem::error(
                "metadata",
                where_,
                format!(
                    "the summary is one sentence, at most {} characters",
                    rules.max_summary
                ),
            ));
        }
    }
}

/// The repository around the plugin: its topic named in the README, no
/// key committed.
fn around(dir: &Path, rules: &Rules, add: &mut dyn FnMut(Problem)) {
    let readme = ["README.md", "README"]
        .iter()
        .find_map(|f| std::fs::read_to_string(dir.join(f)).ok());
    if !readme.is_some_and(|r| r.contains(&rules.topic)) {
        add(Problem::warning(
            "repo.topic",
            "README.md",
            format!(
                "the site finds the repository only by its topic: give it the topic `{}` on \
                 GitHub (About, the gear, Topics), and say so in the README",
                rules.topic
            ),
        ));
    }
    keys(dir, dir, add);
}

/// The screenshots in `store/`: names, kinds, sizes and what each shows.
fn screenshots(
    folder: &Path,
    at: &dyn Fn(&str) -> String,
    store: &[Line],
    rules: &Rules,
    add: &mut dyn FnMut(Problem),
) {
    let Ok(entries) = std::fs::read_dir(folder.join("store")) else {
        return;
    };
    let mut shots: Vec<String> = entries
        .filter_map(Result::ok)
        .filter(|e| e.path().is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| {
            name.rsplit_once('.')
                .is_some_and(|(_, x)| rules.screenshot_types.contains(&x.to_lowercase()))
        })
        .collect();
    shots.sort();
    if shots.is_empty() {
        add(Problem::warning(
            "screenshots",
            at("store/"),
            "no screenshots: one of the plugin at work in store/ (a .png) helps people choose it",
        ));
    }
    if shots.len() > rules.max_screenshots {
        add(Problem::warning(
            "screenshots",
            at("store/"),
            format!(
                "{} screenshots: the site keeps the first {}",
                shots.len(),
                rules.max_screenshots
            ),
        ));
    }
    for shot in shots.iter().take(rules.max_screenshots) {
        let place = at(&format!("store/{shot}"));
        let (stem, extension) = shot.rsplit_once('.').unwrap_or((shot, ""));
        if !valid_name(stem, 40) || extension != extension.to_lowercase() {
            add(Problem::warning(
                "screenshots",
                place.clone(),
                "the site leaves it out: name it with lower-case letters, digits and hyphens, \
                 such as home.png",
            ));
            continue;
        }
        let bytes = std::fs::read(folder.join("store").join(shot)).unwrap_or_default();
        if bytes.len() as u64 > rules.max_screenshot_bytes {
            add(Problem::error(
                "screenshots",
                place.clone(),
                format!(
                    "larger than {} KB: make it smaller",
                    rules.max_screenshot_bytes / 1000
                ),
            ));
        }
        let picture = bytes.starts_with(b"\x89PNG\r\n\x1a\n")
            || bytes.starts_with(&[0xFF, 0xD8, 0xFF])
            || (bytes.len() > 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP");
        if !picture {
            add(Problem::error(
                "screenshots",
                place.clone(),
                "not a PNG, JPEG or WebP picture",
            ));
        }
        let alt = ini::get(store, "screenshots", shot)
            .map(|l| l.value.clone())
            .unwrap_or_default();
        if alt.is_empty() {
            add(Problem::error(
                "metadata",
                place,
                format!("say what it shows: `{shot} = ...` in [screenshots] of store/store.ini"),
            ));
        } else if alt.chars().count() > rules.max_alt {
            add(Problem::error(
                "metadata",
                place,
                format!("what it shows is at most {} characters", rules.max_alt),
            ));
        }
    }
}

/// The files readers install: the folder without `store/`.
fn files(folder: &Path, at: &dyn Fn(&str) -> String, rules: &Rules, add: &mut dyn FnMut(Problem)) {
    let mut all = Vec::new();
    walk(folder, folder, &mut all);
    let mut count = 0;
    let mut total = 0u64;
    for (path, size) in all {
        if path == "store" || path.starts_with("store/") {
            continue;
        }
        let name = path.rsplit('/').next().unwrap_or(&path);
        if LITTER.iter().any(|l| path.split('/').any(|p| p == *l)) || name.starts_with('.') {
            add(Problem::warning(
                "package",
                at(&path),
                "left out by the site, with a warning: delete it (or leave it out of the ZIP)",
            ));
            continue;
        }
        count += 1;
        total += size;
        let extension = name
            .rsplit_once('.')
            .map(|(_, x)| x.to_lowercase())
            .unwrap_or_default();
        if !rules.package_kinds.contains(&extension) {
            add(Problem::error(
                "package",
                at(&path),
                format!(
                    "a plugin's folder holds only {} files: move it out of the folder (keep the \
                     repository's own files, such as README and LICENSE, outside PLUGIN_DIR)",
                    rules.package_kinds.join(", .")
                ),
            ));
        }
        if path.len() > rules.max_path {
            add(Problem::error(
                "package",
                at(&path),
                format!("its path is longer than {} characters", rules.max_path),
            ));
        }
    }
    if count > rules.max_files {
        add(Problem::error(
            "package",
            at(""),
            format!("{count} files: a plugin has at most {}", rules.max_files),
        ));
    }
    if total > rules.max_content {
        add(Problem::error(
            "package",
            at(""),
            format!(
                "{} MB of files: at most {} MB",
                total >> 20,
                rules.max_content >> 20
            ),
        ));
    }
}

/// Every file below `dir`, by its path from `root` with `/`, and its size;
/// not into `.git`, `target` or hidden folders.
fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, u64)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if name.starts_with('.') || name == "target" {
                continue;
            }
            walk(root, &path, out);
        } else {
            let relative = path
                .strip_prefix(root)
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or(name);
            out.push((relative, entry.metadata().map_or(0, |m| m.len())));
        }
    }
}

/// A plugin with code: what its builds and the site's rebuild need.
fn code_checks(
    dir: &Path,
    folder: &Path,
    at: &dyn Fn(&str) -> String,
    store: &[Line],
    description: &str,
    rules: &Rules,
    add: &mut dyn FnMut(Problem),
) {
    if !dir.join("Cargo.lock").is_file() {
        add(Problem::error(
            "source",
            "Cargo.lock",
            "no Cargo.lock: build once (`cargo x4`) and commit it; the workflow builds with \
             --locked, and the site rebuilds from it",
        ));
    }
    let cargo = std::fs::read_to_string(dir.join("Cargo.toml")).unwrap_or_default();
    if let Some(line) = cargo.lines().find(|l| {
        l.trim_start().starts_with("paperpaw-sdk") && (l.contains("path") || l.contains("git"))
    }) {
        add(Problem::error(
            "source",
            "Cargo.toml",
            format!(
                "`{}`: name the SDK of a PaperPaw release by its version, such as \
                 paperpaw-sdk = \"0.6.0-beta.2\", never a path or a git checkout",
                line.trim()
            ),
        ));
    }
    if let Some(path) = ini::get(store, "source", "path")
        && !dir.join(&path.value).join("Cargo.toml").is_file()
    {
        add(Problem::error(
            "source",
            at("store/store.ini"),
            format!(
                "[source] path = {}: there is no Cargo.toml there",
                path.value
            ),
        ));
    }
    let how = std::fs::read_to_string(folder.join("how/en.txt")).unwrap_or_default();
    if how.trim().chars().count() < rules.least_how && description.chars().count() < rules.least_how
    {
        add(Problem::warning(
            "how",
            at("how/en.txt"),
            format!(
                "say how it works, at least {} characters, in how/en.txt or the description: \
                 what it does and what it needs (the network, the card)",
                rules.least_how
            ),
        ));
    }
    let ignore = std::fs::read_to_string(dir.join(".gitignore")).unwrap_or_default();
    if !ignore.lines().any(|l| l.trim() == "*.key") {
        add(Problem::warning(
            "repo.key",
            ".gitignore",
            "add `*.key`, so that an author key is never committed",
        ));
    }
}

/// An author key committed anywhere in the repository.
fn keys(root: &Path, dir: &Path, add: &mut dyn FnMut(Problem)) {
    let mut all = Vec::new();
    walk(root, dir, &mut all);
    for (path, _) in all {
        if path.ends_with(".key") {
            add(Problem::error(
                "repo.key",
                path,
                "a key file in the repository: remove it from the repository and its history, \
                 keep it outside, and give the workflow its contents as the secret \
                 PAPERPAW_PLUGIN_KEY (if it was ever pushed, it is leaked: make a new one)",
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_workflows_switches_are_read() {
        let found =
            switches("env:\n  CODE: \"true\"\n  PLUGIN_DIR: plugin # the folder\n  run: x\n");
        assert_eq!(found[0], ("CODE".into(), "true".into()));
        assert_eq!(found[1], ("PLUGIN_DIR".into(), "plugin".into()));
        assert_eq!(found.len(), 2);
    }
}
