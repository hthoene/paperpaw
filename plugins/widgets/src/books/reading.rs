//! "Now reading": the book being read, its title and author, the page it
//! is at, a bar of how far, the chapter and the pages left in it, and the
//! time left to read them at one's speed.
//!
//! No plugin may read the chapter a reader is at, nor the pages it
//! counted, from what the library keeps; the standard reading events
//! carry them. So this widget records them itself ([`Recorder`]): the
//! page and the chapter of the book read last, and how long a page took
//! (the time between pages turned, pauses left out), kept small in
//! `.paperpaw/plugins/widgets/reading.ini` when the book is closed.

use alloc::boxed::Box;
use alloc::format;
use alloc::rc::Rc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cell::RefCell;

use paperpaw_sdk::builtin::{actions, events};
use paperpaw_sdk::ui::UiStyle;
use paperpaw_sdk::widgets::{
    self, Align, Frame, Padding, TextSize, Widget, WidgetLook, WidgetSpec, WidgetTarget,
};
use paperpaw_sdk::{
    Arg, Canvas, Command, Env, Id, Rect, RegisterError, Registrar, Storage, config,
};

use super::{
    Book, Covers, FitMemo, Lines, Role, add_settings, current, draw_bar, draw_cover, draw_message,
    frame_height, percent_text, text_size,
};
use crate::PLUGIN;
use paperpaw_sdk::SettingSpec;

pub(crate) const ID: Id = Id::new_static(WIDGET);
/// Its id, as text.
const WIDGET: &str = "widgets.reading";
/// The time between two pages turned that counts as reading, in
/// milliseconds: shorter is leafing through, longer a pause.
const READ_PAGE: core::ops::RangeInclusive<u64> = 2_000..=600_000;
/// Pages the speed is worked out from; beyond them the older count half.
const SPEED_PAGES: u32 = 400;
/// Pages read before there is a speed to go by.
const MIN_PAGES: u32 = 10;

/// Where the book read last is, and how long its pages take.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Position {
    pub path: String,
    /// The page (from 1) and the pages, once the reader counted them.
    pub page: Option<u32>,
    pub pages: Option<u32>,
    /// The chapter's title (empty where not known) and the pages after
    /// this one in it, once counted.
    pub chapter: String,
    pub chapter_left: Option<u32>,
    /// Seconds read and the pages they took, for the speed.
    pub seconds: u32,
    pub turned: u32,
}

impl Position {
    /// A page turned after `millis` on the last one.
    pub(crate) fn turned(&mut self, millis: u64) {
        if !READ_PAGE.contains(&millis) {
            return;
        }
        self.seconds = self.seconds.saturating_add((millis / 1000) as u32);
        self.turned += 1;
        if self.turned > SPEED_PAGES {
            self.seconds /= 2;
            self.turned /= 2;
        }
    }

    /// Seconds left to read in the book, if its pages are counted and
    /// enough was read for a speed.
    pub(crate) fn left(&self) -> Option<u64> {
        let pages = self.pages?;
        self.seconds_for(pages.saturating_sub(self.page?))
    }

    /// Seconds left to read in the chapter, likewise.
    pub(crate) fn chapter_seconds(&self) -> Option<u64> {
        self.seconds_for(self.chapter_left?)
    }

    /// Seconds `pages` take at one's speed, once there is one.
    fn seconds_for(&self, pages: u32) -> Option<u64> {
        (self.turned >= MIN_PAGES)
            .then(|| u64::from(pages) * u64::from(self.seconds) / u64::from(self.turned))
    }

    fn file() -> String {
        format!("{}/reading.ini", Env::plugin_dir(PLUGIN.as_str()))
    }

    pub(crate) fn load(storage: &dyn Storage) -> Position {
        let Ok(data) = paperpaw_sdk::read_file(storage, &Self::file(), 4096) else {
            return Position::default();
        };
        let mut position = Position::default();
        let source = core::str::from_utf8(&data).unwrap_or_default();
        for entry in config::parse(source).filter_map(Result::ok) {
            match entry.key {
                "path" => position.path = entry.value.as_str().into_owned(),
                "page" => position.page = entry.value.as_u32(),
                "pages" => position.pages = entry.value.as_u32(),
                "chapter" => position.chapter = entry.value.as_str().into_owned(),
                "chapter-left" => position.chapter_left = entry.value.as_u32(),
                "seconds" => position.seconds = entry.value.as_u32().unwrap_or(0),
                "turned" => position.turned = entry.value.as_u32().unwrap_or(0),
                _ => {}
            }
        }
        position
    }

