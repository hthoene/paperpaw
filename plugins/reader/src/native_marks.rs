//! The marks on the page: the book's highlights (kept in the library as
//! annotations) and the match of a search hit gone to, drawn behind and over
//! the words they cover as Beta 3's `core.highlight` and `core.search-hit`
//! roles were (`paperpaw_sdk::decorations`: a background tone, a line under,
//! a frame, a line through).
//!
//! Host-free but for [`load`] and [`read_decoration`]: the spans are
//! compared with the page's words (`Word` places), the boxes are one a line
//! (`native_selection::line_boxes`), and the drawing goes through [`Pen`] so a
//! test sees what would be painted.
//!
//! **Memory.** Only the highlights are held (24 bytes each, at most
//! [`MAX_HIGHLIGHTS`]); Beta 3 held every note with its text. A book with more
//! highlights draws the first ones the library gives.
use crate::native_selection::line_boxes;
use crate::native_word::Word;
use alloc::string::String;
use alloc::vec::Vec;
use lead::{LEAD_MAX, Lead};
use paperpaw_sdk::{
    Color, Rect,
    decorations::{Decoration, LineKind, THICKNESS, Tone},
    native::{
        Error, Host,
        callbacks::FrameRect,
        canvas::Canvas,
        library::{AnnotationCursor, AnnotationKind, BookRef, Library},
        settings::{self, SettingValue},
    },
};
use paperpaw_ui_kit::mem::{SERVICE_BUFFER, buffer};

/// Most highlights held for the pages: the first ones the library gives.
pub const MAX_HIGHLIGHTS: usize = 512;

/// The most annotation rows looked through for one book.
pub const MAX_ROWS: usize = 8192;

/// The role of one's own highlights.
pub const HIGHLIGHT: &str = "core.highlight";
/// The role of a search hit gone to.
pub const SEARCH_HIT: &str = "core.search-hit";

/// A stretch of the book: from `start` up to (not including) `end`, each a
/// section and an offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: (u32, u32),
    pub end: (u32, u32),
    /// A highlight that has a note (the bar then offers *Edit note*).
    pub noted: bool,
}

impl Span {
    /// Whether `word` is (partly) inside the stretch.
    #[must_use]
    pub fn covers(&self, word: &Word) -> bool {
        let (start, end) = (
            (word.start.section, word.start.offset),
            (word.end.section, word.end.offset),
        );
        self.start < end && start < self.end
    }
}

/// A search hit shown on the page that starts at `page`: it goes with the
/// first draw of another page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hit {
    pub page: (u32, u32),
    pub span: Span,
}

/// Most highlights of another conversion held until a page has their text
/// (#265): the first ones the library gives.
pub const MAX_FOREIGN: usize = 128;

/// A highlight made in another conversion of the book: drawn when a page has the
/// text it kept at the place it has (`lead`), else not at all. It stays in the
/// library as it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Foreign {
    pub span: Span,
    pub lead: Lead,
}

/// What marks the pages of the open book.
#[derive(Debug, Default)]
pub struct Marks {
    highlights: Vec<Span>,
    /// Highlights of another conversion not yet found on a page.
    foreign: Vec<Foreign>,
    hit: Option<Hit>,
}

impl Marks {
    /// The marks of a book with `highlights`.
    #[must_use]
    pub fn new(highlights: Vec<Span>) -> Self {
        Self {
            highlights,
            foreign: Vec::new(),
            hit: None,
        }
    }

    /// The same, with the highlights of other conversions that a page may
    /// show.
    #[must_use]
    pub fn with_foreign(mut self, foreign: Vec<Foreign>) -> Self {
        self.foreign = foreign;
        self
    }

