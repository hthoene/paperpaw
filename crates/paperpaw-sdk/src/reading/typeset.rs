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

use paperpaw_hyphen::Patterns;

use super::WordBox;
use crate::builtin::{fonts, settings};
use crate::document::{Document, DocumentError, Locator};
use crate::layout::{
    BookStyles, Face, LineBidi, LineRun, Page, PageImage, PageLine, Style, Typesetter,
};
use crate::text::ToneCurve;
use crate::{Canvas, Color, Env, FontHandle, FontSet, Id, Point, Rect};

/// Relative sizes of headings 1 to 6.
const HEADING_SCALE: [f32; 6] = [1.6, 1.4, 1.2, 1.1, 1.0, 1.0];

/// Glyphs of a book's embedded fonts kept at most.
const EMBEDDED_CACHE: usize = 256 * 1024;

/// Largest total size of a book's embedded fonts loaded.
const EMBEDDED_LIMIT: usize = 8 * 1024 * 1024;

/// Small capitals are this much of the text size.
const SMALL_CAPS: f32 = 0.78;

/// Superscript and subscript are this much of the text size, raised or
/// lowered by this much of it.
const SCRIPT: f32 = 0.7;
const RAISED: f32 = 0.33;
const LOWERED: f32 = 0.15;

/// Whether a book's own fonts are used: the font chosen is the book's
/// (the default choice), and they render on this device (not on small
/// devices: they take hundreds of kilobytes).
pub fn uses_book_fonts(env: &Env<'_>) -> bool {
    crate::text::OUTLINES
        && env.device().memory != crate::MemoryClass::Small
        && env.settings().read(&settings::READING_FONT) == settings::DEFAULT_FONT
}

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
    /// The book's fonts, if they are used ([`uses_book_fonts`]) and it
    /// has any that load.
    pub fn load(env: &Env<'_>, document: &dyn Document) -> Option<EmbeddedFonts> {
        if !uses_book_fonts(env) {
            return None;
        }
        let fonts = document.fonts(env.storage());
        if fonts.is_empty() {
            return None;
        }
        let mut set = FontSet::new(EMBEDDED_CACHE);
        let mut faces = Vec::new();
        let mut total = 0;
        for (index, font) in fonts.iter().enumerate() {
            let Ok(data) = document.font_data(env.storage(), index) else {
                continue;
            };
            total += data.len();
            if total > EMBEDDED_LIMIT {
                break;
            }
            let Ok(loaded) = crate::Font::from_data(data, &[]) else {
                continue;
            };
            let id = Id::parse(&format!("core.book-font-{index}")).ok()?;
            let at = set.add(id, loaded);
            faces.push((font.family, font.bold, font.italic, at));
        }
        (!faces.is_empty()).then_some(EmbeddedFonts { set, faces })
    }

    /// The face of `family` closest to `bold` and `italic`.
    pub fn face(&self, family: u8, bold: bool, italic: bool) -> Option<FontHandle<'_>> {
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
    /// The rooms page layers reserve beside the content (see
    /// [`layers`](crate::layers)), and the columns the content is set in,
    /// left to right: the content alone without a frame.
    pub rooms: Vec<(Id, Rect)>,
    pub columns: Vec<Rect>,
    /// The language whose hyphenation patterns are used, if hyphenating.
    pub hyphenation: Option<String>,
    /// The fewest letters before and after a hyphen the reader asks for
    /// (0: the language's own), see [`PageType::patterns`].
    pub hyphen_minimums: (u8, u8),
    /// What the plugins' text filters did when it was made
    /// ([`TextFilters::signature`](crate::filters::TextFilters)).
    pub filters: u64,
    /// Names this layout: what pages laid out with it look like depends
    /// on nothing else (the settings, the faces, the box, the filters, the
    /// plugins' stylesheets, the layout's version). A page index of the
    /// book has it in its name.
    pub key: String,
}

