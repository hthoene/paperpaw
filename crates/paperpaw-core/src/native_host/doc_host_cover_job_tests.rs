//! The library's cover job in the production document host (`doc_cover_job`):
//! a preparing book kept open as the job is converted by the preparing loop,
//! ended when ready or failed, given way to any open that is not the library's,
//! and cleaned up when the host closed it.
use super::*;
use crate::native_host::{
    doc_service::{FAILED, JobKind, JobStep, PREPARING, READY},
    fmt_call::FrameRoute,
    lib_cover::path_key,
};

fn fmt_state(state: u32, progress: u32, status: i32) -> Vec<u8> {
    let mut bytes = vec![0_u8; 3840];
    let n = local::reply_encode(
        &[local::Record::FmtState(local::FmtState::new(
            0,
            state,
            progress,
            status,
            0,
            5,
            "epub.epub",
        ))],
        &mut bytes,
    )
    .unwrap();
    bytes.truncate(n);
    bytes
}

/// A preparing book, opened and kept as the cover job.
fn job(w: &World<'_>, path: &str) -> u32 {
    *w.reply.borrow_mut() = Ok(fmt_state(PREPARING, 0, 0));
    let document = w.open(1, path).unwrap();
    assert!(w.state.table.start_cover_job(OPENER, document));
    document
}

fn answer(w: &World<'_>, state: u32, status: i32) {
    *w.reply.borrow_mut() = Ok(fmt_state(state, 0, status));
}

fn prepare(w: &World<'_>) {
    w.state.prepare_step(&w.hosts, &FrameRoute(w));
}

fn look(w: &World<'_>) -> JobStep {
    w.state
        .cover_job_step(&w.hosts, &FrameRoute(w), &mut |_, _| false)
}

fn ops(w: &World<'_>) -> Vec<u32> {
    w.calls.borrow().iter().map(|call| call.2).collect()
}

#[test]
fn jobs_may_begin_only_after_the_start_and_a_held_back_one_is_released_once() {
    world(|w| {
        assert!(!w.state.cover_jobs_allowed());
        assert!(!w.state.cover_jobs_released(), "nothing held back");
        w.state.cover_job_held_back();
        assert!(!w.state.cover_jobs_released(), "not before the start");
        assert_eq!(w.state.allow_cover_jobs(), 0);
        assert!(w.state.cover_jobs_allowed());
        assert!(w.state.cover_jobs_released());
        assert!(!w.state.cover_jobs_released(), "said once");
    });
}

#[test]
fn a_job_held_back_waits_for_the_reader_and_a_displacement_for_a_quiet_moment() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        w.state.allow_cover_jobs();
        w.state.now.set(10_000);
        // A book is open for reading: not released until it has closed.
        *w.reply.borrow_mut() = Ok(fmt_state(READY, 0, 0));
        let reader = w.open(1, "books/r.epub").unwrap();
        w.state.cover_job_held_back();
        assert!(!w.state.cover_jobs_released(), "a book is open");
        w.state.table.close(OPENER, reader).unwrap();
        assert!(w.state.cover_jobs_released());
        // Displaced: held back for the quiet moment too.
        w.state.cover_job_displaced(JobKind::Cover);
        assert!(!w.state.cover_jobs_released(), "not yet");
        w.state.now.set(10_000 + 1_999);
        assert!(!w.state.cover_jobs_released(), "not yet");
        w.state.now.set(10_000 + 2_000);
        assert!(w.state.cover_jobs_released());
        assert!(!w.state.cover_jobs_released(), "said once");
    });
}

#[test]
fn a_job_displaced_three_times_in_a_row_for_one_book_is_given_up() {
    world(|w| {
        w.state.allow_cover_jobs();
        w.state.job_key.set(5);
        for time in [0_u64, 10_000] {
            w.state.now.set(time);
            w.state.cover_job_displaced(JobKind::Cover);
            assert_eq!(w.state.cover_job_given_up(), 0);
            w.state.now.set(time + 2_000);
            assert!(w.state.cover_jobs_released(), "starts again");
        }
        // Another book is not the same run.
        w.state.job_key.set(6);
        w.state.cover_job_displaced(JobKind::Cover);
        assert_eq!(w.state.cover_job_given_up(), 0);
        assert!(w.state.job_held_back.replace(false));
        w.state.job_key.set(5);
        w.state.cover_job_displaced(JobKind::Cover);
        w.state.cover_job_displaced(JobKind::Cover);
        assert!(w.state.job_held_back.replace(false));
        w.state.cover_job_displaced(JobKind::Cover);
        // The third in a row: not started again, and the book is named for
        // the library's ring, once.
        assert!(!w.state.job_held_back.get());
        assert_eq!(w.state.cover_job_given_up(), 5);
        assert_eq!(w.state.cover_job_given_up(), 0);
    });
}

