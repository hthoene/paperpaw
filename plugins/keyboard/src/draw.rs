//! How the keyboard is drawn: the text field, the keys and the
//! variants of a letter above it.

#[allow(
    clippy::wildcard_imports,
    reason = "a part of the plugin, which shares its imports"
)]
use super::*;

impl KeyboardScreen {
    pub(super) fn draw_field(&self, ui: &mut ui::Ui<'_>) {
        // Only in the band it lies in: its text is measured to fit.
        if self.field.intersection(&ui.canvas().clip()).is_none() {
            return;
        }
        let (env, style) = (ui.env(), self.style);
        let Some(font) = style.font(env) else {
            return;
        };
        let margin = style.margin() as f32;
        let f = self.field;
        let title_height = (style.small() * 1.5) as u16;
        let title_area = Rect::new(f.x, f.y, f.width, title_height);
        // With buttons, on a letter that holds others: that it does, at
        // the right end; else how typing ends (held Back, [`DONE`]).
        let held = self
            .held_hint(env)
            .or_else(|| (!self.touch).then(|| env.i18n().tr("keyboard.hold-back").to_string()));
        let held_width = held
            .as_ref()
            .map_or(0.0, |h| font.measure(style.small(), h) + margin);
        if let Some(held) = &held {
            let x = f32::from(f.x + f.width) - margin - font.measure(style.small(), held);
            ui::Label::new(held)
                .px(style.small())
                .color(Color::BLACK)
                .draw_from(ui, x, title_area);
        }
        let title = ui::fit(
            &font,
            style.small(),
            &self.title,
            f32::from(f.width) - 2.0 * margin - held_width,
        );
        ui::Label::new(&title)
            .px(style.small())
            .color(style.faint(ui::Ink::MUTED))
            .draw_from(ui, margin, title_area);
        let box_area = self.text_box();
        ui::stroke_round(
            ui.canvas(),
            &style,
            box_area,
            u32::from(style.line()).max(2),
            Color::BLACK,
        );
        // What is typed in it, for screen readers (dots of a password say
        // nothing).
        ui.canvas()
            .describe(paperpaw_sdk::semantics::Role::Field, "", box_area);
        // The end of the text, with a cursor after it; a password as dots.
        let text = if self.password {
            "•".repeat(self.text.chars().count())
        } else {
            self.text.clone()
        };
        let buttons_width = self.buttons.first().map_or(0.0, |(_, area)| {
            f32::from(box_area.x + box_area.width - area.x) + margin
        });
        let room = f32::from(box_area.width) - margin * 1.5 - buttons_width;
        // The longest end of it that fits, found by halving.
        let starts: Vec<usize> = text.char_indices().map(|(i, _)| i).collect();
        let fits = |n: usize| font.measure(style.text(), &text[starts[n]..]) <= room;
        let (mut low, mut high) = (0, starts.len());
        while low < high {
            let mid = (low + high) / 2;
            if fits(mid) {
                high = mid;
            } else {
                low = mid + 1;
            }
        }
        let start = starts.get(low).copied().unwrap_or(text.len());
        let x = f32::from(box_area.x) + margin / 2.0;
        let shown = &text[start..];
        ui::Label::new(shown)
            .px(style.text())
            .color(Color::BLACK)
            .draw_from(ui, x, box_area);
        let end = x + font.measure(style.text(), shown);
        let cursor = style.text() as u32;
        ui.canvas().fill_rect(
            end as i32 + 2,
            i32::from(box_area.y) + (i32::from(box_area.height) - cursor as i32) / 2,
            u32::from(style.line()).max(2),
            cursor,
            Color::BLACK,
        );
        for (index, &(kind, area)) in self.buttons.iter().enumerate() {
            let focused = self.focus == Some((PASTE_ROW, index));
            let label = env.i18n().tr(kind.label());
            ui.button(area, label, false, focused);
        }
    }

    /// The style keys `height` high are drawn in, by their shape: round
    /// (a letter's a circle, wider ones a pill), or with soft corners.
    fn key_style(&self, height: u16) -> UiStyle {
        let mut style = self.style;
        if !self.touch {
            // Ten narrow keys a row: soft corners, not round ends, leave
            // their letters the whole width, however high they are.
            return style
                .with_text(style.text() * self.letters as f32 / 100.0)
                .with_pill(false)
                .with_corner((style.text() / 4.0) as u16);
        }
        match self.shape.as_str() {
            SHAPE_ROUND => {
                style = style.with_pill(true).with_row(style.row().max(height));
            }
            SHAPE_WIDE => {
                style = style
                    .with_pill(false)
                    .with_corner(height / 4)
                    .with_row(style.row().max(height));
            }
            _ => {}
        }
        style
    }

