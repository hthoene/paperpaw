use super::*;
use crate::store::PageStat;
use crate::testing::Faulty;
use paperpaw_sdk::MemoryStorage;

pub(super) const DIR: &str = ".paperpaw/plugins/stats";

pub(super) fn md5(n: u8) -> String {
    format!("{n:032x}")
}

pub(super) fn book(id: u16, md5: &str) -> Book {
    Book {
        id,
        title: format!("Ours {id}"),
        authors: String::from("N/A"),
        md5: md5.to_string(),
        pages: 100,
        language: String::from("de"),
        series: String::from("N/A"),
        last_open: 10,
        path: format!("books/{id}.epub"),
        finished: 0,
    }
}

pub(super) fn stay(book: u16, page: u32, start: u32, duration: u16) -> PageStat {
    PageStat {
        book,
        page,
        start,
        duration,
        pages: 100,
    }
}

pub(super) fn log(storage: &dyn Storage) -> Vec<(u16, u32, u32, u16)> {
    let mut out = Vec::new();
    store::scan(storage, |s| out.push((s.book, s.page, s.start, s.duration))).unwrap();
    out
}

/// A KOReader file from another device: its own ids and names.
pub(super) fn other_device(
    storage: &dyn Storage,
    path: &str,
    books: &[(i64, &str, &str)],
    stays: &[(i64, u32, u32, u32)],
) {
    let ko: Vec<KoBook> = books
        .iter()
        .map(|(id, title, md5)| KoBook {
            id: *id,
            title: title.to_string(),
            authors: String::from("Autorin"),
            pages: 200,
            md5: md5.to_string(),
            series: String::from("N/A"),
            language: String::from("de"),
            last_open: 99,
            ..KoBook::default()
        })
        .collect();
    let mut records = Vec::new();
    let mut sorted = stays.to_vec();
    sorted.sort_by_key(|s| s.2);
    for (book, page, start, duration) in sorted {
        records.extend_from_slice(&stat_record([book as u32, page, start, duration, 200]));
    }
    storage.write("stays.tmp", &records).unwrap();
    koreader::write(storage, path, &ko, "stays.tmp", "tmp").unwrap();
    storage.remove("stays.tmp").unwrap();
}

/// Two books here and three stays.
pub(super) fn this_device(storage: &dyn Storage) {
    store::save_books(storage, &[book(1, &md5(0xa)), book(2, &md5(0xb))]).unwrap();
    store::append(storage, stay(1, 5, 1000, 30)).unwrap();
    store::append(storage, stay(1, 6, 1040, 20)).unwrap();
    store::append(storage, stay(2, 1, 1100, 10)).unwrap();
}

/// The files of the statistics here, to see they did not change.
pub(super) fn files(storage: &MemoryStorage) -> [Option<Vec<u8>>; 2] {
    ["books.ini", "pages.bin"].map(|name| storage.get(&format!("{DIR}/{name}")))
}

/// What is in the statistics' folder.
pub(super) fn folder(storage: &dyn Storage) -> Vec<String> {
    let mut names = Vec::new();
    storage
        .list(DIR, &mut |entry| names.push(entry.name.to_string()))
        .unwrap();
    names.sort();
    names
}

