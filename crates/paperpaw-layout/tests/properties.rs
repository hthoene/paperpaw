//! Properties of page layout for random books: paragraphs of words of any
//! length (very long ones, CJK without spaces, soft hyphens, non-breaking
//! spaces), headings, bold runs, book sizes and alignments, laid out at
//! random widths and heights, with and without hyphenation.
//!
//! Checked, also with pairs of characters kerned as fonts kern them: no
//! line is wider than its box (except a line of a single glyph that is
//! wider by itself), measured as its words are drawn, every character of the book is on
//! exactly one line of exactly one page and in order, no page is taller
//! than its box, a page laid out again from a kept position is the same,
//! and a position kept at one width is on the page shown at another.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::collections::BTreeSet;

use paperpaw_document::{
    Align, Block, BlockKind, Document, DocumentError, Locator, Metadata, NoStorage, OffsetMap, Run,
    Storage, TextStyle,
};
use paperpaw_layout::{Face, LayoutContext, Page, Style, Typesetter, layout_page, line_start};
use proptest::prelude::*;

/// A paragraph of the test book.
#[derive(Debug, Clone)]
struct Para {
    text: String,
    kind: BlockKind,
    runs: Vec<Run>,
    align: Align,
    size: Option<u16>,
    /// The book's first-line indent, hundredths of an em.
    indent: Option<i16>,
    /// The book's margins at the sides, hundredths of an em.
    margins: (u16, u16),
}

/// Sections of paragraphs; in the source every paragraph ends with `\n`,
/// so a paragraph's text maps to its source one to one.
#[derive(Debug, Clone)]
struct Book {
    sections: Vec<Vec<Para>>,
    metadata: Metadata,
}

impl Book {
    fn source(&self, section: u32) -> String {
        let mut source = String::new();
        for para in &self.sections[section as usize] {
            source.push_str(&para.text);
            source.push('\n');
        }
        source
    }
}

impl Document for Book {
    fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    fn sections(&self) -> u32 {
        self.sections.len() as u32
    }

    fn section_len(&self, section: u32) -> u32 {
        self.sections.get(section as usize).map_or(0, |paras| {
            paras.iter().map(|p| p.text.len() as u32 + 1).sum()
        })
    }

    fn block_at(&self, _: &dyn Storage, at: Locator) -> Result<Option<Block>, DocumentError> {
        let Some(paras) = self.sections.get(at.section as usize) else {
            return Ok(None);
        };
        let mut start = 0u32;
        for para in paras {
            let end = start + para.text.len() as u32 + 1;
            if at.offset < end {
                return Ok(Some(
                    Block::new(para.kind, start, end, para.text.clone())
                        .map(OffsetMap::IDENTITY)
                        .runs(para.runs.clone())
                        .align(para.align)
                        .indent(para.indent)
                        .margins(para.margins.0, para.margins.1)
                        .size(para.size),
                ));
            }
            start = end;
        }
        Ok(None)
    }
}

/// Widths by face size (book sizes, headings) and, if `varied`, also by
/// character and style: CJK twice as wide, soft hyphens without width,
/// bold wider; if `kerned`, pairs closer and wider as fonts kern them.
#[derive(Clone, Copy)]
struct Widths {
    varied: bool,
    kerned: bool,
}

const EVEN: Widths = Widths {
    varied: false,
    kerned: false,
};
const VARIED: Widths = Widths {
    varied: true,
    kerned: false,
};
const KERNED: Widths = Widths {
    varied: true,
    kerned: true,
};

fn scale(face: Face) -> f32 {
    let size = if face.size == 0 {
        1.0
    } else {
        f32::from(face.size) / 100.0
    };
    let heading = if face.heading > 0 { 1.5 } else { 1.0 };
    size * heading
}

impl Typesetter for Widths {
    fn advance(&self, face: Face, c: char) -> f32 {
        if !self.varied {
            return scale(face);
        }
        let base = match c {
            ' ' => 0.5,
            '\u{ad}' => 0.0,
            '-' => 0.6,
            c if ('\u{4e00}'..='\u{9fff}').contains(&c) => 2.0,
            c if c.is_ascii_lowercase() => 1.0 + (c as u32 % 3) as f32 * 0.25,
            _ => 1.1,
        };
        let bold = if face.style.bold() { 1.2 } else { 1.0 };
        base * bold * scale(face)
    }

