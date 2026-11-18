//! Released beta fixtures survive current startup and settings writes.
//! This is host persistence and eligibility evidence, not a device update.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests fail where retained user data cannot be read"
)]

mod common;
use common::AddDriver as _;

use paperpaw_core::{Ask, CodeLoader, Kernel, code_to_load};
use paperpaw_native::header5::Host;
use paperpaw_native::host_glue::MINOR;
use paperpaw_native::{Chip, Header};
use paperpaw_platform::{Instant, Storage};
use paperpaw_sdk::native::{FINGERPRINT, Fingerprint, INTERFACE, Native};
use paperpaw_sdk::{MemoryStorage, PluginProblem, PluginState, positions, read_file};

const GLOBAL: &str = "paperpaw.ini";
const SCOPED: &str = ".paperpaw/plugins/reader/books/0000000000000001/settings.ini";
const LAST: &str = ".paperpaw/plugins/reader/state.ini";
const LEGACY_POSITION: &str = ".paperpaw/plugins/reader/books/0000000000000001/progress.ini";
const BOOK: &str = "books/alice-in-wonderland.txt";
const PLUGIN_FILE: &str = "plugins/legacy/code.ppn";
const USER_DATA: [(&str, &[u8]); 4] = [
    (
        SCOPED,
        include_bytes!("../testdata/beta-upgrade/settings.ini"),
    ),
    (
        LAST,
        include_bytes!("../testdata/beta-upgrade/reader-state.ini"),
    ),
    (
        LEGACY_POSITION,
        include_bytes!("../testdata/beta-upgrade/progress.ini"),
    ),
    (
        BOOK,
        include_bytes!("../../../samples/library/books/alice-in-wonderland.txt"),
    ),
];

fn storage() -> MemoryStorage {
    let mut storage = MemoryStorage::new().with(
        GLOBAL,
        include_bytes!("../testdata/beta-upgrade/paperpaw.ini").to_vec(),
    );
    for (path, bytes) in USER_DATA {
        storage = storage.with(path, bytes.to_vec());
    }
    storage
}

fn unchanged(kernel: &Kernel) {
    for (path, bytes) in USER_DATA {
        assert_eq!(
            read_file(kernel.storage(), path, 1 << 20).unwrap(),
            bytes,
            "{path}"
        );
    }
}

#[test]
fn beta_settings_books_and_positions_survive_start_save_and_restart() {
    let harness = common::Harness::new();
    let mut kernel = Kernel::builder(common::DEVICE)
        .driver(harness.plugin())
        .storage(storage())
        .build()
        .unwrap();
    kernel.start(Instant(0));
    assert_eq!(kernel.settings().int("core.ui-size"), 12);
    assert!(kernel.settings().bool("core.night-mode"));
    assert!(kernel.take_warnings().is_empty());
    unchanged(&kernel);

    harness.run(&mut kernel, 1, [Ask::SetScope(Some(SCOPED.into()))]);
    assert!(kernel.settings().bool("core.scope-only"));
    assert_eq!(
        kernel.settings().text("core.reading-font"),
        "core.font.default"
    );
    harness.run(&mut kernel, 2, [Ask::SetScope(None)]);
    kernel
        .set_setting(Instant(3), "core.ui-size", "14")
        .unwrap();
    unchanged(&kernel);

    let mut copied = MemoryStorage::new();
    for path in [GLOBAL, SCOPED, LAST, LEGACY_POSITION, BOOK] {
        copied = copied.with(path, read_file(kernel.storage(), path, 1 << 20).unwrap());
    }
    let mut restarted = Kernel::builder(common::DEVICE)
        .storage(copied)
        .build()
        .unwrap();
    restarted.start(Instant(0));
    assert_eq!(restarted.settings().int("core.ui-size"), 14);
    assert!(restarted.settings().bool("core.night-mode"));
    unchanged(&restarted);
}

