//! *Today's reading cat* as the sleep screen (a mode the sleep screen
//! offers, `reading-goal.sleep`): the cat of today's mood large in the
//! middle on white, the minutes read and the goal below it, in grays where
//! the panel can. The picture is drawn from its decoded file in bands.

use alloc::string::String;

use paperpaw_sdk::document::ImageRef;
use paperpaw_sdk::semantics::Role;
use paperpaw_sdk::ui::{self, UiStyle};
use paperpaw_sdk::{Canvas, Color, Cx, Env, Handled, Id, Rect, Screen};

use crate::{Kept, Today, pictures, widget};

pub(crate) struct GoalSleep {
    image: Option<ImageRef>,
    /// "12 / 20 min", and what the picture shows.
    text: String,
    described: String,
}

impl GoalSleep {
    pub(crate) fn new(env: &Env<'_>, kept: &Kept) -> Self {
        let today = Today::now(env, kept);
        Self {
            image: pictures::of(env, today.mood),
            text: widget::progress(env, &today),
            described: widget::described(env, &today),
        }
    }
}

impl Screen for GoalSleep {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let display = env.device().display;
        let screen = Rect::new(0, 0, display.width, display.height);
        canvas.fill(screen, Color::WHITE);
        let style = UiStyle::new(env);
        let mut ui = ui::Ui::new(canvas, env, &style);
        let Some(font) = ui.font() else {
            return;
        };
        let margin = style.margin();
        let line = font.line_metrics(style.text()).line_height as u16;
        let gap = style.gap() * 3;
        // The picture as large as leaves room for the line, both together
        // in the middle.
        let room_w = display.width.saturating_sub(2 * margin);
        let room_h = display
            .height
            .saturating_sub(2 * margin + gap + line)
            .min(room_w);
        let (w, h) = match &self.image {
            Some(image) => ui::fit_image(image, room_w, room_h),
            None => (0, 0),
        };
        let top = display.height.saturating_sub(h + gap + line) / 2;
        if let Some(image) = &self.image {
            let area = Rect::new((display.width - w) / 2, top, w, h);
            ui::draw_image(ui.canvas(), env.storage(), image, area);
            ui.canvas().describe(Role::Image, &self.described, area);
        }
        if self.image.is_none() {
            // No cat on the card: its mood in words above the minutes.
            let above = Rect::new(margin, top.saturating_sub(line), room_w, line);
            widget::says(&mut ui, &font, &style, &self.described, above, Color::BLACK);
        }
        let width = f32::from(room_w);
        let text = ui::fit(&font, style.text(), &self.text, width);
        let x = f32::from(margin) + (width - font.measure(style.text(), &text)).max(0.0) / 2.0;
        let row = Rect::new(margin, top + h + gap, room_w, line);
        ui::Label::new(&text)
            .px(style.text())
            .color(Color::BLACK)
            .draw_from(&mut ui, x, row);
    }

    /// Buttons are the sleep screen's (it shows this one in its place).
    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }

    /// The picture in grays where the panel can.
    fn grays(&self) -> bool {
        self.image.is_some()
    }
}
