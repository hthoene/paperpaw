//! The hyphenation tables of `[hyphenation]` data plugins as the kernel
//! keeps them: only running plugins' tables, added as a plugin is
//! installed, taken out (and given back to another plugin) as it is
//! removed.
//!
//! These need formats 5, which the loader reads once `DATA_FORMATS` is
//! raised with interface 7 (`abi/interface-7/raise.md`, row 1); until then
//! the loader refuses such plugins as made for a later PaperPaw.

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
        .hyphenation_tables
        .find(tag)
        .map(|e| (e.plugin.clone(), e.path.clone()))
}

#[test]
#[ignore = "needs DATA_FORMATS 5, raised with interface 7 (abi/interface-7/raise.md, row 1)"]
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
#[ignore = "needs DATA_FORMATS 5, raised with interface 7 (abi/interface-7/raise.md, row 1)"]
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

/// Before formats 5 a plugin naming tables is refused as made for a later
/// PaperPaw: its tables are never kept. The raise removes this test with
/// the `ignore` of the two above.
#[test]
fn a_plugin_made_for_formats_5_is_refused_until_the_raise() {
    let storage = with_plugin(
        MemoryStorage::new(),
        "hyphenation-de",
        "de = hyphenation/de.pphp\n",
        &["hyphenation/de.pphp"],
    );
    let mut kernel = Kernel::builder(DEVICE).storage(storage).build().unwrap();
    assert!(kernel.plugins().get("hyphenation-de").is_none());
    assert_eq!(found(&kernel, "de"), None);
    let warnings = kernel.take_warnings();
    assert!(
        warnings.iter().any(|w| w.message.contains("formats 5")),
        "{warnings:?}"
    );
}
