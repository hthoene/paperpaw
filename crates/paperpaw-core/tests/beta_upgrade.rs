//! Released beta fixtures survive current startup and settings writes.
//! This is host persistence and eligibility evidence, not a device update.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests fail where retained user data cannot be read"
)]

mod common;

use paperpaw_core::{Kernel, code_to_load};
use paperpaw_native::{Chip, Header};
use paperpaw_platform::Instant;
use paperpaw_sdk::native::{FINGERPRINT, Fingerprint, INTERFACE, Native};
use paperpaw_sdk::{Command, MemoryStorage, PluginProblem, positions, read_file};

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
        .plugin(harness.plugin())
        .storage(storage())
        .build()
        .unwrap();
    kernel.start(Instant(0));
    assert_eq!(kernel.settings().int("core.ui-size"), 12);
    assert!(kernel.settings().bool("core.night-mode"));
    assert!(kernel.take_warnings().is_empty());
    unchanged(&kernel);

    harness.run(&mut kernel, 1, [Command::SetScope(Some(SCOPED.into()))]);
    assert!(kernel.settings().bool("core.scope-only"));
    assert_eq!(
        kernel.settings().text("core.reading-font"),
        "core.font.default"
    );
    harness.run(&mut kernel, 2, [Command::SetScope(None)]);
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

/// Synthetic headers isolate interface gating from signature and load checks.
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

#[test]
fn interface_five_is_refused_without_discarding_it_and_interface_six_stays_eligible() {
    // Both released beta tags use 6. An actual bump needs a revised matrix.
    assert_eq!(INTERFACE, 6);
    let native = Native::new("code.ppn");
    for interface in [5, 6] {
        let header = header(interface);
        let bytes = header.to_bytes();
        assert_eq!(Header::read(&bytes), Ok(header));
        let storage = storage().with(PLUGIN_FILE, bytes.to_vec());
        assert_eq!(code_to_load(&storage, &native), interface == 6);
        assert_eq!(
            native.problem(&header),
            if interface == 5 {
                Some(PluginProblem::OlderInterface { built_for: 5 })
            } else {
                None
            }
        );
        assert_eq!(storage.get(PLUGIN_FILE).unwrap(), bytes);
        for (path, bytes) in USER_DATA {
            assert_eq!(storage.get(path).unwrap(), bytes, "{path}");
        }
    }
}
