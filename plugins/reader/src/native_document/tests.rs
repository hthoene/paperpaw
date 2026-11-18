use super::*;
use crate::native_block::{Head, Piece};
use paperpaw_sdk::document::{Align, BlockKind};
use paperpaw_sdk::lang::{Direction, WritingMode};

fn head() -> Head<'static> {
    Head {
        kind: BlockKind::Paragraph,
        start: 0,
        end: 1,
        continues: false,
        continued: false,
        align: Align::Auto,
        indent: None,
        space: None,
        size: None,
        line_height: None,
        margins: (0, 0),
        direction: Direction::Auto,
        writing_mode: WritingMode::HorizontalTb,
        lang: "",
        drop_cap: 0,
        text_bytes: 2,
        row: (false, false, false),
    }
}

#[test]
fn fragments_are_asked_for_until_the_block_is_whole() {
    let mut asked = 0;
    let block = read_block(|_, assembler| {
        asked += 1;
        if asked == 1 {
            assembler.push([Piece::Head(head()), Piece::Text("a")], false)?;
            Ok(Next::More(BlockCursor::start()))
        } else {
            let block = assembler.push([Piece::Text("b")], true)?;
            Ok(Next::Done(block.unwrap()))
        }
    })
    .unwrap();
    assert_eq!((asked, block.text.as_str()), (2, "ab"));
}

#[test]
fn a_block_that_changed_is_read_again_a_few_times() {
    let mut asked = 0;
    let block = read_block(|_, assembler| {
        asked += 1;
        if asked <= MAX_RESTARTS {
            assembler.push([Piece::Head(head()), Piece::Text("a")], false)?;
            return Err(Error::State);
        }
        let block = assembler.push([Piece::Head(head()), Piece::Text("ab")], true)?;
        Ok(Next::Done(block.unwrap()))
    })
    .unwrap();
    assert_eq!((asked, block.text.as_str()), (MAX_RESTARTS + 1, "ab"));
    // One that never settles is refused.
    let result = read_block(|_, _| Err(Error::State));
    assert_eq!(result.unwrap_err(), Error::NoSpace);
}

#[test]
fn a_block_of_too_many_fragments_is_refused() {
    let mut asked = 0;
    let result = read_block(|_, _| {
        asked += 1;
        Ok(Next::More(BlockCursor::start()))
    });
    assert_eq!(result.unwrap_err(), Error::NoSpace);
    assert_eq!(asked, MAX_FRAGMENTS);
}

#[test]
fn other_refusals_end_the_read_and_name_what_the_layout_is_told() {
    let result = read_block(|_, _| Err(Error::NotFound));
    assert_eq!(result.unwrap_err(), Error::NotFound);
    assert_eq!(document_error(Error::NoMemory), DocumentError::OutOfMemory);
    assert_eq!(document_error(Error::Busy), DocumentError::NotReady);
    assert_eq!(
        document_error(Error::Unsupported),
        DocumentError::UnsupportedFormat
    );
    assert_eq!(document_error(Error::Denied), DocumentError::Other(0));
}

#[test]
fn a_section_end_is_only_the_answer_to_a_first_ask() {
    // First ask: the section has no further block.
    let first = read_block(|_, _| Err(Error::NotFound));
    assert!(matches!(block_result(first), Ok(None)));
    // After a fragment, or after a restart, the same answer is an error.
    let mut asked = 0;
    let later = read_block(|_, assembler| {
        asked += 1;
        if asked == 1 {
            assembler.push([Piece::Head(head()), Piece::Text("a")], false)?;
            return Ok(Next::More(BlockCursor::start()));
        }
        Err(Error::NotFound)
    });
    assert_eq!(later.as_ref().unwrap_err(), &Error::Invalid);
    assert!(matches!(
        block_result(later),
        Err(DocumentError::Malformed(_))
    ));
    let mut asked = 0;
    let restarted = read_block(|_, _| {
        asked += 1;
        if asked == 1 {
            Err(Error::State)
        } else {
            Err(Error::NotFound)
        }
    });
    assert_eq!(restarted.unwrap_err(), Error::Invalid);
    assert!(matches!(
        block_result(Err(Error::NoSpace)),
        Err(DocumentError::Malformed(_))
    ));
    assert!(matches!(
        block_result(Err(Error::NoMemory)),
        Err(DocumentError::OutOfMemory)
    ));
}

/// What the reading aids answer with their pictures set to be left out.
fn hiding() -> crate::native_filter::Filter {
    use paperpaw_plugin_aids::{pictures, reader_filter as aids};
    let mut bytes = alloc::vec![0; crate::stages::reader_filter_v1::MAX_REPLY];
    let len = aids::encode(false, "", pictures::HIDE, "Picture", &mut bytes).unwrap();
    crate::native_filter::Filter::from_reply(&bytes[..len]).unwrap()
}

#[test]
fn a_picture_the_rules_leave_out_gives_the_next_block_kept() {
    // #257 (x4-pictures-hidden): the native reader drew the picture anyway.
    let blocks = [
        Block::new(BlockKind::Paragraph, 0, 10, "before".into()),
        Block::new(BlockKind::Image, 10, 20, "ship".into()),
        Block::new(BlockKind::Paragraph, 20, 30, "after".into()),
    ];
    let read = |at: Locator| {
        Ok(blocks
            .iter()
            .find(|b| b.start <= at.offset && at.offset < b.end)
            .cloned())
    };
    let filters = [hiding()];
    let path = "books/pictures.epub";
    let got = filtered_block(read, (&filters, path), Locator::new(0, 10)).unwrap();
    assert_eq!(got.map(|b| b.text), Some("after".into()));
    // Without rules the picture is the block there.
    let got = filtered_block(read, (&[], path), Locator::new(0, 10)).unwrap();
    assert_eq!(got.map(|b| b.kind), Some(BlockKind::Image));
    // In a book whose pictures are its pages, nothing is left out.
    let got = filtered_block(read, (&filters, "a.cbz"), Locator::new(0, 10)).unwrap();
    assert_eq!(got.map(|b| b.kind), Some(BlockKind::Image));
}
