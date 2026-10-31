//! The calendar: a month, its days shaded by how long one read, and below
//! it the chosen day: its reading time, pages and books.
//!
//! Tapping a day chooses it, the arrows beside the month's name turn the
//! month. With buttons, left and right move by a day, up and down by a
//! week, page forward and back by a month.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::ui::{self, UiStyle};
use paperpaw_sdk::{
    Canvas, Color, Command, Cx, Direction, Env, Gesture, GestureKind, Handled, Id, Point, Rect,
    RefreshMode, Screen,
};

use crate::fmt::{self, month_start};
use crate::store::{self, Book, Day};
use crate::totals;

/// Most books listed for a day.
const DAY_BOOKS: usize = 3;

pub(crate) struct Calendar {
    style: UiStyle,
    header: ui::Header,
    /// The row with the month's name and its arrows.
    nav: Rect,
    grid: Rect,
    /// The height of the weekdays' names above the days.
    names: u16,
    detail: Rect,
    /// The height of a line of the chosen day's below the days.
    detail_row: u16,
    /// Laid out close (the largest text on a small screen): the month's
    /// row as high as its arrows, these as wide as high.
    close: bool,
    days: Vec<Day>,
    books: Vec<Book>,
    /// Today, if the device knows the time.
    today: Option<i32>,
    /// The chosen day: at first today, else the last day read.
    day: i32,
    /// Its books (titles and seconds) and distinct pages.
    day_books: Vec<(String, u32)>,
    day_pages: u32,
}

/// A month's name shortened to its first three letters and a dot; one
/// of four letters or fewer as it is.
fn short_month(name: &str) -> String {
    if name.chars().count() <= 4 {
        return String::from(name);
    }
    let mut short: String = name.chars().take(3).collect();
    short.push('.');
    short
}

/// Year and month of a day.
fn month_of(day: i32) -> (i32, u8) {
    let d = fmt::date_of(day);
    (d.year, d.month)
}

/// The first day of the month after the one of `day`.
fn next_month(day: i32) -> i32 {
    let start = month_start(day);
    // At most 31 days on, then back to that month's start.
    month_start(start + 31)
}

fn previous_month(day: i32) -> i32 {
    month_start(month_start(day) - 1)
}

impl Calendar {
    pub(crate) fn new(env: &Env<'_>) -> Self {
        let style = UiStyle::new(env);
        let header = ui::Header::new(env, &style, true);
        let display = env.device().display;
        let row = style.row();
        let margin = style.margin();
        // The month's arrows grow on touch screens, and their row with them
        // (as high as the arrows alone where the days need the room, below).
        let nav_row = style.touch_row(u32::from(row)) as u16;
        let mut nav = Rect::new(
            0,
            header.area().height + style.gap(),
            display.width,
            nav_row,
        );
        let width = display.width - 2 * margin;
        // Square days across the width, unless that leaves too little room
        // for the chosen day below (its date, how long, its books): the
        // weekdays' names (half a day high) and up to six weeks. Where the
        // days would be smaller than their numbers in the small size (the
        // largest text on a small screen), the chosen day's lines are a
        // line of text high and show fewer books; the days are never more
        // than the room there is, their numbers drawn smaller to fit.
        let font = style.font(env);
        let line_of = |size: f32| {
            font.as_ref()
                .map_or(size * 1.3, |f| f.line_metrics(size).line_height) as u16
        };
        let small = line_of(style.small());
        let least = (small * 6 / 5).min(width / 7);
        let cell_for = |nav: Rect, detail_row: u16, lines: u16| {
            let below = style.gap() + detail_row * lines;
            let room = display.height.saturating_sub(nav.y + nav.height + below);
            (width / 7).min(room * 2 / 13)
        };
        let mut detail_row = row * 3 / 4;
        let all = DAY_BOOKS as u16 + 2;
        let close = cell_for(nav, detail_row, all) < least;
        if close {
            detail_row = detail_row.min(line_of(style.text()));
            nav.height = ui::button_height(&style, nav) + style.gap();
        }
        // Its date and how long at least; its books as far as they fit.
        let lines = (2..=all)
            .rev()
            .find(|&lines| cell_for(nav, detail_row, lines) >= least)
            .unwrap_or(2);
        let cell = cell_for(nav, detail_row, lines);
        let names = cell / 2;
        let grid = Rect::new(
            (display.width - cell * 7) / 2,
            nav.y + nav.height,
            cell * 7,
            names + cell * 6,
        );
        let detail_top = grid.y + grid.height + style.gap();
        let detail = Rect::new(
            0,
            detail_top,
            display.width,
            display.height.saturating_sub(detail_top),
        );
        let today = fmt::today(env);
        let days = store::days(env.storage(), |unix| fmt::day_of(env, unix));
        let day = today.or(days.last().map(|d| d.day)).unwrap_or(0);
        let mut calendar = Self {
            style,
            header,
            nav,
            grid,
            names,
            detail,
            detail_row,
            close,
            days,
            books: store::books(env.storage()),
            today,
            day,
            day_books: Vec::new(),
            day_pages: 0,
        };
        calendar.load_day(env);
        calendar
    }

