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
