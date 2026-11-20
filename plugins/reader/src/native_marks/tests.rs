use super::*;
use crate::native_word::Word;
use paperpaw_sdk::document::Locator;

fn word(text: &str, x: u16, y: u16, start: u32, end: u32) -> Word {
    Word::new(
        Rect::new(x, y, 20, 10),
        text,
        Locator::new(0, start),
        Locator::new(0, end),
    )
}

fn span(start: u32, end: u32) -> Span {
    Span {
        start: (0, start),
        end: (0, end),
        noted: false,
    }
}

#[derive(Default)]
struct Log(Vec<(&'static str, i32, i32, u32, u32, u32)>);

impl Pen for Log {
    fn fill(&mut self, x: i32, y: i32, w: u32, h: u32, _: Color) -> Result<(), Error> {
        self.0.push(("fill", x, y, w, h, 0));
        Ok(())
    }
    fn stroke(&mut self, r: (i32, i32, u32, u32), t: u32, _: Color) -> Result<(), Error> {
        self.0.push(("stroke", r.0, r.1, r.2, r.3, t));
        Ok(())
    }
}

const GAUGE: Gauge = Gauge {
    dpi: 254,
    few_grays: false,
};
const FEW: Gauge = Gauge {
    dpi: 254,
    few_grays: true,
};

#[test]
fn a_span_covers_the_words_it_touches() {
    let words = [
        word("a", 0, 0, 0, 5),
        word("b", 30, 0, 6, 11),
        word("c", 60, 0, 12, 17),
    ];
    let s = span(5, 12);
    // `a` ends where the span starts, `c` starts where it ends.
    let covered: Vec<bool> = words.iter().map(|w| s.covers(w)).collect();
    assert_eq!(covered, [false, true, false]);
    let across = Span {
        start: (0, 3),
        end: (1, 2),
        noted: false,
    };
    assert!(words.iter().all(|w| across.covers(w) || w.end.offset <= 3));
}

#[test]
fn a_span_of_another_section_covers_nothing() {
    let w = word("a", 0, 0, 0, 5);
    let other = Span {
        start: (1, 0),
        end: (1, 50),
        noted: false,
    };
    assert!(!other.covers(&w));
}

#[test]
fn the_boxes_are_one_for_each_line_of_covered_words() {
    let words = [
        word("a", 0, 0, 0, 5),
        word("b", 30, 0, 6, 11),
        word("c", 60, 0, 12, 17),
        word("d", 0, 20, 18, 23),
    ];
    let boxes = boxes(&words, &[span(6, 20)]);
    assert_eq!(boxes.len(), 2);
    assert_eq!((boxes[0].x, boxes[0].y, boxes[0].width), (30, 0, 50));
    assert_eq!((boxes[1].x, boxes[1].y), (0, 20));
    assert!(super::boxes(&words, &[]).is_empty());
}

#[test]
fn the_hit_goes_with_its_page() {
    let mut marks = Marks::default();
    marks.found((0, 100), span(110, 115));
    assert_eq!(marks.hit_on((0, 100)), Some(span(110, 115)));
    assert_eq!(marks.hit_on((0, 100)), Some(span(110, 115)));
    // The next page drops it for good.
    assert_eq!(marks.hit_on((0, 200)), None);
    assert_eq!(marks.hit_on((0, 100)), None);
}

#[test]
fn a_page_has_marks_only_where_a_span_reaches_it() {
    let words = [word("a", 0, 0, 100, 105), word("b", 30, 0, 106, 111)];
    let marks = Marks::new(Vec::from([span(0, 50), span(500, 600)]));
    assert!(!marks.on_page(&words, None));
    assert!(marks.on_page(&words, Some(span(104, 108))));
    let marks = Marks::new(Vec::from([span(90, 101)]));
    assert!(marks.on_page(&words, None));
    assert!(!marks.on_page(&[], Some(span(0, 1000))));
}

#[test]
fn a_background_is_behind_each_box_a_little_wider_and_not_off_the_screen() {
    let deco = Decoration::NONE.background(Tone::LightGray);
    let boxes = [Rect::new(1, 10, 40, 12), Rect::new(100, 30, 20, 12)];
    let mut pen = Log::default();
    draw_behind(&mut pen, GAUGE, &boxes, &deco).unwrap();
    // 254 dpi: half a millimetre is five pixels.
    assert_eq!(
        pen.0,
        [("fill", 0, 10, 46, 12, 0), ("fill", 95, 30, 30, 12, 0)]
    );
    let mut none = Log::default();
    draw_behind(&mut none, GAUGE, &boxes, &Decoration::NONE).unwrap();
    assert!(none.0.is_empty());
}

#[test]
fn lines_frames_and_strikes_are_drawn_over() {
    let boxes = [Rect::new(10, 10, 40, 20)];
    let mut pen = Log::default();
    let solid = Decoration::NONE.line(LineKind::Solid).thickness(10);
    draw_over(&mut pen, GAUGE, &boxes, &solid).unwrap();
    // 10 tenths of a millimetre at 254 dpi: ten pixels, its bottom at the box's.
    assert_eq!(pen.0, [("fill", 10, 20, 40, 10, 0)]);
    let mut pen = Log::default();
    let all = Decoration::NONE
        .line(LineKind::Double)
        .frame()
        .struck()
        .thickness(10);
    draw_over(&mut pen, GAUGE, &boxes, &all).unwrap();
    assert_eq!(pen.0.iter().filter(|op| op.0 == "stroke").count(), 1);
    assert_eq!(pen.0.iter().filter(|op| op.0 == "fill").count(), 3);
    let mut none = Log::default();
    draw_over(&mut none, GAUGE, &boxes, &Decoration::NONE).unwrap();
    assert!(none.0.is_empty());
}

#[test]
fn dotted_and_wavy_lines_are_made_of_pieces_inside_the_box() {
    let boxes = [Rect::new(0, 0, 100, 20)];
    for kind in [LineKind::Dotted, LineKind::Dashed, LineKind::Wavy] {
        let mut pen = Log::default();
        let deco = Decoration::NONE.line(kind).thickness(4);
        draw_over(&mut pen, GAUGE, &boxes, &deco).unwrap();
        assert!(pen.0.len() > 1 && pen.0.len() < 100, "{kind:?}");
        assert!(
            pen.0
                .iter()
                .all(|op| op.1 >= 0 && op.1 + op.3 as i32 <= 100)
        );
    }
}

#[test]
fn the_names_of_a_roles_settings() {
    let mut buffer = [0u8; 48];
    assert_eq!(
        join(&mut buffer, HIGHLIGHT, "line-tone").unwrap(),
        "core.highlight.line-tone"
    );
    let mut small = [0u8; 8];
    assert_eq!(join(&mut small, HIGHLIGHT, "line"), Err(Error::NoSpace));
}

#[test]
fn stretches_overlap_where_they_share_a_place() {
    use super::edit::overlaps;
    assert!(overlaps(&span(0, 10), &span(5, 15)));
    assert!(overlaps(&span(5, 15), &span(0, 10)));
    assert!(overlaps(&span(0, 10), &span(2, 3)));
    // Touching is not sharing; a stretch of no length shares nothing.
    assert!(!overlaps(&span(0, 10), &span(10, 20)));
    assert!(!overlaps(&span(4, 4), &span(0, 10)));
}

#[test]
fn a_text_is_one_line_cut_at_a_character() {
    let long = "é".repeat(400);
    let cut = super::edit::snippet(&long).unwrap();
    assert_eq!(cut.len(), 512);
    assert_eq!(super::edit::snippet("a\n b\tc").unwrap(), "a  b c");
}

#[test]
fn the_gauge_of_a_display_is_few_grays_on_black_and_white_and_four_not_on_colour() {
    assert!(Gauge::of(300, Some(2), false).few_grays);
    assert!(Gauge::of(300, Some(4), false).few_grays);
    assert!(!Gauge::of(300, Some(16), false).few_grays);
    assert!(!Gauge::of(300, None, false).few_grays);
    assert!(!Gauge::of(300, Some(2), true).few_grays);
}

#[test]
fn on_few_grays_a_background_is_rows_by_the_tones_darkness() {
    let boxes = [Rect::new(5, 10, 40, 12)];
    // A light gray: one row in four, counted from the top of the screen.
    let mut pen = Log::default();
    draw_behind(
        &mut pen,
        FEW,
        &boxes,
        &Decoration::NONE.background(Tone::LightGray),
    )
    .unwrap();
    let rows: Vec<i32> = pen.0.iter().map(|op| op.2).collect();
    assert_eq!(rows, [12, 16, 20]);
    assert!(
        pen.0
            .iter()
            .all(|op| op.0 == "fill" && op.4 == 1 && op.3 == 50)
    );
    // A dark gray: three rows in four; white: none.
    let mut dark = Log::default();
    draw_behind(
        &mut dark,
        FEW,
        &boxes,
        &Decoration::NONE.background(Tone::DarkGray),
    )
    .unwrap();
    assert_eq!(dark.0.len(), 9);
    let mut white = Log::default();
    draw_behind(
        &mut white,
        FEW,
        &boxes,
        &Decoration::NONE.background(Tone::White),
    )
    .unwrap();
    assert!(white.0.is_empty());
}

#[test]
fn a_list_of_highlights_has_a_number_that_changes_with_a_place_a_note_and_the_count() {
    let list = |spans: &[Span]| Marks::new(spans.to_vec()).fingerprint();
    let one = [span(1, 5)];
    assert_eq!(list(&one), list(&[span(1, 5)]));
    assert_ne!(list(&one), list(&[span(1, 6)]));
    assert_ne!(list(&one), list(&[]));
    assert_ne!(list(&one), list(&[span(1, 5), span(1, 5)]));
    let noted = Span {
        noted: true,
        ..span(1, 5)
    };
    assert_ne!(list(&one), list(&[noted]));
}

/// A page of the words of `text`, each 6 offsets long from `start` on.
fn page_of(text: &str, start: u32) -> Vec<Word> {
    let mut at = start;
    text.split(' ')
        .map(|w| {
            let b = word(w, 0, 0, at, at + 5);
            at += 6;
            b
        })
        .collect()
}

fn foreign(from: u32, to: u32, text: &str) -> Foreign {
    Foreign {
        span: span(from, to),
        lead: Lead::of(text).unwrap(),
    }
}

#[test]
fn a_highlight_of_another_conversion_is_drawn_where_the_page_has_its_text() {
    // The page's words start at offset 100, six apart.
    let words = page_of("down the rabbit hole she fell", 100);
    let mut marks = Marks::new(Vec::new()).with_foreign(Vec::from([
        // "rabbit hole she" starts at 112.
        foreign(112, 127, "rabbit hole she"),
    ]));
    assert!(marks.highlights().is_empty());
    marks.accept_foreign(&words);
    assert_eq!(marks.highlights(), [span(112, 127)]);
    // Taken once: another page does not draw it twice.
    marks.accept_foreign(&words);
    assert_eq!(marks.highlights().len(), 1);
}

#[test]
fn a_word_the_layout_broke_is_read_once_for_a_highlight_of_another_conversion() {
    let mut words = page_of("down the particular case of it all", 100);
    // `particular` broken at a line end: both parts are the word, at its place.
    let second = words[2].clone();
    words.insert(3, second);
    let mut marks = Marks::new(Vec::new()).with_foreign(Vec::from([foreign(
        106,
        130,
        "the particular case of it",
    )]));
    marks.accept_foreign(&words);
    assert_eq!(marks.highlights(), [span(106, 130)]);
}

#[test]
fn a_highlight_whose_text_is_not_there_stays_hidden() {
    let words = page_of("down the rabbit hole she fell", 100);
    // The same place, other words: made for another text.
    let mut marks = Marks::new(Vec::new()).with_foreign(Vec::from([
        foreign(112, 127, "queen of hearts"),
        // The text is there, but the place is another page's.
        foreign(5_000, 5_015, "rabbit hole she"),
    ]));
    marks.accept_foreign(&words);
    assert!(marks.highlights().is_empty());
    // Both are kept to be looked at on the pages they are on.
    let again = page_of("down the rabbit hole she fell", 100);
    marks.accept_foreign(&again);
    assert!(marks.highlights().is_empty());
}

#[test]
fn a_highlight_that_runs_over_the_page_end_is_found_by_the_part_on_the_page() {
    let words = page_of("down the rabbit hole she", 100);
    // It starts at "hole" (118) and goes on over the page break.
    let mut marks = Marks::new(Vec::new()).with_foreign(Vec::from([foreign(
        118,
        160,
        "hole she fell down the long well",
    )]));
    marks.accept_foreign(&words);
    assert_eq!(marks.highlights().len(), 1);
}

#[test]
fn a_new_reading_of_the_library_replaces_both_lists() {
    let mut marks = Marks::new(Vec::from([span(0, 5)])).with_foreign(Vec::from([foreign(
        112,
        127,
        "rabbit hole she",
    )]));
    assert_eq!(marks.take(), [span(0, 5)]);
    marks.accept_foreign(&page_of("down the rabbit hole she fell", 100));
    assert!(marks.highlights().is_empty());
    marks.replace((Vec::from([span(1, 2)]), Vec::new()));
    assert_eq!(marks.highlights(), [span(1, 2)]);
}

#[test]
fn a_highlight_a_character_or_two_off_in_this_conversion_is_still_found() {
    let words = page_of("down the rabbit hole she fell", 100);
    // The place says 113 (inside "rabbit", which starts at 112).
    let mut marks =
        Marks::new(Vec::new()).with_foreign(Vec::from([foreign(113, 127, "rabbit hole she")]));
    marks.accept_foreign(&words);
    assert_eq!(marks.highlights().len(), 1);
}

fn short(from: u32, to: u32, text: &str) -> Foreign {
    Foreign {
        span: span(from, to),
        lead: Lead::of_any(text).unwrap(),
    }
}

fn accepted(words: &[Word], held: Foreign) -> Vec<Span> {
    let mut marks = Marks::new(Vec::new()).with_foreign(Vec::from([held]));
    marks.accept_foreign(words);
    marks.highlights().to_vec()
}

#[test]
fn a_short_highlight_of_another_conversion_is_drawn_over_its_very_words() {
    // "Om", "the": a Beta 3 card's highlights of a word (#265). Too short to
    // have a lead, they were never drawn.
    assert!(Lead::of("the").is_none());
    let words = page_of("down the rabbit hole she fell", 100);
    // "the" is at 106.
    assert_eq!(accepted(&words, short(106, 111, "the")), [span(106, 111)]);
    // A character off, still over that word alone.
    assert_eq!(accepted(&words, short(107, 111, "the")), [span(107, 111)]);
    // Two words that are short together.
    let to_be = page_of("to be or not", 100);
    assert_eq!(accepted(&to_be, short(106, 117, "be or")), [span(106, 117)]);
}

#[test]
fn a_short_highlight_is_hidden_where_it_covers_other_words() {
    let words = page_of("down the rabbit hole she fell", 100);
    // Over "rabbit", though "the" is right before it: near is not enough.
    assert!(accepted(&words, short(112, 117, "the")).is_empty());
    // Over more than its text, and over less.
    assert!(accepted(&words, short(106, 117, "the")).is_empty());
    let to_be = page_of("to be or not", 100);
    assert!(accepted(&to_be, short(106, 111, "be or")).is_empty());
    // Not on this page at all.
    assert!(accepted(&words, short(6, 11, "the")).is_empty());
}

#[test]
fn a_short_highlight_that_runs_over_the_page_end_is_told_by_its_first_words() {
    // The page ends with "to"; "to be" goes on over the break.
    let ends = page_of("it is to", 100);
    assert_eq!(accepted(&ends, short(112, 123, "to be")), [span(112, 123)]);
    // The page goes on with another word: not that text.
    let goes_on = page_of("it is to me", 100);
    assert!(accepted(&goes_on, short(112, 123, "to be")).is_empty());
    // A page that ends inside a word of the text has not its first words.
    assert!(accepted(&ends, short(112, 123, "tomb be")).is_empty());
}

/// A pen that fills a checker, as a host with the slot does.
#[derive(Default)]
struct Checkers(Vec<(&'static str, i32, i32, u32, u32, bool)>);

impl Pen for Checkers {
    fn fill(&mut self, x: i32, y: i32, w: u32, h: u32, _: Color) -> Result<(), Error> {
        self.0.push(("fill", x, y, w, h, false));
        Ok(())
    }
    fn stroke(&mut self, _: (i32, i32, u32, u32), _: u32, _: Color) -> Result<(), Error> {
        Ok(())
    }
    fn checker(&mut self, r: (i32, i32, u32, u32), _: Color, odd: bool) -> Result<(), Error> {
        self.0.push(("checker", r.0, r.1, r.2, r.3, odd));
        Ok(())
    }
}

fn shaded(area: Rect, tone: Tone) -> Vec<(&'static str, i32, i32, u32, u32, bool)> {
    let mut pen = Checkers::default();
    draw_behind(&mut pen, FEW, &[area], &Decoration::NONE.background(tone)).unwrap();
    pen.0
}

#[test]
fn on_few_grays_a_background_is_dots_where_the_pen_fills_a_checker() {
    // Beta 3 dithered a highlight's tone to dots; rows ran through the
    // letters like a line through them (#265).
    let area = Rect::new(5, 10, 40, 12);
    // A light gray: every other pixel of every other row, a quarter of them,
    // counted from the screen's corner (the box starts at 0 here).
    let light = shaded(area, Tone::LightGray);
    let rows: Vec<i32> = light.iter().map(|op| op.2).collect();
    assert_eq!(rows, [10, 12, 14, 16, 18, 20]);
    assert!(
        light
            .iter()
            .all(|op| *op == ("checker", 0, op.2, 50, 1, false))
    );
    // A box that starts on an odd column keeps the screen's columns.
    let moved = shaded(Rect::new(8, 10, 40, 12), Tone::LightGray);
    assert!(
        moved
            .iter()
            .all(|op| *op == ("checker", 3, op.2, 50, 1, true))
    );
    // A gray: one checker, half the pixels, from the screen's corner too.
    assert_eq!(
        shaded(area, Tone::Gray),
        [("checker", 0, 10, 50, 12, false)]
    );
    assert_eq!(
        shaded(Rect::new(8, 11, 40, 12), Tone::Gray),
        [("checker", 3, 11, 50, 12, false)]
    );
    assert_eq!(
        shaded(Rect::new(8, 10, 40, 12), Tone::Gray),
        [("checker", 3, 10, 50, 12, true)]
    );
    // A dark gray: the checker and every other row, three quarters.
    let dark = shaded(area, Tone::DarkGray);
    assert_eq!(dark[0], ("checker", 0, 10, 50, 12, false));
    assert_eq!(dark.len(), 7);
    assert!(
        dark[1..]
            .iter()
            .all(|op| op.0 == "fill" && op.4 == 1 && op.2 % 2 == 0)
    );
    // Black is filled, white is nothing.
    assert!(shaded(area, Tone::Black).iter().all(|op| op.0 == "fill"));
    assert_eq!(shaded(area, Tone::Black).len(), 12);
    assert!(shaded(area, Tone::White).is_empty());
}
