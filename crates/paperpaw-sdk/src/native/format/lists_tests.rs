use super::{Error, FormatCall, FormatRows, LinkKind, PageEnd, schema};

fn call<'a>(op: u32, input: &'a mut [u8]) -> FormatCall<'a> {
    let n = schema::FmtCall::new(op, 9, 0, 0, 0, 0, 0, 0, 600, 300, 22, "", &[])
        .encode(input)
        .unwrap();
    FormatCall::decode(&input[..n]).unwrap()
}
fn records(bytes: &[u8]) -> alloc::vec::Vec<schema::Record<'_>> {
    schema::reply_decode(bytes)
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

#[test]
fn a_page_of_sections_ends_with_the_end_record() {
    let mut input = [0; 256];
    let mut out = [0; 3840];
    let c = call(7, &mut input);
    let mut rows = c.rows(&mut out).unwrap();
    rows.section(0, 100, false).unwrap();
    rows.section(1, 7, true).unwrap();
    assert_eq!((rows.len(), rows.is_empty()), (2, false));
    let n = rows.finish(PageEnd::last(2)).unwrap();
    let got = records(&out[..n]);
    assert!(
        matches!(got[0], schema::Record::FmtSection(s) if (s.index, s.length, s.flags) == (0, 100, 0))
    );
    assert!(matches!(got[1], schema::Record::FmtSection(s) if (s.index, s.flags) == (1, 1)));
    assert!(
        matches!(got[2], schema::Record::FmtEnd(e) if (e.flags, e.total, e.next, e.generation) == (1, 2, 0, 0))
    );
    assert_eq!(got.len(), 3);
}

#[test]
fn a_continued_page_names_the_next_cursor_and_generation() {
    let mut input = [0; 256];
    let mut out = [0; 3840];
    let c = call(9, &mut input);
    let mut rows = c.rows(&mut out).unwrap();
    rows.toc(0, 0, 0, 0, false, "One").unwrap();
    let n = rows.finish(PageEnd::more(500, 1, 77)).unwrap();
    let got = records(&out[..n]);
    assert!(
        matches!(got[1], schema::Record::FmtEnd(e) if (e.flags, e.total, e.next, e.generation) == (0, 500, 1, 77))
    );
    // A continued page needs a generation.
    let mut out = [0; 3840];
    let c = call(9, &mut input);
    let rows = c.rows(&mut out).unwrap();
    assert_eq!(rows.finish(PageEnd::more(5, 1, 0)), Err(Error::Invalid));
}

#[test]
fn rows_check_the_operation_and_their_fields() {
    let mut input = [0; 256];
    let mut out = [0; 3840];
    let c = call(7, &mut input);
    let mut rows = c.rows(&mut out).unwrap();
    assert_eq!(rows.toc(0, 0, 0, 0, false, "x"), Err(Error::State));
    assert_eq!(rows.font(0, 1, false, false, 1), Err(Error::State));
    assert_eq!(rows.link(0, 1, LinkKind::Internal, "#a"), Err(Error::State));
    let c = call(9, &mut input);
    let mut out2 = [0; 3840];
    let mut toc = c.rows(&mut out2).unwrap();
    assert_eq!(toc.toc(0, 16, 0, 0, false, "x"), Err(Error::Invalid));
    assert_eq!(toc.section(0, 1, false), Err(Error::State));
    let c = call(16, &mut input);
    let mut out3 = [0; 3840];
    let mut fonts = c.rows(&mut out3).unwrap();
    for (index, family) in [(124, 1), (0, 0), (0, 32)] {
        assert_eq!(
            fonts.font(index, family, false, false, 1),
            Err(Error::Invalid)
        );
    }
    fonts.font(123, 31, true, true, 9).unwrap();
    let c = call(14, &mut input);
    let mut out4 = [0; 3840];
    let mut links = c.rows(&mut out4).unwrap();
    assert_eq!(
        links.link(5, 4, LinkKind::Internal, "#a"),
        Err(Error::Invalid)
    );
    // Not a list operation, and nothing to write into.
    assert!(matches!(
        call(5, &mut input).rows(&mut out),
        Err(Error::State)
    ));
    assert!(matches!(
        call(7, &mut input).rows(&mut []),
        Err(Error::NoSpace)
    ));
}

