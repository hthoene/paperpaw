//! Op 6: the block stream in fragments (`document.md` 4).
use super::*;
use wire::{
    ServiceDocBlock as Block, ServiceDocCell as Cell, ServiceDocColumns as Columns,
    ServiceDocInlineLang as Lang, ServiceDocMap as Map, ServiceDocRuby as Ruby,
    ServiceDocRun as Run, ServiceDocTextPart as Part,
};

fn header(kind: u32, text_bytes: u32) -> Record<'static> {
    Record::ServiceDocBlock(Block {
        kind,
        level: if kind == 1 { 2 } else { 0 },
        flags: 0,
        start: 0,
        end: text_bytes,
        align: 0,
        indent: i32::MIN,
        space: u32::MAX,
        size: u32::MAX,
        margin_start: 0,
        margin_end: 0,
        line_height: u32::MAX,
        dir: 0,
        writing_mode: 0,
        lang: "en",
        drop_cap: 0,
        text_bytes,
    })
}
fn part(text: &str) -> Record<'_> {
    Record::ServiceDocTextPart(Part { text })
}
fn run(start: u32, end: u32) -> Record<'static> {
    Record::ServiceDocRun(Run {
        start,
        end,
        style: 0,
    })
}
fn cell() -> Record<'static> {
    Record::ServiceDocCell(Cell {
        end: 1,
        span: 1,
        rows: 1,
        align: 0,
        vertical: 0,
        width: 0,
    })
}
fn end(flags: u32, generation: u32, next: u32) -> Record<'static> {
    Record::ServiceEnd(wire::ServiceEnd {
        flags,
        total: 0,
        generation,
        next,
        cursor: None,
    })
}
fn image() -> Record<'static> {
    Record::ServiceDocImage(Image {
        image: 0,
        index: 8,
        width: 90,
        height: 60,
        flags: 0,
    })
}
fn ask(document: u32, cursor: u32, generation: u32) -> Record<'static> {
    Record::ServiceDocAt(At {
        document,
        section: 2,
        offset: 5,
        insert: 0,
        cursor,
        generation,
    })
}
fn block_call(
    fake: &Rc<Fake>,
    id: u32,
    cursor: u32,
    generation: u32,
    answer: &[Record<'_>],
) -> Result<Vec<u8>, i32> {
    fake.say(answer);
    call(fake, 6, &[ask(id, cursor, generation)])
}

#[test]
fn a_whole_block_passes_on_with_its_end() {
    let fake = Fake::new(Limits::X4);
    let id = opened(&fake);
    let record = [
        header(0, 8),
        part("hello"),
        part("abc"),
        run(0, 3),
        run(3, 8),
        Record::ServiceDocMap(Map {
            text: 0,
            source: 10,
        }),
        Record::ServiceDocRuby(Ruby {
            start: 0,
            end: 1,
            kind: 0,
            size: 10,
            text: "r",
        }),
        Record::ServiceDocInlineLang(Lang { lang: "de" }),
        end(1, 0, 91),
    ];
    let reply = block_call(&fake, id, 0, 0, &record).unwrap();
    assert_eq!(reply, stream(&record));
    assert_eq!(
        *fake.calls.borrow().last().unwrap(),
        [fmt::BLOCK, id, 0, 0, 0, 0, 0, 0]
    );
}

#[test]
fn the_largest_text_part_and_a_long_ruby_fit_one_fragment() {
    let fake = Fake::new(Limits::X4);
    let id = opened(&fake);
    let text = "x".repeat(3072);
    let one = [header(0, 3072), part(&text), end(1, 0, 3072)];
    let bytes = stream(&one);
    assert!(bytes.len() <= 3840);
    assert_eq!(block_call(&fake, id, 0, 0, &one).unwrap(), bytes);
    let ruby = "r".repeat(1024);
    let one = [
        header(0, 0),
        Record::ServiceDocRuby(Ruby {
            start: 0,
            end: 1,
            kind: 0,
            size: 10,
            text: &ruby,
        }),
        end(1, 0, 2),
    ];
    assert_eq!(block_call(&fake, id, 0, 0, &one).unwrap(), stream(&one));
    // A fragment is at most the stage payload, 3,840 bytes: the host's reply
    // buffer is larger, but a format that fills it breaks the transport.
    let text = "x".repeat(3072);
    let more = "y".repeat(800);
    let over = [header(0, 3872), part(&text), part(&more), end(1, 0, 3)];
    assert!(stream(&over).len() > 3840);
    assert_eq!(block_call(&fake, id, 0, 0, &over), Err(ERR_INVALID));
}

/// Fragment 0 of an 8-byte block ("hel", 2 records, identity 77), then `second`
/// asked from record 2 under that identity on the same document.
fn continued(second: &[Record<'_>]) -> Result<Vec<u8>, i32> {
    let fake = Fake::new(Limits::X4);
    let id = opened(&fake);
    block_call(&fake, id, 0, 0, &[header(0, 8), part("hel"), end(2, 77, 2)]).unwrap();
    block_call(&fake, id, 2, 77, second)
}

#[test]
fn a_long_block_comes_in_fragments_cut_between_records() {
    let fake = Fake::new(Limits::X4);
    let id = opened(&fake);
    let fresh = |cursor: u32, generation: u32, answer: &[Record<'_>]| {
        let fake = Fake::new(Limits::X4);
        let id = opened(&fake);
        block_call(&fake, id, cursor, generation, answer)
    };
    // Fragment 0: header and the first part; the rest is asked by record index.
    let reply = block_call(&fake, id, 0, 0, &[header(0, 8), part("hel"), end(2, 77, 2)]).unwrap();
    assert!(!reply.is_empty());
    // Fragment 1 starts mid-list (no header), under the block's generation.
    let last = [part("lo!!!"), run(0, 8), end(1, 77, 99)];
    assert!(block_call(&fake, id, 2, 77, &last).is_ok());
    assert_eq!(
        *fake.calls.borrow().last().unwrap(),
        [fmt::BLOCK, id, 2, 77, 0, 0, 0, 0]
    );
    // The finished block is forgotten: asking on from it is a stale cursor.
    assert_eq!(block_call(&fake, id, 99, 77, &last), Err(ERR_STATE));
    // A changed generation is the format's ERR_STATE and the host's too; so is
    // a last fragment that dropped it.
    assert_eq!(continued(&[part("x"), end(1, 78, 99)]), Err(ERR_STATE));
    assert_eq!(continued(&[part("lo!!!"), end(1, 0, 99)]), Err(ERR_STATE));
    // A later fragment never carries a header; the first always does.
    assert_eq!(
        continued(&[header(0, 1), part("x"), end(1, 77, 9)]),
        Err(ERR_INVALID)
    );
    assert_eq!(fresh(0, 0, &[part("x"), end(1, 0, 9)]), Err(ERR_INVALID));
    // `more` needs a generation and a `next` that is the next record index.
    assert_eq!(
        fresh(0, 0, &[header(0, 8), part("hel"), end(2, 0, 2)]),
        Err(ERR_INVALID)
    );
    assert_eq!(
        fresh(0, 0, &[header(0, 8), part("hel"), end(2, 77, 1)]),
        Err(ERR_INVALID)
    );
    assert_eq!(
        fresh(0, 0, &[header(0, 8), part("hel"), end(2, 77, 3)]),
        Err(ERR_INVALID)
    );
    // An empty fragment would loop its reader.
    assert_eq!(fresh(0, 0, &[end(2, 77, 0)]), Err(ERR_INVALID));
    assert_eq!(continued(&[end(1, 77, 9)]), Err(ERR_INVALID));
}

#[test]
fn fragments_are_checked_as_one_block_not_one_by_one() {
    // The parts add up across fragments: short or long is invalid.
    assert!(continued(&[part("lo!!!"), end(1, 77, 9)]).is_ok());
    assert_eq!(
        continued(&[part("x"), end(1, 77, 9)]),
        Err(ERR_INVALID),
        "short"
    );
    assert_eq!(
        continued(&[part("lo!!!!"), end(1, 77, 9)]),
        Err(ERR_INVALID),
        "long"
    );
    // A run past the whole text, and the order of the list, hold across them.
    assert_eq!(
        continued(&[part("lo!!!"), run(0, 9), end(1, 77, 9)]),
        Err(ERR_INVALID)
    );
    let fake = Fake::new(Limits::X4);
    let id = opened(&fake);
    let first = [header(0, 8), part("hel"), run(0, 2), end(2, 77, 3)];
    block_call(&fake, id, 0, 0, &first).unwrap();
    assert_eq!(
        block_call(&fake, id, 3, 77, &[part("lo!!!"), end(1, 77, 9)]),
        Err(ERR_INVALID),
        "text after runs"
    );
    assert_eq!(
        block_call(&fake, id, 3, 77, &[run(1, 3), part_end(), end(1, 77, 9)]),
        Err(ERR_INVALID),
        "runs overlap across fragments"
    );
    // A cursor the host did not hand out, on a block nobody began, is stale.
    let other = Fake::new(Limits::X4);
    let id = opened(&other);
    assert_eq!(
        block_call(&other, id, 2, 77, &[part("lo!!!"), end(1, 77, 9)]),
        Err(ERR_STATE)
    );
    // A failed fragment leaves the block as it was: the same ask can be retried.
    let retry = Fake::new(Limits::X4);
    let id = opened(&retry);
    block_call(
        &retry,
        id,
        0,
        0,
        &[header(0, 8), part("hel"), end(2, 77, 2)],
    )
    .unwrap();
    assert_eq!(
        block_call(&retry, id, 2, 77, &[part("x"), end(1, 77, 9)]),
        Err(ERR_INVALID)
    );
    assert!(block_call(&retry, id, 2, 77, &[part("lo!!!"), end(1, 77, 9)]).is_ok());
    // Another position than the one begun is not its continuation.
    let moved = Fake::new(Limits::X4);
    let id = opened(&moved);
    block_call(
        &moved,
        id,
        0,
        0,
        &[header(0, 8), part("hel"), end(2, 77, 2)],
    )
    .unwrap();
    assert_eq!(
        block_call(&moved, id, 3, 77, &[part("lo!!!"), end(1, 77, 9)]),
        Err(ERR_STATE)
    );
}

fn part_end() -> Record<'static> {
    run(3, 4)
}

#[test]
fn table_row_flags_carry_header_first_and_last_bits() {
    let flagged = |flags: u32| {
        let fake = Fake::new(Limits::X4);
        let id = opened(&fake);
        let Record::ServiceDocBlock(mut block) = header(4, 0) else {
            unreachable!()
        };
        block.flags = flags;
        let answer = [
            Record::ServiceDocBlock(block),
            Record::ServiceDocColumns(Columns { weights: &[1, 0] }),
            cell(),
            end(1, 0, 9),
        ];
        block_call(&fake, id, 0, 0, &answer)
    };
    for flags in [0, 1 << 8, 1 << 9, 1 << 10, 0b111 << 8, 0b11 | (0b111 << 8)] {
        assert!(flagged(flags).is_ok(), "{flags:#x}");
    }
    for flags in [1 << 2, 1 << 7, 1 << 11, 1 << 31] {
        assert_eq!(flagged(flags), Err(ERR_INVALID), "{flags:#x}");
    }
}

#[test]
fn a_block_that_breaks_the_list_is_invalid() {
    // Three malformed answers close a document: each case has its own.
    let bad = |answer: &[Record<'_>]| {
        let fake = Fake::new(Limits::X4);
        let id = opened(&fake);
        block_call(&fake, id, 0, 0, answer)
    };
    // Text parts must add up to text_bytes when the block ends.
    assert_eq!(
        bad(&[header(0, 8), part("hi"), end(1, 0, 9)]),
        Err(ERR_INVALID)
    );
    assert_eq!(
        bad(&[header(0, 1), part("hi"), end(1, 0, 9)]),
        Err(ERR_INVALID)
    );
    // Order: header, parts, runs, maps, image/columns, cells, rubies, languages.
    assert_eq!(
        bad(&[header(0, 2), run(0, 1), part("hi"), end(1, 0, 9)]),
        Err(ERR_INVALID)
    );
    assert_eq!(
        bad(&[header(0, 0), header(0, 0), end(1, 0, 9)]),
        Err(ERR_INVALID)
    );
    assert_eq!(
        bad(&[
            header(0, 0),
            Record::ServiceDocInlineLang(Lang { lang: "de" }),
            Record::ServiceDocMap(Map { text: 0, source: 0 }),
            end(1, 0, 9)
        ]),
        Err(ERR_INVALID)
    );
    // Runs are sorted, not overlapping and inside the text.
    assert_eq!(
        bad(&[
            header(0, 4),
            part("abcd"),
            run(0, 3),
            run(2, 4),
            end(1, 0, 9)
        ]),
        Err(ERR_INVALID)
    );
    assert_eq!(
        bad(&[header(0, 4), part("abcd"), run(3, 2), end(1, 0, 9)]),
        Err(ERR_INVALID)
    );
    assert_eq!(
        bad(&[header(0, 4), part("abcd"), run(0, 5), end(1, 0, 9)]),
        Err(ERR_INVALID)
    );
    // Heading levels 1-6; known flags and directions.
    let heading = |level, flags, dir| {
        let Record::ServiceDocBlock(mut block) = header(1, 0) else {
            unreachable!()
        };
        block.level = level;
        block.flags = flags;
        block.dir = dir;
        Record::ServiceDocBlock(block)
    };
    assert_eq!(bad(&[heading(0, 0, 0), end(1, 0, 9)]), Err(ERR_INVALID));
    assert_eq!(bad(&[heading(7, 0, 0), end(1, 0, 9)]), Err(ERR_INVALID));
    assert_eq!(bad(&[heading(6, 4, 0), end(1, 0, 9)]), Err(ERR_INVALID));
    assert_eq!(bad(&[heading(6, 3, 2), end(1, 0, 9)]), Err(ERR_INVALID));
    assert!(bad(&[heading(6, 3, 1), end(1, 0, 9)]).is_ok());
    // An unknown record, a stray row and anything after the end.
    assert_eq!(
        bad(&[
            header(0, 0),
            Record::ServiceDocLabel(wire::ServiceDocLabel { text: "x" }),
            end(1, 0, 9)
        ]),
        Err(ERR_INVALID)
    );
    assert_eq!(bad(&[header(0, 0)]), Err(ERR_INVALID));
    assert_eq!(
        bad(&[header(0, 0), end(1, 0, 9), end(1, 0, 9)]),
        Err(ERR_INVALID)
    );
}

#[test]
fn image_and_table_blocks_have_exactly_their_records() {
    // Three malformed answers close a document: each case has its own.
    let bad = |answer: &[Record<'_>]| {
        let fake = Fake::new(Limits::X4);
        let id = opened(&fake);
        block_call(&fake, id, 0, 0, answer)
    };
    let columns = |weights: &'static [u8]| Record::ServiceDocColumns(Columns { weights });
    assert!(bad(&[header(3, 0), image(), end(1, 0, 9)]).is_ok());
    // A picture block without its picture keeps its alt text (`document.md`
    // 5); see `a_picture_block_without_its_picture_never_closes_the_document`.
    assert!(bad(&[header(3, 0), end(1, 0, 9)]).is_ok(), "no picture");
    assert_eq!(
        bad(&[header(3, 0), image(), image(), end(1, 0, 9)]),
        Err(ERR_INVALID)
    );
    assert_eq!(
        bad(&[header(0, 0), image(), end(1, 0, 9)]),
        Err(ERR_INVALID),
        "not an image block"
    );
    assert!(
        bad(&[
            header(4, 0),
            columns(&[1, 0, 2, 0]),
            cell(),
            cell(),
            end(1, 0, 9)
        ])
        .is_ok()
    );
    assert_eq!(
        bad(&[header(4, 0), cell(), end(1, 0, 9)]),
        Err(ERR_INVALID),
        "no columns"
    );
    assert_eq!(
        bad(&[header(0, 0), columns(&[1, 0]), end(1, 0, 9)]),
        Err(ERR_INVALID)
    );
    assert_eq!(
        bad(&[header(4, 0), columns(&[1]), end(1, 0, 9)]),
        Err(ERR_INVALID),
        "odd"
    );
    assert!(bad(&[header(4, 0), columns(&[0; 32]), end(1, 0, 9)]).is_ok());
    let cells = |n: usize| -> Vec<u8> {
        let mut all = std::vec![header(4, 0), columns(&[1, 0])];
        all.extend((0..n).map(|_| cell()));
        all.push(end(1, 0, 9));
        stream(&all)
    };
    let fake = Fake::new(Limits::X4);
    let id = opened(&fake);
    *fake.answer.borrow_mut() = cells(16);
    assert!(call(&fake, 6, &[ask(id, 0, 0)]).is_ok(), "16 cells");
    *fake.answer.borrow_mut() = cells(17);
    assert_eq!(
        call(&fake, 6, &[ask(id, 0, 0)]),
        Err(ERR_INVALID),
        "17 cells"
    );
    let langs = |n: usize| -> Vec<u8> {
        let mut all = std::vec![header(0, 0)];
        all.extend((0..n).map(|_| Record::ServiceDocInlineLang(Lang { lang: "de" })));
        all.push(end(1, 0, 9));
        stream(&all)
    };
    *fake.answer.borrow_mut() = langs(15);
    assert!(call(&fake, 6, &[ask(id, 0, 0)]).is_ok(), "15 languages");
    *fake.answer.borrow_mut() = langs(16);
    assert_eq!(
        call(&fake, 6, &[ask(id, 0, 0)]),
        Err(ERR_INVALID),
        "16 languages"
    );
}

#[test]
fn pictures_in_a_stream_get_ids_in_place_and_a_bad_one_never_fails_the_stream() {
    let fake = Fake::new(Limits::X4);
    let id = opened(&fake);
    let stream_of = [header(3, 0), image(), end(1, 0, 9)];
    let plain = stream(&stream_of);
    let mapped = block_call(&fake, id, 0, 0, &stream_of).unwrap();
    assert_eq!(
        mapped.len(),
        plain.len(),
        "the same fixed-width record, in place"
    );
    let (_, used) = wire::record_decode_prefix(&mapped).unwrap();
    let Record::ServiceDocImage(picture) = wire::record_decode_prefix(&mapped[used..]).unwrap().0
    else {
        panic!("not an image")
    };
    assert_eq!(
        (picture.image, picture.index, picture.width, picture.height),
        (41, 8, 90, 60)
    );
    // All ids out and none drawn yet: the row stays with image 0 and its index.
    fake.image.set(Some(Some(0)));
    let unplaced = block_call(&fake, id, 0, 0, &stream_of).unwrap();
    assert_eq!(unplaced, plain);
    // A picture that fails its check is left out; the end is untouched. The
    // block keeps its alt text, so the stream still checks out as a whole.
    fake.image.set(Some(None));
    let reply = block_call(&fake, id, 0, 0, &stream_of).unwrap();
    assert_eq!(reply, stream(&[header(3, 0), end(1, 0, 9)]));
    assert_eq!(*fake.left_out.borrow(), [id], "said, for that document");
}

/// #243: a picture block whose picture the format left out (or the host's
/// adapter dropped before the check) costs only that picture: three of them
/// in a row, more than the malformed answers that strike a document, and the
/// document still reads, with the block's alt text.
#[test]
fn a_picture_block_without_its_picture_never_closes_the_document() {
    let fake = Fake::new(Limits::X4);
    let id = opened(&fake);
    let alt = [header(3, 3), part("owl"), end(1, 0, 9)];
    for _ in 0..4 {
        assert_eq!(block_call(&fake, id, 0, 0, &alt), Ok(stream(&alt)));
    }
    assert_eq!(*fake.left_out.borrow(), [id; 4]);
    // The document is still open: a plain block reads as ever.
    let plain = [header(0, 3), part("owl"), end(1, 0, 9)];
    assert_eq!(block_call(&fake, id, 0, 0, &plain), Ok(stream(&plain)));
    // Two pictures in one block stay malformed.
    let two = [header(3, 0), image(), image(), end(1, 0, 9)];
    assert_eq!(block_call(&fake, id, 0, 0, &two), Err(ERR_INVALID));
    // In fragments, the picture is judged only at the block's last one.
    let first = [header(3, 3), part("o"), end(3, 7, 2)];
    assert!(block_call(&fake, id, 0, 0, &first).is_ok());
    assert_eq!(fake.left_out.borrow().len(), 4, "not before the block ends");
}

#[test]
fn a_block_has_at_most_one_columns_record_and_only_a_table_row_has_it() {
    let columns = || Record::ServiceDocColumns(Columns { weights: &[1, 0] });
    let whole = |answer: &[Record<'_>]| {
        let fake = Fake::new(Limits::X4);
        let id = opened(&fake);
        block_call(&fake, id, 0, 0, answer)
    };
    assert!(whole(&[header(4, 0), columns(), cell(), end(1, 0, 9)]).is_ok());
    // Two on a table row, two on a paragraph, one on a paragraph: all invalid.
    assert_eq!(
        whole(&[header(4, 0), columns(), columns(), cell(), end(1, 0, 9)]),
        Err(ERR_INVALID)
    );
    assert_eq!(
        whole(&[header(0, 0), columns(), columns(), end(1, 0, 9)]),
        Err(ERR_INVALID)
    );
    assert_eq!(
        whole(&[header(0, 0), columns(), end(1, 0, 9)]),
        Err(ERR_INVALID)
    );
    // Split: the second comes in a later fragment.
    let fake = Fake::new(Limits::X4);
    let id = opened(&fake);
    block_call(&fake, id, 0, 0, &[header(4, 0), columns(), end(2, 77, 2)]).unwrap();
    assert_eq!(
        block_call(&fake, id, 2, 77, &[columns(), cell(), end(1, 77, 9)]),
        Err(ERR_INVALID)
    );
    assert!(block_call(&fake, id, 2, 77, &[cell(), end(1, 77, 9)]).is_ok());
}
