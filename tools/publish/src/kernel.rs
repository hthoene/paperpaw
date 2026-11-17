//! A plugin with code must not switch on the SDK's `kernel` feature: it
//! shows the old API that only the firmware and the shells use (ADR 0020),
//! and a plugin built with it is not one the store takes. The feature can be
//! on in the plugin's own `Cargo.toml` or through a crate it depends on, so
//! the check asks Cargo for the feature graph (`cargo tree -e features`) and
//! names the path. Where Cargo cannot resolve the repository (no network, a
//! `Cargo.lock` that does not fit), it reads the `Cargo.toml` for the direct
//! case only, and says that dependencies were not checked.

use std::path::Path;
use std::process::Command;

use crate::Problem;

/// First-party plugins that need the feature: they are built into the
/// firmware and the emulator, never published through the store. Explicit,
/// and kept equal to the plugins of this repository that name it (a test
/// compares).
pub const FIRST_PARTY: &[&str] = &[
    "about",
    "aids",
    "bluetooth",
    "dictionary",
    "file-manager",
    "files",
    "fonts-browser",
    "fonts-default",
    "help",
    "home",
    "keyboard",
    "keys",
    "kosync",
    "lock",
    "mail",
    "menu",
    "menu-editor",
    "news",
    "notes-vault",
    "opds",
    "picture-home",
    "plugin-manager",
    "power",
    "profiles",
    "reader",
    "reading-goal",
    "rules",
    "settings",
    "setup",
    "sleep",
    "store",
    "transfer",
    "update",
    "vocabulary",
    "widgets",
    "wifi",
];

