//! Concept D, "Panel": a clock, cards with thick round outlines, round
//! switches, a bar for the light and a dock of large icons. The focus is a
//! second outline.
use paperpaw_sdk::{Point, native::Error};
use paperpaw_ui_kit::widget::Align;

use crate::{
    data::BOOKS,
    kit::{B, Ui, state},
};

/// The dock at the bottom: home, the panel, the library. Returns what is
/// left above it. `here` is the screen shown, `focus` the icon focused.
fn dock(ui: &mut Ui<'_, '_>, here: usize, focus: Option<usize>) -> Result<B, Error> {
    let h = ((ui.u * 3.4) as i32).clamp(84, ui.screen.h / 7);
    let band = ui.screen.bottom(h + ui.pad / 2).top(h);
    let icons = ["home", "adjustments-horizontal", "books"];
    let room = band.inset_xy(ui.screen.w / 8, 0);
    for (index, icon) in icons.iter().enumerate() {
        let cell = room.col(index as i32, 3, 0);
        let radius = h * 42 / 100;
        let centre = cell.middle();
        let around = B::new(centre.x - radius, centre.y - radius, 2 * radius, 2 * radius);
        if index == here {
            ui.toggle(centre, radius, icon, true)?;
        } else {
            ui.icon(icon, radius as f32 * 1.4, around, ui.ink)?;
        }
        if focus == Some(index) {
            ui.ring(around, radius)?;
        }
    }
    Ok(ui.screen.above(h + ui.pad / 2, ui.gap / 2))
}

/// A card: a thick round outline, black where `on`. Returns its inside.
fn card(ui: &mut Ui<'_, '_>, b: B, on: bool, focused: bool) -> Result<B, Error> {
    let radius = ui.radius() * 3 / 2;
    let ink = ui.ink;
    if on {
        ui.plate(b, radius, ink)?;
    } else {
        ui.frame(b, radius, ui.stroke, ink)?;
    }
    if focused {
        ui.ring(b, radius)?;
    }
    Ok(b.inset_xy(ui.pad * 3 / 4, ui.stroke + 8))
}

/// A slim card: an icon, a word, and a smaller word at the right.
fn row_card(
    ui: &mut Ui<'_, '_>,
    b: B,
    (icon, label, note): (&str, &str, &str),
    focused: bool,
) -> Result<(), Error> {
    let inner = card(ui, b, false, focused)?;
    let ink = ui.ink;
    let picture = inner.left(inner.h.min(inner.w / 5));
    ui.icon(icon, picture.h as f32 * 1.1, picture, ink)?;
    let words = inner.right_of(picture.w, ui.gap / 2);
    let size = (ui.u * 1.2).min(words.h as f32 * 0.62);
    let small = ui.small.min(size * 0.8);
    let note_w = if note.is_empty() {
        0
    } else {
        ui.font.measure_text(small, note)? as i32 + ui.gap / 2
    };
    ui.bold(words.left(words.w - note_w), Align::Left, size, label, ink)?;
    if !note.is_empty() {
        ui.text(words, Align::Right, small, note, ink)?;
    }
    Ok(())
}

/// The segments of the digits 0 to 9: top, top right, bottom right, bottom,
/// bottom left, top left, middle.
const SEGMENTS: [u8; 10] = [63, 6, 91, 79, 102, 109, 125, 7, 127, 111];

/// The time in digits of round strokes, as tall as `b`: the X4's font packs
/// end near 48 pixels, so a larger clock is drawn.
fn clock(ui: &mut Ui<'_, '_>, b: B, time: &str) -> Result<(), Error> {
    let ink = ui.ink;
    let h = b.h;
    let t = (h / 8).max(4);
    let wide = h * 52 / 100;
    let apart = t * 3 / 2;
    let width = |c: char| if c == '1' || c == ':' { t } else { wide };
    let count = time.chars().count() as i32;
    let total: i32 = time.chars().map(width).sum::<i32>() + apart * (count - 1);
    let mut x = b.x + (b.w - total) / 2;
    for c in time.chars() {
        let w = width(c);
        if c == ':' {
            for at in [32, 68] {
                ui.plate(B::new(x, b.y + h * at / 100 - t / 2, t, t), t / 2, ink)?;
            }
        } else if let Some(digit) = c.to_digit(10) {
            let half = (h + t) / 2;
            let low = b.y + (h - t) / 2;
            let parts = [
                B::new(x, b.y, w, t),
                B::new(x + w - t, b.y, t, half),
                B::new(x + w - t, low, t, half),
                B::new(x, b.y + h - t, w, t),
                B::new(x, low, t, half),
                B::new(x, b.y, t, half),
                B::new(x, low, w, t),
            ];
            for (bit, part) in parts.iter().enumerate() {
                if SEGMENTS[digit as usize] & (1 << bit) != 0 {
                    ui.plate(*part, t / 2, ink)?;
                }
            }
        }
        x += w + apart;
    }
    Ok(())
}

