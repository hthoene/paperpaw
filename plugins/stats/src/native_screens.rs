//! Stats' screens on interface 7: the books and one book's numbers. Each view
//! is built when its screen opens, from owned data; the draw callback only
//! replays prepared drawing.

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::native::callbacks::{DrawFrame, DrawPurpose, Input, InputGestureKind};
use paperpaw_sdk::native::commands::CommandSender;
use paperpaw_sdk::native::fonts::{Font, FontRole};
use paperpaw_sdk::native::formatting::Arg;
use paperpaw_sdk::native::snapshot;
use paperpaw_sdk::native::ui::{Open, Param, Ui};
use paperpaw_sdk::native::{Error, Host};
use paperpaw_sdk::{Handled, Rect, Storage, StorageError};
use paperpaw_ui_kit::list::{backdrop, columns};
use paperpaw_ui_kit::mem::{SERVICE_BUFFER, buffer};
use paperpaw_ui_kit::{
    Footer, Item, ItemList, ItemStep, Messages, Page, Paint, Style, header, paint, text,
};

use crate::native_chart;
use crate::store::{self, Book};
use crate::texts::{self, HostWords, Words};
use crate::totals::{self, BookTotals};

/// The days a book's chart shows.
const CHART_DAYS: usize = 10;
/// The hand-over parameter that names a book.
pub(crate) const ID: &str = "id";

/// What a failed read of the plugin's files means to the screen.
pub(crate) fn failed(error: StorageError) -> Error {
    match error {
        StorageError::TooLarge => Error::NoMemory,
        StorageError::NotFound => Error::NotFound,
        _ => Error::Io,
    }
}

/// UTC days until the system time zone comes with the snapshot (draft 25).
pub(crate) fn utc_day(unix: u32) -> i32 {
    (unix / 86_400) as i32
}

/// The books with reading time, the last one read first.
pub(crate) struct BooksView {
    /// The id of the book of each row.
    ids: Vec<u16>,
    list: ItemList,
    empty: String,
}

impl BooksView {
    /// Reads the books over `data`.
    ///
    /// # Errors
    /// Allocation and storage failures; nothing is shown from a partial read.
    pub(crate) fn new(host: &Host<'_>, data: &dyn Storage) -> Result<Self, Error> {
        Self::build(&mut HostWords::new(host)?, data)
    }

    /// As [`Self::new`], with the texts given.
    pub(crate) fn build(words: &mut impl Words, data: &dyn Storage) -> Result<Self, Error> {
        let books = store::load_books(data).map_err(failed)?;
        let seconds = totals::seconds_per_book(data, &books).map_err(failed)?;
        let places = crate::book_list::order(&books, &seconds)?;
        let items = crate::book_list::items(words, &books, &seconds, &places, utc_day)?;
        let mut ids = Vec::new();
        ids.try_reserve_exact(places.len())
            .map_err(|_| Error::NoMemory)?;
        ids.extend(places.iter().filter_map(|&at| books.get(at).map(|b| b.id)));
        Ok(Self {
            ids,
            list: ItemList::new(items),
            empty: words.copy("stats.no-books")?,
        })
    }

    pub(crate) fn draw(&mut self, host: &Host<'_>, frame: &mut DrawFrame<'_>) -> Result<(), Error> {
        let footer = if self.ids.is_empty() {
            Footer::Empty {
                title: &self.empty,
                text: "",
            }
        } else {
            Footer::None
        };
        self.list
            .draw_with(host, frame, "stats.books", true, footer)
    }

    /// A chosen book opens its numbers.
    pub(crate) fn input(
        &mut self,
        host: &Host<'_>,
        ui: &Ui,
        input: &Input<'_>,
    ) -> Result<Handled, Error> {
        let step = self.list.input(host, input)?;
        if let ItemStep::Chosen(index) = step
            && let Some(&id) = self.ids.get(index)
        {
            open_book(host, ui, id)?;
        }
        Ok(ItemList::handled(step))
    }
}

/// Opens the screen of book `id`, which reads the id from its hand-over.
pub(crate) fn open_book(host: &Host<'_>, ui: &Ui, id: u16) -> Result<(), Error> {
    open_with_book(host, ui, "stats.book", id)
}

/// Opens `screen` with the id of a book as its hand-over.
pub(crate) fn open_with_book(host: &Host<'_>, ui: &Ui, screen: &str, id: u16) -> Result<(), Error> {
    let mut digits = [0u8; 20];
    let id = text::decimal(usize::from(id), &mut digits);
    let mut input = buffer(SERVICE_BUFFER)?;
    let mut output = buffer(SERVICE_BUFFER)?;
    let handover = ui.handover(host, &[Param::new(ID, id)?], &mut input, &mut output)?;
    CommandSender::new(host, &mut input).open(Open::screen(screen)?.handover(handover))
}

