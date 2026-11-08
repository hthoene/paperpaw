//! The native decisions (`native_wallabag`) against the interface-6 ones
//! (`lib.rs`): equal on the same input, and refusing instead of aborting
//! when memory runs short.
use crate::fail_alloc::sweep;
use crate::native_wallabag as native;
use crate::{Article, MAX_ARTICLES, READ, Row, articles, data, finished, is_id};
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::MemoryStorage;

#[test]
fn ids_are_those_of_interface_6() {
    for id in [
        "",
        "0",
        "7",
        "0012",
        "12345678901234567890",
        "123456789012345678901",
        "-1",
        "1.5",
        "a",
        " 1",
        "1 ",
        "٣",
        "9\n",
    ] {
        assert_eq!(native::is_id(id), is_id(id), "{id:?}");
    }
    assert_eq!(native::MAX_ARTICLES, MAX_ARTICLES);
}

#[test]
fn the_rows_are_those_of_interface_6() {
    assert_eq!(native::ROWS.len(), 6);
    for (old, new) in [
        (Row::Server, native::Row::Server),
        (Row::User, native::Row::User),
        (Row::Password, native::Row::Password),
        (Row::Client, native::Row::Client),
        (Row::Secret, native::Row::Secret),
        (Row::Fetch, native::Row::Fetch),
    ] {
        assert_eq!((old.title(), old.field()), (new.title(), new.field()));
    }
    for field in [
        "server", "user", "password", "client", "secret", "", "fetch", "Server",
    ] {
        assert_eq!(
            Row::of(field).map(Row::title),
            native::Row::of(field).map(native::Row::title),
            "{field:?}"
        );
    }
}

#[test]
fn the_lists_are_read_as_before() {
    for text in [
        "",
        "7\t/books/Wallabag/a.epub\n9\t/books/Wallabag/b (9).epub\n",
        "7\tx\nno tab\n\n\t\n9\ty\ttab\n",
        "7\tx\r\n8\ty\r\n",
        "\n\n7\tx",
    ] {
        let storage = MemoryStorage::new().with(&data("articles.txt"), text);
        let old = articles(&storage).unwrap();
        let new = native::articles(text).unwrap();
        assert_eq!(old.len(), new.len(), "{text:?}");
        for (o, n) in old.iter().zip(&new) {
            assert_eq!((&o.id, &o.path), (&n.id, &n.path));
        }
        let storage = MemoryStorage::new().with(&data("finished.txt"), text);
        assert_eq!(
            finished(&storage).unwrap(),
            native::finished(text).unwrap(),
            "{text:?}"
        );
    }
    let list: Vec<Article> = (0..3)
        .map(|i| Article {
            id: format!("{i}"),
            path: format!("/b/{i}.epub"),
        })
        .collect();
    let new: Vec<native::Article> = list
        .iter()
        .map(|a| native::Article {
            id: a.id.clone(),
            path: a.path.clone(),
        })
        .collect();
    let old_text: String = list
        .iter()
        .map(|a| format!("{}\t{}\n", a.id, a.path))
        .collect();
    assert_eq!(native::articles_text(&new).unwrap(), old_text);
}

#[test]
fn an_article_counts_as_read_as_before() {
    assert_eq!(native::READ_PPM, (READ * 10_000.0) as u32);
    // The last page, or near the end where pages are not counted.
    for (page, pages) in [
        (Some(5), Some(5)),
        (Some(4), Some(5)),
        (Some(6), Some(5)),
        (None, Some(5)),
        (Some(5), None),
        (None, None),
    ] {
        for percent in [0.0f32, 50.0, 94.9, 95.0, 95.1, 100.0] {
            let old = match (page, pages) {
                (Some(page), Some(pages)) => page >= pages,
                _ => f64::from(percent) >= READ,
            };
            let ppm = (percent * 10_000.0).round() as u32;
            assert_eq!(
                native::is_read(page, pages, ppm),
                old,
                "{page:?} {pages:?} {percent}"
            );
        }
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
