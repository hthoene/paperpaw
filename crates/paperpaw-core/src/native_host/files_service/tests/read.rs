//! `list`, `stat` and `space`: rows, paging at and past the limits, the
//! cursor rules, the root rules of listings, and the statuses.
use super::*;
use paperpaw_native::host_glue::{ERR_INVALID, ERR_IO, ERR_NO_SPACE, ERR_NOT_FOUND};

fn folder_of(card: &Card, dir: &str, n: usize) {
    card.dir(dir);
    for i in 0..n {
        card.file(&std::format!("{dir}/f{i:03}.txt"), i as u64);
    }
}

#[test]
fn a_listing_shows_entries_in_storage_order_with_their_facts() {
    let (card, host) = fixture();
    card.file("Books/a.epub", 1234);
    card.dir("Books/Sub");
    card.file("Books/.hidden", 1);
    let reply = call(&host, 1, list_record(1, "Books", 0)).unwrap();
    let (rows, end) = rows(&reply);
    let names: Vec<_> = rows.iter().map(|r| r.0.as_str()).collect();
    assert_eq!(names, ["a.epub", "Sub", ".hidden"], "hidden names included");
    assert_eq!((rows[0].1, rows[0].2), (0, 1234));
    assert_eq!((rows[1].1, rows[1].2), (1, 0), "a folder: flag, size 0");
    assert!(rows[0].3.is_some_and(|m| m >= 1_000));
    let end = end.unwrap();
    assert_eq!(
        (end.flags, end.total, end.generation, end.next, end.cursor),
        (1, 0, 0, 0, None)
    );
}

#[test]
fn a_page_holds_at_most_32_rows_and_the_cursor_goes_on() {
    for count in [31, 32, 33, 64, 65, 100] {
        let (card, host) = fixture();
        folder_of(&card, "Many", count);
        let reply = call(&host, 1, list_record(1, "Many", 0)).unwrap();
        let (first, end) = rows(&reply);
        assert_eq!(first.len(), count.min(32), "{count}");
        let end = end.unwrap();
        // The page that is not the last carries a cursor; the last none.
        assert_eq!(end.flags & 1 == 1, count <= 32, "{count}");
        assert_eq!(end.cursor.is_some(), count > 32, "{count}");
        let all = list_all(&host, 1, "Many").unwrap();
        let expected: Vec<_> = (0..count).map(|i| std::format!("f{i:03}.txt")).collect();
        assert_eq!(all, expected, "{count}: no row lost or repeated");
    }
}

#[test]
fn a_page_ends_before_a_row_that_does_not_fit() {
    let (card, host) = fixture();
    card.dir("Long");
    // 40 names of 250 bytes: a row is about 280 bytes framed, so a page
    // of 4,096 bytes holds fewer than 32 and never cuts one.
    for i in 0..40 {
        card.file(&std::format!("Long/{i:02}{}", "n".repeat(248)), 1);
    }
    let reply = call(&host, 1, list_record(1, "Long", 0)).unwrap();
    assert!(reply.len() <= 4096);
    let (page, end) = rows(&reply);
    assert!(page.len() < 32 && page.len() >= 5, "{}", page.len());
    assert!(page.iter().all(|r| r.0.len() == 250));
    let end = end.unwrap();
    assert_eq!(end.flags & 1, 0);
    assert_eq!(list_all(&host, 1, "Long").unwrap().len(), 40);
}

#[test]
fn a_name_the_path_rules_refuse_is_listed_unaddressable() {
    let (card, host) = fixture();
    card.dir("Odd");
    let long = "x".repeat(300);
    let longest = "y".repeat(765);
    for name in ["ok.txt", "trailing.", "co:lon", "nul.txt", &long, &longest] {
        card.file(&std::format!("Odd/{name}"), 7);
    }
    let reply = call(&host, 1, list_record(1, "Odd", 0)).unwrap();
    let (page, _) = rows(&reply);
    assert_eq!(page.len(), 6, "no entry is skipped");
    let flags: Vec<_> = page.iter().map(|r| r.1).collect();
    assert_eq!(flags, [0, 2, 2, 2, 2, 2]);
    assert_eq!(page[4].0, long);
    assert_eq!(page[5].0, longest, "the full name up to 765 bytes");
}

