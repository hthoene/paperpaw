//! The shelves, the lists of names, the next books of the series begun and
//! book information over a scripted `core.library` and `core.ui`, with the
//! plugin made and driven by the scripted host: what it asks the library
//! (the facets, the books of a series by number, the book after the last
//! begun one, a book by path), and that no refusal of the host aborts.
#![cfg(feature = "linked")]
#![allow(clippy::unwrap_used, reason = "test assertions")]
use paperpaw_core::native_host::registrations::{
    EventScreenOpened, Record, ServiceEnd, ServiceLibBook, ServiceLibEntry, ServiceLibFacet,
    ServiceLibState, ServiceUiParam, record_encode_one, records_decode,
};
use paperpaw_native::host_glue::ERR_NOT_FOUND;
use paperpaw_plugin_files::NATIVE_ENTRY;
use paperpaw_scripted_host::{ScriptedHost, Slot, event_batch, sweep};
use std::cell::RefCell;
use std::rc::Rc;

const STATE: u32 = 1;
const QUERY: u32 = 2;
const BOOK: u32 = 3;
const FACETS: u32 = 5;
const NEXT_IN_SERIES: u32 = 6;
const PARAMS: u32 = 17;
const SHELVES_TOKEN: u32 = 12;
const SHELF_TOKEN: u32 = 11;
const NAMES_TOKEN: u32 = 13;
const INFO_TOKEN: u32 = 14;
const HANDOVER: u32 = 7;
const GENERATION: u32 = 4;

fn stream(records: &[Record<'_>]) -> Vec<u8> {
    let mut out = Vec::new();
    for record in records {
        let mut one = vec![0; 4096];
        let used = record_encode_one(record, &mut one).unwrap();
        out.extend_from_slice(&one[..used]);
    }
    out
}

fn end(total: usize) -> Record<'static> {
    Record::ServiceEnd(ServiceEnd {
        flags: 1,
        total: u32::try_from(total).unwrap(),
        generation: GENERATION,
        next: u32::try_from(total).unwrap(),
        cursor: None,
    })
}

