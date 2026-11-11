//! Screen/action storage uses the same guarded ownership as other declarations.
use super::*;

fn source(records: &[Record<'_>]) -> Rc<Image> {
    let source = image();
    let mut bytes = vec![0; REGISTRATION_MAX_BYTES];
    let len = records_encode(records, &mut bytes).unwrap();
    bytes.truncate(len);
    source.bytes.replace(bytes);
    source
}
fn pair() -> [Record<'static>; 2] {
    [
        Record::ScreenSpec(ScreenSpec {
            name: "panel",
            title: "Panel",
            token: 11,
            flags: 1,
        }),
        Record::ActionSpec(ActionSpec {
            name: "panel",
            title: "Action",
            token: 12,
            flags: 1,
        }),
    ]
}
fn check_record(record: Record<'_>, kind: Kind) {
    match (kind, record) {
        (Kind::Screen, Record::ScreenSpec(r)) => {
            assert_eq!((r.name, r.token, r.flags), ("panel", 11, 1))
        }
        (Kind::Action, Record::ActionSpec(r)) => {
            assert_eq!((r.name, r.token, r.flags), ("panel", 12, 1))
        }
        _ => panic!("wrong declaration kind"),
    }
}

#[test]
fn screen_and_action_can_share_a_name_and_preserve_tokens_on_both_paths() {
    for owned in [false, true] {
        let source = source(&pair());
        let mut store = DeclarationStore::default();
        let mut allocations = Allocations::default();
        store
            .prepare(
                pending(&source, owned, &mut allocations).unwrap(),
                &mut allocations,
            )
            .unwrap()
            .commit();
        let admitted = keys(&store);
        assert_eq!(admitted.len(), 2);
        for (key, kind) in admitted.into_iter().zip([Kind::Screen, Kind::Action]) {
            assert!(store.find("fixture", kind, "panel", &mut |r| check_record(r, kind)));
            assert!(store.with_record(key, &mut |r| check_record(r, kind)));
        }
    }
}

#[test]
fn committed_raw_construction_refuses_duplicates_and_store_refuses_invalid_flags() {
    for record in pair() {
        for owned in [false, true] {
            let source = source(&[record, record]);
            let bytes = source.bytes.borrow();
            let image = (!owned).then_some(bytes.as_slice());
            let mut raw = crate::native_host::raw_declarations::RawDeclarations::default();
            let prepared = raw
                .prepare(image, Some(&bytes), &records_decode(&bytes).unwrap())
                .unwrap();
            raw.commit(prepared);
            raw.sort_index();
            // Construction checks this before the committed batch may enter
            // the declaration store. The store is not a second constructor.
            assert_eq!(raw.check_duplicates(image), Err(ERR_INVALID));
        }
    }
    let source = source(&[Record::ScreenSpec(ScreenSpec {
        name: "panel",
        title: "Panel",
        token: 1,
        flags: 1 << 9,
    })]);
    let mut allocations = Allocations::default();
    assert!(matches!(
        pending(&source, false, &mut allocations),
        Err(paperpaw_native::host_glue::ERR_UNSUPPORTED)
    ));
}

#[test]
fn a_screen_may_be_declared_with_its_hints_hidden() {
    // Bit 8 `hints_hidden`, beside the other policy bits.
    for flags in [1 << 8, (1 << 8) | (1 << 2) | (1 << 5)] {
        let source = source(&[Record::ScreenSpec(ScreenSpec {
            name: "panel",
            title: "Panel",
            token: 1,
            flags,
        })]);
        let mut allocations = Allocations::default();
        assert!(
            pending(&source, false, &mut allocations).is_ok(),
            "{flags:#x}"
        );
    }
}

#[test]
fn keys_are_invalid_after_replacement_unload_and_image_generation_change() {
    for owned in [false, true] {
        let source = source(&pair());
        let mut store = DeclarationStore::default();
        let mut allocations = Allocations::default();
        store
            .prepare(
                pending(&source, owned, &mut allocations).unwrap(),
                &mut allocations,
            )
            .unwrap()
            .commit();
        let old = keys(&store);
        source.generation.set(2);
        for key in &old {
            assert!(!store.with_record(*key, &mut |_| panic!("stale generation")));
        }
        store
            .prepare(
                pending(&source, owned, &mut allocations).unwrap(),
                &mut allocations,
            )
            .unwrap()
            .commit();
        for key in old {
            assert!(!store.with_record(key, &mut |_| panic!("old incarnation")));
        }
        let live = keys(&store);
        assert_eq!(live.len(), 2);
        store.remove_owner(&Id::new_static("fixture"), 2);
        assert!(keys(&store).is_empty());
        for key in live {
            assert!(!store.with_record(key, &mut |_| panic!("unloaded owner")));
        }
    }
}

#[test]
fn every_new_kind_allocation_refusal_leaves_the_store_unpublished() {
    for owned in [false, true] {
        let source = source(&pair());
        let mut succeeded = false;
        for fail_after in 0..32 {
            let mut store = DeclarationStore::default();
            let mut allocations = Allocations {
                fail_after: Some(fail_after),
                ..Default::default()
            };
            let result = pending(&source, owned, &mut allocations).and_then(|p| {
                store
                    .prepare(p, &mut allocations)
                    .map(|prepared| prepared.commit())
            });
            match result {
                Ok(()) => {
                    assert_eq!(keys(&store).len(), 2);
                    succeeded = true;
                    break;
                }
                Err(error) => {
                    assert_eq!(error, ERR_NO_MEMORY);
                    assert!(keys(&store).is_empty());
                }
            }
        }
        assert!(succeeded);
    }
}

#[test]
fn owned_screen_copies_exhaust_the_existing_aggregate_budget_without_publication() {
    let source = source(&[]);
    let mut pending = PendingDeclarations::new(Id::new_static("fixture"), source);
    let mut allocations = Allocations::default();
    let mut store = DeclarationStore::default();
    let mut refused = false;
    for batch in 0..1024 {
        let names: [alloc::string::String; 8] =
            core::array::from_fn(|index| alloc::format!("screen-{batch}-{index}"));
        let records: [Record<'_>; 8] = core::array::from_fn(|index| {
            Record::ScreenSpec(ScreenSpec {
                name: &names[index],
                title: "Panel",
                token: index as u32,
                flags: 0,
            })
        });
        let mut bytes = vec![0; REGISTRATION_MAX_BYTES];
        let len = records_encode(&records, &mut bytes).unwrap();
        bytes.truncate(len);
        match pending.stage_batch(&bytes, &mut allocations) {
            Ok(()) => assert!(pending.resident <= DECLARATIONS_MAX_BYTES),
            Err(error) => {
                assert_eq!(error, ERR_NO_SPACE);
                refused = true;
                break;
            }
        }
    }
    assert!(refused);
    assert!(matches!(
        store.prepare(pending, &mut allocations),
        Err(ERR_NO_SPACE)
    ));
    assert!(keys(&store).is_empty());
}

#[test]
fn every_declaration_kind_matches_the_raw_index_category() {
    let source = image();
    let bytes = source.bytes.borrow();
    let batch = records_decode(&bytes).unwrap();
    let mut seen = [false; Kind::Action as usize + 1];
    for record in batch.iter().chain(pair()) {
        let kind = Kind::of(&record).unwrap();
        assert_eq!(
            Some(kind.category()),
            crate::native_host::declaration_index::key(record).map(|(category, _)| category)
        );
        seen[kind as usize] = true;
    }
    assert!(seen.into_iter().all(|kind| kind));
}
