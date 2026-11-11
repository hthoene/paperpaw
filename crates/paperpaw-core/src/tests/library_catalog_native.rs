//! The catalog walk with the formats of native plugins (#243, #245): their
//! files are books as those of an interface 6 format are, the two counted
//! together, a tree with native formats only is walked when a view asks,
//! every book counted on the first walk, and a format plugin that stopped
//! takes its books out at the next walk.

use super::*;
use crate::native_host::native_book_formats::tests::{Plugin, declare};
use crate::native_host::native_declarations::NativeDeclarations;
use crate::native_host::shared::Shared;
use core::cell::Cell;
use paperpaw_sdk::builtin::events;
use paperpaw_sdk::document::DocumentError;
use paperpaw_sdk::event::EventData;
use paperpaw_sdk::library::catalog;
use paperpaw_sdk::{FormatSpec, MemoryStorage};

/// An interface 6 format of TXT, beside them.
struct Txt;

impl Plugin for Txt {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("txt"), "TXT", Version::new(1, 0, 0))
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_format(FormatSpec::new(
            Id::new_static("txt.txt"),
            "TXT",
            alloc::vec![Cow::Borrowed("txt")],
            Box::new(|_, _, _| Err(DocumentError::UnsupportedFormat)),
        ))
    }
}

fn books() -> MemoryStorage {
    MemoryStorage::new()
        .with("books/a.epub", "one")
        .with("books/B.DOCX", "two")
        .with("books/deep/c.epub", "three")
        .with("books/notes.txt", "four")
        .with("books/cover.jpg", "no book")
        // PaperPaw's own folder holds no book.
        .with(".paperpaw/kept.epub", "no book")
}

/// Runs what is due within a minute and more, as time goes on.
fn run(kernel: &mut Kernel, mut now: u64, mut effects: crate::Effects) -> u64 {
    for _ in 0..2000 {
        match effects.wake_at() {
            Some(at) if at.0 <= now + 61_000 => {
                now = now.max(at.0);
                effects = kernel.tick(Instant(now));
            }
            _ => break,
        }
    }
    now
}

/// Started with the native format plugin `format-books` (EPUB and DOCX)
/// admitted, and the interface 6 TXT where `txt`.
fn started(txt: bool) -> (Kernel, Rc<Plugin>, u64) {
    let mut builder = Kernel::builder(DEVICE).storage(books());
    if txt {
        builder = builder.plugin(Txt);
    }
    let mut kernel = builder.build().unwrap();
    let directory = Shared::try_new(NativeDeclarations::default()).unwrap();
    let plugin = Rc::new(Plugin {
        id: "format-books",
        live: Cell::new(true),
    });
    declare(
        &mut directory.store.borrow_mut(),
        &plugin,
        "books",
        "epub\ndocx",
    );
    kernel.shared.registry.native_declarations = Some(directory);
    kernel.shared.registry_generation.bump();
    let effects = kernel.start(Instant(0));
    let now = run(&mut kernel, 0, effects);
    (kernel, plugin, now)
}

/// The view's ask, and what is due after it.
fn want(kernel: &mut Kernel, now: u64, anew: bool) -> u64 {
    let wanted = events::CatalogWanted::new(anew).to_params();
    let effects = kernel.send_event(Instant(now), events::CATALOG_WANTED.id().clone(), wanted);
    run(kernel, now, effects)
}

fn paths(kernel: &Kernel) -> Vec<String> {
    let mut paths = Vec::new();
    catalog::each(kernel.storage(), &mut |r| paths.push(r.path));
    paths.sort();
    paths
}

#[test]
fn with_native_formats_only_every_book_is_counted_on_the_first_walk() {
    let (mut kernel, _plugin, now) = started(false);
    // Not walked at the start; walked once a view asks, every book at once.
    assert!(paths(&kernel).is_empty());
    want(&mut kernel, now, false);
    assert!(kernel.shared.formats.supports("books/B.DOCX"));
    assert_eq!(
        paths(&kernel),
        ["books/B.DOCX", "books/a.epub", "books/deep/c.epub"]
    );
}

#[test]
fn native_and_interface_6_formats_are_counted_together() {
    let (mut kernel, _plugin, now) = started(true);
    want(&mut kernel, now, false);
    assert_eq!(
        paths(&kernel),
        [
            "books/B.DOCX",
            "books/a.epub",
            "books/deep/c.epub",
            "books/notes.txt"
        ]
    );
}

#[test]
fn a_format_plugin_that_stopped_takes_its_books_out_at_the_next_walk() {
    let (mut kernel, plugin, now) = started(true);
    let now = want(&mut kernel, now, false);
    assert_eq!(paths(&kernel).len(), 4);
    plugin.live.set(false);
    kernel.shared.registry_generation.bump();
    want(&mut kernel, now, true);
    assert!(!kernel.shared.formats.supports("books/a.epub"));
    assert_eq!(paths(&kernel), ["books/notes.txt"]);
}
