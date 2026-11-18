//! A plugin with code as source (`paperpaw-dev package path/to/crate`):
//! what its Rust files show of the conventions in
//! `docs/plugin-conventions.md`. Warnings only. It reads the text of the
//! files, not their syntax, so it can be wrong where a name is built or
//! the id belongs to another plugin; each message says what it saw.

use std::path::{Path, PathBuf};

use crate::report::{Finding, Report};
use crate::target::Source;

/// The most an embedded file takes before it should be on the card.
const MAX_EMBEDDED: u64 = 4 * 1024;
/// Files embedded for what they are: texts and fixtures (`i18n`, `how`),
/// and the tests'.
const EMBEDDED_OK: [&str; 3] = ["/i18n/", "/how/", "/testdata/"];
/// What writes, in a line that names the plugin's installed folder.
const WRITES: [&str; 7] = [
    ".write(",
    "write_all(",
    ".create(",
    ".remove(",
    "save_to(",
    ".rename(",
    ".append(",
];

pub fn check(source: &Source, report: &mut Report) {
    let mut files = Vec::new();
    rust_files(&source.dir.join("src"), &mut files);
    files.sort();
    for path in files {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let place = path
            .strip_prefix(&source.dir)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        if is_test_file(&place) {
            continue;
        }
        for finding in scan(&source.id, &place, &text, path.parent(), &source.dir) {
            // A plugin built into the firmware says why it embeds files.
            if source.embedded_assets.is_some() && finding.check == "package.embedded-asset" {
                continue;
            }
            report.add(finding.plugin(source.id.clone()));
        }
    }
}

/// Every `.rs` file under `dir`.
fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// Whether the file is a test module's: `tests.rs`, `*_tests.rs`.
fn is_test_file(place: &str) -> bool {
    let name = place.rsplit('/').next().unwrap_or(place);
    name == "tests.rs" || name.ends_with("_tests.rs")
}

/// The line (from 1) that byte `at` of `text` is in.
fn line_of(text: &str, at: usize) -> usize {
    text[..at].matches('\n').count() + 1
}

/// The string literal that starts at `rest` after white space, if it
/// does (`"…"`, without escapes handled: a path has none).
fn literal(rest: &str) -> Option<&str> {
    let rest = rest.trim_start().strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(&rest[..end])
}

/// The findings in one file's `text`, at `place`; `dir` is the folder an
/// embedded file's path is from.
fn scan(id: &str, place: &str, text: &str, dir: Option<&Path>, crate_dir: &Path) -> Vec<Finding> {
    // What follows `#[cfg(test)]` is the file's tests.
    let text = text.split("#[cfg(test)]").next().unwrap_or(text);
    let mut findings = Vec::new();
    embedded(place, text, dir, &mut findings);
    writes(id, place, text, &mut findings);
    ids(id, place, text, crate_dir, &mut findings);
    findings
}

/// `include_bytes!` and `include_str!` of a file over [`MAX_EMBEDDED`].
fn embedded(place: &str, text: &str, dir: Option<&Path>, out: &mut Vec<Finding>) {
    let Some(dir) = dir else {
        return;
    };
    for macro_name in ["include_bytes!(", "include_str!("] {
        for (at, _) in text.match_indices(macro_name) {
            let Some(path) = literal(&text[at + macro_name.len()..]) else {
                continue;
            };
            let full = dir.join(path);
            let slashed = format!("/{}", full.to_string_lossy().replace('\\', "/"));
            if EMBEDDED_OK.iter().any(|ok| slashed.contains(ok)) {
                continue;
            }
            let Ok(size) = std::fs::metadata(&full).map(|m| m.len()) else {
                continue;
            };
            if size > MAX_EMBEDDED && !behind_embedded_feature(text, at) {
                out.push(Finding::warning(
                    "package.embedded-asset",
                    format!("{place}:{}", line_of(text, at)),
                    format!(
                        "`{path}` is {} KB and embedded in the code, where every byte takes the X4's flash room; keep a file over {} KB on the card (`pictures/`, `data/`) and read it when needed (docs/plugin-conventions.md).",
                        size.div_ceil(1000),
                        MAX_EMBEDDED / 1024
                    ),
                ));
            }
        }
    }
}

