//! Comic book archives: pages in the natural order of their names
//! whatever the archive's order, files that are no pictures left out,
//! empty and damaged archives refused, odd `ComicInfo.xml`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::Recording;
use paperpaw_plugin_format_epub::CbzDocument;
use paperpaw_sdk::MemoryStorage;
use paperpaw_sdk::document::{BlockKind, Document, DocumentError, Locator};
use paperpaw_zip::testing::ZipBuilder;
use proptest::prelude::*;

const PNG: &[u8] = include_bytes!("../../../crates/paperpaw-image/tests/data/gray.png");

/// A comic of the files `names` (all with data `data`) in that order.
fn comic(names: &[&str], data: &[u8]) -> Vec<u8> {
    let mut builder = ZipBuilder::new();
    for name in names {
        builder = builder.stored(name, data.to_vec());
    }
    builder.build()
}

fn open(zip: Vec<u8>) -> (MemoryStorage, Result<CbzDocument, DocumentError>) {
    let storage = MemoryStorage::new().with("c.cbz", zip);
    let doc = CbzDocument::open(&storage, "c.cbz");
    (storage, doc)
}

/// The pages' names: the text of pages whose pictures cannot be shown.
fn page_names(doc: &CbzDocument, storage: &MemoryStorage) -> Vec<String> {
    (0..doc.sections())
        .map(|s| {
            doc.block_at(storage, Locator::new(s, 0))
                .unwrap()
                .unwrap()
                .text
        })
        .collect()
}

#[test]
fn pages_come_in_natural_order() {
    let names = [
        "p10.jpg",
        "p9.jpg",
        "p1.jpg",
        "P2.JPG",
        "p01b.png",
        "cover.jpeg",
        "a/b/0.gif",
        "p100.jpg",
        "p0010x.jpg",
        "p18446744073709551616999.jpg",
    ];
    let (storage, doc) = open(comic(&names, b"not a picture"));
    let doc = doc.unwrap();
    assert_eq!(
        page_names(&doc, &storage),
        [
            "a/b/0.gif",
            "cover.jpeg",
            "p1.jpg",
            "p01b.png",
            "P2.JPG",
            "p9.jpg",
            "p10.jpg",
            "p0010x.jpg",
            "p100.jpg",
            "p18446744073709551616999.jpg",
        ]
    );
}

#[test]
fn files_that_are_no_pictures_are_left_out() {
    let names = [
        "readme.txt",
        "ComicInfo.xml",
        ".DS_Store",
        "__MACOSX/._1.jpg",
        "__macosx/2.jpg",
        "dir/.hidden.png",
        "folder/",
        "1.jpg.txt",
        "jpg",
        "2.webp",
        "3.png",
    ];
    let (storage, doc) = open(comic(&names, b"x"));
    let doc = doc.unwrap();
    assert_eq!(page_names(&doc, &storage), ["3.png"]);
}

#[test]
fn comics_without_pictures_are_refused() {
    for zip in [
        ZipBuilder::new().build(),
        comic(&["notes.txt", "ComicInfo.xml"], b"x"),
        comic(&["__MACOSX/1.png"], PNG),
    ] {
        let (_, doc) = open(zip);
        assert!(matches!(doc, Err(DocumentError::Malformed(_))));
    }
    for bytes in [&b""[..], b"PK", b"PK\x05\x06", b"not a zip at all"] {
        let (_, doc) = open(bytes.to_vec());
        assert!(doc.is_err());
    }
}

#[test]
fn pictures_that_decode_are_shown_the_others_named() {
    let zip = ZipBuilder::new()
        .stored("1.png", PNG.to_vec())
        .stored("2.png", PNG[..PNG.len() / 2].to_vec())
        .stored("3.png", Vec::new())
        .build();
    let (storage, doc) = open(zip);
    let doc = doc.unwrap();
    let kinds: Vec<(BlockKind, String)> = (0..3)
        .map(|s| {
            let b = doc.block_at(&storage, Locator::new(s, 0)).unwrap().unwrap();
            (b.kind, b.text)
        })
        .collect();
    assert_eq!(kinds[0].0, BlockKind::Image);
    assert_eq!(kinds[1], (BlockKind::Paragraph, "2.png".into()));
    assert_eq!(kinds[2], (BlockKind::Paragraph, "3.png".into()));
    assert!(doc.cover(&storage).unwrap().is_some());
    // Past the pages, or past a page's one block: nothing.
    assert_eq!(doc.block_at(&storage, Locator::new(3, 0)).unwrap(), None);
    assert_eq!(doc.block_at(&storage, Locator::new(0, 1)).unwrap(), None);
    assert!(doc.whole_pages());
}

