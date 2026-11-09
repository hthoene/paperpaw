use super::{BlockReply, Error, FormatCall, schema};
use crate::document::{
    Align, Annotation, AnnotationKind, Block, BlockKind, ImageRef, OffsetMap, Run, TableCell,
    TableRow, TextStyle, VerticalAlign,
};
use alloc::{boxed::Box, string::String, vec::Vec};
use paperpaw_lang::{Direction, Lang, WritingMode};

fn call<'a>(op: u32, cursor: u32, generation: u32, input: &'a mut [u8]) -> FormatCall<'a> {
    let n = schema::FmtCall::new(op, 9, 3, 0, 0, cursor, generation, 0, 600, 300, 77, "", &[])
        .encode(input)
        .unwrap();
    FormatCall::decode(&input[..n]).unwrap()
}
/// One tag per record, plus what a test inspects.
#[derive(Debug, PartialEq)]
enum Seen {
    Block(u32, u32, u32, String),
    Part(String),
    Run(u32, u32, u32),
    Map(u32, u32),
    Image(String, u32),
    Columns(Vec<u8>),
    Cell(u32, u32, u32, u32, u32, u32),
    Ruby(u32, u32, u32, u32, String),
    Lang(String),
    End(u32, u32, u32, u32),
}
fn seen(bytes: &[u8]) -> Vec<Seen> {
    use schema::Record as R;
    schema::reply_decode(bytes)
        .unwrap()
        .map(|r| match r.unwrap() {
            R::FmtBlock(b) => Seen::Block(b.kind, b.level, b.flags, b.lang.into()),
            R::FmtTextPart(t) => Seen::Part(t.text.into()),
            R::FmtRun(r) => Seen::Run(r.start, r.end, r.style),
            R::FmtMap(m) => Seen::Map(m.text, m.source),
            R::FmtImage(i) => Seen::Image(i.path.into(), i.index),
            R::FmtColumns(c) => Seen::Columns(c.weights.to_vec()),
            R::FmtCell(c) => Seen::Cell(c.end, c.span, c.rows, c.align, c.vertical, c.width),
            R::FmtRuby(r) => Seen::Ruby(r.start, r.end, r.kind, r.size, r.text.into()),
            R::FmtInlineLang(l) => Seen::Lang(l.lang.into()),
            R::FmtEnd(e) => Seen::End(e.flags, e.total, e.generation, e.next),
            other => panic!("unexpected {other:?}"),
        })
        .collect()
}
/// Every fragment of a block asked for in turn through `size`-byte outputs, joined.
fn fragments(reply: &BlockReply<'_>, size: usize) -> Vec<Seen> {
    let mut all = Vec::new();
    let (mut cursor, mut generation) = (0, 0);
    loop {
        let mut input = [0; 256];
        let mut out = alloc::vec![0xa5; size];
        let n = call(8, cursor, generation, &mut input)
            .reply_block(reply, &mut out)
            .unwrap();
        let mut got = seen(&out[..n]);
        let Some(Seen::End(flags, _, gen_, next)) = got.pop() else {
            panic!("no end")
        };
        all.append(&mut got);
        if flags & 2 == 0 {
            all.push(Seen::End(flags, 0, 0, next));
            return all;
        }
        (cursor, generation) = (next, gen_);
    }
}
fn paragraph(text: &str) -> Block {
    Block::new(
        BlockKind::Paragraph,
        100,
        100 + text.len() as u32,
        text.into(),
    )
}

#[test]
fn a_paragraph_is_a_record_list_in_the_documented_order() {
    let mut map = OffsetMap::IDENTITY;
    map.checkpoint(0, 100);
    map.checkpoint(4, 130);
    let mut block = paragraph("héllo wörld")
        .runs(alloc::vec![Run::new(0, 5, TextStyle::default())])
        .map(map)
        .lang(Lang::parse("fa-IR"))
        .inline_langs(&[Lang::parse("ar"), Lang::parse("en")])
        .dir(Direction::Rtl)
        .writing_mode(WritingMode::VerticalRl)
        .align(Align::Justify)
        .indent(-30)
        .margins(4, 8)
        .continues(true);
    block
        .annotations
        .push(Annotation::new(0, 5, "yomi", AnnotationKind::Phonetic));
    let reply = BlockReply::new(&block).last_in_section().next(777);
    let got = fragments(&reply, 3840);
    assert!(
        matches!(&got[0], Seen::Block(0, 0, 1, lang) if lang == "fa-IR"),
        "{:?}",
        got[0]
    );
    assert_eq!(got[1], Seen::Part("héllo wörld".into()));
    assert_eq!(got[2], Seen::Run(0, 5, 0));
    assert_eq!((&got[3], &got[4]), (&Seen::Map(0, 100), &Seen::Map(4, 130)));
    assert_eq!(got[5], Seen::Ruby(0, 5, 1, 0, "yomi".into()));
    assert_eq!(
        (&got[6], &got[7]),
        (&Seen::Lang("ar".into()), &Seen::Lang("en".into()))
    );
    assert_eq!(got[8], Seen::End(1, 0, 0, 777));
    assert_eq!(got.len(), 9);
}