#[test]
fn merges_like_koreader() {
    let storage = MemoryStorage::new();
    this_device(&storage);
    // The other device: book A as its id 7 (a stay the same, one
    // longer, one new), and a book unknown here.
    other_device(
        &storage,
        "income.db",
        &[(7, "Their title", &md5(0xa)), (8, "Neu", &md5(0xc))],
        &[
            (7, 5, 1000, 25),
            (7, 6, 1040, 50),
            (7, 9, 2000, 40),
            (8, 3, 3000, 60),
        ],
    );
    let outcome = merge(&storage, Some("income.db"), None, Some("out.db"), DIR).unwrap();
    assert_eq!(
        outcome,
        Outcome {
            books: 3,
            stays: 5,
            kept: false,
            unclear: false
        }
    );
    let books = store::load_books(&storage).unwrap();
    assert_eq!(
        books.iter().map(|b| b.md5.clone()).collect::<Vec<_>>(),
        [md5(0xa), md5(0xb), md5(0xc)]
    );
    // Named as KOReader names them, the path here kept.
    assert_eq!(
        (books[0].title.as_str(), books[0].authors.as_str()),
        ("Their title", "Autorin")
    );
    assert_eq!(books[0].path, "books/1.epub");
    assert_eq!(books[2].id, 3);
    // In time order; the longer duration of a stay wins.
    assert_eq!(
        log(&storage),
        [
            (1, 5, 1000, 30),
            (1, 6, 1040, 50),
            (2, 1, 1100, 10),
            (1, 9, 2000, 40),
            (3, 3, 3000, 60)
        ]
    );
    // The file to upload reads back with the same.
    let db = Database::open(&storage, "out.db").unwrap();
    assert_eq!(db.user_version, koreader::SCHEMA_VERSION);
    let ko = koreader::books(&db).unwrap();
    assert_eq!(ko.len(), 3);
    assert_eq!(ko[0].total_read_time, 120);
    let mut stays = 0;
    koreader::stats(&db, |_| stays += 1).unwrap();
    assert_eq!(stays, 5);
    // Only what was there before, and the upload.
    assert_eq!(folder(&storage), ["books.ini", "pages.bin"]);
}

/// What was deleted on the other side since the last upload goes here
/// too; what was lost here comes back from there.
#[test]
fn deletions_there_come_here_and_losses_here_come_back() {
    let storage = MemoryStorage::new();
    store::save_books(&storage, &[book(1, &md5(0xa)), book(2, &md5(0xb))]).unwrap();
    store::append(&storage, stay(1, 1, 100, 10)).unwrap();
    store::append(&storage, stay(1, 2, 200, 10)).unwrap();
    store::append(&storage, stay(2, 1, 300, 10)).unwrap();
    // The last upload had both books and four stays; the stay at 250
    // was lost here since.
    other_device(
        &storage,
        "cached.db",
        &[(1, "Ours 1", &md5(0xa)), (2, "Ours 2", &md5(0xb))],
        &[
            (1, 1, 100, 10),
            (1, 2, 200, 10),
            (1, 3, 250, 10),
            (2, 1, 300, 10),
        ],
    );
    // There, book B and the stay at 200 were deleted since.
    other_device(
        &storage,
        "income.db",
        &[(4, "Ours 1", &md5(0xa))],
        &[(4, 1, 100, 10), (4, 3, 250, 10)],
    );
    let outcome = merge(&storage, Some("income.db"), Some("cached.db"), None, DIR).unwrap();
    assert_eq!(
        outcome,
        Outcome {
            books: 1,
            stays: 2,
            kept: false,
            unclear: false
        }
    );
    assert_eq!(log(&storage), [(1, 1, 100, 10), (1, 3, 250, 10)]);
    assert_eq!(store::load_books(&storage).unwrap().len(), 1);
}

/// A book lost here (`books.ini` rewritten without it) that the last
/// upload had is not taken for deleted: it comes back with its stays,
/// and the upload keeps it, so KOReader does not delete it either.
#[test]
fn a_book_lost_here_comes_back() {
    let storage = MemoryStorage::new();
    store::save_books(&storage, &[book(1, &md5(0xa))]).unwrap();
    store::append(&storage, stay(1, 1, 100, 10)).unwrap();
    let both = [(1, "Ours 1", md5(0xa)), (2, "Ours 2", md5(0xb))];
    let both: Vec<(i64, &str, &str)> = both.iter().map(|(i, t, m)| (*i, *t, m.as_str())).collect();
    let stays = [(1, 1, 100, 10), (2, 1, 300, 10), (2, 2, 400, 20)];
    other_device(&storage, "cached.db", &both, &stays);
    other_device(&storage, "income.db", &both, &stays);
    let outcome = merge(
        &storage,
        Some("income.db"),
        Some("cached.db"),
        Some("out.db"),
        DIR,
    )
    .unwrap();
    assert_eq!(
        outcome,
        Outcome {
            books: 2,
            stays: 3,
            kept: false,
            unclear: false
        }
    );
    assert_eq!(
        log(&storage),
        [(1, 1, 100, 10), (2, 1, 300, 10), (2, 2, 400, 20)]
    );
    let db = Database::open(&storage, "out.db").unwrap();
    assert_eq!(koreader::books(&db).unwrap().len(), 2);
}

