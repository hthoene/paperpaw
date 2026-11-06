//! Fixture images exercise the same guarded path as the later shell bridge.
#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]
extern crate std;
use super::super::registrations::*;
use super::*;
use alloc::{vec, vec::Vec};
use core::cell::{Cell, RefCell};

const KINDS: [Kind; 6] = [
    Kind::Group,
    Kind::Spec,
    Kind::Extra,
    Kind::Hyphenation,
    Kind::Face,
    Kind::Extension,
];
struct Image {
    bytes: RefCell<Vec<u8>>,
    generation: Cell<u32>,
    live: Cell<bool>,
    errors: Cell<usize>,
    active: Cell<bool>,
    denied: Cell<usize>,
}
impl ImageOwner for Image {
    fn generation(&self) -> u32 {
        self.generation.get()
    }
    fn with_image(&self, visit: &mut dyn FnMut(Option<&[u8]>)) {
        if self.active.get() {
            self.denied.set(self.denied.get() + 1);
            visit(None);
            return;
        }
        self.active.set(true);
        struct Reset<'a>(&'a Cell<bool>);
        impl Drop for Reset<'_> {
            fn drop(&mut self) {
                self.0.set(false);
            }
        }
        let _reset = Reset(&self.active);
        let bytes = self.bytes.borrow();
        visit(self.live.get().then_some(bytes.as_slice()));
    }
    fn invalid_declaration(&self, _: i32) {
        self.errors.set(self.errors.get() + 1);
    }
}
fn image() -> Rc<Image> {
    let records = [
        Record::SettingGroup(SettingGroup {
            name: "group",
            title: "Group",
            order: 0,
            page: "",
            parent: "",
        }),
        Record::SettingSpec(SettingSpec {
            name: "flag",
            kind: 0,
            flags: 0,
            group: "fixture.group",
            title: "Flag",
            help: "",
            default: &[1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1],
            min: 0,
            max: 0,
            choices: "",
        }),
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
    let mut bytes = vec![0; REGISTRATION_MAX_BYTES];
    let len = records_encode(&records, &mut bytes).unwrap();
    bytes.truncate(len);
    Rc::new(Image {
        bytes: RefCell::new(bytes),
        generation: Cell::new(1),
        live: Cell::new(true),
        errors: Cell::new(0),
        active: Cell::new(false),
        denied: Cell::new(0),
    })
}
fn pending(
    source: &Rc<Image>,
    owned: bool,
    allocations: &mut Allocations,
) -> Result<PendingDeclarations, i32> {
    let mut pending = PendingDeclarations::new(Id::new_static("fixture"), source.clone());
    if owned {
        pending.stage_batch(&source.bytes.borrow().clone(), allocations)?;
    } else {
        pending.stage_batch(&source.bytes.borrow(), allocations)?;
    }
    Ok(pending)
}
fn keys(store: &DeclarationStore) -> Vec<DeclKey> {
    let mut keys = Vec::new();
    for kind in KINDS {
        store.keys(kind, &mut |key| keys.push(key));
    }
    keys
}

#[test]
fn both_entry_kinds_decode_all_six_records_and_unload_removes_them() {
    for owned in [false, true] {
        let source = image();
        let mut store = DeclarationStore::default();
        let mut allocations = Allocations::default();
        let pending = pending(&source, owned, &mut allocations).unwrap();
        assert_eq!(matches!(pending.batches[0], DeclRef::Owned(_)), owned);
        store.prepare(pending, &mut allocations).unwrap().commit();
        let before = keys(&store);
        assert_eq!(before.len(), 6);
        for (key, kind) in before.iter().zip(KINDS) {
            assert!(store.with_record(*key, &mut |record| assert_eq!(
                Kind::of(&record),
                Some(kind)
            )));
        }
        store.remove_owner(&Id::new_static("fixture"), 1);
        source.live.set(false);
        assert!(keys(&store).is_empty());
        for key in before {
            assert!(!store.with_record(key, &mut |_| panic!("removed entry")));
        }
    }
}

#[test]
fn every_reserve_failure_leaves_the_published_store_unchanged() {
    for owned in [false, true] {
        let mut success = false;
        for fail_after in 0..12 {
            let source = image();
            let mut store = DeclarationStore::default();
            let mut allocations = Allocations {
                fail_after: Some(fail_after),
            };
            let result = pending(&source, owned, &mut allocations).and_then(|pending| {
                store.prepare(pending, &mut allocations)?.commit();
                Ok(())
            });
            match result {
                Err(error) => {
                    assert_eq!(error, ERR_NO_MEMORY);
                    assert!(keys(&store).is_empty());
                }
                Ok(()) => {
                    assert_eq!(keys(&store).len(), 6);
                    success = true;
                    break;
                }
            }
        }
        assert!(success);
    }
}

#[test]
fn failed_update_retains_old_entries_and_success_invalidates_old_keys() {
    let mut store = DeclarationStore::default();
    let mut allocations = Allocations::default();
    let old = image();
    store
        .prepare(
            pending(&old, false, &mut allocations).unwrap(),
            &mut allocations,
        )
        .unwrap()
        .commit();
    let old_keys = keys(&store);
    let new = image();
    new.generation.set(2);
    let mut failing = Allocations {
        fail_after: Some(0),
    };
    assert!(pending(&new, true, &mut failing).is_err());
    assert_eq!(keys(&store), old_keys);
    // Dropping a prepared transaction must also leave the old owner in place.
    drop(
        store
            .prepare(
                pending(&new, false, &mut allocations).unwrap(),
                &mut allocations,
            )
            .unwrap(),
    );
    assert_eq!(keys(&store), old_keys);
    store
        .prepare(
            pending(&new, false, &mut allocations).unwrap(),
            &mut allocations,
        )
        .unwrap()
        .commit();
    for key in old_keys {
        assert!(!store.with_record(key, &mut |_| panic!("old generation")));
    }
    old.live.set(false);
    for key in keys(&store) {
        assert!(store.with_record(key, &mut |_| {}));
    }
}

#[test]
fn stale_owner_and_unloading_owner_are_absent_for_both_kinds() {
    for owned in [false, true] {
        let mut store = DeclarationStore::default();
        let mut allocations = Allocations::default();
        let source = image();
        store
            .prepare(
                pending(&source, owned, &mut allocations).unwrap(),
                &mut allocations,
            )
            .unwrap()
            .commit();
        let key = keys(&store)[0];
        source.generation.set(2);
        assert!(!store.with_record(key, &mut |_| panic!("stale owner")));
        assert!(keys(&store).is_empty());
        source.generation.set(1);
        source.live.set(false);
        assert!(!store.with_record(key, &mut |_| panic!("unloading owner")));
    }
}

#[test]
fn corrupted_image_is_revalidated_each_time_and_never_cached() {
    let mut store = DeclarationStore::default();
    let mut allocations = Allocations::default();
    let source = image();
    store
        .prepare(
            pending(&source, false, &mut allocations).unwrap(),
            &mut allocations,
        )
        .unwrap()
        .commit();
    let key = keys(&store)[0];
    assert!(store.with_record(key, &mut |_| {}));
    let first = source.bytes.borrow()[0];
    source.bytes.borrow_mut()[0] = 0xff;
    assert!(!store.with_record(key, &mut |_| panic!("corrupt image")));
    assert_eq!(source.errors.get(), 1);
    source.bytes.borrow_mut()[0] = first;
    assert!(store.with_record(key, &mut |_| {}));
}

#[test]
fn reused_owner_slot_cannot_resurrect_old_keys_with_the_same_generation() {
    let mut store = DeclarationStore::default();
    let mut allocations = Allocations::default();
    let old = image();
    store
        .prepare(
            pending(&old, false, &mut allocations).unwrap(),
            &mut allocations,
        )
        .unwrap()
        .commit();
    let key = keys(&store)[0];
    store.remove_owner(&Id::new_static("fixture"), 1);
    let new = image();
    store
        .prepare(
            pending(&new, false, &mut allocations).unwrap(),
            &mut allocations,
        )
        .unwrap()
        .commit();
    assert_eq!(store.owners.len(), 1);
    assert!(!store.with_record(key, &mut |_| panic!("reused owner slot")));
}

#[test]
fn visitors_hold_the_image_guard_and_ranges_require_the_same_allocation() {
    let source = image();
    let bytes = source.bytes.borrow();
    assert!(image_ref(&bytes, &bytes).is_some());
    assert!(image_ref(&bytes, &bytes[1..]).is_some());
    assert!(image_ref(&bytes, &bytes.clone()).is_none());
    assert!(image_ref(&bytes[1..], &bytes).is_none());
    drop(bytes);
    let mut store = DeclarationStore::default();
    let mut allocations = Allocations::default();
    store
        .prepare(
            pending(&source, false, &mut allocations).unwrap(),
            &mut allocations,
        )
        .unwrap()
        .commit();
    assert!(store.with_record(keys(&store)[0], &mut |_| {
        assert!(source.bytes.try_borrow_mut().is_err());
    }));
}

#[test]
fn layout_is_measured_instead_of_assuming_a_twelve_byte_entry() {
    std::println!(
        "DeclRef={} DeclKey={} Slot={} Owner={}",
        size_of::<DeclRef>(),
        size_of::<DeclKey>(),
        size_of::<Slot>(),
        size_of::<PendingDeclarations>()
    );
    assert_eq!(size_of::<Slot>(), 8);
}

#[test]
fn construction_retains_a_snapshot_and_record_offsets_after_input_is_gone() {
    use super::super::{HostOwner, registration_scope::RegistrationScope};
    let owner = core::pin::pin!(HostOwner::new((), ()));
    let scope = core::pin::pin!(RegistrationScope::<Registration>::new());
    let mut construction = owner.as_ref().begin::<Registration>("fixture").unwrap();
    let source = image();
    let mut input = source.bytes.borrow().clone();
    let expected = input.clone();
    scope
        .as_ref()
        .scope(&mut construction, || {
            // SAFETY: initialized input stays stable for this synchronous submission.
            assert_eq!(
                unsafe { submit(&scope, input.as_ptr(), input.len()) },
                paperpaw_native::host_glue::OK
            );
        })
        .unwrap();
    input.fill(0);
    drop(input);
    let mut committed = construction
        .commit(crate::native_host::Teardown {
            cancel: |_| Ok(()),
            release: |_| Ok(()),
            destroy: |_| {},
        })
        .unwrap();
    let raw = committed.take_raw_declarations();
    assert_eq!(raw.batches.len(), 1);
    let batch = &raw.batches[0];
    assert_eq!(&*batch.bytes, expected);
    assert_eq!(batch.entries, 6);
    assert!(batch.offsets[..6].windows(2).all(|pair| pair[0] < pair[1]));
}

#[test]
fn known_image_batches_have_no_snapshot_and_owned_snapshots_never_promote() {
    use super::super::raw_declarations::RawDeclarations;
    for known_image in [false, true] {
        let source = image();
        let mut raw = RawDeclarations::default();
        let batch = {
            let bytes = source.bytes.borrow();
            raw.prepare(
                known_image.then_some(bytes.as_slice()),
                Some(&bytes),
                &records_decode(&bytes).unwrap(),
            )
            .unwrap()
            .unwrap()
            .batch
        };
        assert_eq!(batch.bytes.is_empty(), known_image);
        let mut pending = PendingDeclarations::new(Id::new_static("fixture"), source.clone());
        pending
            .stage_committed(batch, &mut Allocations::default())
            .unwrap();
        assert_eq!(
            matches!(pending.batches[0], DeclRef::Image { .. }),
            known_image
        );
    }
}

#[test]
fn committed_runtime_copy_moves_into_owned_storage_without_copying_again() {
    use super::super::raw_declarations::RawDeclarations;
    let source = image();
    let mut raw = RawDeclarations::default();
    let batch = {
        let temporary = source.bytes.borrow().clone();
        raw.prepare(None, Some(&temporary), &records_decode(&temporary).unwrap())
            .unwrap()
            .unwrap()
            .batch
    };
    let address = batch.bytes.as_ptr();
    let mut pending = PendingDeclarations::new(Id::new_static("fixture"), source.clone());
    pending
        .stage_committed(batch, &mut Allocations::default())
        .unwrap();
    let DeclRef::Owned(bytes) = &pending.batches[0] else {
        panic!("runtime bytes must be owned");
    };
    assert_eq!(bytes.as_ptr(), address);
}

#[test]
fn ignored_failure_in_a_later_batch_cannot_publish_a_partial_load() {
    let source = image();
    let mut allocations = Allocations::default();
    let mut pending = pending(&source, false, &mut allocations).unwrap();
    let mut failing = Allocations {
        fail_after: Some(0),
    };
    assert_eq!(
        pending.stage_batch(&source.bytes.borrow().clone(), &mut failing),
        Err(ERR_NO_MEMORY)
    );
    assert_eq!(
        pending.stage_batch(&source.bytes.borrow(), &mut allocations),
        Err(ERR_NO_MEMORY)
    );
    let mut store = DeclarationStore::default();
    assert!(matches!(
        store.prepare(pending, &mut allocations),
        Err(ERR_NO_MEMORY)
    ));
    assert!(keys(&store).is_empty());
}

#[test]
fn full_widgets_image_moves_into_the_guarded_store_without_owned_bytes() {
    use super::super::{HostOwner, registration_scope::RegistrationScope};
    let source = Rc::new(Image {
        bytes: RefCell::new(
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../plugins/widgets/testdata/declarations.bin.v1"
            ))
            .to_vec(),
        ),
        generation: Cell::new(1),
        live: Cell::new(true),
        errors: Cell::new(0),
        active: Cell::new(false),
        denied: Cell::new(0),
    });
    let owner = core::pin::pin!(HostOwner::new((), ()));
    let scope = core::pin::pin!(RegistrationScope::<Registration>::new());
    let image = source.bytes.borrow();
    let mut construction = owner.as_ref().begin::<Registration>("widgets").unwrap();
    construction.set_image(&image).unwrap();
    let mut cursor = 0;
    while cursor < image.len() {
        let len = u32::from_le_bytes(image[cursor..cursor + 4].try_into().unwrap()) as usize;
        cursor += 4;
        let bytes = &image[cursor..cursor + len];
        scope
            .as_ref()
            .scope(&mut construction, || {
                // SAFETY: source image remains borrowed and immutable during make.
                assert_eq!(
                    unsafe { submit(&scope, bytes.as_ptr(), bytes.len()) },
                    paperpaw_native::host_glue::OK
                );
            })
            .unwrap();
        cursor += len;
    }
    construction.finish_declarations().unwrap();
    assert_eq!(construction.raw.image_entries, 400);
    assert!(construction.declarations().is_empty());
    let raw = core::mem::take(&mut construction.raw);
    let construction_index_budget = raw.index_bytes;
    construction.abort();
    drop(image);
    let mut allocations = Allocations::default();
    let mut pending = PendingDeclarations::new(Id::new_static("widgets"), source.clone());
    for batch in raw.batches {
        pending.stage_committed(batch, &mut allocations).unwrap();
    }
    assert_eq!((pending.image_entries, pending.resident), (400, 0));
    assert!(
        pending
            .batches
            .iter()
            .all(|batch| matches!(batch, DeclRef::Image { .. }))
    );
    std::println!(
        "widgets full image: construction index charged={construction_index_budget}; committed descriptor/index bytes={}; owner header={}",
        pending.index_bytes,
        size_of::<PendingDeclarations>()
    );
    let mut store = DeclarationStore::default();
    store.prepare(pending, &mut allocations).unwrap().commit();
    let entries = keys(&store);
    assert_eq!(entries.len(), 400);
    for key in entries {
        assert!(store.with_record(key, &mut |_| {}));
    }
}