#[test]
fn a_block_header_says_the_readers_for_what_the_book_leaves_unset() {
    let block = paragraph("x");
    let mut out = [0; 3840];
    let mut input = [0; 256];
    let n = call(8, 0, 0, &mut input)
        .reply_block(&BlockReply::new(&block), &mut out)
        .unwrap();
    let schema::Record::FmtBlock(b) = schema::reply_decode(&out[..n])
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
    else {
        panic!("no block")
    };
    assert_eq!(
        (
            b.indent,
            b.space,
            b.size,
            b.line_height,
            b.dir,
            b.writing_mode,
            b.text_bytes
        ),
        (i32::MIN, u32::MAX, u32::MAX, u32::MAX, 0, 0, 1)
    );
    assert_eq!(b.lang, "");
}

#[test]
fn a_long_text_travels_in_parts_and_fragments_that_join_to_the_same_list() {
    let text: String = "界é".repeat(2500); // 12,500 bytes
    let block = paragraph(&text);
    let whole = fragments(&BlockReply::new(&block), 3840);
    let joined: String = whole
        .iter()
        .filter_map(|r| {
            if let Seen::Part(t) = r {
                Some(t.as_str())
            } else {
                None
            }
        })
        .collect();
    assert_eq!(joined, text);
    assert!(
        whole
            .iter()
            .all(|r| !matches!(r, Seen::Part(t) if t.len() > 3072))
    );
    // The list does not depend on how the fragments are cut.
    assert_eq!(fragments(&BlockReply::new(&block), 3200), whole);
    assert_eq!(fragments(&BlockReply::new(&block), 3300), whole);
}

#[test]
fn a_continued_fragment_needs_this_blocks_generation_and_a_cursor_inside_the_list() {
    let block = paragraph(&"a".repeat(9000));
    let mut input = [0; 256];
    let mut out = [0; 3840];
    let n = call(8, 0, 0, &mut input)
        .reply_block(&BlockReply::new(&block), &mut out)
        .unwrap();
    let Some(Seen::End(2, _, generation, next)) = seen(&out[..n]).pop() else {
        panic!("a 9,000 byte block fits one fragment")
    };
    assert!(generation != 0 && next > 0);
    let mut out2 = [0; 3840];
    assert!(
        call(8, next, generation, &mut input)
            .reply_block(&BlockReply::new(&block), &mut out2)
            .is_ok()
    );
    assert_eq!(
        call(8, next, generation ^ 1, &mut input).reply_block(&BlockReply::new(&block), &mut out2),
        Err(Error::State)
    );
    // Another block (another start) has another identity.
    let other = Block::new(BlockKind::Paragraph, 5, 9005, "a".repeat(9000));
    assert_eq!(
        call(8, next, generation, &mut input).reply_block(&BlockReply::new(&other), &mut out2),
        Err(Error::State)
    );
    assert_eq!(
        call(8, 500, generation, &mut input).reply_block(&BlockReply::new(&block), &mut out2),
        Err(Error::Invalid)
    );
}

#[test]
fn small_outputs_and_other_operations_are_refused_before_anything_is_written() {
    let block = paragraph("x");
    let mut input = [0; 256];
    let mut out = [0xa5; 16];
    assert_eq!(
        call(8, 0, 0, &mut input).reply_block(&BlockReply::new(&block), &mut out),
        Err(Error::NoSpace)
    );
    let mut out = [0xa5; 3840];
    assert_eq!(
        call(9, 0, 0, &mut input).reply_block(&BlockReply::new(&block), &mut out),
        Err(Error::State)
    );
    // The header alone does not fit: nothing useful can be written.
    let mut out = [0xa5; 60];
    assert_eq!(
        call(8, 0, 0, &mut input).reply_block(&BlockReply::new(&block), &mut out),
        Err(Error::NoSpace)
    );
}

#[test]
fn a_table_row_carries_its_columns_cells_and_place() {
    let mut block = Block::new(BlockKind::TableRow, 0, 9, "abcdefghi".into());
    let mut row = TableRow::new(Box::new([
        TableCell::new(3).span(2).align(Align::Center),
        TableCell::new(9).rows(2).vertical(VerticalAlign::Bottom),
    ]));
    row.columns = Box::new([1, 2, 3]);
    row.header = true;
    row.last = true;
    block.table = Some(Box::new(row));
    let got = fragments(&BlockReply::new(&block), 3840);
    assert!(matches!(got[0], Seen::Block(4, 0, flags, _) if flags == (1 << 8) | (1 << 10)));
    assert_eq!(got[2], Seen::Columns(alloc::vec![1, 0, 2, 0, 3, 0]));
    assert_eq!(got[3], Seen::Cell(3, 2, 1, 2, 0, 3));
    assert_eq!(got[4], Seen::Cell(9, 1, 2, 1, 2, 3));
}

#[test]
fn an_image_block_names_the_cache_file_and_the_picture_index() {
    let block = Block::new(BlockKind::Image, 0, 1, "alt".into()).image(ImageRef {
        path: "images/3.pgm".into(),
        width: 200,
        height: 100,
    });
    let got = fragments(&BlockReply::new(&block).image_index(3), 3840);
    assert_eq!(got[1], Seen::Part("alt".into()));
    assert_eq!(got[2], Seen::Image("images/3.pgm".into(), 3));
}

#[test]
fn a_block_that_is_not_last_ends_with_no_last_flag() {
    let block = paragraph("x");
    let got = fragments(&BlockReply::new(&block).next(5), 3840);
    assert_eq!(got.last(), Some(&Seen::End(0, 0, 0, 5)));
}
