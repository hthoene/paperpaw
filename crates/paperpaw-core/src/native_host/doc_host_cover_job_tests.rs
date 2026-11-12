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

/// The format's answer to the cover call (op 17): a gray picture of 8 x 4.
fn cover(w: &World<'_>) {
    let mut bytes = vec![0_u8; 3840];
    let n = local::reply_encode(
        &[local::Record::FmtImage(local::FmtImage::new(
            "cover.gray",
            u32::MAX,
            8,
            4,
            0,
        ))],
        &mut bytes,
    )
    .unwrap();
    bytes.truncate(n);
    *w.reply.borrow_mut() = Ok(bytes);
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
fn a_job_that_has_run_ten_minutes_for_one_book_fails_that_book_alone() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        w.state.now.set(1_000);
        job(w, "books/a.epub");
        w.state.cover_job_guard("books/a.epub").unwrap();
        answer(w, PREPARING, 0);
        w.state.now.set(1_000 + 599_999);
        prepare(w);
        assert_eq!(look(w), JobStep::Wait, "within its time");
        w.state.now.set(1_000 + 600_000);
        prepare(w);
        assert_eq!(
            look(w),
            JobStep::Ended {
                ready: false,
                key: path_key("books/a.epub")
            },
            "its book alone is failed"
        );
        assert_eq!(marker(w), None);
    });
}

#[test]
fn the_time_of_a_displaced_job_counts_for_its_book_and_another_book_starts_afresh() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        w.state.now.set(0);
        job(w, "books/a.epub");
        w.state.cover_job_guard("books/a.epub").unwrap();
        // Ran 400 s, then an open took its place.
        w.state.now.set(400_000);
        w.state.cover_job_end(&w.hosts, &FrameRoute(w));
        // Begins again 100 s later and runs 200 s: 600 s in all.
        w.state.now.set(500_000);
        job(w, "books/a.epub");
        w.state.cover_job_guard("books/a.epub").unwrap();
        answer(w, PREPARING, 0);
        w.state.now.set(699_999);
        prepare(w);
        assert_eq!(look(w), JobStep::Wait);
        w.state.now.set(700_000);
        prepare(w);
        assert!(matches!(look(w), JobStep::Ended { ready: false, .. }));
        // Another book has its own ten minutes.
        job(w, "books/b.epub");
        w.state.cover_job_guard("books/b.epub").unwrap();
        answer(w, PREPARING, 0);
        w.state.now.set(700_000 + 599_999);
        prepare(w);
        assert_eq!(look(w), JobStep::Wait);
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
        cover(w);
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
        cover(w);
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
fn no_step_is_made_while_a_library_list_is_up_and_for_a_quiet_moment_after() {
    use crate::native_host::doc_service::PrepareStep;
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        w.state.now.set(1_000);
        let document = job(w, "books/a.epub");
        w.state.cover_job_guard("books/a.epub").unwrap();
        answer(w, PREPARING, 0);
        // The list is up: the job's row stays, nothing is stepped and the
        // pump is not asked to run.
        w.state.cover_job_list(true, 0);
        assert_eq!(
            w.state.prepare_step(&w.hosts, &FrameRoute(w)),
            PrepareStep::Idle,
            "no document work beside the list"
        );
        assert!(!w.state.preparing());
        // Ten minutes in the list are no awake time of the book.
        w.state.now.set(1_000 + 700_000);
        w.state.cover_job_list(true, 0);
        assert!(!w.state.preparing());
        // The list is gone: still held for the quiet moment.
        w.state.now.set(1_000 + 700_100);
        w.state.cover_job_list(false, 0);
        assert!(!w.state.preparing(), "quiet moment");
        // The list was last up at 701_000: the moment ends 2 s after it.
        w.state.now.set(1_000 + 700_000 + 1_999);
        w.state.cover_job_list(false, 0);
        assert!(!w.state.preparing());
        w.state.now.set(1_000 + 700_000 + 2_000);
        w.state.cover_job_list(false, 0);
        assert!(w.state.preparing(), "the job goes on");
        prepare(w);
        assert_eq!(look(w), JobStep::Wait, "not failed by the time held");
        assert_eq!(w.state.table.cover_job().map(|job| job.id), Some(document));
    });
}

