//! Every crate of PaperPaw's own that the X4 firmware is built from is named in the
//! release profile of `shells/xteink/Cargo.toml`, as hot (`"s"`) or cold (`"z"`), once
//! (#258). A new crate must be put in one of the two lists: left out it would get the
//! profile's level without anyone having decided, and the image's size budget
//! (`shells/xteink/size-budget.txt`) is measured with the cold ones small.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_checks::root;

/// The packages of PaperPaw's own in a lock file: `name = "paperpaw-…"`. The two
/// patched crates without that prefix (`fatfs`, `trouble-host`: `patches/`) are not
/// looked at: they stay unnamed, at the profile's `"s"`.
fn own_packages(lock: &str) -> Vec<String> {
    let mut found: Vec<String> = lock
        .lines()
        .filter_map(|line| line.strip_prefix("name = \"")?.strip_suffix('"'))
        .filter(|name| name.starts_with("paperpaw-"))
        .map(str::to_owned)
        .collect();
    found.sort();
    found.dedup();
    found
}

/// What `[profile.release.package]` of a manifest names: each package with its level,
/// in the file's order. A line that is not `name = { opt-level = … }` is a problem.
fn named(manifest: &str) -> Result<Vec<(String, String)>, String> {
    let mut found = Vec::new();
    let mut inside = false;
    for line in manifest.lines().map(str::trim) {
        if line.starts_with('[') {
            inside = line == "[profile.release.package]";
            continue;
        }
        if !inside || line.is_empty() || line.starts_with('#') {
            continue;
        }
        let entry = line
            .split_once(" = { opt-level = ")
            .and_then(|(name, rest)| Some((name, rest.strip_suffix(" }")?)));
        let Some((name, level)) = entry else {
            return Err(format!("not `name = {{ opt-level = … }}`: {line}"));
        };
        found.push((name.to_owned(), level.to_owned()));
    }
    Ok(found)
}

/// What is wrong with a manifest's list for a lock file's packages.
fn problems(manifest: &str, lock: &str) -> Vec<String> {
    let named = match named(manifest) {
        Ok(named) => named,
        Err(problem) => return vec![problem],
    };
    let own = own_packages(lock);
    let mut found = Vec::new();
    for package in &own {
        match named.iter().filter(|(name, _)| name == package).count() {
            0 => found.push(format!("{package}: not named (hot \"s\" or cold \"z\"?)")),
            1 => {}
            _ => found.push(format!("{package}: named more than once")),
        }
    }
    for (name, level) in &named {
        if !name.starts_with("paperpaw-") {
            continue;
        }
        if !own.contains(name) {
            found.push(format!(
                "{name}: named, but the firmware is not built from it"
            ));
        }
        if level != "\"s\"" && level != "\"z\"" {
            found.push(format!("{name}: level {level}, not \"s\" or \"z\""));
        }
    }
    found
}

#[test]
fn every_crate_of_the_firmware_is_named_hot_or_cold() {
    let folder = root().join("shells/xteink");
    let manifest = std::fs::read_to_string(folder.join("Cargo.toml")).expect("readable");
    let lock = std::fs::read_to_string(folder.join("Cargo.lock")).expect("readable");
    assert!(
        own_packages(&lock).len() > 40,
        "the lock file names PaperPaw's crates"
    );
    let found = problems(&manifest, &lock);
    assert!(
        found.is_empty(),
        "shells/xteink/Cargo.toml, [profile.release.package]:\n{}",
        found.join("\n")
    );
}

#[test]
fn the_check_finds_what_is_wrong() {
    let lock = "[[package]]\nname = \"paperpaw-a\"\n\n[[package]]\nname = \"paperpaw-b\"\n\n[[package]]\nname = \"other\"\n";
    let good = "[profile.release.package]\n# A note.\nother = { opt-level = 3 }\npaperpaw-a = { opt-level = \"s\" }\npaperpaw-b = { opt-level = \"z\" }\n\n[profile.release]\nopt-level = \"s\"\n";
    assert_eq!(problems(good, lock), Vec::<String>::new());
    let missing = good.replace("paperpaw-b = { opt-level = \"z\" }\n", "");
    assert_eq!(
        problems(&missing, lock),
        ["paperpaw-b: not named (hot \"s\" or cold \"z\"?)"]
    );
    let twice = good.replace("# A note.", "paperpaw-a = { opt-level = \"z\" }");
    assert_eq!(problems(&twice, lock), ["paperpaw-a: named more than once"]);
    let gone = good.replace("# A note.", "paperpaw-c = { opt-level = \"z\" }");
    assert_eq!(
        problems(&gone, lock),
        ["paperpaw-c: named, but the firmware is not built from it"]
    );
    let fast = good.replace("\"s\" }", "3 }");
    assert_eq!(
        problems(&fast, lock),
        ["paperpaw-a: level 3, not \"s\" or \"z\""]
    );
    let odd = good.replace("# A note.", "paperpaw-a.opt-level = \"z\"");
    assert_eq!(problems(&odd, lock).len(), 1);
}