#[test]
fn sorted_name_lookup_finds_only_the_requested_live_record() {
    let source = image();
    let mut allocations = Allocations::default();
    let mut store = DeclarationStore::default();
    store
        .prepare(
            pending(&source, false, &mut allocations).unwrap(),
            &mut allocations,
        )
        .unwrap()
        .commit();
    assert!(
        store.find("fixture", Kind::Spec, "flag", &mut |record| assert!(
            matches!(record, Record::SettingSpec(spec) if spec.name == "flag")
        ))
    );
    assert!(!store.find("fixture", Kind::Spec, "missing", &mut |_| panic!("missing")));
    assert!(
        store.find("fixture", Kind::Extension, "extra", &mut |record| assert!(
            matches!(record, Record::ResourceFontExtension(_))
        ))
    );
    source.live.set(false);
    assert!(!store.find("fixture", Kind::Spec, "flag", &mut |_| panic!("stopped")));
}

#[test]
fn image_staging_and_lookup_acquire_a_non_reentrant_guard_only_once() {
    use super::super::raw_declarations::RawDeclarations;
    let source = image();
    let mut raw = RawDeclarations::default();
    let batch = {
        let bytes = source.bytes.borrow();
        raw.prepare(Some(&bytes), Some(&bytes), &records_decode(&bytes).unwrap())
            .unwrap()
            .unwrap()
            .batch
    };
    let mut allocations = Allocations::default();
    let mut pending = PendingDeclarations::new(Id::new_static("fixture"), source.clone());
    pending.stage_committed(batch, &mut allocations).unwrap();
    assert!(matches!(pending.batches[0], DeclRef::Image { .. }));
    assert_eq!(source.denied.get(), 0);
    let mut store = DeclarationStore::default();
    store.prepare(pending, &mut allocations).unwrap().commit();
    assert!(store.find("fixture", Kind::Spec, "flag", &mut |_| {}));
    assert_eq!(source.denied.get(), 0);
    source.active.set(true);
    assert!(!store.find("fixture", Kind::Spec, "flag", &mut |_| panic!("busy guard")));
    assert_eq!(source.denied.get(), 1);
    source.active.set(false);
}
