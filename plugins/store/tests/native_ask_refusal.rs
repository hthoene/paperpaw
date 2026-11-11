//! Every allocation of the store's questions and of the fetch screen's rules may
//! be refused: each ends in `NoMemory` or succeeds, never aborts.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "fixture errors fail the tests"
)]
#[path = "../../test-support/fail_alloc.rs"]
mod fail_alloc;
use paperpaw_plugin_store::native_ask::{self as ask, Decision, Faces, Have, Say};
use paperpaw_plugin_store::native_model::Entry;
use paperpaw_plugin_store::native_queue::Report;
use paperpaw_plugin_store::native_request::Last;
use paperpaw_plugin_store::native_screen_fetch::{Core, Kind};
use paperpaw_plugin_store::native_screens::{Installs, Step, Trouble};
use paperpaw_sdk::native::Error;
use paperpaw_sdk::native::formatting::Arg;
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

/// The host's words: its own allocations are not what is swept.
struct Host;

impl Say for Host {
    fn lookup(&mut self, key: &str) -> Result<Option<String>, Error> {
        Ok(fail_alloc::exempt(|| {
            (key != "core.decimal").then(|| format!("<{key}>"))
        }))
    }
    fn format(&mut self, key: &str, _: &[Arg<'_>]) -> Result<String, Error> {
        Ok(fail_alloc::exempt(|| format!("<{key}>")))
    }
    fn digits(&mut self, text: &str) -> Result<String, Error> {
        Ok(fail_alloc::exempt(|| text.to_string()))
    }
}

struct Going;

impl Installs for Going {
    fn step(&self, id: &str) -> Step {
        if id == "pack-a" {
            Step::Installed
        } else {
            Step::Failed(1)
        }
    }
    fn queued(&self) -> bool {
        false
    }
}

fn strings(ids: &[&str]) -> Vec<String> {
    ids.iter().map(ToString::to_string).collect()
}

#[test]
fn the_requests_survive_refused_allocations() {
    let card = Card(MemoryStorage::new());
    let wanted = strings(&["pack-a", "pack-b"]);
    fail_alloc::sweep(|| {
        fail_alloc::exempt(|| {
            for name in ["request.txt", "request-result.txt", "queue.txt"] {
                let _ = card.remove(name);
            }
        });
        let got = ask::request_ids(["pack-a", "x", "pack-b", "pack-a"], &|_| false)?;
        assert_eq!(got.len(), 3);
        let mut last: Option<Last> = None;
        let reply = ask::asked(
            &card,
            &mut Host,
            None,
            &mut last,
            ("reader", &wanted),
            1,
            (true, false, false),
        )?;
        assert!(matches!(reply, ask::Reply::Dialog(_)));
        let reply = ask::answered(&card, &mut last, 2, true, Decision::Download)?;
        assert!(matches!(reply, ask::Reply::Fetch(_)));
        let done = paperpaw_plugin_store::native_request::finished(&card)?.expect("open");
        ask::finished_answers(&done)?;
        ask::size_text(2_900_000, ".")?;
        ask::shown_size(&mut Host, 2_900_000)?;
        Ok::<(), Error>(())
    });
}

#[test]
fn the_offers_survive_refused_allocations() {
    let card = Card(MemoryStorage::new());
    let lacks = Faces {
        regular: true,
        bold_built_in: false,
        bold_from_pack: false,
        pack_installed: false,
    };
    fail_alloc::sweep(|| {
        fail_alloc::exempt(|| {
            for name in ["queue.txt", "offer-chosen.txt", "styles-tries.txt"] {
                let _ = card.remove(name);
            }
        });
        let have = |_: &str| Have::Absent;
        let rows = ask::rows(&have, true)?;
        let chosen = ask::choose(&rows, &have, true)?;
        ask::keep_chosen(&card, &chosen)?;
        ask::styles_offer(&card, lacks, (true, true, true))?;
        ask::styles_answered(&card, ask::PACK, 200)?;
        ask::tries(&card)?;
        ask::without("a, b, c", "b")?;
        ask::name_of(&mut Host, "stats")?;
        let report = fail_alloc::exempt(|| Report {
            done: "A".into(),
            not: "Bold and italic".into(),
        });
        ask::quiet_notices(&card, &mut Host, &report)?;
        ask::badge_text(&mut Host, ask::Badge::BeingAdded(2))?;
        Ok::<(), Error>(())
    });
}

#[test]
fn the_fetch_screens_rules_survive_refused_allocations() {
    let listed = vec![(
        "pack-a".to_string(),
        Some(Entry {
            id: "pack-a".into(),
            name: "Letters".into(),
            size: 10,
            ..Entry::default()
        }),
    )];
    fail_alloc::sweep(|| {
        let ids = fail_alloc::exempt(|| strings(&["pack-a", "pack-b"]));
        let mut core = Core::new(Kind::Request, "reader", ids, &listed)?;
        core.refresh(&Going, Trouble::None);
        core.message(&mut Host)?;
        core.progress_text(&mut Host)?;
        core.choose(0)?;
        paperpaw_plugin_store::native_screen_fetch::open_next("reader", &core.ids)?;
        paperpaw_plugin_store::native_screen_fetch::request_ids("a\nb")?;
        Ok::<(), Error>(())
    });
}
