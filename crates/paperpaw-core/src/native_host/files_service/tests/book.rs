//! Root 3, the book of a `core.format` call: `list` and `stat` only, read
//! only, and only for a `folder` or `siblings` format inside its call.
use super::*;
use crate::native_host::format_scope::BookKind;
use paperpaw_native::host_glue::{ERR_DENIED, ERR_INVALID, ERR_NOT_FOUND};

fn siblings() -> Setup {
    Setup {
        book: Some((BookKind::Siblings, "Series/vol1.cbz", "vol1.cbz")),
        ..NORMAL
    }
}

fn names(host: &Rc<TestHost>, setup: Setup, path: &str) -> Vec<std::string::String> {
    let reply = call_with(host, setup, 1, list_record(3, path, 0)).unwrap();
    rows(&reply).0.into_iter().map(|r| r.0).collect()
}

fn series() -> (Rc<Card>, Rc<TestHost>) {
    let (card, host) = fixture();
    card.file("Series/vol1.cbz", 10);
    card.file("Series/vol2.cbz", 20);
    card.dir("Series/extras");
    card.file("Series/extras/map.png", 5);
    card.file("Other/x.cbz", 1);
    (card, host)
}

#[test]
fn a_siblings_book_lists_and_stats_its_folder_and_below() {
    let (_card, host) = series();
    let mut top = names(&host, siblings(), "");
    top.sort();
    assert_eq!(top, ["extras", "vol1.cbz", "vol2.cbz"]);
    assert_eq!(names(&host, siblings(), "extras"), ["map.png"]);
    let reply = call_with(&host, siblings(), 2, path(3, "vol2.cbz")).unwrap();
    let (listed, _) = rows(&reply);
    assert_eq!(
        (listed[0].0.as_str(), listed[0].1, listed[0].2),
        ("vol2.cbz", 0, 20)
    );
    let reply = call_with(&host, siblings(), 2, path(3, "")).unwrap();
    assert_eq!(rows(&reply).0[0].1, 1, "the folder itself");
}

#[test]
fn a_folder_book_is_the_folder_itself() {
    let (_card, host) = series();
    let setup = Setup {
        book: Some((BookKind::Folder, "Series", "")),
        ..NORMAL
    };
    let mut top = names(&host, setup, "");
    top.sort();
    assert_eq!(top, ["extras", "vol1.cbz", "vol2.cbz"]);
    assert_eq!(names(&host, setup, "extras"), ["map.png"]);
}

#[test]
fn root_three_needs_no_storage_grant_but_a_format_call() {
    let (_card, host) = series();
    let denied = Setup {
        granted: false,
        ..siblings()
    };
    assert_eq!(names(&host, denied, "extras"), ["map.png"]);
    // Outside a call, and for a single-file format: unsupported.
    let single = Setup {
        book: Some((BookKind::File, "Series/vol1.cbz", "")),
        ..NORMAL
    };
    for setup in [NORMAL, single] {
        assert_eq!(
            call_with(&host, setup, 1, list_record(3, "", 0)),
            Err(ERR_UNSUPPORTED)
        );
        assert_eq!(
            call_with(&host, setup, 2, path(3, "")),
            Err(ERR_UNSUPPORTED)
        );
        assert_eq!(
            call_with(&host, setup, 4, path(3, "x")),
            Err(ERR_UNSUPPORTED)
        );
    }
}

/// The scope is checked first: a change op on root 3 outside a call of a `folder` or
/// `siblings` format (none, or a single-file format) is `ERR_UNSUPPORTED`; inside such a
/// call it is `ERR_DENIED` (`root_three_is_read_only_and_for_list_and_stat_only`).
#[test]
fn a_change_op_outside_a_format_call_is_unsupported_not_denied() {
    let (card, host) = series();
    let before = card.paths();
    let single = Setup {
        book: Some((BookKind::File, "Series/vol1.cbz", "")),
        ..NORMAL
    };
    for setup in [NORMAL, single] {
        for (op, record) in [
            (4, path(3, "new")),
            (5, list_record(3, "extras", 0)),
            (6, move_record(3, "vol1.cbz", 1, "Books/x.cbz", 0)),
            (6, move_record(1, "Other/x.cbz", 3, "y.cbz", 0)),
        ] {
            assert_eq!(
                call_with(&host, setup, op, record),
                Err(ERR_UNSUPPORTED),
                "op {op}"
            );
        }
    }
    assert_eq!(card.paths(), before, "nothing changed");
}

