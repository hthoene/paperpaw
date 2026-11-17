//! The type of book pages as the settings make it: the page's box and
//! spacing ([`PageType`]), the reading faces with the book's own fonts and
//! small capitals ([`ReadingFaces`], a layout
//! [`Typesetter`](crate::layout::Typesetter)), the boxes of a page's words
//! ([`word_boxes`]) and the page drawn ([`draw_lines`], [`draw_image`]).
//! A reader of one's own lays pages out and draws them as the shipped one
//! does; what it puts around them (status lines) is its own.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use super::WordBox;
use crate::builtin::{fonts, settings};
use crate::document::Locator;
use crate::layout::{
    BookStyles, Face, LineBidi, LineRun, Page, PageImage, PageLine, ScriptOptions, Style,
    Typesetter,
};
use crate::text::ToneCurve;
use crate::{Canvas, Color,  FontHandle, FontSet, Id, Point, Rect};

/// Relative sizes of headings 1 to 6.
const HEADING_SCALE: [f32; 6] = [1.6, 1.4, 1.2, 1.1, 1.0, 1.0];

/// Small capitals are this much of the text size.
const SMALL_CAPS: f32 = 0.78;
/// The reader's "Hanging punctuation" setting (the core plugin's), off
/// where it is not defined: punctuation at the lines' ends set out into
/// the margins ([`ScriptOptions::HANGING_PUNCTUATION`]).
const HANGING_PUNCTUATION: crate::setting::Setting<bool> =
    crate::setting::Setting::new("core.hanging-punctuation");

/// Superscript and subscript are this much of the text size, raised or
/// lowered by this much of it.
const SCRIPT: f32 = 0.7;
const RAISED: f32 = 0.33;
const LOWERED: f32 = 0.15;


/// The fonts a book embeds, loaded with the book.
pub struct EmbeddedFonts {
    set: FontSet,
    /// Family, bold, italic and index in `set` of each font.
    faces: Vec<(u8, bool, bool, usize)>,
}

impl core::fmt::Debug for EmbeddedFonts {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("EmbeddedFonts")
            .field("faces", &self.faces.len())
            .finish_non_exhaustive()
    }
}

impl EmbeddedFonts {
    /// The face of `family` closest to `bold` and `italic`.
    pub(crate) fn face(&self, family: u8, bold: bool, italic: bool) -> Option<FontHandle<'_>> {
        let of_family = || self.faces.iter().filter(|f| f.0 == family);
        let exact = of_family().find(|f| f.1 == bold && f.2 == italic);
        let regular = || of_family().find(|f| !f.1 && !f.2);
        exact
            .or_else(regular)
            .or_else(|| of_family().next())
            .and_then(|f| self.set.get(f.3))
    }
}

/// Room for status lines above and below a book's text: where they are
/// shown, how high they are and how far from the edge.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[non_exhaustive]
pub struct StatusRoom {
    pub top: bool,
    pub bottom: bool,
    /// A line's height, in pixels.
    pub height: f32,
    /// From the screen's edge, in pixels.
    pub edge: f32,
}

impl StatusRoom {
    /// No status lines.
    pub const NONE: StatusRoom = StatusRoom {
        top: false,
        bottom: false,
        height: 0.0,
        edge: 0.0,
    };

    /// Lines `height` high, `edge` from the screen's edges, at the top and
    /// the bottom as `top` and `bottom` say.
    pub fn new(top: bool, bottom: bool, height: f32, edge: f32) -> Self {
        Self {
            top,
            bottom,
            height,
            edge,
        }
    }
}

