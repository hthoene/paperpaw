//! `make_dir`, `remove_tree` and `move`: the effects, the limits (64
//! removals, a folder that needs several calls), and the statuses.
use super::*;
use paperpaw_native::host_glue::{
    ERR_BUSY, ERR_DENIED, ERR_FULL, ERR_INVALID, ERR_IO, ERR_NOT_FOUND, ERR_STATE,
};

fn make_dir(host: &Rc<TestHost>, root: u32, p: &str) -> Result<(), i32> {
    call(host, 4, path(root, p)).map(|reply| assert!(reply.is_empty(), "no reply record"))
}

fn do_move(host: &Rc<TestHost>, from: (u32, &str), to: (u32, &str), flags: u32) -> Result<(), i32> {
    let moved = call(host, 6, move_record(from.0, from.1, to.0, to.1, flags))
        .map(|reply| assert!(reply.is_empty()));
    // The library's job (a move of the user's files starts one) runs out.
    host.finish_job();
    moved
}

/// One `remove_tree` call: (removed, done, next).
fn remove(host: &Rc<TestHost>, root: u32, p: &str, cursor: u64) -> Result<(u32, bool, u64), i32> {
    let reply = call(host, 5, list_record(root, p, cursor))?;
    match wire::record_decode_one(&reply).unwrap() {
        Record::ServiceFsProgress(p) => Ok((p.removed, p.flags == 1, p.next)),
        other => panic!("{other:?}"),
    }
}

fn remove_all(host: &Rc<TestHost>, root: u32, p: &str) -> usize {
    let (mut cursor, mut calls) = (0, 0);
    loop {
        let (removed, done, next) = remove(host, root, p, cursor).unwrap();
        assert!(removed <= 64);
        calls += 1;
        assert!(calls < 100);
        if done {
            assert_eq!(next, 0);
            return calls;
        }
        cursor = next;
    }
}

#[test]
fn make_dir_creates_the_folder_and_its_parents() {
    let (card, host) = fixture();
    assert_eq!(make_dir(&host, 1, "A/B/C"), Ok(()));
    assert!(card.is_dir("A") && card.is_dir("A/B") && card.is_dir("A/B/C"));
    assert_eq!(
        make_dir(&host, 1, "A/B/C"),
        Ok(()),
        "an existing folder is fine"
    );
    assert_eq!(make_dir(&host, 1, ""), Ok(()), "the root always exists");
    assert_eq!(make_dir(&host, 0, "cache/x"), Ok(()));
    assert!(card.is_dir(&std::format!("{DATA}/cache/x")));
    assert_eq!(make_dir(&host, 0, ""), Ok(()));
    card.file("A/file", 1);
    assert_eq!(
        make_dir(&host, 1, "A/file"),
        Err(ERR_INVALID),
        "a file at the path"
    );
    assert_eq!(
        make_dir(&host, 1, "A/file/sub"),
        Err(ERR_INVALID),
        "a file at a parent"
    );
    assert_eq!(
        make_dir(&host, 2, "x"),
        Err(ERR_DENIED),
        "root 2 is read only"
    );
    assert_eq!(make_dir(&host, 2, ""), Err(ERR_DENIED));
}

#[test]
fn make_dir_keeps_the_reserve_and_reports_a_full_card() {
    let (card, host) = fixture();
    card.free.set(Some(1000));
    assert_eq!(make_dir(&host, 1, "New"), Err(ERR_FULL));
    assert!(!card.has("New"), "a refused call changes nothing");
    card.free.set(Some(100 << 20));
    card.full.set(true);
    assert_eq!(make_dir(&host, 1, "New"), Err(ERR_FULL));
    card.full.set(false);
    card.gone.set(true);
    assert_eq!(make_dir(&host, 1, "New"), Err(ERR_IO));
}

#[test]
fn make_dir_of_a_root_needs_the_card() {
    let (card, host) = fixture();
    card.gone.set(true);
    for root in [0, 1] {
        assert_eq!(make_dir(&host, root, ""), Err(ERR_IO), "root {root}");
    }
    card.gone.set(false);
    for root in [0, 1] {
        assert_eq!(make_dir(&host, root, ""), Ok(()), "root {root}");
    }
}