    /// Takes the highlights of another conversion whose text `words` (the words
    /// of the page on show) have where the highlight starts: they are drawn from
    /// here on, in memory only. A highlight that continues on the next page is
    /// found by the page it starts on.
    pub fn accept_foreign(&mut self, words: &[Word]) {
        let (Some(first), Some(last)) = (words.first(), words.last()) else {
            return;
        };
        if self.foreign.is_empty() {
            return;
        }
        let (from, to) = (
            (first.start.section, first.start.offset),
            (last.end.section, last.end.offset),
        );
        let mut i = 0;
        while i < self.foreign.len() {
            let Some(held) = self.foreign.get(i).copied() else {
                break;
            };
            let on_page = held.span.start >= from && held.span.start < to;
            if on_page
                && self.highlights.len() < MAX_HIGHLIGHTS
                && self.highlights.try_reserve(1).is_ok()
                && is_there(words, &held)
            {
                self.highlights.push(held.span);
                self.foreign.swap_remove(i);
            } else {
                i += 1;
            }
        }
    }

    /// The highlights held.
    #[must_use]
    pub fn highlights(&self) -> &[Span] {
        &self.highlights
    }

    /// A number that tells one list of highlights from another (a fold of the
    /// places and the notes flags): equal lists have the same one.
    #[must_use]
    pub fn fingerprint(&self) -> u64 {
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        let mut mix = |value: u32| {
            for byte in value.to_le_bytes() {
                hash = (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
            }
        };
        for span in &self.highlights {
            mix(span.start.0);
            mix(span.start.1);
            mix(span.end.0);
            mix(span.end.1);
            mix(u32::from(span.noted));
        }
        hash ^ (self.highlights.len() as u64)
    }

    /// Gives up the highlights held (to free them before a new reading of the
    /// library: the old list and the new one are not held at once).
    pub fn take(&mut self) -> Vec<Span> {
        self.foreign = Vec::new();
        core::mem::take(&mut self.highlights)
    }

    /// Takes the highlights of a new reading of the library.
    pub fn replace(&mut self, (highlights, foreign): (Vec<Span>, Vec<Foreign>)) {
        self.highlights = highlights;
        self.foreign = foreign;
    }

    /// Shows `span` as a search hit on the page that starts at `page`.
    pub fn found(&mut self, page: (u32, u32), span: Span) {
        self.hit = Some(Hit { page, span });
    }

    /// The hit to draw on the page that starts at `page`; a hit of another
    /// page is dropped.
    pub fn hit_on(&mut self, page: (u32, u32)) -> Option<Span> {
        if self.hit.is_some_and(|hit| hit.page != page) {
            self.hit = None;
        }
        self.hit.map(|hit| hit.span)
    }

    /// Whether the page of `words` has anything to draw: a highlight or the
    /// hit inside it.
    #[must_use]
    pub fn on_page(&self, words: &[Word], hit: Option<Span>) -> bool {
        let (Some(first), Some(last)) = (words.first(), words.last()) else {
            return false;
        };
        let (from, to) = (
            (first.start.section, first.start.offset),
            (last.end.section, last.end.offset),
        );
        let inside = |s: &Span| s.start < to && from < s.end;
        hit.as_ref().is_some_and(inside) || self.highlights.iter().any(inside)
    }
}

/// The highlights of the book at `path` in the conversion `mine`, read from the
/// library's annotations (one with none is taken; a highlight of no length marks
/// nothing), and those of other conversions that kept enough text to be
/// found on a page again.
///
/// # Errors
/// `State` for a book the library does not know, `NoMemory`, the host's
/// refusals.
pub fn load(
    host: &Host<'_>,
    library: &Library,
    path: &str,
    mine: u64,
) -> Result<(Vec<Span>, Vec<Foreign>), Error> {
    let book = BookRef::path(path)?;
    // The library's calls take 1,024 bytes of input (a path of up to 512).
    let (mut input, mut out) = (buffer(1024)?, buffer(SERVICE_BUFFER)?);
    let mut found: Vec<Span> = Vec::new();
    let mut foreign: Vec<Foreign> = Vec::new();
    let mut cursor = AnnotationCursor::start();
    let mut seen = 0usize;
    loop {
        let page = library.annotations(host, book, cursor, &mut input, &mut out)?;
        for row in page.rows() {
            seen += 1;
            let row = row?;
            let span = Span {
                start: (row.start_section(), row.start_offset()),
                end: (row.end_section(), row.end_offset()),
                noted: row.noted(),
            };
            if row.kind() != AnnotationKind::Highlight || span.start >= span.end {
                continue;
            }
            if row.conversion().is_none_or(|c| c == mine) {
                if found.len() < MAX_HIGHLIGHTS {
                    // Exactly one more: the usual growth would take room for twice.
                    found.try_reserve_exact(1).map_err(|_| Error::NoMemory)?;
                    found.push(span);
                }
            } else if foreign.len() < MAX_FOREIGN
                && let Some(lead) = Lead::of_any(row.text())
            {
                foreign.try_reserve_exact(1).map_err(|_| Error::NoMemory)?;
                foreign.push(Foreign { span, lead });
            }
        }
        match page.end().next() {
            Some(next) if seen < MAX_ROWS => cursor = next,
            _ => {
                // Nothing held beyond what was read.
                found.shrink_to_fit();
                foreign.shrink_to_fit();
                return Ok((found, foreign));
            }
        }
    }
}

/// Whether the page of `words` has the text of the highlight `held` where it
/// is. A text of a word or two ("Om", "the") is on too many pages to be told by
/// being near its place: it is there only where the words the highlight covers
/// are that text and no other (Beta 3's short highlights were never drawn
/// before, #265).
fn is_there(words: &[Word], held: &Foreign) -> bool {
    if held.lead.is_short() {
        covered_says(words, &held.span, &held.lead)
    } else {
        has_text(words, held.span.start, &held.lead)
    }
}

/// Whether the words `span` covers say `lead` and nothing else (or its start,
/// where the page ends inside the highlight).
fn covered_says(words: &[Word], span: &Span, lead: &Lead) -> bool {
    let mut text = String::new();
    let mut last = None;
    for (at, word) in words.iter().enumerate() {
        if !span.covers(word) {
            continue;
        }
        last = Some(at);
        // The second part of a word the layout broke says the word again.
        if crate::native_selection::continues(words, at) {
            continue;
        }
        if text.len() > LEAD_MAX || text.try_reserve(word.text.len() + 1).is_err() {
            return false;
        }
        if !text.is_empty() {
            text.push(' ');
        }
        text.push_str(&word.text);
    }
    let open = last.is_some_and(|at| at + 1 == words.len());
    !text.is_empty() && lead.says(&text, open)
}

/// Whether the words of a page from a little before `at` say `lead`: the first
/// few words, as a highlight keeps the text of its words. The stretch starts
/// [`SHIFT`] before the place, as another conversion of the same text may count
/// a character or two otherwise.
fn has_text(words: &[Word], at: (u32, u32), lead: &Lead) -> bool {
    let mut text = String::new();
    let from = (at.0, at.1.saturating_sub(SHIFT));
    let first = words.partition_point(|w| (w.end.section, w.end.offset) <= from);
    for (at, word) in words.iter().enumerate().skip(first) {
        // The second part of a word the layout broke says the word again.
        if crate::native_selection::continues(words, at) {
            continue;
        }
        if text.len() > LEAD_MAX + 2 * SHIFT as usize
            || text.try_reserve(word.text.len() + 1).is_err()
        {
            break;
        }
        if !text.is_empty() {
            text.push(' ');
        }
        text.push_str(&word.text);
    }
    lead.is_in(&text)
}

/// How many characters before its place the text of a highlight of another
/// conversion is looked for.
const SHIFT: u32 = 16;

/// The look of a role, from its settings (`<role>.background`, `.ink`, `.line`,
/// `.line-tone`, `.thickness`, `.frame`, `.struck`, as `Decoration::of_role` reads
/// them in interface 6). A setting the host does not give is no mark.
///
/// # Errors
/// The host's refusals other than a missing setting.
pub fn read_decoration(host: &Host<'_>, role: &str) -> Result<Decoration, Error> {
    let mut scratch = [0u8; 96];
    let mut key = [0u8; 48];
    // The text is the value in `scratch` (not the start of the buffer: that
    // holds the reply's framing).
    let mut text = |aspect: &str, scratch: &mut [u8]| -> Result<Option<Look>, Error> {
        let name = join(&mut key, role, aspect)?;
        match settings::read_setting(host, name, scratch) {
            Ok(SettingValue::Text(value)) => Ok(Some(Look {
                tone: Tone::from_id(value),
                line: LineKind::from_id(value),
            })),
            Ok(_) | Err(Error::NotFound) => Ok(None),
            Err(error) => Err(error),
        }
    };
    let mut deco = Decoration::NONE;
    for aspect in ["background", "ink", "line-tone"] {
        let tone = text(aspect, &mut scratch)?.and_then(|look| look.tone);
        match (aspect, tone) {
            ("background", Some(tone)) => deco = deco.background(tone),
            ("ink", Some(tone)) => deco = deco.ink(tone),
            ("line-tone", Some(tone)) => deco = deco.line_tone(tone),
            _ => {}
        }
    }
    if let Some(kind) = text("line", &mut scratch)?.and_then(|look| look.line) {
        deco = deco.line(kind);
    }
    let mut bytes = [0u8; 32];
    for (aspect, set) in [("thickness", 0u8), ("frame", 1), ("struck", 2)] {
        let name = join(&mut key, role, aspect)?;
        match (set, settings::read_setting(host, name, &mut bytes)) {
            (0, Ok(SettingValue::Int(t))) => {
                deco = deco.thickness(t.clamp(0, i64::from(u8::MAX)) as u8);
            }
            (1, Ok(SettingValue::Bool(true))) => deco = deco.frame(),
            (2, Ok(SettingValue::Bool(true))) => deco = deco.struck(),
            (_, Err(error)) if error != Error::NotFound => return Err(error),
            _ => {}
        }
    }
    Ok(deco)
}

/// What a text setting's value names: a tone or a line kind.
struct Look {
    tone: Option<Tone>,
    line: Option<LineKind>,
}

/// `role`, a dot and `aspect` in `buffer`.
fn join<'a>(buffer: &'a mut [u8], role: &str, aspect: &str) -> Result<&'a str, Error> {
    let total = role.len() + 1 + aspect.len();
    let room = buffer.get_mut(..total).ok_or(Error::NoSpace)?;
    room[..role.len()].copy_from_slice(role.as_bytes());
    room[role.len()] = b'.';
    room[role.len() + 1..].copy_from_slice(aspect.as_bytes());
    core::str::from_utf8(room).map_err(|_| Error::Invalid)
}

