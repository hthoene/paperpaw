//! The index kept on the card: written from the list, read back, searched and
//! ordered, however many plugins there are (slice 4, host-free).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "fixture errors fail the tests"
)]
use paperpaw_plugin_store::native_index::{HEAD, HITS, Index, Order, build, hit, name_key, take};
use paperpaw_plugin_store::native_item::Device;
use paperpaw_plugin_store::native_list::Hooks;
use paperpaw_sdk::native::{Fingerprint, INTERFACE};
use paperpaw_sdk::{MemoryStorage, Storage};

const FINGERPRINT: Fingerprint = Fingerprint::of(INTERFACE, 0x4567_89ab_cdef);

fn device() -> Device<'static> {
    Device {
        name: "x4",
        fingerprint: Some(FINGERPRINT),
        code_file: "code.ppn",
        have: None,
        sdk: paperpaw_sdk::SDK_VERSION,
        official: true,
        provides: None,
    }
}

fn nobody(_: &str) -> Option<[u8; 32]> {
    None
}
fn none() -> Vec<String> {
    Vec::new()
}
fn hooks() -> Hooks<'static> {
    Hooks {
        author_of: &nobody,
        stopped: &none,
        host_checks: false,
    }
}

/// `(id, name, group, added, updated)`.
type Fixture = (String, String, &'static str, u32, u32);

fn list(plugins: &[Fixture], sections: &str) -> String {
    let mut text = String::from(r#"{"format": 2, "plugins": ["#);
    for (n, (id, name, group, added, updated)) in plugins.iter().enumerate() {
        if n > 0 {
            text.push(',');
        }
        text.push_str(&format!(
            r#"{{"id": "{id}", "name": "{name}", "summary": "Does {id} well.", "group": "{group}",
 "kind": "data", "author": "A", "licence": "MIT", "added": "{}-{:02}-{:02}",
 "latest": {{"version": "1.0.0", "sdk": "0.4", "formats": 1, "size": 30,
 "date": "{}-{:02}-{:02}"}}}}"#,
            2020 + added / 10_000,
            (added / 100) % 100 + 1,
            added % 28 + 1,
            2020 + updated / 10_000,
            (updated / 100) % 100 + 1,
            updated % 28 + 1,
        ));
    }
    text.push(']');
    text.push_str(sections);
    text.push('}');
    text
}

fn fixture(n: usize) -> Fixture {
    let groups = ["reading", "reading", "look", "tools"];
    (
        format!("plugin-{n:04}"),
        format!("Name {:04}", (n * 7919) % 10_000),
        groups[(n / 3) % groups.len()],
        ((n * 37) % 600) as u32,
        ((n * 101) % 600) as u32,
    )
}

fn kept(storage: &MemoryStorage, plugins: &[Fixture], sections: &str) -> Index {
    storage
        .write("list.json", list(plugins, sections).as_bytes())
        .unwrap();
    let (index, _) = take(storage, "list.json", device(), hooks())
        .unwrap()
        .expect("a list");
    assert!(storage.size("list.json").is_err(), "the fetched list goes");
    index
}

#[test]
fn a_list_becomes_an_index_that_reads_back() {
    let storage = MemoryStorage::new();
    let plugins: Vec<Fixture> = (0..12).map(fixture).collect();
    let sections = r#", "sections": {"recommended": ["plugin-0003", "bad name", "plugin-0001"], "new": ["plugin-0002"]}"#;
    let index = kept(&storage, &plugins, sections);
    assert_eq!(index.len(), 12);
    assert!(index.fair());
    assert_eq!(index.section_len(0), 2);
    assert_eq!(index.section_len(1), 1);
    assert_eq!(
        index.section(&storage, 0).unwrap(),
        ["plugin-0003", "plugin-0001"]
    );
    assert_eq!(index.section(&storage, 1).unwrap(), ["plugin-0002"]);
    assert!(index.section(&storage, 9).unwrap().is_empty());
    for (n, (id, ..)) in plugins.iter().enumerate() {
        assert_eq!(&index.entry(&storage, n).unwrap().unwrap().id, id);
    }
    assert!(index.entry(&storage, 12).unwrap().is_none());
    // The runs are the plugins of one group in a row.
    assert_eq!(index.runs().iter().map(|r| r.1).sum::<usize>(), 12);
    // Kept: a fresh read of the card agrees.
    let again = Index::kept(&storage).unwrap().expect("kept");
    assert_eq!(again.runs(), index.runs());
    assert_eq!(
        again.orders(),
        &[Order::Fair, Order::Name, Order::Added, Order::Updated]
    );
}