/// The id of the book a hand-over names.
pub(crate) fn book_of(
    host: &Host<'_>,
    ui: &Ui,
    handover: paperpaw_sdk::native::ui::HandoverId,
) -> Result<Option<u16>, Error> {
    let mut input = buffer(SERVICE_BUFFER)?;
    let mut output = buffer(SERVICE_BUFFER)?;
    for param in ui.params(host, handover, &mut input, &mut output)? {
        let param = param?;
        if param.key() == ID {
            return Ok(param.value().parse().ok());
        }
    }
    Ok(None)
}

/// The last days a book was read, as a chart.
struct Chart {
    title: String,
    values: Vec<u32>,
    dates: Vec<String>,
    top: String,
}

/// One book's numbers: rows of what and its value, then the last days read.
pub(crate) struct BookView {
    title: String,
    /// `None`: no such book (it was not recorded, or a list went stale).
    rows: Option<Vec<(String, String)>>,
    chart: Option<Chart>,
    none: String,
    header: Page,
}

impl BookView {
    /// Reads book `id` over `data`.
    ///
    /// # Errors
    /// Allocation and storage failures; a book that is not there is a view
    /// that says so.
    pub(crate) fn new(host: &Host<'_>, data: &dyn Storage, id: Option<u16>) -> Result<Self, Error> {
        Self::build(&mut HostWords::new(host)?, data, id)
    }

    /// As [`Self::new`], with the texts given.
    pub(crate) fn build(
        words: &mut impl Words,
        data: &dyn Storage,
        id: Option<u16>,
    ) -> Result<Self, Error> {
        let none = words.copy("stats.no-books")?;
        let books = store::load_books(data).map_err(failed)?;
        let Some(book) = id.and_then(|id| books.iter().find(|b| b.id == id)) else {
            return Ok(Self {
                title: String::new(),
                rows: None,
                chart: None,
                none,
                header: Page::default(),
            });
        };
        let totals = totals::of_book(data, book, utc_day).map_err(failed)?;
        let rows = rows_of(words, book, &totals)?;
        let chart = chart_of(words, &totals)?;
        Ok(Self {
            title: text::copy(&book.title)?,
            rows: Some(rows),
            chart,
            none,
            header: Page::default(),
        })
    }

    pub(crate) fn draw(&mut self, host: &Host<'_>, frame: &mut DrawFrame<'_>) -> Result<(), Error> {
        if !matches!(frame.purpose, DrawPurpose::Screen) {
            return Ok(());
        }
        let display = snapshot::capture_snapshot(host)?;
        let font = Font::acquire_role(host, FontRole::Ui)?;
        let style = Style::read(host, &display, &font)?;
        let (width, height) = (display.display_width(), display.display_height());
        let mut header = Page::default();
        header::prepare(
            &mut header,
            host,
            &font,
            &style,
            &mut Messages::new()?,
            &self.title,
            width,
            display.has_touch(),
            0,
            1,
        )?;
        let top = style.header.saturating_add(style.gap);
        let mut body = Vec::new();
        paint::push(
            &mut body,
            Paint::Fill(
                Rect::new(0, top, width, height.saturating_sub(top)),
                style.paper,
            ),
        )?;
        match &self.rows {
            None => message(
                &mut body,
                &font,
                &style,
                Rect::new(0, top, width, height.saturating_sub(top)),
                &self.none,
            )?,
            Some(rows) => {
                let row = style.row * 4 / 5;
                for (n, (label, value)) in rows.iter().enumerate() {
                    let rect = Rect::new(
                        0,
                        top.saturating_add(row.saturating_mul(n as u16)),
                        width,
                        row,
                    );
                    backdrop(&mut body, &style, rect, false)?;
                    columns(&mut body, &font, &style, rect, label, value, style.ink)?;
                }
                if let Some(chart) = &self.chart {
                    let below = top
                        .saturating_add(row.saturating_mul(rows.len() as u16))
                        .saturating_add(style.gap.saturating_mul(2));
                    draw_chart(&mut body, &font, &style, chart, below, width, height)?;
                }
            }
        }
        paint::replay(frame, &font, header.paint.iter().chain(body.iter()))?;
        font.release()?;
        header.paint = Vec::new();
        self.header = header;
        Ok(())
    }