#[test]
fn the_hosts_received_folder_is_not_made_in() {
    let (card, host) = fixture();
    assert_eq!(make_dir(&host, 0, ".received"), Err(ERR_DENIED));
    // As the FAT driver folds it (`ı` is `I`): the same folder.
    assert_eq!(make_dir(&host, 0, ".receıved"), Err(ERR_DENIED));
    assert_eq!(make_dir(&host, 0, ".RECEıVED"), Err(ERR_DENIED));
    assert_eq!(make_dir(&host, 0, ".RECEIVED/x"), Err(ERR_DENIED));
    card.file(&std::format!("{DATA}/.received/body"), 3);
    card.file(&std::format!("{DATA}/own.txt"), 3);
    // Moves into it, or of it whole, are denied; out of it is fine.
    assert_eq!(
        do_move(&host, (0, "own.txt"), (0, ".received/own.txt"), 0),
        Err(ERR_DENIED)
    );
    assert_eq!(
        do_move(&host, (0, ".received"), (0, "kept"), 0),
        Err(ERR_DENIED)
    );
    assert_eq!(
        do_move(&host, (0, ".received/body"), (0, "mine.bin"), 0),
        Ok(())
    );
    assert!(card.has(&std::format!("{DATA}/mine.bin")));
    // The same name in root 1 is an ordinary folder.
    assert_eq!(make_dir(&host, 1, ".received"), Ok(()));
}

#[test]
fn remove_tree_removes_at_most_64_entries_a_call() {
    for (files, calls) in [
        (10usize, 1usize),
        (62, 1),
        (63, 1),
        (64, 2),
        (65, 2),
        (130, 3),
    ] {
        let (card, host) = fixture();
        for i in 0..files {
            card.file(&std::format!("T/f{i:03}"), 1);
        }
        card.file("Keep/k", 1);
        // The files and the folder itself are `files + 1` removals: 64 at most a call.
        assert_eq!(remove_all(&host, 1, "T"), calls, "{files} files");
        assert!(!card.has("T") && card.has("Keep/k"));
    }
}

#[test]
fn remove_tree_goes_down_and_back_with_the_place() {
    let (card, host) = fixture();
    for d in 0..5 {
        for f in 0..30 {
            card.file(&std::format!("T/d{d}/s/f{f:02}"), 1);
        }
    }
    card.file("T/top", 1);
    card.file("T2/x", 1);
    let (removed, done, next) = remove(&host, 1, "T", 0).unwrap();
    assert_eq!((removed, done), (64, false));
    assert_ne!(next, 0);
    // The same path with the place goes on; the storage keeps the rest whole.
    let calls = remove_all(&host, 1, "T");
    assert!(calls >= 2);
    assert!(
        card.paths()
            .iter()
            .all(|p| !p.starts_with("T/") && p != "T")
    );
    assert!(card.has("T2/x"));
}

#[test]
fn remove_tree_of_a_missing_path_is_done_and_a_file_is_one_entry() {
    let (card, host) = fixture();
    assert_eq!(remove(&host, 1, "Nope/x", 0), Ok((0, true, 0)));
    card.file("f.txt", 1);
    assert_eq!(remove(&host, 1, "f.txt", 0), Ok((1, true, 0)));
    assert!(!card.has("f.txt"));
}

#[test]
fn remove_tree_root_rules() {
    let (card, host) = fixture();
    card.file("Keep/x", 1);
    assert_eq!(
        remove(&host, 1, "", 0),
        Err(ERR_INVALID),
        "never the whole card"
    );
    assert_eq!(remove(&host, 2, "data", 0), Err(ERR_DENIED));
    assert_eq!(remove(&host, 2, "", 0), Err(ERR_DENIED));
    assert_eq!(remove(&host, 3, "x", 0), Err(ERR_UNSUPPORTED));
    assert!(card.has("Keep/x"));
    // Root 0 `""` empties the data folder; the folder stays.
    card.file(&std::format!("{DATA}/a/b/c"), 1);
    card.file(&std::format!("{DATA}/d"), 1);
    card.file(".paperpaw/plugins/files-plus/keep", 1);
    card.file(".paperpaw/plugins/fil/keep", 1);
    assert_eq!(remove_all(&host, 0, ""), 1);
    assert!(card.is_dir(DATA));
    assert!(!card.has(&std::format!("{DATA}/a")) && !card.has(&std::format!("{DATA}/d")));
    assert!(
        card.has(".paperpaw/plugins/files-plus/keep") && card.has(".paperpaw/plugins/fil/keep")
    );
    // The host's received bodies may be removed whole.
    card.file(&std::format!("{DATA}/.received/b1"), 1);
    assert_eq!(remove_all(&host, 0, ".received"), 1);
    assert!(!card.has(&std::format!("{DATA}/.received")));
    let flags = Record::ServiceFsList(wire::ServiceFsList {
        root: 1,
        path: "Keep",
        cursor: 0,
        flags: 2,
    });
    assert_eq!(call(&host, 5, flags), Err(ERR_UNSUPPORTED));
}

