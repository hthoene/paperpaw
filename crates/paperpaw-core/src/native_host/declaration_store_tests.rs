//! Fixture images exercise the same guarded path as the later shell bridge.
#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]
extern crate std;
use super::super::registrations::*;
use super::*;
use alloc::{vec, vec::Vec};
use core::cell::{Cell, RefCell};

const KINDS: [Kind; KIND_COUNT] = [
    Kind::Group,
    Kind::Spec,
    Kind::Extra,
    Kind::Hyphenation,
    Kind::Face,
    Kind::Extension,
    Kind::Screen,
    Kind::Action,
    Kind::Widget,
    Kind::WidgetPlace,
    Kind::StatusItem,
    Kind::StatusLine,
    Kind::StatusPlace,
    Kind::MenuEntry,
    Kind::Icon,
    Kind::MenuNeed,
    Kind::MenuSection,
    Kind::MenuHide,
    Kind::MenuDescribe,
    Kind::SlotSpec,
    Kind::SlotOffer,
    Kind::KeyBind,
    Kind::SettingDefault,
    Kind::SettingChoice,
    Kind::PresetSpec,
    Kind::PresetValue,
    Kind::Stage,
    Kind::Subscription,
    Kind::SetupStep,
    Kind::InputSpec,
    Kind::HelpHint,
    Kind::Format,
    Kind::Reader,
    Kind::Lookup,
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
    fn plugin_id(&self) -> &str {
        "fixture"
    }
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
                ..Default::default()
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
        ..Default::default()
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
    let offset = usize::from(key.record);
    let first = source.bytes.borrow()[offset];
    source.bytes.borrow_mut()[offset] = 0xff;
    assert!(!store.with_record(key, &mut |_| panic!("corrupt image")));
    assert_eq!(source.errors.get(), 1);
    source.bytes.borrow_mut()[offset] = first;
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
    construction.finish_declarations().unwrap();
    let raw = &construction.raw;
    assert_eq!(raw.batches.len(), 1);
    let batch = &raw.batches[0];
    assert_eq!(&*batch.bytes, expected);
    assert_eq!(batch.entries, 6);
    assert!(batch.offsets[..6].windows(2).all(|pair| pair[0] < pair[1]));
    assert!(construction.declarations().is_empty());
    assert!(matches!(
        construction.commit(crate::native_host::Teardown {
            report: |_, _, _| {},
            cancel: |_| Ok(()),
            release: |_| Ok(()),
            destroy: |_| {}
        }),
        Err(super::super::Error::DeclarationViewsUnavailable)
    ));
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
        ..Default::default()
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

#[test]
fn changed_image_bytes_cannot_create_duplicate_names_inside_a_hash_collision_run() {
    use super::super::declaration_index::name_hash;
    const FIRST: &str = "entryboludzyf";
    const SECOND: &str = "entryoxzxketp";
    assert_eq!(name_hash(0, FIRST), name_hash(0, SECOND));
    let mut bytes = vec![0; REGISTRATION_MAX_BYTES];
    let records = [FIRST, SECOND].map(|name| {
        Record::SettingGroup(SettingGroup {
            name,
            title: "Group",
            order: 0,
            page: "",
            parent: "",
        })
    });
    let len = records_encode(&records, &mut bytes).unwrap();
    bytes.truncate(len);
    let source = Rc::new(Image {
        bytes: RefCell::new(bytes),
        generation: Cell::new(1),
        live: Cell::new(true),
        errors: Cell::new(0),
        active: Cell::new(false),
        denied: Cell::new(0),
    });
    let mut allocations = Allocations::default();
    let mut store = DeclarationStore::default();
    store
        .prepare(
            pending(&source, false, &mut allocations).unwrap(),
            &mut allocations,
        )
        .unwrap()
        .commit();
    assert!(store.find("fixture", Kind::Group, FIRST, &mut |_| {}));
    assert!(store.find("fixture", Kind::Group, SECOND, &mut |_| {}));
    let at = source
        .bytes
        .borrow()
        .windows(FIRST.len())
        .position(|bytes| bytes == FIRST.as_bytes())
        .unwrap();
    source.bytes.borrow_mut()[at..at + FIRST.len()].copy_from_slice(SECOND.as_bytes());
    assert!(!store.find("fixture", Kind::Group, SECOND, &mut |_| panic!(
        "mutated duplicate"
    )));
    for key in keys(&store) {
        assert!(!store.with_record(key, &mut |_| panic!("duplicate key")));
    }
    assert!(source.errors.get() > 0);
    source.bytes.borrow_mut()[at..at + FIRST.len()].copy_from_slice(FIRST.as_bytes());
    assert!(store.find("fixture", Kind::Group, FIRST, &mut |_| {}));
    assert!(store.find("fixture", Kind::Group, SECOND, &mut |_| {}));
}

#[test]
fn removing_owner_releases_module_immediately_and_invalidates_keys() {
    let mut store = DeclarationStore::default();
    let mut allocations = Allocations::default();
    let source = image();
    let module = Rc::downgrade(&source);
    store
        .prepare(
            pending(&source, false, &mut allocations).unwrap(),
            &mut allocations,
        )
        .unwrap()
        .commit();
    let key = keys(&store)[0];
    drop(source);
    assert!(module.upgrade().is_some());
    store.remove_owner(&Id::new_static("fixture"), 1);
    assert!(module.upgrade().is_none());
    assert!(keys(&store).is_empty());
    assert!(!store.with_record(key, &mut |_| panic!("removed module")));
}

#[test]
fn lookup_does_not_decode_an_unrelated_record_in_the_same_batch() {
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
    // The extra follows the group and spec in this batch.
    let extra = keys(&store)
        .into_iter()
        .find(|key| {
            let mut extra = false;
            store.with_record(*key, &mut |r| extra = matches!(r, Record::SettingExtra(_)));
            extra
        })
        .unwrap();
    source.bytes.borrow_mut()[1] = 0xff;
    assert!(store.with_record(extra, &mut |r| assert!(matches!(
        r,
        Record::SettingExtra(_)
    ))));
    assert!(
        !store.find("fixture", Kind::Group, "group", &mut |_| panic!(
            "corrupt group"
        ))
    );
}

#[test]
#[ignore = "host timing evidence; run explicitly with --nocapture"]
fn host_lookup_timing() {
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
    let start = std::time::Instant::now();
    for _ in 0..100_000 {
        assert!(store.find(
            "fixture",
            Kind::Extra,
            core::hint::black_box("flag"),
            &mut |r| {
                core::hint::black_box(r);
            }
        ));
    }
    std::println!("100000 guarded name lookups: {:?}", start.elapsed());
}

#[test]
fn raw_lookup_uses_stored_offset_and_budget_overflow_fails_before_reserve() {
    use super::super::raw_declarations::RawDeclarations;
    let source = image();
    let mut raw = RawDeclarations::default();
    {
        let bytes = source.bytes.borrow();
        assert!(matches!(
            raw.prepare_retaining(
                None,
                Some(&bytes),
                &records_decode(&bytes).unwrap(),
                usize::MAX
            ),
            Err(ERR_NO_SPACE)
        ));
        assert_eq!((raw.batches.capacity(), raw.index.capacity()), (0, 0));
        let prepared = raw
            .prepare(Some(&bytes), Some(&bytes), &records_decode(&bytes).unwrap())
            .unwrap();
        raw.commit(prepared);
    }
    raw.sort_index();
    source.bytes.borrow_mut()[1] = 0xff;
    let bytes = source.bytes.borrow();
    assert!(matches!(
        raw.find(Some(&bytes), 2, "flag"),
        Ok(Some(Record::SettingExtra(_)))
    ));
    assert!(raw.find(Some(&bytes), 0, "group").is_err());
}

#[test]
fn shifted_image_bytes_fail_at_still_in_bounds_offsets() {
    use super::super::raw_declarations::RawDeclarations;
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
    let mut raw = RawDeclarations::default();
    {
        let bytes = source.bytes.borrow();
        let prepared = raw
            .prepare(Some(&bytes), Some(&bytes), &records_decode(&bytes).unwrap())
            .unwrap();
        raw.commit(prepared);
    }
    raw.sort_index();
    {
        let mut bytes = source.bytes.borrow_mut();
        let end = bytes.len() - 1;
        bytes.copy_within(..end, 1);
    }
    let bytes = source.bytes.borrow();
    for key in keys(&store) {
        assert!(usize::from(key.record) < bytes.len());
        assert!(!store.with_record(key, &mut |_| panic!("shifted frame")));
    }
    for entry in &raw.index {
        assert!(usize::from(raw.batches[0].offsets[entry.record()]) < bytes.len());
        assert!(raw.record(Some(&bytes), *entry).is_err());
    }
    assert!(
        raw.visit("fixture", Some(&bytes), &mut |_| panic!("shifted visit"))
            .is_err()
    );
}

#[test]
fn changed_image_font_names_are_unique_across_face_and_extension_runs() {
    const FACE: &str = "entryboludzyf";
    const EXTENSION: &str = "entryoxzxketp";
    assert_eq!(
        super::super::declaration_index::name_hash(4, FACE),
        super::super::declaration_index::name_hash(4, EXTENSION)
    );
    for change_face in [false, true] {
        let records = [
            Record::ResourceFontFace(ResourceFontFace {
                name: FACE,
                path: "fonts/a.ttf",
                family: "Fixture",
                style: 0,
                weight: 400,
                role: "",
                priority: 0,
            }),
            Record::ResourceFontExtension(ResourceFontExtension {
                name: EXTENSION,
                path: "fonts/a.ppfp",
                extends: "core.face",
            }),
        ];
        let mut bytes = vec![0; REGISTRATION_MAX_BYTES];
        let len = records_encode(&records, &mut bytes).unwrap();
        bytes.truncate(len);
        let source = Rc::new(Image {
            bytes: RefCell::new(bytes),
            generation: Cell::new(1),
            live: Cell::new(true),
            errors: Cell::new(0),
            active: Cell::new(false),
            denied: Cell::new(0),
        });
        let mut allocations = Allocations::default();
        let mut store = DeclarationStore::default();
        store
            .prepare(
                pending(&source, false, &mut allocations).unwrap(),
                &mut allocations,
            )
            .unwrap()
            .commit();
        assert!(store.find("fixture", Kind::Face, FACE, &mut |_| {}));
        assert!(store.find("fixture", Kind::Extension, EXTENSION, &mut |_| {}));
        let (old, new) = if change_face {
            (FACE, EXTENSION)
        } else {
            (EXTENSION, FACE)
        };
        let at = source
            .bytes
            .borrow()
            .windows(old.len())
            .position(|bytes| bytes == old.as_bytes())
            .unwrap();
        source.bytes.borrow_mut()[at..at + old.len()].copy_from_slice(new.as_bytes());
        for kind in [Kind::Face, Kind::Extension] {
            assert!(!store.find("fixture", kind, new, &mut |_| panic!(
                "cross-kind duplicate"
            )));
        }
        for key in keys(&store) {
            assert!(!store.with_record(key, &mut |_| panic!("duplicate key")));
        }
        assert!(source.errors.get() > 0);
        source.bytes.borrow_mut()[at..at + old.len()].copy_from_slice(old.as_bytes());
        assert!(store.find("fixture", Kind::Face, FACE, &mut |_| {}));
        assert!(store.find("fixture", Kind::Extension, EXTENSION, &mut |_| {}));
    }
}

struct CleanupContext {
    store: Rc<RefCell<DeclarationStore>>,
    busy: Cell<bool>,
    destroyed: Rc<Cell<bool>>,
    reports: Rc<Cell<usize>>,
}
impl CleanupContext {
    fn teardown() -> super::super::Teardown<Self> {
        use super::super::{Error, Teardown};
        Teardown {
            report: |context, step, error| {
                assert_eq!(step, super::super::TeardownStep::Cancel);
                assert_eq!(error, Error::Busy);
                context.reports.set(context.reports.get() + 1);
            },
            cancel: |context| {
                if context.busy.replace(false) {
                    Err(Error::Busy)
                } else {
                    Ok(())
                }
            },
            release: |context| {
                context
                    .store
                    .try_borrow_mut()
                    .map_err(|_| Error::Busy)?
                    .remove_owner(&Id::new_static("fixture"), 1);
                Ok(())
            },
            destroy: |context| context.destroyed.set(true),
        }
    }
}
struct CleanupModule {
    store: Rc<RefCell<DeclarationStore>>,
    batch_live: Rc<Cell<bool>>,
    destroyed: Rc<Cell<bool>>,
    unmapped: Rc<Cell<bool>>,
}
impl Drop for CleanupModule {
    fn drop(&mut self) {
        assert!(
            keys(&self.store.borrow()).is_empty(),
            "declarations must be removed first"
        );
        assert!(!self.batch_live.get(), "batch must be dropped first");
        assert!(self.destroyed.get());
        self.unmapped.set(true);
    }
}
struct CleanupBatchRecord(Rc<Cell<bool>>);
impl Drop for CleanupBatchRecord {
    fn drop(&mut self) {
        self.0.set(false);
    }
}

#[test]
fn busy_cleanup_releases_declarations_and_batch_before_module_unmap() {
    check_busy_cleanup(false);
}

#[test]
fn busy_fallback_releases_declarations_and_batch_before_module_unmap() {
    check_busy_cleanup(true);
}

fn check_busy_cleanup(fallback: bool) {
    use super::super::{Error, HostOwner};
    let store = Rc::new(RefCell::new(DeclarationStore::default()));
    let batch_live = Rc::new(Cell::new(true));
    let destroyed = Rc::new(Cell::new(false));
    let reports = Rc::new(Cell::new(0));
    let unmapped = Rc::new(Cell::new(false));
    let source = image();
    let source_guard = Rc::downgrade(&source);
    let mut allocations = Allocations::default();
    store
        .borrow_mut()
        .prepare(
            pending(&source, false, &mut allocations).unwrap(),
            &mut allocations,
        )
        .unwrap()
        .commit();
    drop(source);
    let owner = Box::pin(HostOwner::new(
        CleanupContext {
            store: Rc::clone(&store),
            busy: Cell::new(true),
            reports: Rc::clone(&reports),
            destroyed: Rc::clone(&destroyed),
        },
        CleanupModule {
            store: Rc::clone(&store),
            batch_live: Rc::clone(&batch_live),
            destroyed: Rc::clone(&destroyed),
            unmapped: Rc::clone(&unmapped),
        },
    ));
    let mut construction = owner.as_ref().begin("fixture").unwrap();
    assert!(
        construction
            .stage(CleanupBatchRecord(Rc::clone(&batch_live)))
            .is_ok()
    );
    let batch = construction.commit(CleanupContext::teardown()).unwrap();
    if fallback {
        drop(batch);
        assert!(!batch_live.get());
    } else {
        assert_eq!(owner.as_ref().prepare_teardown(), Err(Error::Busy));
        assert!(!keys(&store.borrow()).is_empty());
        assert!(batch_live.get() && !unmapped.get());
        owner.as_ref().prepare_teardown().unwrap();
        assert!(keys(&store.borrow()).is_empty());
        assert!(
            source_guard.upgrade().is_none(),
            "store's module guard released immediately"
        );
        assert_eq!(owner.as_ref().finish(), Err(Error::Busy));
        drop(batch);
        assert!(!batch_live.get());
        owner.as_ref().finish().unwrap();
        assert!(destroyed.get() && !unmapped.get());
    }
    drop(owner);
    assert_eq!(reports.get(), usize::from(fallback));
    assert!(source_guard.upgrade().is_none());
    assert!(
        unmapped.get(),
        "module actually unmapped after both releases"
    );
}

#[path = "declaration_screen_action_tests.rs"]
mod screen_action;

#[path = "declaration_reg_admission_tests.rs"]
mod reg_admission;

// A mixed batch retains this unnamed record without giving it a Kind index.
fn unnamed_fixture(owned: bool) -> (Rc<Image>, DeclarationStore, DeclKey) {
    let source = image();
    let records = [
        Record::SettingGroup(SettingGroup {
            name: "group",
            title: "Group",
            order: 0,
            page: "",
            parent: "",
        }),
        Record::Registration(Registration::Draw { token: 17 }),
    ];
    let mut bytes = vec![0; REGISTRATION_MAX_BYTES];
    let len = records_encode(&records, &mut bytes).unwrap();
    bytes.truncate(len);
    let offset = super::super::raw_declarations::offsets(&bytes, records.len()).unwrap()[1];
    *source.bytes.borrow_mut() = bytes;
    let mut store = DeclarationStore::default();
    let mut allocations = Allocations::default();
    store
        .prepare(
            pending(&source, owned, &mut allocations).unwrap(),
            &mut allocations,
        )
        .unwrap()
        .commit();
    let key = DeclKey {
        record: offset,
        ..keys(&store)[0]
    };
    (source, store, key)
}

#[test]
fn unnamed_record_visit_uses_both_backings_and_holds_the_owner_guard() {
    for owned in [false, true] {
        let (source, store, key) = unnamed_fixture(owned);
        assert!(!store.with_record(key, &mut |_| panic!("unnamed indexed record")));
        assert!(store.visit_record(key, &mut |record| {
            assert!(matches!(
                record,
                Record::Registration(Registration::Draw { token: 17 })
            ));
            assert!(source.active.get());
            assert!(source.bytes.try_borrow_mut().is_err());
        }));
    }
}

#[test]
fn unnamed_visit_rejects_stale_missing_and_reused_owner_keys() {
    for owned in [false, true] {
        let (source, mut store, key) = unnamed_fixture(owned);
        let refused: &mut dyn for<'a> FnMut(Record<'a>) =
            &mut |_| panic!("invalid record reached reader");
        source.generation.set(2);
        assert!(!store.visit_record(key, refused));
        source.generation.set(1);
        source.live.set(false);
        assert!(!store.visit_record(key, refused));
        source.live.set(true);
        assert!(!store.visit_record(
            DeclKey {
                owner: u16::MAX,
                ..key
            },
            refused
        ));
        assert!(!store.visit_record(
            DeclKey {
                generation: 2,
                ..key
            },
            refused
        ));
        assert!(!store.visit_record(
            DeclKey {
                batch: u16::MAX,
                ..key
            },
            refused
        ));
        assert!(!store.visit_record(
            DeclKey {
                record: u16::MAX,
                ..key
            },
            refused
        ));
        store.remove_owner(&Id::new_static("fixture"), 1);
        assert!(!store.visit_record(key, refused));
        let replacement = image();
        let mut allocations = Allocations::default();
        store
            .prepare(
                pending(&replacement, owned, &mut allocations).unwrap(),
                &mut allocations,
            )
            .unwrap()
            .commit();
        assert!(!store.visit_record(key, refused));
    }
}