/// The series with a book being read or finished, and the book to read next
/// in each (the key of the last book begun, the title of the one after it).
/// `Seen`'s next book was read before.
struct Series {
    name: &'static str,
    reading: bool,
    last: u32,
    next: Option<(&'static str, u8)>,
}

fn series() -> Vec<Series> {
    vec![
        Series {
            name: "Saga",
            reading: true,
            last: 3,
            next: Some(("Saga 4", 255)),
        },
        Series {
            name: "Done",
            reading: false,
            last: 9,
            next: Some(("Done 10", 255)),
        },
        Series {
            name: "Seen",
            reading: false,
            last: 20,
            next: Some(("Seen 2", 40)),
        },
        Series {
            name: "Last",
            reading: false,
            last: 30,
            next: None,
        },
    ]
}

/// What the library was asked, in order.
type Log = Rc<RefCell<Vec<String>>>;

fn script_library(host: &ScriptedHost, log: &Log) {
    let log = log.clone();
    host.service("core.library", 1, 0, move |op, input| {
        let batch = records_decode(input).map_err(|_| -1)?;
        let rows: Vec<Record<'_>> = batch.iter().collect();
        let series = series();
        match (op, rows.first()) {
            (STATE, _) => Ok(stream(&[Record::ServiceLibState(ServiceLibState {
                state: 2,
                books: 40,
                generation: GENERATION,
                walk_ppm: 1_000_000,
            })])),
            (FACETS, Some(Record::ServiceLibQuery(query))) => {
                log.borrow_mut().push(format!(
                    "facets {} show {} cursor {}",
                    query.facet, query.show, query.cursor
                ));
                let mut out: Vec<Record<'_>> = Vec::new();
                let mut count = 0;
                for s in &series {
                    // Show 2 is the books being read, 3 the finished ones, 0 all.
                    let wanted = match query.show {
                        2 => s.reading,
                        3 => !s.reading,
                        _ => true,
                    };
                    if wanted {
                        count += 1;
                        out.push(Record::ServiceLibFacet(ServiceLibFacet {
                            text: s.name,
                            count: 2,
                            first: 0,
                        }));
                    }
                }
                out.push(end(count));
                Ok(stream(&out))
            }
            (QUERY, Some(Record::ServiceLibQuery(query))) => {
                log.borrow_mut().push(format!(
                    "query show {} series {} sort {} flags {}",
                    query.show, query.series, query.sort, query.flags
                ));
                let Some(s) = series.iter().find(|s| s.name == query.series) else {
                    return Ok(stream(&[end(0)]));
                };
                Ok(stream(&[
                    Record::ServiceLibEntry(ServiceLibEntry {
                        key: s.last,
                        title: "last begun",
                        author: "",
                        series: s.name,
                        number: "",
                        progress: 50,
                        flags: 2,
                        added: 0,
                        badges: "",
                    }),
                    end(1),
                ]))
            }
            (NEXT_IN_SERIES, Some(Record::ServiceLibRef(book))) => {
                let key = book.key.unwrap();
                log.borrow_mut()
                    .push(format!("next {key} in {}", book.generation));
                let Some((title, progress)) =
                    series.iter().find(|s| s.last == key).and_then(|s| s.next)
                else {
                    return Err(ERR_NOT_FOUND);
                };
                Ok(stream(&[Record::ServiceLibEntry(ServiceLibEntry {
                    key: key + 1,
                    title,
                    author: "",
                    series: "",
                    number: "",
                    progress: u32::from(progress),
                    flags: 0,
                    added: 0,
                    badges: "",
                })]))
            }
            (BOOK, Some(Record::ServiceLibRef(book))) => {
                log.borrow_mut().push(format!("book {}", book.path));
                Ok(stream(&[Record::ServiceLibBook(ServiceLibBook {
                    key: 5,
                    generation: GENERATION,
                    path: book.path,
                    title: "A Title",
                    author: "An Author",
                    series: "Saga",
                    number: "3",
                    progress: 45,
                    flags: 2,
                    added: 0,
                    size: 2_400_000,
                    format: "epub",
                    tags: 0,
                    fields: 0,
                    title_bytes: 7,
                    author_bytes: 9,
                    series_bytes: 4,
                })]))
            }
            _ => Err(-1),
        }
    });
}

/// `core.ui`: the hand-over's parameters.
fn script_ui(host: &ScriptedHost, params: &'static [(&'static str, &'static str)]) {
    host.service("core.ui", 1, 0, move |op, _| match op {
        PARAMS => {
            let mut out: Vec<Record<'_>> = params
                .iter()
                .map(|(key, value)| Record::ServiceUiParam(ServiceUiParam { key, value }))
                .collect();
            out.push(Record::ServiceEnd(ServiceEnd {
                flags: 0,
                total: u32::try_from(params.len()).unwrap(),
                generation: 0,
                next: 0,
                cursor: None,
            }));
            Ok(stream(&out))
        }
        _ => Err(-1),
    });
}

fn opened(screen: &str, handover: u32) -> Vec<u8> {
    event_batch(&Record::EventScreenOpened(EventScreenOpened {
        now_ms: 0,
        screen,
        instance: 1,
        slot: "",
        flags: 0,
        handover,
        params: 0,
        text_bytes: 0,
    }))
    .unwrap()
}

/// Opens `screen` on `token` with the hand-over's `params`; the status.
fn run(
    host: &ScriptedHost,
    log: &Log,
    token: u32,
    screen: &str,
    params: &'static [(&'static str, &'static str)],
) -> i32 {
    script_library(host, log);
    script_ui(host, params);
    host.add_catalogs_in(concat!(env!("CARGO_MANIFEST_DIR"), "/i18n"))
        .unwrap();
    let plugin = match host.load(NATIVE_ENTRY.entry()) {
        Ok(plugin) => plugin,
        Err(status) => return status,
    };
    let handover = if params.is_empty() { 0 } else { HANDOVER };
    plugin
        .event(token, &opened(screen, handover))
        .map_or_else(|status| status, |done| done.status)
}

#[test]
fn the_shelves_look_for_one_series_begun_with_a_next_book() {
    let host = ScriptedHost::new();
    let log = Log::default();
    assert_eq!(run(&host, &log, SHELVES_TOKEN, "files.shelves", &[]), 0);
    // The series being read first; Saga's last begun book is the one the
    // library names after (`query` by number, highest first), and one next
    // book is enough for the row.
    assert_eq!(
        *log.borrow(),
        [
            "facets 1 show 2 cursor 0",
            // Sort 2 is by series; flags bit 0 is descending.
            "query show 2 series Saga sort 2 flags 1",
            "next 3 in 4",
        ]
    );
}

#[test]
fn the_next_books_list_each_series_once_and_skips_a_book_read_before() {
    let host = ScriptedHost::new();
    let log = Log::default();
    let status = run(&host, &log, SHELF_TOKEN, "files.shelf", &[("kind", "next")]);
    assert_eq!(status, 0);
    assert_eq!(
        *log.borrow(),
        [
            "facets 1 show 2 cursor 0",
            "query show 2 series Saga sort 2 flags 1",
            "next 3 in 4",
            "facets 1 show 3 cursor 0",
            "query show 3 series Done sort 2 flags 1",
            "next 9 in 4",
            "query show 3 series Seen sort 2 flags 1",
            "next 20 in 4",
            "query show 3 series Last sort 2 flags 1",
            "next 30 in 4",
        ]
    );
}

#[test]
fn a_shelf_lists_its_names_by_the_hosts_facet() {
    let host = ScriptedHost::new();
    let log = Log::default();
    let status = run(
        &host,
        &log,
        NAMES_TOKEN,
        "files.names",
        &[("kind", "authors")],
    );
    assert_eq!(status, 0);
    assert_eq!(*log.borrow(), ["facets 0 show 0 cursor 0"]);
    // An unknown kind is refused, not guessed.
    let host = ScriptedHost::new();
    let status = run(
        &host,
        &Log::default(),
        NAMES_TOKEN,
        "files.names",
        &[("kind", "x")],
    );
    assert!(status < 0, "{status}");
}

#[test]
fn book_information_reads_the_book_by_its_path() {
    let host = ScriptedHost::new();
    let log = Log::default();
    let status = run(
        &host,
        &log,
        INFO_TOKEN,
        "files.info",
        &[("path", "books/A/book.epub")],
    );
    assert_eq!(status, 0);
    assert_eq!(*log.borrow(), ["book books/A/book.epub"]);
}

#[test]
fn no_refusal_of_the_library_makes_a_screen_abort() {
    for (token, screen, params) in [
        (SHELVES_TOKEN, "files.shelves", &[][..]),
        (SHELF_TOKEN, "files.shelf", &[("kind", "next")][..]),
        (NAMES_TOKEN, "files.names", &[("kind", "tags")][..]),
        (INFO_TOKEN, "files.info", &[("path", "books/b.epub")][..]),
    ] {
        let results = sweep(Slot::ServiceCall, -5, |host, _| {
            run(host, &Log::default(), token, screen, params)
        });
        assert_eq!(results[0].1, 0, "{screen}");
        // A refused call is an error status (nothing staged), never a hang.
        assert!(
            results.iter().skip(1).all(|(_, status)| *status <= 0),
            "{screen}"
        );
        let results = sweep(Slot::ServiceOpen, -5, |host, _| {
            run(host, &Log::default(), token, screen, params)
        });
        assert!(
            results.iter().skip(1).all(|(_, status)| *status <= 0),
            "{screen}"
        );
    }
}
