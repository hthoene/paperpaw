//! An owner whose batches are all read from a read-only image keeps no index
//! and answers as an indexed one: the same keys in the same order, the same
//! lookups; a rewritten or broken image neither panics nor reaches another
//! plugin's names.
#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]
extern crate std;
use super::super::super::raw_declarations::varint;
use super::super::super::registrations::*;
use super::super::*;
use alloc::{rc::Rc, vec, vec::Vec};
use core::cell::{Cell, RefCell};

struct Image {
    bytes: RefCell<Vec<u8>>,
    read_only: bool,
    errors: Cell<usize>,
    generation: Cell<u32>,
    /// The module is being unloaded: its image cannot be read.
    gone: Cell<bool>,
}
impl ImageOwner for Image {
    fn plugin_id(&self) -> &str {
        "fixture"
    }
    fn generation(&self) -> u32 {
        self.generation.get()
    }
    fn with_image(&self, visit: &mut dyn FnMut(Option<&[u8]>)) {
        if self.gone.get() {
            visit(None);
            return;
        }
        let bytes = self.bytes.borrow();
        visit(Some(bytes.as_slice()));
    }
    fn invalid_declaration(&self, _: i32) {
        self.errors.set(self.errors.get() + 1);
    }
    fn read_only(&self) -> bool {
        self.read_only
    }
}

fn group(name: &str, order: i32) -> Record<'_> {
    Record::SettingGroup(SettingGroup {
        name,
        title: "Group",
        order,
        page: "",
        parent: "",
    })
}

fn action(name: &str, token: u32) -> Record<'_> {
    Record::ActionSpec(ActionSpec {
        name,
        title: "Action",
        token,
        flags: 0,
    })
}

