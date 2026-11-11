//! The catalog walk with the formats of native plugins (#243, #245): their
//! files are books as those of an interface 6 format are, the two counted
//! together, and a tree with native formats only is walked when a view asks,
//! every book counted on the first walk.

use super::*;
use paperpaw_sdk::builtin::events;
use paperpaw_sdk::document::DocumentError;
use paperpaw_sdk::event::EventData;
use paperpaw_sdk::library::catalog;
use paperpaw_sdk::{FormatSpec, MemoryStorage, OtherFormats};

/// The native formats as the kernel's hook gives them: EPUB and DOCX.
struct Native;

impl OtherFormats for Native {
    fn any_other_format(&self) -> bool {
        true
    }
    fn other_format_claims(&self, name: &str) -> bool {
        let lower = name.to_ascii_lowercase();
        lower.ends_with(".epub") || lower.ends_with(".docx")
    }
}

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

/// Started with the native formats set before the start (as the first native
/// plugin's admission sets them), and the interface 6 TXT where `txt`.
fn started(txt: bool) -> (Kernel, u64) {
    let mut builder = Kernel::builder(DEVICE).storage(books());
    if txt {
        builder = builder.plugin(Txt);
    }
    let mut kernel = builder.build().unwrap();
    kernel.shared.formats.set_others(Box::new(Native));
    let effects = kernel.start(Instant(0));
    let now = run(&mut kernel, 0, effects);
    (kernel, now)
}

/// The view's ask, and what is due after it.
fn want(kernel: &mut Kernel, now: u64) -> u64 {
    let wanted = events::CatalogWanted::new(false).to_params();
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
    let (mut kernel, now) = started(false);
    assert!(kernel.shared.formats.supports("books/B.DOCX"));
    assert!(!kernel.shared.formats.is_empty());
    // Not walked at the start; walked once a view asks, every book at once.
    assert!(paths(&kernel).is_empty());
    want(&mut kernel, now);
    assert_eq!(
        paths(&kernel),
        ["books/B.DOCX", "books/a.epub", "books/deep/c.epub"]
    );
}

#[test]
fn native_and_interface_6_formats_are_counted_together() {
    let (mut kernel, now) = started(true);
    want(&mut kernel, now);
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
