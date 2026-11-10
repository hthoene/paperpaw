//! The document cache and the pictures of a document on the production
//! document host: the cache folder, its stamp, a replaced book, and a format's
//! pictures as the opener's image ids.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
extern crate std;
use super::*;

fn conversion_of(folder: &str) -> &str {
    folder.rsplit('/').next().unwrap()
}

#[test]
fn an_open_has_its_cache_folder_and_a_finished_open_is_stamped_for_its_conversion() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        w.makes_cache.set(true);
        let close = |document: u32| {
            w.call(
                AdmittedFeatures::NONE,
                18,
                &[Record::ServiceDocRef(wire::ServiceDocRef { document })],
            )
            .unwrap();
        };
        close(w.open(1, "books/a.epub").unwrap());
        let folder = seen_cache(w, 0);
        assert!(folder.starts_with(".paperpaw/cache/"), "{folder}");
        assert!(folder.contains("/epub/"), "{folder}");
        assert_eq!(conversion_of(&folder).len(), 16);
        // The format answered ready: the stamp is made.
        assert!(
            w.card
                .get(&std::format!(
                    "{folder}/{}",
                    crate::native_host::doc_cache::STAMP
                ))
                .is_some()
        );
        // The finished cache is kept for the next open of the same conversion.
        close(w.open(1, "books/a.epub").unwrap());
        assert_eq!(seen_cache(w, 1), folder);
        assert!(w.card.get(&std::format!("{folder}/pages/1")).is_some());
        // A new version of the format makes it stale: it is made again.
        w.version.set("2.0.0");
        w.makes_cache.set(false);
        close(w.open(1, "books/a.epub").unwrap());
        assert!(w.card.get(&std::format!("{folder}/pages/1")).is_none());
        // Another book has another cache.
        close(w.open(1, "books/b.epub").unwrap());
        assert_ne!(seen_cache(w, 3), folder);
    });
}

#[test]
fn a_half_made_cache_is_removed_at_the_next_open_and_a_ready_open_that_made_none_is_not_stamped() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        let close = |document: u32| {
            w.call(
                AdmittedFeatures::NONE,
                18,
                &[Record::ServiceDocRef(wire::ServiceDocRef { document })],
            )
            .unwrap();
        };
        close(w.open(1, "books/a.epub").unwrap());
        let folder = seen_cache(w, 0);
        assert!(
            w.card
                .get(&std::format!(
                    "{folder}/{}",
                    crate::native_host::doc_cache::STAMP
                ))
                .is_none()
        );
        // A cache of a cut preparation: pages and no stamp.
        w.card
            .write(&std::format!("{folder}/pages/9"), b"old")
            .unwrap();
        close(w.open(1, "books/a.epub").unwrap());
        assert!(w.card.get(&std::format!("{folder}/pages/9")).is_none());
    });
}

/// A reply of one `fmt.image`: the cover, a gray picture of 8 x 4 at `path`
/// of the document's cache.
fn cover_reply(path: &str) -> Vec<u8> {
    let mut bytes = vec![0_u8; 3840];
    let n = local::reply_encode(
        &[local::Record::FmtImage(local::FmtImage::new(
            path,
            u32::MAX,
            8,
            4,
            0,
        ))],
        &mut bytes,
    )
    .unwrap();
    bytes.truncate(n);
    bytes
}

fn gray_picture(w: &World<'_>, path: &str) {
    use paperpaw_sdk::image::{GrayWriter, ImageSink};
    let mut writer = GrayWriter::new(&*w.card, path);
    writer.start(8, 4).unwrap();
    for _ in 0..4 {
        writer.row(&[9_u8; 8]).unwrap();
    }
    writer.finish().unwrap();
}

fn ask_cover(w: &World<'_>, document: u32) -> Result<wire::ServiceDocImage, i32> {
    let reply = w.call(
        AdmittedFeatures::NONE,
        15,
        &[Record::ServiceDocRef(wire::ServiceDocRef { document })],
    )?;
    match wire::record_decode_one(&reply).unwrap() {
        Record::ServiceDocImage(image) => Ok(image),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_format_picture_is_the_openers_image_id_until_its_document_ends() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        let document = w.open(1, "books/a.epub").unwrap();
        let cache = seen_cache(w, 0);
        gray_picture(w, &std::format!("{cache}/pic.gray"));
        *w.reply.borrow_mut() = Ok(cover_reply("pic.gray"));
        let image = ask_cover(w, document).unwrap();
        assert_ne!(image.image, 0);
        assert_eq!((image.width, image.height, image.index), (8, 4, u32::MAX));
        assert_eq!(w.hosts.images().info(OPENER, image.image).unwrap().width, 8);
        // Another plugin's id space does not have it.
        assert_eq!(
            w.hosts.images().info(OPENER + 1, image.image),
            Err(ERR_NOT_FOUND)
        );
        // The document closes: its picture's id ends with it.
        w.call(
            AdmittedFeatures::NONE,
            18,
            &[Record::ServiceDocRef(wire::ServiceDocRef { document })],
        )
        .unwrap();
        assert_eq!(
            w.hosts.images().info(OPENER, image.image),
            Err(ERR_NOT_FOUND)
        );
    });
}

