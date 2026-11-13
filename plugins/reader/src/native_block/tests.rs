use super::*;
use crate::fail_alloc::sweep;
use paperpaw_sdk::document::TextStyle;

fn head(text_bytes: u32) -> Head<'static> {
    Head {
        kind: BlockKind::Paragraph,
        start: 10,
        end: 10 + text_bytes,
        continues: false,
        continued: false,
        align: Align::Auto,
        indent: Some(150),
        space: None,
        size: Some(120),
        line_height: None,
        margins: (5, 70_000),
        direction: Direction::Rtl,
        writing_mode: WritingMode::HorizontalTb,
        lang: "fr",
        drop_cap: 3,
        text_bytes,
        row: (false, false, false),
    }
}

fn join(fragments: &[&[Piece<'static>]]) -> Result<Block, Error> {
    let mut assembler = Assembler::new();
    let mut done = None;
    for (n, pieces) in fragments.iter().enumerate() {
        done = assembler.push(pieces.iter().cloned(), n + 1 == fragments.len())?;
    }
    done.ok_or(Error::Invalid)
}

#[test]
fn fragments_join_into_one_block() {
    let block = join(&[
        &[
            Piece::Head(head(12)),
            Piece::Text("Hello "),
            Piece::Run(0, 5, TextStyle::REGULAR),
        ],
        &[
            Piece::Text("wörld"),
            Piece::Map(0, 10),
            Piece::Map(6, 20),
            Piece::Lang("de"),
            Piece::Ruby(6, 12, AnnotationKind::Gloss, 50, "earth"),
        ],
    ])
    .unwrap();
    assert_eq!(block.text, "Hello wörld");
    assert_eq!((block.start, block.end), (10, 22));
    assert_eq!(block.indent, Some(150));
    assert_eq!(block.size, Some(120));
    assert_eq!(block.margins, (5, u16::MAX));
    assert_eq!(block.drop_cap, 3);
    assert_eq!(block.dir, Direction::Rtl);
    assert_eq!(block.runs.len(), 1);
    assert_eq!(block.annotations.len(), 1);
    assert_eq!(block.annotations[0].text, "earth");
    assert_eq!(block.annotations[0].size, 50);
    assert_eq!(block.lang, Lang::parse("fr"));
    assert_eq!(block.lang_of(TextStyle::REGULAR), Lang::parse("fr"));
    assert_eq!(block.source_offset(6), 20);
}

#[test]
fn a_picture_or_a_table_row_without_its_records_reads_as_a_paragraph() {
    for kind in [BlockKind::Image, BlockKind::TableRow] {
        let mut h = head(2);
        h.kind = kind;
        let block = join(&[&[Piece::Head(h), Piece::Text("ab")]]).unwrap();
        assert_eq!(block.kind, BlockKind::Paragraph);
        assert!(block.image.is_none() && block.table.is_none());
    }
    let mut h = head(1);
    h.kind = BlockKind::Heading(2);
    let block = join(&[&[Piece::Head(h), Piece::Text("a")]]).unwrap();
    assert_eq!(block.kind, BlockKind::Heading(2));
}

#[test]
fn a_picture_block_carries_the_hosts_handle_and_size() {
    let mut h = head(0);
    h.kind = BlockKind::Image;
    let block = join(&[&[Piece::Head(h), Piece::Image(12, 3, 640, 480)]]).unwrap();
    assert_eq!(block.kind, BlockKind::Image);
    let image = block.image.unwrap();
    assert_eq!(
        (image.path.as_str(), image.width, image.height),
        ("12:3", 640, 480)
    );
}

#[test]
fn a_table_row_carries_its_cells_and_weights_and_refuses_unsound_ones() {
    let cell = |end| Piece::Cell(end, 1, 1, Align::Start, VerticalAlign::Top);
    let mut h = head(4);
    h.kind = BlockKind::TableRow;
    h.row = (true, true, false);
    let pieces = [
        Piece::Head(h.clone()),
        Piece::Text("abcd"),
        Piece::Columns(alloc::vec![1, 3]),
        cell(2),
        cell(4),
    ];
    let block = join(&[&pieces]).unwrap();
    assert_eq!(block.kind, BlockKind::TableRow);
    let row = block.table.unwrap();
    assert_eq!(row.cells.len(), 2);
    assert_eq!(&*row.columns, &[1, 3]);
    assert!(row.header && row.first && !row.last);
    // A cell going back, or past the text, makes it a paragraph.
    for ends in [[3, 2], [2, 9]] {
        let pieces = [
            Piece::Head(h.clone()),
            Piece::Text("abcd"),
            cell(ends[0]),
            cell(ends[1]),
        ];
        let block = join(&[&pieces]).unwrap();
        assert_eq!(block.kind, BlockKind::Paragraph);
        assert!(block.table.is_none());
    }
    // More cells than a row may have are refused.
    let mut many = alloc::vec![Piece::Head(h), Piece::Text("abcd")];
    many.extend((0..17).map(|_| cell(4)));
    assert_eq!(join(&[&many]), Err(Error::NoSpace));
}

#[test]
fn what_does_not_add_up_is_refused() {
    // Text short of the head's length, a piece before the head, two heads.
    assert_eq!(
        join(&[&[Piece::Head(head(5)), Piece::Text("ab")]]),
        Err(Error::Invalid)
    );
    assert_eq!(join(&[&[Piece::Text("ab")]]), Err(Error::Invalid));
    assert_eq!(
        join(&[&[Piece::Head(head(0)), Piece::Head(head(0))]]),
        Err(Error::Invalid)
    );
    // Runs: overlapping, past the text, inside a character, backwards.
    for runs in [
        [
            Piece::Run(0, 3, TextStyle::REGULAR),
            Piece::Run(2, 4, TextStyle::REGULAR),
        ],
        [
            Piece::Run(0, 9, TextStyle::REGULAR),
            Piece::Run(9, 9, TextStyle::REGULAR),
        ],
        [
            Piece::Run(0, 2, TextStyle::REGULAR),
            Piece::Run(2, 1, TextStyle::REGULAR),
        ],
    ] {
        let [a, b] = runs;
        let result = join(&[&[Piece::Head(head(4)), Piece::Text("abcd"), a, b]]);
        assert_eq!(result, Err(Error::Invalid));
    }
    let inside = join(&[&[
        Piece::Head(head(2)),
        Piece::Text("é"),
        Piece::Run(0, 1, TextStyle::REGULAR),
    ]]);
    assert_eq!(inside, Err(Error::Invalid));
    // Checkpoints out of order; an annotation past the text.
    let maps = join(&[&[
        Piece::Head(head(1)),
        Piece::Text("a"),
        Piece::Map(5, 1),
        Piece::Map(2, 1),
    ]]);
    assert_eq!(maps, Err(Error::Invalid));
    let note = join(&[&[
        Piece::Head(head(1)),
        Piece::Text("a"),
        Piece::Ruby(0, 4, AnnotationKind::Ruby, 0, "x"),
    ]]);
    assert_eq!(note, Err(Error::Invalid));
}

#[test]
fn a_block_over_the_limits_is_refused_and_the_next_starts_clean() {
    let mut assembler = Assembler::new();
    let big = u32::try_from(MAX_TEXT).unwrap() + 1;
    assert_eq!(
        assembler.push([Piece::Head(head(big))], false),
        Err(Error::NoSpace)
    );
    let ok = assembler
        .push([Piece::Head(head(1)), Piece::Text("a")], true)
        .unwrap();
    assert_eq!(ok.unwrap().text, "a");
    let langs = (0..=MAX_LANGS).map(|_| Piece::Lang("en"));
    let mut assembler = Assembler::new();
    assert_eq!(
        assembler.push(core::iter::once(Piece::Head(head(0))).chain(langs), true),
        Err(Error::NoSpace)
    );
    // After a refusal nothing of the block is kept.
    assert_eq!(
        assembler.push([Piece::Text("a")], true),
        Err(Error::Invalid)
    );
}

#[test]
fn every_allocation_may_be_refused() {
    sweep(|| {
        let mut assembler = Assembler::new();
        let pieces = [
            Piece::Head(head(12)),
            Piece::Text("Hello wörld"),
            Piece::Run(0, 5, TextStyle::REGULAR),
            Piece::Map(0, 10),
            Piece::Lang("de"),
            Piece::Ruby(6, 12, AnnotationKind::Gloss, 50, "earth"),
        ];
        assembler.push(pieces, true)?;
        Ok(())
    });
}

#[test]
fn a_part_past_the_heads_length_is_refused_at_once() {
    let mut assembler = Assembler::new();
    assert_eq!(
        assembler.push([Piece::Head(head(2)), Piece::Text("abc")], false),
        Err(Error::Invalid)
    );
    // The block was forgotten.
    assert_eq!(
        assembler.push([Piece::Text("a")], true),
        Err(Error::Invalid)
    );
}
