//! REG declarations (`registry.md` section 6) are admitted into the store and
//! the catalog on both storage paths, as all-or-nothing batches.
use super::*;
use crate::native_host::{declaration_checks::View, reg_catalog::RegCatalog};
use paperpaw_native::host_glue::ERR_UNSUPPORTED;

/// A canonical `bool` setting value (`true`): layout 1, kind 1, one payload byte.
const TRUE: [u8; 13] = [1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1];

const SCREEN: Record<'static> = Record::ScreenSpec(ScreenSpec {
    name: "page",
    title: "Page",
    token: 1,
    flags: 0,
});

fn rows() -> [Record<'static>; 7] {
    [
        Record::SlotSpec(SlotSpec {
            name: "slot",
            title: "Slot",
        }),
        Record::SlotOffer(SlotOffer {
            slot: "core.settings",
            screen: "fixture.page",
            priority: 3,
        }),
        Record::KeyBind(KeyBind {
            layer: "core.reader",
            input: "core.next",
            action: "fixture.next",
            when_key: "fixture.mode",
            when_value: "one",
        }),
        Record::SettingDefault(SettingDefault {
            key: "other.setting",
            value: &TRUE,
        }),
        Record::SettingChoice(SettingChoice {
            key: "other.setting",
            value: "one",
            title: "One",
        }),
        Record::PresetSpec(PresetSpec {
            name: "preset",
            kind: "core.reading",
            title: "Preset",
            help: "",
        }),
        Record::PresetValue(PresetValue {
            preset: "fixture.preset",
            key: "other.setting",
            value: &TRUE,
            flags: 0,
        }),
    ]
}
const KINDS_OF_ROWS: [Kind; 7] = [
    Kind::SlotSpec,
    Kind::SlotOffer,
    Kind::KeyBind,
    Kind::SettingDefault,
    Kind::SettingChoice,
    Kind::PresetSpec,
    Kind::PresetValue,
];

