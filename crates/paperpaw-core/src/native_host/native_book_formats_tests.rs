//! The stand-ins of native formats in `Formats`: the declared extensions are
//! books, an interface 6 format keeps its own, and a plugin that stopped takes
//! its extensions at the next generation.
#![allow(clippy::unwrap_used, reason = "fixture failures must fail the test")]
extern crate std;
use super::*;
use crate::native_host::declaration_store::{Allocations, ImageOwner, PendingDeclarations};
use crate::native_host::registrations::{
    FormatSpec as Declared, REGISTRATION_MAX_BYTES, Record, records_encode,
};
use alloc::{rc::Rc, vec};
use core::cell::Cell;
use paperpaw_sdk::document::DocumentError;

/// A native format plugin, live or stopped.
pub(crate) struct Plugin {
    pub(crate) id: &'static str,
    pub(crate) live: Cell<bool>,
}

impl ImageOwner for Plugin {
    fn plugin_id(&self) -> &str {
        self.id
    }
    fn generation(&self) -> u32 {
        u32::from(self.live.get())
    }
    fn owner_id(&self) -> Result<u64, i32> {
        // One key per plugin id.
        Ok(self.id.bytes().fold(7_u64, |key, byte| {
            key.wrapping_mul(31).wrapping_add(u64::from(byte))
        }))
    }
    fn with_image(&self, visit: &mut dyn FnMut(Option<&[u8]>)) {
        visit(self.live.get().then_some(&[][..]));
    }
    fn invalid_declaration(&self, _: i32) {}
}

/// `plugin` declares the format `name` of `extensions` (`\n` between them).
pub(crate) fn declare(
    store: &mut DeclarationStore,
    plugin: &Rc<Plugin>,
    name: &str,
    extensions: &str,
) {
    let record = Record::FormatSpec(Declared {
        name,
        title: "format.title",
        extensions,
        flags: 0,
        priority: 0,
        token: 3,
    });
    let mut bytes = vec![0; REGISTRATION_MAX_BYTES];
    let len = records_encode(&[record], &mut bytes).unwrap();
    let mut allocations = Allocations::default();
    let source: Rc<dyn ImageOwner> = plugin.clone();
    let mut pending = PendingDeclarations::new(Id::try_parse(plugin.id).unwrap(), source);
    pending
        .stage_batch(&bytes[..len], &mut allocations)
        .unwrap();
    store.prepare(pending, &mut allocations).unwrap().commit();
}

fn plugin(id: &'static str) -> Rc<Plugin> {
    Rc::new(Plugin {
        id,
        live: Cell::new(true),
    })
}

/// The stand-in's extensions (none where there is no stand-in).
fn held(formats: &Formats) -> usize {
    formats
        .iter()
        .find(|spec| spec.id.as_str() == STAND_IN_ID)
        .map_or(0, |spec| spec.extensions.len())
}

/// An interface 6 format of `extension`.
fn interface_6(id: &'static str, extension: &'static str) -> FormatSpec {
    FormatSpec::new(
        Id::new_static(id),
        "I6",
        vec![Cow::Borrowed(extension)],
        Box::new(|_, _, _| Err(DocumentError::UnsupportedFormat)),
    )
}

#[test]
fn the_declared_extensions_are_books_and_go_with_the_plugin_at_the_next_generation() {
    let (mut store, mut formats, mut stand_ins) = (
        DeclarationStore::default(),
        Formats::new(),
        NativeBookFormats::default(),
    );
    stand_ins.sync(&mut formats, &store, 1);
    assert!(formats.is_empty());

    let epub = plugin("format-epub");
    declare(&mut store, &epub, "epub", "epub\nfb2.zip");
    // Made again only where the generation moved.
    stand_ins.sync(&mut formats, &store, 1);
    assert!(!formats.supports("books/a.epub"));
    stand_ins.sync(&mut formats, &store, 2);
    assert!(!formats.is_empty());
    for book in ["books/a.epub", "A.EPUB", "deep/b.Fb2.Zip"] {
        assert!(formats.supports(book), "{book}");
    }
    for other in ["books/a.txt", "epub", "books/a.zip", "a.epubx"] {
        assert!(!formats.supports(other), "{other}");
    }
    // Never opened through `Formats`, and no panic.
    let storage = paperpaw_sdk::MemoryStorage::new().with("a.epub", "x");
    assert!(formats.open(&storage, "a.epub").is_err());
    // Again at the same generation: nothing doubled.
    stand_ins.sync(&mut formats, &store, 3);
    assert_eq!(held(&formats), 2, "epub and fb2.zip, once each");
    assert_eq!(formats.iter().count(), 1, "one stand-in");

    // Stopped (a fault, parked) but still in the store: its books stay.
    epub.live.set(false);
    stand_ins.sync(&mut formats, &store, 4);
    assert!(formats.supports("books/a.epub"));
    assert_eq!(held(&formats), 2);
    // Taken out of the store (removed, turned off): they go.
    store.remove_owner(&Id::try_parse("format-epub").unwrap(), 1);
    stand_ins.sync(&mut formats, &store, 5);
    assert!(formats.is_empty());
    assert!(!formats.supports("books/a.epub"));
}

