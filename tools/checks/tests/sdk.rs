//! The plugin SDK's design rules that a machine can check
//! (`docs/sdk-design.md`), over the crates whose types cross between a
//! code plugin and PaperPaw (`crates/paperpaw-sdk/interface/crates.txt`).
//! Each check reads those crates' `api.txt` or their source. What it finds
//! is listed in a file of `tools/checks/sdk/`, which may only get shorter:
//! a new finding fails, and a listed one that is gone must leave the list.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::collections::BTreeSet;

use paperpaw_checks::{root, rust_files};

/// The crossing crates, by name.
fn crossing() -> Vec<String> {
    let path = root().join("crates/paperpaw-sdk/interface/crates.txt");
    std::fs::read_to_string(path)
        .expect("crates.txt")
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_string)
        .collect()
}

/// The lines of each crossing crate's `api.txt`.
fn api() -> Vec<String> {
    crossing()
        .iter()
        .flat_map(|name| {
            let path = root().join("crates").join(name).join("api.txt");
            std::fs::read_to_string(&path)
                .unwrap_or_else(|_| panic!("{}: run tools/interface.py", path.display()))
                .lines()
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .collect()
}

/// What `tools/checks/sdk/<name>` lists: one entry a line, `#` starting
/// a comment (a reason, or a heading).
fn listed(name: &str) -> BTreeSet<String> {
    let path = root().join("tools/checks/sdk").join(name);
    std::fs::read_to_string(path)
        .expect("the list")
        .lines()
        .map(|line| line.split_once('#').map_or(line, |(entry, _)| entry).trim())
        .filter(|entry| !entry.is_empty())
        .map(str::to_string)
        .collect()
}

/// Fails with what `found` has beyond `tools/checks/sdk/<list>` (told
/// `wrong` about each) and what the list still has that is gone.
fn ratchet(list: &str, found: &BTreeSet<String>, wrong: &str) {
    let listed = listed(list);
    let mut errors: Vec<String> = found
        .difference(&listed)
        .map(|entry| format!("{entry}: {wrong}"))
        .collect();
    errors.extend(
        listed
            .difference(found)
            .map(|entry| format!("{entry}: gone; take it out of tools/checks/sdk/{list}")),
    );
    assert!(errors.is_empty(), "\n{}", errors.join("\n"));
}

/// The attributes before an `api.txt` line's item, and the item.
fn attributes(line: &str) -> (&str, &str) {
    let mut rest = line;
    while rest.starts_with("#[") {
        let Some(end) = rest.find("] ") else { break };
        rest = &rest[end + 2..];
    }
    (&line[..line.len() - rest.len()], rest)
}

/// The path an `api.txt` item names: `a::b::C` of `pub fn a::b::C(..)`.
fn path_of(item: &str) -> &str {
    let mut rest = item;
    for word in [
        "pub ", "const ", "unsafe ", "fn ", "type ", "struct ", "enum ", "trait ",
    ] {
        rest = rest.strip_prefix(word).unwrap_or(rest);
    }
    let end = rest
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == ':'))
        .unwrap_or(rest.len());
    rest[..end].trim_end_matches(':')
}

#[test]
fn growing_types_are_non_exhaustive() {
    let api = api();
    // Structs a plugin can build or take apart: with a public field, which
    // `api.txt` lists as `pub path::Struct::field: Type`.
    let mut with_fields = BTreeSet::new();
    for line in &api {
        let Some(field) = attributes(line).1.strip_prefix("pub ") else {
            continue;
        };
        let Some((path, _)) = field.split_once(": ") else {
            continue;
        };
        if let Some((owner, _)) = path
            .rsplit_once("::")
            .filter(|_| !path.contains([' ', '(']))
        {
            with_fields.insert(owner.to_string());
        }
    }
    let mut found = BTreeSet::new();
    for line in &api {
        let (attributes, item) = attributes(line);
        let kind = if item.starts_with("pub enum ") {
            "enum"
        } else if item.starts_with("pub struct ") {
            "struct"
        } else {
            continue;
        };
        let path = path_of(item);
        let open = kind == "enum" || with_fields.contains(path) || item.contains("(pub ");
        if open && !attributes.contains("#[non_exhaustive]") {
            found.insert(path.to_string());
        }
    }
    ratchet(
        "exhaustive.txt",
        &found,
        "a public type a plugin can build or match whole: make it #[non_exhaustive] with a constructor, or list it with the reason it must stay exhaustive",
    );
}

/// The crossing crates' Rust files below `src/`, by their path from the
/// repository's top folder.
fn sources() -> Vec<(String, String)> {
    let crates: Vec<String> = crossing()
        .iter()
        .map(|name| format!("crates/{name}/src/"))
        .collect();
    rust_files(&root())
        .into_iter()
        .filter(|(path, _)| crates.iter().any(|c| path.starts_with(c.as_str())))
        .map(|(path, file)| (path, std::fs::read_to_string(file).expect("readable")))
        .collect()
}

/// Each item with an attribute starting `attribute`, as `path: item` (the
/// item's first line, up to a bracket), with the attribute lines around it.
fn marked(attribute: &str) -> Vec<(String, String)> {
    let mut found = Vec::new();
    for (path, text) in sources() {
        let lines: Vec<&str> = text.lines().map(str::trim).collect();
        for (at, line) in lines.iter().enumerate() {
            if !line.starts_with(attribute) {
                continue;
            }
            // The attributes go back to the doc comment or the item before.
            let start = lines[..at]
                .iter()
                .rposition(|l| l.is_empty() || l.starts_with("//") || l.ends_with(['}', ';']))
                .map_or(0, |p| p + 1);
            let Some(item) = lines[at..].iter().position(|l| l.starts_with("pub ")) else {
                continue;
            };
            let head = lines[at + item]
                .split(['(', '{', '<', '=', ';'])
                .next()
                .unwrap_or("")
                .trim();
            found.push((format!("{path}: {head}"), lines[start..at + item].join(" ")));
        }
    }
    found
}

#[test]
fn hidden_items_are_behind_the_kernel_feature() {
    let found: BTreeSet<String> = marked("#[doc(hidden)]")
        .into_iter()
        .filter(|(_, attributes)| !attributes.contains("feature = \"kernel\""))
        .map(|(item, _)| item)
        .collect();
    ratchet(
        "hidden.txt",
        &found,
        "public but hidden: what only the kernel uses goes behind #[cfg(feature = \"kernel\")]; what a macro of the SDK uses is listed with that reason",
    );
}

#[test]
fn no_deprecated_item_reaches_a_release() {
    let manifest = std::fs::read_to_string(root().join("Cargo.toml")).expect("Cargo.toml");
    let version = manifest
        .lines()
        .find_map(|line| line.strip_prefix("version = \""))
        .and_then(|rest| rest.split('"').next())
        .expect("the workspace's version");
    if version.contains('-') {
        return;
    }
    let found: Vec<String> = marked("#[deprecated")
        .into_iter()
        .map(|(item, _)| item)
        .collect();
    assert!(
        found.is_empty(),
        "{version} is a release: remove what is deprecated\n{}",
        found.join("\n")
    );
}