/// The page's box and the type of its text, from the standard settings
/// (see [`settings::FONT_SIZE`] and those after it) and the screen.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct PageType {
    /// The text's size, in pixels.
    pub size: f32,
    /// Font weight steps (0: as the font is).
    pub weight: u8,
    /// Spaces relative to the font's.
    pub word_spacing: f32,
    /// Space added after every letter, in pixels.
    pub letter_spacing: f32,
    /// Pairs of letters kerned ([`settings::KERNING`]).
    pub kerning: bool,
    /// Ligatures drawn ([`settings::LIGATURES`]); never with letter
    /// spacing, which would pull them apart.
    pub ligatures: bool,
    /// Body text lines: baseline from the top of a line, line height.
    pub ascent: f32,
    pub line_height: f32,
    pub style: Style,
    /// The content box.
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    /// The status lines above and below the content, where there are any.
    pub top_status: Option<Rect>,
    pub status: Option<Rect>,
    /// The rooms page layers reserve beside the content, and the columns the content is set in,
    /// left to right: the content alone without a frame.
    pub rooms: Vec<(Id, Rect)>,
    pub columns: Vec<Rect>,
    /// The language whose hyphenation patterns are used, if hyphenating.
    pub hyphenation: Option<String>,
    /// The fewest letters before and after a hyphen the reader asks for
    /// (0: the language's own).
    pub hyphen_minimums: (u8, u8),
    /// The script the book's language is written in
    /// ([`Lang::likely_script`](crate::Lang::likely_script)), which the tone curves
    /// per script (`core.text-curve`) look at; none for a language written
    /// in Latin letters or one not known.
    pub script: Option<&'static str>,
    /// What the plugins' text filters did when it was made: 0, there are
    /// none (kept in the page key as it was).
    pub filters: u64,
    /// Names this layout: what pages laid out with it look like depends
    /// on nothing else (the settings, the faces, the box, the filters, the
    /// plugins' stylesheets, the layout's version). A page index of the
    /// book has it in its name.
    pub key: String,
}


/// The options of the line breaker for a book in `language`: by word
/// (`KEEP_ALL`) only for a Korean book whose reader chose it. The bit is
/// never global: it would lay out Chinese and Japanese text badly. And
/// hanging punctuation where the reader chose it (`hanging`).
fn script_options(language: &str, korean: &str, hanging: bool) -> ScriptOptions {
    let primary = language.split(['-', '_']).next().unwrap_or("");
    let options = match primary.eq_ignore_ascii_case("ko") && korean == settings::KOREAN_BREAKS_WORD
    {
        true => ScriptOptions::KEEP_ALL,
        false => ScriptOptions::NONE,
    };
    match hanging {
        true => options.with(ScriptOptions::HANGING_PUNCTUATION),
        false => options,
    }
}


/// The reading fonts by face, for layout and drawing: the faces bound to
/// the reading roles (a font the user chose for book text is not mixed
/// with the default bold or italic), the book's own where it uses them,
/// headings larger, small capitals, the letters as smooth and as dark as
/// the settings say.
#[derive(Clone, Copy)]
pub struct ReadingFaces<'a> {
    /// Regular, bold, italic and bold italic.
    fonts: [FontHandle<'a>; 4],
    weight: u8,
    /// The contrast curve of the letters' edges.
    tone: ToneCurve,
    /// Letters black and white, without grays.
    sharp: bool,
    word_spacing: f32,
    letter_spacing: f32,
    /// Pairs kerned and ligatures drawn, as the page's type says.
    kerning: bool,
    ligatures: bool,
    embedded: Option<&'a EmbeddedFonts>,
    size: f32,
    ascent: f32,
    line_height: f32,
}

impl core::fmt::Debug for ReadingFaces<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ReadingFaces")
            .field("size", &self.size)
            .finish_non_exhaustive()
    }
}

impl<'a> ReadingFaces<'a> {