impl PageType {
    /// The type of a book in `language` read in `font` (the reading font),
    /// with `status` for the status lines; a book of `whole_pages` (comics)
    /// is shown edge to edge, without margins or status lines.
    #[expect(
        clippy::too_many_lines,
        reason = "longer than the limit from before it was set; split it when it is next changed"
    )]
    pub fn new(
        env: &Env<'_>,
        font: &FontHandle<'_>,
        language: &str,
        whole_pages: bool,
        status: StatusRoom,
    ) -> Self {
        let s = env.settings();
        let m = env.metrics();
        let size = m.pt(s.read(&settings::FONT_SIZE) as f32);
        let weight = s.read(&settings::FONT_WEIGHT).clamp(0, 4) as u8;
        let word_spacing = s.read(&settings::WORD_SPACING).clamp(50, 200) as f32 / 100.0;
        let metrics = font.line_metrics(size);
        let natural = metrics.ascent + metrics.descent;
        // At least the font's own distance; below 100 % that much of it.
        let spacing = s.read(&settings::LINE_SPACING).clamp(80, 250) as f32 / 100.0;
        let line_height = (size * spacing).max(natural * spacing.min(1.0));
        let indent = s.read(&settings::INDENT);
        let indent_size = s.read(&settings::INDENT_SIZE).clamp(50, 400) as f32 / 100.0;
        let spaced = s.read(&settings::PARAGRAPH_SPACING);
        let paragraph_space = s.read(&settings::PARAGRAPH_SPACE).clamp(20, 150) as f32 / 100.0;
        let balanced = s.read(&settings::LINE_BREAKING) != settings::LINE_BREAKING_FAST;
        let letter_spacing = s.read(&settings::LETTER_SPACING).clamp(-5, 20) as f32 / 100.0 * size;
        let kerning = s.read(&settings::KERNING);
        let ligatures = s.read(&settings::LIGATURES) && letter_spacing == 0.0;
        let book = [
            (&settings::BOOK_ALIGN, BookStyles::ALIGN),
            (&settings::BOOK_INDENT, BookStyles::INDENT),
            (&settings::BOOK_SIZES, BookStyles::SIZE),
            (&settings::BOOK_SPACING, BookStyles::SPACING),
            (&settings::BOOK_FONTS, BookStyles::FONTS),
            (&settings::BOOK_MARGINS, BookStyles::MARGINS),
            (&settings::BOOK_LINE_HEIGHT, BookStyles::LINE_HEIGHT),
        ]
        .iter()
        .filter(|(setting, _)| s.read(setting))
        .fold(BookStyles::NONE, |all, (_, style)| all.with(*style));
        let justify = s.read(&settings::JUSTIFY);
        let ascent = metrics.ascent + (line_height - natural) / 2.0;
        let widows = s.read(&settings::WIDOWS).clamp(1, 4) as u8;
        let orphans = s.read(&settings::ORPHANS).clamp(1, 4) as u8;
        let style = Style::new(
            if spaced {
                line_height * paragraph_space
            } else {
                0.0
            },
            if indent { size * indent_size } else { 0.0 },
            size,
            justify,
        )
        .lines_kept(widows, orphans)
        .book(book)
        .balanced(balanced);

        let display = env.device().display;
        let millimetres = |setting: &crate::setting::Setting<i64>| match whole_pages {
            true => 0.0,
            false => m.mm(s.read(setting).max(0) as u32) as f32,
        };
        // At the sides, and at the top and bottom.
        let (margin, ends) = (
            millimetres(&settings::MARGIN),
            millimetres(&settings::MARGIN_ENDS),
        );
        let status = if whole_pages {
            StatusRoom::NONE
        } else {
            status
        };
        let width = (f32::from(display.width) - 2.0 * margin).max(1.0);
        // Status lines sit close to the edge, in the text's margin; the
        // text keeps its margin, and never runs up to a status line, also
        // on pages it fills to the last pixel.
        let status_room = status.height + m.mm(2) as f32;
        let screen = f32::from(display.height);
        let y = if status.top {
            ends.max(status.edge + status_room)
        } else {
            ends
        };
        let end = if status.bottom {
            (screen - ends).min(screen - status.edge - status_room)
        } else {
            screen - ends
        };
        let mut height = (end - y).max(1.0);
        let (mut x, mut y, mut width) = (margin, y, width);
        // The layers' rooms, then the frame's columns; for whole pages
        // neither.
        let content = Rect::new(x as u16, y as u16, width as u16, height as u16);
        // Asked only where there are layers or frames: nothing is made
        // for each book's type otherwise (the X4's heap).
        let (rooms, columns, layers) = match whole_pages || env.page_layers().is_idle() {
            true => (Vec::new(), alloc::vec![content], None),
            false => {
                let (rooms, text) = env.page_layers().rooms(env, content);
                if !rooms.is_empty() {
                    (x, y) = (f32::from(text.x), f32::from(text.y));
                    (width, height) = (f32::from(text.width), f32::from(text.height));
                }
                let columns = env.page_layers().columns(env, text);
                // Laid out as before, with the key as before, where they
                // change nothing.
                let changed = !rooms.is_empty() || columns != [text];
                let key = changed.then(|| env.page_layers().key(env, content));
                (rooms, columns, key)
            }
        };
        let line = |at: f32| Rect::new(content.x, at as u16, content.width, status.height as u16);
        let top_status = status.top.then(|| line(status.edge));
        let bottom_status = status
            .bottom
            .then(|| line(screen - status.edge - status.height));
        let hyphenation = (s.read(&settings::HYPHENATE)
            && env.hyphenation().get(language).is_some())
        .then(|| language.to_string());
        let hyphen_minimums = (
            s.read(&settings::HYPHEN_MIN_LEFT).clamp(0, 5) as u8,
            s.read(&settings::HYPHEN_MIN_RIGHT).clamp(0, 5) as u8,
        );
        // The language and, where the reader changed them, the minimums:
        // pages laid out with the language's own keep their key.
        let hyphenation_key = match (&hyphenation, hyphen_minimums) {
            (None, _) => String::from("-"),
            (Some(language), (0, 0)) => language.clone(),
            (Some(language), (left, right)) => format!("{language}:{left}:{right}"),
        };
        // The plugins' stylesheets change what the book's text looks like.
        let stylesheets = env.stylesheets().iter().fold(0u64, |hash, sheet| {
            hash.rotate_left(5) ^ paperpaw_digest::fnv1a64(sheet.as_bytes())
        });
        let key = format!(
            "{:016x}",
            paperpaw_digest::fnv1a64(format!(
                "{}|{}|{}|{}|{}|{size}|{line_height}|{}|{}|{justify}|{width}x{height}|{}|{}|{}|{word_spacing}|{}|{whole_pages}|{stylesheets:x}|{widows}:{orphans}|{:x}|{letter_spacing}|{balanced}|{kerning}|{ligatures}|{:x}:{:x}:{:?}",
                crate::layout::VERSION,
                font.id(),
                face_id(env, &fonts::READING_BOLD),
                face_id(env, &fonts::READING_ITALIC),
                face_id(env, &fonts::READING_BOLD_ITALIC),
                style.indent,
                style.paragraph_spacing,
                hyphenation_key,
                s.read(&settings::PUBLISHER_STYLES),
                uses_book_fonts(env),
                env.text_filters().signature(env),
                book.bits(),
                // Options of later layouts: in the key before any is used.
                style.script_options.bits(),
                style.options.bits(),
                style.values,
            ))
        );
        // Pages without layers' rooms or a frame keep their key (and their
        // page indexes).
        let key = match layers {
            None => key,
            Some(layers) => format!("{key}-{:016x}", paperpaw_digest::fnv1a64(&layers)),
        };
        Self {
            size,
            weight,
            word_spacing,
            letter_spacing,
            kerning,
            ligatures,
            ascent,
            line_height,
            style,
            x,
            y,
            width,
            height,
            top_status,
            status: bottom_status,
            rooms,
            columns,
            hyphenation,
            hyphen_minimums,
            filters: env.text_filters().signature(env),
            key,
        }
    }

    /// The patterns words are hyphenated with: those of the page's
    /// language, with the minimums the reader set where they are not 0;
    /// `None` where the page is not hyphenated.
    pub fn patterns(&self, env: &Env<'_>) -> Option<Patterns<'static>> {
        let language = self.hyphenation.as_deref()?;
        let (left, right) = self.hyphen_minimums;
        env.hyphenation()
            .get(language)
            .map(|patterns| patterns.with_minimums(left.into(), right.into()))
    }

    /// The type of a sample of text `height` pixels high in `language`
    /// (a preview of the settings): as wide as book pages and with their
    /// margins at the sides, `pad` above and below, without status lines.
    pub fn sample(
        env: &Env<'_>,
        font: &FontHandle<'_>,
        language: &str,
        height: f32,
        pad: f32,
    ) -> Self {
        let page = Self::new(env, font, language, false, StatusRoom::NONE);
        let height = (height - 2.0 * pad).max(1.0);
        let text = Rect::new(page.x as u16, pad as u16, page.width as u16, height as u16);
        // In the frame chosen, as book pages are.
        let columns = match env.page_layers().has_frames() {
            true => env.page_layers().columns(env, text),
            false => alloc::vec![text],
        };
        Self {
            y: pad,
            height,
            rooms: Vec::new(),
            columns,
            ..page
        }
    }

    /// The text's box, in whole pixels.
    pub fn text_box(&self) -> Rect {
        Rect::new(
            self.x as u16,
            self.y as u16,
            self.width as u16,
            self.height as u16,
        )
    }

    /// Whether the text is set in columns other than its box (a frame,
    /// see [`layers`](crate::layers)).
    pub fn in_columns(&self) -> bool {
        self.columns != [self.text_box()]
    }

    /// The page from `at` as `lay` lays out a column (its width and
    /// height, where it begins): the text's box in one, or column by
    /// column, each taking up where the one before ends, its lines and
    /// pictures moved to its place (one page for all).
    pub fn lay_out(
        &self,
        at: Locator,
        mut lay: impl FnMut(f32, f32, Locator) -> Result<Page, DocumentError>,
    ) -> Result<Page, DocumentError> {
        if !self.in_columns() {
            return lay(self.width, self.height, at);
        }
        let mut page = Page::new(at, at, Vec::new(), Vec::new());
        let mut next = at;
        for (i, column) in self.columns.iter().enumerate() {
            let mut laid = lay(f32::from(column.width), f32::from(column.height), next)?;
            if laid.is_past_end() {
                break;
            }
            let (dx, dy) = (f32::from(column.x) - self.x, f32::from(column.y) - self.y);
            for line in &mut laid.lines {
                line.x += dx;
                line.baseline += dy;
            }
            for image in &mut laid.images {
                image.x += dx;
                image.y += dy;
            }
            if i == 0 {
                page.start = laid.start;
            }
            page.end = laid.end;
            page.lines.append(&mut laid.lines);
            page.images.append(&mut laid.images);
            if laid.end <= next {
                break;
            }
            next = laid.end;
        }
        Ok(page)
    }

    /// The width lines are broken in where the start of a place's line is
    /// looked for: the text's, or its first column's.
    pub fn line_width(&self) -> f32 {
        match (self.in_columns(), self.columns.first()) {
            (true, Some(first)) => f32::from(first.width),
            _ => self.width,
        }
    }
}

