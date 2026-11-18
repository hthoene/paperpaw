//! The native decisions (`native_wallabag`): which texts are article ids, the
//! set-up's rows, the lists as they are read and written, when an article
//! counts as read, and refusing instead of aborting when memory runs short.
use crate::fail_alloc::sweep;
use crate::native_wallabag as native;
use alloc::vec::Vec;

#[test]
fn ids_are_numbers_of_up_to_twenty_digits() {
    for id in ["7", "0", "0012", "12345678901234567890"] {
        assert!(native::is_id(id), "{id:?}");
    }
    for id in [
        "",
        "123456789012345678901",
        "-1",
        "1.5",
        "a",
        " 1",
        "1 ",
        "٣",
        "9\n",
        "../x",
    ] {
        assert!(!native::is_id(id), "{id:?}");
    }
    assert_eq!(native::MAX_ARTICLES, 30);
}

#[test]
fn the_rows_have_their_titles_and_fields() {
    assert_eq!(native::ROWS.len(), 6);
    for (row, title, field) in [
        (native::Row::Server, "wallabag.server", "server"),
        (native::Row::User, "wallabag.user", "user"),
        (native::Row::Password, "wallabag.password", "password"),
        (native::Row::Client, "wallabag.client-id", "client"),
        (native::Row::Secret, "wallabag.client-secret", "secret"),
        (native::Row::Fetch, "wallabag.fetch", ""),
    ] {
        assert_eq!((row.title(), row.field()), (title, field));
        // *Fetch* is not asked for on the keyboard.
        assert_eq!(native::Row::of(field).is_some(), !field.is_empty());
    }
    for field in ["", "Server", "other", "fetch"] {
        assert_eq!(native::Row::of(field), None, "{field:?}");
    }
}

#[test]
fn the_lists_are_read_and_written() {
    let list =
        native::articles("7\t/books/Wallabag/a.epub\n9\t/books/Wallabag/b (9).epub\n").unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(
        (list[1].id.as_str(), list[1].path.as_str()),
        ("9", "/books/Wallabag/b (9).epub")
    );
    // Lines of another shape are left out; a tab in a path stays in it.
    let list = native::articles("7\tx\nno tab\n\n\t\n9\ty\ttab\n").unwrap();
    let ids: Vec<_> = list.iter().map(|a| a.id.as_str()).collect();
    assert_eq!(ids, ["7", "", "9"]);
    assert_eq!(list[2].path, "y\ttab");
    // Lines end at `\n` or `\r\n`.
    assert_eq!(native::articles("7\tx\r\n").unwrap()[0].path, "x");
    assert_eq!(native::articles_text(&list[..1]).unwrap(), "7\tx\n");
    assert_eq!(native::finished("7\n\n9\n").unwrap(), ["7", "9"]);
    // A list too large is not an empty one.
    let big = "7\tx\n".repeat(native::ARTICLES_CAP / 4 + 1);
    assert_eq!(
        native::articles(&big).unwrap_err(),
        paperpaw_sdk::native::Error::NoSpace
    );
    let big = "7\n".repeat(native::FINISHED_CAP / 2 + 1);
    assert_eq!(
        native::finished(&big).unwrap_err(),
        paperpaw_sdk::native::Error::NoSpace
    );
}

#[test]
fn an_article_counts_as_read_on_its_last_page_or_near_the_end() {
    assert_eq!(native::READ_PPM, 950_000);
    for (page, pages, ppm, read) in [
        (Some(5), Some(5), 0, true),
        (Some(6), Some(5), 0, true),
        (Some(4), Some(5), 1_000_000, false),
        // Pages not counted: by progress.
        (None, Some(5), 949_999, false),
        (None, Some(5), 950_000, true),
        (Some(5), None, 950_000, true),
        (None, None, 1_000_000, true),
        (None, None, 0, false),
    ] {
        assert_eq!(
            native::is_read(page, pages, ppm),
            read,
            "{page:?} {pages:?} {ppm}"
        );
    }
}

#[test]
fn refused_allocations_end_in_no_memory() {
    let text = "7\t/books/Wallabag/a.epub\n9\t/books/Wallabag/b (9).epub\n";
    sweep(|| native::articles(text).map(|list| list.len()));
    sweep(|| native::finished("7\n9\n11\n").map(|list| list.len()));
    let list = [native::Article {
        id: "7".into(),
        path: "/b/a.epub".into(),
    }];
    sweep(|| native::articles_text(&list).map(|t| t.len()));
}
