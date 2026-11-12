#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
//! The library's cover through `core.document`: opened as the caller on root 1
//! whatever its features, the format asked for the cover, the document closed
//! again, the picture the caller's own.
use super::*;
use crate::native_host::lib_service::LibWrite;
use crate::native_host::{
    admitted_features::AdmittedFeatures,
    doc_service::cover::{self, Made},
    host_services::Caller,
    image_service::{LIBRARY_COVER, Origin},
};

const BOOK: &str = "books/a.epub";

/// What `make_cover` answers for a book that is queued as (or behind) the cover
/// job (`Made::Queued`), as a status no host answers.
const QUEUED: i32 = i32::MIN;

/// `cover::make`, the picture or [`QUEUED`] as the error.
fn make_cover(
    view: &View<'_>,
    caller: &Caller<'_>,
    path: &str,
    side: u32,
) -> Result<crate::native_host::registrations::ServiceDocImage, i32> {
    match cover::make(view, caller, path, side)? {
        Made::Image(image) => Ok(image),
        Made::Queued => Err(QUEUED),
        Made::Failed => Err(ERR_NOT_FOUND),
    }
}

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
        // The open is a background one: the format builds no contents or
        // positions for a cover.
        assert_eq!(
            calls[0][4],
            crate::native_host::doc_service::table::BACKGROUND
        );
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
fn a_format_that_failed_has_no_cover() {
    // A failed document carries its ERR_* (`check::state`).
    let fake = Fake::new(Limits::X4);
    with(&fake, |view| {
        fake.queue.borrow_mut().push(Ok(state_answer(2, ERR_IO)));
        assert_eq!(
            make_cover(view, &caller(view), BOOK, 0).unwrap_err(),
            ERR_NOT_FOUND
        );
        // Never asked for a cover, never left open.
        assert_eq!(fake.calls.borrow().len(), 1);
        assert_eq!(*fake.released.borrow(), [1]);
        assert!(view.fake.table.cover_job().is_none());
    });
}

#[test]
fn a_format_still_preparing_is_kept_open_as_the_cover_job_and_asks_to_come_back() {
    let fake = Fake::new(Limits::X4);
    with(&fake, |view| {
        fake.queue.borrow_mut().push(Ok(state_answer(1, 0)));
        assert_eq!(
            make_cover(view, &caller(view), BOOK, 0).unwrap_err(),
            QUEUED
        );
        // Only the open; the document stays, as the job, background, held.
        assert_eq!(fake.calls.borrow().len(), 1);
        assert!(fake.released.borrow().is_empty());
        let job = view.fake.table.cover_job().unwrap();
        assert!(job.background && job.cover_job);
        // The host's own job, no opener's.
        assert_eq!(
            (job.id, job.owner),
            (1, crate::native_host::doc_service::HOST_JOB)
        );
    });
}