#[test]
fn a_list_without_sections_is_by_name() {
    let storage = MemoryStorage::new();
    let plugins: Vec<Fixture> = (0..5).map(fixture).collect();
    let index = kept(&storage, &plugins, "");
    assert!(!index.fair());
    assert_eq!(index.orders(), &[Order::Name, Order::Added, Order::Updated]);
    assert_eq!(index.order("added"), Order::Added);
    assert_eq!(index.order("fair"), Order::Name);
}

#[test]
fn what_is_no_list_leaves_no_index() {
    let storage = MemoryStorage::new();
    for text in ["", "not json", r#"{"format": 3, "plugins": []}"#, "[]"] {
        storage.write("list.json", text.as_bytes()).unwrap();
        let names = ["i.part", "o.part", "h.part", "s.part"];
        let list = build(&storage, "list.json", names, device(), hooks()).unwrap();
        assert!(list.is_none(), "{text:?}");
        for name in names {
            assert!(storage.size(name).is_err(), "{name} left behind");
        }
    }
    // A missing list too.
    let names = ["i.part", "o.part", "h.part", "s.part"];
    assert!(
        build(&storage, "nothing.json", names, device(), hooks())
            .unwrap()
            .is_none()
    );
    // The index kept before stays where the new list is no list.
    let plugins: Vec<Fixture> = (0..3).map(fixture).collect();
    kept(&storage, &plugins, "");
    storage.write("list.json", b"junk").unwrap();
    assert!(
        take(&storage, "list.json", device(), hooks())
            .unwrap()
            .is_none()
    );
    assert_eq!(Index::kept(&storage).unwrap().unwrap().len(), 3);
}

#[test]
fn a_head_of_another_format_is_no_index() {
    let storage = MemoryStorage::new();
    storage
        .write(HEAD, b"store-index 4\t0:1\tfair\t0,0,0,0,0\n")
        .unwrap();
    assert!(Index::kept(&storage).unwrap().is_none());
    storage
        .write(HEAD, b"store-index 5\t0:2,99:3,x,1:y,2:1\tname\t1,2\n")
        .unwrap();
    let index = Index::kept(&storage).unwrap().unwrap();
    // A group past the known ones and a run that is none are left out.
    assert_eq!(index.runs(), &[(0, 2), (2, 1)]);
    assert_eq!(index.section_len(1), 2);
    assert!(Index::kept(&MemoryStorage::new()).unwrap().is_none());
}

#[test]
fn a_search_finds_names_and_summaries_whatever_the_case() {
    let storage = MemoryStorage::new();
    let plugins: Vec<Fixture> = (0..30).map(fixture).collect();
    let index = kept(&storage, &plugins, "");
    let found = index.search(&storage, "  NAME 0000 ").unwrap();
    let expected = plugins.iter().filter(|p| p.1 == "Name 0000").count();
    assert_eq!(found, expected);
    // The summary says "Does plugin-0007 well."
    assert_eq!(index.search(&storage, "DOES PLUGIN-0007").unwrap(), 1);
    let at = hit(&storage, 0).unwrap();
    assert_eq!(
        index.entry(&storage, at).unwrap().unwrap().id,
        "plugin-0007"
    );
    assert_eq!(hit(&storage, 1), None);
    assert_eq!(index.search(&storage, "no such plugin").unwrap(), 0);
}

/// Plugins `most` at most in `order`, as ids.
fn picked(index: &Index, storage: &MemoryStorage, order: Order, most: usize) -> Vec<String> {
    let count = index
        .pick(storage, |_, _| Ok(Some(0)), order, most)
        .unwrap();
    (0..count)
        .map(|n| {
            let at = hit(storage, n).unwrap();
            index.entry(storage, at).unwrap().unwrap().id
        })
        .collect()
}

fn expected(plugins: &[Fixture], order: Order, most: usize) -> Vec<String> {
    let mut all: Vec<(u64, usize, &str)> = plugins
        .iter()
        .enumerate()
        .map(|(n, p)| {
            let key = match order {
                Order::Added => u64::from(p.3),
                Order::Updated => u64::from(p.4),
                _ => u64::MAX - name_key(&p.1),
            };
            (key, n, p.0.as_str())
        })
        .collect();
    all.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    all.into_iter()
        .take(most)
        .map(|p| p.2.to_string())
        .collect()
}

