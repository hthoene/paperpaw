use super::*;
use crate::native_typeset::{Letters, Measure};
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::Cell;
use paperpaw_sdk::{
    NoStorage, Storage,
    document::{Block, BlockKind, Document, DocumentError, Locator, Metadata, TextStyle},
    layout::{LayoutContext, Style, layout_page},
};

/// Every character is 10 pixels wide at size 20; a line is 1.25 of the size.
#[derive(Debug, Default)]
struct Fixed;

impl Measure for Fixed {
    fn width(&self, role: Role, size: f32, text: &str) -> Result<f32, Error> {
        let bold = if matches!(role, Role::Bold | Role::BoldItalic) {
            2.0
        } else {
            0.0
        };
        Ok((size * 0.5 + bold) * text.chars().count() as f32)
    }
    fn metrics(&self, _: Role, size: f32) -> Result<(f32, f32), Error> {
        Ok((size * 0.8, size * 0.2))
    }
}

/// Like `Fixed`, but `AV` and `VA` are a tenth of the size closer, and the
/// letter `ﬁ` is 12 wide at size 20 (as the font has it).
#[derive(Debug, Default)]
struct Kerned;

impl Measure for Kerned {
    fn width(&self, _: Role, size: f32, text: &str) -> Result<f32, Error> {
        let mut width = 0.0;
        let mut before = None;
        for c in text.chars() {
            width += if c == '\u{fb01}' {
                size * 0.6
            } else {
                size * 0.5
            };
            if matches!((before, c), (Some('A'), 'V') | (Some('V'), 'A')) {
                width -= size * 0.1;
            }
            before = Some(c);
        }
        Ok(width)
    }
    fn metrics(&self, _: Role, size: f32) -> Result<(f32, f32), Error> {
        Ok((size * 0.8, size * 0.2))
    }
    fn covers(&self, _: Role, c: char) -> bool {
        c == '\u{fb01}'
    }
}

struct Book {
    blocks: Vec<Block>,
    metadata: Metadata,
}

impl Document for Book {
    fn metadata(&self) -> &Metadata {
        &self.metadata
    }
    fn sections(&self) -> u32 {
        1
    }
    fn section_len(&self, _: u32) -> u32 {
        self.blocks.last().map_or(0, |b| b.end)
    }
    fn block_at(&self, _: &dyn Storage, at: Locator) -> Result<Option<Block>, DocumentError> {
        Ok(self.blocks.iter().find(|b| b.end > at.offset).cloned())
    }
}

#[derive(Default)]
struct Recorder {
    calls: Vec<(Role, i32, i32, String)>,
    fills: Vec<(i32, i32, u32, u32)>,
    images: Vec<(u32, i32, i32, u32, u32)>,
    image_error: Option<Error>,
    fail_at: Cell<Option<usize>>,
    /// Runs are as wide as `Kerned` sets them.
    kerned: bool,
}

impl Ink for Recorder {
    fn run(
        &mut self,
        role: Role,
        size: f32,
        x: f32,
        baseline: i32,
        text: &str,
    ) -> Result<f32, Error> {
        if self.fail_at.get() == Some(self.calls.len()) {
            return Err(Error::State);
        }
        self.calls.push((role, x as i32, baseline, text.into()));
        match self.kerned {
            true => Kerned.width(role, size, text),
            false => Fixed.width(role, size, text),
        }
    }

    fn fill(&mut self, x: i32, y: i32, width: u32, height: u32) -> Result<(), Error> {
        self.fills.push((x, y, width, height));
        Ok(())
    }

    fn image(&mut self, image: u32, x: i32, y: i32, width: u32, height: u32) -> Result<(), Error> {
        if let Some(error) = self.image_error {
            return Err(error);
        }
        self.images.push((image, x, y, width, height));
        Ok(())
    }
}

fn page(text: &str, style: TextStyle) -> (Page, Faces<Fixed>) {
    page_in(text, style, Faces::new(Fixed, 20.0, 1.0).unwrap())
}

