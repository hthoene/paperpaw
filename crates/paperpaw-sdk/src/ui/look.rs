//! Another look for the parts every screen draws with this module: a
//! plugin gives switches, ticked boxes, the focus, separators, buttons,
//! headers, the row of button hints, progress bars and sliders a look
//! of its own, for all plugins at once.
//!
//! The SDK keeps where a part goes, what it says to screen readers and
//! how it is touched; a look only paints the part's rectangle. A painter
//! returns whether it painted (else the part is drawn as usual), and the
//! painters of parts with a label on them return the ink for it:
//!
//! ```
//! use paperpaw_sdk::Priority;
//! use paperpaw_sdk::ui::look::{PaintCx, Painters, SwitchPart};
//! use paperpaw_platform::Color;
//!
//! /// Square switches: a box, filled when on.
//! fn switch(cx: &mut PaintCx<'_, '_>, part: &SwitchPart) -> bool {
//!     let ink = if part.inverted { Color::WHITE } else { Color::BLACK };
//!     cx.canvas.stroke_round(part.rect, 2, 0, ink);
//!     if part.on {
//!         cx.canvas.fill(part.rect.inset(4, 4), ink);
//!     }
//!     true
//! }
//!
//! static SQUARE: Painters = Painters::new().switch(switch);
//! // In `register`: r.set_ui_look(&SQUARE, Priority::DEFAULT)?;
//! # let _ = (&SQUARE, Priority::DEFAULT);
//! ```
//!
//! The look in use is the one set with the highest priority (the later
//! of equal ones); [`UiStyle::new`](super::UiStyle::new) takes it from
//! [`Env::look`](crate::Env::look). A look is a table in the program,
//! so it takes no memory while it is not in use.

use core::fmt;

use paperpaw_graphics::Canvas;
use paperpaw_platform::{Color, Rect};

use super::UiStyle;

/// The switch of an on/off setting ([`Ui::switch`](super::Ui::switch)).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct SwitchPart {
    /// Where the switch goes ([`switch_rect`](super::switch_rect)).
    pub rect: Rect,
    pub on: bool,
    /// On a focused (black) row: drawn in white.
    pub inverted: bool,
}

/// The box of a row that is ticked or not, of a list where several may
/// be ([`draw_check`](super::draw_check)).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct CheckPart {
    /// Where the box goes.
    pub rect: Rect,
    pub on: bool,
    /// On a focused (black) row: drawn in white.
    pub inverted: bool,
}

/// The focus of a row or an item ([`Ui::focus`](super::Ui::focus)):
/// what is drawn on it comes in white.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct FocusPart {
    /// Where the focus goes ([`focus_rect`](super::focus_rect)).
    pub rect: Rect,
}

/// The line below a row ([`Ui::separator`](super::Ui::separator)).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct SeparatorPart {
    /// The row.
    pub row: Rect,
    /// Where the row's text starts after the margin.
    pub indent: u16,
}

/// A widget's frame ([`widgets::draw_frame`](crate::widgets::draw_frame)):
/// where it goes and which the widget's settings chose. A painter draws
/// the line, card or dark card its own way (or `false`: as usual).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct FramePart {
    pub area: Rect,
    pub frame: crate::widgets::Frame,
}

impl FramePart {
    /// Frame `frame` round `area`.
    pub fn new(area: Rect, frame: crate::widgets::Frame) -> Self {
        Self { area, frame }
    }
}

/// A button's frame ([`Ui::button`](super::Ui::button),
/// [`Ui::icon_button`](super::Ui::icon_button)); its label or drawing
/// comes after, in the ink the painter returns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct ButtonPart {
    pub rect: Rect,
    /// Chosen, e.g. the value set.
    pub on: bool,
    /// Has the focus of the buttons.
    pub focused: bool,
}

/// A header's background ([`Ui::header`](super::Ui::header)); its title
/// and note come after, in the ink the painter returns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct HeaderPart {
    pub area: Rect,
}

