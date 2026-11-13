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
    let read = StoreRecord::read(storage, 8);
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
    let read = StoreRecord::read(&card, 8);
    let one = Version::parse("1.0.0").unwrap();
    assert_eq!(read.release("cosy", one), Some("1.0"));
    assert_eq!(read.release("odd", one), None);
    assert_eq!(read.release("broken", one), None);
    // No record at all: nobody's.
    let none = StoreRecord::read(&MemoryStorage::new(), 8);
    assert_eq!(none.release("cosy", one), None);
}

/// The card, where the record cannot be written (`write`) or the card
/// cannot say whether there is one (`size`).
pub(in crate::native_host) struct RecordFails<'a> {
    pub(in crate::native_host) card: &'a MemoryStorage,
    pub(in crate::native_host) write: bool,
    pub(in crate::native_host) size: bool,
}

impl Storage for RecordFails<'_> {
    fn read_at(&self, p: &str, at: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.card.read_at(p, at, buf)
    }
    fn size(&self, p: &str) -> Result<u64, StorageError> {
        if self.size && p == STORE_INSTALLS {
            return Err(StorageError::Io);
        }
        self.card.size(p)
    }
    fn write(&self, p: &str, d: &[u8]) -> Result<(), StorageError> {
        if self.write && p == STORE_INSTALLS {
            return Err(StorageError::Io);
        }
        self.card.write(p, d)
    }
    fn append(&self, p: &str, d: &[u8]) -> Result<(), StorageError> {
        self.card.append(p, d)
    }
    fn list(
        &self,
        d: &str,
        v: &mut dyn FnMut(paperpaw_platform::DirEntry<'_>),
    ) -> Result<(), StorageError> {
        self.card.list(d, v)
    }
    fn remove(&self, p: &str) -> Result<(), StorageError> {
        self.card.remove(p)
    }
    fn rename(&self, f: &str, t: &str) -> Result<(), StorageError> {
        self.card.rename(f, t)
    }
    fn create_dir(&self, p: &str) -> Result<(), StorageError> {
        self.card.create_dir(p)
    }
    fn remove_tree_step(
        &self,
        root: &str,
        keep_root: bool,
        place: u64,
    ) -> Result<paperpaw_platform::tree::TreeStep, StorageError> {
        self.card.remove_tree_step(root, keep_root, place)
    }
    fn remove_dir_all(&self, p: &str) -> Result<(), StorageError> {
        self.card.remove_dir_all(p)
    }
    fn sync(&self) -> Result<(), StorageError> {
        self.card.sync()
    }
}

/// Adoption says whether the record is kept, and never writes over one the
/// card could not tell of (Senior R1, R2 on #1388).
#[test]
fn adoption_says_whether_the_record_is_kept_and_a_card_error_writes_nothing() {
    let found = Found::data(&[("cosy", "2.1")]);
    let card = MemoryStorage::new().with(STORE_INSTALLS, "dark\t1.0.0\n");
    let unsure = RecordFails {
        card: &card,
        write: false,
        size: true,
    };
    assert!(!adopt(&unsure, &found.0));
    assert_eq!(record(&card).as_deref(), Some("dark\t1.0.0\n"));
    let card = MemoryStorage::new();
    let unwritable = RecordFails {
        card: &card,
        write: true,
        size: false,
    };
    assert!(!adopt(&unwritable, &found.0));
    assert!(adopt(&card, &found.0));
    assert!(adopt(&card, &found.0), "kept before");
    assert_eq!(record(&card).as_deref(), Some("cosy\t2.1.0\n"));
}

/// A record longer than the plugins can have is read from its end (its
/// newest lines), at most four lines a plugin (Senior R2 on #1388).
#[test]
fn a_long_record_is_read_from_its_newest_lines_only() {
    let mut text = std::string::String::from("first\t1.0.0\n");
    for n in 0..100 {
        text.push_str(&format!("gone-{n}\t1.0.0\n"));
    }
    text.push_str("cosy\t2.1\n");
    let card = MemoryStorage::new().with(STORE_INSTALLS, text.as_str());
    let one = Version::parse("1.0.0").unwrap();
    let read = StoreRecord::read(&card, 1);
    assert_eq!(
        read.release("cosy", Version::parse("2.1.0").unwrap()),
        Some("2.1")
    );
    assert_eq!(read.release("first", one), None, "past the cap");
    assert_eq!(read.release("gone-99", one), Some("1.0.0"));
    // Read whole where the device has plugins for it.
    assert_eq!(
        StoreRecord::read(&card, 100).release("first", one),
        Some("1.0.0")
    );
}