#[test]
fn a_second_preparing_book_waits_behind_the_job_and_is_closed() {
    // Where the device has room for two documents of the caller.
    let fake = Fake::new(Limits::LARGE);
    with(&fake, |view| {
        fake.queue
            .borrow_mut()
            .extend([Ok(state_answer(1, 0)), Ok(state_answer(1, 0))]);
        assert_eq!(
            make_cover(view, &caller(view), BOOK, 0).unwrap_err(),
            QUEUED
        );
        assert_eq!(
            make_cover(view, &caller(view), "books/b.epub", 0).unwrap_err(),
            QUEUED
        );
        // The first stays the job; the second was closed.
        assert_eq!(view.fake.table.cover_job().unwrap().id, 1);
        assert_eq!(*fake.released.borrow(), [2]);
    });
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

/// The cover's open is a background one: with both device-wide places of the
/// X4 held by other openers' documents it is `ERR_FULL` (the library host
/// answers busy), and it displaces none of them.
#[test]
fn a_cover_open_takes_no_place_from_another_opener() {
    let fake = Fake::new(Limits::X4);
    with(&fake, |view| {
        let first = fake
            .table()
            .open(OWNER + 1, false, FormatRef(0), READY, 0, &|_| true)
            .unwrap()
            .0;
        let second = fake
            .table()
            .open(OWNER + 2, false, FormatRef(0), READY, 0, &|_| true)
            .unwrap()
            .0;
        fake.queue.borrow_mut().push(Ok(state_answer(0, 0)));
        assert_eq!(
            make_cover(view, &caller(view), BOOK, 0).unwrap_err(),
            ERR_FULL
        );
        assert!(fake.table().lease(OWNER + 1, first).is_ok(), "stays open");
        assert!(fake.table().lease(OWNER + 2, second).is_ok(), "stays open");
        assert!(fake.calls.borrow().is_empty(), "no format call");
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

/// A preparing book (DOCX, MOBI) is the cover job: busy for now and no event,
/// the host tells the screens when the job has ended, a failed book has no
/// cover and is not opened again, and a run of failed jobs stops the events.
#[test]
fn the_library_host_tells_when_the_cover_job_has_ended() {
    use crate::native_host::{
        lib_cover::{CoverCx, path_key},
        lib_host::LibraryHost,
        lib_reads::tests::card,
        lib_service::{BookRef, Caller as Who, LibQuery},
    };
    let card = card();
    let library = LibraryHost::new(card.storage.clone(), card.env.clone(), None);
    let fake = Fake::new(Limits::X4);
    with(&fake, |view| {
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
        fake.queue.borrow_mut().push(Ok(state_answer(1, 0)));
        let queued = library.cover(&who, BookRef::Path(BOOK), 0, &cx, &mut |_| {});
        assert_eq!(queued, Err(ERR_BUSY));
        assert!(
            !library.take_changed(&mut |_, _, _| {}),
            "no event while the job runs: it ends with one"
        );
        // The job is ready: the screens are told to ask again (no one book).
        library.cover_job_ended(true, path_key(BOOK));
        let mut told = None;
        assert!(library.take_changed(&mut |what, _, path| {
            told = Some((what, std::string::String::from(path)));
        }));
        assert_eq!(told, Some((1 << 6, std::string::String::new())));
        // A failed job: told too, and the book is not found, not opened again.
        library.cover_job_ended(false, path_key(BOOK));
        assert!(library.take_changed(&mut |_, _, _| {}));
        let calls = fake.calls.borrow().len();
        let failed = library.cover(&who, BookRef::Path(BOOK), 0, &cx, &mut |_| {});
        assert_eq!(failed, Err(ERR_NOT_FOUND));
        assert_eq!(fake.calls.borrow().len(), calls, "nothing opened");
        // A run of failed jobs stops the events (no loop); a ready one starts
        // them again.
        library.cover_job_ended(true, 98);
        assert!(library.take_changed(&mut |_, _, _| {}));
        let mut raised = 0;
        for key in 1..=12_u64 {
            library.cover_job_ended(false, key);
            if library.take_changed(&mut |_, _, _| {}) {
                raised += 1;
            }
        }
        assert_eq!(raised, 8, "the ninth failed job in a row raises nothing");
        library.cover_job_ended(true, 99);
        assert!(library.take_changed(&mut |_, _, _| {}));
    });
}

/// While the kernel's start has not returned no cover job begins: the preparing
/// book is closed again, answered busy without an event, and held back; the
/// host's `cover_jobs_began` then tells the screens to ask again.
#[test]
fn start_runs_no_cover_job() {
    use crate::native_host::{
        lib_cover::CoverCx,
        lib_host::LibraryHost,
        lib_reads::tests::card,
        lib_service::{BookRef, Caller as Who, LibQuery},
    };
    let card = card();
    let library = LibraryHost::new(card.storage.clone(), card.env.clone(), None);
    let fake = Fake::new(Limits::X4);
    fake.jobs_blocked.set(true);
    with(&fake, |view| {
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
        fake.queue.borrow_mut().push(Ok(state_answer(1, 0)));
        let asked = library.cover(&who, BookRef::Path(BOOK), 0, &cx, &mut |_| {});
        assert_eq!(asked, Err(ERR_BUSY));
        assert!(fake.held_back.get(), "held back");
        assert!(fake.table().cover_job().is_none(), "no job began");
        assert!(
            !library.take_changed(&mut |_, _, _| {}),
            "no event while start runs"
        );
        library.cover_jobs_began();
        assert!(library.take_changed(&mut |_, _, _| {}), "asked again after");
        // Once it may, the same book is the job.
        fake.jobs_blocked.set(false);
        fake.queue.borrow_mut().push(Ok(state_answer(1, 0)));
        let asked = library.cover(&who, BookRef::Path(BOOK), 0, &cx, &mut |_| {});
        assert_eq!(asked, Err(ERR_BUSY));
        assert!(fake.table().cover_job().is_some(), "the job began");
        assert_eq!(&*fake.guarded.borrow(), &[BOOK], "marked when it began");
    });
}

/// Within the quiet time after the user's last input nothing is opened for a
/// book's cover: its zip and XML would come beside the list's buffers (#248).
#[test]
fn the_users_quiet_time_opens_nothing_in_the_lists_frame() {
    use crate::native_host::{
        lib_cover::CoverCx,
        lib_host::LibraryHost,
        lib_reads::tests::card,
        lib_service::{BookRef, Caller as Who, LibQuery},
    };
    let card = card();
    let library = LibraryHost::new(card.storage.clone(), card.env.clone(), None);
    let fake = Fake::new(Limits::X4);
    fake.noisy.set(true);
    with(&fake, |view| {
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
        fake.queue.borrow_mut().push(Ok(state_answer(1, 0)));
        let asked = library.cover(&who, BookRef::Path(BOOK), 0, &cx, &mut |_| {});
        assert_eq!(asked, Err(ERR_BUSY));
        assert!(fake.held_back.get(), "held back");
        assert!(fake.table().cover_job().is_none(), "no job began");
        assert_eq!(fake.queue.borrow().len(), 1, "no book was opened for it");
        assert!(
            !library.take_changed(&mut |_, _, _| {}),
            "no event while the user works"
        );
        library.cover_jobs_began();
        assert!(library.take_changed(&mut |_, _, _| {}), "asked again after");
        // Once it is quiet, the same book is the job.
        fake.noisy.set(false);
        fake.queue.borrow_mut().push(Ok(state_answer(1, 0)));
        let asked = library.cover(&who, BookRef::Path(BOOK), 0, &cx, &mut |_| {});
        assert_eq!(asked, Err(ERR_BUSY));
        assert!(fake.table().cover_job().is_some(), "the job began");
        assert_eq!(&*fake.guarded.borrow(), &[BOOK], "marked when it began");
    });
}

/// A book open for reading holds the job back: the preparing book is closed
/// again, busy with nothing raised, and begins its job when asked again once the
/// reader has closed.
#[test]
fn a_book_open_for_reading_holds_the_job_back() {
    use crate::native_host::{
        lib_cover::CoverCx,
        lib_host::LibraryHost,
        lib_reads::tests::card,
        lib_service::{BookRef, Caller as Who, LibQuery},
    };
    let card = card();
    let library = LibraryHost::new(card.storage.clone(), card.env.clone(), None);
    let fake = Fake::new(Limits::LARGE);
    with(&fake, |view| {
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
        // Another opener's book, shown.
        let (reader, _) = fake
            .table()
            .open(OWNER + 1, true, FormatRef(0), READY, 0, &|_| true)
            .unwrap();
        fake.queue.borrow_mut().push(Ok(state_answer(1, 0)));
        let asked = library.cover(&who, BookRef::Path(BOOK), 0, &cx, &mut |_| {});
        assert_eq!(asked, Err(ERR_BUSY));
        assert!(fake.held_back.get(), "held back");
        assert!(fake.table().cover_job().is_none(), "no job began");
        assert!(fake.guarded.borrow().is_empty(), "no marker");
        assert!(!library.take_changed(&mut |_, _, _| {}), "nothing raised");
        fake.table().close(OWNER + 1, reader).unwrap();
        fake.queue.borrow_mut().push(Ok(state_answer(1, 0)));
        let asked = library.cover(&who, BookRef::Path(BOOK), 0, &cx, &mut |_| {});
        assert_eq!(asked, Err(ERR_BUSY));
        assert!(fake.table().cover_job().is_some(), "the job began");
    });
}

/// On the X4 the job is the first document a book to show takes (before an
/// opener's background index), and takes no opener's cap: a ready cover of the
/// same plugin that asked for the job gets its picture, not `ERR_FULL`.
#[test]
fn the_job_is_taken_first_and_a_ready_cover_is_not_full_beside_it() {
    use crate::native_host::{
        lib_cover::CoverCx,
        lib_host::LibraryHost,
        lib_reads::tests::card,
        lib_service::{BookRef, Caller as Who, LibQuery},
    };
    let card = card();
    let library = LibraryHost::new(card.storage.clone(), card.env.clone(), None);
    let fake = Fake::new(Limits::X4);
    with(&fake, |view| {
        let id = mapped(view, &fake, 1, "cache/b/cover.gray");
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
        // A DOCX that prepares: the job (one place), the caller holds none.
        fake.queue.borrow_mut().push(Ok(state_answer(1, 0)));
        let asked = library.cover(&who, BookRef::Path("books/b.epub"), 0, &cx, &mut |_| {});
        assert_eq!(asked, Err(ERR_BUSY));
        let job = fake.table().cover_job().unwrap();
        assert_ne!(job.owner, OWNER, "the host's own");
        // A ready EPUB of the same plugin: its picture, not busy.
        fake.queue
            .borrow_mut()
            .extend([Ok(state_answer(0, 0)), Ok(picture(100))]);
        let mut got = None;
        library
            .cover(&who, BookRef::Path(BOOK), 0, &cx, &mut |record| {
                if let Record::ServiceDocImage(image) = record {
                    got = Some(image.image);
                }
            })
            .unwrap();
        assert_eq!(got, Some(id));
        assert_eq!(
            fake.table().cover_job().map(|j| j.id),
            Some(job.id),
            "still runs"
        );
        // The device is full now (the job and a background index of another
        // opener): the reader's book takes the job's place, not the index's.
        let (index, _) = fake
            .table()
            .open(OWNER + 1, false, FormatRef(0), READY, 1 << 2, &|_| true)
            .unwrap();
        let (_, taken) = fake
            .table()
            .open(OWNER + 2, true, FormatRef(0), READY, 0, &|_| true)
            .unwrap();
        assert_eq!(taken.map(|d| d.id), Some(job.id), "the job is taken first");
        assert!(
            fake.table().lease(OWNER + 1, index).is_ok(),
            "the index stays"
        );
    });
}

/// The failure ring keeps 8 books: the oldest gives way, and is asked for again.
#[test]
fn the_failure_ring_keeps_eight_books_and_the_oldest_gives_way() {
    use crate::native_host::{
        lib_cover::{CoverCx, path_key},
        lib_host::LibraryHost,
        lib_reads::tests::card,
        lib_service::{BookRef, Caller as Who, LibQuery},
    };
    let card = card();
    let library = LibraryHost::new(card.storage.clone(), card.env.clone(), None);
    let fake = Fake::new(Limits::X4);
    with(&fake, |view| {
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
        let mut paths: Vec<std::string::String> =
            (1..9).map(|i| std::format!("books/{i}.epub")).collect();
        paths.insert(0, BOOK.into());
        for path in &paths {
            library.cover_job_left(path_key(path));
        }
        let calls = fake.calls.borrow().len();
        // The newest 8 are not found, nothing opened.
        for path in &paths[1..] {
            let asked = library.cover(&who, BookRef::Path(path), 0, &cx, &mut |_| {});
            assert_eq!(asked, Err(ERR_NOT_FOUND), "{path}");
        }
        assert_eq!(fake.calls.borrow().len(), calls, "nothing opened");
        // The oldest was given way to: asked for again (it is opened).
        fake.queue.borrow_mut().push(Ok(state_answer(1, 0)));
        let asked = library.cover(&who, BookRef::Path(&paths[0]), 0, &cx, &mut |_| {});
        assert_ne!(asked, Err(ERR_NOT_FOUND));
        assert!(fake.calls.borrow().len() > calls, "opened");
    });
}

/// The marker cannot be written: the job does not begin, the book counts as
/// failed (not found, not opened again) and its document is closed again; a
/// book that waits behind a running job writes no marker.
#[test]
fn a_job_whose_marker_cannot_be_written_does_not_begin() {
    use crate::native_host::{
        lib_cover::CoverCx,
        lib_host::LibraryHost,
        lib_reads::tests::card,
        lib_service::{BookRef, Caller as Who, LibQuery},
    };
    let card = card();
    let library = LibraryHost::new(card.storage.clone(), card.env.clone(), None);
    let fake = Fake::new(Limits::X4);
    fake.guard_fails.set(true);
    with(&fake, |view| {
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
        fake.queue.borrow_mut().push(Ok(state_answer(1, 0)));
        let asked = library.cover(&who, BookRef::Path(BOOK), 0, &cx, &mut |_| {});
        assert_eq!(asked, Err(ERR_NOT_FOUND));
        assert!(fake.table().cover_job().is_none(), "no job, unguarded");
        assert!(!library.take_changed(&mut |_, _, _| {}), "nothing raised");
        // Not asked for again: the book counts as failed, not found, and its
        // document is not opened any more.
        let calls = fake.calls.borrow().len();
        let asked = library.cover(&who, BookRef::Path(BOOK), 0, &cx, &mut |_| {});
        assert_eq!(asked, Err(ERR_NOT_FOUND));
        assert_eq!(fake.calls.borrow().len(), calls);
        // Marked, a job runs (another book); a second behind it writes no
        // marker.
        fake.guard_fails.set(false);
        fake.queue.borrow_mut().push(Ok(state_answer(1, 0)));
        let _ = library.cover(&who, BookRef::Path("books/c.epub"), 0, &cx, &mut |_| {});
        fake.queue.borrow_mut().push(Ok(state_answer(1, 0)));
        let _ = library.cover(&who, BookRef::Path("books/b.epub"), 0, &cx, &mut |_| {});
        assert_eq!(&*fake.guarded.borrow(), &["books/c.epub"]);
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
fn the_wire_call_of_op_7_reaches_the_format_and_answers_the_picture() {
    use crate::abi7::services_wire;
    use crate::native_host::{
        call_services::validate_reply,
        lib_host::LibraryHost,
        lib_reads::tests::card,
        lib_service::{LibHost, backend},
    };
    let card = card();
    // What the documents wiring tells the library: the screen's long side.
    let policy = Rc::new(crate::native_host::lib_edits::EditPolicy::default());
    policy.set_long_side(800);
    let library: Rc<dyn LibHost> =
        Rc::new(LibraryHost::new(card.storage.clone(), card.env.clone(), None).with_policy(policy));
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
