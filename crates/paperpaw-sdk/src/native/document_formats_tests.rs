use super::*;

fn format(id: &'static str, extensions: &'static str) -> Record<'static> {
    Record::ServiceDocFormat(wire::ServiceDocFormat {
        id,
        title: "Format",
        extensions,
        flags: 0,
        priority: -3,
    })
}
fn end(flags: u32, total: u32, generation: u32, next: u32) -> Record<'static> {
    Record::ServiceEnd(wire::ServiceEnd {
        flags,
        total,
        generation,
        next,
        cursor: None,
    })
}

#[test]
fn format_for_sends_the_root_the_path_and_the_sniff_flag_and_reads_the_format() {
    setup!(table, state, host, docs);
    let (mut input, mut out) = ([0; 1024], [0; 4096]);
    reply(&state, &[format("format-epub.epub", "epub\nkepub.epub")]);
    let found = docs
        .format_for(
            &host,
            Root::Library,
            "books/a.epub",
            false,
            &mut input,
            &mut out,
        )
        .unwrap();
    assert_eq!(state.op.get(), schema::OP_FORMAT_FOR);
    assert_eq!(
        (found.id(), found.title(), found.priority()),
        ("format-epub.epub", "Format", -3)
    );
    assert_eq!(
        found.extensions().collect::<Vec<_>>(),
        ["epub", "kepub.epub"]
    );
    {
        let bytes = state.input.borrow();
        let sent = wire::records_decode(&bytes).unwrap();
        assert!(matches!(
            sent.iter().next(),
            Some(Record::ServiceDocPath(p)) if (p.root, p.path, p.flags) == (1, "books/a.epub", 0)
        ));
    }
    docs.format_for(&host, Root::Data, "x", true, &mut input, &mut out)
        .unwrap();
    let bytes = state.input.borrow();
    let sent = wire::records_decode(&bytes).unwrap();
    assert!(matches!(
        sent.iter().next(),
        Some(Record::ServiceDocPath(p)) if (p.root, p.flags) == (0, 1)
    ));
}

#[test]
fn format_for_refuses_bad_arguments_before_the_host_and_reads_the_hosts_answers() {
    setup!(table, state, host, docs);
    let (mut input, mut out) = ([0; 1024], [0; 4096]);
    for (root, path) in [
        (Root::Library, ""),
        (Root::Library, &*"p".repeat(513)),
        (Root::Handed, "a.epub"),
    ] {
        assert_eq!(
            docs.format_for(&host, root, path, false, &mut input, &mut out)
                .unwrap_err(),
            Error::Invalid
        );
    }
    assert_eq!(state.calls.get(), 0);
    assert!(
        docs.format_for(
            &host,
            Root::Library,
            &"p".repeat(512),
            false,
            &mut input,
            &mut out
        )
        .is_err_and(|e| e == Error::Invalid)
    );
    for (status, error) in [
        (abi::ERR_NOT_FOUND, Error::NotFound),
        (abi::ERR_DENIED, Error::Denied),
        (abi::ERR_UNSUPPORTED, Error::Unsupported),
    ] {
        state.error.set(status);
        assert_eq!(
            docs.format_for(&host, Root::Library, "a.xyz", true, &mut input, &mut out)
                .unwrap_err(),
            error
        );
    }
    state.error.set(abi::OK);
    // A reply that is not one format: none, an empty id, an empty extension, too many.
    let sixteen = "a\nb\nc\nd\ne\nf\ng\nh\ni\nj\nk\nl\nm\nn\no\np";
    let seventeen = "a\nb\nc\nd\ne\nf\ng\nh\ni\nj\nk\nl\nm\nn\no\np\nq";
    state.reply.replace(Vec::new());
    for bad in [
        vec![],
        vec![format("", "epub")],
        vec![format("a.b", "epub\n\ntxt")],
        vec![format("a.b", seventeen)],
        vec![format("a.b", "x"), format("a.c", "y")],
        vec![end(1, 0, 3, 0)],
    ] {
        reply(&state, &bad);
        assert_eq!(
            docs.format_for(&host, Root::Library, "a.xyz", true, &mut input, &mut out)
                .unwrap_err(),
            Error::Invalid,
            "{bad:?}"
        );
    }
    reply(&state, &[format("a.b", sixteen)]);
    assert!(
        docs.format_for(&host, Root::Library, "a.xyz", true, &mut input, &mut out)
            .is_ok()
    );
}

#[test]
fn formats_page_with_a_cursor_and_a_stale_one_is_state() {
    setup!(table, state, host, docs);
    let (mut input, mut out) = ([0; 256], [0; 4096]);
    reply(
        &state,
        &[
            format("a.epub", "epub"),
            format("b.txt", "txt"),
            end(0, 3, 9, 2),
        ],
    );
    let page = docs
        .formats(&host, FormatCursor::start(), &mut input, &mut out)
        .unwrap();
    assert_eq!(state.op.get(), schema::OP_FORMATS);
    assert_eq!((page.len(), page.end().total()), (2, 3));
    assert_eq!(
        page.rows().map(|f| f.id()).collect::<Vec<_>>(),
        ["a.epub", "b.txt"]
    );
    let next = page.end().next().unwrap();
    reply(&state, &[format("c.fb2", "fb2"), end(1, 3, 9, 3)]);
    let page = docs.formats(&host, next, &mut input, &mut out).unwrap();
    assert_eq!((page.len(), page.end().next()), (1, None));
    {
        let bytes = state.input.borrow();
        let sent = wire::records_decode(&bytes).unwrap();
        assert!(matches!(
            sent.iter().next(),
            Some(Record::ServiceDocPage(p))
                if (p.document, p.scope, p.cursor, p.generation) == (0, "", 2, 9)
        ));
    }
    reply(&state, &[format("c.fb2", "fb2"), end(1, 3, 10, 3)]);
    assert_eq!(
        docs.formats(&host, next, &mut input, &mut out).unwrap_err(),
        Error::State
    );
}

#[test]
fn malformed_format_pages_are_refused() {
    setup!(table, state, host, docs);
    let (mut input, mut out) = ([0; 256], [0; 4096]);
    let start = FormatCursor::start();
    let mut ask = |rows: &[Record<'_>]| {
        reply(&state, rows);
        docs.formats(&host, start, &mut input, &mut out)
            .map(|page| page.len())
    };
    assert_eq!(ask(&[format("a.b", ""), end(1, 1, 3, 1)]), Ok(1));
    for bad in [
        vec![],
        vec![format("a.b", "x")],
        vec![format("a.b", "x"), end(1, 1, 0, 1)],
        vec![format("a.b", "x"), end(1, 2, 3, 1)],
        vec![end(0, 1, 3, 0)],
        vec![format("a.b", "x"), end(3, 1, 3, 1)],
        vec![format("", "x"), end(1, 1, 3, 1)],
        vec![format("a.b", "x\n\ny"), end(1, 1, 3, 1)],
        vec![end(1, 0, 3, 0), format("a.b", "x")],
    ] {
        assert_eq!(ask(&bad), Err(Error::Invalid), "{bad:?}");
    }
    let rows: Vec<Record<'static>> = (0..65).map(|_| format("a.b", "x")).collect();
    let mut over = rows.clone();
    over.push(end(1, 65, 3, 65));
    assert_eq!(ask(&over), Err(Error::Invalid));
    let mut full = rows[..64].to_vec();
    full.push(end(1, 64, 3, 64));
    assert_eq!(ask(&full), Ok(64));
}