#[test]
fn a_ready_job_is_ended_beside_a_list_for_only_conversion_is_held() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        w.state.now.set(1_000);
        let document = job(w, "books/a.epub");
        w.state.cover_job_guard("books/a.epub").unwrap();
        w.state.cover_job_list(true, 0);
        assert_eq!(look(w), JobStep::Wait, "converting: held, still open");
        let job_row = w.state.table.cover_job().unwrap();
        w.state
            .table
            .set_prepared(document, READY, 0, 0, job_row.flags)
            .unwrap();
        cover(w);
        assert!(
            matches!(look(w), JobStep::Ended { ready: true, .. }),
            "ready: its cover is made, the list being up"
        );
    });
}

#[test]
fn no_step_is_made_within_the_quiet_time_after_an_input_and_the_wake_is_told() {
    use crate::native_host::doc_service::PrepareStep;
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        w.state.now.set(1_000);
        let document = job(w, "books/a.epub");
        w.state.cover_job_guard("books/a.epub").unwrap();
        answer(w, PREPARING, 0);
        // An input at 1,000: quiet from 2,000. The wake is told, no step.
        assert_eq!(w.state.cover_job_list(false, 2_000), Some(2_000));
        assert_eq!(
            w.state.prepare_step(&w.hosts, &FrameRoute(w)),
            PrepareStep::Idle
        );
        assert!(!w.state.preparing());
        assert!(!w.state.cover_quiet());
        // A later input moves it on.
        w.state.now.set(1_500);
        w.state.cover_job_input(2_500);
        assert_eq!(w.state.cover_job_list(false, 2_500), Some(2_500));
        w.state.now.set(2_500);
        assert_eq!(w.state.cover_job_list(false, 2_500), None);
        assert!(w.state.cover_quiet());
        assert!(w.state.preparing(), "the job goes on");
        prepare(w);
        assert_eq!(look(w), JobStep::Wait);
        assert_eq!(w.state.table.cover_job().map(|job| job.id), Some(document));
    });
}

#[test]
fn a_start_held_back_for_the_users_quiet_time_is_released_when_it_is_over() {
    world(|w| {
        w.state.now.set(10_000);
        w.state.allow_cover_jobs();
        w.state.cover_job_held_back();
        // Nothing to convert, but a start waits: the wake is told too.
        assert_eq!(w.state.cover_job_list(false, 11_000), Some(11_000));
        assert!(!w.state.cover_jobs_released(), "the user works");
        w.state.now.set(11_000);
        assert!(
            w.state.cover_jobs_released(),
            "quiet: the screens ask again"
        );
    });
}