#[test]
fn the_job_is_the_hosts_own_no_openers() {
    use crate::native_host::doc_service::HOST_JOB;
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        let document = job(w, "books/a.epub");
        let row = w.state.table.cover_job().unwrap();
        assert_eq!(row.owner, HOST_JOB);
        assert!(
            w.state.table.lease(OPENER, document).is_err(),
            "not the opener's any more"
        );
        // No event for the host's own job when it is taken.
        let taken = w.state.table.take_cover_job().unwrap();
        w.state.table.raise_closed(&taken, 4);
        assert_eq!(w.state.table.take_closed(HOST_JOB, |_| Ok(1)), Ok(None));
    });
}

#[test]
fn a_sleep_inside_a_callback_ends_the_job_at_the_next_look() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        job(w, "books/a.epub");
        w.state.cover_job_guard("books/a.epub").unwrap();
        w.state.cover_job_suspend_later();
        assert_eq!(look(w), JobStep::None);
        assert!(w.state.table.cover_job().is_none(), "ended");
        assert_eq!(marker(w), None);
        assert!(!w.state.cover_jobs_released(), "not restarted");
    });
}

fn marker(w: &World<'_>) -> Option<std::string::String> {
    use paperpaw_platform::Storage;
    let mut text = [0_u8; 32];
    let n = w
        .card
        .read_at(".paperpaw/cover-job.txt", 0, &mut text)
        .ok()?;
    Some(std::string::String::from_utf8(text[..n].to_vec()).unwrap())
}

#[test]
fn the_marker_is_written_with_the_job_and_goes_when_the_job_has_left() {
    world(|w| {
        assert_eq!(marker(w), None);
        w.state.cover_job_guard("books/a.epub").unwrap();
        assert_eq!(
            marker(w).unwrap(),
            std::format!("{:016x}", path_key("books/a.epub"))
        );
        // No job in the table (an open took its place): the look removes it.
        assert!(w.state.cover_job_active(), "a marker is looked at");
        assert_eq!(look(w), JobStep::None);
        assert_eq!(marker(w), None);
        assert!(!w.state.cover_job_active());
    });
}

#[test]
fn a_job_that_ends_removes_its_marker() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        job(w, "books/a.epub");
        w.state.cover_job_guard("books/a.epub").unwrap();
        answer(w, READY, 0);
        prepare(w);
        assert!(matches!(look(w), JobStep::Ended { ready: true, .. }));
        assert_eq!(marker(w), None);
        // Given way to by an open that is not the library's: gone as well.
        job(w, "books/b.epub");
        w.state.cover_job_guard("books/b.epub").unwrap();
        w.state.cover_job_end(&w.hosts, &FrameRoute(w));
        assert_eq!(marker(w), None);
    });
}

#[test]
fn a_marker_left_from_before_names_its_book_once_at_the_start() {
    world(|w| {
        use paperpaw_platform::Storage;
        w.card
            .write(".paperpaw/cover-job.txt", b"00000000000000ff")
            .unwrap();
        assert_eq!(w.state.allow_cover_jobs(), 0xff);
        assert_eq!(marker(w), None, "removed");
        assert_eq!(w.state.allow_cover_jobs(), 0);
        // A file that is not a marker names no book, and is removed too:
        // too short, too long, not hexadecimal, empty.
        for bad in [
            &b"nonsense"[..],
            b"00000000000000ff0",
            b"zzzzzzzzzzzzzzzz",
            b"00000000000000f",
            b"",
        ] {
            w.card.write(".paperpaw/cover-job.txt", bad).unwrap();
            assert_eq!(w.state.allow_cover_jobs(), 0, "{bad:?}");
            assert_eq!(marker(w), None, "{bad:?} removed");
        }
    });
}

#[test]
fn a_job_that_prepares_is_waited_for_then_ended_ready_with_its_books_key() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        assert_eq!(look(w), JobStep::None, "no job");
        let document = job(w, "books/a.epub");
        assert!(w.state.cover_job_path_is("books/a.epub"));
        assert!(!w.state.cover_job_path_is("books/b.epub"));
        assert!(w.state.cover_job_active());
        assert_eq!(look(w), JobStep::Wait);
        answer(w, READY, 0);
        prepare(w);
        assert_eq!(
            look(w),
            JobStep::Ended {
                ready: true,
                key: path_key("books/a.epub")
            }
        );
        // The job is gone: its document closed, the format got its close.
        assert!(w.state.table.cover_job().is_none());
        assert!(w.state.table.lease(OPENER, document).is_err());
        assert_eq!(ops(w).last(), Some(&4), "op 4 close: {:?}", ops(w));
        assert_eq!(look(w), JobStep::None);
        assert!(!w.state.cover_job_active());
    });
}

#[test]
fn a_job_whose_preparation_failed_ends_failed() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        job(w, "books/a.epub");
        answer(w, FAILED, paperpaw_native::host_glue::ERR_INVALID);
        prepare(w);
        assert_eq!(
            look(w),
            JobStep::Ended {
                ready: false,
                key: path_key("books/a.epub")
            }
        );
        assert!(w.state.table.cover_job().is_none());
        // A card that failed is not the book's fault: no key.
        job(w, "books/a.epub");
        answer(w, FAILED, paperpaw_native::host_glue::ERR_IO);
        prepare(w);
        assert_eq!(
            look(w),
            JobStep::Ended {
                ready: false,
                key: 0
            }
        );
    });
}

