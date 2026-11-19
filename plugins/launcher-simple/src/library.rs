//! The library: a card for each book (title, author, how far it is read), a
//! page at a time, and the dock. No covers.
use alloc::string::String;
use paperpaw_sdk::native::{Error, canvas::PartRole};
use paperpaw_ui_kit::widget::Align;

use crate::{
    data::{Book, Row},
    dock,
    home::state,
    look::{B, Ui},
    model::{self, Target},
};

/// The action of the dock's library icon.
pub const LIBRARY: &str = "core.library";

/// What the page drawn holds, for the keys.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Drawn {
    /// Books on a page, and pages in all.
    pub per_page: usize,
    pub pages: usize,
    /// The books drawn: the zones from the first on.
    pub shown: usize,
}

/// Draws page `page` of `books` into `ui`.
///
/// # Errors
/// [`Error::NoMemory`] and the host's refusals.
pub fn draw(
    ui: &mut Ui<'_, '_>,
    books: &[Book],
    page: usize,
    dock_rows: &[Row],
) -> Result<Drawn, Error> {
    let title = ui.t("launcher-simple.library.title")?;
    let top = ui.header(&title, "")?;
    let rest = ui
        .screen
        .above(dock::height(ui) + dock::edge(ui), ui.gap / 2);
    let area = B::new(
        rest.x + ui.pad,
        top.y + ui.gap * 2 / 3,
        rest.w - 2 * ui.pad,
        rest.y + rest.h - top.y - ui.gap * 2 / 3,
    );
    let ink = ui.ink;
    let apart = ui.gap.max(2 * ui.stroke + 8) * 2 / 3;
    // A card holds the title, the author and the bar: its least height.
    let min = ((ui.u * 1.25) as i32 + ui.small as i32 * 2 + ui.bar_height() + 3 * ui.stroke + 24)
        .max(110);
    let per_page = model::cards_that_fit(area.h, min, apart, 6).max(1);
    let pages = model::pages(books.len(), per_page);
    let page = page.min(pages - 1);
    let start = page * per_page;
    let shown = books.len().saturating_sub(start).min(per_page);
    if books.is_empty() {
        let none = ui.t("launcher-simple.library.empty")?;
        let size = ui.u;
        ui.fit(area.part(10, 40), Align::Center, size, &none, ink)?;
    }
    let count = shown.max(1) as i32;
    for (at, book) in books.iter().skip(start).take(shown).enumerate() {
        let row = area.row(at as i32, per_page as i32, apart);
        let _ = count;
        let focused = ui.focused();
        let (inner, tone) = ui.card(row, focused)?;
        let size = (ui.u * 1.25).min(inner.h as f32 * 0.34);
        ui.bold(inner.part(0, 40), Align::Left, size, &book.title, tone)?;
        let small = ui.small.min(inner.h as f32 * 0.24);
        ui.text(inner.part(38, 66), Align::Left, small, &book.author, tone)?;
        let last = inner.part(68, 100);
        let read = state(ui, book)?;
        let read_w = ui.font.measure_text(small, &read)? as i32 + ui.gap / 2;
        let bar_h = (last.h * 6 / 10).clamp(ui.bar_height(), 18);
        let bar = B::new(
            last.x,
            last.y + (last.h - bar_h) / 2,
            last.w - read_w,
            bar_h,
        );
        ui.bar(bar, u16::from(book.progress) * 10, tone)?;
        ui.text(last, Align::Right, small, &read, tone)?;
        ui.zone(Target::Book(at), row, PartRole::Row, &book.title)?;
    }
    if pages > 1 {
        // The page, and arrows to turn it.
        let note = page_note(ui, page, pages)?;
        let size = ui.small;
        let band = ui.screen.top(top.y - ui.screen.y).inset_xy(ui.pad, 0);
        let arrow = (band.h * 6 / 10).max(40);
        let right = B::new(
            band.x + band.w - arrow,
            band.y + (band.h - arrow) / 2,
            arrow,
            arrow,
        );
        let left = B::new(right.x - arrow - ui.gap / 2, right.y, arrow, arrow);
        let note_w = ui.font.measure_text(size, &note)? as i32 + 2;
        let words = B::new(left.x - ui.gap - note_w, band.y, note_w, band.h - ui.stroke);
        ui.text(words, Align::Right, size, &note, ink)?;
        for (forward, b) in [(false, left), (true, right)] {
            let name = if forward {
                "chevron-right"
            } else {
                "chevron-left"
            };
            let radius = b.w / 2;
            ui.disc(b.middle(), radius, name, false)?;
            if ui.focused() {
                ui.ring(b, radius)?;
            }
            let label = ui.t(if forward {
                "launcher-simple.page.next"
            } else {
                "launcher-simple.page.previous"
            })?;
            ui.zone(Target::Page(forward), b, PartRole::Row, &label)?;
        }
    }
    dock::draw(ui, dock_rows, LIBRARY)?;
    Ok(Drawn {
        per_page,
        pages,
        shown,
    })
}

/// "2 / 6".
fn page_note(_ui: &mut Ui<'_, '_>, page: usize, pages: usize) -> Result<String, Error> {
    paperpaw_ui_kit::text::format(format_args!("{} / {}", page + 1, pages))
}
