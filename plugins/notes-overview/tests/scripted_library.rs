//! The list of books with notes over a scripted `core.library`, with the
//! plugin made and driven by the scripted host: what it asks the library
//! (the pages of the catalog, a book's annotations by catalog reference, the
//! title of the books that have notes only), and that no refusal of
//! the host or of memory makes it abort.
#![cfg(feature = "linked")]
#![allow(clippy::unwrap_used, reason = "test assertions")]
use paperpaw_core::native_host::registrations::{
    EventScreenOpened, Record, ServiceEnd, ServiceLibAnnotation, ServiceLibBook, ServiceLibEntry,
    record_encode_one, records_decode,
};
use paperpaw_native::host_glue::ERR_STATE;
use paperpaw_plugin_notes_overview::NATIVE_ENTRY;
use paperpaw_scripted_host::{ScriptedHost, Slot, event_batch, sweep};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

const QUERY: u32 = 2;
const BOOK: u32 = 3;
const ANNOTATIONS: u32 = 15;
const LIST_TOKEN: u32 = 10;
/// The catalog and the annotations are told in pages of two rows.
const PAGE: usize = 2;
const CATALOG_GENERATION: u32 = 8;

/// Books: title and notes (0 bookmark, 1 highlight, 2 another kind).
fn library() -> Vec<(&'static str, Vec<u32>)> {
    vec![
        ("Zebra", vec![1, 1, 0]),
        ("Plain", vec![]),
        ("Apple", vec![0]),
        ("Other", vec![2]),
        ("Mango", vec![1, 0, 1, 0, 1]),
    ]
}

fn stream(records: &[Record<'_>]) -> Vec<u8> {
    let mut out = Vec::new();
    for record in records {
        let mut one = vec![0; 4096];
        let used = record_encode_one(record, &mut one).unwrap();
        out.extend_from_slice(&one[..used]);
    }
    out
}

fn end(last: bool, total: usize, generation: u32, next: usize) -> Record<'static> {
    Record::ServiceEnd(ServiceEnd {
        flags: u32::from(last),
        total: u32::try_from(total).unwrap(),
        generation,
        next: u32::try_from(next).unwrap(),
        cursor: None,
    })
}

/// What the library was asked, in order.
type Log = Rc<RefCell<Vec<String>>>;

