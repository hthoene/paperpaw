//! The host's details job in the production document host
//! (`doc_details_job`): a native book opened by the host as its own background
//! document in the one job slot, read through ops 5 and 6 once ready (after the
//! preparing loop where it prepares), kept, closed; a book the format failed
//! is named for the library's failed ring; any other open takes its place.
use super::*;
use crate::native_host::{
    doc_service::{FAILED, JobKind, JobStep, PREPARING, READY, details::DetailsJob},
    fmt_call::FrameRoute,
    lib_cover::path_key,
    ui_store::HOST_OWNER,
};
use paperpaw_native::host_glue::{ERR_BUSY, ERR_INVALID};
use paperpaw_sdk::document::Metadata;

fn encoded(record: local::Record<'_>) -> Vec<u8> {
    let mut bytes = vec![0_u8; 3840];
    let n = local::reply_encode(&[record], &mut bytes).unwrap();
    bytes.truncate(n);
    bytes
}

fn fmt_state(state: u32, status: i32) -> Vec<u8> {
    encoded(local::Record::FmtState(local::FmtState::new(
        0,
        state,
        0,
        status,
        0,
        5,
        "epub.epub",
    )))
}

/// `fmt.meta`: the title, the series and its number, the counts, the flags.
fn fmt_meta(title: &str, series: &str, authors: u32, subjects: u32, flags: u32) -> Vec<u8> {
    encoded(local::Record::FmtMeta(local::FmtMeta::new(
        title, "en", series, "2", 0, 0, 3, authors, subjects, 0, flags,
    )))
}

/// `fmt.text` of `kind` and `index`: `text` of a text of `total` bytes.
fn fmt_text(kind: u32, index: u32, total: usize, text: &str) -> Vec<u8> {
    encoded(local::Record::FmtText(local::FmtText::new(
        kind,
        index,
        u32::try_from(total).unwrap(),
        text,
    )))
}

fn script(w: &World<'_>, answers: Vec<Vec<u8>>) {
    w.script.borrow_mut().extend(answers);
}

fn who<'w>(w: &'w World<'_>) -> Caller<'w> {
    Caller {
        owner: HOST_OWNER,
        plugin: "",
        features: AdmittedFeatures::NONE,
        hosts: &w.hosts,
    }
}

/// The details job's open of `path`, the format answering `state`.
fn start(w: &World<'_>, path: &str, state: u32) -> Result<(), i32> {
    *w.reply.borrow_mut() = Ok(fmt_state(state, 0));
    DetailsJob::start(&*w.state, w, &who(w), &FrameRoute(w), path)
}

/// [`start`] once the kernel's start has returned.
fn begin(w: &World<'_>, path: &str, state: u32) {
    w.state.allow_cover_jobs();
    start(w, path, state).unwrap();
}

/// One look at the job, and what it kept (the path and the details).
fn look(w: &World<'_>) -> (JobStep, Option<(std::string::String, Metadata)>) {
    let mut kept = None;
    let step = w
        .state
        .cover_job_step(&w.hosts, &FrameRoute(w), &mut |path, metadata| {
            kept = Some((path.to_string(), metadata.clone()));
            true
        });
    (step, kept)
}

fn ops(w: &World<'_>) -> Vec<u32> {
    w.calls.borrow().iter().map(|call| call.2).collect()
}

fn marker(w: &World<'_>) -> Option<std::string::String> {
    let mut text = [0_u8; 32];
    let n = w
        .card
        .read_at(".paperpaw/cover-job.txt", 0, &mut text)
        .ok()?;
    Some(std::string::String::from_utf8(text[..n].to_vec()).unwrap())
}

