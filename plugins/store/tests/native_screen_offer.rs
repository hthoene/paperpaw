//! The first-run offer's rules (host-free): which plugins are offered, what
//! *Done* does with those chosen (switch on, queue quietly, keep for the
//! setup's card, notice), and that a refusing card or heap is an error, never
//! a panic.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "fixture errors fail the tests"
)]
use paperpaw_plugin_store::native_ask::{CHOSEN, CODE_TOLD, OFFERED};
use paperpaw_plugin_store::native_page::{Installed, Kind, Origin};
use paperpaw_plugin_store::native_queue::{Queue, read_text};
use paperpaw_plugin_store::native_screen_offer::{
    add, applies, chosen_of, have_of, texts, toggled,
};
use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::Item;
#[path = "../../test-support/fail_alloc.rs"]
mod fail_alloc;
use paperpaw_plugin_store::native_screens::{Installs, Next, Step};
use paperpaw_sdk::{MemoryStorage, Storage};

/// What the device has, as the test sets it: (id, kind, enabled).
struct Device {
    have: Vec<(&'static str, Kind, bool)>,
    can_install: bool,
}

impl Installs for Device {
    fn step(&self, _: &str) -> Step {
        Step::Idle
    }
    fn queued(&self) -> bool {
        false
    }
    fn have(&self, id: &str) -> Option<Installed<'_>> {
        let (_, kind, enabled) = self.have.iter().find(|(known, ..)| *known == id)?;
        Some(Installed {
            kind: *kind,
            enabled: *enabled,
            restart_needed: false,
            version: "1.0.0",
            origin: Origin::Store,
            release: None,
            trouble: None,
        })
    }
    fn can_install(&self) -> bool {
        self.can_install
    }
}

#[test]
fn the_step_applies_where_some_plugin_can_be_added() {
    let nothing = Device {
        have: Vec::new(),
        can_install: false,
    };
    assert!(!applies(&nothing));
    let online = Device {
        have: Vec::new(),
        can_install: true,
    };
    assert!(applies(&online));
    // Everything there and on: nothing to offer.
    let all = Device {
        have: OFFERED.iter().map(|id| (*id, Kind::Code, true)).collect(),
        can_install: true,
    };
    assert!(!applies(&all));
    // One built in and off: it can be switched on without a network.
    let off = Device {
        have: vec![("stats", Kind::BuiltIn, false)],
        can_install: false,
    };
    assert!(applies(&off));
}

#[test]
fn nothing_chosen_is_nothing_to_do() {
    let device = Device {
        have: Vec::new(),
        can_install: true,
    };
    let card = MemoryStorage::new();
    assert_eq!(add(&device, &card, &[], true).unwrap(), Next::Close);
    assert_eq!(add(&device, &card, &[], false).unwrap(), Next::None);
    assert_eq!(read_text(&card, CHOSEN, 64).unwrap(), None);
}

#[test]
fn a_built_in_plugin_is_switched_on_and_a_library_one_is_queued_quietly() {
    let device = Device {
        have: vec![("stats", Kind::BuiltIn, false)],
        can_install: true,
    };
    let card = MemoryStorage::new();
    let next = add(&device, &card, &["stats", "widgets"], true).unwrap();
    assert_eq!(
        next,
        Next::Offered {
            switch_on: vec!["stats"],
            queued: true,
            notify: "store.offer.adding",
            close: true,
        }
    );
    // Kept for the setup's card, and the reader was told what code plugins are.
    assert_eq!(
        read_text(&card, CHOSEN, 64).unwrap().as_deref(),
        Some("stats\nwidgets")
    );
    assert!(card.exists(CODE_TOLD));
    let queue = Queue::read(&card).unwrap();
    let ids: Vec<&str> = queue.lines().iter().map(|line| line.id.as_str()).collect();
    assert_eq!(ids, ["widgets"]);
}

#[test]
fn only_built_in_ones_are_switched_on_and_nothing_is_queued() {
    let device = Device {
        have: vec![("stats", Kind::BuiltIn, false)],
        can_install: false,
    };
    let card = MemoryStorage::new();
    let next = add(&device, &card, &["stats"], false).unwrap();
    assert_eq!(
        next,
        Next::Offered {
            switch_on: vec!["stats"],
            queued: false,
            notify: "store.offer.switched-on",
            close: false,
        }
    );
    assert!(!card.exists(CODE_TOLD));
    assert!(Queue::read(&card).unwrap().lines().is_empty());
}

#[test]
fn a_plugin_there_already_is_neither_switched_on_nor_fetched() {
    let device = Device {
        have: vec![("calibre", Kind::Code, true), ("kosync", Kind::Code, false)],
        can_install: true,
    };
    let card = MemoryStorage::new();
    let Next::Offered {
        switch_on, queued, ..
    } = add(&device, &card, &["calibre", "kosync"], true).unwrap()
    else {
        panic!("an offer");
    };
    assert!(switch_on.is_empty());
    assert!(!queued);
    assert_eq!(
        have_of(device.have("kosync")),
        paperpaw_plugin_store::native_ask::Have::OffOther
    );
}

#[global_allocator]
static ALLOC: fail_alloc::Failing = fail_alloc::Failing;

/// The words of the rows, as a catalog with every key its own text.
struct Catalog;

impl paperpaw_plugin_store::native_screen_list::Say for Catalog {
    fn copy(&mut self, key: &str) -> Result<String, Error> {
        paperpaw_ui_kit::text::copy(key)
    }
    fn format(
        &mut self,
        key: &str,
        _: &[paperpaw_sdk::native::formatting::Arg<'_>],
    ) -> Result<String, Error> {
        paperpaw_ui_kit::text::copy(key)
    }
    fn digits(&mut self, text: &str) -> Result<String, Error> {
        paperpaw_ui_kit::text::copy(text)
    }
}

/// A refused allocation (the list of those chosen) is an error, never a panic.
#[test]
fn the_plugins_chosen_are_listed_or_refused() {
    let ids = ["stats", "widgets", "calibre"];
    let on = [true, false, true];
    fail_alloc::sweep(|| {
        let chosen = chosen_of(&ids, &on)?;
        assert_eq!(chosen, ["stats", "calibre"]);
        Ok(())
    });
}

/// A choice whose rows cannot be made is put back: *Done* never adds a plugin the
/// reader sees unchosen.
#[test]
fn a_refused_toggle_leaves_the_choice_as_the_rows_show_it() {
    let words = fail_alloc::exempt(|| texts(&mut Catalog, &["stats", "widgets"], false).unwrap());
    let mut on = [false, true];
    let refused = std::cell::Cell::new(0);
    fail_alloc::sweep(|| {
        let before = on;
        let result = toggled(&mut on, 0, &words);
        match &result {
            Ok(Some(rows)) => {
                assert!(on[0]);
                let Item::Row(first) = &rows[0] else {
                    panic!("a row");
                };
                assert_eq!(first.value, "store.offer.chosen");
                // The next run of the sweep starts as the first did.
                on = before;
            }
            Ok(None) => panic!("row 0 exists"),
            Err(_) => {
                assert_eq!(on, before, "the choice is back");
                refused.set(refused.get() + 1);
            }
        }
        result.map(|_| ())
    });
    assert!(refused.get() > 0, "the sweep refused some allocation");
    // No such row: nothing changes.
    assert!(toggled(&mut on, 9, &words).unwrap().is_none());
}