/// A downloaded file that is no statistics database (a web page with
/// status 200), or one cut short: the merge fails and nothing here
/// changes, nor is anything written to upload.
#[test]
fn an_unreadable_income_changes_nothing() {
    let storage = MemoryStorage::new();
    this_device(&storage);
    other_device(
        &storage,
        "good.db",
        &[(1, "Neu", &md5(0xc))],
        &[(1, 1, 50, 10)],
    );
    let good = storage.get("good.db").unwrap();
    let before = files(&storage);
    let page = b"<html><body>Log in</body></html>".to_vec();
    // The first page (with the schema) and no more.
    let cut = good[..4096 + 100].to_vec();
    for (what, data) in [("page", page), ("cut", cut), ("empty", Vec::new())] {
        storage.write("income.db", &data).unwrap();
        let result = merge(&storage, Some("income.db"), None, Some("out.db"), DIR);
        assert!(
            matches!(result, Err(Failure::Income(_))),
            "{what}: {result:?}"
        );
        assert_eq!(files(&storage), before, "{what}");
        assert!(!storage.exists("out.db"), "{what}");
        assert_eq!(folder(&storage), ["books.ini", "pages.bin"], "{what}");
    }
}

/// Statistics here that cannot be read whole are not merged: what was
/// not read would look deleted.
#[test]
fn unreadable_local_statistics_change_nothing() {
    // `books.ini` saved as Latin-1.
    let storage = MemoryStorage::new();
    this_device(&storage);
    other_device(
        &storage,
        "income.db",
        &[(1, "Neu", &md5(0xc))],
        &[(1, 1, 50, 10)],
    );
    storage
        .write(
            &format!("{DIR}/books.ini"),
            b"[book.1]\ntitle = M\xfcller\n",
        )
        .unwrap();
    let before = files(&storage);
    let result = merge(&storage, Some("income.db"), None, Some("out.db"), DIR);
    assert!(matches!(result, Err(Failure::Here(_))), "{result:?}");
    assert_eq!(files(&storage), before);
    assert!(!storage.exists("out.db"));
    // The log cannot be read to the end.
    let storage = Faulty::new(MemoryStorage::new());
    this_device(&storage);
    for i in 0..100 {
        store::append(&storage, stay(2, i + 2, 2000 + i * 10, 10)).unwrap();
    }
    other_device(
        &storage,
        "income.db",
        &[(1, "Neu", &md5(0xc))],
        &[(1, 1, 50, 10)],
    );
    storage.break_at(&format!("{DIR}/pages.bin"), 16 * 70);
    let before = files(&storage.inner);
    let result = merge(&storage, Some("income.db"), None, Some("out.db"), DIR);
    assert_eq!(result, Err(Failure::Here(Error::Storage(StorageError::Io))));
    assert_eq!(files(&storage.inner), before);
    assert!(!storage.exists("out.db"));
}

/// A last upload that cannot be read tells nothing: nothing counts as
/// deleted there.
#[test]
fn an_unreadable_last_upload_deletes_nothing() {
    let storage = MemoryStorage::new();
    this_device(&storage);
    other_device(
        &storage,
        "income.db",
        &[(1, "Neu", &md5(0xc))],
        &[(1, 1, 50, 10)],
    );
    storage.write("cached.db", b"not a database").unwrap();
    let outcome = merge(&storage, Some("income.db"), Some("cached.db"), None, DIR).unwrap();
    assert_eq!(
        outcome,
        Outcome {
            books: 3,
            stays: 4,
            kept: false,
            unclear: false
        }
    );
}

