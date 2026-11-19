//! The home screen: a status line (the date, the battery), the time in large
//! digits of round strokes, the card of the book being read and the other
//! cards of the user's menu, and the dock. Only the status line and the
//! clock are drawn again each minute.
use alloc::string::String;
use paperpaw_sdk::{
    Rect,
    native::{Error, canvas::PartRole, formatting::Arg},
    time::LocalTime,
};
use paperpaw_ui_kit::widget::Align;

use crate::{
    data::{Book, Row},
    dock,
    look::{B, Ui, percent_text},
    model::Target,
};

/// The action of the *Continue reading* card.
pub const CONTINUE: &str = "launcher-simple.continue";
/// The action of the dock's home icon.
pub const HOME: &str = "core.home";

/// What the home screen shows.
pub struct Data<'d> {
    pub cards: &'d [Row],
    pub dock: &'d [Row],
    pub book: Option<&'d Book>,
    pub time: Option<LocalTime>,
    pub twelve_hours: bool,
    pub show_date: bool,
    pub battery: Option<u8>,
}

/// The segments of the digits 0 to 9: top, top right, bottom right, bottom,
/// bottom left, top left, middle.
const SEGMENTS: [u8; 10] = [63, 6, 91, 79, 102, 109, 125, 7, 127, 111];

/// The time in digits of round strokes, as tall as `b`: the font packs end
/// near 48 pixels, so a larger clock is drawn.
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

/// The time as digits: `HH:MM`, or `H:MM` on a twelve-hour clock; `--:--`
/// where the device does not know the time.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn digits(time: Option<LocalTime>, twelve_hours: bool) -> Result<String, Error> {
    let Some(t) = time else {
        return paperpaw_ui_kit::text::copy("--:--");
    };
    if twelve_hours {
        let hour = match t.hour % 12 {
            0 => 12,
            h => h,
        };
        paperpaw_ui_kit::text::format(format_args!("{hour}:{:02}", t.minute))
    } else {
        paperpaw_ui_kit::text::format(format_args!("{:02}:{:02}", t.hour, t.minute))
    }
}

/// Today in words: "Thursday, 19 November" (the language's `core.date`).
fn date(ui: &mut Ui<'_, '_>, t: &LocalTime) -> Result<String, Error> {
    const WEEKDAYS: [&str; 7] = [
        "core.weekday.0",
        "core.weekday.1",
        "core.weekday.2",
        "core.weekday.3",
        "core.weekday.4",
        "core.weekday.5",
        "core.weekday.6",
    ];
    const MONTHS: [&str; 12] = [
        "core.month.1",
        "core.month.2",
        "core.month.3",
        "core.month.4",
        "core.month.5",
        "core.month.6",
        "core.month.7",
        "core.month.8",
        "core.month.9",
        "core.month.10",
        "core.month.11",
        "core.month.12",
    ];
    let weekday = ui.t(WEEKDAYS[usize::from(t.weekday) % 7])?;
    let month = ui.t(MONTHS[usize::from(t.month.saturating_sub(1)) % 12])?;
    ui.messages.format(
        ui.host,
        "core.date",
        &[
            Arg::Int {
                name: "day",
                value: i64::from(t.day),
            },
            Arg::Text {
                name: "month",
                value: &month,
            },
            Arg::Text {
                name: "weekday",
                value: &weekday,
            },
        ],
    )
}

/// How far a book is read, in words.
pub fn state(ui: &mut Ui<'_, '_>, book: &Book) -> Result<String, Error> {
    if !book.opened {
        ui.t("launcher-simple.state.new")
    } else if book.progress >= 100 {
        ui.t("launcher-simple.state.finished")
    } else {
        percent_text(book.progress)
    }
}

/// A card for the book being read: a label, the title, the bar of how far.
fn reading(ui: &mut Ui<'_, '_>, b: B, index: usize, book: Option<&Book>) -> Result<(), Error> {
    let label_text = ui.t("launcher-simple.continue")?;
    let focused = ui.focused();
    let (inner, ink) = ui.card(b, focused)?;
    let label = ui.small.min(inner.h as f32 / 7.0);
    ui.fit(inner.part(0, 24), Align::Left, label, &label_text, ink)?;
    let Some(book) = book else {
        let none = ui.t("launcher-simple.no-book")?;
        let size = (ui.u * 1.1).min(inner.h as f32 / 4.0);
        ui.fit(inner.part(30, 100), Align::Left, size, &none, ink)?;
        return ui.zone(Target::Card(index), b, PartRole::Row, &label_text);
    };
    let title = (ui.u * 1.3).min(inner.h as f32 / 4.0);
    ui.bold(inner.part(24, 62), Align::Left, title, &book.title, ink)?;
    let last = inner.part(66, 100);
    let read = state(ui, book)?;
    let read_w = ui.font.measure_text(label, &read)? as i32 + ui.gap / 2;
    let bar_h = (last.h * 6 / 10).clamp(ui.bar_height(), 24);
    let bar = B::new(
        last.x,
        last.y + (last.h - bar_h) / 2,
        last.w - read_w,
        bar_h,
    );
    ui.bar(bar, u16::from(book.progress) * 10, ink)?;
    ui.bold(last, Align::Right, label, &read, ink)?;
    ui.zone(Target::Card(index), b, PartRole::Row, &label_text)
}

