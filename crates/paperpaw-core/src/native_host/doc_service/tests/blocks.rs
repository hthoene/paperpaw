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
    // A changed generation is the format's ERR_STATE and the host's too.
    assert_eq!(fresh(2, 77, &[part("x"), end(1, 78, 99)]), Err(ERR_STATE));
    // A later fragment never carries a header; the first always does.
    assert_eq!(
        fresh(2, 77, &[header(0, 1), part("x"), end(1, 77, 9)]),
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
    assert_eq!(fresh(2, 77, &[end(1, 77, 9)]), Err(ERR_INVALID));
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
    assert_eq!(
        bad(&[header(3, 0), end(1, 0, 9)]),
        Err(ERR_INVALID),
        "no picture"
    );
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
}
