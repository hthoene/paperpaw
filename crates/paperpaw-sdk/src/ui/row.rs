//! A row of a list as a value: [`Row`].

use paperpaw_platform::{Point, Rect};

use crate::env::Env;

use super::{Ui, UiStyle};

/// A row of a list as a value: its title, what it shows beside it (a
/// value, a switch, a number with `−` and `+`), a note below. The same
/// value says how high the row is, draws it and tells what a point of it
/// is, so that the three cannot disagree.
///
/// ```
/// use paperpaw_sdk::ui::Row;
///
/// let wifi = Row::switch("Wi-Fi", true).note("Off while reading");
/// let size = Row::number("Size", "12", "000").focused(true);
/// let about = Row::value("Version", "0.5.0");
/// # let _ = (wifi, size, about);
/// ```
///
/// It borrows its words; measuring and drawing it allocate nothing of
/// their own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use = "a row does nothing until it is drawn or measured"]
pub struct Row<'r> {
    title: &'r str,
    kind: RowKind<'r>,
    note: &'r str,
    focused: bool,
}

/// What a [`Row`] shows beside its title.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RowKind<'r> {
    /// A value at the right (none where empty).
    Value(&'r str),
    /// A switch at the right, on or off.
    Switch(bool),
    /// A number between `−` and `+`, with room for `room` (the widest
    /// value of rows above each other, so their buttons line up), the
    /// `−` (`Some(false)`) or the `+` (`Some(true)`) focused.
    Number {
        value: &'r str,
        room: &'r str,
        focus: Option<bool>,
    },
}

/// What a point of a [`Row`] is on ([`Row::hit`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RowPart {
    /// The row itself (its title, its value).
    Row,
    /// Its switch.
    Switch,
    /// The `−` of its number.
    Minus,
    /// The `+` of its number.
    Plus,
}

impl<'r> Row<'r> {
    /// A row of a title alone.
    pub const fn plain(title: &'r str) -> Self {
        Self::value(title, "")
    }

    /// A row of a title and a value at the right.
    pub const fn value(title: &'r str, value: &'r str) -> Self {
        Self::of(title, RowKind::Value(value))
    }

    /// A row of a title and a switch.
    pub const fn switch(title: &'r str, on: bool) -> Self {
        Self::of(title, RowKind::Switch(on))
    }

    /// A row of a title and a number between `−` and `+`, with room for
    /// `room`.
    pub const fn number(title: &'r str, value: &'r str, room: &'r str) -> Self {
        Self::of(
            title,
            RowKind::Number {
                value,
                room,
                focus: None,
            },
        )
    }

    /// A row of `title` showing `kind`.
    pub const fn of(title: &'r str, kind: RowKind<'r>) -> Self {
        Self {
            title,
            kind,
            note: "",
            focused: false,
        }
    }

    /// With `note` below the title (none where empty).
    pub const fn note(mut self, note: &'r str) -> Self {
        self.note = note;
        self
    }

    /// Focused (or not): drawn on the focus, in its ink.
    pub const fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    /// For a number: its `−` (`false`) or `+` (`true`) focused.
    pub const fn step_focus(mut self, focus: Option<bool>) -> Self {
        if let RowKind::Number { focus: f, .. } = &mut self.kind {
            *f = focus;
        }
        self
    }

    /// What it shows beside its title.
    pub const fn kind(&self) -> RowKind<'r> {
        self.kind
    }

    /// How high it is `width` wide: a row (a noted one with a note),
    /// taller where its title takes more lines beside what it shows.
    pub fn height(&self, env: &Env<'_>, style: &UiStyle, width: u16) -> u16 {
        match self.kind {
            RowKind::Value(value) => {
                super::noted_row_height(env, style, width, self.title, value, self.note)
            }
            RowKind::Switch(_) => {
                super::switch_row_height(env, style, width, self.title, self.note)
            }
            RowKind::Number { value, .. } => {
                super::noted_row_height(env, style, width, self.title, value, self.note)
            }
        }
    }

    /// Draws it in `rect`.
    pub fn draw(&self, ui: &mut Ui<'_>, rect: Rect) {
        let (env, style) = (ui.env(), ui.style());
        let (title, note, focused) = (self.title, self.note, self.focused);
        match self.kind {
            RowKind::Value(value) => {
                super::draw_noted_row(ui.canvas(), env, style, rect, focused, title, value, note);
            }
            RowKind::Switch(on) => {
                super::draw_switch_row(ui.canvas(), env, style, rect, focused, title, note, on);
            }
            RowKind::Number { value, room, focus } => {
                // With a note, the number beside the title above it.
                let line = if note.is_empty() {
                    rect
                } else {
                    super::title_area(style, rect)
                };
                let lines = super::RowLines::one(line);
                super::draw_number_lines(ui.canvas(), env, style, lines, title, value, room, focus);
                if !note.is_empty() {
                    super::draw_lines_note(ui.canvas(), env, style, rect, lines, focused, note);
                }
            }
        }
    }

    /// What `point` is on, the row drawn in `rect`; none outside it.
    pub fn hit(&self, env: &Env<'_>, style: &UiStyle, rect: Rect, point: Point) -> Option<RowPart> {
        if !rect.contains(point) {
            return None;
        }
        match self.kind {
            RowKind::Switch(_) if super::switch_rect(style, rect).contains(point) => {
                Some(RowPart::Switch)
            }
            RowKind::Number { value, room, .. } => {
                let line = if self.note.is_empty() {
                    rect
                } else {
                    super::title_area(style, rect)
                };
                match super::value_stepper(env, style, line, room, value) {
                    Some((minus, _)) if minus.contains(point) => Some(RowPart::Minus),
                    Some((_, plus)) if plus.contains(point) => Some(RowPart::Plus),
                    _ => Some(RowPart::Row),
                }
            }
            _ => Some(RowPart::Row),
        }
    }
}