    pub(super) fn draw_key(&self, ui: &mut ui::Ui<'_>, row: usize, index: usize) {
        let (env, style) = (ui.env(), self.style);
        let key = self.rows[row][index].0;
        let area = self.key_area(row, index);
        let focused = self.focus == Some((row, index));
        // Buttons as everywhere. Tapped, they fill their place but for a
        // seam, the focused one (after a button was used) smaller by its
        // focus frame, which stays in its place. Chosen with buttons, they
        // fill it but for a pixel on each side, and the focused one is
        // filled: no frame round it needs room, so ten keys a row are wide
        // on the X4.
        let frame = ui::button_border(&style) * 3;
        let pad = if focused && self.touch {
            frame
        } else {
            self.key_inset()
        } as u16;
        let h = area.height.saturating_sub(2 * pad);
        let r = Rect::new(
            area.x + pad,
            area.y + (area.height - h) / 2,
            area.width.saturating_sub(2 * pad),
            h,
        );
        // The key's own style: its shape.
        let style = self.key_style(r.height);
        // Within its place: chosen with buttons, within the key itself, so
        // that the focus is its fill alone (the frame a button draws round
        // it would reach into the next key).
        let mut canvas = ui.canvas().clipped(if self.touch { area } else { r });
        let mut ui = ui::Ui::new(&mut canvas, env, &style);
        // Shift while capitals are locked (one capital, the letters show
        // it), and *OK* always: the key that ends typing stands out.
        let active = (key == Key::Shift && self.shift == Shift::Locked) || key == Key::Done;
        // With buttons the focus fills the key; a key filled already gets
        // a light line inside.
        let on = active || (focused && !self.touch);
        // Filled for the focus alone: not said to be on.
        let filled_only = on && !active;
        let framed = focused;
        let ring = !self.touch && focused && active;
        let name = match key {
            Key::Shift => "keyboard.shift",
            Key::Delete => "keyboard.delete",
            Key::Space => "keyboard.space",
            _ => {
                let label = self.label(env, key);
                if filled_only {
                    ui.filled_button(r, &label, framed);
                } else {
                    ui.button(r, &label, on, framed);
                }
                if ring {
                    inner_ring(&mut ui, r);
                }
                return;
            }
        };
        // The key drawn: an arrow, a space.
        let name = env.i18n().tr(name);
        let ink = if filled_only {
            ui.filled_icon_button(r, name, framed)
        } else {
            ui.icon_button(r, name, on, framed)
        };
        if ring {
            inner_ring(&mut ui, r);
        }
        let size = (style.text() * 0.6) as i32;
        let (cx, cy) = (
            i32::from(r.x) + i32::from(r.width) / 2,
            i32::from(r.y) + i32::from(r.height) / 2,
        );
        // The icon a theme gave the key, else drawn here.
        let icon = match key {
            Key::Shift => paperpaw_sdk::icons::SHIFT,
            Key::Delete => paperpaw_sdk::icons::DELETE,
            _ => paperpaw_sdk::icons::SPACE,
        };
        let canvas = ui.canvas();
        if ui::draw_icon_at(canvas, env, icon, style.text(), (cx, cy), ink) {
            return;
        }
        match key {
            Key::Shift => arrow_up(canvas, cx, cy, size, ink),
            Key::Delete => arrow_left(canvas, cx, cy, size, ink),
            Key::Space => {
                let w = i32::from(r.width) / 3;
                let t = u32::from(style.line()).max(2) as i32;
                canvas.fill_rect(cx - w / 2, cy + size / 3, w as u32, t as u32, ink);
                canvas.fill_rect(cx - w / 2, cy, t as u32, (size / 3 + t) as u32, ink);
                canvas.fill_rect(cx + w / 2 - t, cy, t as u32, (size / 3 + t) as u32, ink);
            }
            _ => {}
        }
    }

    pub(super) fn draw_variants(&self, ui: &mut ui::Ui<'_>) {
        let Some(popup) = &self.popup else {
            return;
        };
        let style = self.style;
        let a = popup.area;
        let border = (u32::from(style.line()) * 2).max(2);
        let out = border as u16 * 2;
        let frame = Rect::new(
            a.x.saturating_sub(out),
            a.y.saturating_sub(out),
            a.width + 2 * out,
            a.height + 2 * out,
        );
        ui.canvas().fill(frame, Color::WHITE);
        ui.canvas().stroke(frame, border, Color::BLACK);
        for (index, c) in popup.chars.iter().enumerate() {
            let key = popup.key(index);
            let h = ui::button_height(&style, key);
            let pad = ui::button_border(&style) as u16 * 3;
            let r = Rect::new(
                key.x + pad,
                key.y + (key.height - h) / 2,
                key.width.saturating_sub(2 * pad),
                h,
            );
            let focused = popup.focus == Some(index);
            ui.button(r, &c.to_string(), false, focused);
        }
    }
}

/// A light line just inside a filled key `r`: the focus on a key that is
/// filled already (*OK*, *Shift* while locked).
fn inner_ring(ui: &mut ui::Ui<'_>, r: Rect) {
    let style = *ui.style();
    let border = ui::button_border(&style);
    let inset = (border * 2) as u16;
    let inner = Rect::new(
        r.x + inset,
        r.y + inset,
        r.width.saturating_sub(2 * inset),
        r.height.saturating_sub(2 * inset),
    );
    ui::stroke_round(ui.canvas(), &style, inner, border, Color::WHITE);
}

/// An arrow pointing up (*Shift*), centred at `(x, y)`.
pub(crate) fn arrow_up(canvas: &mut Canvas<'_>, x: i32, y: i32, size: i32, color: Color) {
    let half = size / 2;
    for dy in 0..=half {
        canvas.fill_rect(x - dy, y - half + dy, (dy * 2 + 1) as u32, 1, color);
    }
    let stem = (size / 4).max(2);
    canvas.fill_rect(x - stem / 2, y, stem as u32, half as u32, color);
}

/// An arrow pointing left (*Delete*), centred at `(x, y)`.
pub(crate) fn arrow_left(canvas: &mut Canvas<'_>, x: i32, y: i32, size: i32, color: Color) {
    let half = size / 2;
    for dx in 0..=half {
        canvas.fill_rect(x - half + dx, y - dx, 1, (dx * 2 + 1) as u32, color);
    }
    let stem = (size / 4).max(2);
    canvas.fill_rect(x, y - stem / 2, half as u32, stem as u32, color);
}
