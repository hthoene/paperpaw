//! Rollback check (#229): the code of a released beta (v0.6.0-beta.1 to .3) on a card
//! Beta 4 has used. NOT part of this tree's tests: `tools/qualify-update-paths.sh`
//! copies it into a worktree of each tag as `crates/paperpaw-core/tests/` and
//! runs it there, with `PAPERPAW_ROLLBACK_CARD` naming the folder Beta 4's
//! `beta_upgrade` test wrote (`PAPERPAW_ROLLBACK_CARD_OUT`).
//!
//! Every check is `ok` (kept and used), `degrades` (kept as it was, the old build
//! cannot use it: expected, listed in docs/testing/update-rollback.md) or
//! `LOST` (changed or gone: the test fails, after all checks ran).

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "a test")]

use paperpaw_core::Kernel;
use paperpaw_native::{HEADER_LEN, Header};
use paperpaw_platform::{
    DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, PixelFormat, Rotation, Storage,
};
use paperpaw_sdk::native::Native;
use paperpaw_sdk::{MemoryStorage, PluginProblem, annotations, positions, read_file};
use std::path::{Path, PathBuf};

const DEVICE: DeviceInfo = DeviceInfo::new(
    "test",
    "Test device",
    DisplayInfo::new(120, 160, 100, PixelFormat::Gray4, Rotation::Deg0, None),
    InputCaps::new(true, true, true, &[]),
    MemoryClass::Large,
);
const BOOK: &str = "books/alice-in-wonderland.txt";

/// What the shell's loader (`shells/xteink/src/native/mod.rs`) does with a
/// file it cannot run: reads its header, names the problem.
struct ShellLike;
impl paperpaw_core::CodeLoader for ShellLike {
    fn load(
        &mut self,
        storage: &dyn Storage,
        code: &paperpaw_core::CodeFile<'_>,
    ) -> Result<Box<dyn paperpaw_core::sdk::Plugin>, PluginProblem> {
        let mut head = [0u8; HEADER_LEN];
        let mut at = 0;
        while at < head.len() {
            match storage.read_at(code.path, at as u64, &mut head[at..]) {
                Ok(0) | Err(_) => break,
                Ok(n) => at += n,
            }
        }
        let head = &head[..at];
        match Header::read(head) {
            Err(problem) => Err(Native::problem_of_file(head, problem)),
            Ok(_) => Err(PluginProblem::OtherBuild),
        }
    }
}

fn files(dir: &Path, below: &str, all: &mut Vec<(String, Vec<u8>)>) {
    let mut names: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap())
        .collect();
    names.sort_by_key(|e| e.file_name());
    for entry in names {
        let name = entry.file_name().into_string().unwrap();
        let path = if below.is_empty() {
            name
        } else {
            format!("{below}/{name}")
        };
        if entry.file_type().unwrap().is_dir() {
            files(&entry.path(), &path, all);
        } else {
            all.push((path, std::fs::read(entry.path()).unwrap()));
        }
    }
}

#[derive(Default)]
struct Report(Vec<String>, bool);
impl Report {
    fn ok(&mut self, what: &str) {
        self.0.push(format!("ok       {what}"));
    }
    fn degrades(&mut self, what: &str) {
        self.0.push(format!("degrades {what}"));
    }
    fn check(&mut self, good: bool, what: &str) {
        if good {
            self.ok(what);
        } else {
            self.0.push(format!("LOST     {what}"));
            self.1 = true;
        }
    }
}

