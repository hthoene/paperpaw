//! Concept C, "One book": the book being read fills the page, with two large
//! buttons below it. The focus is the black button.
use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::widget::Align;

use crate::{
    data::BOOKS,
    kit::{B, Ui, state},
};

/// A large pill button: black where focused.
fn button(
    ui: &mut Ui<'_, '_>,
    b: B,
    (icon, label): (&str, &str),
    size: f32,
    focused: bool,
) -> Result<(), Error> {
    let (ink, paper) = (ui.ink, ui.paper);
    let radius = (b.h / 2).min(ui.radius() * 2);
    if focused {
        ui.plate(b, radius, ink)?;
    } else {
        ui.frame(b, radius, ui.stroke, ink)?;
    }
    let color = if focused { paper } else { ink };
    if icon.is_empty() {
        return ui.bold(b.inset_xy(ui.pad / 2, 0), Align::Center, size, label, color);
    }
    let picture = b.left(b.h).inset_xy(ui.pad / 3, 0);
    ui.icon(
        icon,
        b.h as f32 * 0.7,
        B::new(picture.x + ui.pad / 3, b.y, b.h, b.h),
        color,
    )?;
    let words = b.right_of(b.h + ui.pad / 3, 0);
    ui.bold(
        B::new(words.x, words.y, words.w - ui.pad / 2, words.h),
        Align::Left,
        size,
        label,
        color,
    )
}

/// Home: the cover, the title, how far it is read, *Continue* and *Library*.
pub fn home(ui: &mut Ui<'_, '_>, focus: usize) -> Result<(), Error> {
    let body = ui.status()?;
    let area = B::new(
        body.x + ui.pad,
        body.y,
        body.w - 2 * ui.pad,
        body.h - ui.pad,
    );
    let (ink, paper) = (ui.ink, ui.paper);
    let buttons = area.bottom(((ui.u * 2.9) as i32).clamp(64, area.h / 6));
    let rest = area.above(buttons.h, ui.gap);
    let book = &BOOKS[0];
    let picture = rest.part(0, 56);
    let cover = picture.centred((picture.h * 2 / 3).min(picture.w), picture.h);
    ui.cover(cover, book, ink, paper)?;
    let max = (ui.u * 1.45).min(rest.h as f32 / 14.0);
    ui.wrapped(
        rest.part(58, 76),
        Align::Center,
        (max, ui.u * 0.7),
        book.title,
        ink,
    )?;
    let small = ui.small.min(rest.h as f32 / 24.0);
    ui.fit(rest.part(76, 83), Align::Center, small, book.author, ink)?;
    let bar_h = ((ui.u * 0.75) as i32).clamp(12, 26);
    let line = rest.part(85, 92);
    ui.bar(line.centred(line.w, bar_h), book.percent, ink)?;
    let notes = rest.part(92, 100);
    let read = state(ui, book)?;
    ui.bold(notes, Align::Left, small, &read, ink)?;
    ui.text(
        notes,
        Align::Right,
        small,
        ui.tr("about 3 h left", "noch etwa 3 Std."),
        ink,
    )?;
    let labels = [
        ui.tr("Continue", "Weiterlesen"),
        ui.tr("Library", "Bibliothek"),
    ];
    let apart = ui.gap.max(2 * ui.stroke + 10);
    let first = buttons.col(0, 2, apart);
    let size = ui.common(
        &labels,
        (ui.u * 1.35).min(first.h as f32 * 0.5),
        first.w - ui.pad - first.w / 12,
    )?;
    for (index, label) in labels.iter().enumerate() {
        let cell = buttons.col(index as i32, 2, apart);
        button(ui, cell, ("", label), size, focus == index)?;
    }
    Ok(())
}

/// The shelf: a small cover, the title and a bar on each row.
pub fn shelf(ui: &mut Ui<'_, '_>, focus: usize) -> Result<(), Error> {
    let body = ui.header(ui.tr("Your books", "Deine Bücher"), "1 / 6")?;
    let area = B::new(
        body.x + ui.pad / 2,
        body.y + ui.gap / 2,
        body.w - ui.pad,
        body.h - ui.gap / 2 - ui.pad / 2,
    );
    let (ink, paper) = (ui.ink, ui.paper);
    let count = 4;
    for (index, book) in BOOKS.iter().skip(1).take(count as usize).enumerate() {
        let row = area.row(index as i32, count, 10);
        if index == focus {
            ui.frame(row, ui.radius(), ui.stroke + 1, ink)?;
        } else if index + 1 != focus && index as i32 + 1 < count {
            ui.fill(
                B::new(row.x + ui.pad / 2, row.y + row.h + 4, row.w - ui.pad, 2),
                ink,
            )?;
        }
        let inner = row.inset_xy(ui.pad / 2, 12);
        let cover = inner.left(inner.h * 2 / 3);
        ui.cover(cover, book, ink, paper)?;
        let words = inner.right_of(cover.w, ui.pad / 2);
        let size = (ui.u * 1.25).min(words.h as f32 * 0.26);
        ui.bold(words.part(4, 40), Align::Left, size, book.title, ink)?;
        let small = ui.small.min(words.h as f32 * 0.2);
        ui.text(words.part(40, 64), Align::Left, small, book.author, ink)?;
        let last = words.part(68, 96);
        let read = state(ui, book)?;
        let read_w = ui.font.measure_text(small, &read)? as i32 + ui.gap / 2;
        let bar_h = (last.h / 2).clamp(10, 18);
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
        ui.text(last, Align::Right, small, &read, ink)?;
    }
    Ok(())
}

/// More: what else there is, as large buttons.
pub fn more(ui: &mut Ui<'_, '_>, focus: usize) -> Result<(), Error> {
    let body = ui.header(ui.tr("More", "Mehr"), "")?;
    let area = B::new(
        body.x + ui.pad,
        body.y + ui.gap,
        body.w - 2 * ui.pad,
        body.h - ui.gap - ui.pad,
    );
    let entries = [
        ("books", ui.tr("Library", "Bibliothek")),
        ("cloud-download", ui.tr("Store", "Store")),
        ("transfer", ui.tr("Wi-Fi transfer", "WLAN-Übertragung")),
        ("settings", ui.tr("Settings", "Einstellungen")),
        ("moon", ui.tr("Sleep", "Schlafen")),
    ];
    let count = entries.len() as i32;
    let apart = ui.gap.max(2 * ui.stroke + 8);
    let first = area.row(0, count, apart);
    let labels: alloc::vec::Vec<&str> = entries.iter().map(|(_, label)| *label).collect();
    let size = ui.common(
        &labels,
        (ui.u * 1.5).min(first.h as f32 * 0.42),
        first.w - first.h - ui.pad - first.w / 10,
    )?;
    for (index, entry) in entries.iter().enumerate() {
        let row = area.row(index as i32, count, apart);
        button(ui, row, *entry, size, focus == index)?;
    }
    Ok(())
}