/// Checks the plugin `id`'s repository at `dir`.
pub fn check(dir: &Path, id: &str, add: &mut dyn FnMut(Problem)) {
    if FIRST_PARTY.contains(&id) {
        return;
    }
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let output = Command::new(&cargo)
        .args(["tree", "--locked", "-e", "features", "-i", "paperpaw-sdk"])
        .args(["--prefix", "depth", "--manifest-path"])
        .arg(dir.join("Cargo.toml"))
        .output();
    match output {
        Ok(out) if out.status.success() => {
            let roots = Command::new(&cargo)
                .args(["tree", "--locked", "--depth", "0", "--prefix", "none"])
                .arg("--manifest-path")
                .arg(dir.join("Cargo.toml"))
                .output()
                .map(|o| {
                    String::from_utf8_lossy(&o.stdout)
                        .lines()
                        .filter_map(|l| l.split(' ').next().map(str::to_string))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            if let Some(path) = path_of(&String::from_utf8_lossy(&out.stdout), &roots) {
                add(refused(id, &path));
            }
        }
        _ => {
            let manifest = std::fs::read_to_string(dir.join("Cargo.toml")).unwrap_or_default();
            if direct(&manifest) {
                add(refused(id, &[]));
            } else {
                add(Problem::warning(
                    "source.kernel",
                    "Cargo.toml",
                    "could not read the feature graph with `cargo tree --locked`, so the crates \
                     this plugin depends on were not checked for paperpaw-sdk/kernel: run it \
                     where Cargo can resolve the repository",
                ));
            }
        }
    }
}

fn refused(id: &str, via: &[String]) -> Problem {
    let how = if via.is_empty() {
        "itself (a `kernel` in the features of paperpaw-sdk)".to_string()
    } else {
        format!("via {}", via.join(" -> "))
    };
    Problem::error(
        "source.kernel",
        "Cargo.toml",
        format!(
            "plugin {id} turns on paperpaw-sdk/kernel {how}: that part of the SDK is for the \
             firmware only and the store takes no plugin built with it; leave the feature out \
             and the crates that switch it on"
        ),
    )
}

/// A `kernel` in the features of `paperpaw-sdk` in a `Cargo.toml`, in the
/// inline form or as a table of its own.
fn direct(cargo: &str) -> bool {
    let mut in_sdk_table = false;
    cargo.lines().any(|line| {
        let line = line.trim();
        if line.starts_with('[') {
            in_sdk_table = line.ends_with(".paperpaw-sdk]");
            return false;
        }
        let features = line.contains("kernel")
            && (line.starts_with("paperpaw-sdk") || in_sdk_table && line.starts_with("features"));
        features || line.contains("\"paperpaw-sdk/kernel\"")
    })
}

/// The crates between the `kernel` node of the inverted tree of
/// `paperpaw-sdk` (`--prefix depth`: each line starts with its depth) and
/// the plugin's own packages (`roots`), along the first dependent at each
/// step: empty when a root itself turns it on. Cargo shows a crate it has
/// shown before as `(*)` without its dependents, so the path may end at
/// the crate that is not a root.
fn path_of(tree: &str, roots: &[String]) -> Option<Vec<String>> {
    let lines: Vec<(usize, &str)> = tree
        .lines()
        .filter_map(|l| {
            let digits = l.len() - l.trim_start_matches(|c: char| c.is_ascii_digit()).len();
            Some((l[..digits].parse().ok()?, &l[digits..]))
        })
        .collect();
    let at = lines
        .iter()
        .position(|(_, t)| t.starts_with("paperpaw-sdk feature \"kernel\""))?;
    let name = |text: &str| text.split(' ').next().unwrap_or_default().to_string();
    let first = lines[at].0 + 1;
    let own = |text: &str| roots.contains(&name(text));
    let mut direct = lines[at + 1..]
        .iter()
        .take_while(|(d, _)| *d >= first)
        .filter(|(d, t)| *d == first && !t.contains(" feature \""));
    if direct.any(|(_, t)| own(t)) {
        return Some(Vec::new());
    }
    let mut names: Vec<String> = Vec::new();
    let mut depth = lines[at].0;
    for (d, text) in &lines[at + 1..] {
        if *d != depth + 1 || own(text) {
            break;
        }
        depth = *d;
        if !text.contains(" feature \"") {
            names.push(name(text));
        }
    }
    Some(names)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_path_is_read_from_the_inverted_tree() {
        let roots = ["clock".to_string()];
        let tree = "0paperpaw-sdk v0.5.0\n1paperpaw-sdk feature \"kernel\"\n2fonts-default v1.0.0\n\
                    3fonts-default feature \"default\"\n4clock v0.1.0\n2other v1.0.0\n3clock v0.1.0 (*)\n";
        assert_eq!(
            path_of(tree, &roots),
            Some(vec!["fonts-default".to_string()])
        );
        let elided =
            "0paperpaw-sdk v0.5.0\n1paperpaw-sdk feature \"kernel\"\n2fonts-default v1.0.0 (*)\n";
        assert_eq!(
            path_of(elided, &roots),
            Some(vec!["fonts-default".to_string()])
        );
        let direct = "0paperpaw-sdk v0.5.0\n1paperpaw-sdk feature \"kernel\"\n2fonts-default v1.0.0 (*)\n2clock v0.1.0\n";
        assert_eq!(path_of(direct, &roots), Some(vec![]));
        assert_eq!(
            path_of("0paperpaw-sdk v0.5.0\n1clock v0.1.0\n", &roots),
            None
        );
    }

    #[test]
    fn the_direct_case_is_read_from_the_manifest() {
        assert!(direct(
            "[dependencies]\npaperpaw-sdk = { version = \"1\", features = [\"kernel\"] }\n"
        ));
        assert!(direct(
            "[dependencies.paperpaw-sdk]\nversion = \"1\"\nfeatures = [\"kernel\"]\n"
        ));
        assert!(direct("[features]\nfull = [\"paperpaw-sdk/kernel\"]\n"));
        assert!(!direct(
            "[dependencies]\npaperpaw-sdk = \"1\"\nkernel = \"2\"\n"
        ));
    }

    /// The exempt list is the plugins of this repository that name the
    /// feature: a new one must be added on purpose, a gone one removed.
    #[test]
    fn the_exempt_list_is_the_first_party_plugins_that_name_the_feature() {
        let plugins = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins");
        let mut named: Vec<String> = std::fs::read_dir(plugins)
            .unwrap()
            .flatten()
            .filter(|e| {
                std::fs::read_to_string(e.path().join("Cargo.toml")).is_ok_and(|c| {
                    c.lines()
                        .any(|l| l.trim_start().starts_with("paperpaw-sdk") && l.contains("kernel"))
                })
            })
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        named.sort();
        assert_eq!(named, FIRST_PARTY);
    }
}