/// The background of the row of button hints below the screen, with the
/// line that sets it apart; the hints come after.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct HintRowPart {
    pub area: Rect,
}

/// A progress bar ([`Ui::progress`](super::Ui::progress)).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct ProgressPart {
    pub area: Rect,
    /// Thousandths done, at most 1000.
    pub permille: u16,
    pub ink: Color,
}

/// A slider's track and knob ([`Slider::draw_track`](super::slider::Slider::draw_track)).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct SliderPart {
    pub track: Rect,
    /// Where the knob's middle is.
    pub knob_x: i32,
    pub ink: Color,
}

/// A sheet that comes up from an edge of the screen over what is below
/// (a menu, a panel of choices).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct SheetPart {
    pub rect: Rect,
    /// The edge it comes from.
    pub edge: crate::chrome::Edge,
    pub inverted: bool,
}

/// A book's cover in a list or a grid, without a picture: its frame and
/// where its title goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct CoverPart {
    pub rect: Rect,
    /// Where the title is written on it.
    pub title_rect: Rect,
    pub inverted: bool,
}

/// A pill: a small rounded choice in a row of them (a filter, a tag).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct PillPart {
    pub rect: Rect,
    pub selected: bool,
    pub inverted: bool,
}

/// The ticks of a scale: `count` of them, the one at `at` marked (a
/// slider's steps, a page's place).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct TicksPart {
    pub rect: Rect,
    pub count: u16,
    pub at: u16,
    pub inverted: bool,
}

/// What a painter paints with: the canvas the part is drawn into and the
/// screen's style. More may come; a painter takes what it needs.
#[non_exhaustive]
pub struct PaintCx<'c, 'a> {
    pub canvas: &'c mut Canvas<'a>,
    pub style: &'c UiStyle,
}

impl fmt::Debug for PaintCx<'_, '_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PaintCx").finish_non_exhaustive()
    }
}

/// Paints a part; `true` when it did.
pub type Painter<P> = fn(&mut PaintCx<'_, '_>, &P) -> bool;
/// Paints a part's background; the ink for what goes on it when it did.
pub type InkPainter<P> = fn(&mut PaintCx<'_, '_>, &P) -> Option<Color>;

/// A look: a painter for each part it changes (see the
/// [module](self)). Made in the program, e.g. as a `static`.
#[derive(Clone, Copy)]
pub struct Painters {
    switch: Option<Painter<SwitchPart>>,
    check: Option<Painter<CheckPart>>,
    focus: Option<Painter<FocusPart>>,
    separator: Option<Painter<SeparatorPart>>,
    button: Option<InkPainter<ButtonPart>>,
    header: Option<InkPainter<HeaderPart>>,
    hints: Option<Painter<HintRowPart>>,
    progress: Option<Painter<ProgressPart>>,
    slider: Option<Painter<SliderPart>>,
    frame: Option<Painter<FramePart>>,
    sheet: Option<Painter<SheetPart>>,
    cover: Option<InkPainter<CoverPart>>,
    pill: Option<InkPainter<PillPart>>,
    ticks: Option<Painter<TicksPart>>,
}

impl fmt::Debug for Painters {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Painters")
            .field("switch", &self.switch.is_some())
            .field("check", &self.check.is_some())
            .field("focus", &self.focus.is_some())
            .field("separator", &self.separator.is_some())
            .field("button", &self.button.is_some())
            .field("header", &self.header.is_some())
            .field("hints", &self.hints.is_some())
            .field("progress", &self.progress.is_some())
            .field("slider", &self.slider.is_some())
            .field("frame", &self.frame.is_some())
            .field("sheet", &self.sheet.is_some())
            .field("cover", &self.cover.is_some())
            .field("pill", &self.pill.is_some())
            .field("ticks", &self.ticks.is_some())
            .finish()
    }
}

impl Default for Painters {
    fn default() -> Self {
        Self::new()
    }
}