fn page_in<M: Measure>(text: &str, style: TextStyle, faces: Faces<M>) -> (Page, Faces<M>) {
    let mut block = Block::new(BlockKind::Paragraph, 0, text.len() as u32, text.into());
    block.runs = alloc::vec![paperpaw_sdk::document::Run::new(
        0,
        text.len() as u32,
        style
    )];
    let book = Book {
        blocks: alloc::vec![block],
        metadata: Metadata::default(),
    };
    let style = Style::new(0.0, 0.0, 20.0, false);
    let cx = LayoutContext::new(&book, &NoStorage, 400.0, &style, &faces);
    (layout_page(&cx, Locator::START, 400.0).unwrap(), faces)
}

#[test]
fn words_are_set_one_after_the_other_with_the_gaps() {
    let (page, faces) = page("one two three", TextStyle::REGULAR);
    let mut ink = Recorder::default();
    draw_lines(&mut ink, &faces, &page, (30.0, 50.0)).unwrap();
    let words: Vec<&str> = ink.calls.iter().map(|c| c.3.as_str()).collect();
    assert_eq!(words, ["one", "two", "three"]);
    // 10 px a letter and a space: "one" 30, a gap of 10, "two" at 70, "three" at 110.
    let xs: Vec<i32> = ink.calls.iter().map(|c| c.1).collect();
    assert_eq!(xs, [30, 70, 110]);
    // One line: one baseline, below the top.
    assert!(ink.calls.iter().all(|c| c.2 == ink.calls[0].2 && c.2 > 50));
    assert!(ink.calls.iter().all(|c| c.0 == Role::Reading));
}

#[test]
fn a_bold_run_is_drawn_in_the_bold_role() {
    let (page, faces) = page("bold", TextStyle::BOLD);
    let mut ink = Recorder::default();
    draw_lines(&mut ink, &faces, &page, (0.0, 0.0)).unwrap();
    assert_eq!(ink.calls[0].0, Role::Bold);
}

#[test]
fn a_long_text_wraps_and_every_line_has_its_own_baseline() {
    let text = "word ".repeat(40);
    let (page, faces) = page(&text, TextStyle::REGULAR);
    assert!(page.lines.len() > 1);
    let mut ink = Recorder::default();
    draw_lines(&mut ink, &faces, &page, (0.0, 0.0)).unwrap();
    let mut baselines: Vec<i32> = ink.calls.iter().map(|c| c.2).collect();
    baselines.dedup();
    assert_eq!(baselines.len(), page.lines.len());
    assert!(baselines.windows(2).all(|w| w[0] < w[1]));
}

#[test]
fn the_first_refusal_of_the_host_ends_the_drawing() {
    let (page, faces) = page("one two three", TextStyle::REGULAR);
    let mut ink = Recorder::default();
    ink.fail_at.set(Some(1));
    assert_eq!(
        draw_lines(&mut ink, &faces, &page, (0.0, 0.0)),
        Err(Error::State)
    );
    assert_eq!(ink.calls.len(), 1);
}

#[test]
fn characters_that_are_not_drawn_are_left_out_of_a_word() {
    let (page, faces) = page("zero\u{200b}width soft\u{feff}", TextStyle::REGULAR);
    let mut ink = Recorder::default();
    draw_lines(&mut ink, &faces, &page, (0.0, 0.0)).unwrap();
    let words: Vec<&str> = ink.calls.iter().map(|c| c.3.as_str()).collect();
    assert_eq!(words, ["zerowidth", "soft"]);
}

#[test]
fn an_underlined_run_has_a_line_below_the_whole_run() {
    let (page, faces) = page("one two", TextStyle::UNDERLINE);
    let mut ink = Recorder::default();
    draw_lines(&mut ink, &faces, &page, (30.0, 50.0)).unwrap();
    let base = ink.calls[0].2;
    // Size 20: a pixel thick, two below the baseline, the run's 70 pixels wide.
    assert_eq!(ink.fills, [(30, base + 2, 70, 1)]);
}

