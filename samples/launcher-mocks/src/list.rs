//! Concept A, "Words": nothing but large words, one below the other. The
//! focus is a black bar.
use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::widget::Align;

use crate::{
    data::BOOKS,
    kit::{B, Ui, state},
};

/// The rows of a list in `body`, with room at the sides and the bottom.
fn rows(ui: &Ui<'_, '_>, body: B) -> B {
    B::new(
        body.x + ui.pad / 2,
        body.y + ui.gap / 2,
        body.w - ui.pad,
        body.h - ui.gap / 2 - ui.pad / 2,
    )
}

/// Home: continue, library, store, transfer, settings.
pub fn home(ui: &mut Ui<'_, '_>, focus: usize) -> Result<(), Error> {
    let body = ui.status()?;
    let labels = [
        ui.tr("Continue reading", "Weiterlesen"),
        ui.tr("Library", "Bibliothek"),
        ui.tr("Store", "Store"),
        ui.tr("Wi-Fi transfer", "WLAN-Übertragung"),
        ui.tr("Settings", "Einstellungen"),
    ];
    let area = rows(ui, body);
    let count = labels.len() as i32;
    let first = area.row(0, count, 6);
    let inner = ui.pad * 3 / 4;
    let size = ui.common(
        &labels,
        (ui.u * 1.9).min(first.h as f32 * 0.42),
        first.w - 2 * inner,
    )?;
    for (index, label) in labels.iter().enumerate() {
        let row = area.row(index as i32, count, 6);
        let focused = index == focus;
        let (ink, paper) = (ui.ink, ui.paper);
        if focused {
            ui.plate(row, ui.radius(), ink)?;
        }
        let color = if focused { paper } else { ink };
        let text = row.inset_xy(inner, 0);
        if index == 0 {
            ui.bold(text.part(10, 62), Align::Left, size, label, color)?;
            let sub = ui.small.min(row.h as f32 * 0.2);
            let line = paperpaw_ui_kit::text::format(format_args!(
                "{} · {} %",
                BOOKS[0].title, BOOKS[0].percent
            ))?;
            ui.text(text.part(60, 88), Align::Left, sub, &line, color)?;
        } else {
            ui.bold(text, Align::Left, size, label, color)?;
        }
    }
    Ok(())
}

/// The library: a title, the author and how far it is read.
pub fn library(ui: &mut Ui<'_, '_>, focus: usize) -> Result<(), Error> {
    let body = ui.header(ui.tr("Library", "Bibliothek"), "1 / 5")?;
    let area = rows(ui, body);
    let count = 5;
    let inner = ui.pad * 3 / 4;
    for (index, book) in BOOKS.iter().take(count as usize).enumerate() {
        let row = area.row(index as i32, count, 6);
        let focused = index == focus;
        let (ink, paper) = (ui.ink, ui.paper);
        if focused {
            ui.plate(row, ui.radius(), ink)?;
        } else if index + 1 != focus && index as i32 + 1 < count {
            ui.fill(
                B::new(row.x + inner, row.y + row.h + 2, row.w - 2 * inner, 2),
                ink,
            )?;
        }
        let color = if focused { paper } else { ink };
        let text = row.inset_xy(inner, 0);
        let size = (ui.u * 1.35).min(row.h as f32 * 0.34);
        ui.bold(text.part(10, 58), Align::Left, size, book.title, color)?;
        let sub = ui.small.min(row.h as f32 * 0.22);
        let read = state(ui, book)?;
        let read_w = ui.font.measure_text(sub, &read)? as i32 + ui.gap;
        let line = text.part(58, 88);
        ui.text(
            line.left(line.w - read_w),
            Align::Left,
            sub,
            book.author,
            color,
        )?;
        ui.text(line, Align::Right, sub, &read, color)?;
    }
    Ok(())
}

/// Quick settings: a name and its value on each row.
pub fn quick(ui: &mut Ui<'_, '_>, focus: usize) -> Result<(), Error> {
    let body = ui.header(ui.tr("Quick settings", "Schnelleinstellungen"), "")?;
    let entries = [
        (ui.tr("Wi-Fi", "WLAN"), ui.tr("Off", "Aus")),
        (ui.tr("Light", "Licht"), "40 %"),
        (ui.tr("Text size", "Textgröße"), ui.tr("Large", "Groß")),
        (ui.tr("Dark mode", "Dunkelmodus"), ui.tr("Off", "Aus")),
        (ui.tr("Language", "Sprache"), ui.tr("English", "Deutsch")),
        (ui.tr("All settings", "Alle Einstellungen"), ""),
    ];
    let area = rows(ui, body);
    let count = entries.len() as i32;
    let inner = ui.pad * 3 / 4;
    let first = area.row(0, count, 6);
    let mut joined = alloc::vec::Vec::new();
    for (name, value) in &entries {
        joined.push(paperpaw_ui_kit::text::concat(&[name, "   ", value])?);
    }
    let lines: alloc::vec::Vec<&str> = joined.iter().map(alloc::string::String::as_str).collect();
    let size = ui.common(
        &lines,
        (ui.u * 1.5).min(first.h as f32 * 0.4),
        first.w - 2 * inner,
    )?;
    for (index, (name, value)) in entries.iter().enumerate() {
        let row = area.row(index as i32, count, 6);
        let focused = index == focus;
        let (ink, paper) = (ui.ink, ui.paper);
        if focused {
            ui.plate(row, ui.radius(), ink)?;
        } else if index + 1 != focus && index as i32 + 1 < count {
            ui.fill(
                B::new(row.x + inner, row.y + row.h + 2, row.w - 2 * inner, 2),
                ink,
            )?;
        }
        let color = if focused { paper } else { ink };
        let text = row.inset_xy(inner, 0);
        ui.text(text, Align::Left, size, name, color)?;
        ui.bold(text, Align::Right, size, value, color)?;
    }
    Ok(())
}