#[test]
fn a_ready_book_is_read_through_meta_and_meta_text_and_its_details_kept() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        begin(w, "books/a.epub", READY);
        let job = w.state.table.cover_job().unwrap();
        assert_eq!(job.job_kind, JobKind::Details);
        assert!(job.background && job.owner == HOST_OWNER);
        assert_eq!(
            marker(w).unwrap(),
            std::format!("{:016x}", path_key("books/a.epub"))
        );
        script(
            w,
            vec![
                fmt_meta("Dune", "Saga", 2, 2, 0),
                fmt_text(1, 0, 13, "Frank Herbert"),
                fmt_text(2, 0, 7, "Fiction"),
                fmt_text(2, 1, 5, "Space"),
            ],
        );
        let (step, kept) = look(w);
        assert_eq!(
            step,
            JobStep::DetailsEnded {
                filled: true,
                key: 0
            }
        );
        let (path, details) = kept.unwrap();
        assert_eq!(path, "books/a.epub");
        assert_eq!(details.title.as_deref(), Some("Dune"));
        assert_eq!(details.authors, ["Frank Herbert"]);
        assert_eq!(details.series.as_deref(), Some("Saga"));
        assert_eq!(details.series_index.as_deref(), Some("2"));
        assert_eq!(details.subjects, ["Fiction", "Space"]);
        // The first author only; each subject below the count.
        assert_eq!(*w.texts.borrow(), [(1, 0, 0), (2, 0, 0), (2, 1, 0)]);
        // Open, meta, three texts, then the format's close.
        assert_eq!(ops(w), [2, 5, 6, 6, 6, 4]);
        assert!(w.state.table.cover_job().is_none());
        assert_eq!(marker(w), None, "the marker goes with the job");
        assert!(!w.state.cover_job_active());
    });
}

#[test]
fn a_cut_title_and_series_are_read_by_offsets_up_to_the_details_cap() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        begin(w, "books/a.epub", READY);
        let title = "t".repeat(1_500);
        let series = "s".repeat(600);
        script(
            w,
            vec![
                fmt_meta(&title[..512], &series[..512], 0, 40, 0b11),
                // The title's first 1,024 bytes: the cap, no more is asked.
                fmt_text(0, 0, 1_500, &title[..1_024]),
                fmt_text(4, 0, 600, &series),
            ],
        );
        // Forty subjects, sixteen read, each cut at 256 bytes.
        let long = "x".repeat(400);
        for index in 0..16 {
            script(w, vec![fmt_text(2, index, 400, &long)]);
        }
        let (step, kept) = look(w);
        assert!(matches!(step, JobStep::DetailsEnded { filled: true, .. }));
        let details = kept.unwrap().1;
        assert_eq!(details.title.unwrap().len(), 1_024);
        assert_eq!(details.series.unwrap(), series);
        assert!(details.authors.is_empty(), "the format named none");
        assert_eq!(details.subjects.len(), 16);
        assert!(details.subjects.iter().all(|subject| subject.len() == 256));
        let texts = w.texts.borrow();
        assert_eq!(texts[..2], [(0, 0, 0), (4, 0, 0)]);
        assert_eq!(texts.len(), 2 + 16);
    });
}

#[test]
fn a_preparing_book_is_converted_by_the_preparing_loop_then_read() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        begin(w, "books/a.docx.epub", PREPARING);
        assert_eq!(look(w).0, JobStep::Wait);
        // The loop's step: the format says it is done.
        *w.reply.borrow_mut() = Ok(fmt_state(READY, 0));
        w.state.prepare_step(&w.hosts, &FrameRoute(w));
        script(w, vec![fmt_meta("Converted", "", 0, 0, 0)]);
        let (step, kept) = look(w);
        assert!(matches!(step, JobStep::DetailsEnded { filled: true, .. }));
        assert_eq!(kept.unwrap().1.title.as_deref(), Some("Converted"));
        assert_eq!(ops(w), [2, 3, 5, 4]);
    });
}

