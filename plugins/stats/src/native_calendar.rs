//! The calendar on interface 7: a month with its days shaded by how long one
//! read, and below it the chosen day: its date, reading time, pages and books.
//! Taps choose a day or turn the month, keys move by a day or a week (the page
//! keys and swipes by a month). The contents are `calendar_model`'s; this lays
//! them out and draws them.

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::native::callbacks::{DrawFrame, DrawPurpose, Input, InputGestureKind};
use paperpaw_sdk::native::commands::{CommandSender, RedrawMode};
use paperpaw_sdk::native::fonts::{Font, FontRole};
use paperpaw_sdk::native::settings::{self, SettingValue};
use paperpaw_sdk::native::snapshot;
use paperpaw_sdk::native::storage::DataStorage;
use paperpaw_sdk::native::{Error, Host};
use paperpaw_sdk::{Color, Handled, Point, Rect, Storage};
use paperpaw_ui_kit::{Messages, Page, Paint, Style, geometry, header, paint, text};

use crate::calendar_model::{self as model, DAY_BOOKS, DayText, Key, Swipe};
use crate::files::DataFolder;
use crate::native_overview::fit_line;
use crate::native_screens::{failed, message, utc_day};
use crate::store::{self, Day};
use crate::texts::{self, HostWords, Words};

/// The calendar's contents and where it was last drawn.
pub(crate) struct CalendarView {
    days: Vec<Day>,
    books: Vec<model::Named>,
    today: Option<i32>,
    /// The first day of the week, 0 being Sunday.
    week_start: u8,
    /// The chosen day: at first today, else the last day read.
    day: i32,
    said: DayText,
    /// The month's name over the days, whole and shortened.
    title: [String; 2],
    heading: String,
    /// The streak, where it is more than a day.
    note: Option<String>,
    no_clock: String,
    /// The weekdays' short names, from the first day of the week.
    names: Vec<String>,
    /// The header's regions and the places of the last frame, for taps.
    page: Page,
    arrows: Option<(Rect, Rect)>,
    cells: Option<Cells>,
}

/// The days' squares as drawn: the first one's corner and their size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Cells {
    x: u16,
    y: u16,
    size: u16,
}

impl Cells {
    /// The square of day number `index` of the 42 shown.
    fn of(&self, index: u16) -> Rect {
        Rect::new(
            self.x.saturating_add(self.size.saturating_mul(index % 7)),
            self.y.saturating_add(self.size.saturating_mul(index / 7)),
            self.size,
            self.size,
        )
    }

    /// The index of the square at `point`, if one is there.
    fn at(&self, point: Point) -> Option<u16> {
        let size = i32::from(self.size);
        let col = (point.x - i32::from(self.x)).checked_div(size)?;
        let row = (point.y - i32::from(self.y)).checked_div(size)?;
        let inside =
            point.x >= i32::from(self.x) && point.y >= i32::from(self.y) && col < 7 && row < 6;
        inside.then(|| (row * 7 + col) as u16)
    }
}

impl CalendarView {
    /// Reads the figures over `data`; `today` is the local day (none until the
    /// device knows the time).
    pub(crate) fn new(
        host: &Host<'_>,
        data: &dyn Storage,
        today: Option<i32>,
    ) -> Result<Self, Error> {
        let mut words = HostWords::new(host)?;
        let week_start = week_start_of(host, &mut words)?;
        Self::build(&mut words, data, today, week_start)
    }

    /// As [`Self::new`], with the texts and the first day of the week given.
    pub(crate) fn build(
        words: &mut impl Words,
        data: &dyn Storage,
        today: Option<i32>,
        week_start: u8,
    ) -> Result<Self, Error> {
        let days = store::days(data, utc_day).map_err(failed)?;
        // Only the names stay: the books' other fields are dropped here.
        let books = model::names(&store::load_books(data).map_err(failed)?)?;
        let day = model::clamp_day(today.or(days.last().map(|d| d.day)).unwrap_or(0));
        let (said, title) = contents(words, data, &books, &days, day)?;
        let mut names = Vec::new();
        names.try_reserve_exact(7).map_err(|_| Error::NoMemory)?;
        for col in 0..7 {
            names.push(texts::weekday_short(words, (week_start + col) % 7)?);
        }
        Ok(Self {
            note: model::streak_note(words, &days, today)?,
            heading: words.copy("stats.calendar")?,
            no_clock: words.copy("stats.no-clock")?,
            days,
            books,
            today,
            week_start,
            day,
            said,
            title,
            names,
            page: Page::default(),
            arrows: None,
            cells: None,
        })
    }