    fn seconds_on(&self, day: i32) -> u32 {
        fmt::seconds_on(&self.days, day)
    }

    /// What was read on the chosen day.
    fn load_day(&mut self, env: &Env<'_>) {
        let day = self.day;
        // The local day, whatever the time zone.
        let unix = |day: i32| fmt::unix_near(day).clamp(0, i64::from(u32::MAX)) as u32;
        let from = unix(day).saturating_sub(14 * 3600);
        let to = unix(day + 1).saturating_add(14 * 3600);
        let mut per_book: Vec<(u16, u32)> = Vec::new();
        let mut pages: Vec<(u16, u32)> = Vec::new();
        if self.seconds_on(day) > 0 {
            store::scan_range(env.storage(), from, to, |stat| {
                if fmt::day_of(env, stat.start) != day {
                    return;
                }
                pages.push((stat.book, stat.page));
                match per_book.iter_mut().find(|(b, _)| *b == stat.book) {
                    Some((_, s)) => *s += u32::from(stat.duration),
                    None => per_book.push((stat.book, u32::from(stat.duration))),
                }
            });
        }
        pages.sort_unstable();
        pages.dedup();
        per_book.sort_by_key(|(_, s)| core::cmp::Reverse(*s));
        self.day_pages = pages.len() as u32;
        self.day_books = per_book
            .into_iter()
            .map(|(id, s)| {
                let title = self
                    .books
                    .iter()
                    .find(|b| b.id == id)
                    .map_or_else(|| "?".to_string(), |b| b.title.clone());
                (title, s)
            })
            .collect();
    }

    /// The first day shown: the start of the week the month begins in.
    fn first_shown(&self, env: &Env<'_>) -> i32 {
        let start = month_start(self.day);
        let weekday = i32::from(fmt::date_of(start).weekday);
        let from = i32::from(fmt::week_start(env));
        start - (weekday - from).rem_euclid(7)
    }

    fn cell(&self, index: i32) -> Rect {
        let size = self.grid.width / 7;
        // Below the weekdays' names.
        let (row, col) = (index / 7, index % 7);
        Rect::new(
            self.grid.x + size * col as u16,
            self.grid.y + self.names + size * row as u16,
            size,
            size,
        )
    }

    /// The month's arrows.
    fn arrows(&self) -> (Rect, Rect) {
        let n = self.nav;
        let size = ui::button_height(&self.style, n);
        let margin = self.style.margin();
        let y = n.y + (n.height - size) / 2;
        let wide = if self.close { size } else { size * 3 / 2 };
        (
            Rect::new(margin, y, wide, size),
            Rect::new(n.width - margin - wide, y, wide, size),
        )
    }

    fn choose(&mut self, day: i32, cx: &mut Cx<'_, '_>) {
        if day == self.day {
            return;
        }
        let whole = month_of(day) != month_of(self.day);
        let left = self.day;
        self.day = day;
        let env = *cx.env();
        self.load_day(&env);
        if whole {
            cx.request_redraw();
            cx.request_refresh(RefreshMode::Partial);
        } else {
            // The day left, the day chosen and what is said of it.
            let first = self.first_shown(&env);
            cx.request_redraw_area(self.cell(left - first));
            cx.request_redraw_area(self.cell(day - first));
            cx.request_redraw_area(self.detail);
            cx.request_refresh(RefreshMode::Fast);
        }
    }