#[test]
fn a_toc_title_is_cut_at_a_character_and_flagged() {
    let mut input = [0; 256];
    let mut out = [0; 3840];
    let c = call(9, &mut input);
    let mut rows = c.rows(&mut out).unwrap();
    rows.toc(3, 2, 4, 5, true, &"界".repeat(200)).unwrap();
    let n = rows.finish(PageEnd::last(1)).unwrap();
    let got = records(&out[..n]);
    let schema::Record::FmtToc(row) = got[0] else {
        panic!("not a toc row")
    };
    assert_eq!(
        (row.index, row.level, row.section, row.offset),
        (3, 2, 4, 5)
    );
    assert_eq!((row.flags, row.title.len()), (3, 510));
}

#[test]
fn a_link_with_an_overlong_href_is_skipped_not_an_error() {
    let mut input = [0; 256];
    let mut out = [0; 3840];
    let c = call(14, &mut input);
    let mut rows = c.rows(&mut out).unwrap();
    assert_eq!(
        rows.link(0, 3, LinkKind::External, &"h".repeat(1025)),
        Ok(false)
    );
    assert_eq!(
        rows.link(0, 3, LinkKind::Footnote, &"h".repeat(1024)),
        Ok(true)
    );
    assert_eq!(rows.len(), 1);
    let n = rows.finish(PageEnd::last(1)).unwrap();
    let got = records(&out[..n]);
    assert!(matches!(got[0], schema::Record::FmtLink(l) if (l.kind, l.href.len()) == (2, 1024)));
}

/// However the page fills, ending it succeeds and decodes: rows never take the
/// room of the closing record.
#[test]
fn a_full_page_can_always_be_ended() {
    let mut input = [0; 256];
    for size in 1..220 {
        let mut out = alloc::vec![0xa5; size];
        let c = call(7, &mut input);
        let Ok(mut rows) = c.rows(&mut out) else {
            continue;
        };
        let mut pushed = 0;
        while rows.section(pushed, 1, false).is_ok() {
            pushed += 1;
        }
        let n = rows.finish(PageEnd::more(1000, pushed, 1)).unwrap();
        let got = records(&out[..n]);
        assert_eq!(got.len(), pushed as usize + 1, "{size}");
    }
}

#[test]
fn a_page_holds_at_most_the_reply_rows_including_the_end() {
    let mut input = [0; 256];
    let mut out = [0; 3840];
    let c = call(7, &mut input);
    let mut rows: FormatRows<'_> = c.rows(&mut out).unwrap();
    let mut pushed = 0;
    while rows.section(pushed, 1, false).is_ok() {
        pushed += 1;
    }
    assert!(pushed as usize <= schema::REPLY_ROWS - 1);
    let n = rows.finish(PageEnd::more(9999, pushed, 1)).unwrap();
    assert_eq!(records(&out[..n]).len(), pushed as usize + 1);
}

#[test]
fn a_picture_reply_checks_the_operation_and_path() {
    let mut input = [0; 256];
    let mut out = [0xa5; 512];
    let c = call(17, &mut input);
    let n = c
        .reply_image("images/0.pgm", u32::MAX, 300, 400, false, &mut out)
        .unwrap();
    let schema::Record::FmtImage(row) = records(&out[..n])[0] else {
        panic!("not an image")
    };
    assert_eq!(
        (row.path, row.index, row.width, row.height, row.flags),
        ("images/0.pgm", u32::MAX, 300, 400, 1)
    );
    assert_eq!(
        c.reply_image("", 0, 1, 1, true, &mut out),
        Err(Error::Invalid)
    );
    assert_eq!(
        c.reply_image(&"p".repeat(257), 0, 1, 1, true, &mut out),
        Err(Error::Invalid)
    );
    assert_eq!(
        call(5, &mut input).reply_image("a", 0, 1, 1, true, &mut out),
        Err(Error::State)
    );
    let n = call(18, &mut input)
        .reply_image("a", 3, 1, 1, true, &mut out)
        .unwrap();
    assert!(matches!(records(&out[..n])[0], schema::Record::FmtImage(r) if r.flags == 0));
}
