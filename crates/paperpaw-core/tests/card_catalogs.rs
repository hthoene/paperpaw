//! A language from the card, compiled (`i18n/*.ppcat`, ADR 0033): the
//! kernel opens it from a data plugin and reads its messages as they are
//! needed, forgets them when the screen on top changes, and falls back to
//! English where the card is pulled; a catalog of an Arabic interface's
//! size takes about a kilobyte.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::AddDriver;

use std::rc::Rc;

use common::{DEVICE, Harness, press};
use paperpaw_core::Kernel;
use paperpaw_core::sdk::MemoryStorage;
use paperpaw_i18n::Catalog;
use paperpaw_platform::{Instant, Storage, StorageError};

const PATH: &str = "plugins/arabic/i18n/ar.ppcat";

/// The Arabic interface: the kernel's home action, and `n` more.
fn arabic(n: usize) -> Vec<u8> {
    let mut source = String::from(
        "[meta]\nlanguage = ar\nname = العربية\nplural = arabic\ndirection = rtl\n\
         [messages]\ncore.action.home = الرئيسية\n",
    );
    for i in 0..n {
        source.push_str(&format!(
            "filler.{i:04} = نص الرسالة رقم {i} لواجهة المستخدم\n"
        ));
    }
    Catalog::compile(&source).unwrap()
}

/// A card with the data plugin `arabic` whose catalog is `catalog`.
fn card(catalog: Vec<u8>) -> Rc<MemoryStorage> {
    Rc::new(
        MemoryStorage::new()
            .with(
                "plugins/arabic/plugin.ini",
                b"[plugin]\nname = Arabic\nformats = 4\n".to_vec(),
            )
            .with(PATH, catalog),
    )
}

fn started(card: &Rc<MemoryStorage>, language: &str) -> Kernel {
    let harness = Harness::new();
    let mut kernel = Kernel::builder(DEVICE)
        .driver(harness.plugin())
        .storage(Shared(card.clone()))
        .user_config(format!("[core]\nlanguage = {language}\n"))
        .build()
        .unwrap();
    kernel.start(Instant(0));
    common::render(&mut kernel);
    kernel
}

#[test]
fn a_compiled_language_on_the_card_is_read_as_needed() {
    let card = card(arabic(3400));
    let mut kernel = started(&card, "ar");
    let t = kernel.translator();
    assert_eq!(t.language(), "ar");
    assert_eq!(t.direction(), paperpaw_i18n::Direction::Rtl);
    assert_eq!(t.tr("core.action.home"), "الرئيسية");
    assert_eq!(t.tr("filler.3399"), "نص الرسالة رقم 3399 لواجهة المستخدم");
    // English where it has no message.
    assert_eq!(
        t.tr("core.action.menu"),
        Kernel::builder(DEVICE)
            .build()
            .unwrap()
            .translator()
            .tr("core.action.menu")
    );
    assert!(t.read_bytes() > 0);
    // A screen opened: what was read is forgotten as the call ends.
    press(&mut kernel, 1000, "menu");
    assert!(
        kernel.translator().read_bytes() < 2048,
        "{}",
        kernel.translator().read_bytes()
    );
    assert_eq!(kernel.translator().tr("core.action.home"), "الرئيسية");
}

#[test]
fn a_language_of_3400_messages_takes_about_a_kilobyte() {
    let english = started(&card(arabic(0)), "en")
        .translator()
        .indexes_on_heap();
    let kernel = started(&card(arabic(3400)), "ar");
    let index = kernel.translator().indexes_on_heap() - english;
    assert!(index < 1500, "{index}");
    // Unused, it keeps less still.
    let kernel = started(&card(arabic(3400)), "en");
    assert_eq!(kernel.translator().indexes_on_heap(), english);
}

#[test]
fn a_pulled_card_gives_english_and_no_panic() {
    let card = card(arabic(100));
    let mut kernel = started(&card, "ar");
    assert_eq!(kernel.translator().tr("core.action.home"), "الرئيسية");
    card.remove(PATH).unwrap();
    press(&mut kernel, 1000, "menu");
    common::render(&mut kernel);
    let english = Kernel::builder(DEVICE).build().unwrap();
    assert_eq!(
        kernel.translator().tr("core.action.home"),
        english.translator().tr("core.action.home")
    );
    press(&mut kernel, 2000, "back");
    common::render(&mut kernel);
}

#[test]
fn a_damaged_catalog_is_reported_and_the_language_falls_back() {
    let mut bytes = arabic(10);
    bytes[0] = b'X';
    let mut kernel = started(&card(bytes), "ar");
    let warnings: Vec<String> = kernel
        .take_warnings()
        .iter()
        .map(ToString::to_string)
        .collect();
    assert!(
        warnings.iter().any(|w| w.contains("ar.ppcat")),
        "{warnings:?}"
    );
    let english = Kernel::builder(DEVICE).build().unwrap();
    assert_eq!(
        kernel.translator().tr("core.action.home"),
        english.translator().tr("core.action.home")
    );
}

#[test]
fn compiled_catalogs_need_formats_4_and_win_over_their_text() {
    // Made for formats 3: left out, and said so.
    let old = Rc::new(
        MemoryStorage::new()
            .with(
                "plugins/arabic/plugin.ini",
                b"[plugin]\nname = Arabic\nformats = 3\n".to_vec(),
            )
            .with(PATH, arabic(10)),
    );
    let mut kernel = started(&old, "ar");
    let warnings: Vec<String> = kernel
        .take_warnings()
        .iter()
        .map(ToString::to_string)
        .collect();
    assert!(
        warnings.iter().any(|w| w.contains("formats = 4")),
        "{warnings:?}"
    );
    assert_ne!(kernel.translator().tr("core.action.home"), "الرئيسية");
    // The text of the same language beside it is for older versions.
    let both = card(arabic(10));
    both.write(
        "plugins/arabic/i18n/AR.INI",
        "[meta]\nlanguage = ar\n[messages]\ncore.action.home = نص\n".as_bytes(),
    )
    .unwrap();
    let kernel = started(&both, "ar");
    assert_eq!(kernel.translator().tr("core.action.home"), "الرئيسية");
}

/// The card, shared with the test so that it can pull files.
struct Shared(Rc<MemoryStorage>);

impl Storage for Shared {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.0.read_at(path, offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.0.size(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.0.write(path, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.0.append(path, data)
    }
    fn list(
        &self,
        dir: &str,
        visit: &mut dyn FnMut(paperpaw_platform::DirEntry<'_>),
    ) -> Result<(), StorageError> {
        self.0.list(dir, visit)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.0.remove(path)
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.0.rename(from, to)
    }
    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        self.0.create_dir(path)
    }
}