    pub(crate) fn save(&self, storage: &dyn Storage) {
        let mut ini = String::new();
        let mut set = |key: &str, value: &str| ini = config::set(&ini, "reading", key, value);
        set("path", &self.path);
        if let Some(page) = self.page {
            set("page", &page.to_string());
        }
        if let Some(pages) = self.pages {
            set("pages", &pages.to_string());
        }
        if !self.chapter.is_empty() {
            set("chapter", &self.chapter);
        }
        if let Some(left) = self.chapter_left {
            set("chapter-left", &left.to_string());
        }
        set("seconds", &self.seconds.to_string());
        set("turned", &self.turned.to_string());
        let _ = storage.create_dir(&Env::plugin_dir(PLUGIN.as_str()));
        let _ = storage.write(&Self::file(), ini.as_bytes());
    }
}

/// Records the position from the reading events, kept in memory while
/// reading and written when the book is closed or the device sleeps or
/// restarts.
#[derive(Debug, Default)]
pub(crate) struct Recorder {
    position: Option<Position>,
    /// When the last page was turned (milliseconds since start).
    last: Option<u64>,
    unsaved: bool,
}

pub(crate) type Shared = Rc<RefCell<Recorder>>;

impl Recorder {
    /// The position, read from its file the first time.
    pub(crate) fn position(&mut self, storage: &dyn Storage) -> &mut Position {
        self.position.get_or_insert_with(|| Position::load(storage))
    }

    fn reading(&mut self, env: &Env<'_>, reading: &events::Reading, turned: bool) {
        let now = env.now().millis();
        let last = self.last.replace(now);
        let position = self.position(env.storage());
        if position.path != reading.path {
            position.path = reading.path.clone();
        } else if turned && let Some(last) = last {
            position.turned(now.saturating_sub(last));
        }
        position.page = reading.page;
        position.pages = reading.pages;
        if position.chapter != reading.chapter {
            position.chapter.clone_from(&reading.chapter);
        }
        position.chapter_left = reading.chapter_left;
        self.unsaved = true;
    }

    fn save(&mut self, storage: &dyn Storage) {
        if let (true, Some(position)) = (self.unsaved, &self.position) {
            position.save(storage);
            self.unsaved = false;
        }
    }
}

/// Its settings, kept in the program.
static SETTINGS: [SettingSpec; 17] = [
    widgets::align_spec("widgets.reading.align", ID, Align::Left),
    widgets::frame_spec("widgets.reading.frame", ID, Frame::None),
    widgets::padding_spec("widgets.reading.padding", ID, Padding::Auto),
    widgets::paper_spec("widgets.reading.paper", ID),
    widgets::ink_spec("widgets.reading.ink", ID),
    widgets::corners_spec("widgets.reading.corners", ID),
    widgets::frame_line_spec("widgets.reading.frame-line", ID),
    widgets::shadow_spec("widgets.reading.drop-shadow", ID),
    widgets::size_spec("widgets.reading.size", ID, TextSize::Medium, false),
    widgets::part_spec(
        "widgets.reading.caption",
        ID,
        "widgets.books.part.caption",
        true,
    ),
    widgets::part_spec(
        "widgets.reading.cover",
        ID,
        "widgets.books.part.cover",
        true,
    ),
    widgets::part_spec(
        "widgets.reading.author",
        ID,
        "widgets.books.part.author",
        true,
    ),
    widgets::part_spec(
        "widgets.reading.pages",
        ID,
        "widgets.books.part.pages",
        true,
    ),
    widgets::part_spec("widgets.reading.bar", ID, "widgets.books.part.bar", true),
    widgets::part_spec("widgets.reading.left", ID, "widgets.books.part.left", true),
    widgets::part_spec(
        "widgets.reading.chapter",
        ID,
        "widgets.books.part.chapter",
        true,
    ),
    widgets::part_spec(
        "widgets.reading.chapter-left",
        ID,
        "widgets.books.part.chapter-left",
        true,
    ),
];