#[test]
fn one_job_runs_at_a_time() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        *w.reply.borrow_mut() = Ok(fmt_state(PREPARING, 0, 0));
        let first = w.open(1, "books/a.epub").unwrap();
        let second = w.open(1, "books/b.epub").unwrap();
        assert!(w.state.table.start_cover_job(OPENER, first));
        assert!(!w.state.table.start_cover_job(OPENER, second), "waits");
        assert!(w.state.cover_job_path_is("books/a.epub"));
    });
}

#[test]
fn an_open_that_is_not_the_librarys_takes_the_job_first() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        let document = job(w, "books/a.epub");
        // The reader (any open of another call) opens its book: the job's
        // place goes first, before the open's own work.
        *w.reply.borrow_mut() = Ok(fmt_state(READY, 0, 0));
        w.open(1, "books/b.epub").unwrap();
        assert!(w.state.table.cover_job().is_none());
        // The job was the host's, never the opener's.
        assert_eq!(
            w.state.table.lease(OPENER, document),
            Err(paperpaw_native::host_glue::ERR_NOT_FOUND)
        );
        assert_eq!(look(w), JobStep::None);
        // The host's own job has no opener to hear reason 4 (needed).
        let mut out = [0_u8; 64];
        assert_eq!(w.state.next_event(OPENER, &mut out), Ok(None));
        assert_eq!(
            w.state
                .next_event(crate::native_host::doc_service::HOST_JOB, &mut out),
            Ok(None)
        );
        // It is held back: it starts again after the quiet moment.
        assert!(w.state.job_held_back.get());
    });
}

#[test]
fn a_job_the_host_closed_leaves_only_its_row_and_that_is_freed() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        let document = job(w, "books/a.epub");
        // The card went (reason 2): the host ends it.
        crate::native_host::doc_service::end_by_host(&*w.state, document, 2).unwrap();
        assert!(
            w.state.table.cover_job().is_some(),
            "its row is still there"
        );
        assert_eq!(look(w), JobStep::None);
        assert!(w.state.table.cover_job().is_none(), "freed, no event");
    });
}

#[test]
fn no_step_is_made_while_a_book_is_open_for_reading() {
    use crate::native_host::doc_service::PrepareStep;
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        let document = job(w, "books/a.epub");
        assert!(w.state.table.only_cover_job_preparing());
        // The reader's book is open (a ready one).
        *w.reply.borrow_mut() = Ok(fmt_state(READY, 0, 0));
        // Not through `open`: that takes the job first. A document that the
        // table holds for a reader, opened by hand, blocks the job's steps.
        let (reading, _) = w
            .state
            .table
            .open(OPENER + 1, true, FormatRef(0), READY, 0, &|_| true)
            .unwrap();
        assert_eq!(
            w.state.prepare_step(&w.hosts, &FrameRoute(w)),
            PrepareStep::Idle,
            "the job is not stepped while a book is open"
        );
        assert!(!w.state.preparing(), "nothing asks the pump to run");
        w.state.table.close(OPENER + 1, reading).unwrap();
        assert!(w.state.preparing(), "the book closed: the job goes on");
        assert_eq!(w.state.table.cover_job().map(|job| job.id), Some(document));
    });
}

#[test]
fn steps_that_cannot_be_made_fail_the_job_and_only_its_book() {
    use crate::native_host::doc_service::PrepareStep;
    use paperpaw_native::host_glue::ERR_BUSY;
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        job(w, "books/a.epub");
        *w.reply.borrow_mut() = Err(ERR_BUSY);
        for _ in 0..7 {
            assert_eq!(
                w.state.prepare_step(&w.hosts, &FrameRoute(w)),
                PrepareStep::Later
            );
            assert_eq!(look(w), JobStep::Wait);
        }
        assert_eq!(
            w.state.prepare_step(&w.hosts, &FrameRoute(w)),
            PrepareStep::Later
        );
        // The eighth in a row: the job failed, its book alone (its key).
        assert_eq!(
            look(w),
            JobStep::Ended {
                ready: false,
                key: path_key("books/a.epub")
            }
        );
    });
}

#[test]
fn a_job_that_sleeps_ends_without_a_restart() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        let document = job(w, "books/a.epub");
        w.state.cover_job_abort(&w.hosts, &FrameRoute(w));
        assert!(w.state.table.cover_job().is_none());
        assert!(w.state.table.lease(OPENER, document).is_err());
        assert_eq!(ops(w).last(), Some(&4), "the format got its close");
        assert_eq!(look(w), JobStep::None, "nothing restarts it");
        assert!(!w.state.preparing());
    });
}