#[test]
fn beta_progress_survives_the_current_position_writer_without_changing_legacy_data() {
    let storage = storage();
    let text = include_str!("../testdata/beta-upgrade/progress.ini");
    let position = positions::Position::parse(text).unwrap();
    assert_eq!(position.at, paperpaw_sdk::document::Locator::new(7, 1234));
    assert_eq!(position.percent, 41.5);
    positions::save(&storage, BOOK, &position).unwrap();
    assert_eq!(
        positions::load_for_update(&storage, BOOK).unwrap(),
        Some(position)
    );
    for (path, bytes) in USER_DATA {
        assert_eq!(storage.get(path).unwrap(), bytes, "{path}");
    }
}

/// A format-4 header (what interfaces 5 and 6 shipped) with a plugin interface.
fn header(interface: u16) -> Header {
    Header {
        chip: Chip::Esp32C3,
        fingerprint: Fingerprint::of(interface, FINGERPRINT.0).0,
        base: paperpaw_native::LINK_BASE,
        image_len: 128,
        code_at: 96,
        entry: 0,
        api: 32,
        relocs: 0,
        align: 16,
        compiler: paperpaw_native::compiler::id("1.99.0"),
        hot_at: 0,
        hot_len: 0,
        patches: 0,
        needs_api: 0,
        families: [0; 8],
        interface,
        sha256: [7; 32],
    }
}

/// Beta 3's own plugin file (interface 6, `hyphenation-it` 1.0.3 for the X4),
/// byte for byte as its release shipped it
/// (`tests/data/beta3-hyphenation-it-1.0.3-x4.source.txt`).
const BETA3_CODE: &[u8] = include_bytes!("data/beta3-hyphenation-it-1.0.3-x4.ppn");
const PLUGIN_INI: &str = "[plugin]\nid = legacy\nname = Legacy\nversion = 1.0.0\n";

/// A firmware whose loader runs interface-7 files only: a file of the format
/// before is not one of its own.
struct Interface7Only;
impl CodeLoader for Interface7Only {}

/// Interface 7 refuses the code plugins of Beta 1 to 3 (interface 6) and of
/// older betas (5) without touching them or the user's data, and says why.
#[test]
fn interface_five_and_six_files_are_refused_with_a_reason_and_kept() {
    assert_eq!(INTERFACE, 7);
    let native = Native::new("code.ppn");
    let x4 = Host::x4(MINOR);
    let five = header(5).to_bytes().to_vec();
    for (name, bytes, built_for) in [
        ("beta3", BETA3_CODE.to_vec(), 6),
        ("synthetic-interface-5", five, 5),
    ] {
        let storage = storage()
            .with(PLUGIN_FILE, bytes.clone())
            .with("plugins/legacy/plugin.ini", PLUGIN_INI);
        assert!(!code_to_load(&storage, &native, Some(&x4)), "{name}");
        let mut kernel = Kernel::builder(common::DEVICE)
            .storage(storage)
            .code_plugins(native.clone(), Box::new(Interface7Only))
            .build()
            .unwrap();
        kernel.start(Instant(0));
        let plugin = kernel.plugins().get("legacy").unwrap();
        assert_eq!(plugin.state, PluginState::Failed, "{name}");
        assert_eq!(
            plugin.problem,
            Some(PluginProblem::OlderInterface { built_for }),
            "{name}"
        );
        assert_eq!(
            read_file(kernel.storage(), PLUGIN_FILE, 1 << 20).unwrap(),
            bytes,
            "{name}"
        );
        unchanged(&kernel);
    }
}