    fn line(&self, face: Face) -> (f32, f32) {
        let h = 10.0 * scale(face);
        (0.8 * h, h)
    }

    fn kern(&self, face: Face, left: char, right: char) -> f32 {
        if !self.kerned {
            return 0.0;
        }
        let pair = match (left, right) {
            ('a' | 'e' | 'o', 'v' | 'w' | 'y') | ('v' | 'w' | 'y' | 't', 'a' | 'e' | 'o') => -0.3,
            ('f', 'f' | 'i' | 'l') => -0.15,
            (c, '-') if c.is_alphabetic() => 0.2,
            ('-', c) if c.is_alphabetic() => 0.1,
            ('\u{4e00}'..='\u{9fff}', _) => -0.05,
            _ => 0.0,
        };
        pair * scale(face)
    }
}

/// Breaks every word of four or more letters after every second letter.
fn every_second(word: &str, out: &mut dyn FnMut(usize)) {
    let offsets: Vec<usize> = word.char_indices().map(|(i, _)| i).collect();
    let n = offsets.len();
    for k in (2..n.saturating_sub(1)).step_by(2) {
        out(offsets[k]);
    }
}

/// Sizes in the book never exceed this, nor headings 1.5 times body text.
const MAX_SCALE: f32 = 2.0 * 1.5;

fn word() -> impl Strategy<Value = String> {
    let letter = prop_oneof![
        8 => proptest::char::range('a', 'z'),
        1 => Just('ä'),
        1 => Just('ß'),
        1 => Just('\u{ad}'),
        1 => Just('-'),
        1 => Just('\u{a0}'),
    ];
    prop_oneof![
        10 => proptest::collection::vec(letter.clone(), 1..10),
        // Longer than any line.
        1 => proptest::collection::vec(letter, 20..70),
        // CJK without spaces.
        2 => proptest::collection::vec(proptest::char::range('\u{4e00}', '\u{4e40}'), 1..40),
    ]
    .prop_map(|chars| chars.into_iter().collect())
}

fn para() -> impl Strategy<Value = Para> {
    (
        proptest::collection::vec((word(), any::<bool>(), 0u8..8), 1..14),
        0u8..10,
        0u8..5,
        prop_oneof![4 => Just(None), 1 => (50u16..200).prop_map(Some)],
        // Now and then the book's margins at the sides, and a hanging
        // indent into them.
        prop_oneof![3 => Just(((0u16, 0u16), None)), 1 => ((0u16..400, 0u16..400), -300i16..100).prop_map(|(m, i)| (m, Some(i)))],
    )
        .prop_map(|(words, kind, align, size, (margins, indent))| {
            let mut text = String::new();
            let mut runs = Vec::new();
            for (i, (word, bold, gap)) in words.iter().enumerate() {
                if i > 0 {
                    // Mostly one space, sometimes two.
                    text.push_str(if *gap == 0 { "  " } else { " " });
                }
                let start = text.len() as u32;
                text.push_str(word);
                if *bold {
                    runs.push(Run::new(start, text.len() as u32, TextStyle::BOLD));
                }
            }
            Para {
                text,
                kind: if kind == 0 {
                    BlockKind::Heading(1 + align % 3)
                } else {
                    BlockKind::Paragraph
                },
                runs,
                align: Align::from_bits(align),
                size,
                indent,
                margins,
            }
        })
}

fn book() -> impl Strategy<Value = Book> {
    proptest::collection::vec(proptest::collection::vec(para(), 1..6), 1..3).prop_map(|sections| {
        Book {
            sections,
            metadata: Metadata::default(),
        }
    })
}

fn style() -> impl Strategy<Value = Style> {
    (0.0f32..6.0, 0.0f32..4.0, any::<bool>(), any::<bool>()).prop_map(
        |(paragraph_spacing, indent, justify, balanced)| {
            Style::new(paragraph_spacing, indent, 10.0, justify).balanced(balanced)
        },
    )
}

fn context<'a>(
    book: &'a Book,
    width: f32,
    style: &'a Style,
    hyphenate: bool,
    widths: &'a Widths,
) -> LayoutContext<'a> {
    LayoutContext::new(book, &NoStorage, width, style, widths)
        .hyphenate(hyphenate.then_some(&every_second as &dyn Fn(&str, &mut dyn FnMut(usize))))
}