/// KOReader's books without a file's MD5 are told apart by title and
/// authors, as KOReader does, and stay so in later merges.
#[test]
fn books_without_md5_stay_apart() {
    let storage = MemoryStorage::new();
    other_device(
        &storage,
        "income.db",
        &[(1, "Erstes", ""), (2, "Zweites", "")],
        &[(1, 1, 100, 10), (2, 1, 200, 20)],
    );
    let outcome = merge(&storage, Some("income.db"), None, Some("out.db"), DIR).unwrap();
    assert_eq!(
        outcome,
        Outcome {
            books: 2,
            stays: 2,
            kept: false,
            unclear: false
        }
    );
    let books = store::load_books(&storage).unwrap();
    assert_eq!(
        books.iter().map(|b| b.title.as_str()).collect::<Vec<_>>(),
        ["Erstes", "Zweites"]
    );
    assert_eq!(log(&storage), [(1, 1, 100, 10), (2, 1, 200, 20)]);
    // Again, with what was uploaded as the last upload.
    storage.rename("out.db", "cached.db").unwrap();
    let again = merge(&storage, Some("income.db"), Some("cached.db"), None, DIR).unwrap();
    assert_eq!(again, outcome);
    assert_eq!(log(&storage), [(1, 1, 100, 10), (2, 1, 200, 20)]);
}

/// A card that fills up at any point of a merge: the error comes back,
/// no working file is left, and the statistics are as they were (or
/// merged whole, once the log was swapped).
#[test]
fn working_files_go_after_an_error() {
    let make = || {
        let storage = Faulty::new(MemoryStorage::new());
        this_device(&storage);
        other_device(
            &storage,
            "income.db",
            &[(7, "Neu", &md5(0xc))],
            &[(7, 1, 50, 10), (7, 2, 60, 10)],
        );
        storage
    };
    let mut failed = 0;
    for room in 0..200 {
        let storage = make();
        let before = files(&storage.inner);
        storage.fill_after(room);
        let result = merge(&storage, Some("income.db"), None, Some("out.db"), DIR);
        if result.is_ok() {
            break;
        }
        failed += 1;
        assert_eq!(folder(&storage), ["books.ini", "pages.bin"], "room {room}");
        assert!(!storage.exists("out.db"), "room {room}");
        let after = files(&storage.inner);
        let merged = log(&storage).len() == 5;
        assert!(after == before || merged, "room {room}");
    }
    assert!(failed > 10, "{failed}");
}

/// The file there replaced by an older backup: what the last upload
/// had and the file lacks was not deleted there, so nothing is deleted
/// here; and it is said. An empty file (a fresh device's, or all
/// statistics reset there) deletes nothing either: the user is asked.
#[test]
fn a_replaced_file_there_deletes_nothing_here() {
    let both = [(1, "Ours 1", md5(0xa)), (2, "Ours 2", md5(0xb))];
    let both: Vec<(i64, &str, &str)> = both.iter().map(|(i, t, m)| (*i, *t, m.as_str())).collect();
    let uploaded = [(1, 5, 1000, 30), (1, 6, 1040, 20), (2, 1, 1100, 10)];
    // Empty, and a backup with one stay of the three.
    type Books<'a> = &'a [(i64, &'a str, &'a str)];
    type Stays<'a> = &'a [(i64, u32, u32, u32)];
    let replaced: [(Books<'_>, Stays<'_>, bool); 2] =
        [(&[], &[], false), (&both[..1], &uploaded[..1], true)];
    for (books, stays, kept) in replaced {
        let storage = MemoryStorage::new();
        this_device(&storage);
        other_device(&storage, "cached.db", &both, &uploaded);
        // Read here since the last upload, never uploaded.
        store::append(&storage, stay(1, 7, 5000, 15)).unwrap();
        other_device(&storage, "income.db", books, stays);
        let outcome = merge(
            &storage,
            Some("income.db"),
            Some("cached.db"),
            Some("out.db"),
            DIR,
        )
        .unwrap();
        assert_eq!(
            outcome,
            Outcome {
                books: 2,
                stays: 4,
                kept,
                unclear: !kept
            },
            "{books:?}"
        );
        assert_eq!(
            log(&storage),
            [
                (1, 5, 1000, 30),
                (1, 6, 1040, 20),
                (2, 1, 1100, 10),
                (1, 7, 5000, 15)
            ]
        );
    }
}