impl Painters {
    /// No painters: every part as usual.
    pub const fn new() -> Self {
        Self {
            switch: None,
            check: None,
            focus: None,
            separator: None,
            button: None,
            header: None,
            hints: None,
            progress: None,
            slider: None,
            frame: None,
            sheet: None,
            cover: None,
            pill: None,
            ticks: None,
        }
    }

    pub const fn switch(mut self, painter: Painter<SwitchPart>) -> Self {
        self.switch = Some(painter);
        self
    }

    /// The boxes of lists where several may be ticked.
    pub const fn check(mut self, painter: Painter<CheckPart>) -> Self {
        self.check = Some(painter);
        self
    }

    pub const fn focus(mut self, painter: Painter<FocusPart>) -> Self {
        self.focus = Some(painter);
        self
    }

    pub const fn separator(mut self, painter: Painter<SeparatorPart>) -> Self {
        self.separator = Some(painter);
        self
    }

    pub const fn button(mut self, painter: InkPainter<ButtonPart>) -> Self {
        self.button = Some(painter);
        self
    }

    pub const fn header(mut self, painter: InkPainter<HeaderPart>) -> Self {
        self.header = Some(painter);
        self
    }

    pub const fn hints(mut self, painter: Painter<HintRowPart>) -> Self {
        self.hints = Some(painter);
        self
    }

    pub const fn progress(mut self, painter: Painter<ProgressPart>) -> Self {
        self.progress = Some(painter);
        self
    }

    pub const fn slider(mut self, painter: Painter<SliderPart>) -> Self {
        self.slider = Some(painter);
        self
    }

    /// Widgets' frames (lines, cards) on every screen.
    pub const fn frame(mut self, painter: Painter<FramePart>) -> Self {
        self.frame = Some(painter);
        self
    }

    /// Sheets that come up from an edge.
    pub const fn sheet(mut self, painter: Painter<SheetPart>) -> Self {
        self.sheet = Some(painter);
        self
    }

    /// Covers without a picture; the ink of the title.
    pub const fn cover(mut self, painter: InkPainter<CoverPart>) -> Self {
        self.cover = Some(painter);
        self
    }

    /// Pills; the ink of their label.
    pub const fn pill(mut self, painter: InkPainter<PillPart>) -> Self {
        self.pill = Some(painter);
        self
    }

    /// The ticks of scales.
    pub const fn ticks(mut self, painter: Painter<TicksPart>) -> Self {
        self.ticks = Some(painter);
        self
    }
}