#[test]
fn a_held_back_start_is_released_beside_a_list_for_the_list_asks_again() {
    world(|w| {
        w.state.now.set(10_000);
        w.state.allow_cover_jobs();
        w.state.cover_job_held_back();
        w.state.cover_job_list(true, 0);
        assert!(
            w.state.cover_jobs_released(),
            "a list is no reason not to ask"
        );
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

#[test]
fn a_ready_books_cover_is_made_by_the_job_and_only_then_answered_inline() {
    use crate::native_host::doc_host::CoverAt;
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        assert_eq!(w.state.cover_at("books/a.epub"), CoverAt::Unmade);
        // Ready at its open: still the job's (no picture in the list's frame).
        answer(w, READY, 0);
        let document = w.open(1, "books/a.epub").unwrap();
        assert!(w.state.table.start_cover_job(OPENER, document));
        assert_eq!(w.state.cover_at("books/a.epub"), CoverAt::Job);
        cover(w);
        assert!(matches!(look(w), JobStep::Ended { ready: true, .. }));
        // The job asked the format for the cover (op 17), then closed it.
        let ops = ops(w);
        assert_eq!(ops.get(ops.len() - 2..), Some(&[17, 4][..]), "{ops:?}");
        // From now on the list's `cover` answers inline from the cache.
        assert_eq!(w.state.cover_at("books/a.epub"), CoverAt::Made);
        assert_eq!(w.state.cover_at("books/b.epub"), CoverAt::Unmade);
    });
}

#[test]
fn the_covers_made_are_kept_for_the_last_32_books() {
    use crate::native_host::doc_host::CoverAt;
    world(|w| {
        for n in 0..33 {
            let _ = w
                .state
                .note_cover_made(path_key(&std::format!("books/{n}.epub")));
        }
        // The first is written over; the 32 after it stay, each once.
        assert_eq!(w.state.cover_at("books/0.epub"), CoverAt::Unmade);
        assert_eq!(w.state.cover_at("books/1.epub"), CoverAt::Made);
        assert_eq!(w.state.cover_at("books/32.epub"), CoverAt::Made);
        let _ = w.state.note_cover_made(path_key("books/32.epub"));
        assert_eq!(w.state.cover_at("books/1.epub"), CoverAt::Made);
        let _ = w.state.note_cover_made(0);
        assert_eq!(w.state.cover_at("books/1.epub"), CoverAt::Made);
    });
}

#[test]
fn a_ready_book_beside_a_job_the_list_holds_gets_its_cover_made_at_once() {
    use crate::native_host::doc_host::CoverAt;
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        w.state.allow_cover_jobs();
        // A ready book (opened first: the test's opens are not the library's
        // own, which leave the job alone).
        answer(w, READY, 0);
        let document = w.open(1, "books/a.epub").unwrap();
        // A long book prepares as the job, and a library list is up: held.
        job(w, "books/long.epub");
        w.state.list_hold(true);
        assert!(w.state.table.list_held());
        // The ready book's document becomes the cover to make beside it.
        assert!(w.state.table.start_cover_make(OPENER, document));
        assert_eq!(w.state.cover_at("books/a.epub"), CoverAt::Job);
        // Any other book not made waits, and is not opened.
        assert_eq!(w.state.cover_at("books/b.epub"), CoverAt::Wait);
        assert!(w.state.cover_job_active());
        let before = ops(w).len();
        cover(w);
        assert_eq!(
            look(w),
            JobStep::Ended {
                ready: true,
                key: path_key("books/a.epub")
            }
        );
        assert_eq!(ops(w).get(before..), Some(&[17, 4][..]), "cover, close");
        assert_eq!(w.state.cover_at("books/a.epub"), CoverAt::Made);
        assert_eq!(w.state.cover_at("books/b.epub"), CoverAt::Unmade);
        // The held job is there as it was, and takes no step.
        assert!(
            w.state
                .table
                .cover_job()
                .is_some_and(|j| j.state == PREPARING)
        );
        assert_eq!(look(w), JobStep::Wait);
    });
}

#[test]
fn a_book_is_noted_as_made_only_once_its_format_answered() {
    use crate::native_host::doc_host::CoverAt;
    use paperpaw_native::host_glue::{ERR_INVALID, ERR_NOT_FOUND};
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        w.state.allow_cover_jobs();
        for (path, reply, noted) in [
            ("books/fault.epub", Err(ERR_INVALID), false),
            ("books/plain.epub", Err(ERR_NOT_FOUND), true),
        ] {
            answer(w, READY, 0);
            let document = w.open(1, path).unwrap();
            assert!(w.state.table.start_cover_make(OPENER, document));
            *w.reply.borrow_mut() = reply;
            let key = path_key(path);
            // A cover call that failed: not noted, and the book counts as
            // failed; one without a cover is noted (not sent again).
            assert_eq!(look(w), JobStep::Ended { ready: noted, key });
            let at = if noted {
                CoverAt::Made
            } else {
                CoverAt::Unmade
            };
            assert_eq!(w.state.cover_at(path), at, "{path}");
            assert!(w.state.table.cover_make().is_none(), "closed");
        }
    });
}