/// The font bound to an optional reading face, for the layout key.
fn face_id(env: &Env<'_>, role: &Id) -> String {
    env.fonts()
        .bound(role.as_str())
        .map_or_else(String::new, |font| font.id().to_string())
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
    /// The faces for `page`; `None` without a reading font.
    pub fn new(
        env: &Env<'a>,
        page: &PageType,
        embedded: Option<&'a EmbeddedFonts>,
    ) -> Option<Self> {
        let all = env.fonts();
        let regular = all.role(fonts::READING.as_str())?;
        let custom = all.user_bound(fonts::READING.as_str());
        let face = |role: &Id| {
            if custom && !all.user_bound(role.as_str()) {
                None
            } else {
                all.bound(role.as_str())
            }
        };
        let bold = face(&fonts::READING_BOLD).unwrap_or(regular);
        let italic = face(&fonts::READING_ITALIC).unwrap_or(regular);
        let bold_italic = face(&fonts::READING_BOLD_ITALIC).unwrap_or(bold);
        let (tone, sharp) = ink(env);
        Some(Self {
            fonts: [regular, bold, italic, bold_italic],
            weight: page.weight,
            tone,
            sharp,
            word_spacing: page.word_spacing,
            letter_spacing: page.letter_spacing,
            kerning: page.kerning,
            ligatures: page.ligatures && page.letter_spacing == 0.0,
            embedded,
            size: page.size,
            ascent: page.ascent,
            line_height: page.line_height,
        })
    }

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
        if self.ligatures {
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
    pub fn shift(&self, face: Face) -> i32 {
        let size = self.line_size(face);
        if face.style.superscript() {
            -(size * RAISED + 0.5) as i32
        } else if face.style.subscript() {
            (size * LOWERED + 0.5) as i32
        } else {
            0
        }
    }

    /// The letter spacing of `text`: after each of its characters.
    fn letters(&self, text: &str) -> f32 {
        match self.letter_spacing {
            0.0 => 0.0,
            spacing => spacing * text.chars().count() as f32,
        }
    }

    /// The width of `text` in `face`, small capitals included.
    pub fn measure(&self, face: Face, text: &str) -> f32 {
        let text = &*visible(text);
        let (font, size) = (self.font(face), self.size(face));
        let letters = self.letters(text);
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

    /// The lines under and through text in `face` from `x`, `width` wide,
    /// on the line's `baseline`, in `ink`.
    pub fn decorate(
        &self,
        canvas: &mut Canvas<'_>,
        face: Face,
        (x, width): (f32, f32),
        baseline: i32,
        ink: Color,
    ) {
        let (under, through) = (face.style.underline(), face.style.strike());
        if !(under || through) || width <= 0.0 {
            return;
        }
        let size = self.line_size(face);
        let thick = ((size / 16.0) as u32).max(1);
        let (left, right) = ((x + 0.5) as i32, (x + width + 0.5) as i32);
        let length = (right - left).max(0) as u32;
        if under {
            let y = baseline + ((size / 10.0) as i32).max(1);
            canvas.fill_rect(left, y, length, thick, ink);
        }
        if through {
            let y = baseline - (size * 0.28) as i32;
            canvas.fill_rect(left, y, length, thick, ink);
        }
    }

    /// A space between words in `face`.
    pub fn space(&self, face: Face) -> f32 {
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
    pub fn draw_in(
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
        // Letter by letter where letters are spaced, each pair kerned as
        // measuring kerns it; at once otherwise.
        let draw = |canvas: &mut Canvas<'_>, size: f32, x: f32, text: &str| -> f32 {
            if self.letter_spacing == 0.0 {
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
                width += self.letter_spacing;
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
            return upper + self.letter_spacing;
        }
        let advance = self.font(face).advance(self.size(face), c);
        // Most books space no letters: no float to add for each one. An
        // advance is never -0.0, the one value adding 0.0 changes.
        match self.letter_spacing.to_bits() {
            0 => advance,
            _ => advance + self.letter_spacing,
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
                        words.push(WordBox::new(
                            Rect::new(x as u16, top as u16, width as u16, height as u16),
                            part,
                            Locator::new(line.section, start),
                            Locator::new(line.section, end),
                        ));
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
                .rtl(run.is_rtl());
                boxes.push((piece.ordinal, piece.range, word));
            }
        }
    }
    boxes.sort_by_key(|(ordinal, _, _)| *ordinal);
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

/// The contrast curve of book text ([`settings::TEXT_CONTRAST`]), and
/// whether it is drawn without grays ([`settings::SMOOTH_TEXT`] off).
/// Not where the panel shows black and white alone: its frame draws the
/// same letters already, and a page shown in grays there for its
/// pictures keeps its letters smooth, as before.
fn ink(env: &Env<'_>) -> (ToneCurve, bool) {
    let s = env.settings();
    let level = s.read(&settings::TEXT_CONTRAST);
    let step = settings::TEXT_CONTRAST_LEVELS
        .iter()
        .position(|l| *l == level)
        .map_or(0, |at| at as i8 + ToneCurve::LIGHTEST);
    let mono = env.device().display.format.gray_levels() == Some(2);
    let sharp = !mono && !s.read(&settings::SMOOTH_TEXT);
    (ToneCurve::contrast(step), sharp)
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
        let above = baseline - (2.0 * page_type.line_height) as i32;
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

/// Draws a picture of the page at its place in `page_type`'s box; where
/// it was not decoded (the heap had no room for it), a frame with a note
/// in its place.
pub fn draw_image(canvas: &mut Canvas<'_>, env: &Env<'_>, page_type: &PageType, image: &PageImage) {
    let t = page_type;
    let left = (t.x + image.x + 0.5) as i32;
    let top = (t.y + image.y + 0.5) as i32;
    let (w, h) = ((image.width + 0.5) as i32, (image.height + 0.5) as i32);
    let area = Rect::new(left as u16, top as u16, w as u16, h as u16);
    // Frames are drawn in bands: nothing read for those it is not in, nor
    // where it is only described.
    if area.intersection(&canvas.clip()).is_some()
        && !canvas.is_blind()
        && crate::image::Decoded::peek(env.storage(), &image.image.path).is_none()
    {
        return draw_missing_image(canvas, env, area);
    }
    crate::ui::draw_image(canvas, env.storage(), &image.image, area);
}

/// A frame in `area` where a picture is not there: with a short note
/// where the note fits whole, else crossed out.
fn draw_missing_image(canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
    let style = crate::ui::UiStyle::new(env);
    let line = u32::from(style.line()).max(1);
    canvas.stroke(area, line, Color::BLACK);
    let pad = style.gap();
    let inner = Rect::new(
        area.x.saturating_add(pad),
        area.y.saturating_add(pad),
        area.width.saturating_sub(2 * pad),
        area.height.saturating_sub(2 * pad),
    );
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    let text = env.i18n().tr("core.picture.missing");
    let note = crate::widgets::Wrapped::fitting(
        env,
        text,
        style.small(),
        style.small() * 0.8,
        inner.width,
        inner.height,
    );
    if note.whole() {
        let height = note.height(env).min(inner.height);
        let at = Rect::new(
            inner.x,
            inner.y + (inner.height - height) / 2,
            inner.width,
            height,
        );
        note.draw(canvas, env, at, crate::widgets::Align::Center, Color::BLACK);
        return;
    }
    let to = |edge: u32| i32::try_from(edge).unwrap_or(i32::MAX);
    let (right, bottom) = (to(inner.right()), to(inner.bottom()));
    let (x, y) = (i32::from(inner.x), i32::from(inner.y));
    canvas.draw_line(
        Point::new(x, y),
        Point::new(right, bottom),
        line,
        Color::BLACK,
    );
    canvas.draw_line(
        Point::new(right, y),
        Point::new(x, bottom),
        line,
        Color::BLACK,
    );
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