#[test]
fn unnamed_visit_revalidates_image_bytes_instead_of_caching_the_record() {
    let (source, store, key) = unnamed_fixture(false);
    let offset = usize::from(key.record);
    let byte = source.bytes.borrow()[offset];
    source.bytes.borrow_mut()[offset] = 0xff;
    assert!(!store.visit_record(key, &mut |_| panic!("corrupt record")));
    source.bytes.borrow_mut().truncate(offset);
    assert!(!store.visit_record(key, &mut |_| panic!("truncated record")));
    source.bytes.borrow_mut().resize(offset + 1, byte);
    assert!(!store.visit_record(key, &mut |_| panic!("short record")));
}

/// An owner holding groups and actions in two batches, interleaved.
fn two_kinds_in_two_batches() -> [Vec<u8>; 2] {
    let group = |name| {
        Record::SettingGroup(SettingGroup {
            name,
            title: "Group",
            order: 0,
            page: "",
            parent: "",
        })
    };
    let action = |name, token| {
        Record::ActionSpec(ActionSpec {
            name,
            title: "Action",
            token,
            flags: 0,
        })
    };
    let encode = |records: &[Record<'_>]| {
        let mut bytes = vec![0; REGISTRATION_MAX_BYTES];
        let len = records_encode(records, &mut bytes).unwrap();
        bytes.truncate(len);
        bytes
    };
    [
        encode(&[
            action("zed", 1),
            group("beta"),
            action("alpha", 2),
            group("alpha"),
        ]),
        encode(&[group("gamma"), action("mid", 3)]),
    ]
}

