//! The data plugins' hyphenation tables reach the shell's sink as the kernel
//! installs and removes plugins, and a card change reaches it when a USB drive
//! ends (`hyphen_feed`).

use super::*;
use crate::code_plugin::CodeLoader;
use crate::native_host::{HyphenTables, hyphen_card::CardTables, hyphen_service::HyphenHost};
use alloc::format;
use paperpaw_sdk::MemoryStorage;
use paperpaw_sdk::native::Native;
use paperpaw_sdk::{Plugin, PluginProblem};

/// A loader with no code, whose sink is the one it was made with.
struct SinkLoader(HyphenTables);

impl CodeLoader for SinkLoader {
    fn load(
        &mut self,
        _: &dyn paperpaw_platform::Storage,
        _: &crate::code_plugin::CodeFile<'_>,
    ) -> Result<Box<dyn Plugin>, PluginProblem> {
        Err(PluginProblem::Changed)
    }

    fn hyphenation_sink(
        &mut self,
        _: &alloc::rc::Rc<dyn paperpaw_platform::Storage>,
    ) -> Option<HyphenTables> {
        Some(self.0.clone())
    }
}

fn with_plugin(storage: MemoryStorage, id: &str, tag: &str, file: &str) -> MemoryStorage {
    storage
        .with(
            &format!("plugins/{id}/plugin.ini"),
            format!("[plugin]\nid = {id}\nversion = 1.0.0\nformats = 5\n\n[hyphenation]\n{tag} = {file}\n"),
        )
        .with(&format!("plugins/{id}/{file}"), "PPHP")
}

fn kernel(storage: MemoryStorage) -> (Kernel, HyphenTables) {
    let sink = HyphenTables::over(alloc::rc::Rc::new(CardTables::new(alloc::rc::Rc::new(
        MemoryStorage::new(),
    ))));
    let kernel = Kernel::builder(DEVICE)
        .storage(storage)
        .code_plugins(Native::new("code.ppn"), Box::new(SinkLoader(sink.clone())))
        .build()
        .unwrap();
    (kernel, sink)
}

#[test]
fn the_sink_gets_the_tables_at_start_and_follows_install_and_removal() {
    let storage = with_plugin(MemoryStorage::new(), "pack-de", "de", "de.pphp");
    let storage = with_plugin(storage, "pack-sv", "sv", "sv.pphp");
    let (mut kernel, sink) = kernel(storage);
    kernel.start(Instant(0));
    assert!(sink.card().find("de").is_some());
    assert!(sink.card().find("sv").is_some());

    // Removed while running: its tables leave the sink before the plugin is
    // gone from the registry.
    kernel.apply(Command::RemovePlugin(Id::new_static("pack-sv")));
    kernel.finish();
    assert!(sink.card().find("de").is_some());
    assert_eq!(sink.card().find("sv"), None);
}

#[test]
fn a_plugin_installed_later_reaches_the_sink() {
    let (mut kernel, sink) = kernel(MemoryStorage::new());
    kernel.start(Instant(0));
    assert_eq!(sink.card().find("nb"), None);
    // Its folder is on the card, then the command loads it.
    kernel.shared.storage.write("plugins/pack-nb/plugin.ini", b"[plugin]\nid = pack-nb\nversion = 1.0.0\nformats = 5\n\n[hyphenation]\nnb = nb.pphp\n").unwrap();
    kernel
        .shared
        .storage
        .write("plugins/pack-nb/nb.pphp", b"PPHP")
        .unwrap();
    kernel.apply(Command::LoadPlugin("plugins/pack-nb".into()));
    kernel.finish();
    assert!(sink.card().find("nb").is_some());
}

#[test]
fn a_card_change_after_a_usb_drive_reaches_the_sink() {
    let (mut kernel, sink) = kernel(MemoryStorage::new());
    kernel.start(Instant(0));
    let before = sink.card().card_epoch();
    kernel.usb_drive_ended(Instant(10));
    assert_ne!(sink.card().card_epoch(), before);
}

#[test]
fn a_list_the_sink_refuses_is_a_warning_and_the_sink_keeps_its_tables() {
    let storage = with_plugin(MemoryStorage::new(), "pack-de", "de", "de.pphp");
    let (mut kernel, sink) = kernel(storage);
    kernel.start(Instant(0));
    kernel.take_warnings();
    // A manifest cannot name a path the sink refuses (100 bytes at most), so
    // the SDK's list is given one directly.
    let long = "x".repeat(600);
    kernel
        .shared
        .hyphenation
        .add_tables(
            "pack-en",
            "plugins/pack-en",
            [("en", long.as_str())].into_iter(),
        )
        .unwrap();
    kernel.feed_hyphenation(None);
    let warnings = kernel.take_warnings();
    assert!(
        warnings
            .iter()
            .any(|w| w.message.contains("not handed to the shell")),
        "{warnings:?}"
    );
    assert!(sink.card().find("de").is_some());
}
