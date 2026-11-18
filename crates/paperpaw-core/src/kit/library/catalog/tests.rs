use super::*;
use paperpaw_sdk::MemoryStorage;

/// Two paths whose hashes share their low 32 bits.
fn alike() -> (String, String) {
    let mut seen = alloc::collections::BTreeMap::new();
    for n in 0u32.. {
        let path = format!("books/b{n}.txt");
        if let Some(other) = seen.insert(fnv1a64(&path) as u32, path.clone()) {
            return (other, path);
        }
    }
    unreachable!()
}

#[test]
fn books_whose_hashes_share_their_low_bits_are_kept_apart() {
    let (a, b) = alike();
    assert_ne!(fnv1a64(&a), fnv1a64(&b));
    let storage = MemoryStorage::new();
    write(
        &storage,
        &[
            Record::of(&a, &Book::default()),
            Record::of(&b, &Book::default()),
        ],
    )
    .unwrap();
    note(&storage, &b, Status::new(Some(40), false, true)).unwrap();
    forget(&storage, &a).unwrap();
    let changes = changes(&storage);
    assert!(changes.removed(fnv1a64(&a)));
    assert!(!changes.removed(fnv1a64(&b)));
    assert_eq!(changes.status(fnv1a64(&b)).unwrap().progress, Some(40));
    // Read, the other book stays.
    let mut left = Vec::new();
    each(&storage, &mut |r| left.push(r.path));
    assert_eq!(left, [b]);
}

#[test]
fn a_catalog_of_format_1_is_read() {
    let storage = MemoryStorage::new();
    storage
        .write(
            &catalog_path(),
            b"books/a.epub\tAnn\tFirst\tTales\t1\tKids\nbooks/b.txt\t\tSecond\t\t\t\n",
        )
        .unwrap();
    let mut all = Vec::new();
    each(&storage, &mut |r| all.push(r));
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].series, "Tales");
    assert_eq!(all[0].status, Status::default());
    assert_eq!(all[1].title, "Second");
}

#[test]
fn series_numbers_order() {
    assert_eq!(series_order("2"), 2000);
    assert_eq!(series_order(" 2.5 "), 2500);
    assert_eq!(series_order("2.25"), 2250);
    assert_eq!(series_order("II"), u32::MAX);
    assert!(series_order("10") > series_order("9"));
}

#[test]
fn its_state() {
    let storage = MemoryStorage::new();
    assert_eq!(state(&storage), State::Missing);
    storage.write(&building_path(), b"").unwrap();
    assert_eq!(state(&storage), State::Building);
    write(&storage, &[]).unwrap();
    assert!(matches!(state(&storage), State::Ready { .. }));
}
