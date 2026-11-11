//! The marks on the page: the book's highlights (kept in the library as
//! annotations) and the match of a search hit gone to, drawn behind and over
//! the words they cover as Beta 3's `core.highlight` and `core.search-hit`
//! roles were (`paperpaw_sdk::decorations`: a background tone, a line under,
//! a frame, a line through).
//!
//! Host-free but for [`load`] and [`read_decoration`]: the spans are
//! compared with the page's words (`WordBox` places), the boxes are one a line
//! (`native_selection::line_boxes`), and the drawing goes through [`Pen`] so a
//! test sees what would be painted.
//!
//! **Memory.** Only the highlights are held (24 bytes each, at most
//! [`MAX_HIGHLIGHTS`]); Beta 3 held every note with its text. A book with more
//! highlights draws the first ones the library gives.
use crate::native_selection::line_boxes;
use alloc::vec::Vec;
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
    reading::WordBox,
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
    pub fn covers(&self, word: &WordBox) -> bool {
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

/// What marks the pages of the open book.
#[derive(Debug, Default)]
pub struct Marks {
    highlights: Vec<Span>,
    hit: Option<Hit>,
}

impl Marks {
    /// The marks of a book with `highlights`.
    #[must_use]
    pub fn new(highlights: Vec<Span>) -> Self {
        Self {
            highlights,
            hit: None,
        }
    }

    /// The highlights held.
    #[must_use]
    pub fn highlights(&self) -> &[Span] {
        &self.highlights
    }

    /// Takes the highlights of a new reading of the library.
    pub fn replace(&mut self, highlights: Vec<Span>) {
        self.highlights = highlights;
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
    pub fn on_page(&self, words: &[WordBox], hit: Option<Span>) -> bool {
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
/// library's annotations (rows of another conversion are left out, one with none
/// is taken; a highlight of no length marks nothing).
///
/// # Errors
/// `State` for a book the library does not know, `NoMemory`, the host's
/// refusals.
pub fn load(host: &Host<'_>, library: &Library, path: &str, mine: u64) -> Result<Vec<Span>, Error> {
    let book = BookRef::path(path)?;
    let (mut input, mut out) = (buffer(256)?, buffer(SERVICE_BUFFER)?);
    let mut found: Vec<Span> = Vec::new();
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
            if row.kind() == AnnotationKind::Highlight
                && row.conversion().is_none_or(|c| c == mine)
                && span.start < span.end
                && found.len() < MAX_HIGHLIGHTS
            {
                found.try_reserve(1).map_err(|_| Error::NoMemory)?;
                found.push(span);
            }
        }
        match page.end().next() {
            Some(next) if seen < MAX_ROWS => cursor = next,
            _ => return Ok(found),
        }
    }
}

/// The look of a role, from its settings (`<role>.background`, `.ink`, `.line`,
/// `.line-tone`, `.thickness`, `.frame`, `.struck`, as `Decoration::of_role` reads
/// them in interface 6). A setting the host does not give is no mark.
///
/// # Errors
/// The host's refusals other than a missing setting.
pub fn read_decoration(host: &Host<'_>, role: &str) -> Result<Decoration, Error> {
    let mut scratch = [0u8; 96];
    let mut key = [0u8; 48];
    let mut text = |aspect: &str, scratch: &mut [u8]| -> Result<Option<usize>, Error> {
        let name = join(&mut key, role, aspect)?;
        match settings::read_setting(host, name, scratch) {
            Ok(SettingValue::Text(value)) => Ok(Some(value.len())),
            Ok(_) | Err(Error::NotFound) => Ok(None),
            Err(error) => Err(error),
        }
    };
    let mut deco = Decoration::NONE;
    for aspect in ["background", "ink", "line-tone"] {
        let tone = match text(aspect, &mut scratch)? {
            Some(len) => core::str::from_utf8(&scratch[..len])
                .ok()
                .and_then(Tone::from_id),
            None => None,
        };
        match (aspect, tone) {
            ("background", Some(tone)) => deco = deco.background(tone),
            ("ink", Some(tone)) => deco = deco.ink(tone),
            ("line-tone", Some(tone)) => deco = deco.line_tone(tone),
            _ => {}
        }
    }
    if let Some(len) = text("line", &mut scratch)?
        && let Some(kind) = core::str::from_utf8(&scratch[..len])
            .ok()
            .and_then(LineKind::from_id)
    {
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

impl Painted {
    /// The page of `words` with the highlights and the `hit` that are on it, in
    /// the looks of the roles' settings, read only where something is on the
    /// page. A setting the host refuses is no mark; only `NoMemory` is passed on.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn new(
        host: &Host<'_>,
        marks: &Marks,
        words: &[WordBox],
        hit: Option<Span>,
        dpi: u16,
    ) -> Result<Self, Error> {
        if !marks.on_page(words, hit) {
            return Ok(Self::default());
        }
        let look = |role: &str| match read_decoration(host, role) {
            Err(Error::NoMemory) => Err(Error::NoMemory),
            Ok(deco) => Ok(deco),
            Err(_) => Ok(Decoration::NONE),
        };
        let mut painted = Self {
            gauge: Some(Gauge { dpi }),
            layers: Vec::new(),
        };
        let hits = hit.as_slice();
        for (spans, role) in [(marks.highlights(), HIGHLIGHT), (hits, SEARCH_HIT)] {
            let boxes = boxes(words, spans);
            if boxes.is_empty() {
                continue;
            }
            let deco = look(role)?;
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
}

/// The boxes, one a line, round the words of `words` that `spans` cover.
#[must_use]
pub fn boxes(words: &[WordBox], spans: &[Span]) -> Vec<Rect> {
    line_boxes(words, |i| {
        words
            .get(i)
            .is_some_and(|word| spans.iter().any(|s| s.covers(word)))
    })
}

/// Sizes the marks are drawn in, from the display: half a millimetre of room
/// round a mark, and a tenth of a millimetre for each unit of thickness.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gauge {
    pub dpi: u16,
}

impl Gauge {
    /// `tenths` tenths of a millimetre, in pixels.
    #[must_use]
    pub fn tenth_mm(self, tenths: u32) -> u32 {
        (u32::from(self.dpi) * tenths + 127) / 254
    }
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
        pen.fill(
            left,
            i32::from(b.y),
            (right - left).max(0) as u32,
            u32::from(b.height),
            tone.color(),
        )?;
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

#[cfg(test)]
mod tests;