/// The marks of one page, ready to draw: where each look goes and the look.
/// Out of memory leaves the marks of the page out (the text is drawn).
#[derive(Debug, Default)]
pub struct Painted {
    gauge: Option<Gauge>,
    layers: Vec<(Vec<Rect>, Decoration)>,
}

/// The looks of a highlight and of a search hit ([`HIGHLIGHT`], [`SEARCH_HIT`]),
/// each from its role's settings, and whether both were read: a setting the
/// host refuses is no mark, and is read again the next time.
///
/// # Errors
/// `NoMemory`.
pub fn read_looks(host: &Host<'_>) -> Result<((Decoration, Decoration), bool), Error> {
    let mut whole = true;
    let mut look = |role: &str| match read_decoration(host, role) {
        Err(Error::NoMemory) => Err(Error::NoMemory),
        Ok(deco) => Ok(deco),
        Err(_) => {
            whole = false;
            Ok(Decoration::NONE)
        }
    };
    let looks = (look(HIGHLIGHT)?, look(SEARCH_HIT)?);
    Ok((looks, whole))
}

impl Painted {
    /// The page of `words` with the highlights and the `hit` that are on it, in
    /// the `looks` of a highlight and of a hit ([`read_looks`]), asked only where
    /// something is on the page.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn new(
        looks: impl FnOnce() -> Result<(Decoration, Decoration), Error>,
        marks: &Marks,
        words: &[Word],
        hit: Option<Span>,
        gauge: Gauge,
    ) -> Result<Self, Error> {
        if !marks.on_page(words, hit) {
            return Ok(Self::default());
        }
        let (highlight, search_hit) = looks()?;
        let mut painted = Self {
            gauge: Some(gauge),
            layers: Vec::new(),
        };
        let hits = hit.as_slice();
        for (spans, deco) in [(marks.highlights(), highlight), (hits, search_hit)] {
            let boxes = boxes(words, spans);
            if boxes.is_empty() {
                continue;
            }
            if deco.is_none() {
                continue;
            }
            painted.layers.try_reserve(1).map_err(|_| Error::NoMemory)?;
            painted.layers.push((boxes, deco));
        }
        Ok(painted)
    }

    /// Whether there is nothing to draw.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.layers.is_empty()
    }

    /// The backgrounds, to be drawn before the text.
    ///
    /// # Errors
    /// The host's refusal.
    pub fn behind(&self, pen: &mut impl Pen) -> Result<(), Error> {
        let Some(gauge) = self.gauge else {
            return Ok(());
        };
        for (boxes, deco) in &self.layers {
            draw_behind(pen, gauge, boxes, deco)?;
        }
        Ok(())
    }

    /// The lines and frames, to be drawn over the text.
    ///
    /// # Errors
    /// The host's refusal.
    pub fn over(&self, pen: &mut impl Pen) -> Result<(), Error> {
        let Some(gauge) = self.gauge else {
            return Ok(());
        };
        for (boxes, deco) in &self.layers {
            draw_over(pen, gauge, boxes, deco)?;
        }
        Ok(())
    }
}

