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

#[allow(
    clippy::too_many_lines,
    reason = "one scripted service: its ops in one match"
)]
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
    run_with(host, log, token, screen, params, script_library)
}

fn run_with(
    host: &ScriptedHost,
    log: &Log,
    token: u32,
    screen: &str,
    params: &'static [(&'static str, &'static str)],
    script: fn(&ScriptedHost, &Log),
) -> i32 {
    script(host, log);
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
fn the_shelves_count_the_favourites_and_each_list_and_leave_the_next_books_alone() {
    // The row of the next books is always there: finding one costs host
    // calls for each series, so it is left to the list that is chosen. The
    // other rows say at their end how many they hold.
    let host = ScriptedHost::new();
    let log = Log::default();
    assert_eq!(run(&host, &log, SHELVES_TOKEN, "files.shelves", &[]), 0);
    assert_eq!(
        *log.borrow(),
        [
            "query show 4 series  sort 0 flags 0",
            "facets 0 show 0 cursor 0",
            "facets 1 show 0 cursor 0",
            "facets 2 show 0 cursor 0",
            "facets 4 show 0 cursor 0",
            // The count of the letters row: how many letters there are.
            "facets 3 show 0 cursor 0",
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
            "query show 2 series Saga sort 0 flags 0",
            "next 3 in 4",
            "facets 1 show 3 cursor 0",
            "query show 3 series Done sort 0 flags 0",
            "next 9 in 4",
            "query show 3 series Seen sort 0 flags 0",
            "next 20 in 4",
            "query show 3 series Last sort 0 flags 0",
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

/// A library of 100 books by title, read 32 at a time, with the letters A (the
/// first book 0), B (60) and C (98), D (no jump). The log says what was
/// asked of it: facet, show, flags (bit 1 the archive left out), sort, cursor.
fn script_letters(host: &ScriptedHost, log: &Log) {
    const TOTAL: u32 = 100;
    let log = log.clone();
    host.service("core.library", 1, 0, move |op, input| {
        let batch = records_decode(input).map_err(|_| -1)?;
        let rows: Vec<Record<'_>> = batch.iter().collect();
        match (op, rows.first()) {
            (STATE, _) => Ok(stream(&[Record::ServiceLibState(ServiceLibState {
                state: 2,
                books: TOTAL,
                generation: GENERATION,
                walk_ppm: 1_000_000,
            })])),
            (FACETS, Some(Record::ServiceLibQuery(query))) => {
                log.borrow_mut().push(format!(
                    "facets {} show {} flags {} cursor {}",
                    query.facet, query.show, query.flags, query.cursor
                ));
                let mut out: Vec<Record<'_>> = [
                    ("A", 40, 0),
                    ("B", 38, 60),
                    ("C", 2, 98),
                    ("D", 1, u32::MAX),
                ]
                .into_iter()
                .map(|(text, count, first)| {
                    Record::ServiceLibFacet(ServiceLibFacet { text, count, first })
                })
                .collect();
                out.push(Record::ServiceEnd(ServiceEnd {
                    flags: 1,
                    total: 4,
                    generation: GENERATION,
                    next: 4,
                    cursor: None,
                }));
                Ok(stream(&out))
            }
            (QUERY, Some(Record::ServiceLibQuery(query))) => {
                log.borrow_mut().push(format!(
                    "query sort {} flags {} cursor {}",
                    query.sort, query.flags, query.cursor
                ));
                let end_at = (query.cursor + 32).min(TOTAL);
                let mut out: Vec<Record<'_>> = (query.cursor..end_at)
                    .map(|key| {
                        Record::ServiceLibEntry(ServiceLibEntry {
                            key,
                            title: "A book",
                            author: "",
                            series: "",
                            number: "",
                            progress: 255,
                            flags: 0,
                            added: 0,
                            badges: "",
                        })
                    })
                    .collect();
                out.push(Record::ServiceEnd(ServiceEnd {
                    flags: u32::from(end_at == TOTAL),
                    total: TOTAL,
                    generation: GENERATION,
                    next: end_at,
                    cursor: None,
                }));
                Ok(stream(&out))
            }
            _ => Err(-1),
        }
    });
}

fn letter(host: &ScriptedHost, log: &Log, value: &'static str) -> i32 {
    let params: &'static [(&str, &str)] = match value {
        "A" => &[("kind", "letter"), ("value", "A")],
        "C" => &[("kind", "letter"), ("value", "C")],
        "D" => &[("kind", "letter"), ("value", "D")],
        _ => &[("kind", "letter"), ("value", "Z")],
    };
    run_with(
        host,
        log,
        SHELF_TOKEN,
        "files.shelf",
        params,
        script_letters,
    )
}

#[test]
fn the_letters_screen_asks_for_the_books_the_library_lists() {
    let host = ScriptedHost::new();
    let log = Log::default();
    let params: &[(&str, &str)] = &[("kind", "letters")];
    let status = run_with(
        &host,
        &log,
        NAMES_TOKEN,
        "files.names",
        params,
        script_letters,
    );
    assert_eq!(status, 0);
    // All books, the archive left out: what the library's own list holds.
    assert_eq!(*log.borrow(), ["facets 3 show 0 flags 2 cursor 0"]);
}

#[test]
fn a_letter_opens_the_library_by_title_at_the_window_with_its_first_book() {
    // The first book of C is the 99th: the second window, read by stepping
    // from the start (three replies, then one), and read again to be shown.
    let host = ScriptedHost::new();
    let log = Log::default();
    assert_eq!(letter(&host, &log, "C"), 0);
    let asked = log.borrow();
    let first = "query sort 0 flags 2 cursor";
    assert_eq!(
        *asked,
        [
            "facets 3 show 0 flags 2 cursor 0".to_string(),
            format!("{first} 0"),
            format!("{first} 32"),
            format!("{first} 64"),
            format!("{first} 96"),
            format!("{first} 96"),
        ]
    );
    drop(asked);
    // A first book in the first window needs no stepping.
    let host = ScriptedHost::new();
    let log = Log::default();
    assert_eq!(letter(&host, &log, "A"), 0);
    // The window is read once to find the book, once to show it.
    assert_eq!(log.borrow().len(), 1 + 3 + 3);
}

#[test]
fn a_letter_that_is_gone_or_has_no_jump_opens_the_list_from_its_start() {
    for value in ["Z", "D"] {
        let host = ScriptedHost::new();
        let log = Log::default();
        assert_eq!(letter(&host, &log, value), 0, "{value}");
        let asked = log.borrow();
        assert_eq!(asked.len(), 1 + 3, "{value}");
        assert_eq!(
            asked.last().map(String::as_str),
            Some("query sort 0 flags 2 cursor 64")
        );
    }
}

#[test]
fn no_refusal_of_the_library_makes_the_letters_abort() {
    for (token, screen, params) in [
        (NAMES_TOKEN, "files.names", &[("kind", "letters")][..]),
        (
            SHELF_TOKEN,
            "files.shelf",
            &[("kind", "letter"), ("value", "C")][..],
        ),
    ] {
        let results = sweep(Slot::ServiceCall, -5, |host, _| {
            run_with(host, &Log::default(), token, screen, params, script_letters)
        });
        assert_eq!(results[0].1, 0, "{screen}");
        assert!(
            results.iter().skip(1).all(|(_, status)| *status <= 0),
            "{screen}"
        );
    }
}