#[test]
fn page_names_climbing_out_stay_in_the_cache() {
    let zip = comic(&["../../../../evil.png", "/abs/1.png", "C:\\x\\2.png"], PNG);
    let storage = Recording::new(MemoryStorage::new().with("c.cbz", zip));
    let doc = CbzDocument::open(&storage, "c.cbz").unwrap();
    for s in 0..doc.sections() {
        doc.block_at(&storage, Locator::new(s, 0)).unwrap();
    }
    assert_eq!(storage.written_outside(&[]), Vec::<String>::new());
}

#[test]
fn odd_comic_info_is_read_or_ignored() {
    for (info, title) in [
        ("<ComicInfo><Title>  Spaced  </Title></ComicInfo>", "Spaced"),
        (
            "<ComicInfo><Series>S</Series><Number>3</Number></ComicInfo>",
            "S 3",
        ),
        (
            "<ComicInfo><Series>S</Series><Title>S: Already</Title></ComicInfo>",
            "S: Already",
        ),
        (
            "<ComicInfo><Title></Title><Writer>,,,</Writer></ComicInfo>",
            "c",
        ),
        ("<<<not xml", "c"),
        ("", "c"),
    ] {
        let zip = ZipBuilder::new()
            .stored("1.png", b"x".to_vec())
            .deflated("comicinfo.XML", info.as_bytes())
            .build();
        let (_, doc) = open(zip);
        let doc = doc.unwrap();
        assert_eq!(doc.metadata().title.as_deref(), Some(title), "{info}");
        assert!(doc.metadata().authors.iter().all(|a| !a.is_empty()));
    }
    // A huge one is read only in part.
    let huge = format!(
        "<ComicInfo><Title>Big</Title><Summary>{}</Summary></ComicInfo>",
        "word ".repeat(100_000)
    );
    let zip = ZipBuilder::new()
        .stored("1.png", b"x".to_vec())
        .deflated("ComicInfo.xml", huge.as_bytes())
        .build();
    let (_, doc) = open(zip);
    assert_eq!(doc.unwrap().metadata().title.as_deref(), Some("Big"));
}

#[test]
fn many_pages_are_capped() {
    let names: Vec<String> = (0..5100).map(|i| format!("{i}.png")).collect();
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    let (_, doc) = open(comic(&names, b""));
    assert_eq!(doc.unwrap().sections(), 5000);
}

/// Characters of generated page names: digits (leading zeros among
/// them), letters of either case, separators; never a hidden file.
const NAME: &[char] = &['0', '1', '2', '9', 'a', 'B', 'z', '_', '-', ' ', 'é'];

proptest! {
    #![proptest_config(common::cases(128))]

    #[test]
    fn any_names_sort_without_panic(names in prop::collection::btree_set(
        prop::collection::vec(prop::sample::select(NAME), 1..12)
            .prop_map(|c| c.into_iter().collect::<String>() + ".png"),
        1..40,
    )) {
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        let (storage, doc) = open(comic(&names, b""));
        let doc = doc.unwrap();
        let mut found = page_names(&doc, &storage);
        found.sort();
        let mut expected: Vec<String> = names.iter().map(|n| (*n).to_string()).collect();
        expected.sort();
        prop_assert_eq!(found, expected);
    }

    #[test]
    fn page_order_does_not_depend_on_the_archives_order(
        names in prop::collection::btree_set(
            // Lower case and no leading zeros: no two names are equal
            // in natural order.
            "[a-c]{0,2}[1-9][0-9]{0,3}[a-c]{0,2}\\.png",
            1..30,
        ),
        rotate in any::<prop::sample::Index>(),
    ) {
        let forward: Vec<&str> = names.iter().map(String::as_str).collect();
        let mut shuffled = forward.clone();
        shuffled.reverse();
        let by = rotate.index(shuffled.len());
        shuffled.rotate_left(by);
        let (s1, d1) = open(comic(&forward, b""));
        let (s2, d2) = open(comic(&shuffled, b""));
        prop_assert_eq!(page_names(&d1.unwrap(), &s1), page_names(&d2.unwrap(), &s2));
    }
}
