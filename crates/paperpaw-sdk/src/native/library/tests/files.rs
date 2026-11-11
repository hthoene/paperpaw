use super::*;
use crate::native::declare::records_wire::Record;

fn host_and_lib<'a>(table: &'a PpHost, state: &mut State) -> (Host<'a>, Library) {
    let host = Host {
        table,
        ctx: core::ptr::from_mut(state).cast(),
    };
    let lib = Library::open(&host).unwrap();
    (host, lib)
}
fn row(path: &'static str, status: i32, flags: u32) -> Record<'static> {
    Record::ServiceLibResult(wire::ServiceLibResult {
        path,
        status,
        flags,
    })
}

#[test]
fn can_delete_sends_the_paths_one_per_line_and_reads_a_row_per_path() {
    let table = table();
    let mut state = State::default();
    let (host, lib) = host_and_lib(&table, &mut state);
    let (mut input, mut out) = ([0; 4096], [0; 4096]);
    reply(
        &state,
        &[
            row("/a.epub", 0, 0b101),
            row("/b.epub", -3, 0),
            end(0, 2, 0, 0),
        ],
    );
    let report = lib
        .can_delete(&host, &["/a.epub", "/b.epub"], &mut input, &mut out)
        .unwrap();
    assert_eq!(state.op.get(), schema::OP_CAN_DELETE);
    assert_eq!(report.len(), 2);
    let rows: Vec<_> = report.rows().collect();
    assert_eq!(rows[0].path(), "/a.epub");
    assert!(rows[0].status().is_ok());
    assert!(rows[0].has_annotations() && !rows[0].has_position() && rows[0].has_book_data());
    assert!(rows[1].status().is_err());
    let bytes = state.input.borrow();
    let sent = wire::records_decode(&bytes).unwrap();
    assert!(matches!(
        sent.iter().next(),
        Some(Record::ServiceLibPaths(p)) if p.paths == "/a.epub\n/b.epub" && p.flags == 0
    ));
}

#[test]
fn delete_carries_with_notes_and_checks_the_reply_against_the_paths() {
    let table = table();
    let mut state = State::default();
    let (host, lib) = host_and_lib(&table, &mut state);
    let (mut input, mut out) = ([0; 4096], [0; 4096]);
    reply(&state, &[row("/a.epub", 0, 0), end(0, 1, 0, 0)]);
    lib.delete(&host, &["/a.epub"], true, &mut input, &mut out)
        .unwrap();
    assert_eq!(state.op.get(), schema::OP_DELETE);
    {
        let bytes = state.input.borrow();
        let sent = wire::records_decode(&bytes).unwrap();
        assert!(matches!(
            sent.iter().next(),
            Some(Record::ServiceLibPaths(p)) if p.flags == 1
        ));
    }
    // A row for another path than the one asked is Invalid.
    reply(&state, &[row("/other.epub", 0, 0), end(0, 1, 0, 0)]);
    assert_eq!(
        lib.delete(&host, &["/a.epub"], false, &mut input, &mut out)
            .unwrap_err(),
        Error::Invalid
    );
    // A row short of the paths, or an end that is not the unpaged one, is Invalid.
    reply(&state, &[row("/a.epub", 0, 0), end(0, 1, 0, 0)]);
    assert_eq!(
        lib.delete(&host, &["/a.epub", "/b.epub"], false, &mut input, &mut out)
            .unwrap_err(),
        Error::Invalid
    );
    reply(&state, &[row("/a.epub", 0, 0), end(1, 1, 5, 1)]);
    assert_eq!(
        lib.delete(&host, &["/a.epub"], false, &mut input, &mut out)
            .unwrap_err(),
        Error::Invalid
    );
}

#[test]
fn bad_path_lists_are_refused_before_the_host_is_called() {
    let table = table();
    let mut state = State::default();
    let (host, lib) = host_and_lib(&table, &mut state);
    let (mut input, mut out) = ([0; 4096], [0; 4096]);
    let long = "x".repeat(513);
    let eight = ["/a"; 8];
    let cases: [(&[&str], Error); 5] = [
        (&[], Error::Invalid),
        (&eight, Error::Invalid),
        (&[""], Error::Invalid),
        (&["/a\n/b"], Error::Invalid),
        (&[long.as_str()], Error::NoSpace),
    ];
    for (paths, want) in cases {
        assert_eq!(
            lib.delete(&host, paths, false, &mut input, &mut out)
                .unwrap_err(),
            want
        );
        assert_eq!(
            lib.can_delete(&host, paths, &mut input, &mut out)
                .unwrap_err(),
            want
        );
    }
    let mut small = [0; 8];
    assert_eq!(
        lib.can_delete(&host, &["/a/long/path.epub"], &mut input, &mut small)
            .unwrap_err(),
        Error::NoSpace
    );
    assert_eq!(state.calls.get(), 0);
}

#[test]
fn import_sends_the_record_and_returns_the_path_it_took() {
    let table = table();
    let mut state = State::default();
    let (host, lib) = host_and_lib(&table, &mut state);
    let (mut input, mut out) = ([0; 2048], [0; 4096]);
    reply(&state, &[row("/Books/News/a (2).epub", 0, 0)]);
    let range = out.as_ptr_range();
    let path = lib
        .import(
            &host,
            &Import::new("/tmp/a.epub", "a.epub")
                .folder("News")
                .existing(Existing::FreeName),
            &mut input,
            &mut out,
        )
        .unwrap();
    assert_eq!(path, "/Books/News/a (2).epub");
    assert!(range.contains(&path.as_ptr()));
    assert_eq!(state.op.get(), schema::OP_IMPORT);
    let bytes = state.input.borrow();
    let sent = wire::records_decode(&bytes).unwrap();
    assert!(matches!(
        sent.iter().next(),
        Some(Record::ServiceLibImport(i))
            if (i.from, i.folder, i.name, i.flags) == ("/tmp/a.epub", "News", "a.epub", 2)
    ));
}

#[test]
fn import_refuses_what_the_host_would_and_a_refusal_is_the_calls_status() {
    let table = table();
    let mut state = State::default();
    let (host, lib) = host_and_lib(&table, &mut state);
    let (mut input, mut out) = ([0; 2048], [0; 4096]);
    let unit = "é".repeat(256); // 256 UTF-16 units, 512 bytes
    let bad_names = [
        "",
        "a/b.epub",
        "a\\b.epub",
        ".hidden",
        "~tmp",
        "a.epub.",
        "a.epub ",
        "a:b.epub",
        "a\"b",
        "a*b",
        "a<b",
        "a>b",
        "a?b",
        "a|b",
        "a\nb",
        "CON",
        "con.txt",
        "Nul.epub",
        "COM1.epub",
        "lpt9",
        "..",
        ".",
        unit.as_str(),
    ];
    for name in bad_names {
        assert_eq!(
            lib.import(
                &host,
                &Import::new("/tmp/a.epub", name),
                &mut input,
                &mut out
            )
            .unwrap_err(),
            Error::Invalid,
            "{name:?}"
        );
    }
    for folder in [
        "/News", "News/", "a//b", "./a", "a/..", "a\\b", "COM2", "a/aux.d",
    ] {
        assert_eq!(
            lib.import(
                &host,
                &Import::new("/tmp/a.epub", "a.epub").folder(folder),
                &mut input,
                &mut out
            )
            .unwrap_err(),
            Error::Invalid,
            "{folder:?}"
        );
    }
    for from in ["", "/tmp/a\n.epub"] {
        assert_eq!(
            lib.import(&host, &Import::new(from, "a.epub"), &mut input, &mut out)
                .unwrap_err(),
            Error::Invalid
        );
    }
    let long = "x".repeat(766);
    assert_eq!(
        lib.import(
            &host,
            &Import::new("/tmp/a.epub", &long),
            &mut input,
            &mut out
        )
        .unwrap_err(),
        Error::NoSpace
    );
    assert_eq!(state.calls.get(), 0);
}

#[test]
fn names_the_card_takes_pass_and_a_refusal_is_the_calls_status() {
    let table = table();
    let mut state = State::default();
    let (host, lib) = host_and_lib(&table, &mut state);
    let (mut input, mut out) = ([0; 2048], [0; 4096]);
    // COM0, COM10 and "console" are not reserved.
    reply(&state, &[row("/Books/COM0.epub", 0, 0)]);
    assert!(
        lib.import(
            &host,
            &Import::new("/tmp/a.epub", "COM0.epub"),
            &mut input,
            &mut out
        )
        .is_ok()
    );
    reply(&state, &[row("/Books/COM10.epub", 0, 0)]);
    assert!(
        lib.import(
            &host,
            &Import::new("/tmp/a.epub", "COM10.epub"),
            &mut input,
            &mut out
        )
        .is_ok()
    );
    // A refusal is the call's status and is never a book.
    state.error.set(-9);
    assert!(
        lib.import(
            &host,
            &Import::new("/tmp/a.epub", "console.epub"),
            &mut input,
            &mut out
        )
        .is_err()
    );
    state.error.set(0);
    reply(&state, &[row("", -9, 0)]);
    assert!(
        lib.import(
            &host,
            &Import::new("/tmp/a.epub", "a.epub"),
            &mut input,
            &mut out
        )
        .is_err()
    );
}

#[test]
fn move_to_sends_the_record_and_the_replace_flag() {
    let table = table();
    let mut state = State::default();
    let (host, lib) = host_and_lib(&table, &mut state);
    let (mut input, mut out) = ([0; 2048], [0; 4096]);
    reply(&state, &[row("/Books/New/a.epub", 0, 0)]);
    lib.move_to(
        &host,
        "/Books/a.epub",
        "/Books/New/a.epub",
        true,
        &mut input,
        &mut out,
    )
    .unwrap();
    assert_eq!(state.op.get(), schema::OP_MOVE);
    let bytes = state.input.borrow();
    let sent = wire::records_decode(&bytes).unwrap();
    assert!(matches!(
        sent.iter().next(),
        Some(Record::ServiceLibMove(m))
            if (m.from, m.to, m.flags) == ("/Books/a.epub", "/Books/New/a.epub", 1)
    ));
}

#[test]
fn move_to_refuses_bad_paths_before_the_call_and_a_refusal_is_the_calls_status() {
    let table = table();
    let mut state = State::default();
    let (host, lib) = host_and_lib(&table, &mut state);
    let (mut input, mut out) = ([0; 2048], [0; 4096]);
    for (from, to) in [("", "/b"), ("/a", ""), ("/a\n", "/b"), ("/a", "/b\u{0}")] {
        assert_eq!(
            lib.move_to(&host, from, to, false, &mut input, &mut out)
                .unwrap_err(),
            Error::Invalid
        );
    }
    let long = "x".repeat(513);
    assert_eq!(
        lib.move_to(&host, &long, "/b", false, &mut input, &mut out)
            .unwrap_err(),
        Error::NoSpace
    );
    assert_eq!(
        lib.move_to(&host, "/a", &long, false, &mut input, &mut out)
            .unwrap_err(),
        Error::NoSpace
    );
    assert_eq!(state.calls.get(), 0);
    // A host status in the reply row is the call's status; a malformed reply is Invalid.
    reply(&state, &[row("/b", -3, 0)]);
    assert!(
        lib.move_to(&host, "/a", "/b", false, &mut input, &mut out)
            .is_err()
    );
    reply(&state, &[]);
    assert_eq!(
        lib.move_to(&host, "/a", "/b", false, &mut input, &mut out)
            .unwrap_err(),
        Error::Invalid
    );
}
