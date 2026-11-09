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
    StorageFiles::new(storage.clone(), Rc::default())
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
    assert!(!super::StorageFiles::new(empty, Rc::default()).excluded(1, "", "PAPERP~1"));
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
    fn busy(&self, path: &str, access: Access) -> bool {
        self.busy.get() && path == "Books/a.epub" && access == Access::Write
    }
    fn moving(&self) -> bool {
        self.moving.get()
    }
}

#[test]
fn busy_and_moving_come_from_the_registered_sources_and_nothing_is_busy_without() {
    let sources = Rc::new(BusySources::default());
    let files = StorageFiles::new(card(), sources.clone());
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
    sources.register(one.clone()).unwrap();
    sources.register(two).unwrap();
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

#[test]
fn the_wiring_gives_the_device_core_files_once() {
    let resources = HostResources::try_new().unwrap();
    assert!(resources.hosts().files().is_none());
    let wiring = || HostWiring::new().files(card());
    resources.wire(wiring()).unwrap();
    assert!(resources.hosts().files().is_some());
    assert_eq!(resources.wire(wiring()).err(), Some(ERR_STATE));
    // The sources of the device are the ones the wired host asks.
    let source = Rc::new(Source {
        busy: Cell::new(true),
        moving: Cell::new(true),
    });
    resources.hosts().busy_sources().register(source).unwrap();
    let files = &resources.hosts().files().unwrap().0;
    assert!(files.busy("Books/a.epub", Access::Write));
    assert!(files.moving());
}
