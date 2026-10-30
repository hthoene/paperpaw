//! The plugin interface (`native::INTERFACE`, ADR 0026) holds as long as
//! what plugins see stays as `interface.txt` and the folder `interface/`
//! keep it:
//!
//! - the items and the shapes of the types of every crate that crosses
//!   (`interface/crates.txt`), as each crate's `api.txt` and `shape.txt`
//!   say (`tools/interface.py` writes them);
//! - the linker script plugins are linked with;
//! - the compilers of the release.
//!
//! What changed is raised by hand: by default every change raises the
//! interface. Only what cannot change what a plugin built before relies
//! on goes without: a new type, module or re-export, a new function or
//! constant outside a trait, a new `impl`, and the fields, variants and
//! items of a type that is itself new. A field, a variant or an item of a
//! trait that was there, any change of order or of a private field, and
//! anything gone raise it. Either way the files are written anew:
//!
//! PAPERPAW_BLESS=1 cargo test -p paperpaw-sdk --test interface

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use paperpaw_sdk::native::{INTERFACE, TOOLCHAIN};

fn here(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(path)
}

/// A file of the repository, line ends as on Linux; `None` outside it
/// (the SDK as published, without the repository around it).
fn read(path: &str) -> Option<String> {
    std::fs::read_to_string(here(path))
        .ok()
        .map(|t| t.replace("\r\n", "\n"))
}