#[test]
fn a_ready_cover_to_make_ends_without_a_cover_while_a_book_is_read_or_at_sleep() {
    use crate::native_host::doc_host::CoverAt;
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        w.state.allow_cover_jobs();
        answer(w, READY, 0);
        // A book open for reading (an open after the cover to make would
        // close it first: it is the host job's alone).
        let reader = w.open(1, "books/r.epub").unwrap();
        let document = w.open(1, "books/a.epub").unwrap();
        assert!(w.state.table.start_cover_make(OPENER, document));
        // No picture beside the reader; nothing is told.
        let before = ops(w).len();
        assert_eq!(look(w), JobStep::None);
        assert_eq!(ops(w).get(before..), Some(&[4][..]), "only its close");
        assert_eq!(w.state.cover_at("books/a.epub"), CoverAt::Unmade);
        w.state.table.close(OPENER, reader).unwrap();
        // At sleep it ends with the job.
        let document = w.open(1, "books/a.epub").unwrap();
        assert!(w.state.table.start_cover_make(OPENER, document));
        w.state.cover_job_abort(&w.hosts, &FrameRoute(w));
        assert!(w.state.table.cover_make().is_none());
        assert!(!w.state.cover_job_active());
        // Any other open closes it first, never takes it over.
        let document = w.open(1, "books/a.epub").unwrap();
        assert!(w.state.table.start_cover_make(OPENER, document));
        let other = w.open(1, "books/a.epub").unwrap();
        assert_ne!(other, document);
        assert!(w.state.table.cover_make().is_none());
    });
}

#[test]
fn a_book_asked_for_stays_in_the_memo_and_33_covers_on_screen_end_the_cycle() {
    use crate::native_host::doc_host::{CoverAt, cover_make::Cover};
    let path = |n: u32| std::format!("books/{n}.epub");
    world(|w| {
        for n in 0..32 {
            assert!(!w.state.note_cover_made(path_key(&path(n))));
        }
        // Asked for: book 0 moves to the front; book 1 is the oldest now.
        assert_eq!(w.state.cover_at(&path(0)), CoverAt::Made);
        assert!(
            !w.state.note_cover_made(path_key(&path(32))),
            "book 1 was not asked for since: not on screen"
        );
        assert_eq!(w.state.cover_at(&path(0)), CoverAt::Made);
        assert_eq!(w.state.cover_at(&path(1)), CoverAt::Unmade);
        // 33 covers on screen: each round asks for all of them; the one not
        // made is made and writes over one on screen, every round.
        for _round in 0..3 {
            let unmade: Vec<u32> = (0..33)
                .filter(|&n| w.state.cover_at(&path(n)) == CoverAt::Unmade)
                .collect();
            assert_eq!(unmade.len(), 1, "{unmade:?}");
            assert!(w.state.note_cover_made(path_key(&path(unmade[0]))));
        }
        // Told to the library as a failed job of no book: its cap of those
        // in a row ends the raises (no endless bit 6).
        assert_eq!(
            Cover::Noted { crowded: true }.step(9),
            JobStep::Ended {
                ready: false,
                key: 0
            }
        );
        assert_eq!(
            Cover::Noted { crowded: false }.step(9),
            JobStep::Ended {
                ready: true,
                key: 9
            }
        );
    });
}

#[test]
fn a_cover_call_that_cannot_be_made_now_is_retried_not_failed() {
    use crate::native_host::doc_host::CoverAt;
    use paperpaw_native::host_glue::{ERR_BUSY, ERR_NO_MEMORY};
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        w.state.allow_cover_jobs();
        for status in [ERR_BUSY, ERR_NO_MEMORY] {
            answer(w, READY, 0);
            let document = w.open(1, "books/a.epub").unwrap();
            assert!(w.state.table.start_cover_make(OPENER, document));
            *w.reply.borrow_mut() = Err(status);
            // Not noted, no book remembered as failed (key 0): asked again.
            assert_eq!(
                look(w),
                JobStep::Ended {
                    ready: false,
                    key: 0
                },
                "{status}"
            );
            assert_eq!(w.state.cover_at("books/a.epub"), CoverAt::Unmade);
            assert!(w.state.table.cover_make().is_none(), "closed");
        }
        // The next try makes it.
        answer(w, READY, 0);
        let document = w.open(1, "books/a.epub").unwrap();
        assert!(w.state.table.start_cover_make(OPENER, document));
        cover(w);
        assert!(matches!(look(w), JobStep::Ended { ready: true, .. }));
        assert_eq!(w.state.cover_at("books/a.epub"), CoverAt::Made);
    });
}
