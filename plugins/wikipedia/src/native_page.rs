//! What the summary screen shows, without the host: the text wrapped into
//! lines, a page of lines at a time, and what an input means to it.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Point, Rect,
    native::{
        Error,
        callbacks::{Input, InputGestureKind},
    },
};
use paperpaw_ui_kit::text::wrap_with;

/// Lines kept at most; a longer text ends in "…".
pub const MAX_LINES: usize = 600;

/// What an input meant to the screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Not for the screen.
    Ignored,
    /// The first line shown is now this one.
    Scroll(usize),
    /// The screen should close.
    Close,
    /// A scroll key at the first or last page: taken, nothing moves (it must
    /// not reach the page below).
    Edge,
}

/// The lines of a text and the one shown first.
#[derive(Debug, Default)]
pub struct Page {
    lines: Vec<String>,
    top: usize,
}

impl Page {
    /// `text` wrapped to `width` as `measure` measures; a blank line of the
    /// text stays a blank line.
    ///
    /// # Errors
    /// `NoMemory` or the measure's error.
    pub fn new(
        text: &str,
        mut measure: impl FnMut(&str) -> Result<f32, Error>,
        width: f32,
    ) -> Result<Self, Error> {
        let mut lines: Vec<String> = Vec::new();
        for paragraph in text.split('\n') {
            if lines.len() >= MAX_LINES {
                break;
            }
            if paragraph.trim().is_empty() {
                lines.try_reserve(1).map_err(|_| Error::NoMemory)?;
                lines.push(String::new());
                continue;
            }
            let room = MAX_LINES - lines.len();
            let wrapped = wrap_with(&mut measure, paragraph, width, room)?;
            lines
                .try_reserve(wrapped.len())
                .map_err(|_| Error::NoMemory)?;
            lines.extend(wrapped);
        }
        Ok(Self { lines, top: 0 })
    }

    /// Scrolls so that line `top` is the first shown.
    pub fn set_top(&mut self, top: usize) {
        self.top = top.min(self.lines.len().saturating_sub(1));
    }

    /// The lines of the page of `rows` shown.
    pub fn page(&self, rows: usize) -> impl Iterator<Item = &str> {
        self.lines
            .iter()
            .skip(self.top)
            .take(rows)
            .map(String::as_str)
    }

    /// What `input` means; `region` is where the screen is drawn and `rows`
    /// the lines it holds. Touch: the right part scrolls on, the left part
    /// goes back (up, or closes on the first page), outside closes.
    #[must_use]
    pub fn decide(&self, rows: usize, region: Rect, input: &Input<'_>) -> Step {
        let rows = rows.max(1);
        let forward = || {
            let next = self.top + rows;
            if next < self.lines.len() {
                Step::Scroll(next)
            } else {
                Step::Edge
            }
        };
        let back = || {
            if self.top == 0 {
                Step::Edge
            } else {
                Step::Scroll(self.top.saturating_sub(rows))
            }
        };
        match *input {
            Input::Action { action, .. } => match action {
                "core.next-page" | "core.down" | "core.next" | "core.right" => forward(),
                "core.prev-page" | "core.up" | "core.previous" | "core.left" => back(),
                "core.confirm" | "core.back" => Step::Close,
                _ => Step::Ignored,
            },
            Input::Gesture {
                position,
                kind: InputGestureKind::Tap,
                ..
            } => Self::tap(region, position, forward, back),
            _ => Step::Ignored,
        }
    }

    fn tap(
        region: Rect,
        position: Point,
        forward: impl Fn() -> Step,
        back: impl Fn() -> Step,
    ) -> Step {
        if !region.contains(position) {
            return Step::Close;
        }
        let middle = i32::from(region.x) + i32::from(region.width) / 2;
        let left = position.x < middle;
        let step = if left { back() } else { forward() };
        // On the first page the left part has nothing to go back to: it closes.
        if step == Step::Edge && left {
            Step::Close
        } else {
            step
        }
    }
}

#[cfg(test)]
mod tests;