    /// The book's font for text in an embedded family, else the reading
    /// face; kerned and with ligatures as the settings say.
    pub fn font(&self, face: Face) -> FontHandle<'a> {
        let style = face.style;
        let font = match style.family() {
            0 => None,
            family => self
                .embedded
                .and_then(|e| e.face(family, style.bold(), style.italic())),
        };
        let font = font.unwrap_or(self.fonts[(style.bits() & 3) as usize]);
        let font = if self.kerning { font } else { font.unkerned() };
        // No ligatures in spaced letters: they are drawn one by one.
        if self.ligatures && style.letter_spacing() == 0 {
            font.ligatures()
        } else {
            font
        }
    }

    /// The size of the line's text in `face`, in pixels: what
    /// superscript and subscript are set smaller than.
    fn line_size(&self, face: Face) -> f32 {
        match (face.size, face.heading) {
            // The book's size wins.
            (percent, _) if percent > 0 => self.size * f32::from(percent) / 100.0,
            (_, 0) => self.size,
            (_, level) => self.size * HEADING_SCALE[usize::from(level.clamp(1, 6)) - 1],
        }
    }

    /// The size of text in `face`, in pixels.
    pub fn size(&self, face: Face) -> f32 {
        let size = self.line_size(face);
        match face.style.superscript() || face.style.subscript() {
            true => size * SCRIPT,
            false => size,
        }
    }

    /// How far text in `face` sits below the line's baseline, in pixels:
    /// raised above it (negative) for superscript, lowered for subscript.
    pub(crate) fn shift(&self, face: Face) -> i32 {
        let size = self.line_size(face);
        if face.style.superscript() {
            -(size * RAISED + 0.5) as i32
        } else if face.style.subscript() {
            (size * LOWERED + 0.5) as i32
        } else {
            0
        }
    }

    /// The space after each letter in `face`: the reader's letter
    /// spacing and the book's for the run ([`TextStyle::letter_spacing`],
    /// in steps of its size).
    ///
    /// [`TextStyle::letter_spacing`]: paperpaw_document::TextStyle::letter_spacing
    fn spacing(&self, face: Face) -> f32 {
        match face.style.letter_spacing() {
            0 => self.letter_spacing,
            steps => {
                let step = self.size(face) / f32::from(paperpaw_document::LETTER_SPACING_STEPS);
                self.letter_spacing + f32::from(steps) * step
            }
        }
    }

    /// The letter spacing of `text` in `face`: after each of its
    /// characters.
    fn letters(&self, face: Face, text: &str) -> f32 {
        match self.spacing(face) {
            0.0 => 0.0,
            spacing => spacing * text.chars().count() as f32,
        }
    }

    /// The width of `text` in `face`, small capitals included.
    pub(crate) fn measure(&self, face: Face, text: &str) -> f32 {
        let text = &*visible(text);
        let (font, size) = (self.font(face), self.size(face));
        let letters = self.letters(face, text);
        if !face.style.small_caps() {
            return font.measure(size, text) + letters;
        }
        small_caps_parts(text)
            .map(|(part, lower)| match lower {
                true => font.measure(size * SMALL_CAPS, &part.to_uppercase()),
                false => font.measure(size, part),
            })
            .sum::<f32>()
            + letters
    }

    /// The lines under, over and through text in `face` from `x`, `width`
    /// wide, on the line's `baseline`, in `ink`, drawn as its line style
    /// says (one, two, dotted or wavy).
    pub(crate) fn decorate(
        &self,
        canvas: &mut Canvas<'_>,
        face: Face,
        (x, width): (f32, f32),
        baseline: i32,
        ink: Color,
    ) {
        let style = face.style;
        let (under, over, through) = (style.underline(), style.overline(), style.strike());
        if !(under || over || through) || width <= 0.0 {
            return;
        }
        let size = self.line_size(face);
        let thick = ((size / 16.0) as u32).max(1);
        let (left, right) = ((x + 0.5) as i32, (x + width + 0.5) as i32);
        let length = (right - left).max(0) as u32;
        let line = style.line_style();
        let mut draw = |y: i32| decoration(canvas, line, (left, length), y, thick, ink);
        if under {
            draw(baseline + ((size / 10.0) as i32).max(1));
        }
        if over {
            // Above the capitals, below the line's top.
            draw(baseline - (size * 0.78) as i32);
        }
        if through {
            draw(baseline - (size * 0.28) as i32);
        }
    }

    /// A space between words in `face`.
    pub(crate) fn space(&self, face: Face) -> f32 {
        self.font(face).advance(self.size(face), ' ') * self.word_spacing
    }

    /// Draws `text` in `face`, as heavy as the font weight setting asks;
    /// its width.
    pub fn draw(
        &self,
        canvas: &mut Canvas<'_>,
        face: Face,
        x: f32,
        baseline: i32,
        text: &str,
    ) -> f32 {
        self.draw_in(canvas, face, x, baseline, text, Color::BLACK)
    }

    /// Draws `text` as [`ReadingFaces::draw`] does, in `ink` (e.g. a
    /// mark's).
    pub(crate) fn draw_in(
        &self,
        canvas: &mut Canvas<'_>,
        face: Face,
        x: f32,
        baseline: i32,
        text: &str,
        ink: Color,
    ) -> f32 {
        let text = &*visible(text);
        let size = self.size(face);
        let (extra, tone) = self.inking(canvas, size);
        let font = self.font(face);
        let baseline = baseline + self.shift(face);
        let spacing = self.spacing(face);
        // Letter by letter where letters are spaced, each pair kerned as
        // measuring kerns it; at once otherwise.
        let draw = |canvas: &mut Canvas<'_>, size: f32, x: f32, text: &str| -> f32 {
            if spacing == 0.0 {
                return font.draw_toned(canvas, size, x, baseline, text, ink, extra, tone);
            }
            let mut width = 0.0;
            let mut buf = [0u8; 4];
            let mut before = None;
            for c in text.chars() {
                if let Some(left) = before {
                    width += font.kern(size, left, c);
                }
                before = Some(c);
                let letter = c.encode_utf8(&mut buf);
                width +=
                    font.draw_toned(canvas, size, x + width, baseline, letter, ink, extra, tone);
                width += spacing;
            }
            width
        };
        if !face.style.small_caps() {
            return draw(canvas, size, x, text);
        }
        // Lower case letters as smaller capitals.
        let mut width = 0.0;
        for (part, lower) in small_caps_parts(text) {
            width += match lower {
                true => draw(canvas, size * SMALL_CAPS, x + width, &part.to_uppercase()),
                false => draw(canvas, size, x + width, part),
            };
        }
        width
    }
}