/// A slim card: an icon, a word, and a smaller word at the right.
fn row_card(ui: &mut Ui<'_, '_>, b: B, index: usize, row: &Row) -> Result<(), Error> {
    let focused = ui.focused();
    let (inner, ink) = ui.card(b, focused)?;
    let picture = inner.left(inner.h.min(inner.w / 5));
    let icon = if row.icon.is_empty() {
        "apps"
    } else {
        &row.icon
    };
    ui.icon(icon, picture.h as f32 * 1.1, picture, ink)?;
    let words = inner.right_of(picture.w, ui.gap / 2);
    let size = (ui.u * 1.2).min(words.h as f32 * 0.62);
    let small = ui.small.min(size * 0.8);
    let note_w = if row.badge.is_empty() {
        0
    } else {
        ui.font.measure_text(small, &row.badge)? as i32 + ui.gap / 2
    };
    ui.bold(
        words.left(words.w - note_w),
        Align::Left,
        size,
        &row.title,
        ink,
    )?;
    if !row.badge.is_empty() {
        ui.text(words, Align::Right, small, &row.badge, ink)?;
    }
    ui.zone(Target::Card(index), b, PartRole::Row, &row.title)
}

/// What the minute's tick draws again: the status line and the clock.
#[derive(Clone, Copy, Debug)]
pub struct Drawn {
    pub clock: Rect,
}

/// Draws the home screen into `ui`.
///
/// # Errors
/// [`Error::NoMemory`] and the host's refusals.
pub fn draw(ui: &mut Ui<'_, '_>, data: &Data<'_>) -> Result<Drawn, Error> {
    // The cards come first in the order the focus visits them, the dock after.
    let rest = ui
        .screen
        .above(dock::height(ui) + dock::edge(ui), ui.gap / 2);
    let area = B::new(
        rest.x + ui.pad,
        rest.y + ui.pad / 4,
        rest.w - 2 * ui.pad,
        rest.h - ui.pad / 4,
    );
    let ink = ui.ink;
    let apart = ui.gap.max(2 * ui.stroke + 10);
    // The status line: the date at the left, the battery at the right.
    let status_h = (ui.small * 2.0) as i32;
    let status = B::new(area.x, area.y, area.w, status_h);
    if let (true, Some(time)) = (data.show_date, data.time) {
        let words = date(ui, &time)?;
        let room = if data.battery.is_some() {
            status.left(status.w * 7 / 10)
        } else {
            status
        };
        ui.fit(room, Align::Left, ui.small, &words, ink)?;
    }
    if let Some(level) = data.battery {
        let text = percent_text(level)?;
        let width = ui.pill_width(&text)?;
        let at = B::new(status.x + status.w - width, status.y, width, status_h);
        ui.pill(at.inset_xy(0, status_h / 8), &text, ink, false)?;
    }
    let top = B::new(area.x, area.y + status_h, area.w, area.h * 20 / 100);
    let time = digits(data.time, data.twelve_hours)?;
    let band = top.centred(top.w, top.h.min(area.w * 26 / 100));
    clock(ui, band, &time)?;
    let drawn = Drawn {
        clock: Rect::new(
            status.x.max(0) as u16,
            status.y.max(0) as u16,
            status.w.max(0) as u16,
            (band.y + band.h - status.y).max(0) as u16,
        ),
    };
    // The cards: the book being read is taller, the others slim.
    let cards = B::new(
        area.x,
        top.y + top.h + apart,
        area.w,
        area.y + area.h - (top.y + top.h + apart),
    );
    let slim_min = ((ui.u * 1.2) as i32 + 2 * ui.stroke + 24).max(72);
    let reading_min = slim_min * 2;
    let mut rows: alloc::vec::Vec<&Row> = alloc::vec::Vec::new();
    for row in data.cards {
        rows.try_reserve(1).map_err(|_| Error::NoMemory)?;
        rows.push(row);
    }
    // What does not fit is left out, from the end.
    let mut keep = rows.len();
    while keep > 0 {
        let big = usize::from(rows[..keep].iter().any(|r| r.item == CONTINUE));
        let slim = keep - big;
        let needed = big as i32 * reading_min + slim as i32 * slim_min + (keep as i32 - 1) * apart;
        if needed <= cards.h {
            break;
        }
        keep -= 1;
    }
    let big = rows[..keep].iter().filter(|r| r.item == CONTINUE).count() as i32;
    let slim = keep as i32 - big;
    // The book card takes 2.2 slim cards' height.
    let weights = big * 22 + slim * 10;
    let room = cards.h - (keep as i32 - 1).max(0) * apart;
    let mut y = cards.y;
    for (index, row) in rows[..keep].iter().enumerate() {
        let weight = if row.item == CONTINUE { 22 } else { 10 };
        let h = (room * weight / weights.max(1)).max(0);
        let b = B::new(cards.x, y, cards.w, h);
        if row.item == CONTINUE {
            reading(ui, b, index, data.book)?;
        } else {
            row_card(ui, b, index, row)?;
        }
        y += h + apart;
    }
    dock::draw(ui, data.dock, HOME)?;
    Ok(drawn)
}
