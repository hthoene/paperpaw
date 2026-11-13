#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
//! The record of the store's installs: read as interface 6 wrote it, and
//! taken over once where a device kept none, by the first that needs it.
extern crate std;
use super::*;
use crate::native_host::firmware_install::BuiltIns;
use alloc::format;
use paperpaw_sdk::{Id, Manifest, ManifestRef, MemoryStorage, PluginInfo, PluginState};

/// The built-ins of a job's tests, with the plugins the kernel found.
pub(in crate::native_host) struct Found(pub(in crate::native_host) Plugins);

impl Found {
    /// Data plugins `(id, version)` installed, and `core` built in.
    pub(in crate::native_host) fn data(plugins: &[(&'static str, &str)]) -> Self {
        let info = |id: &'static str, version: &str, kind| {
            let version = Version::parse(version).unwrap();
            let manifest = Manifest::new(Id::new_static(id), id, version);
            let manifest = ManifestRef::try_new(manifest, None).unwrap();
            PluginInfo::new(
                manifest,
                PluginState::Running,
                true,
                Default::default(),
                kind,
                None,
                None,
            )
        };
        let mut found = Plugins::new();
        found.push(info("core", "1.0.0", PluginKind::BuiltIn));
        for (id, version) in plugins {
            found.push(info(id, version, PluginKind::Data));
        }
        Self(found)
    }
}

impl BuiltIns for Found {
    fn count(&self) -> usize {
        0
    }
    fn index_of(&self, _: &str) -> Option<usize> {
        None
    }
    fn found(&self) -> Option<&Plugins> {
        Some(&self.0)
    }
}

/// A kernel with data plugins `(id, version)` on `card`.
fn start(card: MemoryStorage, plugins: &[(&str, &str)]) -> crate::Kernel {
    let mut card = card;
    for (id, version) in plugins {
        let ini = format!("[plugin]\nname = {id}\nversion = {version}\nformats = 4\n");
        card = card.with(&format!("plugins/{id}/plugin.ini"), ini);
    }
    crate::Kernel::builder(crate::tests::DEVICE)
        .storage(card)
        .build()
        .unwrap()
}

/// The record as the card keeps it, `None` where there is none.
pub(in crate::native_host) fn record(storage: &dyn Storage) -> Option<std::string::String> {
    let size = usize::try_from(storage.size(STORE_INSTALLS).ok()?).unwrap();
    let mut bytes = alloc::vec![0; size];
    storage.read_at(STORE_INSTALLS, 0, &mut bytes).unwrap();
    Some(std::string::String::from_utf8(bytes).unwrap())
}

#[test]
fn a_device_that_kept_no_record_takes_its_plugins_for_the_stores_once() {
    let kernel = start(
        MemoryStorage::new(),
        &[("cosy", "1.2.0-beta.1"), ("dark", "2.1")],
    );
    let storage = kernel.shared.storage.as_ref();
    // Not at start: the first that needs the record takes it over.
    assert_eq!(record(storage), None);
    adopt(storage, &kernel.shared.plugins);
    let kept = record(storage).unwrap();
    // The built-in plugins (core) are never the store's.
    assert_eq!(kept, "cosy\t1.2.0-beta.1\ndark\t2.1.0\n");
    let read = StoreRecord::read(storage).unwrap();
    let v = |text| Version::parse(text).unwrap();
    assert_eq!(
        read.release("cosy", v("1.2.0-beta.1")),
        Some("1.2.0-beta.1")
    );
    assert_eq!(read.release("dark", v("2.1.0")), Some("2.1.0"));
    assert_eq!(read.release("dark", v("2.0.0")), None);
}

#[test]
fn a_kept_record_is_never_taken_over_again() {
    let card = MemoryStorage::new().with(STORE_INSTALLS, "dark\t1.0.0\n");
    let kernel = start(card, &[("cosy", "1.0.0"), ("dark", "2.0.0")]);
    let storage = kernel.shared.storage.as_ref();
    adopt(storage, &kernel.shared.plugins);
    // Plugins put in place since (cosy) or replaced (dark 2.0.0) stay local.
    assert_eq!(record(storage).as_deref(), Some("dark\t1.0.0\n"));
}

#[test]
fn the_last_line_of_a_release_counts_and_an_unreadable_one_is_none() {
    let card = MemoryStorage::new().with(
        STORE_INSTALLS,
        b"cosy\t1.0.0\nbroken\nodd\t\xff\ncosy\t1.0\n".to_vec(),
    );
    let read = StoreRecord::read(&card).unwrap();
    let one = Version::parse("1.0.0").unwrap();
    assert_eq!(read.release("cosy", one), Some("1.0"));
    assert_eq!(read.release("odd", one), None);
    assert_eq!(read.release("broken", one), None);
    // No record at all: nobody's.
    let none = StoreRecord::read(&MemoryStorage::new()).unwrap();
    assert_eq!(none.release("cosy", one), None);
}
