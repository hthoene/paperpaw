#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
//! The library's cover through `core.document`: opened as the caller on root 1
//! whatever its features, the format asked for the cover, the document closed
//! again, the picture the caller's own.
use super::*;
use crate::native_host::{
    admitted_features::AdmittedFeatures,
    doc_service::cover::make as make_cover,
    host_services::Caller,
    image_service::{LIBRARY_COVER, Origin},
};

const BOOK: &str = "books/a.epub";

fn caller<'a>(view: &'a View<'_>) -> Caller<'a> {
    Caller {
        owner: OWNER,
        plugin: "home",
        features: AdmittedFeatures::NONE,
        hosts: &view.hosts,
    }
}

fn picture(width: u32) -> Vec<u8> {
    stream(&[Record::ServiceDocImage(Image {
        image: 0,
        index: u32::MAX,
        width,
        height: 50,
        flags: 0,
    })])
}

/// What the document service's image translation would have made for
/// `document`: a picture of the opener at `file`.
fn mapped(view: &View<'_>, fake: &Fake, document: u32, file: &str) -> u32 {
    let id = view
        .hosts
        .images()
        .add(
            OWNER,
            Origin::Document {
                document,
                index: u32::MAX,
            },
            file,
            100,
            50,
            false,
        )
        .unwrap();
    fake.image.set(Some(Some(id)));
    id
}

#[test]
fn a_cover_is_made_for_a_caller_without_uses_storage_and_the_document_is_closed() {
    let fake = Fake::new(Limits::X4);
    // Root 0 only: root 1 is not the caller's to open.
    fake.roots.set(0b01);
    with(&fake, |view| {
        let id = mapped(view, &fake, 1, "cache/a/cover.gray");
        fake.queue
            .borrow_mut()
            .extend([Ok(state_answer(0, 0)), Ok(picture(100))]);
        let image = make_cover(view, &caller(view), BOOK, 0).unwrap();
        assert_eq!((image.image, image.index, image.width), (id, u32::MAX, 100));
        // Open (2), cover (17) at the default side, then the format's close.
        let calls = fake.calls.borrow();
        assert_eq!((calls[0][0], calls[1][0]), (fmt::OPEN, fmt::COVER));
        assert_eq!(calls[1][7], 300);
        assert_eq!(fake.readers.borrow()[0], (1, 1, BOOK.into()));
        assert_eq!(*fake.ended.borrow(), [1]);
        assert_eq!(*fake.released.borrow(), [1]);
        // The picture outlives the document that made it.
        view.hosts.images().document_closed(1).unwrap();
        let info = view.hosts.images().info(OWNER, id).unwrap();
        assert_eq!((info.width, info.height, info.index), (100, 50, u32::MAX));
    });
}

#[test]
fn the_side_asked_for_is_the_covers_side() {
    let fake = Fake::new(Limits::X4);
    with(&fake, |view| {
        mapped(view, &fake, 1, "cache/a/cover.gray");
        fake.queue
            .borrow_mut()
            .extend([Ok(state_answer(0, 0)), Ok(picture(100))]);
        make_cover(view, &caller(view), BOOK, 640).unwrap();
        assert_eq!(fake.calls.borrow()[1][7], 640);
    });
}

#[test]
fn a_book_asked_for_again_has_one_picture_and_one_id() {
    let fake = Fake::new(Limits::X4);
    with(&fake, |view| {
        let first = mapped(view, &fake, 1, "cache/a/cover.gray");
        fake.queue
            .borrow_mut()
            .extend([Ok(state_answer(0, 0)), Ok(picture(100))]);
        assert_eq!(
            make_cover(view, &caller(view), BOOK, 0).unwrap().image,
            first
        );
        // The second call's document is 2: its translation made another id for
        // the same file, which ends; the caller keeps the one it has.
        let second = mapped(view, &fake, 2, "cache/a/cover.gray");
        assert_ne!(first, second);
        fake.queue
            .borrow_mut()
            .extend([Ok(state_answer(0, 0)), Ok(picture(100))]);
        let image = make_cover(view, &caller(view), BOOK, 0).unwrap();
        assert_eq!(image.image, first);
        assert!(view.hosts.images().info(OWNER, second).is_err());
        assert!(view.hosts.images().info(OWNER, first).is_ok());
        // Another file is another picture.
        let other = mapped(view, &fake, 3, "cache/b/cover.gray");
        fake.queue
            .borrow_mut()
            .extend([Ok(state_answer(0, 0)), Ok(picture(100))]);
        assert_eq!(
            make_cover(view, &caller(view), "books/b.epub", 0)
                .unwrap()
                .image,
            other
        );
        assert_eq!(
            view.hosts.images().info(OWNER, other).unwrap().index,
            u32::MAX
        );
        let _ = LIBRARY_COVER;
    });
}

