//! `doc_open.flags` bit 3 `details` (`document.md` 2.1, #245): the host job's
//! own open of a `prepared` format's book, read for its details (and cover)
//! without a conversion: refused from a plugin, dropped for other formats,
//! never prepared, never stamped, an unstamped cache never removed for it, busy
//! beside a conversion of the same cache, and never handed to another open.
use super::*;
use crate::native_host::{
    doc_cache::STAMP,
    doc_service::{
        DETAILS, DocHost, HOST_JOB, JobKind, JobStep, PREPARING, READY, details::DetailsJob,
    },
    fmt_call::FrameRoute,
    lib_cover::path_key,
};
use paperpaw_native::host_glue::ERR_INVALID;

/// `doc_open.flags` bit 2 `background`.
const BACKGROUND: u32 = 1 << 2;

/// A format plugin whose books are converted before they are read.
const DOCX: u64 = 13;

fn state(state: u32) -> Vec<u8> {
    let mut bytes = vec![0_u8; 3840];
    let n = local::reply_encode(
        &[local::Record::FmtState(local::FmtState::new(
            0,
            state,
            0,
            0,
            0,
            5,
            "docx.docx",
        ))],
        &mut bytes,
    )
    .unwrap();
    bytes.truncate(n);
    bytes
}

fn install_docx(w: &World<'_>) {
    // `format.spec.flags` bit 0 `prepared`.
    w.install("docx", DOCX, &[format("docx", "docx", 1, 0)]);
    w.state.allow_cover_jobs();
}

/// The details job's open of `path`, the format answering `answer`.
fn start(w: &World<'_>, path: &str, answer: u32) -> Result<(), i32> {
    *w.reply.borrow_mut() = Ok(state(answer));
    let who = Caller {
        owner: HOST_JOB,
        plugin: "",
        features: AdmittedFeatures::NONE,
        hosts: &w.hosts,
    };
    DetailsJob::start(&*w.state, w, &who, &FrameRoute(w), path)
}

fn look(w: &World<'_>) -> JobStep {
    w.state
        .cover_job_step(&w.hosts, &FrameRoute(w), &mut |_, _| true)
}

fn ops(w: &World<'_>) -> Vec<u32> {
    w.calls.borrow().iter().map(|call| call.2).collect()
}

fn stamped(w: &World<'_>, folder: &str) -> bool {
    w.card.get(&std::format!("{folder}/{STAMP}")).is_some()
}

#[test]
fn a_plugins_open_with_bit_3_is_refused() {
    world(|w| {
        install_docx(w);
        let opened = w.call(
            AdmittedFeatures::test(1, FEATURES),
            1,
            &[Record::ServiceDocOpen(wire::ServiceDocOpen {
                root: 1,
                path: "books/a.docx",
                format: "",
                flags: DETAILS,
                image_side: 0,
                cover_side: 0,
                css_root: 0,
                css_path: "",
            })],
        );
        assert_eq!(opened.err(), Some(ERR_INVALID));
        assert!(ops(w).is_empty(), "the format was not asked");
    });
}

#[test]
fn the_job_opens_a_prepared_books_details_with_bit_3_and_other_formats_without() {
    world(|w| {
        install_docx(w);
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        start(w, "books/a.docx", READY).unwrap();
        assert_eq!(w.open_flags.borrow().last(), Some(&(BACKGROUND | DETAILS)));
        assert!(matches!(look(w), JobStep::DetailsEnded { .. }));
        start(w, "books/b.epub", READY).unwrap();
        assert_eq!(
            w.open_flags.borrow().last(),
            Some(&BACKGROUND),
            "not prepared"
        );
    });
}

#[test]
fn a_details_open_answered_preparing_is_never_prepared_and_ends_with_no_details() {
    world(|w| {
        install_docx(w);
        start(w, "books/a.docx", PREPARING).unwrap();
        assert!(!DocHost::preparing(&*w.state), "no preparing step for it");
        w.state.prepare_step(&w.hosts, &FrameRoute(w));
        assert_eq!(
            look(w),
            JobStep::DetailsEnded {
                filled: false,
                key: path_key("books/a.docx")
            }
        );
        // Open, close: never op 3.
        assert_eq!(ops(w), [2, 4]);
        assert!(w.state.table.cover_job().is_none());
    });
}

#[test]
fn a_details_open_neither_stamps_nor_removes_and_a_normal_open_then_prepares_afresh() {
    world(|w| {
        install_docx(w);
        w.makes_cache.set(true);
        // The details open answers ready without converting; the fixture
        // writes into the folder as a cover picture would be.
        start(w, "books/a.docx", READY).unwrap();
        let folder = seen_cache(w, 0);
        let job = w.state.table.cover_job().unwrap();
        assert_eq!(job.job_kind, JobKind::Details);
        assert!(!stamped(w, &folder), "no stamp at a details open");
        let _ = look(w);
        assert!(!stamped(w, &folder), "nor when it ends");
        // A second details open reads what is there: nothing is removed.
        w.makes_cache.set(false);
        start(w, "books/a.docx", READY).unwrap();
        let last = w.scoped.borrow().len() - 1;
        assert_eq!(seen_cache(w, last), folder, "the same cache");
        assert!(w.card.get(&std::format!("{folder}/pages/1")).is_some());
        let job = w.state.table.cover_job().unwrap();
        // A plugin's open of the book: the details document is closed first,
        // never handed over; a new open, without bit 3, which removes the
        // unstamped folder and prepares.
        *w.reply.borrow_mut() = Ok(state(PREPARING));
        let before = ops(w).len();
        let document = w.open(1, "books/a.docx").unwrap();
        assert_ne!(document, job.id);
        assert_eq!(ops(w).get(before..), Some(&[4, 2][..]), "close, then open");
        assert_eq!(w.open_flags.borrow().last(), Some(&0));
        assert!(w.state.table.cover_job().is_none());
        assert!(w.card.get(&std::format!("{folder}/pages/1")).is_none());
        assert!(DocHost::preparing(&*w.state), "it prepares");
    });
}

#[test]
fn a_details_open_beside_a_conversion_of_the_same_cache_is_busy() {
    use crate::native_host::doc_service::cover;
    world(|w| {
        install_docx(w);
        // A reader converts the book.
        *w.reply.borrow_mut() = Ok(state(PREPARING));
        let reading = w.open(1, "books/a.docx").unwrap();
        // The library's cover of it (bit 3): the same cache, half made.
        let opener = Caller {
            owner: OPENER,
            plugin: "opener",
            features: AdmittedFeatures::NONE,
            hosts: &w.hosts,
        };
        let opens = w.open_flags.borrow().len();
        let made = cover::make(w, &opener, "books/a.docx", 0);
        assert!(matches!(made, Err(ERR_BUSY)), "{:?}", made.as_ref().err());
        assert_eq!(
            w.open_flags.borrow().len(),
            opens,
            "no open reached the format"
        );
        assert!(
            w.state.table.lease(OPENER, reading).is_ok(),
            "the reader's is kept"
        );
    });
}