#[test]
fn a_cursor_that_no_longer_holds_never_fails_or_shows_what_is_not_there() {
    let (card, host) = fixture();
    folder_of(&card, "Gone", 70);
    let reply = call(&host, 1, list_record(1, "Gone", 0)).unwrap();
    let cursor = rows(&reply).1.unwrap().cursor.unwrap();
    // The entry the place named is removed, and the folder emptied further.
    for i in 30..70 {
        card.nodes_remove(&std::format!("Gone/f{i:03}.txt"));
    }
    let reply = call(&host, 1, list_record(1, "Gone", cursor)).unwrap();
    let (page, end) = rows(&reply);
    assert!(page.iter().all(|r| card.has(&std::format!("Gone/{}", r.0))));
    assert_eq!(end.unwrap().flags & 1, 1);
    // Places of nothing in particular.
    for cursor in [1, 12_345, u64::MAX] {
        let reply = call(&host, 1, list_record(1, "Gone", cursor)).unwrap();
        let (page, _) = rows(&reply);
        assert!(page.iter().all(|r| card.has(&std::format!("Gone/{}", r.0))));
    }
}

#[test]
fn root_one_hides_the_protected_folders_by_name_and_by_alias() {
    let (card, host) = fixture();
    for dir in ["Books", "plugins", ".paperpaw", "PLUGINS2"] {
        card.dir(dir);
    }
    card.dir("PAPERP~1");
    host.aliases.borrow_mut().push((1, "PAPERP~1"));
    let names = list_all(&host, 1, "").unwrap();
    assert_eq!(names, ["Books", "PLUGINS2"]);
    // Below the root nothing is hidden: a folder named `plugins` in `Books`.
    card.dir("Books/plugins");
    assert_eq!(list_all(&host, 1, "Books").unwrap(), ["plugins"]);
    // Their own paths are refused, in any case and by alias.
    for protected in [
        "plugins",
        "PLUGINS",
        ".paperpaw",
        ".PaperPaw/x",
        "PAPERP~1",
        "plugins/files",
    ] {
        assert_eq!(
            call(&host, 1, list_record(1, protected, 0)),
            Err(ERR_INVALID),
            "{protected}"
        );
        assert_eq!(
            call(&host, 2, path(1, protected)),
            Err(ERR_INVALID),
            "{protected}"
        );
    }
}

#[test]
fn root_one_does_not_list_paperpaw_ini_in_any_case() {
    let (card, host) = fixture();
    card.file("paperpaw.ini", 7);
    card.file("Books/paperpaw.ini", 1);
    card.file("PAPERPAW.INI.bak", 1);
    // The top hides it; its neighbours and a same-named file below stay.
    assert_eq!(
        list_all(&host, 1, "").unwrap(),
        ["Books", "PAPERPAW.INI.bak"]
    );
    assert_eq!(list_all(&host, 1, "Books").unwrap(), ["paperpaw.ini"]);
    for name in [
        "paperpaw.ini",
        "PAPERPAW.INI",
        "Paperpaw.ini",
        "paperpaw.ını",
    ] {
        assert_eq!(
            call(&host, 2, path(1, name)),
            Err(ERR_INVALID),
            "stat {name}"
        );
        assert_eq!(
            call(&host, 1, list_record(1, name, 0)),
            Err(ERR_INVALID),
            "list {name}"
        );
    }
    // The host's own read of it (the kernel's settings) is the storage's, and
    // whole.
    assert!(card.has("paperpaw.ini"));
}

#[test]
fn the_protected_folders_are_known_as_the_fat_driver_folds_names() {
    // The driver compares names in upper case, the whole name: `ı` is `I`,
    // `ſ` is `S`, so these reach `plugins` and `.paperpaw` unless refused.
    let (card, host) = fixture();
    for dir in ["Books", "plugins", "plugıns", "pluginſ"] {
        card.dir(dir);
    }
    assert_eq!(list_all(&host, 1, "").unwrap(), ["Books"]);
    for protected in [
        "plugıns",
        "PLUGıNS/x",
        "pluginſ",
        "PLUGINſ/x",
        "plugıns/files",
        "plugınſ",
    ] {
        assert_eq!(
            call(&host, 1, list_record(1, protected, 0)),
            Err(ERR_INVALID),
            "{protected}"
        );
        assert_eq!(
            call(&host, 2, path(1, protected)),
            Err(ERR_INVALID),
            "{protected}"
        );
    }
    // Folds that do not land on the name are not it: the Kelvin sign stays a
    // letter of its own, and `plugin` + `ß` is `PLUGINSS`.
    for other in ["plugin\u{df}", "\u{212a}Books"] {
        card.dir(other);
        assert!(
            list_all(&host, 1, "")
                .unwrap()
                .contains(&std::string::String::from(other)),
            "{other}"
        );
    }
}