#[test]
fn a_book_the_format_failed_is_named_for_the_failed_ring_and_nothing_is_kept() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        // Its preparation failed.
        begin(w, "books/a.epub", PREPARING);
        *w.reply.borrow_mut() = Ok(fmt_state(FAILED, ERR_INVALID));
        w.state.prepare_step(&w.hosts, &FrameRoute(w));
        let (step, kept) = look(w);
        let key = path_key("books/a.epub");
        assert_eq!(step, JobStep::DetailsEnded { filled: false, key });
        assert!(kept.is_none());
        // Ready, but its `meta` is no `fmt.meta`.
        start(w, "books/b.epub", READY).unwrap();
        script(w, vec![fmt_state(READY, 0)]);
        let (step, kept) = look(w);
        let key = path_key("books/b.epub");
        assert_eq!(step, JobStep::DetailsEnded { filled: false, key });
        assert!(kept.is_none());
        // A card that failed is no fault of the book: no key.
        start(w, "books/c.epub", PREPARING).unwrap();
        *w.reply.borrow_mut() = Ok(fmt_state(FAILED, ERR_IO));
        w.state.prepare_step(&w.hosts, &FrameRoute(w));
        assert_eq!(
            look(w).0,
            JobStep::DetailsEnded {
                filled: false,
                key: 0
            }
        );
        assert_eq!(marker(w), None);
    });
}

#[test]
fn a_book_that_names_nothing_keeps_nothing() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        begin(w, "books/a.epub", READY);
        script(w, vec![fmt_meta("", "", 0, 0, 0)]);
        let (step, kept) = look(w);
        assert_eq!(
            step,
            JobStep::DetailsEnded {
                filled: false,
                key: 0
            }
        );
        assert!(kept.is_none(), "never marked as read with nothing read");
    });
}

#[test]
fn no_details_job_begins_before_the_start_returned_nor_beside_another_job() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        assert_eq!(start(w, "books/a.epub", READY), Err(ERR_STATE));
        assert!(w.calls.borrow().is_empty(), "nothing opened");
        assert_eq!(marker(w), None);
        w.state.allow_cover_jobs();
        // A cover job holds the one slot.
        *w.reply.borrow_mut() = Ok(fmt_state(PREPARING, 0));
        let cover = w.open(1, "books/c.epub").unwrap();
        assert!(w.state.table.start_cover_job(OPENER, cover));
        let calls = w.calls.borrow().len();
        assert_eq!(start(w, "books/a.epub", READY), Err(ERR_BUSY));
        assert_eq!(w.calls.borrow().len(), calls, "nothing opened");
        // And the other way round: the slot is the details job's.
        w.state.cover_job_end(&w.hosts, &FrameRoute(w));
        *w.reply.borrow_mut() = Ok(fmt_state(PREPARING, 0));
        let cover = w.open(1, "books/c.epub").unwrap();
        start(w, "books/a.epub", PREPARING).unwrap();
        assert!(!w.state.table.start_cover_job(OPENER, cover), "one slot");
    });
}

#[test]
fn an_open_that_is_not_the_librarys_takes_the_details_jobs_place_at_once() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        begin(w, "books/a.epub", PREPARING);
        assert!(!w.state.reading_open());
        *w.reply.borrow_mut() = Ok(fmt_state(READY, 0));
        w.open(1, "books/b.epub").unwrap();
        assert!(w.state.reading_open(), "a book is open for reading");
        assert!(w.state.table.cover_job().is_none());
        // The host hears nothing of its own document's end.
        assert!(!w.state.table.has_closed());
        assert_eq!(look(w).0, JobStep::None);
        assert_eq!(marker(w), None);
    });
}

#[test]
fn an_open_that_fails_leaves_no_job_and_no_marker() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        w.state.allow_cover_jobs();
        *w.reply.borrow_mut() = Err(ERR_INVALID);
        assert_eq!(
            DetailsJob::start(&*w.state, w, &who(w), &FrameRoute(w), "books/a.epub"),
            Err(ERR_INVALID)
        );
        assert!(w.state.table.cover_job().is_none());
        assert_eq!(w.state.table.len(), 0);
        assert_eq!(marker(w), None);
        assert!(!w.state.cover_job_active());
    });
}