/// All pages from the start, as a reader turns them.
fn paginate(cx: &LayoutContext<'_>, height: f32, book: &Book) -> Vec<Page> {
    let bytes: u32 = (0..book.sections()).map(|s| book.section_len(s)).sum();
    let mut pages = Vec::new();
    let mut at = Locator::START;
    loop {
        let page = layout_page(cx, at, height).unwrap();
        if page.is_past_end() {
            break;
        }
        assert!(page.end > at, "page at {at:?} does not move on: {page:?}");
        at = page.end;
        pages.push(page);
        assert!(
            pages.len() as u32 <= bytes + 2 * book.sections(),
            "more pages than characters"
        );
    }
    pages
}

/// The width a line takes as it is drawn, word by word and each run of a
/// face at once: its characters in their faces with the pairs inside
/// each, plus the extra space of justification.
fn line_width(line: &paperpaw_layout::PageLine, widths: Widths) -> f32 {
    let mut glyphs = 0.0;
    for run in &line.runs {
        for word in line.text[run.start..run.end].split(' ') {
            let mut before = None;
            for c in word.chars() {
                if let Some(left) = before {
                    glyphs += widths.kern(run.face, left, c);
                }
                glyphs += widths.advance(run.face, c);
                before = Some(c);
            }
        }
        // The spaces between the words.
        let spaces = line.text[run.start..run.end].matches(' ').count();
        glyphs += spaces as f32 * widths.advance(run.face, ' ');
    }
    glyphs + line.word_spacing * line.text.matches(' ').count() as f32
}

fn check_pages(cx: &LayoutContext<'_>, book: &Book, pages: &[Page], height: f32, widths: Widths) {
    let (width, hyphenate) = (cx.width, cx.hyphenate.is_some());
    let mut words: Vec<(u32, u32, u32)> = Vec::new();
    for page in pages {
        let mut last_baseline = f32::NEG_INFINITY;
        for line in &page.lines {
            // Runs cover the text in order.
            let mut at = 0;
            for run in &line.runs {
                assert_eq!(run.start, at, "runs leave a gap in {line:?}");
                assert!(run.end > run.start);
                at = run.end;
            }
            assert_eq!(at, line.text.len(), "runs do not cover {line:?}");

            // No wider than the box, unless a single glyph is.
            let glyphs = line.text.chars().filter(|c| *c != ' ').count();
            let right = line.x + line_width(line, widths);
            assert!(
                right <= width + 1e-3 || glyphs <= 1,
                "line {:?} ends at {right}, box is {width} wide",
                line.text
            );
            assert!(line.x >= 0.0);

            // No taller than the box: lines go down; a line that does
            // not fit is alone on its page.
            let face = line.runs[0].face;
            let (ascent, line_height) = widths.line(face);
            assert!(line.baseline > last_baseline);
            last_baseline = line.baseline;
            let bottom = line.baseline - ascent + line_height;
            assert!(
                bottom <= height + 1e-3 || page.lines.len() == 1,
                "line {:?} reaches {bottom}, page is {height} high",
                line.text
            );

            // The words are the line's text, from where the book has them;
            // a hyphen is added only where a word was broken.
            let source = book.source(line.section);
            let from_source: String = line
                .words
                .iter()
                .map(|&(s, e)| &source[s as usize..e as usize])
                .collect();
            let shown: String = line.text.chars().filter(|c| *c != ' ').collect();
            if shown != from_source {
                assert!(hyphenate, "{shown:?} is not {from_source:?}");
                assert_eq!(shown.strip_suffix('-'), Some(from_source.as_str()));
            }
            words.extend(line.words.iter().map(|&(s, e)| (line.section, s, e)));
        }
    }

    // Every character but spaces and paragraph ends is on exactly one
    // line, in order.
    for pair in words.windows(2) {
        let ((s0, _, e0), (s1, b1, _)) = (pair[0], pair[1]);
        assert!((s0, e0) <= (s1, b1), "words overlap or go back: {pair:?}");
    }
    let covered: BTreeSet<(u32, u32)> = words
        .iter()
        .flat_map(|&(section, s, e)| (s..e).map(move |b| (section, b)))
        .collect();
    let mut expected = BTreeSet::new();
    for section in 0..book.sections() {
        for (i, b) in book.source(section).bytes().enumerate() {
            if b != b' ' && b != b'\n' {
                expected.insert((section, i as u32));
            }
        }
    }
    assert_eq!(
        covered.len(),
        words
            .iter()
            .map(|&(_, s, e)| (e - s) as usize)
            .sum::<usize>(),
        "a character is shown twice"
    );
    let lost: Vec<_> = expected.difference(&covered).take(5).collect();
    assert!(lost.is_empty(), "characters lost: {lost:?}");
    let extra: Vec<_> = covered.difference(&expected).take(5).collect();
    assert!(extra.is_empty(), "spaces shown as words: {extra:?}");
}