pub(crate) fn register(r: &mut Registrar) -> Result<(), RegisterError> {
    let recorder: Shared = Rc::default();
    for (event, turned) in [(events::BOOK_OPENED, false), (events::PAGE_TURNED, true)] {
        let recorder = recorder.clone();
        r.on(event, move |env, reading, _| {
            recorder.borrow_mut().reading(env, reading, turned);
        })?;
    }
    let saving = recorder.clone();
    r.on(events::BOOK_CLOSED, move |env, reading, _| {
        let mut recorder = saving.borrow_mut();
        recorder.reading(env, reading, false);
        recorder.last = None;
        recorder.save(env.storage());
    })?;
    for event in [events::SUSPEND, events::STOPPING] {
        let saving = recorder.clone();
        r.on(event, move |env, _, _| {
            saving.borrow_mut().save(env.storage());
        })?;
    }

    add_settings(
        r,
        {
            static SPEC: paperpaw_sdk::SettingGroupSpec =
                paperpaw_sdk::SettingGroupSpec::new(ID, "widgets.reading");
            &SPEC
        },
        &SETTINGS,
    )?;
    r.add_widget(
        WidgetSpec::new(ID, "widgets.reading", move |_| {
            Box::new(Reading {
                recorder: recorder.clone(),
                shown: None,
                area: None,
                covers: Covers::default(),
                memo: Memo::default(),
            })
        })
        .category(widgets::categories::BOOKS),
    )
}

/// The book being read and where in it, read when updated.
struct Reading {
    recorder: Shared,
    shown: Option<(Book, Position)>,
    /// Where it is drawn, once placed.
    area: Option<Rect>,
    covers: Covers,
    /// Its text made to fit, kept for the frames that follow.
    memo: Memo,
}

/// What drawing "Now reading" keeps between frames: the text laid out in
/// its place (until the book or where it is read changes, or what it
/// depends on) and its lines made to fit.
#[derive(Debug, Default)]
struct Memo {
    lines: FitMemo,
    layout: RefCell<Option<(String, Layout)>>,
}

impl Memo {
    fn forget(&mut self) {
        *self.layout.get_mut() = None;
    }

    /// The layout made for `key`, as kept, else as `make` makes it.
    fn layout(&self, key: String, make: impl FnOnce() -> Layout) -> Layout {
        if let Some((kept, layout)) = &*self.layout.borrow()
            && *kept == key
        {
            return layout.clone();
        }
        let layout = make();
        *self.layout.borrow_mut() = Some((key, layout.clone()));
        layout
    }
}

impl Reading {
    /// The text laid out for where it goes, so that the first frame
    /// finds it done.
    fn make_layout(&self, env: &Env<'_>) {
        if let (Some(area), Some((book, position))) = (self.area, &self.shown) {
            let area = widgets::widget_look(env, WIDGET, area).area;
            let (at, rest) = text_place(env, area, book);
            let key = layout_key(env, rest, at.is_some());
            self.memo.layout(key, || layout(env, rest, book, position));
        }
    }

    /// The cover shown, made small for where it goes.
    fn make_covers(&mut self, env: &Env<'_>) {
        self.covers = match (self.area, &self.shown) {
            (Some(area), Some((book, _))) => {
                let area = widgets::widget_look(env, WIDGET, area).area;
                Covers::make(env, cover_at(env, area, book).map(|at| (book, at)))
            }
            _ => Covers::default(),
        };
    }
}

impl Widget for Reading {
    fn update(&mut self, env: &Env<'_>) -> bool {
        let shown = current(env.storage()).map(|book| {
            let mut recorder = self.recorder.borrow_mut();
            let position = recorder.position(env.storage());
            let position = if position.path == book.path {
                position.clone()
            } else {
                Position::default()
            };
            (book, position)
        });
        let changed = shown != self.shown;
        self.shown = shown;
        if changed {
            self.memo.forget();
            self.make_covers(env);
            self.make_layout(env);
        }
        changed
    }

    fn place(&mut self, env: &Env<'_>, area: Rect) {
        if self.area != Some(area) {
            self.area = Some(area);
            self.make_covers(env);
            self.make_layout(env);
        }
    }

    fn live(&self) -> bool {
        false
    }

    fn height(&self, env: &Env<'_>, _: u16) -> u16 {
        (u32::from(UiStyle::new(env).row()) * 3) as u16 + frame_height(env, WIDGET)
    }

    fn min_height(&self, env: &Env<'_>, _: u16) -> u16 {
        (u32::from(UiStyle::new(env).row()) * 2) as u16 + frame_height(env, WIDGET)
    }

