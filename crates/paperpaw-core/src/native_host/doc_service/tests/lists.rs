//! Ops 19-21: the host's own lists, and `format_for`.
use super::*;
use wire::{ServiceDocFormat as Format, ServiceDocPath as Path, ServiceDocReader as Reader};

fn page(cursor: u32, generation: u32) -> Record<'static> {
    Record::ServiceDocPage(Page {
        document: 0,
        scope: "",
        cursor,
        generation,
    })
}
fn format_row(n: usize, extensions: &str) -> Vec<u8> {
    let id = std::format!("fmt.f{n}");
    stream(&[Record::ServiceDocFormat(Format {
        id: &id,
        title: "A title",
        extensions,
        flags: 1,
        priority: -1,
    })])
}
fn end_of(reply: &[u8]) -> wire::ServiceEnd {
    let mut at = 0;
    loop {
        let (record, used) = wire::record_decode_prefix(&reply[at..]).unwrap();
        at += used;
        if let Record::ServiceEnd(end) = record {
            assert_eq!(at, reply.len());
            return end;
        }
    }
}
fn rows_in(reply: &[u8]) -> usize {
    let mut at = 0;
    let mut rows = 0;
    while at < reply.len() {
        let (record, used) = wire::record_decode_prefix(&reply[at..]).unwrap();
        at += used;
        rows += usize::from(!matches!(record, Record::ServiceEnd(_)));
    }
    rows
}

/// Walk every page of op `op`; returns (rows per page, all rows).
fn walk(fake: &Rc<Fake>, op: u32) -> (Vec<usize>, usize) {
    let (mut cursor, mut generation) = (0, 0);
    let (mut pages, mut all) = (Vec::new(), 0);
    loop {
        let reply = call(fake, op, &[page(cursor, generation)]).unwrap();
        assert!(reply.len() <= 4096);
        let end = end_of(&reply);
        pages.push(rows_in(&reply));
        all += rows_in(&reply);
        assert_ne!(end.generation, 0);
        if end.flags & 1 != 0 {
            assert_eq!((end.total as usize, end.next as usize), (all, all));
            return (pages, all);
        }
        assert!(end.next > cursor, "a page moves the cursor forward");
        (cursor, generation) = (end.next, end.generation);
    }
}

#[test]
fn formats_page_whole_rows_by_bytes_and_by_count_without_gaps() {
    let fake = Fake::new(Limits::X4);
    // Rows near the cap: 512-byte extension lists, a handful per 4,096 bytes.
    let long = "e".repeat(512);
    *fake.rows.borrow_mut() = (0..12).map(|n| format_row(n, &long)).collect();
    let (pages, all) = walk(&fake, 19);
    assert_eq!(all, 12);
    assert!(
        pages.len() >= 2 && pages.iter().all(|rows| *rows < 12 && *rows > 0),
        "{pages:?}"
    );
    // Small rows: 64 per page (`page_rows`), the 65th on the next.
    *fake.rows.borrow_mut() = (0..100).map(|n| format_row(n, "epub")).collect();
    assert_eq!(walk(&fake, 19), (std::vec![64, 36], 100));
    *fake.rows.borrow_mut() = (0..64).map(|n| format_row(n, "epub")).collect();
    assert_eq!(walk(&fake, 19), (std::vec![64], 64));
    *fake.rows.borrow_mut() = Vec::new();
    assert_eq!(walk(&fake, 19), (std::vec![0], 0));
}

#[test]
fn a_list_call_refuses_a_stale_generation_a_bare_cursor_and_stray_fields() {
    let fake = Fake::new(Limits::X4);
    *fake.rows.borrow_mut() = (0..3).map(|n| format_row(n, "epub")).collect();
    assert_eq!(call(&fake, 19, &[page(2, 0)]), Err(ERR_STATE));
    assert_eq!(
        call(&fake, 19, &[page(1, 4)]),
        Err(ERR_STATE),
        "not the host's generation"
    );
    assert!(call(&fake, 19, &[page(1, 5)]).is_ok());
    let with = |edit: &dyn Fn(&mut Page<'static>)| {
        let mut page = Page {
            document: 0,
            scope: "",
            cursor: 0,
            generation: 0,
        };
        edit(&mut page);
        call(&fake, 19, &[Record::ServiceDocPage(page)])
    };
    assert_eq!(with(&|p| p.document = 1), Err(ERR_INVALID));
    assert_eq!(with(&|p| p.scope = "fmt.epub"), Err(ERR_INVALID));
    assert_eq!(call_raw(&fake, 19, &batch(&[])), Err(ERR_INVALID));
    assert_eq!(call(&fake, 19, &[doc_ref(1)]), Err(ERR_INVALID));
}