#[test]
fn a_book_without_a_cover_is_not_found_and_the_document_is_closed() {
    let fake = Fake::new(Limits::X4);
    with(&fake, |view| {
        fake.queue
            .borrow_mut()
            .extend([Ok(state_answer(0, 0)), Err(ERR_NOT_FOUND)]);
        assert_eq!(
            make_cover(view, &caller(view), BOOK, 0).unwrap_err(),
            ERR_NOT_FOUND
        );
        assert_eq!(*fake.released.borrow(), [1]);
        assert_eq!(*fake.ended.borrow(), [1]);
    });
}

#[test]
fn a_format_still_preparing_or_that_failed_has_no_cover_now() {
    // A failed document carries its ERR_* (`check::state`).
    for (state, status, expected) in [(1, 0, ERR_NOT_FOUND), (2, ERR_IO, ERR_NOT_FOUND)] {
        let fake = Fake::new(Limits::X4);
        with(&fake, |view| {
            fake.queue
                .borrow_mut()
                .push(Ok(state_answer(state, status)));
            assert_eq!(
                make_cover(view, &caller(view), BOOK, 0).unwrap_err(),
                expected
            );
            // Never asked for a cover, never left open.
            assert_eq!(fake.calls.borrow().len(), 1);
            assert_eq!(*fake.released.borrow(), [1]);
        });
    }
}

#[test]
fn a_format_that_does_not_open_is_the_formats_status_and_nothing_stays_open() {
    let fake = Fake::new(Limits::X4);
    with(&fake, |view| {
        fake.queue.borrow_mut().push(Err(ERR_IO));
        assert_eq!(
            make_cover(view, &caller(view), BOOK, 0).unwrap_err(),
            ERR_IO
        );
        assert!(view.fake.table.close(OWNER, 1).is_err(), "no open document");
    });
}

#[test]
fn no_place_in_the_callers_table_is_full_for_the_library_host_to_call_busy() {
    let fake = Fake::new(Limits::X4);
    with(&fake, |view| {
        // The caller holds the one place an opener may take on the X4.
        let held = opened(&fake);
        assert_eq!(held, 1);
        fake.queue.borrow_mut().push(Ok(state_answer(0, 0)));
        assert_eq!(
            make_cover(view, &caller(view), BOOK, 0).unwrap_err(),
            ERR_FULL
        );
        assert_eq!(fake.calls.borrow().len(), 1, "only the first open");
    });
}

#[test]
fn a_host_without_the_document_service_has_no_covers() {
    let fake = Fake::new(Limits::X4);
    with(&fake, |view| {
        let none = crate::native_host::host_services::HostServices::new();
        let who = Caller {
            hosts: &none,
            ..caller(view)
        };
        assert_eq!(make_cover(view, &who, BOOK, 0).unwrap_err(), ERR_STATE);
    });
}

/// The two numbers a cover goes by, pinned to the doc tables so neither can
/// be flipped to make a test pass: `core.document` op 15 (host) and the
/// format's own op 17 (`docs/abi/document.md`, the op table of 1 and of 3).
#[test]
fn the_cover_ops_are_the_documents_numbers() {
    use crate::native_host::doc_service::{fmt, op};
    const DOC: &str = include_str!("../../../../../../docs/abi/document.md");
    let row = |number: u32, name: &str| {
        let head = std::format!("| {number} | `{name}`");
        DOC.lines().any(|line| line.starts_with(&head))
    };
    assert_eq!((op::COVER, fmt::COVER), (15, 17));
    assert!(row(op::COVER, "cover"), "the host op table");
    assert!(row(fmt::COVER, "cover"), "the format op table");
    assert_eq!(schema_op(op::COVER).name, "cover");
}

