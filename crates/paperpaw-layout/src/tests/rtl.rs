//! Lines of right-to-left and mixed paragraphs (UAX #9): where they
//! start, how their runs split by level, how wide joined letters are.

use super::*;

/// The page of one block of `text` going `dir`, 10 wide.
fn one(text: &str, dir: Direction, typesetter: &dyn Typesetter) -> Page {
    let len = text.len() as u32;
    let owned = String::from(text);
    let doc =
        Blocks::new(move |at: Locator| (at.offset < len).then(|| block(&owned, 0, len).dir(dir)));
    layout_with(&doc, typesetter, Locator::START, 100.0).unwrap()
}

fn levels(line: &PageLine) -> Vec<(&str, u8)> {
    line.runs
        .iter()
        .map(|r| (&line.text[r.start..r.end], r.level))
        .collect()
}

#[test]
fn right_to_left_lines_start_at_the_right() {
    // Hebrew: from its first strong letter; 5 wide of 10.
    let page = one("\u{5d0}\u{5d1} \u{5d2}\u{5d3}", Direction::Auto, &Flat1);
    let line = &page.lines[0];
    assert!(line.bidi.is_rtl() && line.bidi.has(LineBidi::REORDER));
    assert_eq!(line.x, 5.0);
    assert_eq!(levels(line), [("\u{5d0}\u{5d1} \u{5d2}\u{5d3}", 1)]);
    // Latin in a block the book sets right to left: at the right too, its
    // letters at level 2.
    let page = one("abc", Direction::Rtl, &Flat1);
    let line = &page.lines[0];
    assert_eq!((line.x, levels(line)), (7.0, vec![("abc", 2)]));
    // Left to right alone: as before, nothing to put in order.
    let page = one("abc", Direction::Auto, &Flat1);
    let line = &page.lines[0];
    assert_eq!(line.x, 0.0);
    assert!(!line.bidi.is_rtl() && !line.bidi.has(LineBidi::REORDER));
    assert_eq!(levels(line), [("abc", 0)]);
}

#[test]
fn runs_split_where_the_level_changes() {
    let page = one("\u{5d0} 12 \u{5d1}", Direction::Auto, &Flat1);
    assert_eq!(
        levels(&page.lines[0]),
        [("\u{5d0} ", 1), ("12", 2), (" \u{5d1}", 1)]
    );
    // Hebrew in a Latin paragraph.
    let page = one("ab \u{5d0}\u{5d1} cd", Direction::Auto, &Flat1);
    assert_eq!(
        levels(&page.lines[0]),
        [("ab ", 0), ("\u{5d0}\u{5d1}", 1), (" cd", 0)]
    );
    assert!(page.lines[0].bidi.has(LineBidi::REORDER) && !page.lines[0].bidi.is_rtl());
}

#[test]
fn a_lines_end_takes_the_paragraphs_level() {
    // The space a line breaks at is gone; spaces before Latin at its end
    // in a right-to-left paragraph stay with the paragraph (rule L1).
    let text = "\u{5d0}\u{5d1}\u{5d2} abcd efgh";
    let page = one(text, Direction::Auto, &Flat1);
    let texts: Vec<&str> = page.lines.iter().map(|l| l.text.as_str()).collect();
    assert_eq!(texts, ["\u{5d0}\u{5d1}\u{5d2} abcd", "efgh"]);
    assert_eq!(
        levels(&page.lines[0]),
        [("\u{5d0}\u{5d1}\u{5d2} ", 1), ("abcd", 2)]
    );
    assert_eq!(page.lines[1].x, 6.0);
}

/// 1 per character; letters that join between two others 3, as a shaped
/// font would set them (wider medial forms).
struct Joined;

impl Typesetter for Joined {
    fn advance(&self, _: Face, _: char) -> f32 {
        1.0
    }

    fn kern(&self, _: Face, _: char, _: char) -> f32 {
        -0.5
    }

    fn advance_text(&self, _: Face, text: &str, around: &Around) -> f32 {
        let between = around.before.is_some_and(char::is_alphabetic)
            && around.after.is_some_and(char::is_alphabetic);
        text.chars().count() as f32 * if between { 3.0 } else { 1.0 }
    }