#[test]
fn remove_tree_of_the_data_folder_leaves_nothing_even_names_no_path_holds() {
    let (card, host) = fixture();
    // Made by a computer while the card was a drive: names the paths refuse.
    for name in ["bad.", "nul.txt", "co:lon", "trail ", "a/b"] {
        if name.contains('/') {
            continue;
        }
        card.file(&std::format!("{DATA}/{name}"), 1);
        card.file(&std::format!("{DATA}/sub/{name}"), 1);
    }
    card.file(&std::format!("{DATA}/{}", "x".repeat(300)), 1);
    card.file(&std::format!("{DATA}/ok.txt"), 1);
    card.file(".paperpaw/plugins/other/keep", 1);
    let calls = remove_all(&host, 0, "");
    assert!(calls >= 1);
    let left: Vec<_> = card
        .paths()
        .into_iter()
        .filter(|p| p.starts_with(DATA) && p != DATA)
        .collect();
    assert!(left.is_empty(), "{left:?}");
    assert!(card.is_dir(DATA) && card.has(".paperpaw/plugins/other/keep"));
    // The same on root 1.
    card.file("Junk/bad.", 1);
    card.file("Junk/d/nul.txt", 1);
    remove_all(&host, 1, "Junk");
    assert!(!card.has("Junk"));
}

#[test]
fn remove_tree_with_a_hostile_cursor_removes_only_inside() {
    let (card, host) = fixture();
    card.file("T/a/one", 1);
    card.file("T/two", 1);
    card.file("Keep/k", 1);
    card.file("Keep/T/inner", 1);
    for cursor in [1, 99, 1 << 56, 3 << 56 | 4, u64::MAX] {
        let mut place = cursor;
        for _ in 0..50 {
            let (_, done, next) = remove(&host, 1, "T", place).unwrap();
            if done {
                break;
            }
            place = next;
        }
    }
    assert!(card.has("Keep/k") && card.has("Keep/T/inner"));
}

#[test]
fn move_renames_a_file_or_a_folder() {
    let (card, host) = fixture();
    card.file("A/book.epub", 5);
    card.file("A/Sub/inner.txt", 1);
    card.dir("B");
    assert_eq!(
        do_move(&host, (1, "A/book.epub"), (1, "B/book.epub"), 0),
        Ok(())
    );
    assert!(!card.has("A/book.epub") && card.has("B/book.epub"));
    assert_eq!(do_move(&host, (1, "A/Sub"), (1, "B/Moved"), 0), Ok(()));
    assert!(card.has("B/Moved/inner.txt") && !card.has("A/Sub"));
    // A new name in the same folder.
    assert_eq!(
        do_move(&host, (1, "B/book.epub"), (1, "B/novel.epub"), 0),
        Ok(())
    );
    assert!(card.has("B/novel.epub"));
}