/// The library host's `cover`: the catalog's path for the book, the cover made
/// through the caller's frame, a book that is not in the catalog never opened.
#[test]
fn the_library_host_makes_the_cover_of_a_catalog_book() {
    use crate::native_host::{
        lib_cover::CoverCx,
        lib_host::LibraryHost,
        lib_reads::tests::card,
        lib_service::{BookRef, Caller as Who, LibQuery, LibWrite},
    };
    let card = card();
    let library = LibraryHost::new(card.storage.clone(), card.env.clone(), None);
    let fake = Fake::new(Limits::X4);
    with(&fake, |view| {
        let id = mapped(view, &fake, 1, "cache/a/cover.gray");
        fake.queue
            .borrow_mut()
            .extend([Ok(state_answer(0, 0)), Ok(picture(100))]);
        let who = Who {
            owner: OWNER,
            plugin: "home",
            storage: Err(ERR_DENIED),
        };
        let host = caller(view);
        let cx = CoverCx {
            frame: view,
            caller: &host,
        };
        let mut got = None;
        library
            .cover(&who, BookRef::Path(BOOK), 0, &cx, &mut |record| {
                if let Record::ServiceDocImage(image) = record {
                    got = Some(image.image);
                }
            })
            .unwrap();
        assert_eq!(got, Some(id));
        assert_eq!(fake.readers.borrow()[0], (1, 1, BOOK.into()));
        // A format that is not running yet is busy and the caller is told to
        // ask again (bit 6, one change for that book).
        fake.fails.set(Some(ERR_BUSY));
        let busy = library.cover(&who, BookRef::Path(BOOK), 0, &cx, &mut |_| {});
        assert_eq!(busy, Err(ERR_BUSY));
        fake.fails.set(None);
        let mut told = None;
        assert!(library.take_changed(&mut |what, _, path| {
            told = Some((what, std::string::String::from(path)));
        }));
        assert_eq!(told, Some((1 << 6, std::string::String::from(BOOK))));
        // Asked again while the format is still not running: busy again, and
        // the one change for the book is raised again (a format that starts
        // ends it; a caller is not looped by anything else).
        fake.fails.set(Some(ERR_BUSY));
        // (The first busy above raised once already.)
        let mut raised = 1;
        for _ in 0..12 {
            let again = library.cover(&who, BookRef::Path(BOOK), 0, &cx, &mut |_| {});
            assert_eq!(again, Err(ERR_BUSY));
            if library.take_changed(&mut |what, _, _| assert_eq!(what, 1 << 6)) {
                raised += 1;
            }
        }
        // A start that never ends does not loop a caller that redraws on it.
        assert_eq!(raised, 6);
        // A cover made starts the count again.
        fake.fails.set(None);
        fake.queue
            .borrow_mut()
            .extend([Ok(state_answer(0, 0)), Ok(picture(100))]);
        library
            .cover(&who, BookRef::Path(BOOK), 0, &cx, &mut |_| {})
            .unwrap();
        fake.fails.set(Some(ERR_BUSY));
        let busy = library.cover(&who, BookRef::Path(BOOK), 0, &cx, &mut |_| {});
        assert_eq!(busy, Err(ERR_BUSY));
        assert!(library.take_changed(&mut |_, _, _| {}));
        fake.fails.set(None);
        // No place in the caller's table is busy too, but raises nothing: it
        // does not clear by itself, and a plugin that redraws on the event
        // would ask for ever.
        let held = opened(&fake);
        fake.queue.borrow_mut().push(Ok(state_answer(0, 0)));
        let full = library.cover(&who, BookRef::Path(BOOK), 0, &cx, &mut |_| {});
        assert_eq!(full, Err(ERR_BUSY));
        assert!(!library.take_changed(&mut |_, _, _| {}));
        assert!(view.fake.table.close(OWNER, held).is_ok());
        fake.queue.borrow_mut().clear();
        // Not in the catalog: not found, no document opened.
        let calls = fake.calls.borrow().len();
        let none = library.cover(&who, BookRef::Path("books/none.epub"), 0, &cx, &mut |_| {});
        assert_eq!(none, Err(ERR_NOT_FOUND));
        assert_eq!(fake.calls.borrow().len(), calls);
    });
}

