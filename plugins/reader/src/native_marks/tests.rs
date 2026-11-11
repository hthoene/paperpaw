use super::*;
use paperpaw_sdk::document::Locator;

fn word(text: &str, x: u16, y: u16, start: u32, end: u32) -> WordBox {
    WordBox::new(
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

const GAUGE: Gauge = Gauge { dpi: 254 };

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