    fn line(&self, _: Face) -> (f32, f32) {
        (8.0, 10.0)
    }
}

#[test]
fn joined_letters_are_as_wide_as_their_forms_and_not_kerned() {
    // بتب: the middle letter between two others; no pair kerned.
    let page = one("\u{628}\u{62a}\u{628}", Direction::Auto, &Joined);
    assert_eq!(page.lines[0].x, 10.0 - 5.0);
    // Latin keeps its kerning: 3 - 2 * 0.5.
    let page = one("abc", Direction::Auto, &Joined);
    assert_eq!(page.lines[0].x, 0.0);
    let page = one("abc", Direction::Rtl, &Joined);
    assert_eq!(page.lines[0].x, 10.0 - 2.0);
}

/// 1 per character, lines 10 high.
struct Flat1;

impl Typesetter for Flat1 {
    fn advance(&self, _: Face, _: char) -> f32 {
        1.0
    }

    fn line(&self, _: Face) -> (f32, f32) {
        (8.0, 10.0)
    }
}

#[test]
fn isolated_text_keeps_its_own_direction_and_its_controls_take_no_room() {
    // As an EPUB sets apart `<span dir="ltr">a!</span>`: the `!` stays
    // with the Latin; without the isolate it would go with the Hebrew.
    let page = one("\u{5d0} \u{2066}a!\u{2069} \u{5d1}", Direction::Rtl, &Flat1);
    let line = &page.lines[0];
    assert_eq!(line.x, 4.0);
    assert_eq!(
        levels(line),
        [("\u{5d0} \u{2066}", 1), ("a!", 2), ("\u{2069} \u{5d1}", 1)]
    );
    let page = one("\u{5d0} a! \u{5d1}", Direction::Rtl, &Flat1);
    assert_eq!(
        levels(&page.lines[0]),
        [("\u{5d0} ", 1), ("a", 2), ("! \u{5d1}", 1)]
    );
}

/// A paragraph in three pieces: an embedding opened in the first stays
/// open in the second, and the neutrals at the first's end take the
/// direction of what starts the second.
const PIECES: [&str; 3] = [
    "\u{5d0} x \u{202b}ab !!",
    "\u{5d1} cd \u{202c}e",
    " f \u{5d2}.",
];

fn pieces_doc() -> Blocks<impl Fn(Locator) -> Option<Block>> {
    Blocks::new(|at: Locator| {
        let mut start = 0u32;
        for (i, text) in PIECES.iter().enumerate() {
            let end = start + text.len() as u32;
            if at.offset < end {
                return Some(block(text, start, end).continues(i > 0).continued(i < 2));
            }
            start = end;
        }
        None
    })
}

/// The levels of the pieces, laid out with `cx` in `order`, by byte of
/// the whole paragraph.
fn piece_levels(cx: &LayoutContext<'_>, order: [usize; 3]) -> Vec<u8> {
    let doc = cx.document;
    let mut by_piece: [Vec<u8>; 3] = Default::default();
    for i in order {
        let start: usize = PIECES[..i].iter().map(|p| p.len()).sum();
        let block = doc
            .block_at(cx.storage, Locator::new(0, start as u32))
            .unwrap()
            .unwrap();
        let levels = BlockLevels::in_context(&block, cx, 0);
        by_piece[i] = (0..block.text.len())
            .map(|b| levels.at(b).number())
            .collect();
    }
    by_piece.concat()
}