    /// Only back does anything here.
    pub(crate) fn input(&self, host: &Host<'_>, input: &Input<'_>) -> Result<Handled, Error> {
        let Input::Gesture {
            position,
            kind: InputGestureKind::Tap,
            ..
        } = *input
        else {
            return Ok(Handled::No);
        };
        let Some(action) = self.header.action(position) else {
            return Ok(Handled::No);
        };
        let mut scratch = [0; 128];
        CommandSender::new(host, &mut scratch).run_action(action)?;
        Ok(Handled::Yes)
    }
}

/// The rows of `book`'s numbers as pairs of what and its value.
fn rows_of(
    words: &mut impl Words,
    book: &Book,
    totals: &BookTotals,
) -> Result<Vec<(String, String)>, Error> {
    let items = crate::book_rows::rows(words, book, totals)?;
    let mut rows = Vec::new();
    rows.try_reserve_exact(items.len())
        .map_err(|_| Error::NoMemory)?;
    for item in items {
        if let Item::Row(row) = item {
            rows.push((row.title, row.value));
        }
    }
    Ok(rows)
}

/// The chart of the last [`CHART_DAYS`] days read; none where no day was.
fn chart_of(words: &mut impl Words, totals: &BookTotals) -> Result<Option<Chart>, Error> {
    if totals.days.is_empty() {
        return Ok(None);
    }
    let days = totals
        .days
        .get(totals.days.len().saturating_sub(CHART_DAYS)..)
        .unwrap_or_default();
    let mut values = Vec::new();
    let mut dates = Vec::new();
    values
        .try_reserve_exact(days.len())
        .map_err(|_| Error::NoMemory)?;
    dates
        .try_reserve_exact(days.len())
        .map_err(|_| Error::NoMemory)?;
    for &(day, seconds) in days {
        let date = texts::date_of(day);
        values.push(seconds);
        dates.push(words.format(
            "stats.day-month",
            &[
                Arg::Int {
                    name: "day",
                    value: i64::from(date.day),
                },
                Arg::Int {
                    name: "month",
                    value: i64::from(date.month),
                },
            ],
        )?);
    }
    let max = values.iter().copied().max().unwrap_or(0);
    Ok(Some(Chart {
        title: words.copy("stats.by-day")?,
        top: texts::duration(words, u64::from(max))?,
        values,
        dates,
    }))
}

/// The chart under the rows, where there is room for it.
fn draw_chart(
    body: &mut Vec<Paint>,
    font: &Font<'_>,
    style: &Style,
    chart: &Chart,
    top: u16,
    width: u16,
    height: u16,
) -> Result<(), Error> {
    let room = height.saturating_sub(top.saturating_add(style.gap));
    if room < style.row.saturating_mul(2) {
        return Ok(());
    }
    let title_height = (style.small * 1.6) as u16;
    text::label(
        body,
        font,
        &chart.title,
        style.small,
        f32::from(style.margin),
        Rect::new(
            style.margin,
            top,
            width.saturating_sub(2 * style.margin),
            title_height,
        ),
        style.muted,
        None,
    )?;
    let mut labels: Vec<&str> = Vec::new();
    labels
        .try_reserve_exact(chart.dates.len())
        .map_err(|_| Error::NoMemory)?;
    labels.extend(chart.dates.iter().map(String::as_str));
    let area = Rect::new(
        style.margin,
        top.saturating_add(title_height),
        width.saturating_sub(2 * style.margin),
        room.saturating_sub(title_height)
            .min(style.row.saturating_mul(5)),
    );
    native_chart::bars(
        body,
        font,
        style,
        area,
        &chart.values,
        &labels,
        (None, &chart.top),
    )
}

/// `words` centred in `area`, wrapped, as a hint where there is nothing to show.
pub(crate) fn message(
    body: &mut Vec<Paint>,
    font: &Font<'_>,
    style: &Style,
    area: Rect,
    words: &str,
) -> Result<(), Error> {
    let room = f32::from(area.width.saturating_sub(4 * style.margin)).max(1.0);
    let lines = text::wrap_with(|line| font.measure_text(style.small, line), words, room, 6)?;
    let line = paperpaw_ui_kit::geometry::line_px(font, style.small)?;
    let total = u32::from(line) * lines.len() as u32;
    let mut y = area
        .y
        .saturating_add((u32::from(area.height).saturating_sub(total) / 2) as u16);
    for text_line in &lines {
        let x = (f32::from(area.width) - font.measure_text(style.small, text_line)?).max(0.0) / 2.0;
        text::label(
            body,
            font,
            text_line,
            style.small,
            f32::from(area.x) + x,
            Rect::new(area.x, y, area.width, line),
            style.muted,
            None,
        )?;
        y = y.saturating_add(line);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