#[test]
fn move_statuses() {
    let (card, host) = fixture();
    card.file("A/f", 1);
    card.file("A/g", 2);
    card.file("A/Dir/x", 1);
    card.dir("B");
    assert_eq!(
        do_move(&host, (1, "A/none"), (1, "B/none"), 0),
        Err(ERR_NOT_FOUND)
    );
    assert_eq!(
        do_move(&host, (1, "A/f"), (1, "Nowhere/f"), 0),
        Err(ERR_NOT_FOUND),
        "the parent of `to` must exist"
    );
    assert_eq!(
        do_move(&host, (1, "A/f"), (1, "A/f/in"), 0),
        Err(ERR_INVALID),
        "a file is no parent"
    );
    assert_eq!(
        do_move(&host, (1, "A/f"), (1, "A/g"), 0),
        Err(ERR_STATE),
        "no replace flag"
    );
    assert_eq!(
        do_move(&host, (1, "A/f"), (1, "A/Dir"), 1),
        Err(ERR_STATE),
        "a folder at `to`"
    );
    for flags in [0, 1] {
        assert_eq!(
            do_move(&host, (1, "A/Dir"), (1, "A/g"), flags),
            Err(ERR_STATE),
            "a folder never replaces a file, flag {flags}"
        );
    }
    assert_eq!(
        do_move(&host, (1, "A/Dir"), (1, "B"), 1),
        Err(ERR_STATE),
        "never merged"
    );
    assert_eq!(
        do_move(&host, (1, "A"), (1, "A/Dir/inside"), 0),
        Err(ERR_INVALID),
        "into itself"
    );
    assert_eq!(do_move(&host, (1, "A"), (1, "A"), 0), Err(ERR_INVALID));
    assert_eq!(do_move(&host, (1, "A/f"), (1, "A/f"), 0), Err(ERR_STATE));
    assert_eq!(
        do_move(&host, (1, ""), (1, "X"), 0),
        Err(ERR_INVALID),
        "a root is never moved"
    );
    assert_eq!(do_move(&host, (1, "A/f"), (1, ""), 0), Err(ERR_INVALID));
    assert_eq!(
        do_move(&host, (0, ""), (1, "X"), 0),
        Err(ERR_INVALID),
        "not the data folder onto the card"
    );
    assert_eq!(
        do_move(&host, (1, "A/f"), (1, "B/f"), 2),
        Err(ERR_UNSUPPORTED)
    );
    assert_eq!(
        do_move(&host, (1, "A/f"), (1, "B/f"), 0x8000_0001),
        Err(ERR_UNSUPPORTED)
    );
    // Nothing changed.
    assert!(card.has("A/f") && card.has("A/g") && card.has("A/Dir/x") && !card.has("B/f"));
    card.full.set(true);
    assert_eq!(do_move(&host, (1, "A/f"), (1, "B/f"), 0), Err(ERR_FULL));
    card.full.set(false);
    card.gone.set(true);
    assert_eq!(do_move(&host, (1, "A/f"), (1, "B/f"), 0), Err(ERR_IO));
}

#[test]
fn move_replaces_a_file_only_with_the_flag() {
    let (card, host) = fixture();
    card.file("A/new", 11);
    card.file("A/old", 22);
    assert_eq!(do_move(&host, (1, "A/new"), (1, "A/old"), 1), Ok(()));
    assert!(!card.has("A/new"));
    assert_eq!(card.size("A/old"), Ok(11));
    // The flag with nothing to replace is a plain move.
    card.file("A/c", 1);
    assert_eq!(do_move(&host, (1, "A/c"), (1, "A/d"), 1), Ok(()));
    // Onto itself with the flag: nothing to do.
    assert_eq!(do_move(&host, (1, "A/d"), (1, "A/d"), 1), Ok(()));
}

#[test]
fn a_case_only_rename_is_a_rename_where_the_storage_ignores_case() {
    let (card, host) = fixture();
    card.file("A/Book.epub", 3);
    assert_eq!(
        do_move(&host, (1, "A/Book.epub"), (1, "A/book.epub"), 0),
        Ok(())
    );
    assert!(card.paths().contains(&"A/book.epub".into()));
    // A folder too, though `to` is "within" `from` by case.
    card.file("A/Dir/x", 1);
    assert_eq!(do_move(&host, (1, "A/Dir"), (1, "A/DIR"), 0), Ok(()));
    assert!(card.has("A/DIR/x"));
    // Where case matters, the same names are two entries.
    card.fold.set(false);
    card.file("A/z", 1);
    card.file("A/Z", 2);
    assert_eq!(do_move(&host, (1, "A/z"), (1, "A/Z"), 0), Err(ERR_STATE));
}

