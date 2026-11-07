use super::*;

fn request(op: u32) -> schema::FmtCall<'static> {
    schema::FmtCall::new(
        op,
        9,
        2,
        37,
        3,
        4,
        5,
        0,
        600,
        300,
        123,
        "chapter.txt",
        b"book",
    )
}
fn decode<'a>(row: schema::FmtCall<'_>, bytes: &'a mut [u8]) -> FormatCall<'a> {
    let n = row.encode(bytes).unwrap();
    FormatCall::decode(&bytes[..n]).unwrap()
}
#[test]
fn requests_preserve_provider_operations_and_source_positions() {
    let mut input = [0; 1024];
    let variants = [
        FormatOp::Sniff,
        FormatOp::Open,
        FormatOp::Prepare,
        FormatOp::Close,
        FormatOp::Meta,
        FormatOp::MetaText,
        FormatOp::Sections,
        FormatOp::Block,
        FormatOp::Toc,
        FormatOp::TocTarget,
        FormatOp::PageLabel,
        FormatOp::Xpointer,
        FormatOp::Locate,
        FormatOp::Links,
        FormatOp::LinkTarget,
        FormatOp::Fonts,
        FormatOp::Cover,
        FormatOp::Image,
        FormatOp::Progress,
    ];
    let range = input.as_ptr_range();
    for (index, op) in variants.into_iter().enumerate() {
        let call = decode(request(u32::try_from(index + 1).unwrap()), &mut input);
        assert_eq!(call.operation(), op);
        assert_eq!(
            (call.document(), call.section(), call.offset(), call.index()),
            (9, 2, 37, 3)
        );
        assert_eq!(
            (call.cursor(), call.generation(), call.conversion()),
            (4, 5, 123)
        );
        assert_eq!((call.image_side(), call.cover_side()), (600, 300));
        assert_eq!(
            (call.text(), call.sniff_bytes()),
            ("chapter.txt", b"book".as_slice())
        );
        assert!(range.contains(&call.text().as_ptr()));
        assert!(call.options().next().is_none());
    }
    let mut row = request(2);
    row.flags = 7;
    let call = decode(row, &mut input);
    assert!(call.publisher_styles() && call.color() && call.background());
}
#[test]
fn malformed_or_unknown_calls_fail_before_use() {
    let mut input = [0; 1024];
    for op in [0, 20, u32::MAX] {
        let n = request(op).encode(&mut input).unwrap();
        assert!(matches!(
            FormatCall::decode(&input[..n]),
            Err(Error::Unsupported)
        ));
    }
    let mut row = request(8);
    row.document = 0;
    let n = row.encode(&mut input).unwrap();
    assert!(matches!(
        FormatCall::decode(&input[..n]),
        Err(Error::Invalid)
    ));
    row.op = 1;
    let n = row.encode(&mut input).unwrap();
    assert_eq!(FormatCall::decode(&input[..n]).unwrap().document(), 0);
    for n in 0..n {
        assert!(FormatCall::decode(&input[..n]).is_err());
    }
    input[n] = 0;
    assert!(FormatCall::decode(&input[..=n]).is_err());
    let n = schema::FmtOption::new("format-txt.encoding", "utf-16")
        .encode(&mut input)
        .unwrap();
    assert!(matches!(
        FormatCall::decode(&input[..n]),
        Err(Error::Invalid)
    ));
    for (op, flags, side) in [(2, 8, 600), (6, 5, 600), (2, 0, 1601), (18, 0, 1601)] {
        row = request(op);
        row.flags = flags;
        row.image_side = side;
        let n = row.encode(&mut input).unwrap();
        assert!(matches!(
            FormatCall::decode(&input[..n]),
            Err(Error::Invalid)
        ));
    }
}
#[test]
fn state_replies_retain_identity_and_validate_errors_and_caps() {
    let mut input = [0; 1024];
    let call = decode(request(2), &mut input);
    let mut out = [0xa5; 1024];
    let flags = FormatFeatures::new()
        .table_of_contents()
        .xpointers()
        .cache_made();
    let n = call
        .reply_state(
            "format-txt.txt",
            FormatState::preparing(123).unwrap(),
            flags,
            &mut out,
        )
        .unwrap();
    let row = schema::FmtState::decode(&out[..n]).unwrap();
    assert_eq!(
        (row.document, row.conversion, row.state, row.progress_ppm),
        (9, 123, 1, 123)
    );
    assert_eq!(
        (row.flags, row.status, row.format),
        (70, 0, "format-txt.txt")
    );
    let n = call
        .reply_state("format-txt.txt", FormatState::ready(), flags, &mut out)
        .unwrap();
    assert_eq!(schema::FmtState::decode(&out[..n]).unwrap().state, 0);
    let n = call
        .reply_state(
            "format-txt.txt",
            FormatState::failed(Error::Io).unwrap(),
            flags,
            &mut out,
        )
        .unwrap();
    let row = schema::FmtState::decode(&out[..n]).unwrap();
    assert_eq!((row.state, row.status), (2, Error::Io.status()));
    assert!(FormatState::preparing(1_000_001).is_err());
    assert!(FormatState::failed(Error::Other(0)).is_err());
    assert!(FormatState::failed(Error::Other(42)).is_err());
    out.fill(0xa5);
    assert!(matches!(
        call.reply_state("bad-id", FormatState::ready(), flags, &mut out),
        Err(Error::Invalid)
    ));
    assert_eq!(out, [0xa5; 1024]);
    assert!(matches!(
        call.reply_state("format-txt.txt", FormatState::ready(), flags, &mut out[..1]),
        Err(Error::NoSpace)
    ));
    assert_eq!(out, [0xa5; 1024]);
}
#[test]
fn detail_replies_are_operation_specific_and_atomic() {
    let mut input = [0; 1024];
    let mut out = [0xa5; 1024];
    let call = decode(request(19), &mut input);
    assert!(matches!(
        call.reply_progress(1_000_001, &mut out),
        Err(Error::Invalid)
    ));
    assert!(matches!(call.reply_label("i", &mut out), Err(Error::State)));
    assert!(matches!(
        call.reply_progress(500, &mut out[..1]),
        Err(Error::NoSpace)
    ));
    assert_eq!(out, [0xa5; 1024]);
    let n = call.reply_progress(1_000_000, &mut out).unwrap();
    assert_eq!(
        schema::FmtProgress::decode(&out[..n]).unwrap().progress_ppm,
        1_000_000
    );
    let call = decode(request(10), &mut input);
    let n = call.reply_position(3, 91, &mut out).unwrap();
    let row = schema::FmtAt::decode(&out[..n]).unwrap();
    assert_eq!(
        (
            row.document,
            row.section,
            row.offset,
            row.insert,
            row.cursor,
            row.generation
        ),
        (9, 3, 91, 0, 0, 0)
    );
    let call = decode(request(11), &mut input);
    let n = call.reply_label("ix", &mut out).unwrap();
    assert_eq!(schema::FmtLabel::decode(&out[..n]).unwrap().text, "ix");
    out.fill(0xa5);
    assert!(matches!(
        call.reply_label(&"x".repeat(33), &mut out),
        Err(Error::NoSpace)
    ));
    assert_eq!(out, [0xa5; 1024]);
    let call = decode(request(12), &mut input);
    let n = call.reply_xpointer("/book/paragraph", &mut out).unwrap();
    let row = schema::FmtXpointer::decode(&out[..n]).unwrap();
    assert_eq!((row.document, row.text), (9, "/book/paragraph"));
    let call = decode(request(1), &mut input);
    let n = call.reply_accept(&mut out).unwrap();
    assert_eq!(schema::FmtYes::decode(&out[..n]).unwrap().flags, 0);
    let n = call.reply_decline(&mut out).unwrap();
    assert_eq!(schema::reply_decode(&out[..n]).unwrap().len(), 0);
    let call = decode(request(4), &mut input);
    let n = call.reply_closed(&mut out).unwrap();
    assert_eq!(schema::reply_decode(&out[..n]).unwrap().len(), 0);
    out.fill(0xa5);
    assert!(matches!(call.reply_accept(&mut out), Err(Error::State)));
    assert!(matches!(
        call.reply_state(
            "format-txt.txt",
            FormatState::ready(),
            FormatFeatures::new(),
            &mut out
        ),
        Err(Error::State)
    ));
    assert_eq!(out, [0xa5; 1024]);
}
