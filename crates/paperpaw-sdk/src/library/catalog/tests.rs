use super::*;
use crate::MemoryStorage;

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
fn written_read_and_searched() {
    let storage = MemoryStorage::new();
    let book = Book::default()
        .title(Some("Second".into()))
        .author(Some("Ann".into()))
        .series(Some("Tales".into()))
        .series_index(Some("2".into()))
        .tags(alloc::vec!["Kids".into(), "Old\u{1f}times".into()])
        .progress(Some(40))
        .added(Some(20_000))
        .fields(alloc::vec![
            ("stars.rating".into(), "4".into()),
            ("other.x".into(), "y".into()),
        ]);
    let record = Record::of("books/b.epub", &book)
        .with_status(Status::new(Some(40), true, true))
        .with_fields(&book, &["stars.rating"]);
    assert_eq!(record.tags().collect::<Vec<_>>(), ["Kids", "Old times"]);
    assert_eq!(record.field("stars.rating"), Some("4"));
    assert_eq!(record.field("other.x"), None, "not kept in the catalog");
    write(
        &storage,
        &[
            record.clone(),
            Record::of("books/Untitled.txt", &Book::default()),
        ],
    )
    .unwrap();
    let mut all = Vec::new();
    each(&storage, &mut |r| all.push(r));
    assert_eq!(all.len(), 2);
    assert_eq!(all[0], record);
    assert_eq!(all[1].title, "Untitled");
    assert_eq!(all[1].reading(), Reading::Unread);
    assert_eq!(all[0].reading(), Reading::Begun);
    assert_eq!(search(&storage, "ann TALES"), [record]);
    assert!(search(&storage, "nothing").is_empty());
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
fn status_changes_and_removals_are_logged_and_read_with_the_catalog() {
    let storage = MemoryStorage::new();
    let books: Vec<Record> = (0..5)
        .map(|n| Record::of(&format!("books/{n}.txt"), &Book::default()))
        .collect();
    write(&storage, &books).unwrap();
    note(&storage, "books/2.txt", Status::new(Some(30), false, true)).unwrap();
    note(&storage, "books/2.txt", Status::new(Some(55), true, true)).unwrap();
    note(&storage, "books/4.txt", Status::new(Some(100), false, true)).unwrap();
    let found = |show| found(&storage, &Query::new().show(show));
    assert_eq!(found(Show::Reading), 1);
    assert_eq!(found(Show::Finished), 1);
    assert_eq!(found(Show::Favourites), 1);
    assert_eq!(found(Show::Unread), 3);
    // Read by where their lines start, the log read once: with their
    // changes.
    let mut at = Vec::new();
    each_at(&storage, &mut |offset, _| at.push(offset));
    let page = read_page(&storage, &at);
    assert_eq!(page[2].as_ref().unwrap().status.progress, Some(55));
    assert!(page[2].as_ref().unwrap().status.favourite);
    // Removed: left out at once, by every reader, its status no
    // longer counting; noted again, it counts.
    forget(&storage, "books/4.txt").unwrap();
    assert_eq!(found(Show::All), 4);
    assert_eq!(found(Show::Finished), 0);
    assert!(read_page(&storage, &at)[4].is_none());
    assert!(changes(&storage).removed(fnv1a64("books/4.txt")));
    // The log grows long only by lines: the kernel folds it then.
    assert!(!log_is_long(&storage));
    for n in 0..MAX_LOG {
        let status = Status::new(Some((n % 100) as u8), false, true);
        note(&storage, &format!("books/{}.txt", n % 4), status).unwrap();
    }
    assert!(log_is_long(&storage));
    // Nothing is noted without a catalog: the walk takes the details.
    let bare = MemoryStorage::new();
    note(&bare, "books/1.txt", Status::default()).unwrap();
    assert!(!bare.exists(&log_path()));
}

#[test]
fn queries_find_filter_sort_and_page() {
    let storage = MemoryStorage::new();
    let mut records = Vec::new();
    for n in 0..12u32 {
        let book = Book::default()
            .title(Some(format!(
                "{} Tale {n:02}",
                if n % 2 == 0 { "a" } else { "B" }
            )))
            .author(Some(String::from(if n < 6 { "Ann" } else { "Bob" })))
            .series(Some(String::from("Tales")))
            .series_index(Some(format!("{}", 12 - n)))
            .added(Some(100 + n));
        let folder = if n % 3 == 0 { "books/Old" } else { "books" };
        records.push(Record::of(&format!("{folder}/{n}.epub"), &book));
    }
    write(&storage, &records).unwrap();
    let all = |q: &Query| query(&storage, q, None, 100);
    assert_eq!(all(&Query::new().author("Bob")).len(), 6);
    assert_eq!(all(&Query::new().folder("books/Old")).len(), 4);
    assert_eq!(all(&Query::new().folder("books/Ol")).len(), 0);
    assert_eq!(all(&Query::new().added_since(110)).len(), 2);
    assert_eq!(all(&Query::new().words("bob TALE")).len(), 6);
    // A series by its numbers; added last first.
    let series = all(&Query::new().sort(Sort::Series));
    assert_eq!(series[0].number, "1");
    assert_eq!(series[11].number, "12");
    let added = all(&Query::new().sort(Sort::Added));
    assert_eq!(added[0].added, Some(111));
    // Pages of 5 make up the whole, by title.
    let by_title = all(&Query::new());
    let mut seen = Vec::new();
    let mut last: Option<Record> = None;
    loop {
        let page = query(&storage, &Query::new(), last.as_ref(), 5);
        if page.is_empty() {
            break;
        }
        last = page.last().cloned();
        seen.extend(page);
    }
    assert_eq!(seen, by_title);
    assert_eq!(found(&storage, &Query::new().author("Ann")), 6);
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

#[test]
fn searches_page_by_title_holding_a_page() {
    let storage = MemoryStorage::new();
    // Titles in no order, two alike but for case and path.
    let records: Vec<Record> = (0..30)
        .map(|i| {
            let book = Book::default().title(Some(format!(
                "{}Tale {:02}",
                if i % 2 == 0 { "a" } else { "A" },
                (i * 7) % 30
            )));
            Record::of(&format!("books/{i}.epub"), &book)
        })
        .collect();
    write(&storage, &records).unwrap();
    let by_title = Query::new();
    let mut sorted = records.clone();
    sorted.sort_by_key(|r| by_title.key(r));
    let mut seen = Vec::new();
    let mut last: Option<Record> = None;
    loop {
        let page = search_after(&storage, "tale", last.as_ref(), 7);
        if page.is_empty() {
            break;
        }
        assert!(page.len() <= 7);
        last = page.last().cloned();
        seen.extend(page);
    }
    assert_eq!(seen, sorted);
    assert_eq!(count(&storage, &|r| r.title.ends_with('5')), 3);
    assert_eq!(search(&storage, "tale").len(), 30);
    assert!(search_after(&storage, "  ", None, 5).is_empty());
}