fn encode(records: &[Record<'_>]) -> Vec<u8> {
    let mut bytes = vec![0; REGISTRATION_MAX_BYTES];
    let len = records_encode(records, &mut bytes).unwrap();
    bytes.truncate(len);
    bytes
}

/// Two batches one after the other in one image, and where each is.
fn image(read_only: bool) -> (Rc<Image>, [core::ops::Range<usize>; 2]) {
    let first = encode(&[
        action("zed", 1),
        group("beta", 1),
        action("alpha", 2),
        group("alpha", 2),
    ]);
    let second = encode(&[group("gamma", 3), action("mid", 3)]);
    let ranges = [0..first.len(), first.len()..first.len() + second.len()];
    let mut bytes = first;
    bytes.extend_from_slice(&second);
    let image = Rc::new(Image {
        bytes: RefCell::new(bytes),
        read_only,
        errors: Cell::new(0),
        generation: Cell::new(1),
        gone: Cell::new(false),
    });
    (image, ranges)
}

/// A store with the fixture owner, its batches staged from the image (or
/// copied).
fn store(read_only: bool, copied: bool) -> (DeclarationStore, Rc<Image>) {
    store_with(read_only, [copied; 2])
}

/// [`store`], each batch copied or not on its own (a mixed owner).
fn store_with(read_only: bool, copied: [bool; 2]) -> (DeclarationStore, Rc<Image>) {
    let (source, ranges) = image(read_only);
    let mut allocations = Allocations::default();
    let mut pending = PendingDeclarations::new(Id::new_static("fixture"), source.clone());
    for (range, copied) in ranges.into_iter().zip(copied) {
        let bytes = source.bytes.borrow();
        if copied {
            // A copy outside the image: staged as a runtime batch, not as
            // the image's own bytes.
            let copy = bytes[range].to_vec();
            pending.stage_batch(&copy, &mut allocations).unwrap();
        } else {
            pending
                .stage_batch(&bytes[range], &mut allocations)
                .unwrap();
        }
    }
    let mut store = DeclarationStore::default();
    store.prepare(pending, &mut allocations).unwrap().commit();
    (store, source)
}

fn places(store: &DeclarationStore, kind: Kind) -> Vec<(u16, u16)> {
    let mut places = Vec::new();
    store.keys(kind, &mut |key| places.push((key.batch, key.record)));
    places
}

fn order_of(store: &DeclarationStore, kind: Kind, name: &str) -> Option<i64> {
    let mut found = None;
    store.find("fixture", kind, name, &mut |record| {
        found = match record {
            Record::SettingGroup(group) => Some(i64::from(group.order)),
            Record::ActionSpec(action) => Some(i64::from(action.token)),
            _ => None,
        };
    });
    found
}

#[test]
fn the_kind_of_a_frame_tag_is_the_kind_of_its_decoded_record() {
    let fixtures = [
        group("group", 0),
        action("action", 1),
        Record::SettingExtra(SettingExtra {
            name: "flag",
            unit: "",
            section: "",
            follows: "",
            shown_id: "",
            shown_value: "",
            editor_kind: 0,
            editor: "",
            step: 0,
            flags: 0,
            check: 0,
        }),
        Record::ResourceHyphenation(ResourceHyphenation {
            language: "en",
            path: "hyph/en.pphp",
        }),
        Record::ResourceFontFace(ResourceFontFace {
            name: "face",
            path: "fonts/a.ttf",
            family: "Fixture",
            style: 0,
            weight: 400,
            role: "",
            priority: 0,
        }),
        Record::ResourceFontExtension(ResourceFontExtension {
            name: "extra",
            path: "fonts/a.ppfp",
            extends: "fixture.face",
        }),
    ];
    for (at, record) in fixtures.iter().enumerate() {
        let wire = encode(core::slice::from_ref(record));
        let mut cursor = 0;
        assert_eq!(varint(&wire, &mut cursor).unwrap(), 1);
        let tag = varint(&wire, &mut cursor).unwrap();
        assert_eq!(Kind::of_tag(tag), Kind::of(record), "record {at}");
    }
    // Every kind has exactly one tag.
    for kind in Kind::ALL {
        let tags = (0..4096)
            .filter(|&tag| Kind::of_tag(tag) == Some(kind))
            .count();
        assert_eq!(tags, 1, "{kind:?}");
    }
}

#[test]
fn an_image_only_owner_keeps_no_slots_and_answers_as_an_indexed_one() {
    let (scanned, _) = store(true, false);
    let (indexed, _) = store(false, false);
    let owner = scanned.owners[0].as_ref().unwrap();
    assert!(owner.image_only());
    assert_eq!(
        owner.slots.capacity(),
        0,
        "the index is given back at commit"
    );
    // And the batch references: only where each batch starts is kept.
    assert!(matches!(&owner.batches, Batches::Starts(starts, _) if starts.len() == 2));
    let other = indexed.owners[0].as_ref().unwrap();
    assert!(!other.image_only());
    assert_eq!(other.slots.len(), 6);
    for kind in Kind::ALL {
        assert_eq!(places(&scanned, kind), places(&indexed, kind), "{kind:?}");
    }
    // Declaration order, batch then record.
    assert_eq!(places(&scanned, Kind::Group), [(0, 1), (0, 3), (1, 0)]);
    assert_eq!(places(&scanned, Kind::Action), [(0, 0), (0, 2), (1, 1)]);
    for (kind, name) in [
        (Kind::Group, "alpha"),
        (Kind::Group, "beta"),
        (Kind::Group, "gamma"),
        (Kind::Action, "alpha"),
        (Kind::Action, "zed"),
        (Kind::Action, "mid"),
        (Kind::Group, "zed"),
        (Kind::Action, "beta"),
        (Kind::Screen, "alpha"),
        (Kind::Group, "missing"),
    ] {
        assert_eq!(
            order_of(&scanned, kind, name),
            order_of(&indexed, kind, name),
            "{kind:?} {name}"
        );
    }
    assert_eq!(order_of(&scanned, Kind::Group, "gamma"), Some(3));
    let mut allocations = Allocations::default();
    assert_eq!(scanned.all_keys(&mut allocations).unwrap().len(), 6);
}

#[test]
fn copied_batches_and_a_changing_image_keep_their_index() {
    let (copied, _) = store(true, true);
    assert!(!copied.owners[0].as_ref().unwrap().image_only());
    let (changing, _) = store(false, false);
    assert!(!changing.owners[0].as_ref().unwrap().image_only());
}

#[test]
fn a_rewritten_image_with_two_records_of_one_name_finds_the_first_and_visits_both() {
    let (store, source) = store(true, false);
    // "gamma" becomes a second "alpha" group in the second batch: same length,
    // so the frames still walk.
    {
        let mut bytes = source.bytes.borrow_mut();
        let at = bytes
            .windows(5)
            .position(|window| window == b"gamma")
            .unwrap();
        bytes[at..at + 5].copy_from_slice(b"alpha");
    }
    assert_eq!(order_of(&store, Kind::Group, "alpha"), Some(2));
    assert_eq!(order_of(&store, Kind::Group, "gamma"), None);
    let mut orders = Vec::new();
    store.keys(Kind::Group, &mut |key| {
        store.with_record(key, &mut |record| {
            if let Record::SettingGroup(group) = record {
                orders.push((group.name == "alpha", group.order));
            }
        });
    });
    assert_eq!(orders, [(false, 1), (true, 2), (true, 3)]);
    assert_eq!(source.errors.get(), 0);
}

#[test]
fn a_broken_frame_skips_its_batch_and_is_said_without_a_panic() {
    let (store, source) = store(true, false);
    {
        let mut bytes = source.bytes.borrow_mut();
        // The first batch's count: more records than a batch may hold.
        bytes[0] = 9;
    }
    // The first batch is skipped; the second still answers.
    assert_eq!(places(&store, Kind::Group), [(1, 0)]);
    assert!(source.errors.get() > 0);
    assert_eq!(order_of(&store, Kind::Group, "gamma"), Some(3));
    assert_eq!(order_of(&store, Kind::Group, "alpha"), None);
}

/// The names the fixture declares, of both kinds, and some it does not.
const NAMES: [(Kind, &str); 8] = [
    (Kind::Group, "alpha"),
    (Kind::Group, "beta"),
    (Kind::Group, "gamma"),
    (Kind::Action, "alpha"),
    (Kind::Action, "zed"),
    (Kind::Action, "mid"),
    (Kind::Group, "zed"),
    (Kind::Screen, "alpha"),
];

#[test]
fn a_mixed_owner_keeps_its_index_and_answers_as_an_indexed_one() {
    // The first batch read from the image, the second a copy of its own.
    let (mixed, _) = store_with(true, [false, true]);
    let (indexed, _) = store(false, false);
    let owner = mixed.owners[0].as_ref().unwrap();
    assert!(!owner.image_only(), "one batch of its own keeps the index");
    assert_eq!(owner.slots.len(), 6);
    assert!(matches!(&owner.batches, Batches::Refs(refs) if refs.len() == 2));
    for kind in Kind::ALL {
        assert_eq!(places(&mixed, kind), places(&indexed, kind), "{kind:?}");
    }
    for (kind, name) in NAMES {
        assert_eq!(
            order_of(&mixed, kind, name),
            order_of(&indexed, kind, name),
            "{kind:?} {name}"
        );
    }
}

#[test]
fn an_image_only_owner_answers_nothing_once_stale_unloading_or_removed() {
    let (mut store, source) = store(true, false);
    assert!(store.owners[0].as_ref().unwrap().image_only());
    let mut first = None;
    store.keys(Kind::Group, &mut |key| {
        first.get_or_insert(key);
    });
    let key = first.unwrap();
    let silent = |store: &DeclarationStore| {
        for kind in Kind::ALL {
            assert!(places(store, kind).is_empty(), "{kind:?}");
        }
        for (kind, name) in NAMES {
            assert_eq!(order_of(store, kind, name), None, "{kind:?} {name}");
        }
        assert!(!store.with_record(key, &mut |_| panic!("an absent owner")));
    };
    // A later generation of the plugin: the owner is stale.
    source.generation.set(2);
    silent(&store);
    source.generation.set(1);
    // Unloading: the module's image cannot be read any more.
    source.gone.set(true);
    silent(&store);
    source.gone.set(false);
    assert_eq!(order_of(&store, Kind::Group, "gamma"), Some(3));
    // Removed: nothing is left of it.
    store.remove_owner(&Id::new_static("fixture"), 1);
    silent(&store);
    assert_eq!(
        source.errors.get(),
        0,
        "nothing of this is the plugin's fault"
    );
}