fn staged(allocations: &mut Allocations) -> Result<PendingDeclarations, i32> {
    let source = image();
    let mut pending = PendingDeclarations::new(Id::new_static("fixture"), source);
    for batch in two_kinds_in_two_batches() {
        pending.stage_batch(&batch, allocations)?;
    }
    Ok(pending)
}

/// The ranges of the one list: in kind order, whole, and each kind's records
/// together (`starts[k]..starts[k + 1]`).
fn ranges_hold(pending: &PendingDeclarations) {
    assert!(pending.starts.windows(2).all(|pair| pair[0] <= pair[1]));
    assert_eq!(pending.starts[0], 0);
    assert_eq!(
        usize::from(pending.starts[KIND_COUNT]),
        pending.slots.len(),
        "the last offset ends the list"
    );
}

#[test]
fn every_kind_lives_in_one_list_by_ranges_and_names_are_found_in_their_kind() {
    let mut allocations = Allocations::default();
    let pending = staged(&mut allocations).unwrap();
    ranges_hold(&pending);
    assert_eq!(pending.of_kind(Kind::Group).len(), 3);
    assert_eq!(pending.of_kind(Kind::Action).len(), 3);
    assert_eq!(pending.slots.len(), 6);
    let mut store = DeclarationStore::default();
    store.prepare(pending, &mut allocations).unwrap().commit();
    let owner = store.owners[0].as_ref().unwrap();
    for kind in [Kind::Group, Kind::Action] {
        let slots = owner.of_kind(kind);
        assert!(
            slots.windows(2).all(|pair| pair[0].hash <= pair[1].hash),
            "{kind:?} is sorted by hash once prepared"
        );
    }
    for kind in KINDS {
        let mut count = 0;
        store.keys(kind, &mut |_| count += 1);
        let expected = if matches!(kind, Kind::Group | Kind::Action) {
            3
        } else {
            0
        };
        assert_eq!(count, expected, "{kind:?}");
    }
    let mut allocations = Allocations::default();
    assert_eq!(store.all_keys(&mut allocations).unwrap().len(), 6);
    // A name is found in its own kind's range only. "alpha" is declared as a
    // group and as an action, so it is found in both; every other name in the
    // one kind that declares it.
    let mut found = 0;
    for name in ["alpha", "beta", "gamma"] {
        assert!(store.find("fixture", Kind::Group, name, &mut |_| found += 1));
    }
    for name in ["alpha", "zed", "mid"] {
        assert!(store.find("fixture", Kind::Action, name, &mut |_| found += 1));
    }
    for name in ["beta", "gamma"] {
        assert!(
            !store.find("fixture", Kind::Action, name, &mut |_| found += 1),
            "{name} is no action"
        );
    }
    for name in ["zed", "mid"] {
        assert!(
            !store.find("fixture", Kind::Group, name, &mut |_| found += 1),
            "{name} is no group"
        );
    }
    assert!(!store.find("fixture", Kind::Group, "missing", &mut |_| found += 1));
    // Another kind has none of them.
    for name in ["alpha", "beta", "zed"] {
        assert!(!store.find("fixture", Kind::Screen, name, &mut |_| found += 1));
    }
    assert_eq!(found, 6);
}