/// The production close path: closing the document ends its pictures, so the
/// cover must be the caller's before that (adopted first), and a cover whose
/// id cannot be adopted ends with the document.
#[test]
fn the_cover_survives_the_close_that_ends_the_documents_pictures() {
    use crate::native_host::host_services::HostServices;
    let fake = Fake::new(Limits::X4);
    // The hosts the fake's close path ends pictures in (a test's one leak).
    let hosts: &'static HostServices =
        std::boxed::Box::leak(std::boxed::Box::new(HostServices::new()));
    hosts.wire_documents(fake.clone()).unwrap();
    fake.ending.set(Some(hosts));
    with(&fake, |view| {
        let who = Caller {
            hosts,
            ..caller(view)
        };
        let id = hosts
            .images()
            .add(
                OWNER,
                Origin::Document {
                    document: 1,
                    index: u32::MAX,
                },
                "cache/a/cover.gray",
                100,
                50,
                false,
            )
            .unwrap();
        fake.image.set(Some(Some(id)));
        fake.queue
            .borrow_mut()
            .extend([Ok(state_answer(0, 0)), Ok(picture(100))]);
        let image = make_cover(view, &who, BOOK, 0).unwrap();
        // The document was closed and its pictures ended in the real table.
        assert_eq!(*fake.images_ended.borrow(), [1]);
        assert_eq!(image.image, id);
        let info = hosts.images().info(OWNER, id).unwrap();
        assert_eq!((info.width, info.height, info.index), (100, 50, u32::MAX));
        // A document that is not the caller's: adopting fails, nothing is
        // handed out, and its picture ends with the document.
        let foreign = hosts
            .images()
            .add(
                OWNER + 1,
                Origin::Document {
                    document: 2,
                    index: u32::MAX,
                },
                "cache/b/cover.gray",
                100,
                50,
                false,
            )
            .unwrap();
        fake.image.set(Some(Some(foreign)));
        fake.queue
            .borrow_mut()
            .extend([Ok(state_answer(0, 0)), Ok(picture(100))]);
        assert_eq!(
            make_cover(view, &who, "books/b.epub", 0).unwrap_err(),
            ERR_NOT_FOUND
        );
        assert!(hosts.images().info(OWNER + 1, foreign).is_err());
        assert_eq!(*fake.released.borrow(), [1, 2]);
    });
}

/// The call as a plugin's callback makes it: the wire op 7 of `core.library`
/// through the service's backend (the dispatcher's entry), the real library
/// host on a card catalog, the document service and the format behind it.
/// The reply is checked as the dispatcher checks it, and is the picture.
#[test]
#[ignore = "needs LibraryHost::screen_long_side (Host's #2348): the op checks `side` against it first and answers ERR_UNSUPPORTED until then; remove this attribute with that slice"]
fn the_wire_call_of_op_7_reaches_the_format_and_answers_the_picture() {
    use crate::abi7::services_wire;
    use crate::native_host::{
        call_services::validate_reply,
        lib_host::LibraryHost,
        lib_reads::tests::card,
        lib_service::{LibHost, backend},
    };
    let card = card();
    let library: Rc<dyn LibHost> = Rc::new(LibraryHost::new(
        card.storage.clone(),
        card.env.clone(),
        None,
    ));
    let fake = Fake::new(Limits::X4);
    with(&fake, |view| {
        view.hosts.wire_library(library).unwrap();
        let id = mapped(view, &fake, 1, "cache/a/cover.gray");
        fake.queue
            .borrow_mut()
            .extend([Ok(state_answer(0, 0)), Ok(picture(100))]);
        let op = services_wire::service_schema("core.library", 1)
            .unwrap()
            .op(7)
            .unwrap();
        assert_eq!(op.name, "cover");
        let mut input = std::vec![0; 4096];
        let used = wire::records_encode_counted(
            &[
                Record::ServiceLibRef(wire::ServiceLibRef {
                    path: BOOK,
                    generation: 0,
                    key: None,
                }),
                Record::ServiceLibCover(wire::ServiceLibCover { side: 0 }),
            ],
            usize::from(op.input_rows),
            op.max_input,
            &mut input,
        )
        .unwrap();
        // The reply buffer is the op's whole `max_reply`.
        let mut out = std::vec![0; op.max_reply];
        let length = backend(view, &caller(view), op, &input[..used], &mut out).unwrap();
        validate_reply(&out[..length], op).unwrap();
        match wire::record_decode_one(&out[..length]).unwrap() {
            Record::ServiceDocImage(image) => {
                assert_eq!((image.image, image.index), (id, u32::MAX));
            }
            _ => panic!("a picture"),
        }
        assert_eq!(*fake.ended.borrow(), [1], "the format's document is closed");
    });
}