/// Whether the item that holds byte `at` of `text` (up to the empty line
/// above it) is behind `#[cfg(feature = "embedded-…")]`: a plugin of the
/// store that is also part of an app carries a copy of the files it reads
/// from the card there, and its plugin file for a reader is built without
/// it (docs/plugin-conventions.md).
fn behind_embedded_feature(text: &str, at: usize) -> bool {
    text[..at]
        .lines()
        .rev()
        .skip(1)
        .take_while(|line| !line.trim().is_empty())
        .any(|line| line.trim().starts_with("#[cfg(feature = \"embedded-"))
}

/// A line that writes where the plugin is installed (`plugins/<id>/`),
/// which an update replaces.
fn writes(id: &str, place: &str, text: &str, out: &mut Vec<Finding>) {
    let own = format!("plugins/{id}/");
    let data = format!(".paperpaw/{own}");
    for (n, line) in text.lines().enumerate() {
        // The plugin's data folder is `.paperpaw/plugins/<id>/`.
        if line.trim_start().starts_with("//") || !line.replace(&data, "").contains(&own) {
            continue;
        }
        if WRITES.iter().any(|w| line.contains(w)) {
            out.push(Finding::warning(
                "package.runtime-path",
                format!("{place}:{}", n + 1),
                format!(
                    "This writes under `{own}`, which an update replaces; what a plugin writes belongs in `data::plugin_dir` (`.paperpaw/plugins/{id}/`)."
                ),
            ));
        }
    }
}

/// Whether the first part of `name` is `core`, another plugin's id (a
/// folder beside the crate that is a crate too, as in PaperPaw's
/// `plugins/`), or the id of one of the crate's own plugins (a crate that
/// ships several has a folder `how/<plugin-id>/` each).
fn is_other_plugin(name: &str, crate_dir: &Path) -> bool {
    let first = name.split('.').next().unwrap_or(name);
    first == "core"
        || crate_dir.join("how").join(first).is_dir()
        || crate_dir
            .parent()
            .is_some_and(|plugins| plugins.join(first).join("Cargo.toml").is_file())
}