#[test]
fn a_strike_through_and_an_overline_are_drawn_where_beta_3_drew_them() {
    let style = TextStyle::STRIKE.union(TextStyle::OVERLINE);
    let (page, faces) = page("abc", style);
    let mut ink = Recorder::default();
    draw_lines(&mut ink, &faces, &page, (0.0, 0.0)).unwrap();
    let base = ink.calls[0].2;
    assert_eq!(ink.fills, [(0, base - 15, 30, 1), (0, base - 5, 30, 1)]);
}

#[test]
fn a_double_line_has_a_second_line_and_a_dotted_one_has_pieces() {
    let double = TextStyle::UNDERLINE.with_line_style(paperpaw_sdk::document::LineStyle::Double);
    let (double_page, faces) = page("abc", double);
    let mut ink = Recorder::default();
    draw_lines(&mut ink, &faces, &double_page, (0.0, 0.0)).unwrap();
    let base = ink.calls[0].2;
    assert_eq!(ink.fills, [(0, base + 2, 30, 1), (0, base + 4, 30, 1)]);
    let dotted = TextStyle::UNDERLINE.with_line_style(paperpaw_sdk::document::LineStyle::Dotted);
    let (dotted_page, faces) = page("abc", dotted);
    let mut ink = Recorder::default();
    draw_lines(&mut ink, &faces, &dotted_page, (0.0, 0.0)).unwrap();
    // One pixel dots with one pixel gaps over 30 pixels.
    assert_eq!(ink.fills.len(), 15);
    assert!(ink.fills.iter().all(|f| f.2 == 1 && f.3 == 1));
}

#[test]
fn a_text_without_lines_and_a_page_without_rules_fill_nothing() {
    let (page, faces) = page("plain", TextStyle::REGULAR);
    let mut ink = Recorder::default();
    draw_lines(&mut ink, &faces, &page, (0.0, 0.0)).unwrap();
    assert!(ink.fills.is_empty());
}

#[test]
fn a_tables_rules_are_filled_at_the_content_box() {
    let (mut page, faces) = page("x", TextStyle::REGULAR);
    page.rules
        .push(paperpaw_sdk::layout::PageRule::new(5.0, 6.0, 100.0, 1.0));
    page.rules
        .push(paperpaw_sdk::layout::PageRule::new(5.0, 6.0, 0.2, 40.0));
    let mut ink = Recorder::default();
    draw_lines(&mut ink, &faces, &page, (30.0, 50.0)).unwrap();
    // A rule is a pixel thick at least.
    assert_eq!(ink.fills, [(35, 56, 100, 1), (35, 56, 1, 40)]);
}

#[test]
fn a_refused_fill_ends_the_drawing() {
    struct Refuses;
    impl Ink for Refuses {
        fn run(&mut self, _: Role, _: f32, _: f32, _: i32, text: &str) -> Result<f32, Error> {
            Ok(text.len() as f32 * 10.0)
        }
        fn fill(&mut self, _: i32, _: i32, _: u32, _: u32) -> Result<(), Error> {
            Err(Error::State)
        }
        fn image(&mut self, _: u32, _: i32, _: i32, _: u32, _: u32) -> Result<(), Error> {
            Ok(())
        }
    }
    let (page, faces) = page("abc", TextStyle::UNDERLINE);
    assert!(draw_lines(&mut Refuses, &faces, &page, (0.0, 0.0)).is_err());
}

fn marked(page: &Page, faces: &Faces<Fixed>, words: core::ops::Range<usize>) -> Recorder {
    let mut ink = Recorder::default();
    draw_lines_marked(&mut ink, faces, page, (30.0, 50.0), Some(words)).unwrap();
    ink
}

#[test]
fn the_words_of_the_sentence_read_aloud_have_one_line_under_them() {
    let (page, faces) = page("one two three", TextStyle::REGULAR);
    let ink = marked(&page, &faces, 1..3);
    let base = ink.calls[0].2;
    // "two" from 70 to 100, a gap of 10, "three" to 160: one line, as the book's own.
    assert_eq!(ink.fills, [(70, base + 2, 90, 1)]);
    // Nothing marked: nothing under it, and the words are drawn as always.
    assert!(marked(&page, &faces, 0..0).fills.is_empty());
    assert!(marked(&page, &faces, 5..9).fills.is_empty());
    assert_eq!(ink.calls.len(), 3);
    // Without a mark it is draw_lines.
    let mut plain = Recorder::default();
    draw_lines_marked(&mut plain, &faces, &page, (30.0, 50.0), None).unwrap();
    assert!(plain.fills.is_empty());
}