#[test]
fn an_extension_of_an_interface_6_format_stays_its_own() {
    let (mut store, mut formats, mut stand_ins) = (
        DeclarationStore::default(),
        Formats::new(),
        NativeBookFormats::default(),
    );
    formats.add(interface_6("old.epub", "epub")).unwrap();
    let native = plugin("format-books");
    declare(&mut store, &native, "books", "epub\ndocx");
    declare(&mut store, &plugin("format-epub"), "epub", "epub");
    stand_ins.sync(&mut formats, &store, 2);
    // EPUB is still the interface 6 format's; DOCX is the native one's; the
    // native format left without an extension adds nothing.
    assert_eq!(formats.for_path("a.epub").unwrap().id.as_str(), "old.epub");
    assert_eq!(formats.for_path("a.docx").unwrap().id.as_str(), STAND_IN_ID);
    assert_eq!(
        held(&formats),
        1,
        "docx alone: epub stays the interface 6 one's"
    );
    // Taken out again, the interface 6 format stays.
    store.remove_owner(&Id::try_parse("format-books").unwrap(), 1);
    stand_ins.sync(&mut formats, &store, 3);
    assert!(formats.supports("a.epub"));
    assert!(!formats.supports("a.docx"));
}

/// What the stand-ins hold on the heap, for the PR's memory figure.
#[test]
fn a_stand_in_costs_little() {
    let mut store = DeclarationStore::default();
    declare(&mut store, &plugin("format-epub"), "epub", "epub");
    let mut formats = Formats::new();
    let mut stand_ins = NativeBookFormats::default();
    let live = || crate::native_host::shell::tests::HEAP_LIVE.with(Cell::get);
    let before = live();
    stand_ins.sync(&mut formats, &store, 2);
    let held = live().saturating_sub(before);
    // The `FormatSpec` in `Formats` (its list grows from empty here), the
    // texts and the id kept here.
    std::println!(
        "one native format stand-in: {held} B held (FormatSpec {} B)",
        core::mem::size_of::<FormatSpec>()
    );
    assert!(held < 1024, "{held} B");
}

#[test]
fn a_sync_the_heap_refused_is_made_again_at_the_next_call_not_the_next_generation() {
    // Each allocation of the sync in turn: the lists' room, the copies,
    // `Formats`' room.
    for nth in 1..=16 {
        let mut store = DeclarationStore::default();
        declare(&mut store, &plugin("format-docx"), "docx", "docx");
        declare(&mut store, &plugin("format-mobi"), "mobi", "mobi\nazw3");
        let (mut formats, mut stand_ins) = (Formats::new(), NativeBookFormats::default());
        crate::native_host::shell::tests::arm_failure(nth);
        stand_ins.sync(&mut formats, &store, 2);
        crate::native_host::shell::tests::arm_failure(0);
        // Whatever was cut short, the same generation's next call makes it
        // whole, and nothing is there twice.
        stand_ins.sync(&mut formats, &store, 2);
        for book in ["a.docx", "b.mobi", "c.azw3"] {
            assert!(formats.supports(book), "{book} after failure {nth}");
        }
        assert_eq!(held(&formats), 3, "after failure {nth}");
        assert_eq!(formats.iter().count(), 1, "after failure {nth}");
        assert_eq!(stand_ins.synced, 2);
    }
}

#[test]
fn a_plugin_turned_off_and_on_again_has_its_stand_ins_once() {
    let (mut store, mut formats, mut stand_ins) = (
        DeclarationStore::default(),
        Formats::new(),
        NativeBookFormats::default(),
    );
    declare(&mut store, &plugin("format-docx"), "docx", "docx");
    stand_ins.sync(&mut formats, &store, 2);
    // Turned off and on again between two syncs: taken out of the store and
    // declared anew.
    store.remove_owner(&Id::try_parse("format-docx").unwrap(), 1);
    declare(&mut store, &plugin("format-docx"), "docx", "docx");
    // Within the same generation nothing is made again, and its books stay.
    stand_ins.sync(&mut formats, &store, 2);
    assert!(formats.supports("a.docx"));
    assert_eq!(held(&formats), 1);
    // At the next one its stand-in is made again, once.
    stand_ins.sync(&mut formats, &store, 3);
    assert!(formats.supports("a.docx"));
    assert_eq!(held(&formats), 1);
    assert_eq!(formats.for_path("a.docx").unwrap().id.as_str(), STAND_IN_ID);
    // Declared anew and then stopped (a fault): its stand-in stays, once.
    let docx = plugin("format-docx");
    store.remove_owner(&Id::try_parse("format-docx").unwrap(), 1);
    declare(&mut store, &docx, "docx", "docx");
    docx.live.set(false);
    stand_ins.sync(&mut formats, &store, 4);
    assert!(formats.supports("a.docx"));
    assert_eq!(held(&formats), 1);
}

/// Ten native format plugins of two extensions each: one stand-in, not ten
/// (#248: ten stand-ins held about 2.3 KiB at the X4's start peak).
#[test]
fn ten_native_formats_are_one_stand_in_of_little_memory() {
    let mut store = DeclarationStore::default();
    let names = [
        "format-a", "format-b", "format-c", "format-d", "format-e", "format-f", "format-g",
        "format-h", "format-i", "format-j",
    ];
    for (n, id) in names.iter().enumerate() {
        let list = std::format!("x{n}\ny{n}");
        declare(&mut store, &plugin(*id), "f", &list);
    }
    let mut formats = Formats::new();
    let mut stand_ins = NativeBookFormats::default();
    let live = || crate::native_host::shell::tests::HEAP_LIVE.with(Cell::get);
    let before = live();
    stand_ins.sync(&mut formats, &store, 2);
    let bytes = live().saturating_sub(before);
    std::println!("ten native formats, twenty extensions: one stand-in, {bytes} B held");
    assert_eq!(formats.iter().count(), 1);
    assert_eq!(held(&formats), 20);
    assert!(formats.supports("a.x0") && formats.supports("b.Y9"));
    assert!(bytes < 1_100, "{bytes} B");
}