impl ReadingFaces<'_> {
    /// The width of `text` in `face` as [`ReadingFaces::draw_around`]
    /// sets it.
    pub(crate) fn measure_around(
        &self,
        face: Face,
        text: &str,
        around: &crate::text::Around,
    ) -> f32 {
        match shaped(text, around) && !face.style.small_caps() {
            true => self.font(face).measure_in(self.size(face), text, around),
            false => self.measure(face, text),
        }
    }

    /// Draws `text` in `face` and `ink` as what lies `around` it asks:
    /// Hebrew and Arabic in the order shown, letters joined, without
    /// letter spacing ([`FontHandle::draw_in`]); other text as
    /// [`ReadingFaces::draw_in`] draws it. Its width.
    #[allow(
        clippy::too_many_arguments,
        reason = "its inputs are separate things; a struct of them would serve this call alone"
    )]
    pub(crate) fn draw_around(
        &self,
        canvas: &mut Canvas<'_>,
        face: Face,
        x: f32,
        baseline: i32,
        text: &str,
        ink: Color,
        around: &crate::text::Around,
    ) -> f32 {
        if !shaped(text, around) || face.style.small_caps() {
            return self.draw_in(canvas, face, x, baseline, text, ink);
        }
        let size = self.size(face);
        let (extra, tone) = self.inking(canvas, size);
        let baseline = baseline + self.shift(face);
        self.font(face)
            .draw_in(canvas, size, x, baseline, text, ink, (extra, tone), around)
    }

    /// How much bolder and in which tone text at `size` is drawn on
    /// `canvas`, as the settings ask.
    fn inking(&self, canvas: &Canvas<'_>, size: f32) -> (u16, ToneCurve) {
        // About a pixel per step at 48 px; at least one.
        let extra = match self.weight {
            0 => 0,
            w => (f32::from(w) * size / 48.0 + 0.5).max(1.0) as u16,
        };
        // Without grays from the coverage a frame of black and white draws
        // from: the letters it would draw.
        let tone = match self.sharp {
            true => self.tone.sharp(canvas.mono_coverage()),
            false => self.tone,
        };
        (extra, tone)
    }
}