#[test]
fn a_marked_stretch_ends_with_its_line_and_goes_on_on_the_next() {
    let text = "word ".repeat(20);
    let (page, faces) = page(text.trim_end(), TextStyle::REGULAR);
    assert!(page.lines.len() > 1);
    let first_line = page.lines[0].text.split(' ').count();
    // The last word of the first line and the first of the second: two lines.
    let ink = marked(&page, &faces, first_line - 1..first_line + 1);
    assert_eq!(ink.fills.len(), 2);
    assert!(ink.fills[0].1 < ink.fills[1].1);
}

#[test]
fn a_word_in_two_runs_is_one_word() {
    let mut block = Block::new(BlockKind::Paragraph, 0, 8, "abcd efg".into());
    block.runs = alloc::vec![
        paperpaw_sdk::document::Run::new(0, 2, TextStyle::BOLD),
        paperpaw_sdk::document::Run::new(2, 8, TextStyle::REGULAR),
    ];
    let book = Book {
        blocks: alloc::vec![block],
        metadata: Metadata::default(),
    };
    let faces = Faces::new(Fixed, 20.0, 1.0).unwrap();
    let style = Style::new(0.0, 0.0, 20.0, false);
    let cx = LayoutContext::new(&book, &NoStorage, 400.0, &style, &faces);
    let page = layout_page(&cx, Locator::START, 400.0).unwrap();
    // "abcd" is word 0 (the bold "ab" and the regular "cd" pieces): both underlined
    // in one line over its 44 pixels (a bold letter is 2 wider in this measure).
    let ink = marked(&page, &faces, 0..1);
    assert_eq!(ink.fills.len(), 1);
    assert_eq!((ink.fills[0].0, ink.fills[0].2), (30, 44));
    // "efg" is word 1: from its own start, after a space.
    let second = marked(&page, &faces, 1..2);
    assert_eq!(second.fills, [(84, second.calls[0].2 + 2, 30, 1)]);
}

fn picture_page(handle: &str) -> (Page, Faces<Fixed>) {
    let mut block = Block::new(BlockKind::Image, 0, 1, String::new());
    block.image = Some(paperpaw_sdk::document::ImageRef {
        path: handle.into(),
        width: 200,
        height: 100,
    });
    let book = Book {
        blocks: alloc::vec![block],
        metadata: Metadata::default(),
    };
    let faces = Faces::new(Fixed, 20.0, 1.0).unwrap();
    let style = Style::new(0.0, 0.0, 20.0, false);
    let cx = LayoutContext::new(&book, &NoStorage, 400.0, &style, &faces);
    (layout_page(&cx, Locator::START, 400.0).unwrap(), faces)
}

#[test]
fn the_draw_that_counts_pictures_draws_the_pictures_and_no_text() {
    // The same picture as the page draws, at the same box: and nothing else.
    let (picture, faces) = picture_page("5:0");
    let mut whole = Recorder::default();
    draw_lines(&mut whole, &faces, &picture, (10.0, 20.0)).unwrap();
    let mut only = Recorder::default();
    draw_pictures(&mut only, &picture, (10.0, 20.0), Pictures::Drawn).unwrap();
    assert_eq!(only.images, whole.images);
    assert_eq!(only.images.len(), 1);
    assert!(only.calls.is_empty() && only.fills.is_empty());
    // A text page: no draw at all. Their places kept empty: none either.
    let (text, _) = page("one two three", TextStyle::REGULAR);
    let mut none = Recorder::default();
    draw_pictures(&mut none, &text, (0.0, 0.0), Pictures::Drawn).unwrap();
    draw_pictures(&mut none, &picture, (0.0, 0.0), Pictures::Places).unwrap();
    assert!(none.calls.is_empty() && none.images.is_empty());
}

