//! `core.files` over the shell's storage: the identity of the host's folders
//! (the safe fallback), the busy sources, and the wiring.
#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]
extern crate std;
use super::*;
use crate::native_host::{resources::HostResources, shell_wiring::HostWiring};
use alloc::rc::Rc;
use core::cell::Cell;
use paperpaw_native::host_glue::ERR_STATE;
use paperpaw_sdk::MemoryStorage;

fn files(storage: &Rc<MemoryStorage>) -> StorageFiles {
    StorageFiles::new(storage.clone(), Rc::default(), Rc::default())
}

fn card() -> Rc<MemoryStorage> {
    let card = Rc::new(MemoryStorage::new());
    card.write(".paperpaw/state", b"x").unwrap();
    card.write("plugins/a/plugin.ppn", b"x").unwrap();
    card.write("Books/a.epub", b"x").unwrap();
    card
}

#[test]
fn the_host_folders_are_known_by_spelling_in_any_case() {
    let files = files(&card());
    assert!(files.excluded(1, "", "plugins"));
    assert!(files.excluded(1, "", "PLUGINS"));
    assert!(files.excluded(1, "", ".Paperpaw"));
    assert!(files.excluded(0, ".paperpaw/plugins/a", ".RECEIVED"));
    // The driver's fold: `ı` is `I`, `ſ` is `S` (and `ß` is `SS`).
    assert!(files.excluded(1, "", "plugıns"));
    assert!(files.excluded(1, "", "pluginſ"));
    assert!(files.excluded(0, ".paperpaw/plugins/a", ".receıved"));
    assert!(!files.excluded(1, "", "plugin\u{df}"));
    // The kernel's settings file, at the card's top only.
    for name in [
        "paperpaw.ini",
        "PAPERPAW.INI",
        "Paperpaw.Ini",
        "paperpaw.ını",
    ] {
        assert!(files.excluded(1, "", name), "{name}");
    }
    for name in ["paperpaw.inx", "paperpaw.ini2", "xpaperpaw.ini"] {
        assert!(!files.excluded(1, "", name), "{name}");
    }
    assert!(!files.excluded(0, ".paperpaw/plugins/a", "paperpaw.ini"));
    assert!(!files.excluded(1, "", "Books"));
    assert!(!files.excluded(0, ".paperpaw/plugins/a", "notes"));
    assert!(!files.excluded(2, "", "plugins"), "only roots 0 and 1 hide");
}

#[test]
fn an_8_3_alias_of_a_host_folder_is_refused_and_a_name_that_is_nothing_is_not() {
    let card = card();
    // The storage finds an entry under the alias (the fake card stands one
    // up as an entry of its own).
    card.write("PAPERP~1/x", b"x").unwrap();
    let files = files(&card);
    assert!(
        files.excluded(1, "", "PAPERP~1"),
        "an alias next to .paperpaw"
    );
    // A tilde name that is not there, and a name without a tilde.
    assert!(!files.excluded(1, "", "GONE~1"));
    assert!(!files.excluded(1, "", "Books"));
    // The fallback over-refuses a user's own tilde name; it never lets one
    // through (documented in the module).
    card.write("MYDOCU~1/x", b"x").unwrap();
    assert!(files.excluded(1, "", "MYDOCU~1"));
    // With no host folder in that folder there is nothing to be an alias of.
    let empty = Rc::new(MemoryStorage::new());
    empty.write("PAPERP~1/x", b"x").unwrap();
    assert!(!files.excluded(0, "nowhere", "RECEIV~1"));
    assert!(
        !super::StorageFiles::new(empty, Rc::default(), Rc::default()).excluded(1, "", "PAPERP~1")
    );
}

#[test]
fn an_alias_of_received_is_found_under_the_plugins_data_folder() {
    let card = card();
    card.write(".paperpaw/plugins/a/.received/1/body", b"x")
        .unwrap();
    card.write(".paperpaw/plugins/a/RECEIV~1/x", b"x").unwrap();
    let files = files(&card);
    assert!(files.excluded(0, ".paperpaw/plugins/a", "RECEIV~1"));
    // Another plugin's folder has no `.received`: nothing to alias.
    card.write(".paperpaw/plugins/b/RECEIV~1/x", b"x").unwrap();
    assert!(!files.excluded(0, ".paperpaw/plugins/b", "RECEIV~1"));
}

#[test]
fn a_name_too_long_for_the_check_is_refused_not_guessed() {
    let files = files(&card());
    let long = std::format!("{}~1", "n".repeat(400));
    assert!(files.excluded(1, "", &long));
}

struct Source {
    busy: Cell<bool>,
    moving: Cell<bool>,
}
impl BusySource for Source {
    fn busy(&self, _: &dyn Storage, path: &str, access: Access) -> bool {
        self.busy.get() && path == "Books/a.epub" && access == Access::Write
    }
    fn moving(&self) -> bool {
        self.moving.get()
    }
}

