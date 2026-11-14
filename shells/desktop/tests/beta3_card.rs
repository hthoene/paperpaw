//! The card a Beta 3 reader brings to this build (the update path, #243).
//!
//! `tests/beta3-card` holds the smallest real state a Beta 3 emulator wrote
//! (reading position, last book, library details, statistics, the `paperpaw.ini`
//! with settings, a plugin switched off and keys of plugins that are not loaded,
//! and a veiled `secrets.txt`; the ini and the secrets were written by hand in
//! Beta 3's formats). The current build starts on a copy of it: what the reader
//! chose and where they stopped must survive, and a settings save must not drop
//! what this build does not know. A change that breaks a file's reading or
//! migrates it wrongly fails here.
//!
//! Interface-6 code plugins (`plugins/<id>/code.ppn`) are a loader matter on the
//! device: see `code_plugin_migration_tests` in `paperpaw-core`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const BOOK: &str = "alice-in-wonderland.epub";
const HASH: &str = "b1348f1e1a4d5649";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// A card as Beta 3 left it: the sample book and the fixture's files.
fn card(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("paperpaw-beta3-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("books")).unwrap();
    fs::copy(
        root().join("samples/library/books").join(BOOK),
        dir.join("books").join(BOOK),
    )
    .unwrap();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/beta3-card");
    fs::copy(fixture.join("paperpaw.ini"), dir.join("paperpaw.ini")).unwrap();
    copy_tree(&fixture.join("data"), &dir.join(".paperpaw"));
    // Past the setup, as a reader who has used it is.
    let setup = dir.join(".paperpaw/plugins/setup");
    fs::create_dir_all(&setup).unwrap();
    fs::write(setup.join("done"), "").unwrap();
    dir
}

/// What the emulator said (one JSON line each) to `requests`, started on `library`.
fn session(library: &Path, requests: &[&str]) -> String {
    let path = paperpaw_core::sdk::json::string(&library.display().to_string());
    let mut child = Command::new(env!("CARGO_BIN_EXE_paperpaw-emu"))
        .arg("--stdio")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut lines =
        format!("{{\"op\": \"start\", \"profile\": \"xteink-x4\", \"library\": {path}}}\n");
    for request in requests {
        lines.push_str(request);
        lines.push('\n');
    }
    lines.push_str("{\"op\": \"quit\"}\n");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(lines.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| !l.contains("\"event\": \"frame\""))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The line of the event `name`, or a failure with the whole conversation.
fn event<'a>(said: &'a str, name: &str) -> &'a str {
    said.lines()
        .find(|l| l.contains(&format!("\"event\": \"{name}\"")))
        .unwrap_or_else(|| panic!("no `{name}` in:\n{said}"))
}

fn ini_of(library: &Path) -> String {
    fs::read_to_string(library.join("paperpaw.ini")).unwrap()
}

#[test]
fn a_beta3_card_keeps_position_settings_switches_and_unknown_keys() {
    let library = card("keeps");
    let position = library.join(format!(".paperpaw/positions/{HASH}.txt"));
    let before = fs::read_to_string(&position).unwrap_or_default();
    let said = session(
        &library,
        &[
            r#"{"op": "describe"}"#,
            r#"{"op": "setting", "id": "core.night-mode"}"#,
            r#"{"op": "setting", "id": "core.reading-font"}"#,
            r#"{"op": "setting", "id": "core.language"}"#,
            // A save of the settings file, as the reader's own change makes.
            r#"{"op": "set", "id": "core.font-size", "value": "13"}"#,
            r#"{"op": "wait", "ms": 300}"#,
        ],
    );
    // Starts where the reader stopped: the last book, open.
    assert!(
        event(&said, "described").contains("\"screen\": \"reader.screen\""),
        "{said}"
    );
    // Settings the reader chose.
    let settings: Vec<&str> = said
        .lines()
        .filter(|l| l.contains("\"event\": \"setting\""))
        .collect();
    assert!(settings[0].contains("\"value\": \"on\""), "{said}");
    assert!(settings[1].contains("\"value\": \"Literata\""), "{said}");
    assert!(settings[2].contains("\"value\": \"de\""), "{said}");
    // The position: where it was, in the same book, by the same conversion.
    let after = fs::read_to_string(&position).unwrap();
    for line in [
        "section = 1",
        "offset = 190",
        "conversion = af63bc4c8601b62c",
        "xpointer = /body/DocFragment[2]/body/header[1]/div[1]/text()[1].111",
        "book = 189249 59feb863c3a0652fe66a99c0978404f9",
    ] {
        assert!(
            before.contains(line) && after.contains(line),
            "{line}: {after}"
        );
    }
    // The saved file: the change, what was chosen, the plugin switched off and
    // the keys of plugins this build does not load (they come back when the
    // plugin is installed again).
    let ini = ini_of(&library);
    for line in [
        "language = de",
        "night-mode = on",
        "font-size = 13",
        "reading-font = Literata",
        "network.kosync = on",
        "[kosync]",
        "user = alice",
        "auto = on",
        "[notes-vault]",
        "folder = Notes",
        "[plugins]",
        "battery = off",
    ] {
        assert!(ini.lines().any(|l| l == line), "`{line}` lost:\n{ini}");
    }
    // The last book, the library and the statistics are read, not rewritten away.
    let paperpaw = library.join(".paperpaw");
    assert_eq!(
        fs::read_to_string(paperpaw.join("plugins/reader/state.ini")).unwrap(),
        "[reader]\nlast = books/alice-in-wonderland.epub\n"
    );
    assert!(
        fs::read_to_string(paperpaw.join("library/opened.txt"))
            .unwrap()
            .contains(BOOK)
    );
    assert!(
        fs::read_to_string(paperpaw.join("plugins/stats/books.ini"))
            .unwrap()
            .contains("md5 = 59feb863c3a0652fe66a99c0978404f9")
    );
    // Beta 3's veiled values are sealed by this build (as a device that seals does
    // at its first radio-on, `secrets_i6` tests): the same ids, labels and flags,
    // the value no longer veiled hex.
    let kept = fs::read_to_string(paperpaw.join("secrets.txt")).unwrap();
    let was = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/beta3-card/data/secrets.txt"),
    )
    .unwrap();
    let head = |text: &str| -> Vec<String> {
        text.lines()
            .map(|l| l.rsplit_once('\t').map_or(l, |(h, _)| h).to_string())
            .collect()
    };
    assert_eq!(head(&kept), head(&was), "{kept}");
    assert!(
        kept.lines()
            .all(|l| l.rsplit('\t').next().unwrap().starts_with("s1:")),
        "not sealed: {kept}"
    );
    let _ = fs::remove_dir_all(&library);
}