#[test]
fn move_between_the_data_folder_and_the_card() {
    let (card, host) = fixture();
    card.file(&std::format!("{DATA}/article.html"), 9);
    card.dir("Books");
    // The parent folder of `to` is the card's: it exists.
    assert_eq!(
        do_move(&host, (0, "article.html"), (1, "Books/article.html"), 0),
        Ok(())
    );
    assert!(card.has("Books/article.html") && !card.has(&std::format!("{DATA}/article.html")));
    assert_eq!(
        do_move(
            &host,
            (1, "Books/article.html"),
            (0, "back/article.html"),
            0
        ),
        Err(ERR_NOT_FOUND)
    );
    assert_eq!(
        do_move(&host, (1, "Books/article.html"), (0, "article.html"), 0),
        Ok(())
    );
    // Root 2 is never a side of a change.
    assert_eq!(do_move(&host, (2, "x"), (0, "y"), 0), Err(ERR_DENIED));
    assert_eq!(
        do_move(&host, (0, "article.html"), (2, "y"), 0),
        Err(ERR_DENIED)
    );
    assert_eq!(
        do_move(&host, (0, "article.html"), (3, "y"), 0),
        Err(ERR_UNSUPPORTED)
    );
}

#[test]
fn the_library_follows_moves_of_the_users_files_only() {
    let (card, host) = fixture();
    card.file("Books/a.epub", 1);
    card.file(&std::format!("{DATA}/x"), 1);
    card.dir("Other");
    assert_eq!(do_move(&host, (0, "x"), (0, "y"), 0), Ok(()));
    assert_eq!(
        host.library_moves.get(),
        0,
        "the data folder is not the library's"
    );
    assert!(
        call(
            &host,
            6,
            move_record(1, "Books/a.epub", 1, "Other/a.epub", 0)
        )
        .is_ok()
    );
    assert_eq!(host.library_moves.get(), 1);
    assert!(
        host.journal.borrow().is_some(),
        "the entry is written before the rename"
    );
    assert!(host.moves.active());
    host.finish_job();
    assert!(!host.moves.active() && host.journal.borrow().is_none());
}

/// #612: a plugin's export, a flash card list and its `.part` are no books;
/// their move on root 1 is the card's rename, and a second one right after it
/// is not `ERR_BUSY` behind the first one's job.
#[test]
fn a_file_that_is_no_book_moves_without_the_librarys_job() {
    let (card, host) = fixture();
    host.formats.set(true);
    card.file("Books/a.epub", 1);
    card.file("Notes/Vocabulary.tsv.part", 3);
    card.file("Notes/Vocabulary.tsv", 2);
    card.file("Notes/Old.tsv.part", 2);
    card.dir("Notes/Kept");
    // The call alone: `do_move` would run a job out before the next one.
    let mv = |from: &str, to: &str, flags: u32| {
        call(&host, 6, move_record(1, from, 1, to, flags)).map(|reply| assert!(reply.is_empty()))
    };
    for (from, to) in [
        ("Notes/Old.tsv.part", "Notes/Old.tsv"),
        ("Notes/Vocabulary.tsv.part", "Notes/Vocabulary.tsv"),
    ] {
        assert_eq!(mv(from, to, 1), Ok(()), "{from}");
        assert!(!host.moves.active(), "no job after {from}");
    }
    assert_eq!(host.library_moves.get(), 0);
    assert!(host.journal.borrow().is_none(), "nothing to replay");
    assert_eq!(card.size("Notes/Vocabulary.tsv"), Ok(3));
    assert!(!card.exists("Notes/Vocabulary.tsv.part"));
    // The service's refusals are the same for it.
    assert_eq!(
        mv("Notes/Old.tsv", "Notes/Vocabulary.tsv", 0),
        Err(ERR_STATE)
    );
    assert_eq!(mv("Notes/Old.tsv", "Notes/Kept", 1), Err(ERR_STATE));
    // A folder, a book and a book's `.bak` are the library's as before.
    for (from, to) in [
        ("Notes/Kept", "Notes/Shelf"),
        ("Books/a.epub", "Books/a.epub.bak"),
        ("Books/a.epub.bak", "Books/a.old"),
    ] {
        let before = host.library_moves.get();
        assert_eq!(mv(from, to, 0), Ok(()), "{from}");
        assert_eq!(host.library_moves.get(), before + 1, "{from}");
        // One move at a time: a file that is no book waits too.
        assert_eq!(
            mv("Notes/Old.tsv", "Notes/Older.tsv", 0),
            Err(ERR_BUSY),
            "{from}"
        );
        host.finish_job();
    }
}