/// FNV-1a with 64 bits of `text`.
fn fnv(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, b| {
        (hash ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// `key = value` of the toolchain files the release builds with.
fn value(text: &str, key: &str) -> String {
    text.lines()
        .find_map(|line| {
            let (k, v) = line.split_once('=')?;
            (k.trim() == key).then(|| v.trim().trim_matches('"').to_string())
        })
        .unwrap_or_default()
}

/// The crates whose types cross, as `interface/crates.txt` names them.
fn crates() -> Vec<String> {
    read("interface/crates.txt")
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(String::from)
        .collect()
}

/// What plugins see besides the crates, as `interface.txt` writes it;
/// `None` outside the repository.
fn now() -> Option<String> {
    let linker = read("../paperpaw-native/paperpaw-plugin.x")?;
    let x4 = value(&read("../../shells/xteink/rust-toolchain.toml")?, "channel");
    let esp = value(
        &read("../../shells/xteink/versions.env")?,
        "ESP_RUST_VERSION",
    );
    let android = value(&read("../../shells/android/versions.env")?, "RUST_VERSION");
    let third = third_party().join(", ");
    Some(format!(
        "# What code plugins see at this plugin interface (ADR 0026), beside\n\
         # the crates of interface/: kept so that CI fails where it changes\n\
         # without native::INTERFACE raised. Written by `PAPERPAW_BLESS=1\n\
         # cargo test -p paperpaw-sdk --test interface`.\n\
         interface = {INTERFACE}\n\
         linker = {:016x}\n\
         x4 = rustc {x4}\n\
         x4-pro = esp {esp}\n\
         android = rustc {android}\n\
         third-party = {third}\n",
        fnv(&linker)
    ))
}

/// The crates of others the crossing crates depend on, as `name version`:
/// their types may lie inside those that cross, so each is pinned to one
/// version (`=x.y.z`), and a new one raises the interface. Panics where one
/// is not pinned so.
fn third_party() -> Vec<String> {
    let mut found = BTreeSet::new();
    for crate_ in crates() {
        let Some(manifest) = read(&format!("../{crate_}/Cargo.toml")) else {
            continue;
        };
        for line in dependencies(&manifest).lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with("paperpaw-") {
                continue;
            }
            let name = line.split([' ', '=', '.']).next().unwrap_or_default();
            let version = line
                .split_once("version = \"")
                .or_else(|| line.split_once("= \""))
                .and_then(|(_, rest)| rest.split('"').next())
                .unwrap_or_default();
            let exact = version.strip_prefix('=').unwrap_or_else(|| {
                panic!("{crate_} takes {name} as {version:?}: pin it, as `={version}`")
            });
            found.insert(format!("{name} {exact}"));
        }
    }
    found.into_iter().collect()
}

/// A manifest's `[dependencies]`, and those of some targets only
/// (`[target.'cfg(…)'.dependencies]`), one after the other.
fn dependencies(manifest: &str) -> String {
    let mut found = String::new();
    for section in manifest.split("\n[").skip(1) {
        let (head, body) = section.split_once('\n').unwrap_or((section, ""));
        let head = head.trim_end();
        if head == "dependencies]"
            || (head.starts_with("target.") && head.ends_with(".dependencies]"))
        {
            found.push_str(body);
            found.push('\n');
        }
    }
    found
}

/// The path an item's line names: its first path, generics and all after
/// them left out.
fn path_of(text: &str) -> &str {
    let mut end = 0;
    let bytes = text.as_bytes();
    while end < bytes.len() {
        let b = bytes[end];
        if b.is_ascii_alphanumeric() || b == b'_' {
            end += 1;
        } else if text[end..].starts_with("::") {
            end += 2;
        } else {
            break;
        }
    }
    &text[..end]
}

/// The path without its last part: the type, trait or module that holds it.
fn parent(path: &str) -> &str {
    path.rsplit_once("::").map_or("", |(parent, _)| parent)
}

/// The lines of a crate's `shape.txt` by what they say: a type by its
/// kind and path (`struct a::B`), a constant by its path and the trait
/// it belongs to (`const a::B::C (a::T)`).
fn types(shape: &str) -> BTreeMap<String, &str> {
    shape
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
        .filter_map(|line| {
            let (kind, rest) = line.split_once(' ')?;
            let name = if kind == "const" {
                rest.split_once(": ").map_or(rest, |(name, _)| name)
            } else {
                path_of(rest)
            };
            Some((format!("{kind} {name}"), line))
        })
        .collect()
}

/// Whether `kept` has a type of this path, of any kind.
fn has_type(kept: &BTreeMap<String, &str>, path: &str) -> bool {
    ["struct", "enum", "union", "trait"]
        .iter()
        .any(|kind| kept.contains_key(&format!("{kind} {path}")))
}

/// Whether an item's line, new in `api.txt`, leaves what a plugin built
/// before relies on as it was: a new item that is not part of a trait or
/// of a type that was there (`kept`, the types as they were).
fn is_free(line: &str, kept: &BTreeMap<String, &str>) -> bool {
    let mut rest = line;
    while let Some(after) = rest.strip_prefix("#[") {
        rest = after.split_once("] ").map_or("", |(_, r)| r);
    }
    if rest.starts_with("impl") {
        return true;
    }
    let Some(rest) = rest.strip_prefix("pub ") else {
        return false;
    };
    let is_trait = |path: &str| kept.contains_key(&format!("trait {path}"));
    for keyword in [
        "struct ", "enum ", "union ", "trait ", "mod ", "use ", "macro ", "static ",
    ] {
        if rest.starts_with(keyword) {
            return true;
        }
    }
    let mut item = rest;
    for keyword in [
        "const ",
        "unsafe ",
        "async ",
        "extern \"C\" ",
        "fn ",
        "type ",
    ] {
        item = item.strip_prefix(keyword).unwrap_or(item);
    }
    if item.len() < rest.len() {
        // A function, constant or type alias: new outside a trait that
        // was there (a trait's would be a new item of its table).
        return !is_trait(parent(path_of(item)));
    }
    // A field or a variant: free only in a type that is itself new.
    let path = path_of(rest);
    let holder = parent(path);
    !has_type(kept, holder) && !has_type(kept, parent(holder))
}

/// What changed in one crate from `kept` to `now` (its `api.txt` and
/// `shape.txt`): what breaks plugins built before, and what is only new.
#[derive(Default)]
struct Changes {
    breaking: Vec<String>,
    new: Vec<String>,
}

/// The shape of the firmware's table, which grows at its end: fields
/// after `build_len`, the last of interface 5, are new, not changed
/// (`native::LAYOUT` digests only the fields up to it, and a plugin
/// that uses a later one says so in its `needs_api`).
const API_SHAPE: &str = "struct paperpaw_native::interface::Api";
const API_FIELD: &str = "pub paperpaw_native::Api::";
const API_LAST: &str = ", build_len: u32";

/// Whether `path` is a constant that `tools/shared-table.py` derives from
/// `shared.list` and the signatures (`paperpaw_table::index`): the
/// revisions, the hash of the tables and the number of entries of a
/// family. They change where entries are appended or a family is bumped,
/// which the pinned list and signatures (`shared.list`, `table.sig`:
/// lines only appended) guard.
fn table_index_derived(crate_: &str, path: &str) -> bool {
    let Some(name) = path.strip_prefix("const paperpaw_table::index::") else {
        return false;
    };
    crate_ == "paperpaw-table"
        && matches!(
            name.rsplit("::").next(),
            Some("REVISIONS" | "OLDEST" | "HASH" | "COUNT")
        )
}

/// Whether `after` is `before`, the table's shape, with fields appended
/// after the last of interface 5.
fn api_appended(path: &str, before: &str, after: &str) -> bool {
    let Some(open) = before.strip_suffix(" }") else {
        return false;
    };
    path == API_SHAPE
        && open.ends_with(API_LAST)
        && after.ends_with(" }")
        && after
            .strip_prefix(open)
            .is_some_and(|rest| rest.starts_with(", "))
}

/// Whether an `api.txt` line is a field of the firmware's table that is
/// new where the kept shape has all fields of interface 5.
fn api_field_appended(line: &str, kept: &BTreeMap<String, &str>) -> bool {
    let Some(field) = line.strip_prefix(API_FIELD) else {
        return false;
    };
    let name = field.split(':').next().unwrap_or_default();
    kept.get(API_SHAPE)
        .is_some_and(|shape| shape.contains(API_LAST) && !shape.contains(&format!(" {name}: ")))
}

/// The lists that the plugin interface pins beside the crates, as
/// `interface/<name>` keeps them: they only grow at their end.
/// - `shared.list`: the shared function tables (`paperpaw-table`), whose
///   order is where plugins find each function;
/// - `table.sig`: the signature of each of those functions;
/// - `lang.list`: the scripts and regions whose index a `Lang` keeps;
/// - `i18n.list`: the plural rules, digits and calendars whose index a
///   catalog keeps.
fn pins() -> Option<Vec<(&'static str, String)>> {
    let lines = |text: String| -> String {
        text.lines()
            .map(|l| l.split('#').next().unwrap_or_default().trim())
            .filter(|l| !l.is_empty())
            .map(|l| format!("{l}\n"))
            .collect()
    };
    let lang: String = paperpaw_lang::scripts()
        .map(|s| format!("script {s}\n"))
        .chain(paperpaw_lang::regions().map(|r| format!("region {r}\n")))
        .collect();
    let i18n: String = paperpaw_i18n::cldr_rules()
        .map(|r| format!("plural {r}\n"))
        .chain(
            paperpaw_i18n::DIGITS
                .iter()
                .map(|d| format!("digits {d}\n")),
        )
        .chain(
            paperpaw_i18n::CALENDARS
                .iter()
                .map(|c| format!("calendar {c}\n")),
        )
        .collect();
    Some(vec![
        ("shared.list", lines(read("../paperpaw-table/shared.list")?)),
        ("table.sig", lines(read("../paperpaw-table/table.sig")?)),
        ("lang.list", lang),
        ("i18n.list", i18n),
    ])
}

/// What changed in a pinned list from `kept` to `now`: breaking where
/// `kept` is not where `now` starts, new where `now` goes on.
fn pin_changes(name: &str, kept: &str, now: &str) -> Changes {
    let mut out = Changes::default();
    if !now.starts_with(kept) {
        out.breaking.push(format!(
            "{name}: entries changed or gone, not only appended"
        ));
    } else if now.len() > kept.len() {
        out.new
            .push(format!("{name}: appended: {}", now.len() - kept.len()));
    }
    out
}

fn changes(crate_: &str, kept: (&str, &str), now: (&str, &str)) -> Changes {
    let mut out = Changes::default();
    let (kept_types, now_types) = (types(kept.1), types(now.1));
    for (path, line) in &kept_types {
        match now_types.get(path) {
            None => out.breaking.push(format!("{crate_}: gone: {line}")),
            Some(now)
                if now != line
                    && !user_agent_release_only(path, line, now)
                    && !layout_cache_revision(crate_, path, line, now)
                    && !api_appended(path, line, now)
                    && !table_index_derived(crate_, path) =>
            {
                out.breaking
                    .push(format!("{crate_}: changed: {line}\n  now: {now}"));
            }
            Some(now) if now != line => out.new.push(format!("{crate_}: grew: {now}")),
            Some(_) => {}
        }
    }
    for (path, line) in &now_types {
        if !kept_types.contains_key(path) {
            out.new.push(format!("{crate_}: {line}"));
        }
    }
    let kept_api: BTreeSet<&str> = kept.0.lines().collect();
    let now_api: BTreeSet<&str> = now.0.lines().collect();
    for line in kept_api.difference(&now_api) {
        out.breaking
            .push(format!("{crate_}: gone or changed: {line}"));
    }
    for line in now_api.difference(&kept_api) {
        if is_free(line, &kept_types) || api_field_appended(line, &kept_types) {
            out.new.push(format!("{crate_}: {line}"));
        } else {
            out.breaking
                .push(format!("{crate_}: added to what was there: {line}"));
        }
    }
    out
}

/// The SDK release in an HTTP header is metadata, not a crossing layout.
/// Every other part of this constant stays guarded.
fn user_agent_release_only(path: &str, before: &str, after: &str) -> bool {
    let release = |line: &str| {
        let Some(version) = line
            .strip_prefix("const paperpaw_sdk::web::USER_AGENT: &str = \"PaperPaw/")
            .and_then(|v| v.strip_suffix(" (e-reader)\""))
        else {
            return false;
        };
        let core = version.split(['-', '+']).next().unwrap_or_default();
        let numbers: Vec<&str> = core.split('.').collect();
        let valid_core = numbers.len() == 3
            && numbers.iter().all(|n| {
                !n.is_empty()
                    && n.bytes().all(|b| b.is_ascii_digit())
                    && (n.len() == 1 || !n.starts_with('0'))
            });
        let metadata = version.split_once('+').map(|(_, text)| text);
        let valid_metadata = metadata.is_none_or(|text| {
            text.split('.').all(|part| {
                !part.is_empty() && part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            })
        });
        valid_core
            && valid_metadata
            && version.trim() == version
            && paperpaw_sdk::Version::parse(version).is_some()
    };
    path == "const paperpaw_sdk::web::USER_AGENT" && release(before) && release(after)
}

/// A newer layout cache key keeps old page indexes apart from new ones.
/// It changes neither a crossing type nor how its fields are interpreted.
fn layout_cache_revision(crate_: &str, path: &str, before: &str, after: &str) -> bool {
    if crate_ != "paperpaw-layout" || path != "const paperpaw_layout::VERSION" {
        return false;
    }
    let revision = |line: &str| -> Option<u32> {
        let value = line.strip_prefix("const paperpaw_layout::VERSION: u32 = ")?;
        // Rustdoc evaluates the constant as a typed literal in shape.txt.
        let value = value.strip_suffix("u32").unwrap_or(value);
        if value.is_empty()
            || !value.bytes().all(|b| b.is_ascii_digit())
            || (value.len() > 1 && value.starts_with('0'))
        {
            return None;
        }
        value.parse().ok()
    };
    matches!((revision(before), revision(after)), (Some(old), Some(new)) if new > old)
}

#[test]
fn only_what_the_table_list_derives_may_change_in_the_table_index() {
    for name in ["REVISIONS", "OLDEST", "HASH", "text::COUNT", "ui::COUNT"] {
        let path = format!("const paperpaw_table::index::{name}");
        assert!(table_index_derived("paperpaw-table", &path), "{name}");
        assert!(
            !table_index_derived("paperpaw-sdk", &path),
            "{name}, other crate"
        );
    }
    // An index is the plugin interface: it never changes its number.
    assert!(!table_index_derived(
        "paperpaw-table",
        "const paperpaw_table::index::text::TEXT_MEASURE"
    ));
    assert!(!table_index_derived(
        "paperpaw-table",
        "const paperpaw_table::FAMILIES"
    ));
}

#[test]
fn layout_cache_revisions_only_increase_as_u32_metadata() {
    let shape = |value: &str| format!("const paperpaw_layout::VERSION: u32 = {value}");
    let path = "const paperpaw_layout::VERSION";
    for (before, after) in [
        ("0", "1"),
        ("7", "8"),
        ("7", "42"),
        ("4294967294", "4294967295"),
        ("7u32", "8u32"),
        ("7", "8u32"),
        ("7u32", "8"),
        ("4294967294u32", "4294967295u32"),
    ] {
        let (before, after) = (shape(before), shape(after));
        assert!(layout_cache_revision(
            "paperpaw-layout",
            path,
            &before,
            &after
        ));
        assert!(
            changes("paperpaw-layout", ("", &before), ("", &after))
                .breaking
                .is_empty(),
            "{before} to {after}"
        );
    }
    let before = shape("7");
    // An unchanged value is handled by the ordinary equality check.
    assert!(!layout_cache_revision(
        "paperpaw-layout",
        path,
        &before,
        &before
    ));
    assert!(
        changes("paperpaw-layout", ("", &before), ("", &before))
            .breaking
            .is_empty()
    );
    for value in [
        "6",
        "0",
        "4294967296",
        "-1",
        "+8",
        "8.0",
        "8u64",
        "8u32u32",
        "4294967296u32",
        "6u32",
        "08u32",
        "8 u32",
        "08",
        " 8",
        "8 ",
        "",
    ] {
        let after = shape(value);
        assert!(!layout_cache_revision(
            "paperpaw-layout",
            path,
            &before,
            &after
        ));
        assert!(
            !changes("paperpaw-layout", ("", &before), ("", &after))
                .breaking
                .is_empty(),
            "{after}"
        );
    }
    for value in ["07", "-1", "+7", "7u64", "7u32u32", "4294967296u32", ""] {
        let (before, after) = (shape(value), shape("8"));
        assert!(!layout_cache_revision(
            "paperpaw-layout",
            path,
            &before,
            &after
        ));
        assert!(
            !changes("paperpaw-layout", ("", &before), ("", &after))
                .breaking
                .is_empty()
        );
    }
}

#[test]
fn layout_cache_revision_accepts_the_generated_shape_literal() {
    let shape = include_str!("../../paperpaw-layout/shape.txt");
    let before = shape
        .lines()
        .find(|line| line.starts_with("const paperpaw_layout::VERSION: "))
        .expect("the generated layout cache constant");
    let (declaration, literal) = before.split_once(" = ").unwrap();
    let revision: u32 = literal.trim_end_matches("u32").parse().unwrap();
    let after = format!("{declaration} = {}u32", revision.checked_add(1).unwrap());
    assert!(
        changes("paperpaw-layout", ("", before), ("", &after))
            .breaking
            .is_empty()
    );
}

#[test]
fn layout_cache_revision_keeps_other_interface_changes_guarded() {
    let before = "const paperpaw_layout::VERSION: u32 = 7";
    let after = "const paperpaw_layout::VERSION: u32 = 8";
    for crate_ in ["paperpaw-sdk", "paperpaw-text", "other", ""] {
        assert!(
            !changes(crate_, ("", before), ("", after))
                .breaking
                .is_empty()
        );
    }
    for (before, after) in [
        (
            before.replace("VERSION", "OTHER"),
            after.replace("VERSION", "OTHER"),
        ),
        (
            before.replace("VERSION", "CACHE::VERSION"),
            after.replace("VERSION", "CACHE::VERSION"),
        ),
        (before.replace("u32", "u64"), after.to_string()),
        (before.to_string(), after.replace("u32", "u64")),
        (
            before.replace("u32", "usize"),
            after.replace("u32", "usize"),
        ),
        (before.to_string(), String::new()),
    ] {
        assert!(
            !changes("paperpaw-layout", ("", &before), ("", &after))
                .breaking
                .is_empty(),
            "{before} to {after}"
        );
    }
    let kept = format!("{before}\nstruct paperpaw_layout::Page {{count: u32}}");
    let now = format!("{after}\nstruct paperpaw_layout::Page {{count: u64}}");
    let changed = changes("paperpaw-layout", ("", &kept), ("", &now));
    assert_eq!(changed.breaking.len(), 1);
    assert!(changed.breaking[0].contains("struct paperpaw_layout::Page"));
}

#[test]
fn only_the_user_agent_s_release_is_interface_metadata() {
    let before =
        "const paperpaw_sdk::web::USER_AGENT: &str = \"PaperPaw/0.5.0-beta.8 (e-reader)\"\n";
    let changed = |line: &str| changes("paperpaw-sdk", ("", before.trim()), ("", line));
    for version in ["0.5.0-beta.9", "0.5.0", "0.5.0+build.1"] {
        let after = before.replace("0.5.0-beta.8", version);
        assert!(changed(after.trim()).breaking.is_empty(), "{after}");
    }
    for after in [
        before.replace("PaperPaw/", "OtherPaw/"),
        before.replace("(e-reader)", "(browser)"),
        before.replace("&str", "String"),
        before.replace("0.5.0-beta.8", "0.5"),
        before.replace("0.5.0-beta.8", "00.5.0"),
        before.replace("0.5.0-beta.8", "0.5.0+build..1"),
        before.replace("0.5.0-beta.8", "0.5.0-beta.9 "),
    ] {
        assert!(!changed(after.trim()).breaking.is_empty(), "{after}");
    }
    let other = before.replace("web::USER_AGENT", "web::OTHER_AGENT");
    let after = other.replace("0.5.0-beta.8", "0.5.0-beta.9");
    assert!(
        !changes("paperpaw-sdk", ("", other.trim()), ("", after.trim()))
            .breaking
            .is_empty()
    );
}

#[test]
fn what_plugins_see_changes_only_with_the_interface() {
    let Some(now) = now() else {
        return;
    };
    let bless = std::env::var_os("PAPERPAW_BLESS").is_some();
    let kept = read("interface.txt").unwrap_or_default();
    let kept_interface: u16 = value(&kept, "interface").parse().unwrap_or(0);
    let mut all = Changes::default();
    let mut files = Vec::new();
    for crate_ in crates() {
        let folder = format!("../{crate_}");
        let api = read(&format!("{folder}/api.txt")).expect("api.txt: python3 tools/interface.py");
        let shape =
            read(&format!("{folder}/shape.txt")).expect("shape.txt: python3 tools/interface.py");
        let kept_api = read(&format!("interface/{crate_}.api")).unwrap_or_default();
        let kept_shape = read(&format!("interface/{crate_}.shape")).unwrap_or_default();
        let found = changes(&crate_, (&kept_api, &kept_shape), (&api, &shape));
        all.breaking.extend(found.breaking);
        all.new.extend(found.new);
        files.push((format!("interface/{crate_}.api"), api));
        files.push((format!("interface/{crate_}.shape"), shape));
    }
    for (name, now) in pins().unwrap_or_default() {
        let kept = read(&format!("interface/{name}")).unwrap_or_default();
        let found = pin_changes(name, &kept, &now);
        all.breaking.extend(found.breaking);
        all.new.extend(found.new);
        files.push((format!("interface/{name}"), now));
    }
    let rest_same = value(&now, "linker") == value(&kept, "linker")
        && ["x4", "x4-pro", "android", "third-party"]
            .iter()
            .all(|key| value(&now, key) == value(&kept, key));
    let breaking = !all.breaking.is_empty() || !rest_same;
    if bless {
        assert!(
            !breaking || INTERFACE > kept_interface,
            "what plugins see changed: raise native::INTERFACE (now {INTERFACE}) first:\n{}",
            all.breaking.join("\n")
        );
        std::fs::write(here("interface.txt"), &now).unwrap();
        for (path, text) in files {
            std::fs::write(here(&path), text).unwrap();
        }
        return;
    }
    assert_eq!(
        INTERFACE, kept_interface,
        "native::INTERFACE is {INTERFACE}, interface.txt says {kept_interface}: write it anew \
         (PAPERPAW_BLESS=1 cargo test -p paperpaw-sdk --test interface)"
    );
    assert!(
        !breaking,
        "what plugins built before rely on changed, so they would not run: raise \
         native::INTERFACE to {} and write the files anew (PAPERPAW_BLESS=1 cargo test -p \
         paperpaw-sdk --test interface). The linker script, the compilers and the crates of \
         others {}. Changed:\n{}",
        INTERFACE + 1,
        if rest_same {
            "are as they were"
        } else {
            "changed"
        },
        all.breaking.join("\n")
    );
    assert!(
        all.new.is_empty(),
        "only new items, which plugins built before do not use: write the files anew \
         (PAPERPAW_BLESS=1 cargo test -p paperpaw-sdk --test interface), INTERFACE stays. \
         New:\n{}",
        all.new.join("\n")
    );
    assert_eq!(now, kept);
}

/// Every crate of PaperPaw that a crate of `interface/crates.txt` depends
/// on is named there too: its types may cross as well. The SDK names the
/// same (`native::CROSSING_CRATES`).
#[test]
fn every_crate_below_the_sdk_is_kept() {
    let listed = crates();
    if listed.is_empty() {
        return;
    }
    let named: Vec<String> = paperpaw_sdk::native::CROSSING_CRATES
        .iter()
        .map(|c| c.replace('_', "-"))
        .collect();
    assert_eq!(
        named, listed,
        "native::CROSSING_CRATES and interface/crates.txt"
    );
    for crate_ in &listed {
        let manifest = read(&format!("../{crate_}/Cargo.toml")).unwrap();
        for line in dependencies(&manifest).lines() {
            let name = line.split(['.', '=', ' ']).next().unwrap_or_default();
            if name.starts_with("paperpaw-") {
                assert!(
                    listed.iter().any(|l| l == name),
                    "{crate_} depends on {name}: add it to interface/crates.txt"
                );
            }
        }
    }
}

/// The compilers the SDK names for the chips are the release's, and every
/// plugin project of the repository builds with the X4's.
#[test]
fn the_toolchains_named_are_the_releases() {
    let (Some(toolchain), Some(versions)) = (
        read("../../shells/xteink/rust-toolchain.toml"),
        read("../../shells/xteink/versions.env"),
    ) else {
        return;
    };
    let x4 = value(&toolchain, "channel");
    let esp = value(&versions, "ESP_RUST_VERSION");
    let src = read("src/native.rs").unwrap();
    assert!(
        src.contains(&format!("\"rustc {x4}, riscv32imc-unknown-none-elf\"")),
        "native::TOOLCHAIN names rustc {x4}"
    );
    assert!(
        src.contains(&format!("\"esp {esp}, xtensa-esp32s3-none-elf\"")),
        "native::TOOLCHAIN names esp {esp}"
    );
    assert!(!TOOLCHAIN.is_empty());
    for project in [
        "../..",
        "../../samples/hello-native",
        "../../tools/plugin-files",
    ] {
        let pinned = value(
            &read(&format!("{project}/rust-toolchain.toml")).unwrap(),
            "channel",
        );
        assert_eq!(pinned, x4, "{project}/rust-toolchain.toml");
    }
    // The nightly that writes api.txt and shape.txt is CI's.
    let script = read("../../tools/interface.py").unwrap();
    let ci = read("../../.github/workflows/ci.yml").unwrap();
    let nightly = value(&script, "NIGHTLY");
    assert!(
        ci.contains(&format!("NIGHTLY: {nightly}")),
        "tools/interface.py and ci.yml name the same nightly"
    );
    let public_api = value(&script, "PUBLIC_API");
    assert!(
        ci.contains(&format!("cargo-public-api@{public_api}")),
        "tools/interface.py and ci.yml name the same cargo-public-api"
    );
}

/// Every plugin project of the repository links with the format's own
/// linker script.
#[test]
fn plugin_projects_link_with_the_formats_script() {
    let Some(script) = read("../paperpaw-native/paperpaw-plugin.x") else {
        return;
    };
    for project in ["../../samples/hello-native", "../../tools/plugin-files"] {
        let copy = read(&format!("{project}/paperpaw-plugin.x"));
        assert_eq!(
            copy.as_deref(),
            Some(script.as_str()),
            "{project}/paperpaw-plugin.x"
        );
    }
}

/// What the gate takes as new and what as changed.
#[test]
fn only_what_cannot_matter_to_a_plugin_is_free() {
    let kept_shape = "\
struct a::S { x: u8 }
enum a::E { One }
trait a::T { function f }
";
    let kept = types(kept_shape);
    for line in [
        "pub fn a::free() -> u8",
        "pub fn a::S::new() -> a::S",
        "pub const a::LIMIT: u8",
        "pub const fn a::S::make() -> a::S",
        "pub struct a::New",
        "pub a::New::y: u16",
        "pub a::Fresh::Variant",
        "#[non_exhaustive] pub enum a::Fresh",
        "impl core::fmt::Debug for a::S",
        "pub use a::S",
        "pub mod a::m",
    ] {
        assert!(is_free(line, &kept), "{line}");
    }
    for line in [
        "pub a::S::y: u16",
        "pub a::E::Two",
        "#[non_exhaustive] pub a::E::Two(u8)",
        "pub fn a::T::g(&self)",
        "pub const a::T::C: u8",
        "pub type a::T::Out",
        "something unknown",
    ] {
        assert!(!is_free(line, &kept), "{line}");
    }
    // A field moved, a private one added, the order of variants.
    let found = changes(
        "a",
        ("", kept_shape),
        (
            "",
            "struct a::S { y: u8, x: u8 }\nenum a::E { One }\ntrait a::T { function f }\n",
        ),
    );
    assert_eq!(found.breaking.len(), 1);
    let found = changes("a", ("pub fn a::gone()", kept_shape), ("", kept_shape));
    assert_eq!(found.breaking.len(), 1);
    // A constant's value, and one of a generic type beside the type.
    let with = |value: u8| {
        format!("{kept_shape}const a::S<T>::MAX: u8 = {value}\nconst a::LIMIT: u8 = 3\n")
    };
    let found = changes("a", ("", &with(1)), ("", &with(2)));
    assert_eq!(found.breaking.len(), 1, "{:?}", found.breaking);
    assert!(types(&with(1)).contains_key("struct a::S"));
}

/// The firmware's table grows at its end without a raise, after the
/// fields of interface 5 only; anything else in it is a change.
#[test]
fn the_firmware_s_table_grows_only_at_its_end() {
    let kept = "struct paperpaw_native::interface::Api #{} { size: u32, build_len: u32 }";
    let grown =
        "struct paperpaw_native::interface::Api #{} { size: u32, build_len: u32, more: u8 }";
    let moved =
        "struct paperpaw_native::interface::Api #{} { more: u8, size: u32, build_len: u32 }";
    let changed = |before: &str, after: &str| changes("paperpaw-native", ("", before), ("", after));
    assert!(changed(kept, grown).breaking.is_empty());
    assert_eq!(changed(kept, grown).new.len(), 1);
    assert_eq!(changed(kept, moved).breaking.len(), 1);
    assert_eq!(changed(grown, kept).breaking.len(), 1);
    // Before interface 5 (no `build_len`), every field was a raise.
    let old = "struct paperpaw_native::interface::Api #{} { size: u32 }";
    let old_grown = "struct paperpaw_native::interface::Api #{} { size: u32, more: u8 }";
    assert_eq!(changed(old, old_grown).breaking.len(), 1);
    let kept_types = types(kept);
    assert!(api_field_appended(
        "pub paperpaw_native::Api::more: u8",
        &kept_types
    ));
    assert!(!api_field_appended(
        "pub paperpaw_native::Api::size: u64",
        &kept_types
    ));
    assert!(!api_field_appended(
        "pub paperpaw_native::Other::more: u8",
        &kept_types
    ));
}

/// A pinned list may only grow at its end.
#[test]
fn pinned_lists_only_grow_at_their_end() {
    assert!(pin_changes("x", "a\nb\n", "a\nb\n").breaking.is_empty());
    assert_eq!(pin_changes("x", "a\nb\n", "a\nb\nc\n").new.len(), 1);
    assert_eq!(pin_changes("x", "a\nb\n", "a\nc\nb\n").breaking.len(), 1);
    assert_eq!(pin_changes("x", "a\nb\n", "a\n").breaking.len(), 1);
}