#[test]
fn root_zero_and_two_are_the_plugins_own_folders() {
    let (card, host) = fixture();
    card.file(&std::format!("{DATA}/notes.txt"), 5);
    card.file(&std::format!("{DATA}/.received/body"), 5);
    card.file(&std::format!("{BUNDLE}/data/phone.txt"), 9);
    card.file(".paperpaw/plugins/other/secret.txt", 1);
    card.file("plugins/other/data.txt", 1);
    assert_eq!(list_all(&host, 0, "").unwrap(), ["notes.txt", ".received"]);
    assert_eq!(list_all(&host, 2, "").unwrap(), ["data"]);
    assert_eq!(list_all(&host, 2, "data").unwrap(), ["phone.txt"]);
    // No way into another plugin's folders.
    for bad in ["../other", "../other/secret.txt", "..", "/other"] {
        assert_eq!(
            call(&host, 1, list_record(0, bad, 0)),
            Err(ERR_INVALID),
            "{bad}"
        );
        assert_eq!(call(&host, 2, path(2, bad)), Err(ERR_INVALID), "{bad}");
    }
}

#[test]
fn a_root_whose_folder_is_not_made_yet_is_empty() {
    let (_, host) = fixture();
    for root in [0, 2] {
        let reply = call(&host, 1, list_record(root, "", 0)).unwrap();
        let (page, end) = rows(&reply);
        assert!(page.is_empty());
        assert_eq!(end.unwrap().flags, 1);
        let entry = call(&host, 2, path(root, "")).unwrap();
        let (entry, _) = rows(&entry);
        assert_eq!((entry[0].0.as_str(), entry[0].1, entry[0].2), ("", 1, 0));
        assert_eq!(call(&host, 2, path(root, "x")), Err(ERR_NOT_FOUND));
        assert_eq!(
            call(&host, 1, list_record(root, "x", 0)),
            Err(ERR_NOT_FOUND)
        );
    }
}

#[test]
fn a_built_in_plugins_install_folder_is_empty_whatever_a_leftover_holds() {
    let (card, host) = fixture();
    // A leftover of an older install (Beta 3 kept `plugins/<id>/` for the
    // linked plugins too).
    card.file(&std::format!("{BUNDLE}/data/old.txt"), 9);
    card.file(&std::format!("{BUNDLE}/plugin.ini"), 9);
    assert_eq!(list_all(&host, 2, "").unwrap(), ["data", "plugin.ini"]);
    host.built_in.borrow_mut().push("files");
    // Listing: no rows, the last page. Stat of the folder: a folder.
    let reply = call(&host, 1, list_record(2, "", 0)).unwrap();
    let (page, end) = rows(&reply);
    assert!(page.is_empty());
    assert_eq!(end.unwrap().flags, 1);
    let entry = call(&host, 2, path(2, "")).unwrap();
    assert_eq!(rows(&entry).0[0].1, 1);
    // Every path in it is not found; a bad one is still refused as bad.
    for p in ["plugin.ini", "data", "data/old.txt", "nope"] {
        assert_eq!(call(&host, 2, path(2, p)), Err(ERR_NOT_FOUND), "{p}");
        assert_eq!(
            call(&host, 1, list_record(2, p, 0)),
            Err(ERR_NOT_FOUND),
            "{p}"
        );
    }
    assert_eq!(call(&host, 2, path(2, "../x")), Err(ERR_INVALID));
    // Another root of the same plugin is as it was; a change of root 2 is
    // still denied.
    card.file(&std::format!("{DATA}/notes.txt"), 5);
    assert_eq!(list_all(&host, 0, "").unwrap(), ["notes.txt"]);
}

#[test]
fn list_statuses() {
    let (card, host) = fixture();
    card.file("Books/a.txt", 1);
    assert_eq!(
        call(&host, 1, list_record(1, "Nope", 0)),
        Err(ERR_NOT_FOUND)
    );
    assert_eq!(
        call(&host, 1, list_record(1, "Books/a.txt", 0)),
        Err(ERR_INVALID),
        "a file"
    );
    assert_eq!(
        call(&host, 1, list_record(1, "Books/a.txt/x", 0)),
        Err(ERR_NOT_FOUND)
    );
    let flags = Record::ServiceFsList(wire::ServiceFsList {
        root: 1,
        path: "Books",
        cursor: 0,
        flags: 1,
    });
    assert_eq!(call(&host, 1, flags), Err(ERR_UNSUPPORTED));
    let wrong = path(1, "Books");
    assert_eq!(call(&host, 1, wrong), Err(ERR_INVALID), "the wrong record");
}

