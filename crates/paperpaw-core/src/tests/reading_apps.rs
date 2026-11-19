//! Which reading app opens a book on interface 7 (#260): the native reading
//! apps feed the choice, a format's setting, a book's and a folder's kept
//! choice win in that order, and with one app there is nothing to choose.
//! A second (fake) reading app, `comics.app`, beside the shipped one.
use super::*;
use crate::native_host::declaration_store::{Allocations, ImageOwner, PendingDeclarations};
use crate::native_host::native_book_formats::tests::Plugin;
use crate::native_host::native_declarations::NativeDeclarations;
use crate::native_host::registrations::{
    FormatSpec as Format, REGISTRATION_MAX_BYTES, ReaderSpec as Reader, Record, records_encode,
};
use crate::native_host::shared::Shared;
use core::cell::Cell;
use paperpaw_sdk::MemoryStorage;

const READER: &str = "reader.screen";
const COMICS: &str = "comics.screen";

/// A page that shows nothing.
struct Page;

impl crate::testing::Fake for Page {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Place<'_>) {}

    fn on_action(&mut self, _: &Id, _: &mut Turn<'_, '_>) -> Handled {
        Handled::No
    }
}

/// A reading app's screen as plugin `plugin`'s; the shipped one is bound to
/// the slot.
fn screen(id: &'static str) -> impl Fn(&mut Staging) -> Result<(), RegisterError> {
    move |r| {
        r.add_fake(crate::testing::FakeSpec::new(
            Id::new_static(id),
            "page",
            Box::new(|_, _| Box::new(Page)),
        ))?;
        if id == READER {
            r.bind_slot(slots::READER, Id::new_static(READER), Priority::DEFAULT)?;
        }
        Ok(())
    }
}

fn manifest(plugin: &'static str) -> Manifest {
    Manifest::new(Id::new_static(plugin), "Pages", Version::new(1, 0, 0))
        .uses(paperpaw_sdk::Uses::NOTHING)
}