    /// Whether there is no day to show: neither today nor one read.
    fn empty(&self) -> bool {
        self.today.is_none() && self.days.is_empty()
    }

    /// The first day shown: the start of the week the month begins in.
    fn first_shown(&self) -> i32 {
        model::first_shown(self.day, self.week_start)
    }

    pub(crate) fn draw(&mut self, host: &Host<'_>, frame: &mut DrawFrame<'_>) -> Result<(), Error> {
        if !matches!(frame.purpose, DrawPurpose::Screen) {
            return Ok(());
        }
        let display = snapshot::capture_snapshot(host)?;
        let font = Font::acquire_role(host, FontRole::Ui)?;
        let style = Style::read(host, &display, &font)?;
        let (width, height) = (display.display_width(), display.display_height());
        let small_line = geometry::line_px(&font, style.small)?;
        let plan = Plan::of(&style, width, height, small_line);
        let mut header = Page::default();
        header::prepare(
            &mut header,
            host,
            &font,
            &style,
            &mut Messages::new()?,
            &self.heading,
            width,
            display.has_touch(),
            0,
            1,
        )?;
        self.streak(&mut header, &font, &style, width)?;
        let mut body = Vec::new();
        let below = style.header.saturating_add(style.gap);
        paint::push(
            &mut body,
            Paint::Fill(
                Rect::new(0, below, width, height.saturating_sub(below)),
                style.paper,
            ),
        )?;
        let (mut arrows, mut cells) = (None, None);
        if self.empty() {
            let area = Rect::new(
                0,
                plan.nav.y,
                width,
                plan.detail
                    .y
                    .saturating_add(plan.detail.height)
                    .saturating_sub(plan.nav.y),
            );
            message(&mut body, &font, &style, area, &self.no_clock)?;
        } else {
            self.draw_nav(&mut body, &font, &style, &plan)?;
            self.draw_grid(&mut body, &font, &style, &plan)?;
            self.draw_detail(&mut body, &font, &style, &plan)?;
            arrows = Some((plan.left, plan.right));
            cells = Some(Cells {
                x: plan.grid_x,
                y: plan.cells_y,
                size: plan.cell,
            });
        }
        paint::replay(frame, &font, header.paint.iter().chain(body.iter()))?;
        font.release()?;
        header.paint = Vec::new();
        self.page = header;
        self.arrows = arrows;
        self.cells = cells;
        Ok(())
    }

    /// The streak, small at the header's right end, where there is room beside
    /// the title.
    fn streak(
        &self,
        header: &mut Page,
        font: &Font<'_>,
        style: &Style,
        width: u16,
    ) -> Result<(), Error> {
        let Some(note) = &self.note else {
            return Ok(());
        };
        let note_width = font.measure_text(style.small, note)?;
        let title_width = font.measure_text(style.head, &self.heading)?;
        let x = f32::from(width) - f32::from(style.margin) - note_width;
        if x < title_width + f32::from(style.header) + 3.0 * f32::from(style.margin) {
            return Ok(());
        }
        let band = Rect::new(0, 0, width, style.header);
        let ink = if style.dark { style.paper } else { style.ink };
        let (ascent, descent, _) = geometry::line_metrics(font, style.small)?;
        let baseline = ((f32::from(band.height) + ascent - descent) / 2.0) as i32;
        paint::push(
            &mut header.paint,
            Paint::Text {
                text: text::copy(note)?,
                size: style.small,
                origin: paperpaw_sdk::native::fonts::TextOrigin::new(x, baseline),
                color: ink,
                clip: Some(band),
            },
        )
    }

    /// The month's name between its arrows.
    fn draw_nav(
        &self,
        body: &mut Vec<Paint>,
        font: &Font<'_>,
        style: &Style,
        plan: &Plan,
    ) -> Result<(), Error> {
        for (rect, arrow) in [(plan.left, "‹"), (plan.right, "›")] {
            let w = font.measure_text(style.title, arrow)?;
            let x = f32::from(rect.x) + (f32::from(rect.width) - w) / 2.0;
            text::label(body, font, arrow, style.title, x, rect, style.ink, None)?;
            paint::push(
                body,
                Paint::Focus(
                    rect,
                    style.corner,
                    Some(u32::from(style.line.max(1))),
                    style.rule,
                ),
            )?;
        }
        // A gap from each arrow (a word's space at least, at large sizes).
        let gap = f32::from(style.gap).max(style.title * 0.4);
        let from = f32::from(plan.left.x + plan.left.width) + gap;
        let room = (f32::from(plan.right.x) - gap - from).max(1.0);
        let least = style.small * 0.7;
        let mut shown = None;
        for name in &self.title {
            let (size, line) = fit_line(font, style.title, least, name, room)?;
            if line == *name {
                shown = Some((size, line));
                break;
            }
            shown = Some((size, line));
        }
        let Some((size, line)) = shown else {
            return Ok(());
        };
        let x = from + (room - font.measure_text(size, &line)?).max(0.0) / 2.0;
        text::label(body, font, &line, size, x, plan.nav, style.ink, None)
    }