#[test]
fn a_picture_is_drawn_scaled_into_the_box_the_layout_gave_it() {
    let (page, faces) = picture_page("7");
    let mut ink = Recorder::default();
    draw_lines(&mut ink, &faces, &page, (30.0, 50.0)).unwrap();
    // 200 by 100 fits a 400-wide, 400-high box at twice its size, centred.
    assert_eq!(ink.images.len(), 1);
    let (handle, x, y, w, h) = ink.images[0];
    assert_eq!((handle, w, h), (7, 400, 200));
    assert_eq!(x, 30);
    assert!(y >= 50);
}

#[test]
fn a_picture_the_host_does_not_know_leaves_its_place_empty() {
    for error in [Error::NotFound, Error::Unsupported] {
        let (page, faces) = picture_page("7");
        let mut ink = Recorder {
            image_error: Some(error),
            ..Recorder::default()
        };
        draw_lines(&mut ink, &faces, &page, (0.0, 0.0)).unwrap();
    }
    // Handle 0 (the host had no id to give) is no picture either, and does not fail the page.
    let (page, faces) = picture_page("0");
    let mut ink = Recorder::default();
    draw_lines(&mut ink, &faces, &page, (0.0, 0.0)).unwrap();
    assert!(ink.images.is_empty());
    // A handle that is no number is no picture; another refusal ends the drawing.
    let (page, faces) = picture_page("x7");
    let mut ink = Recorder::default();
    draw_lines(&mut ink, &faces, &page, (0.0, 0.0)).unwrap();
    assert!(ink.images.is_empty());
    let (page, faces) = picture_page("7");
    let mut ink = Recorder {
        image_error: Some(Error::Io),
        ..Recorder::default()
    };
    assert_eq!(
        draw_lines(&mut ink, &faces, &page, (0.0, 0.0)),
        Err(Error::Io)
    );
}

#[test]
fn the_text_of_a_page_is_the_same_with_its_pictures_and_without() {
    let mut block = Block::new(BlockKind::Paragraph, 0, 9, "Some text".into());
    block.runs = alloc::vec![paperpaw_sdk::document::Run::new(0, 9, TextStyle::REGULAR)];
    let mut picture = Block::new(BlockKind::Image, 9, 10, String::new());
    picture.image = Some(paperpaw_sdk::document::ImageRef {
        path: "7".into(),
        width: 200,
        height: 100,
    });
    let book = Book {
        blocks: alloc::vec![block, picture],
        metadata: Metadata::default(),
    };
    let faces = Faces::new(Fixed, 20.0, 1.0).unwrap();
    let style = Style::new(0.0, 0.0, 20.0, false);
    let cx = LayoutContext::new(&book, &NoStorage, 400.0, &style, &faces);
    let page = layout_page(&cx, Locator::START, 400.0).unwrap();
    let draw = |pictures| {
        let mut ink = Recorder::default();
        draw_page(&mut ink, &faces, &page, (30.0, 50.0), (None, pictures)).unwrap();
        ink
    };
    let (first, second) = (draw(Pictures::Places), draw(Pictures::Drawn));
    // The first frame keeps the picture's place empty; the second draws it
    // there, in the box `picture_boxes` names; the text is the same in both.
    assert!(first.images.is_empty());
    assert_eq!(second.images.len(), 1);
    assert_eq!(first.calls, second.calls);
    assert_eq!(first.fills, second.fills);
    let (_, x, y, w, h) = second.images[0];
    assert_eq!(
        picture_boxes(&page, (30.0, 50.0)).collect::<Vec<_>>(),
        [(x, y, w, h)]
    );
}

#[test]
fn a_pictures_path_names_its_image_id_and_its_index() {
    assert_eq!(picture_of("12:3"), Some((12, Some(3))));
    assert_eq!(picture_of("0:7"), Some((0, Some(7))));
    assert_eq!(picture_of("12"), Some((12, None)));
    assert_eq!(picture_of("12:x"), None);
    assert_eq!(picture_of("cover.png"), None);
}