/// Home: the clock, the book being read, the library and the store.
pub fn home(ui: &mut Ui<'_, '_>, focus: usize) -> Result<(), Error> {
    let rest = dock(ui, 0, focus.checked_sub(3))?;
    let area = B::new(
        rest.x + ui.pad,
        rest.y + ui.pad / 2,
        rest.w - 2 * ui.pad,
        rest.h - ui.pad / 2,
    );
    let ink = ui.ink;
    let apart = ui.gap.max(2 * ui.stroke + 10);
    let top = area.part(2, 22);
    clock(
        ui,
        top.centred(top.w, top.h.min(area.w * 26 / 100)),
        "12:40",
    )?;
    let date = area.part(24, 32);
    let small = ui.small.min(date.h as f32 * 0.7);
    ui.fit(
        date,
        Align::Center,
        small,
        ui.tr("Thursday, 19 November", "Donnerstag, 19. November"),
        ink,
    )?;
    let cards = area.part(36, 100);
    let book = &BOOKS[0];
    let reading = cards.part(0, 46);
    let inner = card(ui, reading, false, focus == 0)?;
    let label = ui.small.min(inner.h as f32 / 7.0);
    ui.fit(
        inner.part(0, 24),
        Align::Left,
        label,
        ui.tr("Continue reading", "Weiterlesen"),
        ink,
    )?;
    let title = (ui.u * 1.3).min(inner.h as f32 / 4.0);
    ui.bold(inner.part(24, 62), Align::Left, title, book.title, ink)?;
    let last = inner.part(66, 100);
    let read = state(ui, book)?;
    let read_w = ui.font.measure_text(label, &read)? as i32 + ui.gap / 2;
    let bar_h = (last.h * 6 / 10).clamp(12, 24);
    ui.bar(
        B::new(
            last.x,
            last.y + (last.h - bar_h) / 2,
            last.w - read_w,
            bar_h,
        ),
        book.percent,
        ink,
    )?;
    ui.bold(last, Align::Right, label, &read, ink)?;
    let rows = B::new(
        cards.x,
        reading.y + reading.h + apart,
        cards.w,
        cards.h - reading.h - apart,
    );
    let entries = [
        (
            "books",
            ui.tr("Library", "Bibliothek"),
            ui.tr("24 books", "24 Bücher"),
        ),
        (
            "cloud-download",
            ui.tr("Store", "Store"),
            ui.tr("2 updates", "2 Updates"),
        ),
    ];
    for (index, entry) in entries.iter().enumerate() {
        row_card(
            ui,
            rows.row(index as i32, 2, apart),
            *entry,
            focus == index + 1,
        )?;
    }
    Ok(())
}

/// A bar with an icon and a value: a thick pill filled from the left.
fn slider(
    ui: &mut Ui<'_, '_>,
    b: B,
    (icon, value, percent): (&str, &str, i32),
    focused: bool,
) -> Result<(), Error> {
    let (ink, paper) = (ui.ink, ui.paper);
    let radius = b.h / 2;
    ui.frame(b, radius, ui.stroke, ink)?;
    if focused {
        ui.ring(b, radius)?;
    }
    let inner = b.inset(ui.stroke + 3);
    let filled = inner.left((inner.w * percent / 100).max(inner.h));
    ui.plate(filled, inner.h / 2, ink)?;
    ui.icon(icon, inner.h as f32 * 0.62, filled.left(inner.h), paper)?;
    let size = (ui.u * 1.1).min(inner.h as f32 * 0.5);
    ui.bold(
        B::new(inner.x, inner.y, inner.w - inner.h / 2, inner.h),
        Align::Right,
        size,
        value,
        ink,
    )
}