/// Whether `text` set with `around` is shaped: right to left, or Hebrew
/// or Arabic in it.
/// `text` without the controls that draw nothing and take no room (an
/// EPUB's `dir`, marks of direction): drawn letter by letter or with
/// small capitals, they would be missing glyphs.
fn visible(text: &str) -> alloc::borrow::Cow<'_, str> {
    match text.chars().any(paperpaw_text::is_invisible) {
        true => text
            .chars()
            .filter(|&c| !paperpaw_text::is_invisible(c))
            .collect(),
        false => alloc::borrow::Cow::Borrowed(text),
    }
}

fn shaped(text: &str, around: &crate::text::Around) -> bool {
    around.base == crate::lang::Direction::Rtl
        || text.chars().any(|c| matches!(c, '\u{0590}'..='\u{08FF}'))
}

/// One line under, over or through text, `thick` pixels thick, its top
/// at `y`, from `left` and `length` long, drawn as `line`: a second line
/// as far below the first as it is thick (double), dots as long as the
/// line is thick with gaps as wide (dotted), or going up and down by its
/// thickness every two of them (wavy).
fn decoration(
    canvas: &mut Canvas<'_>,
    line: paperpaw_document::LineStyle,
    (left, length): (i32, u32),
    y: i32,
    thick: u32,
    ink: Color,
) {
    use paperpaw_document::LineStyle;
    match line {
        LineStyle::Double => {
            canvas.fill_rect(left, y, length, thick, ink);
            canvas.fill_rect(left, y + 2 * thick as i32, length, thick, ink);
        }
        LineStyle::Dotted | LineStyle::Wavy => {
            let step = thick.max(1) as usize * 2;
            for (i, at) in (0..length).step_by(step).enumerate() {
                let piece = match line {
                    LineStyle::Dotted => thick.min(length - at),
                    _ => (step as u32).min(length - at),
                };
                let up = match line {
                    LineStyle::Wavy if i % 2 == 1 => thick as i32,
                    _ => 0,
                };
                canvas.fill_rect(left + at as i32, y - up, piece, thick, ink);
            }
        }
        // Solid, and a style of a later build: one line.
        _ => canvas.fill_rect(left, y, length, thick, ink),
    }
}

impl Typesetter for ReadingFaces<'_> {
    fn advance(&self, face: Face, c: char) -> f32 {
        if c == ' ' {
            return self.space(face);
        }
        if face.style.small_caps() && c.is_lowercase() {
            let small = self.size(face) * SMALL_CAPS;
            let font = self.font(face);
            // Its capitals (one or more: `ß` is `SS`), kerned among them.
            let mut upper = 0.0;
            let mut before = None;
            for u in c.to_uppercase() {
                if let Some(left) = before {
                    upper += font.kern(small, left, u);
                }
                upper += font.advance(small, u);
                before = Some(u);
            }
            return upper + self.spacing(face);
        }
        let advance = self.font(face).advance(self.size(face), c);
        // Most books space no letters: no float to add for each one. An
        // advance is never -0.0, the one value adding 0.0 changes.
        match self.spacing(face) {
            spacing if spacing.to_bits() == 0 => advance,
            spacing => advance + spacing,
        }
    }

    /// The pair as drawing sets it ([`FontHandle::kern`]): in small
    /// capitals, lower case letters as smaller capitals and no pair
    /// between those and the rest, which are drawn apart.
    fn kern(&self, face: Face, left: char, right: char) -> f32 {
        if left == ' ' || right == ' ' {
            return 0.0;
        }
        let font = self.font(face);
        if face.style.small_caps() && (left.is_lowercase() || right.is_lowercase()) {
            if left.is_lowercase() != right.is_lowercase() {
                return 0.0;
            }
            let (Some(last), Some(first)) =
                (left.to_uppercase().last(), right.to_uppercase().next())
            else {
                return 0.0;
            };
            return font.kern(self.size(face) * SMALL_CAPS, last, first);
        }
        font.kern(self.size(face), left, right)
    }

    /// Text that joins or runs right to left as wide as drawn
    /// ([`FontHandle::measure_in`]), without letter spacing (it would part
    /// joined letters); other text as its characters and pairs.
    fn advance_text(&self, face: Face, text: &str, around: &crate::text::Around) -> f32 {
        if shaped(text, around) && !face.style.small_caps() {
            return self.font(face).measure_in(self.size(face), text, around);
        }
        let mut width = 0.0;
        let mut before = None;
        for c in text.chars() {
            if let Some(left) = before {
                width += self.kern(face, left, c);
            }
            width += self.advance(face, c);
            before = Some(c);
        }
        width
    }

    fn line(&self, face: Face) -> (f32, f32) {
        if face.heading == 0 {
            // Body text the book sets larger or smaller keeps the line
            // spacing chosen.
            let scale = match face.size {
                0 => 1.0,
                percent => f32::from(percent) / 100.0,
            };
            return (self.ascent * scale, self.line_height * scale);
        }
        let size = self.size(face);
        let metrics = self.font(face).line_metrics(size);
        let natural = metrics.ascent + metrics.descent;
        let line_height = (size * 1.2).max(natural);
        (metrics.ascent + (line_height - natural) / 2.0, line_height)
    }
}