/// Books of this device and the last upload, the same: A read before
/// and after B, a big book; B read last. The file there as KOReader
/// uploads it after the user deleted B's statistics, read once more
/// there: B goes here too, though it was most of the last upload.
#[test]
fn a_big_book_deleted_there_goes_here() {
    let storage = MemoryStorage::new();
    store::save_books(&storage, &[book(1, &md5(0xa)), book(2, &md5(0xb))]).unwrap();
    let mut uploaded = Vec::from([(1, 1, 100, 10), (1, 2, 200, 10)]);
    uploaded.extend((0..10).map(|n| (2, n + 1, 1000 + n * 20, 10)));
    uploaded.push((1, 3, 9000, 10));
    for &(book, page, start, duration) in &uploaded {
        store::append(&storage, stay(book as u16, page, start, duration as u16)).unwrap();
    }
    let both = [(1, "Ours 1", md5(0xa)), (2, "Ours 2", md5(0xb))];
    let both: Vec<(i64, &str, &str)> = both.iter().map(|(i, t, m)| (*i, *t, m.as_str())).collect();
    other_device(&storage, "cached.db", &both, &uploaded);
    let a = [
        (4, 1, 100, 10),
        (4, 2, 200, 10),
        (4, 3, 9000, 10),
        (4, 4, 9500, 10),
    ];
    other_device(&storage, "income.db", &[(4, "Ours 1", &md5(0xa))], &a);
    let outcome = merge(&storage, Some("income.db"), Some("cached.db"), None, DIR).unwrap();
    assert_eq!(
        outcome,
        Outcome {
            books: 1,
            stays: 4,
            kept: false,
            unclear: false
        }
    );
    assert_eq!(
        log(&storage),
        [
            (1, 1, 100, 10),
            (1, 2, 200, 10),
            (1, 3, 9000, 10),
            (1, 4, 9500, 10)
        ]
    );
}

/// What the file there lacks cannot be told deleted there from a
/// file replaced: the book read last gone (a backup older than it, or
/// deleted there), or the stays of a book read before gone while
/// newer ones are there (all statistics reset there and read since,
/// or another device's file). Nothing goes, the user is asked; and as
/// the user decides, it goes or stays.
#[test]
fn what_cannot_be_told_is_kept_until_decided() {
    let both = [(1, "Ours 1", md5(0xa)), (2, "Ours 2", md5(0xb))];
    let both: Vec<(i64, &str, &str)> = both.iter().map(|(i, t, m)| (*i, *t, m.as_str())).collect();
    let mut uploaded = Vec::from([(1, 1, 100, 10), (1, 2, 200, 10)]);
    uploaded.extend((0..10).map(|n| (2, n + 1, 1000 + n * 20, 10)));
    // B gone, and only a stay newer than all there.
    let a = [(4, 1, 100, 10), (4, 2, 200, 10)];
    let theirs: [&[(i64, u32, u32, u32)]; 2] = [&a, &[(4, 9, 9000, 10)]];
    for stays in theirs {
        for decided in [None, Some(true), Some(false)] {
            let storage = MemoryStorage::new();
            store::save_books(&storage, &[book(1, &md5(0xa)), book(2, &md5(0xb))]).unwrap();
            for &(book, page, start, duration) in &uploaded {
                store::append(&storage, stay(book as u16, page, start, duration as u16)).unwrap();
            }
            other_device(&storage, "cached.db", &both, &uploaded);
            other_device(&storage, "income.db", &[(4, "Ours 1", &md5(0xa))], stays);
            let outcome = merge_as(
                &storage,
                Some("income.db"),
                Some("cached.db"),
                None,
                DIR,
                decided,
            )
            .unwrap();
            let all = uploaded.len() + usize::from(stays.len() == 1);
            let (books, kept_stays) = match decided {
                Some(true) => (1, stays.len()),
                _ => (2, all),
            };
            let what = format!("{} {decided:?}", stays.len());
            assert_eq!(
                outcome,
                Outcome {
                    books,
                    stays: kept_stays,
                    kept: false,
                    unclear: decided.is_none()
                },
                "{what}"
            );
            assert_eq!(log(&storage).len(), kept_stays, "{what}");
        }
    }
}