#[test]
fn a_picture_the_cache_does_not_hold_or_a_hostile_one_is_left_out() {
    use paperpaw_sdk::image::{GrayWriter, ImageSink};
    // One case a world: three failed answers in a row close a document.
    let cases = [
        "missing.gray",
        "../x.gray",
        ".host/x.gray",
        "junk.gray",
        "big.gray",
    ];
    for path in cases {
        world(|w| {
            w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
            let document = w.open(1, "books/a.epub").unwrap();
            let cache = seen_cache(w, 0);
            w.card
                .write(&std::format!("{cache}/junk.gray"), b"not a picture")
                .unwrap();
            // The header says another size than the format does.
            let big = std::format!("{cache}/big.gray");
            let mut writer = GrayWriter::new(&*w.card, &big);
            writer.start(9, 4).unwrap();
            for _ in 0..4 {
                writer.row(&[1_u8; 9]).unwrap();
            }
            writer.finish().unwrap();
            *w.reply.borrow_mut() = Ok(cover_reply(path));
            // No cover at all (`document.md` op 15: `ERR_NOT_FOUND` for none; a
            // picture that fails its check is a failed read, not a bad request).
            assert_eq!(ask_cover(w, document).err(), Some(ERR_NOT_FOUND), "{path}");
            // The document is not struck for it, however often it is asked
            // (a malformed answer ends it after a few).
            for _ in 0..16 {
                assert_eq!(ask_cover(w, document).err(), Some(ERR_NOT_FOUND), "{path}");
            }
        });
    }
    // An absolute name is a malformed answer: the call fails.
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        let document = w.open(1, "books/a.epub").unwrap();
        *w.reply.borrow_mut() = Ok(cover_reply("/abs.gray"));
        assert!(ask_cover(w, document).is_err());
    });
}

#[test]
fn the_pictures_of_a_document_the_host_ended_end_at_once() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        let document = w.open(1, "books/a.epub").unwrap();
        gray_picture(w, &std::format!("{}/pic.gray", seen_cache(w, 0)));
        *w.reply.borrow_mut() = Ok(cover_reply("pic.gray"));
        let image = ask_cover(w, document).unwrap().image;
        // The host ends the document (the card went): no later call is
        // needed, a stale id must not draw what a new cache holds.
        w.hosts.end_document_by_host(document, 2).unwrap();
        assert_eq!(w.hosts.images().info(OPENER, image), Err(ERR_NOT_FOUND));
    });
}

#[test]
fn the_pictures_of_a_document_ended_out_of_reach_of_the_table_end_with_the_next_call() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        let document = w.open(1, "books/a.epub").unwrap();
        gray_picture(w, &std::format!("{}/pic.gray", seen_cache(w, 0)));
        *w.reply.borrow_mut() = Ok(cover_reply("pic.gray"));
        let image = ask_cover(w, document).unwrap().image;
        // Out of reach of the table (the lifecycle alone): the next call to
        // the service ends the ids.
        doc_service::end_by_host(&*w.state, document, 2).unwrap();
        let _ = w.call(
            AdmittedFeatures::NONE,
            19,
            &[Record::ServiceDocPage(wire::ServiceDocPage {
                document: 0,
                scope: "",
                cursor: 0,
                generation: 0,
            })],
        );
        assert_eq!(w.hosts.images().info(OPENER, image), Err(ERR_NOT_FOUND));
    });
}

#[test]
fn a_refused_allocation_while_mapping_a_picture_is_no_memory_and_the_next_ask_works() {
    for nth in 1..=40 {
        world(|w| {
            w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
            let document = w.open(1, "books/a.epub").unwrap();
            gray_picture(w, &std::format!("{}/pic.gray", seen_cache(w, 0)));
            *w.reply.borrow_mut() = Ok(cover_reply("pic.gray"));
            w.fail.set(nth);
            let first = ask_cover(w, document);
            w.fail.set(0);
            assert!(
                matches!(first, Ok(_) | Err(ERR_NO_MEMORY)),
                "allocation {nth}: {first:?}"
            );
            // Nothing is half-made: the same picture is asked for again.
            assert!(ask_cover(w, document).is_ok(), "allocation {nth}");
        });
    }
}

/// Opens `path` (with the CSS file `css` of root 1 when it is not empty) and
/// closes the document again; the folder of its cache is the format's call
/// number `at`.
fn open_and_close(w: &World<'_>, path: &str, css: &str) {
    let document = if css.is_empty() {
        w.open(1, path)
    } else {
        w.open_with_css(1, path, 1, css)
    }
    .unwrap();
    w.call(
        AdmittedFeatures::NONE,
        18,
        &[Record::ServiceDocRef(wire::ServiceDocRef { document })],
    )
    .unwrap();
}