/// `text` in parts of lower case letters and of the rest, each with
/// whether it is lower case.
pub fn small_caps_parts(text: &str) -> impl Iterator<Item = (&str, bool)> {
    let mut rest = text;
    core::iter::from_fn(move || {
        let first = rest.chars().next()?;
        let lower = first.is_lowercase();
        let end = rest
            .char_indices()
            .find(|(_, c)| c.is_lowercase() != lower)
            .map_or(rest.len(), |(i, _)| i);
        let (part, tail) = rest.split_at(end);
        rest = tail;
        Some((part, lower))
    })
}

/// The words of `page`, laid out with `faces` in `page_type`, with their
/// boxes on the screen, in reading order (measured as they are drawn: a
/// word that goes on in another face is one word).
pub fn word_boxes(faces: &ReadingFaces<'_>, page_type: &PageType, page: &Page) -> Vec<WordBox> {
    let t = page_type;
    let mut words: Vec<WordBox> = Vec::new();
    // Lines a filter inserted have no places: no words of the book.
    for line in page.lines.iter().filter(|l| !l.inserted) {
        if line.bidi.has(LineBidi::REORDER) {
            words.extend(reordered_boxes(faces, page_type, line));
            continue;
        }
        let mut x = t.x + line.x;
        // Inside a word that continues in the next run (e.g. bold).
        let mut joined = false;
        // The next of the line's words.
        let mut ordinal = 0;
        for run in &line.runs {
            let (ascent, height) = faces.line(run.face);
            let top = t.y + line.baseline - ascent;
            let space = faces.space(run.face) + line.word_spacing;
            for (i, part) in line.text[run.start..run.end].split(' ').enumerate() {
                if i > 0 {
                    x += space;
                    joined = false;
                }
                if part.is_empty() {
                    continue;
                }
                let width = faces.measure(run.face, part);
                match words.last_mut() {
                    Some(last) if joined => {
                        last.text.push_str(part);
                        last.area.width = (x + width) as u16 - last.area.x;
                    }
                    _ => {
                        let (start, end) = line.words.get(ordinal).copied().unwrap_or_default();
                        ordinal += 1;
                        words.push(
                            WordBox::new(
                                Rect::new(x as u16, top as u16, width as u16, height as u16),
                                part,
                                Locator::new(line.section, start),
                                Locator::new(line.section, end),
                            )
                            .bidi(line.bidi),
                        );
                    }
                }
                x += width;
                joined = true;
            }
        }
    }
    words
}

