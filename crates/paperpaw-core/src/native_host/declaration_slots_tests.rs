//! The 4-byte slots (#248): records addressed by their place in a batch,
//! reached by walking the frame headers before them, and the batch limit.
use super::*;

/// Where record `index` of `bytes` starts.
fn start(bytes: &[u8], index: usize) -> usize {
    bytes.len()
        - super::super::super::raw_declarations::record_at(bytes, index)
            .unwrap()
            .len()
}

#[test]
fn record_at_walks_the_frames_to_each_place_and_refuses_the_rest() {
    use super::super::super::raw_declarations::record_at;
    let source = image();
    let bytes = source.bytes.borrow();
    let kinds: Vec<Kind> = (0..6)
        .map(|index| {
            let (record, _) = host_decode_prefix(record_at(&bytes, index).unwrap()).unwrap();
            Kind::of(&record).unwrap()
        })
        .collect();
    assert_eq!(
        kinds,
        [
            Kind::Group,
            Kind::Spec,
            Kind::Extra,
            Kind::Hyphenation,
            Kind::Face,
            Kind::Extension
        ]
    );
    // Past the batch's count, an empty batch, a frame that leaves the batch.
    assert_eq!(record_at(&bytes, 6).err(), Some(ERR_INVALID));
    assert_eq!(record_at(&[], 0).err(), Some(ERR_INVALID));
    let cut = &bytes[..start(&bytes, 2) + 1];
    assert_eq!(record_at(cut, 3).err(), Some(ERR_INVALID));
    // Only the headers before the place are read: its own frame may be cut.
    assert!(record_at(cut, 2).is_ok());
}

#[test]
fn a_damaged_frame_header_makes_the_records_after_it_unreadable() {
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
    let mut spec = None;
    store.keys(Kind::Spec, &mut |key| spec = Some(key));
    let spec = spec.unwrap();
    assert!(store.with_record(spec, &mut |_| {}));
    {
        // The group's length, after its tag: longer than the group is.
        let mut bytes = source.bytes.borrow_mut();
        let mut at = start(&bytes, 0);
        while bytes[at] & 0x80 != 0 {
            at += 1;
        }
        at += 1;
        assert!(bytes[at] < 0x7f, "a one-byte length");
        bytes[at] = 0x7f;
    }
    let errors = source.errors.get();
    assert!(!store.with_record(spec, &mut |_| panic!("record behind a damaged header")));
    assert_eq!(source.errors.get(), errors + 1);
}

#[test]
fn a_batch_past_the_last_place_a_slot_holds_is_no_space() {
    let mut allocations = Allocations::default();
    let source = image();
    let mut pending = PendingDeclarations::new(Id::new_static("fixture"), source);
    let [first, second] = two_kinds_in_two_batches();
    // Stand-ins for the batches before: only their count matters here.
    pending
        .kept
        .batches
        .refs_mut()
        .unwrap()
        .reserve_exact(BATCHES_MAX - 1);
    for _ in 1..BATCHES_MAX {
        pending
            .kept
            .batches
            .refs_mut()
            .unwrap()
            .push(DeclRef::Image { offset: 0, len: 0 });
    }
    // The last batch a slot can name is taken and indexed.
    pending.stage_batch(&first, &mut allocations).unwrap();
    assert!(!pending.kept.slots.is_empty());
    let last = u16::try_from(BATCHES_MAX - 1).unwrap();
    assert!(pending.kept.slots.iter().all(|slot| slot.batch() == last));
    // The next is refused, and nothing of it is kept.
    let slots = pending.kept.slots.len();
    assert_eq!(
        pending.stage_batch(&second, &mut allocations).err(),
        Some(ERR_NO_SPACE)
    );
    assert_eq!(pending.kept.batches.len(), BATCHES_MAX);
    assert_eq!(pending.kept.slots.len(), slots);
}

#[test]
fn two_names_on_one_key_are_each_found() {
    use super::super::super::declaration_index::record_hash;
    fn group(name: &str) -> Record<'_> {
        Record::SettingGroup(SettingGroup {
            name,
            title: "Group",
            order: 0,
            page: "",
            parent: "",
        })
    }
    let names: Vec<std::string::String> = (0..4096).map(|n| std::format!("g{n}")).collect();
    let key = |name: &str| Kind::Group.slot_key(record_hash(group(name)).unwrap());
    // Two names whose hashes differ but whose ten bits of key are equal.
    let (a, b) = names
        .iter()
        .enumerate()
        .find_map(|(at, a)| {
            names[at + 1..]
                .iter()
                .find(|b| key(b) == key(a))
                .map(|b| (a.as_str(), b.as_str()))
        })
        .unwrap();
    assert_ne!(record_hash(group(a)), record_hash(group(b)));
    let mut bytes = vec![0; REGISTRATION_MAX_BYTES];
    let len = records_encode(&[group(a), group(b)], &mut bytes).unwrap();
    bytes.truncate(len);
    let mut allocations = Allocations::default();
    let mut pending = PendingDeclarations::new(Id::new_static("fixture"), image());
    pending.stage_batch(&bytes, &mut allocations).unwrap();
    let mut store = DeclarationStore::default();
    store.prepare(pending, &mut allocations).unwrap().commit();
    let owner = store.owners[0].as_ref().unwrap();
    assert_eq!(
        owner
            .run(Kind::Group, record_hash(group(a)).unwrap())
            .count(),
        2
    );
    for name in [a, b] {
        let mut found = None;
        assert!(store.find("fixture", Kind::Group, name, &mut |record| {
            if let Record::SettingGroup(group) = record {
                found = Some(std::string::String::from(group.name));
            }
        }));
        assert_eq!(found.as_deref(), Some(name));
    }
    assert!(!store.find("fixture", Kind::Group, "g-none", &mut |_| {}));
}