/// A random non-space character of the book.
fn position(book: &Book, pick: (usize, usize)) -> Option<Locator> {
    let section = pick.0 % book.sections.len();
    let source = book.source(section as u32);
    let chars: Vec<usize> = source
        .char_indices()
        .filter(|(_, c)| *c != ' ' && *c != '\n')
        .map(|(i, _)| i)
        .collect();
    let offset = *chars.get(pick.1 % chars.len().max(1))?;
    Some(Locator::new(section as u32, offset as u32))
}

/// Whether a page shows the character at `at`.
fn shows(page: &Page, at: Locator) -> bool {
    page.lines.iter().any(|line| {
        line.section == at.section
            && line
                .words
                .iter()
                .any(|&(s, e)| s <= at.offset && at.offset < e)
    })
}

/// Lays the book out from the start and checks every page.
fn pages_fit(book: &Book, style: &Style, width: f32, height: f32, hyphenate: bool, widths: Widths) {
    let cx = context(book, width, style, hyphenate, &widths);
    let pages = paginate(&cx, height, book);
    assert!(!pages.is_empty());
    check_pages(&cx, book, &pages, height, widths);
    // Laid out again from where it starts, every page is the same.
    for page in &pages {
        assert_eq!(&layout_page(&cx, page.start, height).unwrap(), page);
    }
}

/// Positions of the book kept at one width are found at another: the
/// line `line_start` gives is the one reading from the start shows at
/// that place, and a page laid out from it begins with that very line.
fn positions_survive(
    book: &Book,
    style: &Style,
    width: f32,
    height: f32,
    hyphenate: bool,
    widths: Widths,
    picks: &[(usize, usize)],
) {
    let cx = context(book, width, style, hyphenate, &widths);
    let pages = paginate(&cx, height, book);
    let starts: Vec<(Locator, &paperpaw_layout::PageLine)> = pages
        .iter()
        .flat_map(|page| page.lines.iter())
        .filter_map(|line| {
            let first = line.words.first()?;
            Some((Locator::new(line.section, first.0), line))
        })
        .collect();
    for &pick in picks {
        let Some(at) = position(book, pick) else {
            continue;
        };
        let start = line_start(&cx, at).unwrap();
        assert!(start <= at, "{start:?} after {at:?}");
        let (expected_start, expected) = *starts
            .iter()
            .rev()
            .find(|(s, _)| *s <= at)
            .expect("a line before every character");
        assert_eq!(start, expected_start, "the line of {at:?}");
        let page = layout_page(&cx, start, height).unwrap();
        assert!(shows(&page, at), "{at:?} not on the page from {start:?}");
        assert_eq!(page.lines[0].text, expected.text);
        assert_eq!(page.lines[0].words, expected.words);
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(96),
        failure_persistence: None,
        ..ProptestConfig::default()
    })]

    #[test]
    fn lines_fit_and_every_character_is_shown_once(
        book in book(),
        style in style(),
        width in 2.0f32..60.0,
        lines in 0.5f32..15.0,
        hyphenate in any::<bool>(),
    ) {
        pages_fit(&book, &style, width, lines * 10.0 * MAX_SCALE, hyphenate, EVEN);
    }

    #[test]
    fn a_kept_position_is_on_the_page_shown_at_any_width(
        book in book(),
        style in style(),
        widths in (3.0f32..60.0, 3.0f32..60.0),
        lines in 1.0f32..15.0,
        hyphenate in any::<bool>(),
        picks in proptest::collection::vec((any::<usize>(), any::<usize>()), 1..8),
    ) {
        let height = lines * 10.0 * MAX_SCALE;
        for width in [widths.0, widths.1] {
            positions_survive(&book, &style, width, height, hyphenate, EVEN, &picks);
        }
    }

    #[test]
    fn lines_of_varied_glyphs_fit_and_positions_survive(
        book in book(),
        style in style(),
        widths in (3.0f32..60.0, 3.0f32..60.0),
        lines in 1.0f32..15.0,
        hyphenate in any::<bool>(),
        picks in proptest::collection::vec((any::<usize>(), any::<usize>()), 1..8),
    ) {
        let height = lines * 10.0 * MAX_SCALE;
        for width in [widths.0, widths.1] {
            pages_fit(&book, &style, width, height, hyphenate, VARIED);
            positions_survive(&book, &style, width, height, hyphenate, VARIED, &picks);
        }
    }

    #[test]
    fn kerned_lines_fit_as_drawn_and_positions_survive(
        book in book(),
        style in style(),
        widths in (3.0f32..60.0, 3.0f32..60.0),
        lines in 1.0f32..15.0,
        hyphenate in any::<bool>(),
        picks in proptest::collection::vec((any::<usize>(), any::<usize>()), 1..8),
    ) {
        let height = lines * 10.0 * MAX_SCALE;
        for width in [widths.0, widths.1] {
            pages_fit(&book, &style, width, height, hyphenate, KERNED);
            positions_survive(&book, &style, width, height, hyphenate, KERNED, &picks);
        }
    }
}

