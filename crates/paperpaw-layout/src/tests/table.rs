//! Tables' rows set in columns with rules, or as paragraphs where they
//! cannot be; the rules of block quotes. A character is one unit wide,
//! a line ten high, an em ten units.

use super::*;
use paperpaw_document::{TableCell, TableRow};

/// A row of the cells `texts`, the second and later set apart by the
/// separator as formats write it, in columns of `weights`.
fn row(texts: &[&str], weights: &[u16], start: u32) -> Block {
    let mut text = String::new();
    let mut cells = Vec::new();
    for (n, cell) in texts.iter().enumerate() {
        if n > 0 {
            text.push_str(" \u{b7} ");
        }
        text.push_str(cell);
        cells.push(TableCell::new(text.len() as u32));
    }
    let end = start + text.len() as u32;
    let mut table = TableRow::new(cells.into_boxed_slice());
    table.columns = weights.into();
    let mut block = block(&text, start, end);
    block.kind = BlockKind::TableRow;
    block.table = Some(alloc::boxed::Box::new(table));
    block
}

/// The page of `blocks` (one after the other), `width` wide and `height`
/// high.
fn laid(blocks: Vec<Block>, width: f32, height: f32) -> Page {
    let doc = Blocks::new(move |at: Locator| {
        blocks
            .iter()
            .find(|b| at.offset < b.end && b.start <= at.offset)
            .cloned()
    });
    let storage = paperpaw_document::NoStorage;
    let cx = LayoutContext::new(&doc, &storage, width, &STYLE, &Units);
    layout_page(&cx, Locator::START, height).unwrap()
}

fn rules(page: &Page) -> Vec<(f32, f32, f32, f32)> {
    page.rules
        .iter()
        .map(|r| (r.x, r.y, r.width, r.height))
        .collect()
}

#[test]
fn a_table_is_set_in_columns_with_rules_around_its_cells() {
    let first = row(&["Name", "Description of it"], &[1, 3], 0);
    let second = row(&["Ab", "cd"], &[1, 3], first.end);
    let page = laid(vec![first, second], 100.0, 100.0);
    assert_eq!(texts(&page), ["Name", "Description of it", "Ab", "cd"]);
    // Columns of 21.25 and 63.75 units, each with 3 on either side and a
    // rule of one between: the text 4 in, the second column's 32.25.
    let placed: Vec<_> = page.lines.iter().map(|l| (l.x, l.baseline)).collect();
    assert_eq!(
        placed,
        [(4.0, 10.5), (32.25, 10.5), (4.0, 24.5), (32.25, 24.5)]
    );
    // A rule above the first row, one below each, and the rules beside
    // the cells as tall as the rows (15 and 14: the second has none above).
    assert_eq!(
        rules(&page),
        [
            (0.0, 0.0, 100.0, 1.0),
            (0.0, 14.0, 100.0, 1.0),
            (0.0, 0.0, 1.0, 15.0),
            (28.25, 0.0, 1.0, 15.0),
            (99.0, 0.0, 1.0, 15.0),
            (0.0, 28.0, 100.0, 1.0),
            (0.0, 15.0, 1.0, 14.0),
            (28.25, 15.0, 1.0, 14.0),
            (99.0, 15.0, 1.0, 14.0),
        ]
    );
    // A cell's text broken in its column: a quarter of 45 units would be
    // narrower than two ems, so the column is 20 wide.
    let narrow = row(&["Description of the thing", "x"], &[1, 3], 0);
    let page_narrow = laid(vec![narrow], 60.0, 100.0);
    assert_eq!(texts(&page_narrow), ["Description of the", "thing", "x"]);
    // A row that does not fit goes to the next page whole.
    let first = row(&["a", "b"], &[1, 1], 0);
    let tall = row(&["aaaaaaaaaa bbbbbbbbbbbb", "g"], &[1, 1], first.end);
    let short = laid(vec![first, tall], 60.0, 30.0);
    assert_eq!(texts(&short), ["a", "b"]);
    assert_eq!(short.end.offset, 6);
}

#[test]
fn a_table_too_wide_for_the_page_is_set_as_its_text() {
    // Columns of two ems at least do not fit 30 units: the row's text as
    // a paragraph, its cells set apart, nothing cut and no rules.
    let wide = row(&["Name", "Value"], &[1, 1], 0);
    let narrow = laid(vec![wide.clone()], 30.0, 100.0);
    assert_eq!(texts(&narrow), ["Name \u{b7} Value"]);
    assert!(narrow.rules.is_empty());
    // In 60 units it is set in columns.
    let set = laid(vec![wide.clone()], 60.0, 100.0);
    assert_eq!(texts(&set), ["Name", "Value"]);
    assert!(!set.rules.is_empty());
    // A row taller than a page is set as its text too, which goes on.
    let tall = laid(vec![wide], 60.0, 12.0);
    assert_eq!(texts(&tall), ["Name \u{b7} Value"]);
    assert!(tall.rules.is_empty());
}

/// A row of empty cells has only its rules on the page: the page has
/// something on it, and a paragraph after it that does not fit below it
/// goes to the next page.
#[test]
fn a_row_of_empty_cells_is_something_on_the_page() {
    let empty = row(&["", ""], &[1, 1], 0);
    let after = block("dd", empty.end, empty.end + 2);
    let page = laid(vec![empty.clone(), after.clone()], 60.0, 100.0);
    assert!(!page.rules.is_empty());
    assert!(!page.is_past_end());
    let below = page.lines[0].baseline;
    // Too low for the paragraph's line: the page ends before it.
    let short = laid(vec![empty.clone(), after], 60.0, below);
    assert!(short.lines.is_empty() && !short.rules.is_empty());
    assert_eq!(short.end.offset, empty.end);
}

/// A position in a row set as its text starts the page with its line, as a
/// paragraph's does; in a row set in columns, with the row.
#[test]
fn a_row_set_as_its_text_is_found_by_its_lines() {
    let wide = row(&["aaaa bbbb cccc", "dddd eeee ffff"], &[1, 1], 0);
    let doc = Blocks::new(move |at: Locator| (at.offset < wide.end).then(|| wide.clone()));
    let storage = paperpaw_document::NoStorage;
    let narrow = LayoutContext::new(&doc, &storage, 30.0, &STYLE, &Units);
    let page = layout_page(&narrow, Locator::START, 100.0).unwrap();
    assert!(page.rules.is_empty() && page.lines.len() > 1);
    let second = Locator::new(0, page.lines[1].words[0].0);
    let inside = Locator::new(0, second.offset + 2);
    assert_eq!(line_start(&narrow, inside).unwrap(), second);
    let columns = LayoutContext::new(&doc, &storage, 100.0, &STYLE, &Units);
    assert_eq!(line_start(&columns, inside).unwrap(), Locator::START);
}

#[test]
fn a_quote_has_a_rule_beside_its_lines() {
    let mut quote = block("aaaa bbbb cccc dddd eeee ffff gggg", 0, 34);
    quote.kind = BlockKind::Extended(decor::QUOTE);
    let page = laid(vec![quote, block("dd", 34, 36)], 40.0, 100.0);
    // Set in by an em, the rule half an em before the lines, as tall as
    // they are; none beside the paragraph after.
    assert_eq!(
        texts(&page),
        ["aaaa bbbb cccc dddd eeee ffff", "gggg", "dd"]
    );
    assert_eq!(page.lines[0].x, 10.0);
    assert_eq!(rules(&page), [(5.0, 0.0, 1.0, 20.0)]);
}