#[test]
fn a_failed_reservation_in_staging_keeps_the_list_and_its_offsets_whole() {
    let mut reached_success = false;
    for fail_after in 0..12 {
        let mut allocations = Allocations {
            fail_after: Some(fail_after),
            ..Default::default()
        };
        let source = image();
        let mut pending = PendingDeclarations::new(Id::new_static("fixture"), source);
        let mut results = Vec::new();
        for batch in two_kinds_in_two_batches() {
            results.push(pending.stage_batch(&batch, &mut allocations));
            // Whatever failed, the offsets describe the list exactly.
            ranges_hold(&pending);
        }
        if results.iter().all(Result::is_ok) {
            reached_success = true;
            assert_eq!(pending.slots.len(), 6);
        } else {
            assert!(results.contains(&Err(ERR_NO_MEMORY)));
            // A failed owner stays failed: nothing publishes it.
            let mut store = DeclarationStore::default();
            assert!(store.prepare(pending, &mut Allocations::default()).is_err());
        }
    }
    assert!(reached_success, "the walk reached a fully staged owner");
}

#[test]
fn an_owner_costs_a_flat_amount_however_many_kinds_exist() {
    // The offsets and counts are 16-bit tables, one list for every kind: the
    // owner no longer pays a `Vec` and a counter per kind (`abi/heap-word.md`
    // section 9). On a 32-bit target the build pins it (`PENDING_BUDGET`).
    let per_kind = size_of::<[u16; KIND_COUNT + 1]>() + size_of::<[u16; KIND_COUNT]>();
    assert_eq!(per_kind, 2 * (2 * KIND_COUNT) + 2);
    assert!(
        size_of::<PendingDeclarations>() <= 2 * size_of::<Vec<Slot>>() + per_kind + 160,
        "{}",
        size_of::<PendingDeclarations>()
    );
}