/// The words of `line`, whose runs are put in order ([`word_boxes`]):
/// in reading order, each where it is drawn, a word in two runs one box
/// around both.
fn reordered_boxes(
    faces: &ReadingFaces<'_>,
    page_type: &PageType,
    line: &PageLine,
) -> Vec<WordBox> {
    let t = page_type;
    let mut boxes: Vec<(usize, core::ops::Range<usize>, WordBox)> = Vec::new();
    for piece in super::visual::pieces(faces, line) {
        let Some(run) = line.runs.get(piece.run).filter(|_| !piece.space) else {
            continue;
        };
        let (ascent, height) = faces.line(run.face);
        let top = t.y + line.baseline - ascent;
        let x = t.x + line.x + piece.x;
        let area = Rect::new(x as u16, top as u16, piece.width as u16, height as u16);
        match boxes
            .iter_mut()
            .find(|(ordinal, _, _)| *ordinal == piece.ordinal)
        {
            Some((_, range, word)) => {
                word.area = word.area.union(&area);
                *range = range.start.min(piece.range.start)..range.end.max(piece.range.end);
            }
            None => {
                let (start, end) = line.words.get(piece.ordinal).copied().unwrap_or_default();
                let word = WordBox::new(
                    area,
                    "",
                    Locator::new(line.section, start),
                    Locator::new(line.section, end),
                )
                .rtl(run.is_rtl())
                .bidi(line.bidi);
                boxes.push((piece.ordinal, piece.range, word));
            }
        }
    }
    paperpaw_sort::sort_by_key(&mut boxes, |(ordinal, _, _)| *ordinal);
    // Without the controls that set text apart (an EPUB's `dir`), which
    // draw nothing: a word of them alone is none.
    boxes
        .into_iter()
        .filter_map(|(_, range, mut word)| {
            let text = line.text.get(range).unwrap_or_default();
            word.text = text
                .chars()
                .filter(|&c| !paperpaw_text::is_invisible(c))
                .collect();
            (!word.text.is_empty()).then_some(word)
        })
        .collect()
}


/// What a value of [`settings::TEXT_SHARPNESS`] does to book text: the
/// steps it moves the contrast by (soft -1, sharp +1) and whether it
/// draws without grays (sharp). Normal, and a value it does not know,
/// change nothing.
fn sharpness(level: &str) -> (i8, bool) {
    match settings::TEXT_SHARPNESS_LEVELS
        .iter()
        .position(|l| *l == level)
    {
        Some(0) => (-1, false),
        Some(2) => (1, true),
        _ => (0, false),
    }
}