#[test]
fn an_older_beta_on_a_beta4_card_loses_nothing() {
    let dir =
        PathBuf::from(std::env::var_os("PAPERPAW_ROLLBACK_CARD").expect("PAPERPAW_ROLLBACK_CARD"));
    let mut card_files = Vec::new();
    files(&dir, "", &mut card_files);
    let mut card = MemoryStorage::new();
    for (path, bytes) in &card_files {
        card = card.with(path, bytes.clone());
    }
    let mut r = Report::default();
    // The sha of a code file's header, as the store named it installed.
    let clock = card.get("plugins/clock/code.ppn").unwrap();
    assert!(paperpaw_sdk::native::remember_code(
        &card,
        "clock",
        clock[64..96].try_into().unwrap()
    ));
    let named = card.get(".paperpaw/plugin-code.txt").unwrap();

    let mut kernel = Kernel::builder(DEVICE)
        .storage(card)
        .code_plugins(Native::new("code.ppn"), Box::new(ShellLike))
        .build()
        .expect("the old build builds its kernel on the card");
    kernel.start(Instant(0));
    r.ok("starts on the card (no panic, kernel built)");
    let warnings = kernel.take_warnings();
    r.ok(&format!("start warnings: {}", warnings.len()));

    // Settings: Beta 4's value is read; a save keeps what the old build does not know.
    r.check(
        kernel.settings().int("core.ui-size") == 14,
        "paperpaw.ini: Beta 4's core.ui-size read",
    );
    kernel
        .set_setting(Instant(1), "core.ui-size", "13")
        .unwrap();
    let ini =
        String::from_utf8(read_file(kernel.storage(), "paperpaw.ini", 1 << 20).unwrap()).unwrap();
    r.check(
        ini.contains("ui-size = 13"),
        "paperpaw.ini: the old build's save is written",
    );
    r.check(
        ini.contains("[later-plugin]\nsome-key = some value"),
        "paperpaw.ini: a section of a plugin the old build does not have survives the old build's save",
    );

    // The plugin of interface 7 (`formats = 5`): refused by its manifest with
    // a warning that says why, not listed, its folder not touched.
    let said = warnings.iter().any(|w| {
        w.message
            .contains("plugins/clock/plugin.ini: made for a later PaperPaw")
    });
    if kernel.plugins().get("clock").is_none() && said {
        r.degrades(
            "plugin clock (interface 7): not listed; a warning says \"made for a later PaperPaw\"",
        );
    } else {
        r.check(
            false,
            "plugin clock (interface 7) is refused with a warning that names the later PaperPaw",
        );
    }
    r.check(
        read_file(kernel.storage(), "plugins/clock/code.ppn", 1 << 20).unwrap() == clock,
        "plugin folder: Beta 4's code file is not deleted or rewritten",
    );

    // Positions: Beta 4's file reads, with the fields the old build knows.
    let at = positions::load_for_update(kernel.storage(), BOOK);
    match at {
        Ok(Some(p)) => {
            r.check(
                p.at == paperpaw_sdk::document::Locator::new(7, 1234) && p.percent == 41.5,
                "position: the place and percent of Beta 4's file are read",
            );
        }
        other => r.check(
            false,
            &format!("position: Beta 4's file reads, got {other:?}"),
        ),
    }

    // Annotations (version 3): not read, not written over.
    let notes_before = card_files
        .iter()
        .find(|(p, _)| p.starts_with(".paperpaw/annotations/"))
        .unwrap()
        .1
        .clone();
    match annotations::load_for_update(kernel.storage(), BOOK) {
        Err(_) => r.degrades("annotations (version 3): not shown (InvalidData)"),
        Ok(rows) => r.ok(&format!(
            "annotations (version 3): read, {} rows",
            rows.len()
        )),
    }
    let mark = annotations::Mark::new(
        annotations::Kind::Bookmark,
        paperpaw_sdk::document::Locator::new(1, 1),
        paperpaw_sdk::document::Locator::new(1, 1),
        false,
    );
    let added = annotations::add(
        kernel.storage(),
        BOOK,
        annotations::Annotation::new(mark, 1, 0, 0, "x", ""),
    );
    let now = read_file(kernel.storage(), &annotations::file(BOOK), 1 << 20).unwrap();
    r.check(
        added.is_err() && now == notes_before,
        "annotations (version 3): the old build's add is refused and the file is not written over",
    );

    // Passwords: every line stays, the sealed one unusable (not decoded).
    let text =
        String::from_utf8(read_file(kernel.storage(), ".paperpaw/secrets.txt", 1 << 20).unwrap())
            .unwrap();
    let secrets = paperpaw_sdk::secrets::Secrets::decode(&text);
    r.check(
        secrets.encode() == text,
        "passwords: the old build's decode and encode give the file back byte for byte",
    );
    r.check(
        secrets.get("wifi.Home Net").is_some(),
        "passwords: the veiled password is usable",
    );
    r.check(
        secrets.get("mail.work").is_none(),
        "passwords: the sealed one is not decoded (stays unread)",
    );
    r.degrades("passwords: a password a sealing device wrote (s1:) is not usable: enter it again");

    // Nothing else changed, whatever the old build did at start and on its saves.
    for (path, bytes) in &card_files {
        if path == "paperpaw.ini" {
            continue;
        }
        let now = read_file(kernel.storage(), path, 1 << 24).ok();
        r.check(
            now.as_deref() == Some(bytes.as_slice()),
            &format!("unchanged: {path}"),
        );
    }
    let _ = named; // the list of installed code written for the test

    println!(
        "\nan older beta on a Beta 4 card ({} files):",
        card_files.len()
    );
    for line in &r.0 {
        println!("  {line}");
    }
    assert!(!r.1, "something of the user's was lost, see above");
}