#[test]
fn busy_and_moving_come_from_the_registered_sources_and_nothing_is_busy_without() {
    let sources = Rc::new(BusySources::default());
    let files = StorageFiles::new(card(), sources.clone(), Rc::default());
    assert!(!files.busy("Books/a.epub", Access::Write));
    assert!(!files.moving());
    let one = Rc::new(Source {
        busy: Cell::new(true),
        moving: Cell::new(false),
    });
    let two = Rc::new(Source {
        busy: Cell::new(false),
        moving: Cell::new(true),
    });
    sources.register(BusyKind::Network, one.clone()).unwrap();
    sources.register(BusyKind::LibraryMoves, two).unwrap();
    assert!(
        files.busy("Books/a.epub", Access::Write),
        "the first holds it"
    );
    assert!(!files.busy("Books/a.epub", Access::Remove));
    assert!(!files.busy("Books/b.epub", Access::Write));
    assert!(files.moving(), "the second says a move runs");
    one.busy.set(false);
    assert!(!files.busy("Books/a.epub", Access::Write));
}

fn quiet() -> Rc<Source> {
    Rc::new(Source {
        busy: Cell::new(false),
        moving: Cell::new(false),
    })
}

#[test]
fn the_wiring_refuses_core_files_until_every_kind_of_busy_state_has_a_source() {
    let resources = HostResources::try_new().unwrap();
    let wiring = || HostWiring::new().files(card());
    // None, then some: nothing is wired, and the refusal is `ERR_STATE`. The
    // network's source is the host's own and is there from the first call.
    assert_eq!(resources.wire(wiring()).err(), Some(ERR_STATE));
    let hosts = resources.hosts();
    let sources = hosts.busy_sources();
    assert!(sources.has(BusyKind::Network));
    assert!(
        !sources.has(BusyKind::ReaderBook),
        "the books are reported by the document service, not assumed"
    );
    sources.register(BusyKind::LibraryMoves, quiet()).unwrap();
    assert_eq!(resources.wire(wiring()).err(), Some(ERR_STATE));
    assert!(hosts.files().is_none(), "nothing half wired");
    // The document service reports its books: the last source. Now it wires,
    // once; the report can be registered once only.
    hosts.register_reader_book_reporting().unwrap();
    assert_eq!(hosts.register_reader_book_reporting(), Err(ERR_STATE));
    resources.wire(wiring()).unwrap();
    assert!(hosts.files().is_some());
    assert_eq!(resources.wire(wiring()).err(), Some(ERR_STATE));
}

#[test]
fn the_wired_host_asks_the_device_sources() {
    let resources = HostResources::try_new().unwrap();
    let sources = resources.hosts().busy_sources();
    let moving = Rc::new(Source {
        busy: Cell::new(true),
        moving: Cell::new(true),
    });
    sources.register(BusyKind::LibraryMoves, moving).unwrap();
    resources.hosts().register_reader_book_reporting().unwrap();
    resources.wire(HostWiring::new().files(card())).unwrap();
    let files = &resources.hosts().files().unwrap().0;
    assert!(files.busy("Books/a.epub", Access::Write));
    assert!(files.moving());
}

#[test]
fn what_the_network_holds_in_use_is_the_network_source_of_core_files() {
    let resources = HostResources::try_new().unwrap();
    let hosts = resources.hosts();
    // Registered once, however many times the wiring asks, and with neither
    // `core.net` nor `core.sockets` wired.
    hosts.register_in_use_source().unwrap();
    hosts.register_in_use_source().unwrap();
    let sources = hosts.busy_sources();
    assert!(sources.has(BusyKind::Network));
    assert!(!sources.complete());
    sources.register(BusyKind::LibraryMoves, quiet()).unwrap();
    hosts.register_reader_book_reporting().unwrap();
    assert!(hosts.net().is_none() && hosts.sockets().is_none());
    resources.wire(HostWiring::new().files(card())).unwrap();
    let files = &hosts.files().unwrap().0;
    assert!(!files.busy("Books/a.epub", Access::Write));
    hosts.in_use().claim(7, "Books/a.epub").unwrap();
    for access in [Access::Write, Access::Remove, Access::Rename] {
        assert!(files.busy("Books/a.epub", access), "{access:?}");
        assert!(files.busy("Books", access), "the folder that holds it");
    }
    assert!(!files.busy("Books/b.epub", Access::Write));
    // The test card keeps case; a card that does not asks the table folded.
    assert!(!files.busy("books/A.EPUB", Access::Write));
    assert!(hosts.in_use().holds("books/A.EPUB", true));
    hosts.in_use().release(7, "Books/a.epub");
    assert!(!files.busy("Books/a.epub", Access::Write));
}

#[test]
fn a_linked_plugin_is_built_in_and_a_loaded_one_is_not() {
    let linked = Rc::new(crate::native_host::linked_plugins::LinkedPlugins::default());
    let files = StorageFiles::new(card(), Rc::default(), linked.clone());
    assert!(!files.built_in("home"));
    linked.add("home").unwrap();
    assert!(files.built_in("home"));
    assert!(!files.built_in("other") && !files.built_in("hom"));
    linked.remove("home");
    assert!(!files.built_in("home"));
}