#[test]
fn stat_answers_one_entry() {
    let (card, host) = fixture();
    card.file("Books/a.epub", 4096);
    card.dir("Books/Sub");
    let file = rows(&call(&host, 2, path(1, "Books/a.epub")).unwrap()).0;
    assert_eq!(
        (file[0].0.as_str(), file[0].1, file[0].2),
        ("a.epub", 0, 4096)
    );
    assert!(file[0].3.is_some());
    let folder = rows(&call(&host, 2, path(1, "Books/Sub")).unwrap()).0;
    assert_eq!(
        (folder[0].0.as_str(), folder[0].1, folder[0].2),
        ("Sub", 1, 0)
    );
    let root = rows(&call(&host, 2, path(1, "")).unwrap()).0;
    assert_eq!((root[0].0.as_str(), root[0].1), ("", 1));
    assert_eq!(call(&host, 2, path(1, "Books/none")), Err(ERR_NOT_FOUND));
    assert_eq!(call(&host, 2, path(1, "none/x")), Err(ERR_NOT_FOUND));
    assert_eq!(
        call(&host, 2, path(1, "Books/a.epub/x")),
        Err(ERR_NOT_FOUND)
    );
    // A name over 255 bytes in a segment is over a cap; at 255 it is a name.
    assert_eq!(call(&host, 2, path(1, &"s".repeat(256))), Err(ERR_NO_SPACE));
    assert_eq!(
        call(&host, 2, path(1, &"s".repeat(255))),
        Err(ERR_NOT_FOUND)
    );
    let wrong = list_record(1, "Books", 0);
    assert_eq!(call(&host, 2, wrong), Err(ERR_INVALID));
}

#[test]
fn space_is_the_free_room_after_the_reserve() {
    let (card, host) = fixture();
    card.free.set(Some(100 << 20));
    card.total.set(Some(1 << 30));
    let space = |host: &Rc<TestHost>, root| {
        let reply = call(host, 3, path(root, "")).unwrap();
        let mut found = None;
        crate::native_host::stage_wire::visit_stream(&reply, |_, framed| {
            if let Record::ServiceFsSpace(s) = wire::record_decode_one(framed).unwrap() {
                found = Some(s);
            }
            Ok(())
        })
        .unwrap();
        found.unwrap()
    };
    // The reserve is the larger of 256 KiB and 1 % of the card (10 MiB here).
    let reserve = (1u64 << 30) / 100;
    let s = space(&host, 1);
    assert_eq!(
        (s.flags, s.free, s.total),
        (7, (100 << 20) - reserve, 1 << 30)
    );
    // FAT before its first count: unknown, never a guess.
    card.free.set(None);
    let s = space(&host, 0);
    assert_eq!((s.flags, s.free), (6, 0));
    card.total.set(None);
    card.fold.set(false);
    assert_eq!(space(&host, 2).flags, 0, "nothing known, no case folding");
    card.fold.set(true);
    // Less free than the reserve: 0, known.
    card.free.set(Some(1000));
    card.total.set(Some(1 << 30));
    let s = space(&host, 1);
    assert_eq!((s.flags & 1, s.free), (1, 0));
    // The path must be the root.
    assert_eq!(call(&host, 3, path(1, "Books")), Err(ERR_INVALID));
}

#[test]
fn a_card_that_is_away_is_an_io_error_for_every_read() {
    let (card, host) = fixture();
    card.file("Books/a.txt", 1);
    card.gone.set(true);
    assert_eq!(call(&host, 1, list_record(1, "Books", 0)), Err(ERR_IO));
    assert_eq!(call(&host, 1, list_record(0, "", 0)), Err(ERR_IO));
    assert_eq!(call(&host, 2, path(1, "Books/a.txt")), Err(ERR_IO));
    // The `files` plugin's probe: root 1 `""`.
    assert_eq!(call(&host, 2, path(1, "")), Err(ERR_IO));
    assert_eq!(call(&host, 2, path(0, "")), Err(ERR_IO));
    card.free.set(None);
    card.total.set(None);
    assert_eq!(call(&host, 3, path(1, "")), Err(ERR_IO));
    card.gone.set(false);
    assert!(call(&host, 2, path(1, "")).is_ok(), "present again");
}