fn encoded(records: &[Record<'_>]) -> Vec<u8> {
    let mut bytes = vec![0; REGISTRATION_MAX_BYTES];
    let len = records_encode(records, &mut bytes).unwrap();
    bytes.truncate(len);
    bytes
}
fn source(records: &[Record<'_>]) -> Rc<Image> {
    let source = image();
    source.bytes.replace(encoded(records));
    source
}
fn stage(
    id: &'static str,
    source: &Rc<Image>,
    owned: bool,
    allocations: &mut Allocations,
) -> Result<PendingDeclarations, i32> {
    let mut pending = PendingDeclarations::new(Id::new_static(id), source.clone());
    let bytes = source.bytes.borrow().clone();
    if owned {
        pending.stage_batch(&bytes, allocations)?;
    } else {
        pending.stage_batch(&source.bytes.borrow(), allocations)?;
    }
    Ok(pending)
}
fn install(
    store: &mut DeclarationStore,
    id: &'static str,
    source: &Rc<Image>,
    owned: bool,
) -> Result<(), i32> {
    let mut allocations = Allocations::default();
    let pending = stage(id, source, owned, &mut allocations)?;
    store.prepare(pending, &mut allocations)?.commit();
    Ok(())
}
fn count(store: &DeclarationStore, kind: Kind) -> usize {
    let mut count = 0;
    store.keys(kind, &mut |_| count += 1);
    count
}

#[test]
fn every_reg_declaration_installs_and_is_found_on_both_paths() {
    for owned in [false, true] {
        let source = source(&rows());
        let mut store = DeclarationStore::default();
        install(&mut store, "fixture", &source, owned).unwrap();
        for kind in KINDS_OF_ROWS {
            assert_eq!(count(&store, kind), 1, "{kind:?}");
        }
        // Plain names, then every field of the composite keys.
        for (name, kind) in [("slot", Kind::SlotSpec), ("preset", Kind::PresetSpec)] {
            assert!(store.find("fixture", kind, name, &mut |_| {}), "{name}");
        }
        let composite: [(Kind, &[&str]); 5] = [
            (Kind::SlotOffer, &["core.settings", "fixture.page"]),
            (
                Kind::KeyBind,
                &["core.reader", "core.next", "fixture.mode", "one"],
            ),
            (Kind::SettingDefault, &["other.setting"]),
            (Kind::SettingChoice, &["other.setting", "one"]),
            (Kind::PresetValue, &["fixture.preset", "other.setting"]),
        ];
        for (kind, parts) in composite {
            assert!(store.find_parts("fixture", kind, parts, &mut |_| {}));
            // One field different is another key.
            let mut other = parts.to_vec();
            other[0] = "zzz.other";
            assert!(!store.find_parts("fixture", kind, &other, &mut |_| {}));
        }
        let mut allocations = Allocations::default();
        let all = store.all_keys(&mut allocations).unwrap();
        assert_eq!(all.len(), 7);
        for (key, kind) in all.iter().zip(KINDS_OF_ROWS) {
            assert!(store.with_record(*key, &mut |record| {
                assert_eq!(Kind::of(&record), Some(kind));
            }));
        }
    }
}

#[test]
fn offers_of_one_slot_and_bindings_of_one_input_are_separate_keys() {
    let offer = |screen| {
        Record::SlotOffer(SlotOffer {
            slot: "core.settings",
            screen,
            priority: 1,
        })
    };
    let bind = |when_value| {
        Record::KeyBind(KeyBind {
            layer: "global",
            input: "core.up",
            action: "fixture.up",
            when_key: "fixture.mode",
            when_value,
        })
    };
    for owned in [false, true] {
        let source = source(&[
            offer("fixture.one"),
            offer("fixture.two"),
            bind("a"),
            bind("b"),
        ]);
        let mut store = DeclarationStore::default();
        install(&mut store, "fixture", &source, owned).unwrap();
        assert_eq!(count(&store, Kind::SlotOffer), 2);
        assert_eq!(count(&store, Kind::KeyBind), 2);
        for screen in ["fixture.one", "fixture.two"] {
            let mut found = None;
            assert!(store.find_parts(
                "fixture",
                Kind::SlotOffer,
                &["core.settings", screen],
                &mut |record| {
                    let Record::SlotOffer(offer) = record else {
                        panic!("kind")
                    };
                    found = Some(offer.screen == screen);
                }
            ));
            assert_eq!(found, Some(true));
        }
        for when in ["a", "b"] {
            assert!(store.find_parts(
                "fixture",
                Kind::KeyBind,
                &["global", "core.up", "fixture.mode", when],
                &mut |_| {}
            ));
        }
        // Reading by key checks the indexed name against its peers: two
        // offers sharing a first field are no duplicates.
        let mut allocations = Allocations::default();
        for key in store.all_keys(&mut allocations).unwrap() {
            assert!(store.with_record(key, &mut |_| {}));
        }
    }
}

#[test]
fn a_bad_batch_is_refused_as_a_whole_and_the_store_stays_empty() {
    let bad: [(&str, Record<'static>, i32); 8] = [
        (
            "offer of another plugin's screen",
            Record::SlotOffer(SlotOffer {
                slot: "core.settings",
                screen: "other.page",
                priority: 0,
            }),
            ERR_INVALID,
        ),
        (
            "key.bind with a half condition",
            Record::KeyBind(KeyBind {
                layer: "global",
                input: "core.up",
                action: "fixture.up",
                when_key: "fixture.mode",
                when_value: "",
            }),
            ERR_INVALID,
        ),
        (
            "a default with no value",
            Record::SettingDefault(SettingDefault {
                key: "other.setting",
                value: &[],
            }),
            ERR_INVALID,
        ),
        (
            "an unqualified choice key",
            Record::SettingChoice(SettingChoice {
                key: "setting",
                value: "one",
                title: "One",
            }),
            ERR_INVALID,
        ),
        (
            "an empty preset title",
            Record::PresetSpec(PresetSpec {
                name: "preset",
                kind: "core.reading",
                title: "",
                help: "",
            }),
            ERR_INVALID,
        ),
        (
            "an unknown preset-value flag",
            Record::PresetValue(PresetValue {
                preset: "fixture.preset",
                key: "other.setting",
                value: &[],
                flags: 2,
            }),
            ERR_UNSUPPORTED,
        ),
        (
            "an empty slot title",
            Record::SlotSpec(SlotSpec {
                name: "slot",
                title: "",
            }),
            ERR_INVALID,
        ),
        (
            "a bad name",
            Record::SlotSpec(SlotSpec {
                name: "Bad",
                title: "Slot",
            }),
            ERR_INVALID,
        ),
    ];
    for owned in [false, true] {
        for (what, record, status) in bad {
            // The good records before and after it are refused with it.
            let source = source(&[rows()[0], record, rows()[5]]);
            let mut store = DeclarationStore::default();
            assert_eq!(install(&mut store, "fixture", &source, owned), Err(status));
            assert_eq!(count(&store, Kind::SlotSpec), 0, "{what}");
            assert_eq!(count(&store, Kind::PresetSpec), 0, "{what}");
            let mut allocations = Allocations::default();
            assert!(store.all_keys(&mut allocations).unwrap().is_empty());
        }
    }
}

#[test]
fn setup_steps_stay_refused_until_their_own_arm() {
    let step = Record::SetupStep(SetupStep {
        name: "setup",
        title: "Setup",
        help: "",
        order: 0,
        page_kind: 0,
        page: "fixture.page",
        applies_key: "",
        applies_value: "",
        need: 0,
        capability: 0,
        flags: 1,
    });
    let source = source(&[rows()[0], step]);
    let mut store = DeclarationStore::default();
    assert_eq!(
        install(&mut store, "fixture", &source, false),
        Err(ERR_UNSUPPORTED)
    );
    assert_eq!(count(&store, Kind::SlotSpec), 0);
}

#[test]
fn different_plugins_may_declare_the_same_keys_and_unload_apart() {
    for owned in [false, true] {
        let shared = [
            Record::SlotOffer(SlotOffer {
                slot: "core.settings",
                screen: "fixture.page",
                priority: 1,
            }),
            Record::SettingDefault(SettingDefault {
                key: "third.setting",
                value: &TRUE,
            }),
            Record::SettingChoice(SettingChoice {
                key: "third.setting",
                value: "one",
                title: "One",
            }),
        ];
        let rivals = [
            Record::SlotOffer(SlotOffer {
                slot: "core.settings",
                screen: "rival.page",
                priority: 9,
            }),
            shared[1],
            shared[2],
        ];
        let first = source(&shared);
        let second = source(&rivals);
        let mut store = DeclarationStore::default();
        install(&mut store, "fixture", &first, owned).unwrap();
        install(&mut store, "rival", &second, owned).unwrap();
        assert_eq!(count(&store, Kind::SettingDefault), 2);
        assert_eq!(count(&store, Kind::SettingChoice), 2);
        assert_eq!(count(&store, Kind::SlotOffer), 2);
        store.remove_owner(&Id::new_static("rival"), 1);
        assert_eq!(count(&store, Kind::SettingDefault), 1);
        assert!(store.find(
            "fixture",
            Kind::SettingDefault,
            "third.setting",
            &mut |_| {}
        ));
        assert!(!store.find("rival", Kind::SettingDefault, "third.setting", &mut |_| {}));
        store.remove_owner(&Id::new_static("fixture"), 1);
        assert_eq!(count(&store, Kind::SlotOffer), 0);
    }
}

#[test]
fn unloading_drops_the_catalog_rows_with_the_owner() {
    for owned in [false, true] {
        let source = source(&rows());
        let store = RefCell::new(DeclarationStore::default());
        install(&mut store.borrow_mut(), "fixture", &source, owned).unwrap();
        let catalog = RegCatalog::new();
        let mut allocations = Allocations::default();
        let keys = store.borrow().all_keys(&mut allocations).unwrap();
        let generation = catalog.replace(&store, &keys, &mut allocations).unwrap();
        for index in 0..7 {
            catalog
                .with_record(&store, generation, index, &mut |_| Ok(()))
                .unwrap();
        }
        store
            .borrow_mut()
            .remove_owner(&Id::new_static("fixture"), 1);
        // A row of an unloaded owner is no longer readable, and the next
        // prune removes it from the catalog.
        assert_eq!(
            catalog.with_record(&store, generation, 0, &mut |_| Ok(())),
            Err(ERR_STATE)
        );
        catalog.prune(&store.borrow()).unwrap();
        assert_ne!(catalog.generation(), generation);
        assert_eq!(
            catalog.with_record(&store, catalog.generation(), 0, &mut |_| Ok(())),
            Err(ERR_STATE)
        );
    }
}

#[test]
fn every_reg_allocation_refusal_leaves_the_store_unpublished() {
    for owned in [false, true] {
        let source = source(&rows());
        let mut succeeded = false;
        for fail_after in 0..40 {
            let mut store = DeclarationStore::default();
            let mut allocations = Allocations {
                fail_after: Some(fail_after),
            };
            let result = stage("fixture", &source, owned, &mut allocations).and_then(|pending| {
                store.prepare(pending, &mut allocations)?.commit();
                let mut keys = Vec::new();
                keys.extend(store.all_keys(&mut allocations)?);
                Ok(keys.len())
            });
            match result {
                Ok(len) => {
                    assert_eq!(len, 7);
                    succeeded = true;
                    break;
                }
                Err(error) => {
                    assert_eq!(error, ERR_NO_MEMORY, "{fail_after}");
                    // A refusal before the commit leaves nothing; one at the
                    // key listing leaves the committed owner, as the catalog's
                    // publication guard then removes it.
                }
            }
        }
        assert!(succeeded);
    }
}

#[test]
fn reg_rows_share_the_aggregate_budget_without_publication() {
    let source = source(&[]);
    let mut pending = PendingDeclarations::new(Id::new_static("fixture"), source);
    let mut allocations = Allocations::default();
    let mut refused = false;
    for batch in 0..1024 {
        let names: [alloc::string::String; 8] =
            core::array::from_fn(|index| alloc::format!("slot-{batch}-{index}"));
        let records: [Record<'_>; 8] = core::array::from_fn(|index| {
            Record::SlotSpec(SlotSpec {
                name: &names[index],
                title: "Slot",
            })
        });
        match pending.stage_batch(&encoded(&records), &mut allocations) {
            Ok(()) => assert!(pending.resident <= DECLARATIONS_MAX_BYTES),
            Err(error) => {
                assert_eq!(error, ERR_NO_SPACE);
                refused = true;
                break;
            }
        }
    }
    assert!(refused);
    let mut store = DeclarationStore::default();
    assert!(store.prepare(pending, &mut allocations).is_err());
    assert_eq!(count(&store, Kind::SlotSpec), 0);
}

fn finish(records: &[Record<'_>]) -> Result<(), i32> {
    let bytes = encoded(records);
    let mut raw = crate::native_host::raw_declarations::RawDeclarations::default();
    let prepared = raw
        .prepare(Some(&bytes), Some(&bytes), &records_decode(&bytes).unwrap())
        .unwrap();
    raw.commit(prepared);
    raw.sort_index();
    let owned = crate::native_host::staged::Declarations::default();
    View {
        raw: &raw,
        owned: &owned,
        image: Some(&bytes),
        plugin: "fixture",
    }
    .finish()
}

#[test]
fn an_offer_needs_a_declared_screen_of_its_own_plugin() {
    let offer = rows()[1];
    assert_eq!(finish(&[offer]), Err(ERR_INVALID));
    assert_eq!(finish(&[offer, SCREEN]), Ok(()));
    // The screen may be declared after the offer, or in another batch.
    assert_eq!(finish(&[SCREEN, offer]), Ok(()));
    // Any other reference stays open: it names other plugins' items too.
    assert_eq!(
        finish(&[rows()[2], rows()[3], rows()[4], rows()[6]]),
        Ok(())
    );
}