fn script(
    host: &ScriptedHost,
    books: Vec<(&'static str, Vec<u32>)>,
    log: Log,
    stale: Rc<Cell<bool>>,
) {
    host.service("core.ui", 1, 0, |_, _| Ok(Vec::new()));
    host.service("core.library", 1, 0, move |op, input| {
        let batch = records_decode(input).map_err(|_| -1)?;
        let rows: Vec<Record<'_>> = batch.iter().collect();
        match (op, rows.first()) {
            (QUERY, Some(Record::ServiceLibQuery(query))) => {
                log.borrow_mut()
                    .push(format!("query {} {}", query.cursor, query.generation));
                if query.generation != 0 && query.generation != CATALOG_GENERATION {
                    return Err(ERR_STATE);
                }
                // The catalog changed under the scan once, at its second page.
                if query.cursor == 2 && stale.replace(false) {
                    return Err(ERR_STATE);
                }
                let from = query.cursor as usize;
                let to = (from + PAGE).min(books.len());
                let mut out: Vec<Record<'_>> = (from..to)
                    .map(|key| {
                        Record::ServiceLibEntry(ServiceLibEntry {
                            key: u32::try_from(key).unwrap(),
                            title: books[key].0,
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
                out.push(end(to == books.len(), books.len(), CATALOG_GENERATION, to));
                Ok(stream(&out))
            }
            (ANNOTATIONS, Some(Record::ServiceLibRef(book))) => {
                let Some(Record::ServiceLibPage(page)) = rows.get(1) else {
                    return Err(-1);
                };
                let key = book.key.unwrap() as usize;
                assert_eq!(book.generation, CATALOG_GENERATION);
                log.borrow_mut()
                    .push(format!("notes {key} {}", page.cursor));
                let notes = &books[key].1;
                let from = page.cursor as usize;
                let to = (from + PAGE).min(notes.len());
                let mut out: Vec<Record<'_>> = (from..to)
                    .map(|id| {
                        Record::ServiceLibAnnotation(ServiceLibAnnotation {
                            id: u32::try_from(id).unwrap(),
                            kind: notes[id],
                            start_section: 0,
                            start_offset: 0,
                            end_section: 0,
                            end_offset: 0,
                            flags: 0,
                            percent: 10,
                            conversion: 0,
                            created: 0,
                            updated: 0,
                            client_key: "",
                            text: "text",
                            note: "",
                        })
                    })
                    .collect();
                out.push(end(to == notes.len(), notes.len(), 3, to));
                Ok(stream(&out))
            }
            (BOOK, Some(Record::ServiceLibRef(book))) => {
                let key = book.key.unwrap() as usize;
                log.borrow_mut().push(format!("book {key}"));
                let title = books[key].0;
                Ok(stream(&[Record::ServiceLibBook(ServiceLibBook {
                    key: u32::try_from(key).unwrap(),
                    generation: CATALOG_GENERATION,
                    path: &format!("Books/{title}.epub"),
                    title,
                    author: "",
                    series: "",
                    number: "",
                    progress: 255,
                    flags: 0,
                    added: 0,
                    size: 1,
                    format: "epub",
                    tags: 0,
                    fields: 0,
                    title_bytes: u32::try_from(title.len()).unwrap(),
                    author_bytes: 0,
                    series_bytes: 0,
                })]))
            }
            _ => Err(-1),
        }
    });
}

fn opened(screen: &str) -> Vec<u8> {
    event_batch(&Record::EventScreenOpened(EventScreenOpened {
        now_ms: 0,
        screen,
        instance: 1,
        slot: "",
        flags: 0,
        handover: 0,
        params: 0,
        text_bytes: 0,
    }))
    .unwrap()
}

/// The list is made whole: the host answers `i18n.copy` and `i18n.format`
/// from the plugin's own catalogs (`run` adds them).
fn finished(status: i32) -> bool {
    status == 0
}

fn run(host: &ScriptedHost) -> i32 {
    host.add_catalogs_in(concat!(env!("CARGO_MANIFEST_DIR"), "/i18n"))
        .unwrap();
    // A refusal while the plugin is made is the factory's status.
    let plugin = match host.load(NATIVE_ENTRY.entry()) {
        Ok(plugin) => plugin,
        Err(status) => return status,
    };
    plugin
        .event(LIST_TOKEN, &opened("notes-overview.screen"))
        .map_or_else(|status| status, |done| done.status)
}

#[test]
fn the_list_asks_the_library_for_pages_and_notes_and_names_only_the_books_with_notes() {
    let host = ScriptedHost::new();
    let log: Log = Log::default();
    script(&host, library(), log.clone(), Rc::default());
    let status = run(&host);
    assert!(finished(status), "{status}");
    // The catalog a page at a time, each page continued in its generation;
    // every book's notes counted from the start, page after page; only the
    // books with a highlight or a bookmark described.
    assert_eq!(
        *log.borrow(),
        [
            "query 0 0",
            "notes 0 0",
            "notes 0 2",
            "book 0",
            "notes 1 0",
            "query 2 8",
            "notes 2 0",
            "book 2",
            "notes 3 0",
            "query 4 8",
            "notes 4 0",
            "notes 4 2",
            "notes 4 4",
            "book 4"
        ]
    );
}

#[test]
fn a_stale_catalog_is_read_again_from_the_start() {
    let host = ScriptedHost::new();
    let log: Log = Log::default();
    script(&host, library(), log.clone(), Rc::new(Cell::new(true)));
    let status = run(&host);
    assert!(finished(status), "{status}");
    let queries: Vec<String> = log
        .borrow()
        .iter()
        .filter(|line| line.starts_with("query"))
        .cloned()
        .collect();
    assert_eq!(
        queries,
        [
            "query 0 0",
            "query 2 8",
            "query 0 0",
            "query 2 8",
            "query 4 8"
        ]
    );
}

#[test]
fn no_refusal_makes_the_list_abort() {
    let results = sweep(Slot::ServiceCall, -5, |host, _| {
        script(host, library(), Log::default(), Rc::default());
        run(host)
    });
    assert!(finished(results[0].1));
    assert!(results.len() > 10, "{}", results.len());
    // A refused call is an error status (nothing staged), never a hang.
    assert!(results.iter().skip(1).all(|(_, status)| *status <= 0));
    // The same for the opening of each service. (Memory refused inside the
    // scan is swept in `native_scan_tests`: the plugin linked into a test
    // allocates from the test's allocator, not through the host's heap slot.)
    let results = sweep(Slot::ServiceOpen, -5, |host, _| {
        script(host, library(), Log::default(), Rc::default());
        run(host)
    });
    assert!(results.len() >= 2, "{}", results.len());
    assert!(results.iter().skip(1).all(|(_, status)| *status <= 0));
}