    fn draw(&self, canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
        let look = widgets::draw_frame(canvas, env, WIDGET, area);
        match &self.shown {
            Some((book, position)) => draw(
                canvas,
                env,
                &look,
                (book, position),
                (&self.covers, &self.memo),
            ),
            None => draw_message(canvas, env, &look, "widgets.books.nothing-read"),
        }
    }

    fn targets(&self, _: &Env<'_>, area: Rect) -> Vec<WidgetTarget> {
        let command = self
            .shown
            .as_ref()
            .map_or(Command::run(actions::LIBRARY), |(book, _)| book.open());
        alloc::vec![WidgetTarget::new(area, command)]
    }
}

/// "Page 34 of 210 · 16 %", or the percentage alone while the pages are
/// not counted.
pub(crate) fn where_text(env: &Env<'_>, book: &Book, position: &Position) -> String {
    let percent = percent_text(env, book.percent());
    match (position.page, position.pages) {
        (Some(page), Some(pages)) if pages > 0 => {
            let page = env.i18n().format(
                "widgets.reading.page",
                &[
                    ("page", Arg::Int(i64::from(page))),
                    ("pages", Arg::Int(i64::from(pages))),
                ],
            );
            format!("{page} · {percent}")
        }
        _ => env.i18n().format(
            "widgets.reading.percent",
            &[("percent", Arg::Str(&percent))],
        ),
    }
}

/// "About 2 h 5 min left", "About 40 min left".
pub(crate) fn left_text(env: &Env<'_>, seconds: u64) -> String {
    duration(env, seconds, "widgets.reading.left")
}

/// `seconds` in whole minutes (at least one), as message `key`-minutes
/// or `key`-hours says.
fn duration(env: &Env<'_>, seconds: u64, key: &str) -> String {
    let minutes = seconds.div_ceil(60).max(1);
    let (hours, minutes) = (minutes / 60, minutes % 60);
    if hours == 0 {
        env.i18n().format(
            &format!("{key}-minutes"),
            &[("minutes", Arg::Int(minutes as i64))],
        )
    } else {
        env.i18n().format(
            &format!("{key}-hours"),
            &[
                ("hours", Arg::Int(hours as i64)),
                ("minutes", Arg::Int(minutes as i64)),
            ],
        )
    }
}

/// What is left of the chapter, the longest way that fits `width` at
/// `size`: "12 pages left in the chapter · about 6 min" where the time is
/// known and wanted (`timed`), "12 pages left · about 6 min" below the
/// chapter's title (`titled`), or either without the time; "Last page of
/// the chapter" on its last.
pub(crate) fn chapter_left_text(
    env: &Env<'_>,
    position: &Position,
    timed: bool,
    titled: bool,
    size: f32,
    width: u16,
) -> Option<String> {
    let left = position.chapter_left?;
    let font = UiStyle::new(env).font(env);
    let fits = |text: &str| {
        font.as_ref()
            .is_none_or(|font| font.measure(size, text) <= f32::from(width))
    };
    if left == 0 {
        // The long way, else a short one that still says "chapter", else
        // the long way to be broken in lines.
        let last = env.i18n().tr("widgets.reading.chapter-last");
        let short = env.i18n().tr("widgets.reading.chapter-last-short");
        let shown = if !fits(last) && fits(short) {
            short
        } else {
            last
        };
        return Some(shown.to_string());
    }
    let count = i64::from(left);
    let long = env
        .i18n()
        .plural("widgets.reading.chapter-left", count, &[]);
    let short = titled.then(|| {
        env.i18n()
            .plural("widgets.reading.chapter-short", count, &[])
    });
    let time = position
        .chapter_seconds()
        .filter(|_| timed)
        .map(|seconds| duration(env, seconds, "widgets.reading.chapter"));
    let with_time = |pages: &str| {
        time.as_ref().map(|time| {
            env.i18n().format(
                "widgets.reading.chapter-time",
                &[("left", Arg::Str(pages)), ("time", Arg::Str(time))],
            )
        })
    };
    let mut ways = Vec::with_capacity(4);
    ways.extend(with_time(&long));
    ways.extend(short.as_deref().and_then(with_time));
    ways.push(long);
    ways.extend(short);
    let at = ways
        .iter()
        .position(|way| fits(way))
        .unwrap_or(ways.len() - 1);
    Some(ways.swap_remove(at))
}