    /// The weekdays' names above the days, as large as their room holds
    /// them (the small size at most); and the size the days' numbers are
    /// in, so.
    fn draw_names(&self, ui: &mut ui::Ui<'_>, font: &paperpaw_sdk::FontHandle<'_>) -> f32 {
        let style = self.style;
        let env = ui.env();
        let size = self.grid.width / 7;
        let from = fmt::week_start(env);
        let fit = |high: u16, wide: f32, widest: &dyn Fn(f32) -> f32| {
            paperpaw_sdk::widgets::largest_size(style.small(), style.small() * 0.4, |at| {
                font.line_metrics(at).line_height <= f32::from(high) && widest(at) <= wide
            })
            .unwrap_or(style.small() * 0.4)
        };
        let names: Vec<&str> = (0..7u8)
            .map(|col| fmt::weekday_short(env, (from + col) % 7))
            .collect();
        let name_size = fit(self.names, f32::from(size) * 0.9, &|at| {
            names
                .iter()
                .map(|n| font.measure(at, n))
                .fold(0.0, f32::max)
        });
        for (col, name) in (0u16..).zip(&names) {
            let area = Rect::new(self.grid.x + size * col, self.grid.y, size, self.names);
            let x = f32::from(area.x) + (f32::from(size) - font.measure(name_size, name)) / 2.0;
            ui::Label::new(name)
                .px(name_size)
                .color(style.faint(ui::Ink::MUTED))
                .draw_from(ui, x, area);
        }
        let pad = (u32::from(style.line()) * 2).max(2) as u16;
        let inner = size.saturating_sub(2 * pad);
        fit(inner, f32::from(inner), &|at| font.measure(at, "28"))
    }

    fn draw_grid(&self, ui: &mut ui::Ui<'_>) {
        let style = self.style;
        let env = ui.env();
        let Some(font) = ui.font() else {
            return;
        };
        let number_size = self.draw_names(ui, &font);
        let pad = (u32::from(style.line()) * 2).max(2) as u16;
        let first = self.first_shown(env);
        let (year, month) = month_of(self.day);
        for index in 0..42 {
            let day = first + index;
            if month_of(day) != (year, month) {
                continue;
            }
            let r = self.cell(index);
            let inner = Rect::new(
                r.x + pad,
                r.y + pad,
                r.width.saturating_sub(2 * pad),
                r.height.saturating_sub(2 * pad),
            );
            let minutes = self.seconds_on(day) / 60;
            // Darker the longer one read.
            let shade = match minutes {
                0 => None,
                1..=14 => Some(220),
                15..=29 => Some(180),
                30..=59 => Some(120),
                _ => Some(40),
            };
            if let Some(gray) = shade {
                ui.canvas().fill(inner, Color::gray(gray));
                // And as a bar, an hour long at most: visible on displays
                // without grays too.
                let full = u32::from(inner.width) * 3 / 4;
                let len = (full * minutes.min(60) / 60).max(u32::from(style.line()).max(2) * 2);
                let thick = (u32::from(style.line()) * 2).max(2);
                let bar_ink = if gray < 150 {
                    Color::WHITE
                } else {
                    Color::BLACK
                };
                ui.canvas().fill_rect(
                    i32::from(inner.x) + (i32::from(inner.width) - len as i32) / 2,
                    i32::from(inner.y + inner.height) - thick as i32 * 2,
                    len,
                    thick,
                    bar_ink,
                );
            }
            let ink = if shade.is_some_and(|g| g < 150) {
                Color::WHITE
            } else {
                Color::BLACK
            };
            let number = format!("{}", fmt::date_of(day).day);
            // Each day a part to choose, the chosen one focused.
            ui.canvas()
                .describe(paperpaw_sdk::semantics::Role::Button, &number, r)
                .focused(day == self.day);
            let x = f32::from(inner.x)
                + (f32::from(inner.width) - font.measure(number_size, &number)) / 2.0;
            // Above the bar of reading time, if there is one.
            let bar_room = if shade.is_some() {
                (u32::from(style.line()) * 2).max(2) as u16 * 4
            } else {
                0
            };
            let text = Rect::new(
                inner.x,
                inner.y,
                inner.width,
                inner.height.saturating_sub(bar_room),
            );
            ui::Label::new(&number)
                .px(number_size)
                .color(ink)
                .draw_from(ui, x, text);
            let border = ui::button_border(&style);
            if day == self.day {
                ui.canvas().stroke(r, border * 2, Color::BLACK);
            } else if Some(day) == self.today {
                ui.canvas()
                    .stroke(inner, border, style.faint(ui::Ink::MUTED));
            }
        }
    }

