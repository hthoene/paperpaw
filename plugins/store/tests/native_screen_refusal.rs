//! Every allocation of the start's and the lists' code may be refused: each
//! ends in `NoMemory` or succeeds, never aborts (slice 3, host-free).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "fixture errors fail the tests"
)]
#[path = "../../test-support/fail_alloc.rs"]
mod fail_alloc;
use paperpaw_plugin_store::native_index::{Index, Order, take};
use paperpaw_plugin_store::native_item::Device;
use paperpaw_plugin_store::native_list::Hooks;
use paperpaw_plugin_store::native_page::{Installed, Kind, Origin};
use paperpaw_plugin_store::native_screen_list::{Row, Say, choose, count, items, rows, searched};
use paperpaw_plugin_store::native_screen_source;
use paperpaw_plugin_store::native_screen_view::{
    Source, Which, first_from, hit_entry, open_page, pick, size_text, tops,
};
use paperpaw_plugin_store::native_screens::{Installs, Step};
use paperpaw_sdk::native::formatting::Arg;
use paperpaw_sdk::native::{Error, Fingerprint, INTERFACE};
use paperpaw_sdk::{DirEntry, MemoryStorage, Storage, StorageError};

#[global_allocator]
static ALLOC: fail_alloc::Failing = fail_alloc::Failing;

/// A card whose own bookkeeping is not what the sweep refuses.
struct Card(MemoryStorage);

impl Storage for Card {
    fn read_at(&self, p: &str, o: u64, b: &mut [u8]) -> Result<usize, StorageError> {
        fail_alloc::exempt(|| self.0.read_at(p, o, b))
    }
    fn size(&self, p: &str) -> Result<u64, StorageError> {
        fail_alloc::exempt(|| self.0.size(p))
    }
    fn write(&self, p: &str, d: &[u8]) -> Result<(), StorageError> {
        fail_alloc::exempt(|| self.0.write(p, d))
    }
    fn append(&self, p: &str, d: &[u8]) -> Result<(), StorageError> {
        fail_alloc::exempt(|| self.0.append(p, d))
    }
    fn list(&self, d: &str, v: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        fail_alloc::exempt(|| self.0.list(d, v))
    }
    fn remove(&self, p: &str) -> Result<(), StorageError> {
        fail_alloc::exempt(|| self.0.remove(p))
    }
    fn rename(&self, a: &str, b: &str) -> Result<(), StorageError> {
        fail_alloc::exempt(|| self.0.rename(a, b))
    }
}

/// The words, each made with an allocation of its own.
struct Words;

impl Say for Words {
    fn copy(&mut self, key: &str) -> Result<String, Error> {
        paperpaw_ui_kit::text::copy(key)
    }
    fn format(&mut self, key: &str, _: &[Arg<'_>]) -> Result<String, Error> {
        paperpaw_ui_kit::text::copy(key)
    }
    fn digits(&mut self, text: &str) -> Result<String, Error> {
        paperpaw_ui_kit::text::copy(text)
    }
}

struct Have;

impl Installs for Have {
    fn step(&self, id: &str) -> Step {
        if id == "plugin-0004" {
            Step::Downloading { done: 1, total: 3 }
        } else {
            Step::Idle
        }
    }
    fn queued(&self) -> bool {
        false
    }
    fn have(&self, id: &str) -> Option<Installed<'_>> {
        (id.as_bytes().last().is_some_and(|b| b % 2 == 0)).then_some(Installed {
            kind: Kind::Data,
            enabled: true,
            restart_needed: false,
            version: "0.9.0",
            origin: Origin::Store,
            release: None,
            trouble: None,
        })
    }
}

fn nobody(_: &str) -> Option<[u8; 32]> {
    None
}
fn none() -> Vec<String> {
    Vec::new()
}

fn list(count: usize) -> String {
    let mut text = String::from(r#"{"format": 2, "plugins": ["#);
    for n in 0..count {
        if n > 0 {
            text.push(',');
        }
        text.push_str(&format!(
            r#"{{"id": "plugin-{n:04}", "name": "Äpfel {n}", "summary": "Does {n}.", "group": "look",
 "kind": "data", "author": "A", "licence": "MIT",
 "latest": {{"version": "1.0.0", "sdk": "0.4", "formats": 1, "size": 3000}}}}"#
        ));
    }
    text.push_str(r#"], "sections": {"recommended": ["plugin-0001", "plugin-0002"]}}"#);
    text
}

#[test]
fn counting_picking_and_building_the_rows_survive_refused_allocations() {
    let card = Card(MemoryStorage::new());
    let text = list(8);
    fail_alloc::exempt(|| card.write("list.json", text.as_bytes()).unwrap());
    let device = Device {
        name: "x4",
        fingerprint: Some(Fingerprint::of(INTERFACE, 0x4567_89ab_cdef)),
        code_file: "code.ppn",
        have: None,
        sdk: paperpaw_sdk::SDK_VERSION,
        official: true,
        provides: None,
    };
    let hooks = Hooks {
        author_of: &nobody,
        stopped: &none,
    };
    let index = fail_alloc::exempt(|| {
        take(&card, "list.json", device, hooks)
            .unwrap()
            .expect("a list")
            .0
    });
    let index: &Index = &index;
    let installs = Have;
    fail_alloc::sweep(|| {
        let counts = count(index, &card, &installs)?;
        let list = rows(&counts, true)?;
        items(&mut Words, &list, &counts)?;
        for row in [Row::Search, Row::Installed, Row::Section(0), Row::Group(5)] {
            choose(&mut Words, row)?;
        }
        searched("clock")?;
        // The test's own list of lists is not what is refused.
        let whiches = fail_alloc::exempt(|| {
            vec![
                Which::Group(5),
                Which::Installed,
                Which::New,
                Which::Section(0),
                Which::Search("pfel 3".into()),
            ]
        });
        for which in &whiches {
            let picked = pick(which, Order::Name, index, &card, &installs)?;
            let top = tops(which, picked.hits, 3, true, picked.updates)?;
            which.open()?;
            which.title(&mut Words)?;
            let source = Source {
                index,
                storage: &card,
                installs: &installs,
                top: &top,
                hits: picked.hits,
                order: Order::Name,
            };
            source.rows(&mut Words, 0, 4, "help")?;
        }
        hit_entry(index, &card, 0)?;
        first_from(3, 5, |_| Ok(4))?;
        open_page(3, "clock")?;
        size_text(123_456, ",")?;
        native_screen_source::open("Clock", "https://x.example/clock")?;
        native_screen_source::items("Clock", "Scan it.")?;
        Ok::<(), Error>(())
    });
}