/// What the marks are painted with.
pub trait Pen {
    /// Fills a rectangle.
    ///
    /// # Errors
    /// The host's refusal.
    fn fill(&mut self, x: i32, y: i32, width: u32, height: u32, color: Color) -> Result<(), Error>;

    /// Strokes a rectangle `thickness` pixels thick, inside it.
    ///
    /// # Errors
    /// The host's refusal.
    fn stroke(
        &mut self,
        rect: (i32, i32, u32, u32),
        thickness: u32,
        color: Color,
    ) -> Result<(), Error>;

    /// Fills every other pixel of a rectangle, as a checker: its top-left
    /// pixel and those an even number of steps from it, or the others where
    /// `odd`. One call whatever the size (`Canvas::fill_checker`).
    ///
    /// # Errors
    /// `Unsupported` where the pen has no such fill (a host without the
    /// slot): a tone is rows then. The host's refusal.
    fn checker(
        &mut self,
        _rect: (i32, i32, u32, u32),
        _color: Color,
        _odd: bool,
    ) -> Result<(), Error> {
        Err(Error::Unsupported)
    }
}

/// The page's canvas as a [`Pen`].
#[derive(Debug)]
pub struct CanvasPen<'a, 'b>(pub &'a Canvas<'b>);

impl Pen for CanvasPen<'_, '_> {
    fn fill(&mut self, x: i32, y: i32, width: u32, height: u32, color: Color) -> Result<(), Error> {
        if width == 0 || height == 0 {
            return Ok(());
        }
        self.0
            .fill_rectangle(FrameRect::new(x, y, width, height), color)
    }