    fn draw_detail(&self, ui: &mut ui::Ui<'_>) {
        let style = self.style;
        let env = ui.env();
        let Some(font) = ui.font() else {
            return;
        };
        let d = self.detail;
        let row = self.detail_row;
        // Its texts end a margin before the edge.
        let inside = d.width.saturating_sub(style.margin());
        let line = |n: u16| Rect::new(d.x, d.y + row * n, inside, row);
        // The lines the area holds (the books last).
        let fits = |n: u16| row * (n + 1) <= d.height;
        let margin = style.margin() as f32;
        let seconds = self.seconds_on(self.day);
        let summary = if seconds == 0 {
            env.i18n().tr("stats.not-read").to_string()
        } else {
            format!(
                "{} · {}",
                fmt::duration(env, u64::from(seconds)),
                env.i18n()
                    .plural("stats.pages", i64::from(self.day_pages), &[])
            )
        };
        // The date whole: smaller, down to the small size, where needed;
        // then with the weekday and month short.
        let room = f32::from(inside) - margin;
        let month = fmt::month(env, fmt::date_of(self.day).month);
        let dates = [
            fmt::long_date(env, self.day),
            fmt::short_date(env, self.day, &short_month(month)),
        ];
        let fitted = |date: &String| {
            paperpaw_sdk::widgets::fit_line(&font, style.text(), style.small(), date, room)
        };
        let (size, title) = dates
            .iter()
            .map(fitted)
            .zip(&dates)
            .find(|((_, shown), date)| shown == *date)
            .map_or_else(|| fitted(&dates[1]), |(fit, _)| fit);
        ui::Label::new(&title)
            .px(size)
            .color(Color::BLACK)
            .draw_from(ui, margin, line(0));
        ui::Label::new(&summary)
            .px(style.small())
            .color(style.faint(ui::Ink::NOTE))
            .draw_from(ui, margin, line(1));
        let shown = (0..DAY_BOOKS as u16).take_while(|n| fits(n + 2)).count();
        for (n, (book, s)) in self.day_books.iter().take(shown).enumerate() {
            let value = fmt::duration(env, u64::from(*s));
            let width = f32::from(d.width) - 2.0 * margin;
            let (book, value, vw) = ui::fit_pair(&font, style.small(), book, &value, width, margin);
            let r = line(n as u16 + 2);
            ui::Label::new(&book)
                .px(style.small())
                .color(style.faint(ui::Ink::NOTE))
                .draw_from(ui, margin, r);
            ui::Label::new(&value)
                .px(style.small())
                .color(style.faint(ui::Ink::NOTE))
                .draw_from(ui, f32::from(d.width) - margin - vw, r);
        }
    }
}