/// The parts of "Now reading" beside the cover, in the order they are
/// drawn. Where there is too little room, they give way as [`DROPPED`]
/// says; the title never.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Part {
    Caption,
    Author,
    Bar,
    Pages,
    Left,
    Chapter,
    ChapterLeft,
}

/// The parts in the order they give way: the time left in the book
/// first, the heading and the author (known anyway) next, then what is
/// left of the chapter, the bar (the percentage says it too), the
/// chapter, the page last.
const DROPPED: [Part; 7] = [
    Part::Left,
    Part::Caption,
    Part::Author,
    Part::ChapterLeft,
    Part::Bar,
    Part::Chapter,
    Part::Pages,
];

/// Where the cover goes in `area` (inside the frame): on the left, where
/// shown and there is room for the text beside it.
fn cover_at(env: &Env<'_>, area: Rect, book: &Book) -> Option<Rect> {
    let style = UiStyle::new(env);
    let (w, h) = super::cover_size(book, area.width / 3, area.height);
    let text_room = area.width.saturating_sub(w + style.gap());
    (widgets::shows(env, WIDGET, "cover") && f32::from(text_room) >= style.small() * 7.0)
        .then(|| Rect::new(area.x, area.y + (area.height - h) / 2, w, h))
}

/// What the text of "Now reading" comes to in `rest` (the place beside
/// the cover): the parts that stay, the lines they are broken into and
/// how high they are together.
#[derive(Debug, Clone)]
struct Layout {
    parts: Vec<Part>,
    left: Option<String>,
    title_size: f32,
    title: Vec<String>,
    pages: Vec<String>,
    chapter: Vec<String>,
    chapter_left: Vec<String>,
    total: u16,
}

/// Where the cover goes in `area` (inside the frame), if shown, and the
/// place left for the text.
fn text_place(env: &Env<'_>, area: Rect, book: &Book) -> (Option<Rect>, Rect) {
    let at = cover_at(env, area, book);
    let rest = match at {
        Some(at) => {
            let text_room = area
                .width
                .saturating_sub(at.width + UiStyle::new(env).gap());
            Rect::new(
                area.x + area.width - text_room,
                area.y,
                text_room,
                area.height,
            )
        }
        None => area,
    };
    (at, rest)
}