fn one_paragraph(text: &str, align: Align) -> Book {
    Book {
        sections: vec![vec![Para {
            text: String::from(text),
            kind: BlockKind::Paragraph,
            runs: Vec::new(),
            align,
            size: None,
            indent: None,
            margins: (0, 0),
        }]],
        metadata: Metadata::default(),
    }
}

const PLAIN: Style = Style::new(0.0, 0.0, 10.0, false);

fn shown(pages: &[Page]) -> Vec<Vec<&str>> {
    pages
        .iter()
        .map(|p| p.lines.iter().map(|l| l.text.as_str()).collect())
        .collect()
}

/// Glyphs wider than the box get a line each, justified or not, and the
/// book still ends.
#[test]
fn glyphs_wider_than_the_box_get_a_line_each() {
    let book = one_paragraph("一二 三", Align::Justify);
    let mut style = PLAIN;
    style.justify = true;
    let cx = context(&book, 1.0, &style, true, &VARIED);
    let pages = paginate(&cx, 100.0, &book);
    assert_eq!(shown(&pages), [["一", "二", "三"]]);
}

/// A page lower than a single line still shows one line and moves on.
#[test]
fn pages_lower_than_a_line_still_move_on() {
    let book = one_paragraph("aa bb cc", Align::Auto);
    let cx = context(&book, 3.0, &PLAIN, false, &EVEN);
    let pages = paginate(&cx, 1.0, &book);
    assert_eq!(shown(&pages), [["aa"], ["bb"], ["cc"]]);
}

/// The shrunk case of `lines_of_varied_glyphs_fit_and_positions_survive`:
/// after "d " the bold CJK word moves to the next line and keeps its
/// seventh character, 16.8 wide in a box of 16.25.
#[test]
fn a_word_moved_to_the_next_line_still_fits_it() {
    let mut book = one_paragraph("aaaaa-aaa\u{ad}äcaaadqqad 一一一一一一一", Align::Auto);
    book.sections[0][0].runs = vec![Run::new(23, 44, TextStyle::BOLD)];
    let cx = context(&book, 16.254_377, &PLAIN, false, &VARIED);
    let pages = paginate(&cx, 100.0, &book);
    for line in &pages[0].lines {
        let right = line.x + line_width(line, VARIED);
        assert!(right <= cx.width, "{:?} ends at {right}", line.text);
    }
}

/// A book's negative indent (a hanging first line) hangs into the book's
/// margin, never out of the box: the first line is wider than the
/// others; what does not fit it is carried to the second line, which
/// must still fit the box. Here "aaaaaaaaaaa" (11) comes on a line of 10.
#[test]
fn lines_after_a_hanging_first_line_fit_the_box() {
    let mut book = one_paragraph("aaaa-aaaaaaaaaaa", Align::Auto);
    book.sections[0][0].indent = Some(-50);
    book.sections[0][0].margins = (50, 0);
    let cx = context(&book, 10.0, &PLAIN, false, &EVEN);
    let pages = paginate(&cx, 100.0, &book);
    assert_eq!(pages[0].lines[0].x, 0.0, "the first line hangs");
    assert!(
        pages[0].lines[1..].iter().all(|l| l.x > 0.0),
        "the others in the margin"
    );
    for line in &pages[0].lines[1..] {
        let right = line.x + line_width(line, EVEN);
        assert!(right <= cx.width, "{:?} ends at {right}", line.text);
    }
}
