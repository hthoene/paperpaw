//! The hyphenation tables of `[hyphenation]` data plugins as the kernel
//! keeps them: only running plugins' tables, added as a plugin is
//! installed, taken out (and given back to another plugin) as it is
//! removed.

use super::*;
use alloc::format;
use paperpaw_sdk::MemoryStorage;

fn manifest(id: &str, lines: &str) -> String {
    format!("[plugin]\nid = {id}\nversion = 1.0.0\nformats = 5\n\n[hyphenation]\n{lines}")
}

fn with_plugin(storage: MemoryStorage, id: &str, lines: &str, files: &[&str]) -> MemoryStorage {
    let mut storage = storage.with(&format!("plugins/{id}/plugin.ini"), manifest(id, lines));
    for file in files {
        storage = storage.with(&format!("plugins/{id}/{file}"), "PPHP");
    }
    storage
}

fn found(kernel: &Kernel, tag: &str) -> Option<(String, String)> {
    kernel
        .shared
        .hyphenation
        .card_table(tag)
        .map(|e| (e.plugin().into(), e.path().into()))
}

#[test]
fn only_running_plugins_tables_are_kept_at_start_up() {
    let storage = with_plugin(
        MemoryStorage::new(),
        "hyphenation-de",
        "de = hyphenation/de.pphp\n",
        &["hyphenation/de.pphp"],
    );
    let storage = with_plugin(
        storage,
        "hyphenation-nb",
        "nb = hyphenation/nb.pphp\nno = hyphenation/nb.pphp\n",
        &["hyphenation/nb.pphp"],
    );
    let kernel = Kernel::builder(DEVICE)
        .storage(storage)
        .user_config("[plugins]\nhyphenation-nb = off\n")
        .build()
        .unwrap();
    assert_eq!(
        found(&kernel, "de-AT"),
        Some((
            "hyphenation-de".into(),
            "plugins/hyphenation-de/hyphenation/de.pphp".into()
        ))
    );
    assert_eq!(found(&kernel, "nb"), None);
    assert_eq!(found(&kernel, "no"), None);
}

#[test]
fn installed_plugins_add_their_tables_and_removed_ones_give_them_back() {
    let storage = with_plugin(
        MemoryStorage::new(),
        "hyphenation",
        "en = hyphenation/en.pphp\n",
        &["hyphenation/en.pphp"],
    );
    let storage = with_plugin(
        storage,
        "hyphenation-en-gb",
        "en = hyphenation/en-gb.pphp\n",
        &["hyphenation/en-gb.pphp"],
    );
    let mut kernel = Kernel::builder(DEVICE).storage(storage).build().unwrap();
    kernel.start(Instant(0));
    assert_eq!(found(&kernel, "en").unwrap().0, "hyphenation-en-gb");
    let warnings = kernel.take_warnings();
    assert!(
        warnings
            .iter()
            .any(|w| w.message.contains("hyphenation:en: hyphenation left out")),
        "{warnings:?}"
    );

    let storage = kernel.storage();
    storage
        .write(
            "plugins/hyphenation-sv/plugin.ini",
            manifest("hyphenation-sv", "sv = hyphenation/sv.pphp\n").as_bytes(),
        )
        .unwrap();
    storage
        .write("plugins/hyphenation-sv/hyphenation/sv.pphp", b"PPHP")
        .unwrap();
    kernel.apply(Command::LoadPlugin("plugins/hyphenation-sv".into()));
    kernel.finish();
    assert_eq!(found(&kernel, "sv-FI").unwrap().0, "hyphenation-sv");

    kernel.apply(Command::RemovePlugin(Id::new_static("hyphenation-en-gb")));
    kernel.finish();
    assert_eq!(
        found(&kernel, "en-GB"),
        Some((
            "hyphenation".into(),
            "plugins/hyphenation/hyphenation/en.pphp".into()
        ))
    );
    kernel.apply(Command::RemovePlugin(Id::new_static("hyphenation-sv")));
    kernel.finish();
    assert_eq!(found(&kernel, "sv"), None);
}

/// The tables open give their heap back when a book is closed and before
/// the device sleeps (Senior's remark 2 on PR #195): each is opened again
/// at its next word.
#[test]
fn open_tables_are_closed_when_a_book_closes_and_before_sleep() {
    let table = paperpaw_sdk::hyphen::compile("1b", "", 1, 1);
    let inner = MemoryStorage::new()
        .with(
            "plugins/hyphenation-en/plugin.ini",
            manifest("hyphenation-en", "en = en.pphp\n"),
        )
        .with("plugins/hyphenation-en/en.pphp", table);
    let mut kernel = Kernel::builder(DEVICE).storage(inner).build().unwrap();
    let open = |kernel: &Kernel| kernel.shared.hyphenation.open_tables().0;
    kernel.start(Instant(0));
    let breaks = |kernel: &Kernel| {
        let mut at = Vec::new();
        let hyphenation = &kernel.shared.hyphenation;
        hyphenation
            .get("en")
            .unwrap()
            .hyphenate("abab", &mut |n| at.push(n));
        at
    };
    assert_eq!(open(&kernel), 0);
    assert_eq!(breaks(&kernel), [1, 3]);
    assert_eq!(open(&kernel), 1);
    kernel.emit(
        paperpaw_sdk::builtin::events::BOOK_CLOSED.id(),
        &paperpaw_sdk::Params::new(),
    );
    assert_eq!(open(&kernel), 0);
    assert_eq!(breaks(&kernel), [1, 3]);
    assert_eq!(open(&kernel), 1);
    kernel.suspend(Instant(1_000));
    assert_eq!(open(&kernel), 0);
    kernel.resume(Instant(2_000));
    assert_eq!(breaks(&kernel), [1, 3]);
}

/// Network, store and firmware work want the heap: the tables open give
/// theirs back as it begins; the radio switched off keeps them.
#[test]
fn open_tables_are_closed_for_network_store_and_firmware_work() {
    use paperpaw_sdk::firmware::FirmwareCommand;
    use paperpaw_sdk::wifi::WifiCommand;
    let table = paperpaw_sdk::hyphen::compile("1b", "", 1, 1);
    let inner = MemoryStorage::new()
        .with(
            "plugins/hyphenation-en/plugin.ini",
            manifest("hyphenation-en", "en = en.pphp\n"),
        )
        .with("plugins/hyphenation-en/en.pphp", table);
    let mut kernel = Kernel::builder(DEVICE).storage(inner).build().unwrap();
    kernel.start(Instant(0));
    let open = |kernel: &Kernel| kernel.shared.hyphenation.open_tables().0;
    let hyphenate = |kernel: &Kernel| {
        let hyphenation = &kernel.shared.hyphenation;
        hyphenation
            .get("en")
            .unwrap()
            .hyphenate("abab", &mut |_| {});
    };
    let closing = [
        Command::Wifi(WifiCommand::On),
        Command::Wifi(WifiCommand::Join {
            ssid: String::from("home"),
            password: String::new(),
        }),
        Command::Firmware(FirmwareCommand::install("update.bin", None, false)),
        Command::InstallPlugin(Id::new_static("absent"), String::from("plugins/absent")),
    ];
    for command in closing {
        hyphenate(&kernel);
        assert_eq!(open(&kernel), 1);
        kernel.apply(command);
        assert_eq!(open(&kernel), 0);
    }
    hyphenate(&kernel);
    kernel.apply(Command::Wifi(WifiCommand::Off));
    assert_eq!(open(&kernel), 1, "the radio off wants no heap");
}