    /// The weekdays' names and the month's days, shaded by the time read.
    fn draw_grid(
        &self,
        body: &mut Vec<Paint>,
        font: &Font<'_>,
        style: &Style,
        plan: &Plan,
    ) -> Result<(), Error> {
        let size = plan.cell;
        let cells = Cells {
            x: plan.grid_x,
            y: plan.cells_y,
            size,
        };
        let name_size = fit_size(
            font,
            style.small,
            style.small * 0.4,
            f32::from(size) * 0.9,
            plan.names_height,
            self.names.iter().map(String::as_str),
        )?;
        for (col, name) in (0u16..).zip(&self.names) {
            let area = Rect::new(
                plan.grid_x.saturating_add(size.saturating_mul(col)),
                plan.cells_y.saturating_sub(plan.names_height),
                size,
                plan.names_height,
            );
            let x =
                f32::from(area.x) + (f32::from(size) - font.measure_text(name_size, name)?) / 2.0;
            text::label(body, font, name, name_size, x, area, style.muted, None)?;
        }
        let pad = (style.line.saturating_mul(2)).max(2);
        let inner_size = size.saturating_sub(2 * pad);
        let number_size = fit_size(
            font,
            style.small,
            style.small * 0.4,
            f32::from(inner_size),
            inner_size,
            ["28"],
        )?;
        let first = self.first_shown();
        let month = model::month_of(self.day);
        for index in 0..42u16 {
            let day = first.saturating_add(i32::from(index));
            if model::month_of(day) != month {
                continue;
            }
            let square = cells.of(index);
            let inner = Rect::new(
                square.x.saturating_add(pad),
                square.y.saturating_add(pad),
                inner_size,
                inner_size,
            );
            let minutes = texts::seconds_on(&self.days, day) / 60;
            let gray = model::shade(minutes);
            if let Some(gray) = gray {
                paint::push(body, Paint::Fill(inner, Color::gray(gray)))?;
                // And as a bar, an hour long at most: visible on displays
                // without grays too.
                let thick = pad.max(2);
                let full = u32::from(inner.width) * 3 / 4;
                let len = (full * minutes.min(60) / 60).max(u32::from(thick) * 2) as u16;
                let bar = Rect::new(
                    inner.x.saturating_add(inner.width.saturating_sub(len) / 2),
                    (inner.y.saturating_add(inner.height)).saturating_sub(thick * 2),
                    len.min(inner.width),
                    thick,
                );
                let ink = if gray < 150 {
                    Color::WHITE
                } else {
                    Color::BLACK
                };
                paint::push(body, Paint::Fill(bar, ink))?;
            }
            let ink = if gray.is_some_and(|g| g < 150) {
                Color::WHITE
            } else {
                Color::BLACK
            };
            let mut digits = [0u8; 20];
            let number = text::decimal(usize::from(texts::date_of(day).day), &mut digits);
            let w = font.measure_text(number_size, number)?;
            let bar_room = if gray.is_some() { pad.max(2) * 4 } else { 0 };
            let area = Rect::new(
                inner.x,
                inner.y,
                inner.width,
                inner.height.saturating_sub(bar_room),
            );
            let x = f32::from(inner.x) + (f32::from(inner.width) - w) / 2.0;
            text::label(body, font, number, number_size, x, area, ink, None)?;
            let border = u32::from(style.line.max(1)).max(2);
            if day == self.day {
                paint::push(
                    body,
                    Paint::Focus(square, style.corner, Some(border * 2), Color::BLACK),
                )?;
            } else if Some(day) == self.today {
                paint::push(
                    body,
                    Paint::Focus(inner, style.corner, Some(border), style.muted),
                )?;
            }
        }
        Ok(())
    }