/// An image of `records` for the fixture owner.
fn image_of(records: &[Record<'_>]) -> Rc<Image> {
    let mut bytes = vec![0; REGISTRATION_MAX_BYTES];
    let len = records_encode(records, &mut bytes).unwrap();
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

fn hint<'a>(screen: &'a str, input: u32, text: &'a str) -> Record<'a> {
    Record::HelpHint(HelpHint {
        screen,
        input,
        x: 0,
        y: 0,
        width: 0,
        height: 0,
        text,
    })
}

fn input<'a>(name: &'a str, kind: u32) -> Record<'a> {
    Record::InputSpec(InputSpec {
        name,
        title: "fixture.input.title",
        kind,
        need: 0,
        capability: 0,
        flags: 0,
    })
}

#[test]
fn inputs_and_hints_are_kept_listed_and_removed_with_their_owner() {
    // Hints for another plugin's screen too; one screen takes a hint for each
    // input kind and each text.
    let source = image_of(&[
        input("chord", 0),
        input("shake", 1),
        hint("reader.page", 0, "fixture.help.page"),
        hint("reader.page", 1, "fixture.help.page"),
        hint("reader.page", 2, "fixture.help.page"),
        hint("reader.page", 0, "fixture.help.other"),
        hint("fixture.main", 0, "fixture.help.page"),
    ]);
    let mut store = DeclarationStore::default();
    let mut allocations = Allocations::default();
    store
        .prepare(
            pending(&source, false, &mut allocations).unwrap(),
            &mut allocations,
        )
        .unwrap()
        .commit();
    let count = |store: &DeclarationStore, kind| {
        let mut count = 0;
        store.keys(kind, &mut |_| count += 1);
        count
    };
    assert_eq!(count(&store, Kind::InputSpec), 2);
    assert_eq!(count(&store, Kind::HelpHint), 5);
    assert!(store.find("fixture", Kind::InputSpec, "shake", &mut |_| {}));
    assert!(!store.find("fixture", Kind::InputSpec, "other", &mut |_| {}));
    assert!(store.find_parts(
        "fixture",
        Kind::HelpHint,
        &["reader.page", "2", "fixture.help.page"],
        &mut |_| {}
    ));
    store.remove_owner(&Id::new_static("fixture"), 1);
    assert_eq!(
        count(&store, Kind::InputSpec) + count(&store, Kind::HelpHint),
        0
    );
}