#[expect(
    clippy::too_many_lines,
    reason = "longer than the limit from before it was set; split it when it is next changed"
)]
fn layout(env: &Env<'_>, rest: Rect, book: &Book, position: &Position) -> Layout {
    let style = UiStyle::new(env);
    let gap = style.gap();
    let id = WIDGET;
    let timed = widgets::shows(env, id, "left");
    let left = position.left().filter(|_| timed).map(|s| left_text(env, s));
    let font = style.font(env);
    let width = f32::from(rest.width);
    // Whether a small line fits the width whole.
    let whole = |text: &str| {
        font.as_ref()
            .is_none_or(|font| font.measure(style.small(), text) <= width)
    };
    let mut parts: Vec<Part> = [
        (Part::Caption, "caption", true),
        (Part::Author, "author", !book.author.is_empty()),
        (Part::Bar, "bar", true),
        (Part::Pages, "pages", true),
        (Part::Left, "left", left.is_some()),
        (Part::Chapter, "chapter", !position.chapter.is_empty()),
        (
            Part::ChapterLeft,
            "chapter-left",
            position.chapter_left.is_some(),
        ),
    ]
    .into_iter()
    .filter(|(_, name, there)| *there && widgets::shows(env, id, name))
    .map(|(part, _, _)| part)
    .collect();
    let small = widgets::line_height(env, style.small());
    let line_gap = gap / 4;
    let bar_h = (style.line() * 3).max(4);
    // A title too wide on one line in the body size, as the lines are
    // drawn ([`Lines`]).
    let title_size = text_size(env, id);
    let title_size = match font.as_ref() {
        Some(font) if font.measure(title_size, &book.title) > width => {
            let least = title_size.min(style.text());
            widgets::largest_size(title_size, least, |s| font.measure(s, &book.title) <= width)
                .unwrap_or(least)
        }
        _ => title_size,
    };
    // A line of the title no higher than the place: smaller in a flat
    // cell, down to a little below the small size.
    let least = style.small().min(title_size) * 0.8;
    let title_size = widgets::largest_size(title_size, least, |s| {
        widgets::line_height(env, s) <= rest.height
    })
    .unwrap_or(least);
    // The chapter set a little apart from the book's page and time.
    let apart = gap / 2;
    let height = |parts: &[Part]| -> u16 {
        let mut total = widgets::line_height(env, title_size);
        for part in parts {
            total += match part {
                Part::Bar => bar_h + gap,
                _ => small + line_gap,
            };
        }
        let book = parts.iter().any(|p| matches!(p, Part::Pages | Part::Left));
        let chapter = parts
            .iter()
            .any(|p| matches!(p, Part::Chapter | Part::ChapterLeft));
        total + if book && chapter { apart } else { 0 }
    };
    for drop in DROPPED {
        if height(&parts) <= rest.height {
            break;
        }
        parts.retain(|p| *p != drop);
    }
    // Room to spare: a long title in more lines, then a long chapter and
    // what is left of it, each broken between words only (else on one
    // line, shortened).
    let mut total = height(&parts);
    // In more lines where there is room: whole words, else (with `cut`)
    // up to the word that has to be shortened; else one line.
    // `heading`: the title's lines, as a heading's closer than others.
    let wrapped = |total: &mut u16, size: f32, text: &str, cut: bool, heading: bool| {
        let line = if heading {
            widgets::heading_line_height(env, size)
        } else {
            widgets::line_height(env, size) + line_gap
        };
        let more = usize::from(rest.height.saturating_sub(*total) / line);
        let lines = match font.as_ref() {
            Some(font) if more > 0 => {
                let lines = super::wrap_words(font, size, text, rest.width, more + 1);
                if lines.len() == 1 && cut && font.measure(size, text) > width {
                    super::cut_lines(font, size, text, rest.width, more + 1)
                } else {
                    lines
                }
            }
            _ => alloc::vec![String::from(text)],
        };
        *total += line * (lines.len() as u16 - 1);
        lines
    };
    let title = wrapped(&mut total, title_size, &book.title, true, true);
    // The page and the percentage: on a line, the percentage below the
    // page (in lines broken between words where it needs them), as
    // "5/395 · 2 %", as "5/395"; else shortened.
    let pages = if parts.contains(&Part::Pages) {
        let full = where_text(env, book, position);
        let percent = percent_text(env, book.percent());
        let counted = match (position.page, position.pages) {
            (Some(page), Some(pages)) if pages > 0 => Some((page, pages)),
            _ => None,
        };
        let mut lines = alloc::vec![full.clone()];
        if !whole(&full) {
            let line = small + line_gap;
            let more = usize::from(rest.height.saturating_sub(total) / line);
            let page = counted.map(|(page, pages)| {
                env.i18n().format(
                    "widgets.reading.page",
                    &[
                        ("page", Arg::Int(i64::from(page))),
                        ("pages", Arg::Int(i64::from(pages))),
                    ],
                )
            });
            let broken = match (&page, font.as_ref()) {
                (Some(page), Some(font)) if more > 0 && whole(&percent) => {
                    let mut broken = super::wrap_words(font, style.small(), page, rest.width, more);
                    broken.push(percent.clone());
                    let fits = broken.len() <= more + 1 && broken.iter().all(|l| whole(l));
                    fits.then_some(broken)
                }
                _ => None,
            };
            lines = match (broken, counted) {
                (Some(broken), _) => broken,
                (None, Some((page, pages))) => {
                    let shorter = env
                        .i18n()
                        .localize_digits(&format!("{page}/{pages}"))
                        .into_owned();
                    let short = format!("{shorter} · {percent}");
                    if whole(&short) {
                        alloc::vec![short]
                    } else if whole(&shorter) {
                        alloc::vec![shorter]
                    } else {
                        lines
                    }
                }
                (None, None) => lines,
            };
            total += line * (lines.len() as u16 - 1);
        }
        lines
    } else {
        Vec::new()
    };
    let chapter = if parts.contains(&Part::Chapter) {
        wrapped(&mut total, style.small(), &position.chapter, true, false)
    } else {
        Vec::new()
    };
    let titled = parts.contains(&Part::Chapter);
    let chapter_left = if parts.contains(&Part::ChapterLeft) {
        // The long way on a line or in a few, else a short way that fits.
        let last = position.chapter_left == Some(0);
        let long = if last {
            Some(env.i18n().tr("widgets.reading.chapter-last").to_string())
        } else {
            chapter_left_text(env, position, timed, titled, style.small(), rest.width)
        };
        let long = long.unwrap_or_default();
        if whole(&long) {
            alloc::vec![long]
        } else {
            let lines = wrapped(&mut total, style.small(), &long, false, false);
            let short = chapter_left_text(env, position, timed, titled, style.small(), rest.width);
            match short {
                _ if lines.len() > 1 => lines,
                Some(short) if whole(&short) => alloc::vec![short],
                _ => lines,
            }
        }
    } else {
        Vec::new()
    };
    let total = total.min(rest.height);
    Layout {
        parts,
        left,
        title_size,
        title,
        pages,
        chapter,
        chapter_left,
        total,
    }
}