    /// The chosen day: its date, how long and pages, and its books as far as
    /// they fit.
    fn draw_detail(
        &self,
        body: &mut Vec<Paint>,
        font: &Font<'_>,
        style: &Style,
        plan: &Plan,
    ) -> Result<(), Error> {
        let d = plan.detail;
        let row = plan.detail_row;
        let margin = f32::from(style.margin);
        // Its texts end a margin before the edge.
        let inside = d.width.saturating_sub(style.margin);
        let line = |n: u16| Rect::new(d.x, d.y.saturating_add(row.saturating_mul(n)), inside, row);
        let room = f32::from(inside) - margin;
        // The date whole: smaller, down to the small size, where needed; then
        // with the weekday and month short.
        let (size, title) = {
            let whole = fit_line(font, style.text, style.small, &self.said.long, room)?;
            if whole.1 == self.said.long {
                whole
            } else {
                fit_line(font, style.text, style.small, &self.said.short, room)?
            }
        };
        text::label(body, font, &title, size, margin, line(0), style.ink, None)?;
        text::label(
            body,
            font,
            &self.said.summary,
            style.small,
            margin,
            line(1),
            style.note,
            None,
        )?;
        let fits = |n: u16| u32::from(row) * (u32::from(n) + 1) <= u32::from(d.height);
        let shown = (0..DAY_BOOKS as u16).take_while(|n| fits(n + 2)).count();
        for (n, (book, value)) in self.said.books.iter().take(shown).enumerate() {
            let at = line(n as u16 + 2);
            let w = font.measure_text(style.small, value)?;
            // The title ends a margin before the time.
            let name_room = (f32::from(d.width) - 2.0 * margin - w - margin).max(0.0);
            let left = Rect::new(d.x, at.y, (margin + name_room) as u16, row);
            text::label(
                body,
                font,
                book,
                style.small,
                margin,
                left,
                style.note,
                None,
            )?;
            let x = f32::from(d.width) - margin - w;
            text::label(body, font, value, style.small, x, at, style.note, None)?;
        }
        Ok(())
    }

    pub(crate) fn input(&mut self, host: &Host<'_>, input: &Input<'_>) -> Result<Handled, Error> {
        match *input {
            Input::Action { action, .. } => {
                if self.empty() {
                    return Ok(Handled::No);
                }
                let key = match action {
                    "core.left" => Key::Left,
                    "core.previous" => Key::Previous,
                    "core.right" => Key::Right,
                    "core.next" => Key::Next,
                    "core.up" => Key::Up,
                    "core.down" => Key::Down,
                    "core.prev-page" => Key::PreviousPage,
                    "core.next-page" => Key::NextPage,
                    _ => return Ok(Handled::No),
                };
                self.choose(host, model::step(self.day, model::key_step(key)))?;
                Ok(Handled::Yes)
            }
            Input::Gesture {
                kind: kind @ (InputGestureKind::SwipeLeft | InputGestureKind::SwipeRight),
                ..
            } => {
                if self.empty() {
                    return Ok(Handled::No);
                }
                let swipe = if kind == InputGestureKind::SwipeLeft {
                    Swipe::Left
                } else {
                    Swipe::Right
                };
                self.choose(host, model::step(self.day, model::swipe_step(swipe)))?;
                Ok(Handled::Yes)
            }
            Input::Gesture {
                position,
                kind: InputGestureKind::Tap,
                ..
            } => self.tap(host, position),
            _ => Ok(Handled::No),
        }
    }

    fn tap(&mut self, host: &Host<'_>, at: Point) -> Result<Handled, Error> {
        if let Some(action) = self.page.action(at) {
            let mut scratch = [0; 128];
            CommandSender::new(host, &mut scratch).run_action(action)?;
            return Ok(Handled::Yes);
        }
        if let Some((left, right)) = self.arrows {
            if left.contains(at) {
                self.choose(host, model::previous_month(self.day))?;
                return Ok(Handled::Yes);
            }
            if right.contains(at) {
                self.choose(host, model::next_month(self.day))?;
                return Ok(Handled::Yes);
            }
        }
        let Some(index) = self.cells.and_then(|cells| cells.at(at)) else {
            return Ok(Handled::No);
        };
        // A day of the weeks around the month (another month) does nothing.
        if let Some(day) = model::tap_day(
            self.day,
            self.first_shown().saturating_add(i32::from(index)),
        ) {
            self.choose(host, day)?;
        }
        Ok(Handled::Yes)
    }

    /// Chooses `day`: reads what was read on it and redraws.
    fn choose(&mut self, host: &Host<'_>, day: i32) -> Result<(), Error> {
        if day == self.day {
            return Ok(());
        }
        // The redraw is asked for first: a refusal leaves the chosen day as
        // it was, and a failure to read the day's contents only redraws the
        // screen as it was.
        let mut scratch = [0; 64];
        CommandSender::new(host, &mut scratch).queue_redraw(RedrawMode::Partial)?;
        let storage = DataStorage::new(host);
        let data = DataFolder(&storage);
        let mut words = HostWords::new(host)?;
        self.show(&mut words, &data, day)
    }