    fn stroke(
        &mut self,
        (x, y, width, height): (i32, i32, u32, u32),
        thickness: u32,
        color: Color,
    ) -> Result<(), Error> {
        if width == 0 || height == 0 {
            return Ok(());
        }
        self.0
            .stroke_rounded_rectangle(FrameRect::new(x, y, width, height), 0, thickness, color)
    }

    fn checker(
        &mut self,
        (x, y, width, height): (i32, i32, u32, u32),
        color: Color,
        odd: bool,
    ) -> Result<(), Error> {
        if width == 0 || height == 0 {
            return Ok(());
        }
        self.0
            .fill_checker(FrameRect::new(x, y, width, height), color, odd)
    }
}

/// The boxes, one a line, round the words of `words` that `spans` cover.
#[must_use]
pub fn boxes(words: &[Word], spans: &[Span]) -> Vec<Rect> {
    line_boxes(words, |i| {
        words
            .get(i)
            .is_some_and(|word| spans.iter().any(|s| s.covers(word)))
    })
}

/// Sizes the marks are drawn in, from the display: half a millimetre of room
/// round a mark, and a tenth of a millimetre for each unit of thickness; and
/// whether the panel has so few grays that a background is dithered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gauge {
    pub dpi: u16,
    /// Black and white alone, or four grays (not colour): a tone behind text is
    /// a pattern of black dots by its lightness, as Beta 3's `fill_shaded`.
    pub few_grays: bool,
}