/// `Id::new_static("…")` and `Setting::new("…")` of a name that does not
/// start with `<id>.`.
fn ids(id: &str, place: &str, text: &str, crate_dir: &Path, out: &mut Vec<Finding>) {
    let own = format!("{id}.");
    for macro_name in ["Id::new_static(", "Setting::new("] {
        for (at, _) in text.match_indices(macro_name) {
            let Some(name) = literal(&text[at + macro_name.len()..]) else {
                continue;
            };
            if name == id || name.starts_with(&own) || is_other_plugin(name, crate_dir) {
                continue;
            }
            out.push(Finding::warning(
                "package.id-prefix",
                format!("{place}:{}", line_of(text, at)),
                format!(
                    "`{name}` does not start with `{own}`; what a plugin registers is in its own namespace (if it names another plugin's, ignore this)."
                ),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(id: &str, text: &str) -> Vec<(&'static str, String)> {
        scan(id, "src/lib.rs", text, None, Path::new("/nonexistent/owls"))
            .into_iter()
            .map(|f| (f.check, f.place))
            .collect()
    }

    #[test]
    fn ids_are_in_the_plugins_namespace() {
        let text = "const A: Id = Id::new_static(\"owls.screen\");\nconst B: Setting<bool> = Setting::new(\"owls\");\nconst C: Id = Id::new_static(\"hoots.screen\");\nconst D: Setting<bool> = Setting::new(\n    \"core.night-mode\",\n);\n";
        assert_eq!(
            found("owls", text),
            [("package.id-prefix", "src/lib.rs:3".to_string())]
        );
    }

    #[test]
    fn the_ids_of_core_and_of_a_plugin_beside_are_not_said() {
        let dir = std::env::temp_dir().join(format!("paperpaw-dev-beside-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("stats")).unwrap();
        std::fs::write(dir.join("stats/Cargo.toml"), "").unwrap();
        let text = "const A: Id = Id::new_static(\"stats.group\");\nconst B: Id = Id::new_static(\"core.x\");\nconst C: Id = Id::new_static(\"nobody.x\");\n";
        let findings: Vec<_> = scan("owls", "lib.rs", text, None, &dir.join("owls"))
            .into_iter()
            .map(|f| f.place)
            .collect();
        // A crate of several plugins has a `how/<plugin-id>/` each.
        std::fs::create_dir_all(dir.join("owls/how/owls-html")).unwrap();
        let own: Vec<_> = scan(
            "owls",
            "lib.rs",
            "const A: Id = Id::new_static(\"owls-html.x\");\n",
            None,
            &dir.join("owls"),
        )
        .into_iter()
        .map(|f| f.place)
        .collect();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(findings, ["lib.rs:3"]);
        assert!(own.is_empty());
    }

    #[test]
    fn writing_into_the_installed_folder_is_said() {
        let text = "fn f() {\n    // plugins/owls/ is read only\n    storage.write(\"plugins/owls/state.txt\", b\"\");\n    storage.read(\"plugins/owls/owl.png\");\n    storage.write(\".paperpaw/plugins/owls/state.txt\", b\"\");\n}\n";
        assert_eq!(
            found("owls", text),
            [("package.runtime-path", "src/lib.rs:3".to_string())]
        );
    }

    #[test]
    fn tests_after_cfg_test_are_left_alone() {
        let text = "fn f() {}\n#[cfg(test)]\nmod tests {\n    const A: Id = Id::new_static(\"other.x\");\n}\n";
        assert_eq!(found("owls", text), []);
    }

    #[test]
    fn an_embedded_file_over_4_kb_is_said_unless_it_is_text_for_people() {
        let dir = std::env::temp_dir().join(format!("paperpaw-dev-source-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("pictures")).unwrap();
        std::fs::create_dir_all(dir.join("i18n")).unwrap();
        std::fs::write(dir.join("pictures/big.png"), vec![0u8; 5000]).unwrap();
        std::fs::write(dir.join("pictures/small.png"), vec![0u8; 100]).unwrap();
        std::fs::write(dir.join("i18n/en.ini"), vec![b'a'; 9000]).unwrap();
        let text = "const A: &[u8] = include_bytes!(\"pictures/big.png\");\nconst B: &[u8] = include_bytes!(\"pictures/small.png\");\nconst C: &str = include_str!(\"i18n/en.ini\");\nconst D: &[u8] = include_bytes!(concat!(env!(\"OUT_DIR\"), \"/t.bin\"));\n";
        let findings: Vec<_> = scan("owls", "lib.rs", text, Some(&dir), &dir)
            .into_iter()
            .map(|f| (f.check, f.place))
            .collect();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(
            findings,
            [("package.embedded-asset", "lib.rs:1".to_string())]
        );
    }

    #[test]
    fn a_copy_behind_an_embedded_feature_is_not_said() {
        let dir = std::env::temp_dir().join(format!("paperpaw-dev-feature-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("pictures")).unwrap();
        std::fs::write(dir.join("pictures/big.png"), vec![0u8; 5000]).unwrap();
        let text = "/// The cats.\n#[cfg(feature = \"embedded-pictures\")]\nconst A: [&[u8]; 1] = [\n    include_bytes!(\"pictures/big.png\"),\n];\n\nconst B: &[u8] = include_bytes!(\"pictures/big.png\");\n";
        let findings: Vec<_> = scan("owls", "lib.rs", text, Some(&dir), &dir)
            .into_iter()
            .map(|f| (f.check, f.place))
            .collect();
        let _ = std::fs::remove_dir_all(&dir);
        // Only the one with no feature.
        assert_eq!(
            findings,
            [("package.embedded-asset", "lib.rs:7".to_string())]
        );
    }

    #[test]
    fn a_plugin_may_say_why_it_embeds_large_files() {
        let manifest = "[package]\nname = \"x\"\n\n[package.metadata.paperpaw]\ncard-files = [\"a\"]\nembedded-assets = \"in the firmware\"\n\n[features]\nembedded-assets = []\n";
        assert_eq!(
            crate::target::embedded_assets(manifest).as_deref(),
            Some("in the firmware")
        );
        // Not under another table, and not without a reason.
        assert_eq!(
            crate::target::embedded_assets("[features]\nembedded-assets = \"x\"\n"),
            None
        );
        assert_eq!(
            crate::target::embedded_assets("[package.metadata.paperpaw]\nembedded-assets = \"\"\n"),
            None
        );
    }

    #[test]
    fn a_crate_is_read_for_its_id() {
        let dir = std::env::temp_dir().join(format!("paperpaw-dev-crate-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"paperpaw-plugin-owls\"\nversion = \"1.0.0\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("src/lib.rs"),
            "const A: Id = Id::new_static(\"x.y\");\n",
        )
        .unwrap();
        let source = Source::read(&dir).unwrap();
        let mut report = Report::default();
        check(&source, &mut report);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(source.id, "owls");
        assert_eq!(report.findings.len(), 1);
        assert_eq!(report.findings[0].check, "package.id-prefix");
    }
}
