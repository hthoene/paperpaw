//! Concept B, "Tiles": the book being read on a large tile with its cover,
//! and a few tiles with an icon and a word. The focus is a second outline.
use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::widget::Align;

use crate::{
    data::BOOKS,
    kit::{B, Ui, state},
};

/// The room of the tiles below a top band.
fn room(ui: &Ui<'_, '_>, body: B) -> B {
    B::new(
        body.x + ui.pad,
        body.y + ui.gap / 2,
        body.w - 2 * ui.pad,
        body.h - ui.gap / 2 - ui.pad,
    )
}

/// A tile with an icon over a word (and a second, smaller word): black
/// where `on`.
fn tile(
    ui: &mut Ui<'_, '_>,
    cell: B,
    (icon, label, note): (&str, &str, &str),
    on: bool,
    focused: bool,
) -> Result<(), Error> {
    let radius = ui.radius();
    let (ink, paper) = (ui.ink, ui.paper);
    if on {
        ui.plate(cell, radius, ink)?;
    } else {
        ui.frame(cell, radius, ui.stroke, ink)?;
    }
    if focused {
        ui.ring(cell, radius)?;
    }
    let color = if on { paper } else { ink };
    let inner = cell.inset(ui.stroke + 6);
    let (picture, words) = if note.is_empty() {
        (inner.part(6, 64), inner.part(64, 94))
    } else {
        (inner.part(4, 54), inner.part(54, 78))
    };
    ui.icon(icon, picture.h as f32 * 1.3, picture, color)?;
    let size = (ui.u * 1.2).min(words.h as f32 * 0.8);
    ui.bold(words, Align::Center, size, label, color)?;
    if !note.is_empty() {
        let small = ui.small.min(size * 0.8);
        ui.fit(inner.part(78, 96), Align::Center, small, note, color)?;
    }
    Ok(())
}

/// Home: the book being read, then library, store, transfer and settings.
pub fn home(ui: &mut Ui<'_, '_>, focus: usize) -> Result<(), Error> {
    let body = ui.status()?;
    let area = room(ui, body);
    let radius = ui.radius();
    let apart = ui.gap.max(2 * ui.stroke + 10);
    let hero = area.top(area.h * 42 / 100);
    let ink = ui.ink;
    ui.frame(hero, radius, ui.stroke, ink)?;
    if focus == 0 {
        ui.ring(hero, radius)?;
    }
    let inner = hero.inset(ui.stroke + ui.pad / 2);
    let cover = inner.left((inner.h * 2 / 3).min(inner.w * 45 / 100));
    let paper = ui.paper;
    ui.cover(cover, &BOOKS[0], ink, paper)?;
    let words = inner.right_of(cover.w, ui.pad / 2);
    let small = ui.small.min(words.h as f32 / 9.0);
    ui.fit(
        words.part(0, 14),
        Align::Left,
        small,
        ui.tr("Continue reading", "Weiterlesen"),
        ink,
    )?;
    ui.wrapped(
        words.part(16, 62),
        Align::Left,
        ((ui.u * 1.4).min(words.h as f32 / 5.0), ui.u * 0.7),
        BOOKS[0].title,
        ink,
    )?;
    ui.fit(words.part(63, 77), Align::Left, small, BOOKS[0].author, ink)?;
    let last = words.part(80, 100);
    let read = state(ui, &BOOKS[0])?;
    let read_w = ui.font.measure_text(small, &read)? as i32 + ui.gap / 2;
    let bar_h = (last.h * 55 / 100).clamp(10, 22);
    let bar = B::new(
        last.x,
        last.y + (last.h - bar_h) / 2,
        last.w - read_w,
        bar_h,
    );
    ui.bar(bar, BOOKS[0].percent, ink)?;
    ui.bold(last, Align::Right, small, &read, ink)?;

    let grid = area.below(hero.h, apart);
    let tiles = [
        ("books", ui.tr("Library", "Bibliothek")),
        ("cloud-download", ui.tr("Store", "Store")),
        ("transfer", ui.tr("Transfer", "Übertragen")),
        ("settings", ui.tr("Settings", "Einstellungen")),
    ];
    for (index, (icon, label)) in tiles.iter().enumerate() {
        let cell = grid
            .row(index as i32 / 2, 2, apart)
            .col(index as i32 % 2, 2, apart);
        tile(ui, cell, (icon, label, ""), false, focus == index + 1)?;
    }
    Ok(())
}

/// The library: four covers a page.
pub fn library(ui: &mut Ui<'_, '_>, focus: usize) -> Result<(), Error> {
    let body = ui.header(ui.tr("Library", "Bibliothek"), "1 / 6")?;
    let area = room(ui, body);
    let apart = ui.gap.max(2 * ui.stroke + 10);
    let (ink, paper) = (ui.ink, ui.paper);
    for (index, book) in BOOKS.iter().take(4).enumerate() {
        let cell = area
            .row(index as i32 / 2, 2, apart)
            .col(index as i32 % 2, 2, apart);
        if index == focus {
            ui.ring(cell.inset(3), ui.radius())?;
        }
        let inner = cell.inset(10);
        let small = ui.small.min(inner.h as f32 / 12.0);
        let words = (small * 1.5) as i32;
        let bar_h = 10;
        let picture = inner.top(inner.h - words - bar_h - 10);
        let cover = picture.centred((picture.h * 2 / 3).min(picture.w), picture.h);
        ui.cover(cover, book, ink, paper)?;
        let title = B::new(inner.x, picture.y + picture.h + 4, inner.w, words);
        ui.text(title, Align::Center, small, book.title, ink)?;
        let bar = B::new(cover.x, inner.y + inner.h - bar_h, cover.w, bar_h);
        ui.bar(bar, book.percent, ink)?;
    }
    Ok(())
}

/// Quick settings: tiles that are black while on.
pub fn quick(ui: &mut Ui<'_, '_>, focus: usize) -> Result<(), Error> {
    let body = ui.header(ui.tr("Quick settings", "Schnelleinstellungen"), "")?;
    let area = room(ui, body);
    let apart = ui.gap.max(2 * ui.stroke + 10);
    let tiles = [
        ("wifi", ui.tr("Wi-Fi", "WLAN"), ui.tr("Off", "Aus"), false),
        ("bluetooth", "Bluetooth", ui.tr("Off", "Aus"), false),
        ("bulb", ui.tr("Light", "Licht"), "40 %", true),
        (
            "moon",
            ui.tr("Dark mode", "Dunkelmodus"),
            ui.tr("Off", "Aus"),
            false,
        ),
        (
            "text-size",
            ui.tr("Text size", "Textgröße"),
            ui.tr("Large", "Groß"),
            false,
        ),
        ("settings", ui.tr("All settings", "Alle"), "", false),
    ];
    for (index, (icon, label, note, on)) in tiles.iter().enumerate() {
        let cell = area
            .row(index as i32 / 2, 3, apart)
            .col(index as i32 % 2, 2, apart);
        tile(ui, cell, (icon, label, note), *on, focus == index)?;
    }
    Ok(())
}