    /// Makes `day` the chosen one; nothing changes where this fails.
    fn show(&mut self, words: &mut impl Words, data: &dyn Storage, day: i32) -> Result<(), Error> {
        let (said, title) = contents(words, data, &self.books, &self.days, day)?;
        self.day = day;
        self.said = said;
        self.title = title;
        Ok(())
    }
}

/// What the screen says of `day`: the day itself and the month's name.
fn contents(
    words: &mut impl Words,
    data: &dyn Storage,
    books: &[model::Named],
    days: &[Day],
    day: i32,
) -> Result<(DayText, [String; 2]), Error> {
    let detail = model::detail(data, books, days, day, utc_day)?;
    Ok((
        model::day_text(words, day, &detail)?,
        model::title(words, day)?,
    ))
}

/// The first day of the week, 0 being Sunday: as set or, where it is left to
/// the language's habit, as the translation `core.week-start-day` says.
fn week_start_of(host: &Host<'_>, words: &mut impl Words) -> Result<u8, Error> {
    let mut scratch = [0u8; 32];
    let set = match settings::read_setting(host, "core.week-start", &mut scratch) {
        Ok(SettingValue::Text(text)) => model::week_start_set(text),
        _ => None,
    };
    match set {
        Some(day) => Ok(day),
        None => match words.copy("core.week-start-day") {
            Ok(habit) => Ok(model::week_start_habit(&habit)),
            Err(Error::NoMemory) => Err(Error::NoMemory),
            Err(_) => Ok(1),
        },
    }
}

/// Where the parts of the screen go, from the stock sizes.
#[derive(Debug, PartialEq, Eq)]
struct Plan {
    nav: Rect,
    left: Rect,
    right: Rect,
    /// Where the days start across, and the top of the first week.
    grid_x: u16,
    cells_y: u16,
    names_height: u16,
    cell: u16,
    detail: Rect,
    detail_row: u16,
}

impl Plan {
    /// `small_line` is the height of a line of text in the small size.
    fn of(style: &Style, width: u16, height: u16, small_line: u16) -> Self {
        let (row, gap, margin) = (style.row, style.gap, style.margin);
        let nav = Rect::new(0, style.header.saturating_add(gap), width, row);
        let wide = row.saturating_mul(3) / 2;
        let left = Rect::new(margin, nav.y, wide, row);
        let right = Rect::new(
            width.saturating_sub(margin).saturating_sub(wide),
            nav.y,
            wide,
            row,
        );
        // Square days across the width, unless that leaves too little room for
        // the chosen day below (its date, how long, its books): the weekdays'
        // names (half a day high) and up to six weeks.
        let across = width.saturating_sub(2 * margin) / 7;
        let least = (small_line.saturating_mul(6) / 5).min(across);
        let detail_row = (row.saturating_mul(3) / 4).max(1);
        let cell_for = |lines: u16| {
            let below = gap.saturating_add(detail_row.saturating_mul(lines));
            let room =
                height.saturating_sub(nav.y.saturating_add(nav.height).saturating_add(below));
            across.min(room.saturating_mul(2) / 13)
        };
        // Its date and how long at least; its books as far as they fit.
        let lines = (2..=DAY_BOOKS as u16 + 2)
            .rev()
            .find(|&lines| cell_for(lines) >= least)
            .unwrap_or(2);
        let cell = cell_for(lines);
        let names_height = cell / 2;
        let cells_y = nav
            .y
            .saturating_add(nav.height)
            .saturating_add(names_height);
        let top = cells_y
            .saturating_add(cell.saturating_mul(6))
            .saturating_add(gap);
        Self {
            nav,
            left,
            right,
            grid_x: width.saturating_sub(cell.saturating_mul(7)) / 2,
            cells_y,
            names_height,
            cell,
            detail: Rect::new(0, top, width, height.saturating_sub(top)),
            detail_row,
        }
    }
}

/// The largest size, down to `least`, at which each of `words` fits `wide` and
/// a line of it `high`.
fn fit_size<'a>(
    font: &Font<'_>,
    most: f32,
    least: f32,
    wide: f32,
    high: u16,
    words: impl IntoIterator<Item = &'a str> + Clone,
) -> Result<f32, Error> {
    let mut size = most;
    while size > least {
        let mut fits = geometry::line_px(font, size)? <= high;
        for word in words.clone() {
            fits &= font.measure_text(size, word)? <= wide;
        }
        if fits {
            return Ok(size);
        }
        size -= 0.5;
    }
    Ok(least)
}

#[cfg(test)]
mod tests;