/// What a [`Layout`] depends on besides the book and where it is read:
/// the place, the sizes and font the text is measured by, the language
/// and which parts are shown.
fn layout_key(env: &Env<'_>, rest: Rect, cover: bool) -> String {
    let style = UiStyle::new(env);
    let font = style.font(env).map(|f| f.id().clone());
    let mut key = format!(
        "{}x{} {} {} {} {} {} {} {:?} {}",
        rest.width,
        rest.height,
        style.small().to_bits(),
        style.text().to_bits(),
        style.gap(),
        style.line(),
        text_size(env, WIDGET).to_bits(),
        env.i18n().language(),
        font.as_ref().map(paperpaw_sdk::Id::as_str),
        cover,
    );
    for name in [
        "caption",
        "author",
        "bar",
        "pages",
        "left",
        "chapter",
        "chapter-left",
    ] {
        key.push(if widgets::shows(env, WIDGET, name) {
            '1'
        } else {
            '0'
        });
    }
    key
}

fn draw(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    look: &WidgetLook,
    (book, position): (&Book, &Position),
    (covers, memo): (&Covers, &Memo),
) {
    let style = UiStyle::new(env);
    let gap = style.gap();
    let id = WIDGET;
    let area = look.area;
    let (at, rest) = text_place(env, area, book);
    if let Some(at) = at {
        draw_cover(canvas, env, look, at, book, covers);
    }
    let cover = at.is_some();
    let caption_text = env.i18n().tr("widgets.reading.caption");
    let Layout {
        parts,
        left,
        title_size,
        title,
        pages,
        chapter,
        chapter_left,
        total,
    } = memo.layout(layout_key(env, rest, cover), || {
        layout(env, rest, book, position)
    });
    let line_gap = gap / 4;
    let bar_h = (style.line() * 3).max(4);
    let apart = gap / 2;
    let has = |part: Part| parts.contains(&part);
    let total = total.min(rest.height);
    let mut y = rest.y + (rest.height - total) / 2;
    let align = widgets::align(env, id);
    let mut top = Lines::remembering(&memo.lines);
    if has(Part::Caption) {
        top.add(Role::Caption, style.small(), caption_text, look.faint);
    }
    for line in title {
        top.add(Role::Title, title_size, line, look.ink);
    }
    if has(Part::Author) {
        top.add(Role::Note, style.small(), book.author.as_str(), look.faint);
    }
    let h = top.height(env);
    top.draw(canvas, env, Rect::new(rest.x, y, rest.width, h), align);
    y += h;
    if has(Part::Bar) {
        let bar = Rect::new(rest.x, y + gap / 2, rest.width, bar_h);
        draw_bar(canvas, env, look, bar, book.percent());
        y += bar_h + gap;
    }
    let mut place = Lines::remembering(&memo.lines);
    for line in pages {
        place.add(Role::Main, style.small(), line, look.ink);
    }
    if let (true, Some(left)) = (has(Part::Left), left) {
        place.add(Role::Note, style.small(), left, look.faint);
    }
    let mut chapter_lines = Lines::remembering(&memo.lines);
    for line in chapter {
        chapter_lines.add(Role::Main, style.small(), line, look.ink);
    }
    for line in chapter_left {
        chapter_lines.add(Role::Main, style.small(), line, look.faint);
    }
    y += line_gap;
    for (i, mut lines) in [place, chapter_lines].into_iter().enumerate() {
        if lines.is_empty() {
            continue;
        }
        if i == 1 && (has(Part::Pages) || has(Part::Left)) {
            y += line_gap + apart;
        }
        let h = lines.height(env);
        lines.draw(canvas, env, Rect::new(rest.x, y, rest.width, h), align);
        y += h;
    }
}
