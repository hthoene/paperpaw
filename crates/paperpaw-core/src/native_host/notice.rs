//! The host's notice (`docs/abi/ui.md`, *Messages*): what `command.notify`
//! and the kernel's own messages show where no plugin's screen fills the
//! notice slot ([`slots::NOTICE`](paperpaw_sdk::builtin::slots::NOTICE)),
//! as with native plugins, whose menu has no notice of its own (#257). The
//! look is Beta 3's: a dark box with rounded corners above the bottom of the
//! screen, between the margins, the text centred in white in at most three
//! lines. The kernel shows it over whatever is open and takes it away after
//! `core.notice-seconds`.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::params;
use paperpaw_sdk::ui::{self, UiStyle};
use paperpaw_sdk::{Canvas, Color, Cx, Env, Handled, Id, Params, Rect, Screen};

/// The screen's id: a host screen, bound to the notice slot as a fallback.
pub(crate) const SCREEN: Id = Id::new_static("core.notice-box");
/// Its title, for lists of screens.
pub(crate) const TITLE: &str = "core.notice-box";
/// Lines of a message, at most.
const LINES: usize = 3;

/// A notice: the lines it shows and where.
pub(crate) struct Notice {
    style: UiStyle,
    region: Rect,
    lines: Vec<String>,
}

impl Notice {
    pub(crate) fn new(env: &Env<'_>, params: &Params) -> Self {
        let style = UiStyle::new(env);
        let display = env.device().display;
        let margin = style.margin();
        let pad = style.gap() * 3 / 2;
        let width = display.width.saturating_sub(2 * margin);
        let inner = f32::from(width.saturating_sub(2 * pad));
        let text = params.get(params::TEXT).unwrap_or_default();
        let (lines, line_height) = match style.font(env) {
            Some(font) => {
                let ranges = font.wrap(style.text(), text, inner);
                let mut lines: Vec<String> = ranges
                    .iter()
                    .take(LINES)
                    .filter_map(|r| text.get(r.clone()))
                    .map(|line| line.trim().to_string())
                    .collect();
                if ranges.len() > LINES
                    && let (Some(last), Some(rest)) = (
                        lines.last_mut(),
                        ranges.get(LINES - 1).and_then(|r| text.get(r.start..)),
                    )
                {
                    *last = ui::fit(&font, style.text(), rest.trim(), inner);
                }
                (lines, font.line_metrics(style.text()).line_height as u16)
            }
            None => (Vec::new(), style.row()),
        };
        let count = u16::try_from(lines.len().max(1)).unwrap_or(1);
        let height = line_height
            .saturating_mul(count)
            .saturating_add(pad.saturating_mul(2));
        // Above the reader's status line, out of the way of the page.
        let bottom = display.height.saturating_sub(style.row());
        let region = Rect::new(margin, bottom.saturating_sub(height), width, height);
        Self {
            style,
            region,
            lines,
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

impl Screen for Notice {
    fn region(&self, _: &Env<'_>) -> Option<Rect> {
        Some(self.region)
    }

    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let mut ui = ui::Ui::new(canvas, env, &style);
        let r = self.region;
        // Round, with what is below in the corners; never quite square.
        let corner = u32::from(style.corner()).max(u32::from(style.gap()) / 2);
        ui.canvas().fill_round(r, corner, Color::BLACK);
        let Some(font) = ui.font() else {
            return;
        };
        let line_height = font.line_metrics(style.text()).line_height as u16;
        let mut y = r.y.saturating_add(style.gap() * 3 / 2);
        for line in &self.lines {
            let width = font.measure(style.text(), line);
            let x = f32::from(r.x) + (f32::from(r.width) - width) / 2.0;
            let area = Rect::new(r.x, y, r.width, line_height);
            ui::Label::new(line)
                .px(style.text())
                .color(Color::WHITE)
                .draw_from(&mut ui, x, area);
            y = y.saturating_add(line_height);
        }
    }

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }
}