#[test]
fn small_capitals_are_drawn_as_smaller_capitals() {
    // #257 (boox-small-caps): "The reader" in small caps was drawn as written.
    let (page, faces) = page("The reader", TextStyle::SMALL_CAPS);
    let mut ink = Recorder::default();
    draw_lines(&mut ink, &faces, &page, (0.0, 0.0)).unwrap();
    let parts: Vec<&str> = ink.calls.iter().map(|c| c.3.as_str()).collect();
    assert_eq!(parts, ["T", "HE", "READER"]);
    // The capitals of lower-case letters are narrower than full ones, as the
    // layout measured them: "HE" starts where "T" ends, at 10.
    assert_eq!(ink.calls[1].1, 10);
}

/// The runs a text is drawn in, as (text, x), with `letters` set on `Kerned` fonts.
fn runs_of(text: &str, letters: Letters) -> Vec<(String, i32)> {
    let faces = Faces::new(Kerned, 20.0, 1.0).unwrap().with_letters(letters);
    let (page, faces) = page_in(text, TextStyle::REGULAR, faces);
    let mut ink = Recorder {
        kerned: true,
        ..Recorder::default()
    };
    draw_lines(&mut ink, &faces, &page, (30.0, 50.0)).unwrap();
    ink.calls.into_iter().map(|c| (c.3, c.1)).collect()
}

#[test]
fn spaced_letters_are_drawn_one_by_one_with_the_space_after_each() {
    // 10 percent of 20 px: a letter is 10 px and 2 px of space.
    let runs = runs_of("abc", Letters::new(10, true, true));
    let expected: Vec<(String, i32)> = [("a", 30), ("b", 42), ("c", 54)]
        .into_iter()
        .map(|(t, x)| (t.into(), x))
        .collect();
    assert_eq!(runs, expected);
    // None: the word is one run, as the host draws it.
    assert_eq!(
        runs_of("abc", Letters::new(0, true, true)),
        [("abc".into(), 30)]
    );
}

#[test]
fn a_space_between_words_is_not_spaced() {
    // "ab cd": "ab" 24 wide (two letters of 12), a space of 10, "cd" at 30 + 24 + 10.
    let runs = runs_of("ab cd", Letters::new(10, true, true));
    let xs: Vec<i32> = runs.iter().map(|r| r.1).collect();
    assert_eq!(xs, [30, 42, 64, 76]);
}

#[test]
fn kerning_off_draws_a_kerned_pair_as_two_runs_each_by_its_advance() {
    // Kerned: the host draws the word at once and sets the pair closer.
    assert_eq!(
        runs_of("AVA", Letters::new(0, true, true)),
        [("AVA".into(), 30)]
    );
    // Not kerned: the pairs are cut, the pen goes by the advance alone.
    let expected: Vec<(String, i32)> = [("A", 30), ("V", 40), ("A", 50)]
        .into_iter()
        .map(|(t, x)| (t.into(), x))
        .collect();
    assert_eq!(runs_of("AVA", Letters::new(0, false, true)), expected);
    // A word with no kerned pair stays one run.
    assert_eq!(
        runs_of("abc", Letters::new(0, false, true)),
        [("abc".into(), 30)]
    );
}

#[test]
fn a_ligature_is_drawn_in_place_of_its_letters_where_the_font_has_it() {
    // "ﬁ" is 12 wide: the rest of the word follows at 30 + 12.
    let expected: Vec<(String, i32)> = [("\u{fb01}", 30), ("ne", 42)]
        .into_iter()
        .map(|(t, x)| (t.into(), x))
        .collect();
    assert_eq!(runs_of("fine", Letters::new(0, true, true)), expected);
    // Off, spaced, or a letter the font lacks (`fl`): the letters as they are.
    assert_eq!(
        runs_of("fine", Letters::new(0, true, false)),
        [("fine".into(), 30)]
    );
    assert_eq!(
        runs_of("flat", Letters::new(0, true, true)),
        [("flat".into(), 30)]
    );
    assert_eq!(runs_of("fi", Letters::new(5, true, true)).len(), 2);
}

