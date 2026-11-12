//! The catalog walk with the formats of native plugins (#243, #245): their
//! files are books as those of an interface 6 format are, the two counted
//! together, a tree with native formats only is walked when a view asks,
//! every book counted on the first walk, and a format plugin that stopped
//! takes its books out at the next walk.

use super::*;
use crate::native_host::native_book_formats::tests::{Plugin as FormatPlugin, declare};
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
fn started(txt: bool) -> (Kernel, Rc<FormatPlugin>, u64) {
    let mut builder = Kernel::builder(DEVICE).storage(books());
    if txt {
        builder = builder.plugin(Txt);
    }
    let mut kernel = builder.build().unwrap();
    let directory = Shared::try_new(NativeDeclarations::default()).unwrap();
    let plugin = Rc::new(FormatPlugin {
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
fn a_format_plugin_that_stopped_keeps_its_books_and_one_taken_out_takes_them_at_the_next_walk() {
    let (mut kernel, plugin, now) = started(true);
    let now = want(&mut kernel, now, false);
    assert_eq!(paths(&kernel).len(), 4);
    // Stopped (a fault, parked) but still installed: its books stay.
    plugin.live.set(false);
    kernel.shared.registry_generation.bump();
    let now = want(&mut kernel, now, true);
    assert!(kernel.shared.formats.supports("books/a.epub"));
    assert_eq!(paths(&kernel).len(), 4);
    // Taken out (removed, turned off): they go at the next walk.
    kernel
        .shared
        .registry
        .native_declarations
        .as_ref()
        .unwrap()
        .store
        .borrow_mut()
        .remove_owner(&paperpaw_sdk::Id::try_parse("format-books").unwrap(), 1);
    kernel.shared.registry_generation.bump();
    want(&mut kernel, now, true);
    assert!(!kernel.shared.formats.supports("books/a.epub"));
    assert_eq!(paths(&kernel), ["books/notes.txt"]);
}

/// The library service's status notes (op 8 `set_status`, #248): past
/// [`catalog::MAX_LOG`] lines the log beside the catalog is folded into it at
/// a calm moment, as after the kernel's own notes, so that a catalog read
/// reserves for at most that many lines (12 bytes each).
#[test]
fn status_notes_of_the_library_service_past_the_logs_cap_are_folded_in() {
    use crate::native_host::lib_host::LibraryHost;
    use crate::native_host::lib_reads::tests::Env;
    use crate::native_host::lib_service::{BookRef, LibWrite};
    let (mut kernel, _plugin, now) = started(false);
    let now = want(&mut kernel, now, false);
    assert_eq!(paths(&kernel).len(), 3, "walked");
    let env = Rc::new(Env {
        walking: Cell::new(None),
        today: Cell::new(None),
    });
    let host = Rc::new(LibraryHost::new(kernel.shared.storage.clone(), env, None));
    // The host services live with the native instances (one, not running).
    kernel
        .shared
        .native_instances
        .test_insert(paperpaw_sdk::Id::try_parse("viewer").unwrap(), None, false)
        .unwrap();
    kernel
        .shared
        .native_instances
        .test_resources()
        .unwrap()
        .hosts()
        .wire_library(host.clone())
        .unwrap();
    // Favourite, not, …: a line of the log each, until it is past its cap.
    let mut notes = 0;
    while !catalog::log_is_long(kernel.storage()) && notes < 4 * catalog::MAX_LOG {
        let (set, clear) = if notes % 2 == 0 { (2, 0) } else { (0, 2) };
        host.set_status(BookRef::Path("books/a.epub"), set, clear)
            .unwrap();
        notes += 1;
    }
    assert!(catalog::log_is_long(kernel.storage()), "past the cap");
    let effects = kernel.tick(Instant(now + 1));
    run(&mut kernel, now + 1, effects);
    let lines = paperpaw_sdk::read_file(kernel.storage(), ".paperpaw/library/status.log", 1 << 16)
        .map_or(0, |log| log.iter().filter(|b| **b == b'\n').count());
    assert!(lines <= catalog::MAX_LOG, "folded in: {lines} lines left");
    assert!(!catalog::log_is_long(kernel.storage()));
    // What was noted last stands in the catalog (an even call set it).
    let mut favourite = None;
    catalog::each(kernel.storage(), &mut |r| {
        if r.path == "books/a.epub" {
            favourite = Some(r.status.favourite);
        }
    });
    assert_eq!(favourite, Some((notes - 1) % 2 == 0));
}
