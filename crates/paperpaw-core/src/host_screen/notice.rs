//! The host's notice (`docs/abi/ui.md`, *Messages*): what `command.notify`
//! and the kernel's own messages show where no plugin's screen fills the
//! notice slot ([`slots::NOTICE`](paperpaw_sdk::builtin::slots::NOTICE)),
//! as with native plugins, whose menu has no notice of its own (#257). The
//! look is Beta 3's: a dark box with rounded corners above the bottom of the
//! screen, between the margins, the text centred in white in at most three
//! lines. The kernel shows it over whatever is open and takes it away after
//! `core.notice-seconds`.

use crate::Place;
use alloc::string::String;
use alloc::vec::Vec;

use super::paint::{self, Look};
use paperpaw_sdk::builtin::params;
use paperpaw_sdk::{Canvas, Color, Id, Params, Rect};

/// The screen's id: a host screen, bound to the notice slot as a fallback.
pub(crate) const SCREEN: Id = Id::new_static("core.notice-box");
/// Its title, for lists of screens.
pub(crate) const TITLE: &str = "core.notice-box";
/// Lines of a message, at most.
const LINES: usize = 3;

/// A notice: the lines it shows and where.
pub(crate) struct Notice {
    look: Look,
    region: Rect,
    lines: Vec<String>,
}

impl Notice {
    /// The notice of `params`' text. Its allocations (the lines) are
    /// infallible, as Beta 3's were: accepted (Code Reviewer, #1270), since
    /// the text is bounded by the wire record (at most 256 bytes) and a notice
    /// exists only while it shows.
    pub(crate) fn new(env: &Place<'_>, params: &Params) -> Self {
        let look = Look::of(env.device(), env.settings(), env.fonts());
        let display = env.device().display;
        let pad = look.gap * 3 / 2;
        let width = display.width.saturating_sub(2 * look.margin);
        let inner = f32::from(width.saturating_sub(2 * pad));
        let text = params.get(params::TEXT).unwrap_or_default();
        let (lines, line_height) = match paint::font(env.fonts()) {
            Some(font) => {
                let mut lines = paint::wrapped(&font, look.text, text, inner);
                if lines.len() > LINES {
                    // The rest on the last line, shortened.
                    let rest = lines[LINES - 1..].join(" ");
                    lines.truncate(LINES);
                    lines[LINES - 1] = paint::fit(&font, look.text, &rest, inner);
                }
                (lines, font.line_metrics(look.text).line_height as u16)
            }
            None => (Vec::new(), look.row),
        };
        let count = u16::try_from(lines.len().max(1)).unwrap_or(1);
        let height = line_height
            .saturating_mul(count)
            .saturating_add(pad.saturating_mul(2));
        // Above the reader's status line, out of the way of the page.
        let bottom = display.height.saturating_sub(look.row);
        let region = Rect::new(look.margin, bottom.saturating_sub(height), width, height);
        Self {
            look,
            region,
            lines,
        }
    }

    /// Where it shows.
    pub(super) fn region(&self) -> Rect {
        self.region
    }

    /// A dark box with round corners, the lines centred in white.
    pub(super) fn draw(&self, canvas: &mut Canvas<'_>, env: &Place<'_>) {
        let look = &self.look;
        let r = self.region;
        // Round, with what is below in the corners; never quite square.
        let corner = look.corner.max(u32::from(look.gap) / 2);
        canvas.fill_round(r, corner, Color::BLACK);
        let Some(font) = paint::font(env.fonts()) else {
            return;
        };
        let line_height = font.line_metrics(look.text).line_height as u16;
        let mut y = r.y.saturating_add(look.gap * 3 / 2);
        for line in &self.lines {
            // Each line said from where it starts, as Beta 3's were.
            let width = font.measure(look.text, line);
            let x = f32::from(r.x) + (f32::from(r.width) - width) / 2.0;
            let row = Rect::new(r.x, y, r.width, line_height);
            paint::line_at(canvas, &font, look.text, line, x, row, Color::WHITE);
            y = y.saturating_add(line_height);
        }
    }
}

#[cfg(test)]
impl Notice {
    /// The lines it shows.
    pub(crate) fn lines(&self) -> &[String] {
        &self.lines
    }
}