/// A trailing `/` of a book's own path is not part of its folder (the storage joins
/// folders without it); a trailing `/` in a request path is an empty segment, which the
/// path rules (`files.md` §1.1) refuse.
#[test]
fn a_trailing_slash_is_dropped_from_the_book_and_refused_in_a_request() {
    let (_card, host) = series();
    let folder = Setup {
        book: Some((BookKind::Folder, "Series/", "")),
        ..NORMAL
    };
    let siblings_slash = Setup {
        book: Some((BookKind::Siblings, "Series/vol1.cbz/", "vol1.cbz")),
        ..NORMAL
    };
    for setup in [folder, siblings_slash] {
        let mut top = names(&host, setup, "");
        top.sort();
        assert_eq!(top, ["extras", "vol1.cbz", "vol2.cbz"]);
        assert_eq!(names(&host, setup, "extras"), ["map.png"]);
        for bad in ["extras/", "/", "vol1.cbz/"] {
            assert_eq!(
                call_with(&host, setup, 2, path(3, bad)),
                Err(ERR_INVALID),
                "{bad}"
            );
            assert_eq!(
                call_with(&host, setup, 1, list_record(3, bad, 0)),
                Err(ERR_INVALID),
                "{bad}"
            );
        }
    }
}

#[test]
fn paths_stay_below_the_book_folder() {
    let (_card, host) = series();
    for bad in [
        "..",
        "../Other",
        "extras/../../Other",
        "/Other",
        "a\\b",
        "a//b",
    ] {
        assert_eq!(
            call_with(&host, siblings(), 2, path(3, bad)),
            Err(ERR_INVALID),
            "{bad}"
        );
        assert_eq!(
            call_with(&host, siblings(), 1, list_record(3, bad, 0)),
            Err(ERR_INVALID),
            "{bad}"
        );
    }
    assert_eq!(
        call_with(&host, siblings(), 2, path(3, "nothing")),
        Err(ERR_NOT_FOUND)
    );
}

#[test]
fn root_three_is_read_only_and_for_list_and_stat_only() {
    let (card, host) = series();
    let before = card.paths();
    assert_eq!(
        call_with(&host, siblings(), 3, path(3, "")),
        Err(ERR_UNSUPPORTED),
        "space"
    );
    for (op, record) in [
        (4, path(3, "new")),
        (5, list_record(3, "extras", 0)),
        (6, move_record(3, "vol1.cbz", 1, "Books/x.cbz", 0)),
        (6, move_record(1, "Other/x.cbz", 3, "y.cbz", 0)),
    ] {
        assert_eq!(
            call_with(&host, siblings(), op, record),
            Err(ERR_DENIED),
            "op {op}"
        );
    }
    assert_eq!(card.paths(), before, "nothing changed");
}

#[test]
fn a_book_at_the_top_of_the_card_does_not_reach_the_kernel_folders() {
    let (card, host) = series();
    card.file("top.cbz", 1);
    card.file("plugins/files/x", 1);
    card.file(".paperpaw/secrets.txt", 1);
    let top = Setup {
        book: Some((BookKind::Siblings, "top.cbz", "top.cbz")),
        ..NORMAL
    };
    let listed = names(&host, top, "");
    assert!(listed.contains(&"Series".into()));
    assert!(!listed.contains(&"plugins".into()));
    assert!(!listed.contains(&".paperpaw".into()));
    for bad in ["plugins", ".paperpaw/secrets.txt", "Plugins/files"] {
        assert_eq!(
            call_with(&host, top, 2, path(3, bad)),
            Err(ERR_INVALID),
            "{bad}"
        );
    }
    assert!(call_with(&host, top, 2, path(3, "")).is_ok());
}

#[test]
fn a_book_folder_that_is_gone_is_not_found() {
    let (_card, host) = fixture();
    assert_eq!(
        call_with(&host, siblings(), 2, path(3, "")),
        Err(ERR_NOT_FOUND)
    );
    assert_eq!(
        call_with(&host, siblings(), 1, list_record(3, "", 0)),
        Err(ERR_NOT_FOUND)
    );
}