#[test]
fn a_book_replaced_at_its_path_or_an_edited_css_file_makes_the_cache_again() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        w.makes_cache.set(true);
        w.card.write("books/a.epub", b"first edition").unwrap();
        w.card.write("style.css", b"p { color: red }").unwrap();
        open_and_close(w, "books/a.epub", "style.css");
        let folder = seen_cache(w, 0);
        let page = std::format!("{folder}/pages/1");
        w.card.write(&page, b"old text").unwrap();
        // The same book and CSS file: the finished cache is used as it is.
        w.makes_cache.set(false);
        open_and_close(w, "books/a.epub", "style.css");
        assert_eq!(w.card.get(&page).as_deref(), Some(&b"old text"[..]));
        // Another edition of the book at the same path, even of the same size.
        w.card.write("books/a.epub", b"FIRST EDITION").unwrap();
        open_and_close(w, "books/a.epub", "style.css");
        assert!(w.card.get(&page).is_none(), "the old book's cache is used");
        // The cache of the new edition is finished in turn; then the CSS file
        // is edited (other bytes, same size).
        w.makes_cache.set(true);
        open_and_close(w, "books/a.epub", "style.css");
        w.card.write(&page, b"new text").unwrap();
        w.makes_cache.set(false);
        open_and_close(w, "books/a.epub", "style.css");
        assert_eq!(w.card.get(&page).as_deref(), Some(&b"new text"[..]));
        w.card.write("style.css", b"p { color: RED }").unwrap();
        open_and_close(w, "books/a.epub", "style.css");
        assert!(w.card.get(&page).is_none(), "the old CSS's cache is used");
    });
}

#[test]
fn a_finished_cache_found_at_the_open_keeps_its_stamp() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        w.makes_cache.set(true);
        w.card.write("books/a.epub", b"book").unwrap();
        open_and_close(w, "books/a.epub", "");
        let folder = seen_cache(w, 0);
        let stamp = std::format!("{folder}/{}", crate::native_host::doc_cache::STAMP);
        let first = w.card.get(&stamp).unwrap();
        // One rename made it (from the host's folder); finding it current at
        // the next opens makes no other: the host does not make it again.
        assert_eq!(w.renames.get(), 1);
        open_and_close(w, "books/a.epub", "");
        open_and_close(w, "books/a.epub", "");
        assert_eq!(w.renames.get(), 1);
        assert_eq!(w.card.get(&stamp).unwrap(), first);
    });
}

#[test]
fn a_cache_that_ran_out_of_room_is_removed_with_what_was_counted() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        w.makes_cache.set(true);
        w.card.write("books/a.epub", b"book").unwrap();
        *w.reply.borrow_mut() = Err(ERR_NO_SPACE);
        assert_eq!(w.open(1, "books/a.epub"), Err(ERR_NO_SPACE));
        let folder = seen_cache(w, 0);
        assert!(!folder.is_empty());
        assert!(w.card.get(&std::format!("{folder}/pages/1")).is_none());
        assert_eq!(w.hosts.format_scope().total(&folder), 0);
    });
}

#[test]
fn what_a_format_wrote_after_the_stamp_is_in_the_stamp_when_its_last_document_closes() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        w.makes_cache.set(true);
        w.card.write("books/a.epub", b"book").unwrap();
        let id = w.open(1, "books/a.epub").unwrap();
        let folder = seen_cache(w, 0);
        let stamp = std::format!("{folder}/{}", crate::native_host::doc_cache::STAMP);
        let bytes = |w: &World<'_>| {
            let text = w.card.get(&stamp).unwrap();
            let line = text.rsplit(|b| *b == b'\n').nth(1).unwrap().to_vec();
            std::string::String::from_utf8(line).unwrap()
        };
        // The format answered ready with nothing counted; it writes on later.
        assert_eq!(bytes(w), "0");
        let scope = w.hosts.format_scope();
        scope.charge(&folder, 700, 0, u64::MAX).unwrap();
        // A second document of the same cache keeps the count.
        let second = w.open(1, "books/a.epub").unwrap();
        let close = |document: u32| {
            w.call(
                AdmittedFeatures::NONE,
                18,
                &[Record::ServiceDocRef(wire::ServiceDocRef { document })],
            )
            .unwrap();
        };
        close(id);
        assert_eq!((bytes(w), scope.total(&folder)), ("0".into(), 700));
        close(second);
        // The last one closed: the stamp says it, the count starts again
        // from the stamp at the next open.
        assert_eq!((bytes(w), scope.total(&folder)), ("700".into(), 0));
        open_and_close(w, "books/a.epub", "");
        assert_eq!(bytes(w), "700");
    });
}