#[test]
fn a_word_is_as_wide_as_the_layout_measured_it_for_every_setting() {
    for set in [
        Letters::new(0, true, true),
        Letters::new(0, false, true),
        Letters::new(0, false, false),
        Letters::new(10, true, true),
        Letters::new(-5, false, true),
    ] {
        let faces = Faces::new(Kerned, 20.0, 1.0).unwrap().with_letters(set);
        let face = paperpaw_sdk::layout::Face::new(TextStyle::REGULAR, 0, 0);
        for word in ["AVA", "fine", "affix", "AVfi"] {
            let mut ink = Recorder {
                kerned: true,
                ..Recorder::default()
            };
            let drawn = if faces.drawn_at_once(face, word) {
                draw_word(&mut ink, (Role::Reading, 20.0), 0.0, 0, word).unwrap()
            } else {
                letters::draw_apart(&mut ink, &faces, face, (0.0, 0), word).unwrap()
            };
            let laid = faces.advance_text(face, word, &paperpaw_sdk::text::Around::NONE);
            assert!(
                (drawn - laid).abs() < 1e-3,
                "{word} {set:?}: {drawn} {laid}"
            );
        }
    }
}

/// Where each line of a justified page of kerned pairs ends when it is laid out
/// with `Kerned` and drawn with runs as wide as `drawn_kerned` says.
fn line_ends(drawn_kerned: bool) -> Vec<f32> {
    let words = "AVA VAV fine AVAVA office VAVAV affix AVA fine";
    let text = alloc::vec![words; 8].join(" ");
    let faces = Faces::new(Kerned, 20.0, 1.0).unwrap();
    let mut block = Block::new(BlockKind::Paragraph, 0, text.len() as u32, text.clone());
    block.runs = alloc::vec![paperpaw_sdk::document::Run::new(
        0,
        text.len() as u32,
        TextStyle::REGULAR
    )];
    let book = Book {
        blocks: alloc::vec![block],
        metadata: Metadata::default(),
    };
    let style = Style::new(0.0, 0.0, 20.0, true);
    let cx = LayoutContext::new(&book, &NoStorage, 400.0, &style, &faces);
    let page = layout_page(&cx, Locator::START, 400.0).unwrap();
    let mut ink = Recorder {
        kerned: drawn_kerned,
        ..Recorder::default()
    };
    draw_lines(&mut ink, &faces, &page, (0.0, 0.0)).unwrap();
    let mut ends: Vec<(i32, f32)> = Vec::new();
    for (role, x, baseline, run) in &ink.calls {
        let width = match drawn_kerned {
            true => Kerned.width(*role, 20.0, run),
            false => Fixed.width(*role, 20.0, run),
        }
        .unwrap();
        let end = *x as f32 + width;
        match ends.last_mut() {
            Some(line) if line.0 == *baseline => line.1 = line.1.max(end),
            _ => ends.push((*baseline, end)),
        }
    }
    ends.into_iter().map(|line| line.1).collect()
}

#[test]
fn no_line_is_drawn_wider_than_the_measure_it_was_laid_out_in() {
    // Laid out and drawn by the same font: every line of the page ends inside
    // the 400 px (a justified one at it, to a pixel).
    let ends = line_ends(true);
    assert!(ends.len() >= 5, "{ends:?}");
    for (at, end) in ends.iter().enumerate() {
        assert!(*end <= 400.5, "line {at} ends at {end}");
        if at + 1 < ends.len() {
            assert!(*end >= 399.0, "line {at} ends at {end}, short of 400");
        }
    }
}

#[test]
fn the_check_for_a_line_wider_than_its_measure_sees_one() {
    // A font that draws the pairs wider than the layout measured them (`Fixed`
    // ignores the pair `AV` the layout closed): the same check finds lines past
    // the measure. Without this the check above could pass on any page.
    let ends = line_ends(false);
    assert!(ends.iter().any(|end| *end > 400.5), "{ends:?}");
}