/// Draws `lines` laid out with `faces` in `page_type`, their content box
/// at `origin`, each word in its ink (`inks`, by the words' order as
/// [`word_boxes`] has them; black where none). Lines outside the part
/// being drawn (another band) are only counted.
pub fn draw_lines(
    canvas: &mut Canvas<'_>,
    faces: &ReadingFaces<'_>,
    page_type: &PageType,
    (left, top): (f32, f32),
    lines: &[PageLine],
    inks: &[Option<Color>],
) {
    let clip = canvas.clip();
    // The words counted as `word_boxes` counts them: a word that goes on
    // in the next run (e.g. bold) is one word.
    let mut ordinal = 0usize;
    for line in lines {
        // Words of lines a filter inserted are not counted (they are not
        // the book's): drawn in black.
        let ink_of = |i: usize| match line.inserted {
            true => None,
            false => inks.get(i).copied().flatten(),
        };
        let baseline = (top + line.baseline + 0.5) as i32;
        // Letters reach up to two lines, or as high as the line's largest
        // text (a drop capital spans several lines above its baseline).
        let tallest = line
            .runs
            .iter()
            .map(|run| faces.size(run.face))
            .fold(0.0f32, f32::max);
        let above = baseline - (2.0 * page_type.line_height).max(1.2 * tallest) as i32;
        let below = baseline + page_type.line_height as i32;
        let shown = below >= i32::from(clip.y) && above <= clip.bottom() as i32;
        if !shown {
            if !line.inserted {
                ordinal += words_in(&line.text, &line.runs);
            }
            continue;
        }
        if line.bidi.has(LineBidi::REORDER) {
            for piece in super::visual::pieces(faces, line) {
                let Some(run) = line.runs.get(piece.run) else {
                    continue;
                };
                let x = left + line.x + piece.x;
                let ink = ink_of(ordinal + piece.ordinal).unwrap_or(Color::BLACK);
                if piece.space {
                    // Lines under and through words go on under the space
                    // between them, in the first word's ink.
                    if piece.decorated {
                        faces.decorate(canvas, run.face, (x, piece.width), baseline, ink);
                    }
                    continue;
                }
                let word = line.text.get(piece.range.clone()).unwrap_or_default();
                let around = super::visual::around(line, piece.run, &piece.range);
                let width = faces.draw_around(canvas, run.face, x, baseline, word, ink, &around);
                faces.decorate(canvas, run.face, (x, width), baseline, ink);
            }
            if !line.inserted {
                ordinal += words_in(&line.text, &line.runs);
            }
            continue;
        }
        let mut x = left + line.x;
        let mut joined = false;
        for run in &line.runs {
            let text = &line.text[run.start..run.end];
            let space = faces.space(run.face) + line.word_spacing;
            for (i, word) in text.split(' ').enumerate() {
                if i > 0 {
                    // Lines under and through words go on under the space
                    // between them, in the first word's ink.
                    if !word.is_empty() {
                        let ink = ink_of(ordinal.saturating_sub(1));
                        let ink = ink.unwrap_or(Color::BLACK);
                        faces.decorate(canvas, run.face, (x, space), baseline, ink);
                    }
                    x += space;
                    joined = false;
                }
                if word.is_empty() {
                    continue;
                }
                if !joined && !line.inserted {
                    ordinal += 1;
                }
                joined = true;
                let ink = ink_of(ordinal.saturating_sub(1));
                let width = match ink {
                    Some(ink) => faces.draw_in(canvas, run.face, x, baseline, word, ink),
                    None => faces.draw(canvas, run.face, x, baseline, word),
                };
                let ink = ink.unwrap_or(Color::BLACK);
                faces.decorate(canvas, run.face, (x, width), baseline, ink);
                x += width;
            }
        }
    }
}

/// How many words `runs` of `text` hold, counted as [`word_boxes`] and
/// [`draw_lines`] count them: a word that goes on in the next run is one.
fn words_in(text: &str, runs: &[LineRun]) -> usize {
    let mut count = 0;
    let mut joined = false;
    for run in runs {
        for (i, word) in text[run.start..run.end].split(' ').enumerate() {
            if i > 0 {
                joined = false;
            }
            if word.is_empty() {
                continue;
            }
            if !joined {
                count += 1;
            }
            joined = true;
        }
    }
    count
}



#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    #[test]
    fn small_caps_split_lower_case() {
        let parts: Vec<(&str, bool)> = small_caps_parts("Mr. Gregor Samsa").collect();
        assert_eq!(
            parts,
            [
                ("M", false),
                ("r", true),
                (". G", false),
                ("regor", true),
                (" S", false),
                ("amsa", true)
            ]
        );
        assert_eq!(small_caps_parts("").count(), 0);
        assert_eq!(
            small_caps_parts("straße").collect::<Vec<_>>(),
            [("straße", true)]
        );
    }

    #[test]
    fn words_are_counted_across_runs() {
        let run = |start, end| LineRun::new(start, end, Face::default());
        // "a bo|ld c": "bold" goes on in the next run, one word.
        assert_eq!(words_in("a bold c", &[run(0, 4), run(4, 8)]), 3);
        // A run that starts with a space starts a new word.
        assert_eq!(words_in("a b", &[run(0, 1), run(1, 3)]), 2);
        // Spaces alone and runs of nothing hold no words.
        assert_eq!(words_in("  x  ", &[run(0, 0), run(0, 5)]), 1);
        assert_eq!(words_in("", &[]), 0);
    }

    #[test]
    fn words_are_looked_up_without_punctuation() {
        let word = |text| WordBox::new(Rect::new(0, 0, 1, 1), text, Locator::START, Locator::START);
        let words = ["\"Rabbit,\"", "(dear)", "well-known"].map(word);
        let looked: Vec<&str> = words.iter().map(WordBox::lookup_text).collect();
        assert_eq!(looked, ["Rabbit", "dear", "well-known"]);
    }
}