impl Gauge {
    /// The gauge of the display with `dpi`, `grays` levels (`None`: many) and
    /// colour or not.
    #[must_use]
    pub fn of(dpi: u16, grays: Option<u16>, colour: bool) -> Self {
        Self {
            dpi,
            few_grays: !colour && grays.is_some_and(|g| g <= 4),
        }
    }

    /// `tenths` tenths of a millimetre, in pixels.
    #[must_use]
    pub fn tenth_mm(self, tenths: u32) -> u32 {
        (u32::from(self.dpi) * tenths + 127) / 254
    }
}

/// Rows of the pattern that stands for a gray on a panel of few grays.
const PATTERN_ROWS: i32 = 4;

/// A tone as a pattern of black pixels, as dark as the tone is (a quarter, a
/// half, three quarters of them, or all): dots as Beta 3's `fill_shaded`
/// dithered a tone, so a light gray stays a light pattern the text reads well
/// on and no line runs through the letters (#265). The pattern is counted from
/// the top-left of the screen, so the boxes of two lines meet without a seam.
/// A pen that fills no checker (a host without the slot) draws the tone as
/// rows instead.
fn shade(pen: &mut impl Pen, area: (i32, i32, u32, u32), tone: Tone) -> Result<(), Error> {
    let dark = i32::from(255 - tone.color().luma());
    let black = ((dark * PATTERN_ROWS + 127) / 255).clamp(0, PATTERN_ROWS);
    if black == 0 || area.2 == 0 || area.3 == 0 {
        return Ok(());
    }
    if black < PATTERN_ROWS {
        match dots(pen, area, black) {
            Err(Error::Unsupported) => {}
            done => return done,
        }
    }
    let (x, y, width, height) = area;
    for row in y..y + height as i32 {
        if row.rem_euclid(PATTERN_ROWS) < black {
            pen.fill(x, row, width, 1, Color::BLACK)?;
        }
    }
    Ok(())
}

/// `quarters` (1 to 3) of the pixels of `area` black, as dots: the pixels of
/// every other row and column for one, a checker for two, the checker and
/// every other row for three. The first thing asked of the pen is a checker:
/// where it has none nothing is drawn yet.
fn dots(
    pen: &mut impl Pen,
    (x, y, width, height): (i32, i32, u32, u32),
    quarters: i32,
) -> Result<(), Error> {
    // A checker starts at its own corner: `odd` moves it to the screen's.
    if quarters >= 2 {
        let odd = (x + y).rem_euclid(2) == 1;
        pen.checker((x, y, width, height), Color::BLACK, odd)?;
    }
    for row in y..y + height as i32 {
        if row.rem_euclid(2) != 0 {
            continue;
        }
        if quarters == 1 {
            pen.checker((x, row, width, 1), Color::BLACK, x.rem_euclid(2) == 1)?;
        } else if quarters >= 3 {
            pen.fill(x, row, width, 1, Color::BLACK)?;
        }
    }
    Ok(())
}

