//! A notice box for the hosted tests of plugins that give notices: the dark
//! rounded box the interface-6 menu drew above the bottom of the screen,
//! bound to `core.notice`. Test-only; the product's notice is the host's
//! (`core.notice-box`).
#![allow(dead_code, reason = "shared by the tests; not every one uses it")]

const NOTICE: Id = Id::new_static("test.notice");

/// Draws the notices.
#[derive(Debug, Default)]
pub struct NoticeBox;

static SCREENS: [FixedScreen; 1] = [FixedScreen::new(NOTICE, "test.notice", |env, params| {
    Box::new(NoticeScreen::new(env, params))
})];

impl Plugin for NoticeBox {
    fn manifest(&self) -> Manifest {
        Manifest::new(
            Id::new_static("test-notice"),
            "Notice box",
            paperpaw_sdk::SDK_VERSION,
        )
        .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_screens(&SCREENS)?;
        r.bind_slot(slots::NOTICE, NOTICE, Priority::DEFAULT)?;
        Ok(())
    }
}

extern crate alloc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{params, slots};
use paperpaw_sdk::ui::{self, UiStyle};
use paperpaw_sdk::{
    Canvas, Color, Cx, Env, FixedScreen, Handled, Id, Manifest, Params, Plugin, Priority, Rect,
    RegisterError, Registrar, Screen,
};

/// Lines of a message, at most.
const LINES: usize = 3;

struct NoticeScreen {
    style: UiStyle,
    region: Rect,
    lines: Vec<String>,
}

impl NoticeScreen {
    fn new(env: &Env<'_>, params: &Params) -> Self {
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
                    .map(|r| text[r.clone()].trim().to_string())
                    .collect();
                if ranges.len() > LINES
                    && let (Some(last), Some(range)) = (lines.last_mut(), ranges.get(LINES - 1))
                {
                    *last = ui::fit(&font, style.text(), text[range.start..].trim(), inner);
                }
                (lines, font.line_metrics(style.text()).line_height as u16)
            }
            None => (Vec::new(), style.row()),
        };
        let height = line_height * lines.len().max(1) as u16 + 2 * pad;
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

impl Screen for NoticeScreen {
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
        let mut y = r.y + style.gap() * 3 / 2;
        for line in &self.lines {
            let width = font.measure(style.text(), line);
            let x = f32::from(r.x) + (f32::from(r.width) - width) / 2.0;
            let area = Rect::new(r.x, y, r.width, line_height);
            ui::Label::new(line)
                .px(style.text())
                .color(Color::WHITE)
                .draw_from(&mut ui, x, area);
            y += line_height;
        }
    }

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }
}