/// A book deleted there keeps the stays read here since the last
/// upload, and so stays; one deleted there without such goes.
#[test]
fn stays_not_uploaded_keep_a_book_deleted_there() {
    let storage = MemoryStorage::new();
    store::save_books(
        &storage,
        &[book(1, &md5(0xa)), book(2, &md5(0xb)), book(3, &md5(0xc))],
    )
    .unwrap();
    for page in 1..=4 {
        store::append(&storage, stay(1, page, 100 * page, 10)).unwrap();
    }
    store::append(&storage, stay(2, 1, 600, 10)).unwrap();
    store::append(&storage, stay(3, 1, 700, 10)).unwrap();
    let books = [
        (1, "Ours 1", md5(0xa)),
        (2, "Ours 2", md5(0xb)),
        (3, "Ours 3", md5(0xc)),
    ];
    let books: Vec<(i64, &str, &str)> =
        books.iter().map(|(i, t, m)| (*i, *t, m.as_str())).collect();
    let a = [
        (1, 1, 100, 10),
        (1, 2, 200, 10),
        (1, 3, 300, 10),
        (1, 4, 400, 10),
    ];
    let mut uploaded = a.to_vec();
    uploaded.extend([(2, 1, 600, 10), (3, 1, 700, 10)]);
    other_device(&storage, "cached.db", &books, &uploaded);
    // Read here since.
    store::append(&storage, stay(2, 2, 800, 10)).unwrap();
    // There, books B and C were deleted since.
    other_device(&storage, "income.db", &books[..1], &a);
    let outcome = merge(&storage, Some("income.db"), Some("cached.db"), None, DIR).unwrap();
    assert_eq!(
        outcome,
        Outcome {
            books: 2,
            stays: 5,
            kept: false,
            unclear: false
        }
    );
    assert_eq!(
        log(&storage),
        [
            (1, 1, 100, 10),
            (1, 2, 200, 10),
            (1, 3, 300, 10),
            (1, 4, 400, 10),
            (2, 2, 800, 10)
        ]
    );
    let ids: Vec<u16> = store::load_books(&storage)
        .unwrap()
        .iter()
        .map(|b| b.id)
        .collect();
    assert_eq!(ids, [1, 2]);
}

/// `bytes` in the file at `path` replaced by `with` (as long).
fn patch(storage: &MemoryStorage, path: &str, bytes: &[u8], with: &[u8]) {
    let mut data = storage.get(path).unwrap();
    let at = data.windows(bytes.len()).position(|w| w == bytes).unwrap();
    data[at..at + with.len()].copy_from_slice(with);
    storage.write(path, &data).unwrap();
}