#[test]
fn the_orders_agree_for_few_plugins_and_for_more_than_a_run() {
    let storage = MemoryStorage::new();
    // 1,300 plugins: more than two runs of 512 for the merge.
    let plugins: Vec<Fixture> = (0..1300).map(fixture).collect();
    // The dates of the fixture are by month: the index keeps `yyyymmdd`.
    let index = kept(&storage, &plugins, r#", "sections": {"new": []}"#);
    assert_eq!(index.len(), 1300);
    for order in [Order::Added, Order::Updated] {
        let all = picked(&index, &storage, order, usize::MAX);
        assert_eq!(all.len(), 1300);
        // Keys equal where the dates agree; ids of equal keys keep list order.
        let mut sorted = all.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), 1300, "no plugin lost or repeated");
        let few = picked(&index, &storage, order, 20);
        assert_eq!(few, all[..20], "the best few are the first of all");
        assert!(!storage.exists("sort.bin"), "the runs file goes");
    }
    // By name, the list being fair: sorted by name keys.
    let names = picked(&index, &storage, Order::Name, 10);
    assert_eq!(names, expected(&plugins, Order::Name, 10));
    // In the list's order.
    let fair = picked(&index, &storage, Order::Fair, 5);
    assert_eq!(
        fair,
        [
            "plugin-0000",
            "plugin-0001",
            "plugin-0002",
            "plugin-0003",
            "plugin-0004"
        ]
    );
}

#[test]
fn a_rank_orders_the_hits_and_a_refusing_key_keeps_none() {
    let storage = MemoryStorage::new();
    let plugins: Vec<Fixture> = (0..10).map(fixture).collect();
    let index = kept(&storage, &plugins, "");
    let count = index
        .pick(
            &storage,
            |n, _| Ok((n % 2 == 0).then_some(u32::try_from(n).unwrap())),
            Order::Rank,
            3,
        )
        .unwrap();
    assert_eq!(count, 3);
    assert_eq!(
        (0..3)
            .map(|n| hit(&storage, n).unwrap())
            .collect::<Vec<_>>(),
        [8, 6, 4]
    );
    let none = index
        .pick(&storage, |_, _| Ok(None), Order::Added, 3)
        .unwrap();
    assert_eq!(none, 0);
    let _ = storage.remove(HITS);
    assert_eq!(hit(&storage, 0), None);
}

#[test]
fn names_sort_by_letters_and_digits_with_accents_and_case_aside() {
    assert!(name_key("Äpfel") < name_key("Birne"));
    assert!(name_key("zebra") < name_key("ёж"));
    assert!(name_key("7 days") < name_key("Alpha"));
    assert_eq!(name_key("a-b c"), name_key("ABC"));
    assert_eq!(name_key("---"), 0);
    // Only the first twelve count.
    assert_eq!(name_key("abcdefghijklmX"), name_key("abcdefghijklmY"));
}

#[test]
fn a_card_that_refuses_writes_leaves_no_half_index() {
    use paperpaw_sdk::{DirEntry, StorageError};
    struct Refusing<'a>(&'a MemoryStorage, std::cell::Cell<usize>);
    impl Storage for Refusing<'_> {
        fn read_at(&self, p: &str, o: u64, b: &mut [u8]) -> Result<usize, StorageError> {
            self.0.read_at(p, o, b)
        }
        fn size(&self, p: &str) -> Result<u64, StorageError> {
            self.0.size(p)
        }
        fn write(&self, p: &str, d: &[u8]) -> Result<(), StorageError> {
            if self.1.get() == 0 {
                return Err(StorageError::Full);
            }
            self.1.set(self.1.get() - 1);
            self.0.write(p, d)
        }
        fn append(&self, p: &str, d: &[u8]) -> Result<(), StorageError> {
            self.0.append(p, d)
        }
        fn list(&self, d: &str, v: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
            self.0.list(d, v)
        }
        fn remove(&self, p: &str) -> Result<(), StorageError> {
            self.0.remove(p)
        }
    }
    let plugins: Vec<Fixture> = (0..4).map(fixture).collect();
    // Each write in turn refused (the two empty starts, the sections, the head: the first three of the four).
    for allowed in 0..3 {
        let storage = MemoryStorage::new();
        storage
            .write("list.json", list(&plugins, "").as_bytes())
            .unwrap();
        let card = Refusing(&storage, std::cell::Cell::new(allowed + 1));
        let names = ["i", "o", "h", "s"];
        let built = build(&card, "list.json", names, device(), hooks());
        assert!(
            built.is_err() || built.unwrap().is_none(),
            "write {allowed}"
        );
        for name in names {
            assert!(
                storage.size(name).is_err(),
                "{name} left after write {allowed}"
            );
        }
    }
}