#[test]
fn the_pieces_of_a_paragraph_are_resolved_as_the_whole_paragraph() {
    let whole: String = PIECES.concat();
    let mut chars = vec![paperpaw_text::bidi::Level::LTR; whole.chars().count()];
    paperpaw_text::bidi::resolve(&whole, paperpaw_text::bidi::Level::RTL, &mut chars);
    let expected: Vec<u8> = whole
        .chars()
        .zip(&chars)
        .flat_map(|(c, l)| core::iter::repeat_n(l.number(), c.len_utf8()))
        .collect();
    let doc = pieces_doc();
    let storage = paperpaw_document::NoStorage;
    let style = STYLE;
    // Each piece on its own resolves otherwise: the test tells.
    let second = doc
        .block_at(&storage, Locator::new(0, PIECES[0].len() as u32))
        .unwrap()
        .unwrap();
    let alone = BlockLevels::of(&second);
    let from = PIECES[0].len();
    assert_ne!(
        (0..second.text.len())
            .map(|b| alone.at(b).number())
            .collect::<Vec<_>>(),
        expected[from..from + second.text.len()]
    );
    // Without starts kept: each piece walks from the paragraph's start.
    let cx = LayoutContext::new(&doc, &storage, 100.0, &style, &Flat1);
    assert_eq!(piece_levels(&cx, [2, 0, 1]), expected);
    // Kept: the last piece first walks back and keeps both starts; read
    // again, it reads itself alone.
    let kept = PieceStarts::new();
    let cx = cx.pieces(&kept);
    assert_eq!(piece_levels(&cx, [2, 1, 0]), expected);
    assert_eq!(kept.len(), 2);
    let before = doc.calls.get();
    let last = PIECES[0].len() + PIECES[1].len();
    assert_eq!(piece_levels(&cx, [2, 2, 2]), expected[last..]);
    assert_eq!(
        doc.calls.get() - before,
        3,
        "three times the piece alone: the last is continued by none"
    );
    kept.clear();
    assert!(kept.is_empty());
}

/// A paragraph of 70 pieces of `dir` auto: Hebrew first, so right to
/// left; the others start with Latin, Hebrew in them but in `latin_only`.
fn long_doc(latin_only: usize) -> Blocks<impl Fn(Locator) -> Option<Block>> {
    Blocks::new(move |at: Locator| {
        let piece = (at.offset / 20) as usize;
        if piece >= 70 {
            return None;
        }
        let text = match piece {
            0 => "\u{5d0}\u{5d1} abc def gh ",
            _ if piece == latin_only => "abcd efg hijk lmno ",
            _ => "abcd \u{5d2}\u{5d3} efgh ij ",
        };
        // Each 20 bytes, so the pieces follow one another.
        let mut text = String::from(text);
        while text.len() < 20 {
            text.push(' ');
        }
        let start = piece as u32 * 20;
        Some(
            block(&text, start, start + 20)
                .continues(piece > 0)
                .continued(piece < 69),
        )
    })
}

#[test]
fn a_jump_into_a_paragraph_of_70_pieces_keeps_its_direction() {
    let storage = paperpaw_document::NoStorage;
    // Piece 66 starts with Latin; the walk back gives up at 64 pieces.
    let doc = long_doc(usize::MAX);
    let cold = PieceStarts::new();
    let cx = LayoutContext::new(&doc, &storage, 100.0, &STYLE, &Flat1).pieces(&cold);
    let page = layout_page(&cx, Locator::new(0, 66 * 20), 10.0).unwrap();
    assert!(
        page.lines[0].bidi.is_rtl(),
        "the paragraph runs right to left"
    );
    // Read forward, the same.
    let kept = PieceStarts::new();
    let cx = LayoutContext::new(&doc, &storage, 100.0, &STYLE, &Flat1).pieces(&kept);
    let mut at = Locator::START;
    while at.offset < 66 * 20 {
        at = layout_page(&cx, at, 100.0).unwrap().end;
    }
    let page = layout_page(&cx, Locator::new(0, 66 * 20), 10.0).unwrap();
    assert!(page.lines[0].bidi.is_rtl());
    // A piece of Latin alone in it: right to left once a state before it
    // is kept, as it is read forward.
    let doc = long_doc(30);
    let kept = PieceStarts::new();
    let cx = LayoutContext::new(&doc, &storage, 100.0, &STYLE, &Flat1).pieces(&kept);
    let mut at = Locator::START;
    while at.offset < 30 * 20 {
        at = layout_page(&cx, at, 100.0).unwrap().end;
    }
    let page = layout_page(&cx, Locator::new(0, 30 * 20), 10.0).unwrap();
    assert!(page.lines[0].bidi.is_rtl());
}