/// A file there whose tables lack a column needed (renamed in a later
/// version of KOReader) cannot be read: nothing is merged, rather than
/// books and stays taken with what is missing as empty.
#[test]
fn a_file_there_without_a_column_cannot_be_read() {
    let columns: [(&[u8], &[u8]); 4] = [
        (b"md5 text", b"md6 text"),
        (b"title text", b"tytle text"),
        (b"id_book     integer", b"book_id     integer"),
        (b"start_time  integer", b"start       integer"),
    ];
    for (column, renamed) in columns {
        let storage = MemoryStorage::new();
        this_device(&storage);
        other_device(
            &storage,
            "income.db",
            &[(1, "Neu", &md5(0xc))],
            &[(1, 1, 50, 10)],
        );
        patch(&storage, "income.db", column, renamed);
        let before = files(&storage);
        let result = merge(&storage, Some("income.db"), None, Some("out.db"), DIR);
        assert!(matches!(result, Err(Failure::Income(_))), "{result:?}");
        assert_eq!(files(&storage), before);
        assert!(!storage.exists("out.db"));
    }
}

/// A file there of another version of KOReader's statistics is
/// merged, but what it lacks is not taken for deleted there.
#[test]
fn a_file_there_of_another_version_deletes_nothing() {
    let storage = MemoryStorage::new();
    this_device(&storage);
    let both = [(1, "Ours 1", md5(0xa)), (2, "Ours 2", md5(0xb))];
    let both: Vec<(i64, &str, &str)> = both.iter().map(|(i, t, m)| (*i, *t, m.as_str())).collect();
    let stays = [(1, 5, 1000, 30), (1, 6, 1040, 20), (2, 1, 1100, 10)];
    other_device(&storage, "cached.db", &both, &stays);
    other_device(&storage, "income.db", &both, &stays[..2]);
    // The version (`PRAGMA user_version`) at byte 60.
    let mut data = storage.get("income.db").unwrap();
    data[60..64].copy_from_slice(&20_991_231u32.to_be_bytes());
    storage.write("income.db", &data).unwrap();
    let outcome = merge(&storage, Some("income.db"), Some("cached.db"), None, DIR).unwrap();
    assert_eq!(
        outcome,
        Outcome {
            books: 2,
            stays: 3,
            kept: true,
            unclear: false
        }
    );
}

/// Stays of a book whose entry was lost here are kept by a merge, and
/// a book added takes an id above theirs.
#[test]
fn stays_without_a_book_are_kept() {
    let storage = MemoryStorage::new();
    this_device(&storage);
    store::append(&storage, stay(5, 1, 1200, 10)).unwrap();
    other_device(
        &storage,
        "income.db",
        &[(1, "Neu", &md5(0xc))],
        &[(1, 1, 50, 10)],
    );
    let outcome = merge(&storage, Some("income.db"), None, Some("out.db"), DIR).unwrap();
    assert_eq!(outcome.stays, 5);
    assert!(log(&storage).contains(&(5, 1, 1200, 10)));
    let books = store::load_books(&storage).unwrap();
    assert_eq!(books.last().map(|b| b.id), Some(6));
    assert!(log(&storage).contains(&(6, 1, 50, 10)));
    // The upload has no stays of a book it has no row of.
    let db = Database::open(&storage, "out.db").unwrap();
    let mut stays = 0;
    koreader::stats(&db, |_| stays += 1).unwrap();
    assert_eq!(stays, 4);
}

/// Merges a real KOReader file into made-up statistics and writes the
/// result for SQLite to check:
/// `PAPERPAW_KO_IN=… PAPERPAW_KO_OUT=… cargo test -p paperpaw-plugin-stats -- --ignored`.
#[test]
#[ignore]
fn real_koreader_file() {
    extern crate std;
    let (Ok(input), Ok(output)) = (
        std::env::var("PAPERPAW_KO_IN"),
        std::env::var("PAPERPAW_KO_OUT"),
    ) else {
        return;
    };
    let storage = MemoryStorage::new().with("income.db", std::fs::read(input).unwrap());
    store::save_books(&storage, &[book(1, &md5(0xd))]).unwrap();
    store::append(&storage, stay(1, 1, 1_700_000_000, 30)).unwrap();
    let outcome = merge(&storage, Some("income.db"), None, Some("out.db"), DIR).unwrap();
    std::println!("{outcome:?}");
    std::fs::write(output, storage.get("out.db").unwrap()).unwrap();
}