fn install(
    store: &mut crate::native_host::declaration_store::DeclarationStore,
    id: &'static str,
    records: &[Record<'_>],
) {
    let plugin = Rc::new(Plugin {
        id,
        live: Cell::new(true),
    });
    let mut bytes = alloc::vec![0; REGISTRATION_MAX_BYTES];
    let len = records_encode(records, &mut bytes).unwrap();
    let mut allocations = Allocations::default();
    let source: Rc<dyn ImageOwner> = plugin;
    let mut pending = PendingDeclarations::new(Id::parse(id).unwrap(), source);
    pending
        .stage_batch(&bytes[..len], &mut allocations)
        .unwrap();
    store.prepare(pending, &mut allocations).unwrap().commit();
}

fn format<'a>(name: &'a str, extensions: &'a str) -> Record<'a> {
    Record::FormatSpec(Format {
        name,
        title: "fmt.title",
        extensions,
        flags: 0,
        priority: 0,
        token: 3,
    })
}

fn reader<'a>(screen: &'a str, formats: &'a str) -> Record<'a> {
    Record::ReaderSpec(Reader {
        name: "app",
        screen,
        formats,
        flags: 0,
    })
}

fn books(kept: &str) -> MemoryStorage {
    let card = MemoryStorage::new()
        .with("books/a.epub", "a")
        .with("books/b.cbz", "b")
        .with("books/manga/c.epub", "c")
        .with("books/manga/deep/d.epub", "d");
    if kept.is_empty() {
        card
    } else {
        card.with(crate::reading_apps::FILE, kept)
    }
}

/// Started on `card` with the formats EPUB and CBZ, the shipped reader and,
/// where `comics`, a second reading app of both formats.
fn started(card: MemoryStorage, config: &str, comics: bool) -> Kernel {
    let mut kernel = Kernel::builder(DEVICE)
        .storage(card)
        .user_config(alloc::format!("[core]\n{config}"))
        .items(manifest("reader"), screen(READER))
        .items(manifest("comics"), screen(COMICS))
        .build()
        .unwrap();
    let directory = Shared::try_new(NativeDeclarations::default()).unwrap();
    {
        let mut store = directory.store.borrow_mut();
        install(
            &mut store,
            "fmt",
            &[format("epub", "epub"), format("cbz", "cbz")],
        );
        install(&mut store, "reader", &[reader(READER, "")]);
        if comics {
            install(&mut store, "comics", &[reader(COMICS, "fmt.epub\nfmt.cbz")]);
        }
    }
    kernel.shared.registry.native_declarations = Some(directory);
    kernel.shared.registry_generation.bump();
    kernel.start(Instant(0));
    kernel
}

fn opened(kernel: &mut Kernel, path: &str) -> String {
    kernel.open_book(Instant(100), path);
    kernel
        .active_screen()
        .map(|id| id.to_string())
        .unwrap_or_default()
}

/// An open as a library's book menu makes it: in `screen` (none: the slot's),
/// keeping the choice for the book or its folder.
fn keep(kernel: &mut Kernel, path: &str, screen: Option<&str>, keep: &str) -> String {
    let params = paperpaw_sdk::Params::new()
        .with("path", path)
        .with(crate::reading_apps::KEEP, keep);
    match screen {
        Some(screen) => kernel.apply(Ask::OpenInSlot(
            slots::READER,
            Id::parse(screen).unwrap(),
            params,
        )),
        None => kernel.apply(Ask::OpenSlot(slots::READER, params)),
    }
    kernel.finish();
    kernel
        .active_screen()
        .map(|id| id.to_string())
        .unwrap_or_default()
}

#[test]
fn a_formats_reading_app_opens_its_books() {
    let config = "reading-app.fmt.cbz = comics.app\n";
    let mut kernel = started(books(""), config, true);
    let warnings = kernel.take_warnings();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(opened(&mut kernel, "books/b.cbz"), COMICS);
    assert_eq!(opened(&mut kernel, "books/a.epub"), READER);
    // The default app, for the formats without their own.
    kernel
        .shared
        .settings
        .set_from_text("core.reading-app", "comics.app")
        .unwrap();
    assert_eq!(opened(&mut kernel, "books/a.epub"), COMICS);
    // An app that is gone is passed over.
    kernel
        .shared
        .settings
        .set_from_text("core.reading-app", "gone.app")
        .unwrap();
    assert_eq!(opened(&mut kernel, "books/a.epub"), READER);
}

#[test]
fn a_book_kept_with_its_reading_app_opens_there_after_a_restart() {
    let config = "reading-app.fmt.epub = comics.app\n";
    let mut kernel = started(books(""), config, true);
    // Kept with the shipped reader: over the format's choice.
    assert_eq!(
        keep(&mut kernel, "books/a.epub", Some(READER), "book"),
        READER
    );
    assert_eq!(opened(&mut kernel, "books/manga/c.epub"), COMICS);
    assert_eq!(opened(&mut kernel, "books/a.epub"), READER);
    let kept = paperpaw_sdk::read_file(kernel.storage(), crate::reading_apps::FILE, 4096).unwrap();
    assert_eq!(kept, b"b\treader.app\tbooks/a.epub\n");
    // Read once at the next start.
    let mut again = started(books("b\treader.app\tbooks/a.epub\n"), config, true);
    assert_eq!(opened(&mut again, "books/a.epub"), READER);
    // *Standard* forgets it.
    keep(&mut again, "books/a.epub", None, "book");
    assert_eq!(opened(&mut again, "books/a.epub"), COMICS);
}

#[test]
fn a_folders_reading_app_opens_the_books_in_it() {
    let mut kernel = started(books(""), "", true);
    assert_eq!(
        keep(&mut kernel, "books/manga/c.epub", Some(COMICS), "folder"),
        COMICS
    );
    assert_eq!(opened(&mut kernel, "books/manga/deep/d.epub"), COMICS);
    assert_eq!(opened(&mut kernel, "books/a.epub"), READER);
    // A nearer folder's choice wins; the book's own over both.
    keep(
        &mut kernel,
        "books/manga/deep/d.epub",
        Some(READER),
        "folder",
    );
    assert_eq!(opened(&mut kernel, "books/manga/deep/d.epub"), READER);
    assert_eq!(opened(&mut kernel, "books/manga/c.epub"), COMICS);
    keep(&mut kernel, "books/manga/c.epub", Some(READER), "book");
    assert_eq!(opened(&mut kernel, "books/manga/c.epub"), READER);
}

#[test]
fn with_one_reading_app_there_is_nothing_to_choose() {
    let kept = "b\tcomics.app\tbooks/a.epub\n";
    let mut kernel = started(books(kept), "reading-app.fmt.epub = comics.app\n", false);
    let spec = |kernel: &Kernel, id: &str| kernel.shared.settings.spec(id).cloned();
    let default = spec(&kernel, "core.reading-app").unwrap();
    assert!(crate::reading_apps::nothing_to_choose(&default));
    assert!(spec(&kernel, "core.reading-app.fmt.epub").is_none());
    assert_eq!(opened(&mut kernel, "books/a.epub"), READER);
    // Two apps: the default and each format's offer them both.
    let kernel = started(books(""), "", true);
    let default = spec(&kernel, "core.reading-app").unwrap();
    assert!(!crate::reading_apps::nothing_to_choose(&default));
    let SettingKind::Choice(choices) = &default.kind else {
        panic!("{:?}", default.kind);
    };
    assert_eq!(
        choices.iter().collect::<Vec<_>>(),
        ["core.reading-app.default", "reader.app", "comics.app"]
    );
    let epub = spec(&kernel, "core.reading-app.fmt.epub").unwrap();
    assert!(!crate::reading_apps::nothing_to_choose(&epub));
}

/// `command.pass_on` (#369 G) as the native host stages it for the top screen.
fn pass_on(kernel: &mut Kernel) -> String {
    let key = paperpaw_sdk::ScreenKey::new(kernel.stack.last().unwrap().serial);
    kernel.apply(Ask::PassOn(
        key,
        paperpaw_native::host_glue::ERR_UNSUPPORTED,
    ));
    kernel.finish();
    kernel
        .active_screen()
        .map(|id| id.to_string())
        .unwrap_or_default()
}

#[test]
fn a_reading_app_that_cannot_open_a_book_passes_it_on_and_never_loops() {
    let config = "reading-app.fmt.cbz = comics.app\n";
    let mut kernel = started(books(""), config, true);
    assert_eq!(opened(&mut kernel, "books/b.cbz"), COMICS);
    // The format's app cannot: the slot's own reader gets the book.
    assert_eq!(pass_on(&mut kernel), READER);
    assert_eq!(
        kernel.stack.last().unwrap().params.get("path"),
        Some("books/b.cbz")
    );
    // Neither can it: no app is left, the reader closes.
    let left = pass_on(&mut kernel);
    assert!(left != READER && left != COMICS, "{left}");
    assert!(
        kernel
            .stack
            .iter()
            .all(|s| s.slot.as_ref() != Some(&slots::READER))
    );
    // The next open of the book starts afresh with its own app.
    assert_eq!(opened(&mut kernel, "books/b.cbz"), COMICS);
}

#[test]
fn only_a_screen_of_the_reading_slot_passes_a_book_on() {
    let mut kernel = started(books(""), "", true);
    let home = kernel.active_screen().map(|id| id.to_string());
    let key = paperpaw_sdk::ScreenKey::new(kernel.stack.last().unwrap().serial);
    kernel.apply(Ask::PassOn(key, -1));
    kernel.finish();
    assert_eq!(kernel.active_screen().map(|id| id.to_string()), home);
}