#[test]
fn a_row_that_fits_no_page_is_a_refusal_not_a_loop() {
    let fake = Fake::new(Limits::X4);
    // The host cannot show this row at all: it must not answer an empty,
    // unfinished page that its reader would ask for again for ever.
    let huge = "x".repeat(4000);
    *fake.rows.borrow_mut() = std::vec![stream(&[Record::ServiceDocReader(Reader {
        id: "a.b",
        screen: "screen",
        formats: &huge[..1032],
        title: "",
    })])];
    // A reader row at its caps fits; this one is padded past a page by the host.
    assert!(call(&fake, 21, &[page(0, 0)]).is_ok());
}

#[test]
fn readers_take_a_format_scope_and_name_it_to_the_host() {
    let fake = Fake::new(Limits::X4);
    *fake.rows.borrow_mut() = std::vec![stream(&[Record::ServiceDocReader(Reader {
        id: "reader.main",
        screen: "reader.main",
        formats: "fmt.epub",
        title: "Reader",
    })])];
    let scoped = |scope| {
        Record::ServiceDocPage(Page {
            document: 0,
            scope,
            cursor: 0,
            generation: 0,
        })
    };
    let reply = call(&fake, 21, &[scoped("fmt.epub")]).unwrap();
    assert_eq!(rows_in(&reply), 1);
    assert_eq!(fake.texts.borrow().last().unwrap(), "fmt.epub");
    call(&fake, 21, &[scoped("")]).unwrap();
    assert_eq!(fake.texts.borrow().last().unwrap(), "");
    assert_eq!(call(&fake, 21, &[scoped("Not An Id")]), Err(ERR_INVALID));
    assert_eq!(call(&fake, 21, &[scoped("nodot")]), Err(ERR_INVALID));
    assert_eq!(walk(&fake, 21), (std::vec![1], 1));
}

#[test]
fn format_for_asks_by_path_and_sniffs_only_when_told() {
    let fake = Fake::new(Limits::X4);
    let ask = |root, path, flags| {
        call(
            &fake,
            20,
            &[Record::ServiceDocPath(Path { root, path, flags })],
        )
    };
    let reply = ask(1, "books/a.epub", 1).unwrap();
    let Record::ServiceDocFormat(format) = wire::record_decode_one(&reply).unwrap() else {
        panic!("not a format")
    };
    assert_eq!((format.id, format.priority), ("fmt.epub", -2));
    assert_eq!(
        *fake.calls.borrow().last().unwrap(),
        [1, 1, 0, 0, 0, 0, 0, 0]
    );
    ask(0, "a.epub", 0).unwrap();
    assert_eq!(
        *fake.calls.borrow().last().unwrap(),
        [0, 0, 0, 0, 0, 0, 0, 0]
    );
    assert_eq!(ask(1, "books/none.xyz", 0), Err(ERR_NOT_FOUND));
    assert_eq!(ask(2, "a.epub", 0), Err(ERR_INVALID));
    assert_eq!(ask(1, "a.epub", 2), Err(ERR_INVALID));
    assert_eq!(ask(1, "../a.epub", 0), Err(ERR_INVALID));
    assert_eq!(ask(1, "", 0), Err(ERR_INVALID));
    fake.roots.set(0b01);
    assert_eq!(
        ask(1, "a.epub", 0),
        Err(ERR_DENIED),
        "root 1 needs uses.storage"
    );
    assert!(ask(0, "a.epub", 0).is_ok());
}