impl Screen for Calendar {
    fn hint(&self, action: &Id, _: &Env<'_>) -> paperpaw_sdk::Hint {
        // Days are moved through; choosing one does nothing more.
        if *action == actions::CONFIRM {
            paperpaw_sdk::Hint::Hidden
        } else {
            paperpaw_sdk::Hint::Default
        }
    }

    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let title = env.i18n().tr("stats.calendar");
        let streak = self
            .today
            .map_or(0, |today| totals::streak(&self.days, today));
        let note = (streak > 1).then(|| {
            env.i18n()
                .plural("stats.streak-note", i64::from(streak), &[])
        });
        let mut ui = ui::Ui::new(canvas, env, &style);
        self.header.draw(&mut ui, title, note.as_deref(), None);
        if self.today.is_none() && self.days.is_empty() {
            // No day to show: neither today nor one read.
            let area = Rect::new(
                0,
                self.nav.y,
                self.nav.width,
                (self.detail.y + self.detail.height).saturating_sub(self.nav.y),
            );
            ui.message(area, env.i18n().tr("stats.no-clock"));
            return;
        }
        let (left, right) = self.arrows();
        ui.button(left, "‹", false, false);
        ui.button(right, "›", false, false);
        if let Some(font) = ui.font() {
            let (year, month) = month_of(self.day);
            let name = format!("{} {year}", fmt::month(env, month));
            // Between the arrows, a gap from each (a word's space at least,
            // at large sizes): smaller where it does not fit there.
            let gap = (style.gap() as f32).max(style.title() * 0.4);
            let from = f32::from(left.x + left.width) + gap;
            let room = (f32::from(right.x) - gap - from).max(1.0);
            // On one line, smaller where needed; else month and year on
            // two; else the month's name shortened ("Oct. 2026") so.
            let short = format!("{} {year}", short_month(fmt::month(env, month)));
            let high = font.line_metrics(style.small()).line_height as u16;
            let least = style.small() * 0.7;
            let shown = [&name, &short].into_iter().find_map(|text| {
                let (size, line) =
                    paperpaw_sdk::widgets::fit_line(&font, style.title(), least, text, room);
                if line == *text {
                    return Some((size, alloc::vec![0..text.len()], text));
                }
                let lines = font.wrap(style.small(), text, room);
                (lines.len() == 2 && 2 * high <= self.nav.height).then_some((
                    style.small(),
                    lines,
                    text,
                ))
            });
            let (size, lines, text) = shown.unwrap_or((least, alloc::vec![0..short.len()], &short));
            let line = font.line_metrics(size).line_height as u16;
            let count = lines.len() as u16;
            let top = if count > 1 {
                self.nav.y + self.nav.height.saturating_sub(count * line) / 2
            } else {
                self.nav.y
            };
            for (i, range) in lines.into_iter().enumerate() {
                let part = ui::fit(&font, size, text[range].trim(), room + 1.0);
                let x = from + (room - font.measure(size, &part)).max(0.0) / 2.0;
                let at = if count > 1 {
                    Rect::new(self.nav.x, top + i as u16 * line, self.nav.width, line)
                } else {
                    self.nav
                };
                ui::Label::new(&part)
                    .px(size)
                    .color(Color::BLACK)
                    .draw_from(&mut ui, x, at);
            }
        }
        self.draw_grid(&mut ui);
        self.draw_detail(&mut ui);
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let a = action;
        let day = if *a == actions::LEFT || *a == actions::PREVIOUS {
            self.day - 1
        } else if *a == actions::RIGHT || *a == actions::NEXT {
            self.day + 1
        } else if *a == actions::UP {
            self.day - 7
        } else if *a == actions::DOWN {
            self.day + 7
        } else if *a == actions::PREV_PAGE {
            previous_month(self.day)
        } else if *a == actions::NEXT_PAGE {
            next_month(self.day)
        } else {
            return Handled::No;
        };
        self.choose(day, cx);
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        let env = *cx.env();
        // A swipe turns the month, like a page.
        match gesture.kind {
            GestureKind::Swipe(Direction::Left) => return self.on_action(&actions::NEXT_PAGE, cx),
            GestureKind::Swipe(Direction::Right) => return self.on_action(&actions::PREV_PAGE, cx),
            _ => {}
        }
        if gesture.kind != GestureKind::Tap {
            return Handled::No;
        }
        let p: Point = gesture.start;
        if let Some(action) = self.header.tap(&env, &self.style, p, None) {
            cx.command(Command::run(action));
            return Handled::Yes;
        }
        let (left, right) = self.arrows();
        if left.contains(p) {
            self.choose(previous_month(self.day), cx);
            return Handled::Yes;
        }
        if right.contains(p) {
            self.choose(next_month(self.day), cx);
            return Handled::Yes;
        }
        let first = self.first_shown(&env);
        let hit = (0..42).find(|&i| self.cell(i).contains(p));
        if let Some(index) = hit {
            let day = first + index;
            if month_of(day) == month_of(self.day) {
                self.choose(day, cx);
            }
            return Handled::Yes;
        }
        Handled::No
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The local day of a date.
    fn day(year: i32, month: u8, d: u8) -> i32 {
        (0..30_000)
            .find(|&n| {
                let t = fmt::date_of(n);
                (t.year, t.month, t.day) == (year, month, d)
            })
            .unwrap()
    }

    #[test]
    fn months_turn() {
        let d = day(2026, 1, 31);
        assert_eq!(month_start(d), day(2026, 1, 1));
        assert_eq!(next_month(d), day(2026, 2, 1));
        assert_eq!(next_month(day(2026, 12, 15)), day(2027, 1, 1));
        assert_eq!(previous_month(day(2026, 3, 31)), day(2026, 2, 1));
        assert_eq!(previous_month(day(2026, 1, 5)), day(2025, 12, 1));
    }
}