/// The look a [`UiStyle`] draws with: the one set, or none.
#[derive(Clone, Copy, Default)]
pub struct Look(Option<&'static Painters>);

impl Look {
    /// Draws with `painters`.
    pub const fn of(painters: &'static Painters) -> Self {
        Self(Some(painters))
    }

    /// The parts as usual.
    pub const fn none() -> Self {
        Self(None)
    }

    /// The painters, if a look is set.
    pub fn painters(&self) -> Option<&'static Painters> {
        self.0
    }

    pub(super) fn switch(
        &self,
        canvas: &mut Canvas<'_>,
        style: &UiStyle,
        part: SwitchPart,
    ) -> bool {
        self.0
            .and_then(|p| p.switch)
            .is_some_and(|paint| paint(&mut PaintCx { canvas, style }, &part))
    }

    /// Paints a sheet with the look's painter; `false` where it has none.
    pub fn sheet(&self, canvas: &mut Canvas<'_>, style: &UiStyle, part: SheetPart) -> bool {
        self.0
            .and_then(|p| p.sheet)
            .is_some_and(|paint| paint(&mut PaintCx { canvas, style }, &part))
    }

    /// Paints a cover with the look's painter: the ink of its title, or
    /// `None` where it has none.
    pub fn cover(
        &self,
        canvas: &mut Canvas<'_>,
        style: &UiStyle,
        part: CoverPart,
    ) -> Option<Color> {
        self.0
            .and_then(|p| p.cover)
            .and_then(|paint| paint(&mut PaintCx { canvas, style }, &part))
    }

    /// Paints a pill with the look's painter: the ink of its label, or
    /// `None` where it has none.
    pub fn pill(&self, canvas: &mut Canvas<'_>, style: &UiStyle, part: PillPart) -> Option<Color> {
        self.0
            .and_then(|p| p.pill)
            .and_then(|paint| paint(&mut PaintCx { canvas, style }, &part))
    }

    /// Paints ticks with the look's painter; `false` where it has none.
    pub fn ticks(&self, canvas: &mut Canvas<'_>, style: &UiStyle, part: TicksPart) -> bool {
        self.0
            .and_then(|p| p.ticks)
            .is_some_and(|paint| paint(&mut PaintCx { canvas, style }, &part))
    }

    pub(super) fn check(&self, canvas: &mut Canvas<'_>, style: &UiStyle, part: CheckPart) -> bool {
        self.0
            .and_then(|p| p.check)
            .is_some_and(|paint| paint(&mut PaintCx { canvas, style }, &part))
    }

    pub(crate) fn frame(&self, canvas: &mut Canvas<'_>, style: &UiStyle, part: FramePart) -> bool {
        self.0
            .and_then(|p| p.frame)
            .is_some_and(|paint| paint(&mut PaintCx { canvas, style }, &part))
    }

    pub(super) fn focus(&self, canvas: &mut Canvas<'_>, style: &UiStyle, part: FocusPart) -> bool {
        self.0
            .and_then(|p| p.focus)
            .is_some_and(|paint| paint(&mut PaintCx { canvas, style }, &part))
    }

    pub(super) fn separator(
        &self,
        canvas: &mut Canvas<'_>,
        style: &UiStyle,
        part: SeparatorPart,
    ) -> bool {
        self.0
            .and_then(|p| p.separator)
            .is_some_and(|paint| paint(&mut PaintCx { canvas, style }, &part))
    }

    pub(super) fn button(
        &self,
        canvas: &mut Canvas<'_>,
        style: &UiStyle,
        part: ButtonPart,
    ) -> Option<Color> {
        self.0
            .and_then(|p| p.button)
            .and_then(|paint| paint(&mut PaintCx { canvas, style }, &part))
    }

    pub(super) fn header(
        &self,
        canvas: &mut Canvas<'_>,
        style: &UiStyle,
        part: HeaderPart,
    ) -> Option<Color> {
        self.0
            .and_then(|p| p.header)
            .and_then(|paint| paint(&mut PaintCx { canvas, style }, &part))
    }

    pub(super) fn hints(
        &self,
        canvas: &mut Canvas<'_>,
        style: &UiStyle,
        part: HintRowPart,
    ) -> bool {
        self.0
            .and_then(|p| p.hints)
            .is_some_and(|paint| paint(&mut PaintCx { canvas, style }, &part))
    }

    pub(super) fn progress(
        &self,
        canvas: &mut Canvas<'_>,
        style: &UiStyle,
        part: ProgressPart,
    ) -> bool {
        self.0
            .and_then(|p| p.progress)
            .is_some_and(|paint| paint(&mut PaintCx { canvas, style }, &part))
    }

    pub(super) fn slider(
        &self,
        canvas: &mut Canvas<'_>,
        style: &UiStyle,
        part: SliderPart,
    ) -> bool {
        self.0
            .and_then(|p| p.slider)
            .is_some_and(|paint| paint(&mut PaintCx { canvas, style }, &part))
    }
}

impl PartialEq for Look {
    fn eq(&self, other: &Self) -> bool {
        match (self.0, other.0) {
            (Some(a), Some(b)) => core::ptr::eq(a, b),
            (None, None) => true,
            _ => false,
        }
    }
}

impl fmt::Debug for Look {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some(painters) => painters.fmt(f),
            None => f.write_str("Look::none()"),
        }
    }
}