/// The card as Beta 4 leaves it, for the rollback check (`tools/qualify-update-paths.sh`
/// runs Beta 3's code on it): Beta 3's data after Beta 4 started and saved,
/// plus what only Beta 4 writes. Set `PAPERPAW_ROLLBACK_CARD_OUT` to a new
/// folder to keep a copy; the assertions hold either way.
#[test]
fn beta4_card_keeps_beta3_data_and_is_written_for_rollback() {
    use paperpaw_sdk::document::Locator;
    // A setting of a plugin this build does not have (a later one, or one
    // switched off): it must stay in the file as it was.
    let later = "\n[later-plugin]\nsome-key = some value\n";
    let ini = [include_str!("../testdata/beta-upgrade/paperpaw.ini"), later].concat();
    let mut kernel = Kernel::builder(common::DEVICE)
        .storage(storage().with(GLOBAL, ini.into_bytes()))
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel
        .set_setting(Instant(1), "core.ui-size", "14")
        .unwrap();
    let written = String::from_utf8(read_file(kernel.storage(), GLOBAL, 1 << 20).unwrap()).unwrap();
    assert!(
        written.contains("[later-plugin]\nsome-key = some value"),
        "{written}"
    );
    assert!(written.contains("ui-size = 14"), "{written}");
    unchanged(&kernel);

    let card = kernel.storage();
    // Where a book was left, as Beta 4 keeps it (with the book's content stamp).
    let at = positions::Position::new(Locator::new(7, 1234), 41.5)
        .conversion(255)
        .xpointer(String::from("/body/DocFragment[8]/body/p[3]/text()[1].12"))
        .updated(1_760_000_000);
    let stamp =
        paperpaw_sdk::library::Fingerprint::parse("482133 0b6e8f2a6c1d4e5f8a9b0c1d2e3f4a5b");
    positions::save_with_book(card, BOOK, &at, stamp.as_ref()).unwrap();
    // Its annotations in version 3, which only Beta 4's host writes
    // (`paperpaw_sdk::annotations::write`; the text is that module's test file).
    let notes = "# format 3\n# book 5 abc\n# next-id 9\n# generation 7\n\
                 h\t1\t0\t1\t4\t10\t1\t2\t00000000000000ff\t/a\t/b\ttext\tnote\t4\tdt-1\n\
                 b\t2\t0\t2\t0\t20\t0\t0\t\t\t\t\t\t8\t\n";
    card.write(&paperpaw_sdk::annotations::file(BOOK), notes.as_bytes())
        .unwrap();
    // Passwords: Beta 3's veiled line beside one a sealing device wrote
    // (`docs/abi/secrets.md` section 3: the value is `s1:` and hex).
    let veiled = include_str!("../../paperpaw-sdk/testdata/secrets.txt.v1");
    let sealed = "mail.work\tMail work\tc\ts1:00112233445566778899aabbccddeeff00112233\n";
    card.write(
        ".paperpaw/secrets.txt",
        [veiled, sealed].concat().as_bytes(),
    )
    .unwrap();
    // A plugin of interface 7 (format 5) in its folder.
    card.write(
        "plugins/clock/plugin.ini",
        b"[plugin]\nid = clock\nname = Clock\nversion = 1.0.0\nformats = 5\n",
    )
    .unwrap();
    card.write(
        "plugins/clock/code.ppn",
        include_bytes!("../../paperpaw-native/tests/data/abi7-x4-min.ppn"),
    )
    .unwrap();
    assert!(code_to_load(
        card,
        &Native::new("code.ppn"),
        Some(&Host::x4(MINOR))
    ));

    let Some(out) = std::env::var_os("PAPERPAW_ROLLBACK_CARD_OUT") else {
        return;
    };
    let out = std::path::PathBuf::from(out);
    assert!(!out.exists(), "{} exists: use a new folder", out.display());
    fn dump(card: &dyn Storage, dir: &str, out: &std::path::Path) {
        let mut entries = Vec::new();
        card.list(dir, &mut |e| entries.push((e.name.to_string(), e.is_dir)))
            .unwrap();
        for (name, is_dir) in entries {
            let path = if dir.is_empty() {
                name.clone()
            } else {
                format!("{dir}/{name}")
            };
            if is_dir {
                dump(card, &path, out);
            } else {
                let file = out.join(&path);
                std::fs::create_dir_all(file.parent().unwrap()).unwrap();
                std::fs::write(file, read_file(card, &path, 1 << 24).unwrap()).unwrap();
            }
        }
    }
    dump(card, "", &out);
}