/// Behind the words of `boxes` (one a line): the background.
///
/// # Errors
/// The host's refusal.
pub fn draw_behind(
    pen: &mut impl Pen,
    gauge: Gauge,
    boxes: &[Rect],
    deco: &Decoration,
) -> Result<(), Error> {
    let Some(tone) = deco.background else {
        return Ok(());
    };
    // A little wider than the words, so a marked word does not touch the edge
    // of its mark; not past the left edge of the screen.
    let pad = gauge.tenth_mm(5).max(1) as i32;
    for b in boxes {
        let left = (i32::from(b.x) - pad).max(0);
        let right = i32::from(b.x) + i32::from(b.width) + pad;
        let area = (
            left,
            i32::from(b.y),
            (right - left).max(0) as u32,
            u32::from(b.height),
        );
        if gauge.few_grays {
            shade(pen, area, tone)?;
        } else {
            pen.fill(area.0, area.1, area.2, area.3, tone.color())?;
        }
    }
    Ok(())
}

/// Over the words of `boxes` (one a line): the line under them, the frame, the
/// line through them.
///
/// # Errors
/// The host's refusal.
pub fn draw_over(
    pen: &mut impl Pen,
    gauge: Gauge,
    boxes: &[Rect],
    deco: &Decoration,
) -> Result<(), Error> {
    if deco.line.is_none() && !deco.frame && !deco.struck {
        return Ok(());
    }
    let tenths = if deco.thickness == 0 {
        THICKNESS
    } else {
        deco.thickness
    };
    let t = gauge.tenth_mm(u32::from(tenths)).max(1);
    let color = deco
        .line_tone
        .or(deco.ink)
        .map_or(Color::BLACK, Tone::color);
    for b in boxes {
        let (x, width) = (i32::from(b.x), u32::from(b.width));
        let bottom = i32::from(b.y) + i32::from(b.height);
        if let Some(kind) = deco.line {
            draw_line(pen, kind, (x, width), bottom, t, color)?;
        }
        if deco.frame {
            let pad = gauge.tenth_mm(5).max(1);
            pen.stroke(
                (
                    x - pad as i32,
                    i32::from(b.y),
                    width + 2 * pad,
                    u32::from(b.height),
                ),
                t,
                color,
            )?;
        }
        if deco.struck {
            // Through the middle of the small letters.
            let y = i32::from(b.y) + i32::from(b.height) * 3 / 5 - t as i32 / 2;
            pen.fill(x, y, width, t, color)?;
        }
    }
    Ok(())
}

/// A line of `kind` under text from `x`, `width` wide, its lowest pixels at
/// `bottom - 1`, `t` thick.
fn draw_line(
    pen: &mut impl Pen,
    kind: LineKind,
    (x, width): (i32, u32),
    bottom: i32,
    t: u32,
    color: Color,
) -> Result<(), Error> {
    let ti = t as i32;
    let end = x + width as i32;
    match kind {
        LineKind::Dotted | LineKind::Dashed => {
            let (on, step) = if kind == LineKind::Dotted {
                (ti, 2 * ti)
            } else {
                (3 * ti, 5 * ti)
            };
            let mut at = x;
            while at < end {
                pen.fill(at, bottom - ti, on.min(end - at) as u32, t, color)?;
                at += step.max(1);
            }
            Ok(())
        }
        LineKind::Wavy => {
            // Up and down by `t` over a period of four strokes, in pieces
            // `t` wide (a call to the host for each).
            let amplitude = ti.max(1);
            let period = (4 * ti).max(4);
            let mut column = x;
            while column < end {
                let phase = (column - x) % period;
                let rise = if phase < period / 2 {
                    phase
                } else {
                    period - phase
                };
                let dy = rise * 2 * amplitude / period;
                pen.fill(
                    column,
                    bottom - ti - dy,
                    ti.max(1).min(end - column) as u32,
                    t,
                    color,
                )?;
                column += ti.max(1);
            }
            Ok(())
        }
        LineKind::Double => {
            pen.fill(x, bottom - ti, width, t, color)?;
            pen.fill(x, bottom - 3 * ti, width, t, color)
        }
        // Solid, and a kind of a later build: one line.
        _ => pen.fill(x, bottom - ti, width, t, color),
    }
}

pub mod edit;
pub mod lead;

#[cfg(test)]
mod tests;
