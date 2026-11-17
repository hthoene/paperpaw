//! What a screen draws with: [`Ui`].

use paperpaw_graphics::Canvas;
use paperpaw_platform::{Color, Rect};

use crate::env::Env;
use crate::fonts::FontHandle;
use crate::widgets::Frame;

use super::UiStyle;

/// What a screen draws with: the canvas, the environment and the style
/// together, the interface's parts its methods, so that a screen names
/// them once and not with every part it draws.
///
/// ```
/// use paperpaw_sdk::ui::{self, Ui, UiStyle};
/// use paperpaw_sdk::{Canvas, Env, Rect};
///
/// fn draw(canvas: &mut Canvas<'_>, env: &Env<'_>, style: &UiStyle, focus: usize) {
///     let mut ui = Ui::new(canvas, env, style);
///     let focused = ui::focus_shown(env) && focus == 0;
///     ui.button(Rect::new(20, 700, 200, 60), env.i18n().tr("hello.ok"), false, focused);
///     ui.message(Rect::new(0, 100, 480, 400), env.i18n().tr("hello.text"));
/// }
/// ```
///
/// Making one allocates nothing: it borrows what it is given and looks up
/// the interface's font once.
pub struct Ui<'c> {
    canvas: Canvas<'c>,
    env: &'c Env<'c>,
    style: &'c UiStyle,
    /// The interface's font, looked up once.
    font: Option<FontHandle<'c>>,
}

impl core::fmt::Debug for Ui<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Ui").finish_non_exhaustive()
    }
}

impl<'c> Ui<'c> {
    /// Draws into `canvas` with `env` and `style`.
    pub fn new(canvas: &'c mut Canvas<'_>, env: &'c Env<'_>, style: &'c UiStyle) -> Self {
        Self {
            canvas: canvas.disabling(false),
            env,
            style,
            font: style.font(env),
        }
    }

    /// The canvas, for drawing of one's own beside the parts.
    pub fn canvas(&mut self) -> &mut Canvas<'c> {
        &mut self.canvas
    }

    /// The environment.
    pub fn env(&self) -> &'c Env<'c> {
        self.env
    }

    /// The style.
    pub fn style(&self) -> &'c UiStyle {
        self.style
    }

    /// The interface's font; none where no font is loaded.
    pub fn font(&self) -> Option<FontHandle<'c>> {
        self.font
    }

    /// The same, drawing within `area` only.
    pub fn clipped(&mut self, area: Rect) -> Ui<'_> {
        Ui {
            canvas: self.canvas.clipped(area),
            env: self.env,
            style: self.style,
            font: self.font,
        }
    }

    /// A button.
    pub fn button(&mut self, rect: Rect, label: &str, on: bool, focused: bool) {
        super::draw_button(
            &mut self.canvas,
            self.env,
            self.style,
            rect,
            label,
            on,
            focused,
        );
    }

    /// A button that does nothing now.
    pub fn faint_button(&mut self, rect: Rect, label: &str, focused: bool) {
        super::draw_faint_button(&mut self.canvas, self.env, self.style, rect, label, focused);
    }

    /// A button with an icon; the ink of its icon.
    pub fn icon_button(&mut self, rect: Rect, name: &str, on: bool, focused: bool) -> Color {
        super::draw_icon_button(&mut self.canvas, self.style, rect, name, on, focused)
    }

    /// The focus round `rect`.
    pub fn focus(&mut self, rect: Rect) {
        super::draw_focus(&mut self.canvas, self.style, rect);
    }

    /// The line below a row.
    pub fn separator(&mut self, rect: Rect, indent: u16) {
        super::draw_separator(&mut self.canvas, self.style, rect, indent);
    }

    /// A heading over a group of rows.
    pub fn heading(&mut self, rect: Rect, text: &str) {
        super::draw_heading(&mut self.canvas, self.env, self.style, rect, text);
    }

    /// A switch in `row`.
    pub fn switch(&mut self, row: Rect, on: bool, inverted: bool) {
        super::draw_switch(&mut self.canvas, self.style, row, on, inverted);
    }

    /// Wrapped, centred text.
    pub fn message(&mut self, area: Rect, text: &str) {
        super::draw_message(&mut self.canvas, self.env, self.style, area, text);
    }

    /// A note at the bottom of a screen.
    pub fn note_line(&mut self, area: Rect, text: &str) {
        super::draw_note_line(&mut self.canvas, self.env, self.style, area, text);
    }

    /// What a screen with nothing to show says.
    pub fn empty(&mut self, area: Rect, icon: &str, title: &str, text: &str) {
        super::draw_empty(
            &mut self.canvas,
            self.env,
            self.style,
            area,
            icon,
            title,
            text,
        );
    }

    /// A card or frame round `area`; the room inside it.
    pub fn card(&mut self, area: Rect, frame: Frame) -> Rect {
        super::draw_card(&mut self.canvas, self.style, area, frame)
    }

    /// A progress bar.
    pub fn progress(&mut self, area: Rect, permille: u16, ink: Color) {
        super::draw_progress(&mut self.canvas, self.style, area, permille, ink);
    }

    /// The icon at the start of a row; the room right of it.
    pub fn row_icon(&mut self, rect: Rect, room: u16, icon: Option<&str>, ink: Color) -> Rect {
        super::draw_row_icon(
            &mut self.canvas,
            self.env,
            self.style,
            rect,
            room,
            icon,
            ink,
        )
    }

    /// A screen's header.
    pub fn header(&mut self, area: Rect, title: &str, note: Option<&str>) {
        super::draw_header(&mut self.canvas, self.env, self.style, area, title, note);
    }

    /// A field of text in a row.
    pub fn text_field(
        &mut self,
        row: Rect,
        focused: bool,
        title: &str,
        value: &str,
        placeholder: &str,
    ) {
        super::draw_text_field(
            &mut self.canvas,
            self.env,
            self.style,
            row,
            focused,
            title,
            value,
            placeholder,
        );
    }
}