/// The panel: round switches, the light, the text size, the battery,
/// sleep and lock.
pub fn panel(ui: &mut Ui<'_, '_>, focus: usize) -> Result<(), Error> {
    let rest = dock(ui, 1, focus.checked_sub(9))?;
    let area = B::new(
        rest.x + ui.pad,
        rest.y + ui.pad,
        rest.w - 2 * ui.pad,
        rest.h - ui.pad,
    );
    let ink = ui.ink;
    let apart = ui.gap.max(2 * ui.stroke + 10);
    let switches = [
        ("wifi", ui.tr("Wi-Fi", "WLAN"), false),
        ("bluetooth", "Bluetooth", false),
        ("bulb", ui.tr("Light", "Licht"), true),
        ("moon", ui.tr("Dark", "Dunkel"), false),
    ];
    let count = switches.len() as i32;
    let row = area.part(0, 25);
    let first = row.col(0, count, apart / 2);
    let small = ui.small.min(first.w as f32 / 4.6);
    let words = (small * 1.6) as i32;
    let radius = (first.w / 2).min((row.h - words) / 2) - 2;
    for (index, (icon, label, on)) in switches.iter().enumerate() {
        let cell = row.col(index as i32, count, apart / 2);
        let centre = Point {
            x: cell.x + cell.w / 2,
            y: cell.y + radius + 2,
        };
        ui.toggle(centre, radius, icon, *on)?;
        if focus == index {
            let around = B::new(centre.x - radius, centre.y - radius, 2 * radius, 2 * radius);
            ui.ring(around, radius)?;
        }
        let under = B::new(cell.x - 4, cell.y + 2 * radius + 8, cell.w + 8, words);
        ui.fit(under, Align::Center, small, label, ink)?;
    }
    let bars = area.part(28, 56);
    let light = bars.row(0, 2, apart);
    slider(ui, light, ("sun", "40 %", 40), focus == 4)?;
    let size = bars.row(1, 2, apart);
    slider(
        ui,
        size,
        ("text-size", ui.tr("Large", "Groß"), 60),
        focus == 5,
    )?;
    let cards = area.part(60, 100);
    let entries = [
        (
            "battery",
            ui.tr("Battery 82 %", "Akku 82 %"),
            ui.tr("3 weeks left", "noch 3 Wochen"),
        ),
        ("moon", ui.tr("Sleep", "Schlafen"), ""),
        ("settings", ui.tr("All settings", "Alle Einstellungen"), ""),
    ];
    let between = apart * 2 / 3;
    for (index, entry) in entries.iter().enumerate() {
        row_card(
            ui,
            cards.row(index as i32, 3, between),
            *entry,
            focus == 6 + index,
        )?;
    }
    Ok(())
}

/// The library: a card for each book, black where focused.
pub fn library(ui: &mut Ui<'_, '_>, focus: usize) -> Result<(), Error> {
    let rest = dock(ui, 2, focus.checked_sub(4))?;
    let top = ui.header(ui.tr("Library", "Bibliothek"), "1 / 6")?;
    let area = B::new(
        rest.x + ui.pad,
        top.y + ui.gap * 2 / 3,
        rest.w - 2 * ui.pad,
        rest.y + rest.h - top.y - ui.gap * 2 / 3,
    );
    let (ink, paper) = (ui.ink, ui.paper);
    let count = 4;
    let apart = ui.gap.max(2 * ui.stroke + 8) * 2 / 3;
    for (index, book) in BOOKS.iter().take(count as usize).enumerate() {
        let row = area.row(index as i32, count, apart);
        let focused = index == focus;
        let inner = card(ui, row, focused, false)?;
        let color = if focused { paper } else { ink };
        let size = (ui.u * 1.25).min(inner.h as f32 * 0.34);
        ui.bold(inner.part(0, 40), Align::Left, size, book.title, color)?;
        let small = ui.small.min(inner.h as f32 * 0.24);
        ui.text(inner.part(38, 66), Align::Left, small, book.author, color)?;
        let last = inner.part(68, 100);
        let read = state(ui, book)?;
        let read_w = ui.font.measure_text(small, &read)? as i32 + ui.gap / 2;
        let bar_h = (last.h * 6 / 10).clamp(10, 18);
        ui.bar(
            B::new(
                last.x,
                last.y + (last.h - bar_h) / 2,
                last.w - read_w,
                bar_h,
            ),
            book.percent,
            color,
        )?;
        ui.text(last, Align::Right, small, &read, color)?;
    }
    Ok(())
}
