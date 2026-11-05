//! The boundary as it is now: Rust types, Rust calling convention.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::Cell;

use paperpaw_graphics::Canvas;
use paperpaw_platform::{Color, Point, Rect};
use paperpaw_sdk::Command;
use paperpaw_text::{Font, GlyphCache, TextStyle};

use crate::widget::Gfx;

/// The shared functions a plugin calls in the firmware (as the family
/// tables are used, `paperpaw_table`): pointers to the real functions.
pub struct RustyTable {
    pub fill_rect: fn(&mut Canvas<'_>, i32, i32, u32, u32, Color),
    pub draw_line: fn(&mut Canvas<'_>, Point, Point, u32, Color),
    pub fill_round: fn(&mut Canvas<'_>, Rect, u32, Color),
    pub stroke_round: fn(&mut Canvas<'_>, Rect, u32, u32, Color),
    pub measure: fn(&Font, f32, &str) -> f32,
    pub draw_text: fn(&mut Canvas<'_>, &mut GlyphCache, &TextStyle<'_>, f32, i32, &str) -> f32,
}

/// The firmware's table.
pub static RUSTY: RustyTable = RustyTable {
    fill_rect: |c, x, y, w, h, col| c.fill_rect(x, y, w, h, col),
    draw_line: |c, a, b, w, col| c.draw_line(a, b, w, col),
    fill_round: |c, r, rad, col| c.fill_round(r, rad, col),
    stroke_round: |c, r, t, rad, col| c.stroke_round(r, t, rad, col),
    measure: paperpaw_text::measure,
    draw_text: paperpaw_text::draw_text,
};

/// What a plugin draws with: the table, and what the SDK's `Canvas` and
/// font handle hold (Rust references cross).
pub struct RustyGfx<'a, 'c> {
    pub table: &'a RustyTable,
    pub canvas: &'a mut Canvas<'c>,
    pub font: &'a Font,
    pub cache: &'a mut GlyphCache,
}

impl Gfx for RustyGfx<'_, '_> {
    fn fill_rect(&mut self, x: i32, y: i32, w: u32, h: u32, color: Color) {
        (self.table.fill_rect)(self.canvas, x, y, w, h, color);
    }

    fn draw_line(&mut self, from: Point, to: Point, width: u32, color: Color) {
        (self.table.draw_line)(self.canvas, from, to, width, color);
    }

    fn fill_round(&mut self, area: Rect, radius: u32, color: Color) {
        (self.table.fill_round)(self.canvas, area, radius, color);
    }

    fn stroke_round(&mut self, area: Rect, thickness: u32, radius: u32, color: Color) {
        (self.table.stroke_round)(self.canvas, area, thickness, radius, color);
    }

    fn measure(&mut self, size: f32, text: &str) -> f32 {
        (self.table.measure)(self.font, size, text)
    }

    fn text(&mut self, size: f32, x: f32, baseline: i32, text: &str, color: Color) -> f32 {
        let style = TextStyle::new(self.font, 0, size, color);
        (self.table.draw_text)(self.canvas, self.cache, &style, x, baseline, text)
    }
}

/// An event a plugin hears.
pub struct Event<'a> {
    pub kind: u32,
    pub payload: &'a str,
}

/// A plugin's handler as the SDK keeps it: a boxed closure.
pub type Handler = Box<dyn Fn(&Event<'_>, &mut Vec<Command>)>;

/// The prototype's plugin: counts the books opened, and says so.
pub fn plugin() -> Handler {
    let opened = Cell::new(0u32);
    Box::new(move |event, out| {
        if event.kind == 1 {
            opened.set(opened.get() + 1);
            let mut text = String::with_capacity(16 + event.payload.len());
            text.push_str("Opened: ");
            text.push_str(event.payload);
            out.push(Command::Notify(text));
        }
    })
}
