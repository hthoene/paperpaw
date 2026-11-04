//! "Next up": a book to read next, as the reader chooses: the next one in
//! the series of the book being read, the newest unread one, or an unread
//! one by chance (another each day); with its cover and title.
//!
//! Which books are new the library does not say: this widget keeps the
//! books it saw (`seen.bits`, see [`Seen`]) and the ones that came since
//! (`newest.txt`, a few) in its folder, from the catalog when it changed.

use alloc::boxed::Box;
use alloc::format;
use alloc::rc::Rc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cell::RefCell;

use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::library;
use paperpaw_sdk::ui::UiStyle;
use paperpaw_sdk::widgets::{
    self, Align, Frame, Padding, TextSize, Widget, WidgetLook, WidgetSpec, WidgetTarget,
};
use paperpaw_sdk::{Arg, Canvas, Command, Env, Id, Rect, RegisterError, Registrar, Storage};

use super::series::{Member, members};
use super::{
    Book, Covers, FitMemo, Generation, Lines, Role, add_settings, catalog_stamp, chosen,
    cover_size, current, draw_cover, draw_message, each_book, frame_height, least_cover_width, mix,
    path_hash, text_size, today,
};
use crate::PLUGIN;
use paperpaw_sdk::SettingSpec;

pub(crate) const ID: Id = Id::new_static(WIDGET);
/// Its id, as text.
const WIDGET: &str = "widgets.next";
/// How the book is chosen.
const PICKS: [&str; 3] = [
    "widgets.next.series",
    "widgets.next.newest",
    "widgets.next.random",
];
/// Unread books chosen by chance looked at, at most, to find one unread.
const CANDIDATES: usize = 6;
/// The bytes of the books seen ([`Seen`]).
const SEEN_BYTES: usize = 8 * 1024;
/// The places in [`Seen`] a book sets.
const SEEN_PLACES: u64 = 5;
/// New books remembered, at most.
const MAX_NEWEST: usize = 16;

/// Its settings, kept in the program.
static SETTINGS: [SettingSpec; 11] = [
    widgets::align_spec("widgets.next.align", ID, Align::Left),
    widgets::frame_spec("widgets.next.frame", ID, Frame::None),
    widgets::padding_spec("widgets.next.padding", ID, Padding::Auto),
    widgets::paper_spec("widgets.next.paper", ID),
    widgets::ink_spec("widgets.next.ink", ID),
    widgets::size_spec("widgets.next.size", ID, TextSize::Medium, false),
    SettingSpec::choice(
        "widgets.next.pick",
        "widgets.books.setting.pick",
        &PICKS,
        PICKS[0],
        ID,
    )
    .description("widgets.books.setting.pick.help"),
    widgets::part_spec(
        "widgets.next.caption",
        ID,
        "widgets.books.part.caption",
        true,
    ),
    widgets::part_spec("widgets.next.cover", ID, "widgets.books.part.cover", true),
    widgets::part_spec("widgets.next.author", ID, "widgets.books.part.author", true),
    widgets::part_spec("widgets.next.series", ID, "widgets.books.part.series", true),
];

pub(crate) fn register(r: &mut Registrar, generation: &Generation) -> Result<(), RegisterError> {
    add_settings(
        r,
        {
            static SPEC: paperpaw_sdk::SettingGroupSpec =
                paperpaw_sdk::SettingGroupSpec::new(ID, "widgets.next");
            &SPEC
        },
        &SETTINGS,
    )?;
    let kept: Kept = Rc::default();
    let generation = generation.clone();
    r.add_widget(
        WidgetSpec::new(ID, "widgets.next", move |_| {
            Box::new(Next {
                kept: kept.clone(),
                generation: generation.clone(),
                book: None,
                area: None,
                covers: Covers::default(),
                memo: FitMemo::default(),
            })
        })
        .category(widgets::categories::BOOKS),
    )
}

/// The book chosen, and what it was chosen for (the way, the day, the
/// library's generation, the catalog, the book being read).
type Kept = Rc<RefCell<Option<(Key, Option<String>)>>>;
type Key = (String, i64, u32, u64, String);

/// Whether the book at `path` was not begun.
fn unread(storage: &dyn Storage, path: &str) -> bool {
    match library::book(storage, path) {
        Some(book) => book.progress.unwrap_or(0) == 0,
        None => storage.exists(path),
    }
}

/// The book after `current` in its series (in order) not read yet.
pub(crate) fn next_in_series<'m>(members: &'m [Member], current: &str) -> Option<&'m Member> {
    let at = members.iter().position(|m| m.path == current)?;
    members[at + 1..].iter().find(|m| !m.read)
}

/// The order books are tried in by chance on `day`: by a number that
/// looks random, another each day.
pub(crate) fn chance(path: &str, day: i64) -> u64 {
    mix(path_hash(path) ^ mix(day as u64))
}

/// Keeps the `n` lowest of `(score, path)` in `kept`, lowest first.
pub(crate) fn keep_lowest(kept: &mut Vec<(u64, String)>, n: usize, score: u64, path: &str) {
    if kept.len() == n && kept.last().is_some_and(|(s, _)| *s <= score) {
        return;
    }
    let at = kept.partition_point(|(s, _)| *s <= score);
    kept.insert(at, (score, path.to_string()));
    kept.truncate(n);
}

/// An unread book by chance, another each day; not one opened lately.
fn by_chance(env: &Env<'_>, day: i64) -> Option<String> {
    let opened: Vec<u64> = {
        let mut hashes: Vec<u64> = library::opened_books(env.storage())
            .iter()
            .map(|p| path_hash(p))
            .collect();
        hashes.sort_unstable();
        hashes
    };
    let mut kept = Vec::new();
    each_book(env, &mut |record| {
        if opened.binary_search(&path_hash(&record.path)).is_err() {
            keep_lowest(
                &mut kept,
                CANDIDATES,
                chance(&record.path, day),
                &record.path,
            );
        }
    });
    kept.into_iter()
        .map(|(_, path)| path)
        .find(|path| unread(env.storage(), path))
}

/// The next book in the series of `book`, if there is one not read.
fn in_series(env: &Env<'_>, book: &Book) -> Option<String> {
    if book.series.is_empty() {
        return None;
    }
    let begun = |path: &str| !unread(env.storage(), path);
    let members = members(env, &book.series, Some(&book.number), &begun);
    next_in_series(&members.books, &book.path).map(|m| m.path.clone())
}

fn seen_file() -> String {
    format!(
        "{}/seen.bits",
        paperpaw_sdk::Env::plugin_dir(PLUGIN.as_str())
    )
}

/// Where the books seen were kept before, as a list of their hashes.
fn old_seen_file() -> String {
    format!(
        "{}/seen.bin",
        paperpaw_sdk::Env::plugin_dir(PLUGIN.as_str())
    )
}

/// The books seen, by the hashes of their paths, in a set of
/// [`SEEN_BYTES`] whatever the size of the library (a Bloom filter): a
/// book not seen counts as seen now and then (about one in a thousand
/// with 4,000 books, one in twenty with 10,000), a book seen never counts
/// as new.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Seen {
    bits: Vec<u8>,
}

impl Seen {
    /// None seen.
    pub(crate) fn new() -> Self {
        Self {
            bits: alloc::vec![0; SEEN_BYTES],
        }
    }

    /// As kept; `None` where none are kept (the first time).
    fn read(storage: &dyn Storage) -> Option<Self> {
        let bits = paperpaw_sdk::read_file(storage, &seen_file(), SEEN_BYTES).ok()?;
        (bits.len() == SEEN_BYTES).then_some(Self { bits })
    }

    /// The places the book `hash` sets.
    fn places(hash: u64) -> impl Iterator<Item = usize> {
        let bits = (SEEN_BYTES * 8) as u64;
        let step = mix(hash) | 1;
        (0..SEEN_PLACES).map(move |i| (hash.wrapping_add(i.wrapping_mul(step)) % bits) as usize)
    }

    pub(crate) fn add(&mut self, hash: u64) {
        for place in Self::places(hash) {
            self.bits[place / 8] |= 1 << (place % 8);
        }
    }

    pub(crate) fn has(&self, hash: u64) -> bool {
        Self::places(hash).all(|place| self.bits[place / 8] & (1 << (place % 8)) != 0)
    }
}

fn newest_file() -> String {
    format!(
        "{}/newest.txt",
        paperpaw_sdk::Env::plugin_dir(PLUGIN.as_str())
    )
}

/// The books new since they were last looked at, `unseen`, before those
/// kept, `newest`: as many as are remembered.
pub(crate) fn new_books(unseen: Vec<String>, newest: Vec<String>) -> Vec<String> {
    let mut list = unseen;
    for path in newest {
        if !list.contains(&path) {
            list.push(path);
        }
    }
    list.truncate(MAX_NEWEST);
    list
}

/// Brings the books seen and the newest up to date with the library;
/// returns the newest, newest first.
fn newest(env: &Env<'_>) -> Vec<String> {
    let storage = env.storage();
    let seen = Seen::read(storage);
    let kept: Vec<String> = paperpaw_sdk::read_file(storage, &newest_file(), MAX_NEWEST * 512)
        .map(|data| {
            String::from_utf8_lossy(&data)
                .lines()
                .filter(|l| !l.is_empty())
                .map(String::from)
                .collect()
        })
        .unwrap_or_default();
    // The books now, and those not seen (the first time, none is new).
    let mut now = Seen::new();
    let mut unseen = Vec::new();
    each_book(env, &mut |record| {
        let hash = path_hash(&record.path);
        now.add(hash);
        if seen.as_ref().is_some_and(|seen| !seen.has(hash)) && unseen.len() < MAX_NEWEST {
            unseen.push(record.path);
        }
    });
    let list = new_books(unseen, kept.clone());
    // Written only when something changed (a cache: not written, the
    // same books are found new again).
    if seen.as_ref() != Some(&now) {
        let dir = paperpaw_sdk::Env::plugin_dir(PLUGIN.as_str());
        let _ = storage.create_dir(&dir);
        let _ = storage.write(&seen_file(), &now.bits);
        let _ = storage.remove(&old_seen_file());
        if list != kept {
            let _ = storage.write(&newest_file(), list.join("\n").as_bytes());
        }
    }
    list
}

/// The newest book not begun.
fn newest_unread(env: &Env<'_>) -> Option<String> {
    newest(env)
        .into_iter()
        .find(|path| unread(env.storage(), path))
}

struct Next {
    kept: Kept,
    generation: Generation,
    book: Option<Book>,
    /// Where it is drawn, once placed.
    area: Option<Rect>,
    covers: Covers,
    /// Its lines made to fit, kept for the frames that follow.
    memo: FitMemo,
}

impl Next {
    /// The cover shown, made small for where it goes.
    fn make_covers(&mut self, env: &Env<'_>) {
        self.covers = match (self.area, &self.book) {
            (Some(area), Some(book)) => {
                let look = widgets::widget_look(env, WIDGET, area);
                let at = layout(env, &look, book, &self.memo).cover;
                Covers::make(env, at.map(|at| (book, at)))
            }
            _ => Covers::default(),
        };
    }

    fn choose(&self, env: &Env<'_>) -> Option<String> {
        let pick = chosen(env, WIDGET, "pick").to_string();
        let current = current(env.storage());
        let day = today(env);
        let key: Key = (
            pick.clone(),
            day,
            self.generation.get(),
            catalog_stamp(env),
            current.as_ref().map(|b| b.path.clone()).unwrap_or_default(),
        );
        if let Some((k, path)) = self.kept.borrow().as_ref()
            && *k == key
        {
            return path.clone();
        }
        let first = match pick.as_str() {
            "widgets.next.newest" => newest_unread(env),
            "widgets.next.random" => None,
            _ => current.as_ref().and_then(|book| in_series(env, book)),
        };
        // Else by chance.
        let path = first.or_else(|| by_chance(env, day));
        *self.kept.borrow_mut() = Some((key, path.clone()));
        path
    }
}

impl Widget for Next {
    fn update(&mut self, env: &Env<'_>) -> bool {
        let book = self.choose(env).and_then(|p| Book::of(env.storage(), &p));
        let changed = book != self.book;
        self.book = book;
        if changed {
            self.make_covers(env);
        }
        changed
    }

    fn place(&mut self, env: &Env<'_>, area: Rect) {
        if self.area != Some(area) {
            self.area = Some(area);
            self.make_covers(env);
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
        match &self.book {
            Some(book) => draw(canvas, env, &look, book, (&self.covers, &self.memo)),
            None => draw_message(canvas, env, &look, "widgets.next.none"),
        }
    }

    fn targets(&self, _: &Env<'_>, area: Rect) -> Vec<WidgetTarget> {
        let command = self
            .book
            .as_ref()
            .map_or(Command::run(actions::LIBRARY), Book::open);
        alloc::vec![WidgetTarget::new(area, command)]
    }
}

/// "Discworld · 3".
fn series_text(env: &Env<'_>, book: &Book) -> String {
    match (book.series.is_empty(), book.number.is_empty()) {
        (true, _) => String::new(),
        (false, true) => book.series.clone(),
        (false, false) => env.i18n().format(
            "widgets.next.in-series",
            &[
                ("series", Arg::Str(&book.series)),
                ("number", Arg::Str(&book.number)),
            ],
        ),
    }
}

/// Where the cover and the text go.
struct Layout<'b> {
    /// The cover, if shown.
    cover: Option<Rect>,
    lines: Lines<'b>,
    /// Where the text goes, if anywhere.
    text: Option<Rect>,
}

/// The cover and the text of `book` in `look`'s area: without the cover,
/// the text alone; where wide enough, the cover on the left and the text
/// beside it; else the cover above and the text below (what fits in
/// half), or the cover alone.
fn layout<'b>(env: &Env<'b>, look: &WidgetLook, book: &'b Book, memo: &'b FitMemo) -> Layout<'b> {
    let style = UiStyle::new(env);
    let id = WIDGET;
    let gap = style.gap();
    let area = look.area;
    let align = widgets::align(env, id);
    let mut lines = Lines::remembering(memo);
    let caption = widgets::shows(env, id, "caption");
    if caption {
        lines.add(
            Role::Caption,
            style.small(),
            env.i18n().tr("widgets.next.caption"),
            look.faint,
        );
    }
    lines.add(
        Role::Title,
        text_size(env, id),
        book.title.as_str(),
        look.ink,
    );
    if widgets::shows(env, id, "author") {
        lines.add(Role::Note, style.small(), book.author.as_str(), look.faint);
    }
    if widgets::shows(env, id, "series") {
        lines.add(
            Role::Note,
            style.small(),
            series_text(env, book),
            look.faint,
        );
    }
    if !widgets::shows(env, id, "cover") {
        return Layout {
            cover: None,
            lines,
            text: Some(area),
        };
    }
    let (w, h) = cover_size(book, area.width * 2 / 5, area.height);
    let text_room = f32::from(area.width.saturating_sub(w + gap));
    if area.width >= area.height && text_room >= style.small() * 7.0 {
        // Wide: the cover on the left, the text beside it.
        let shift = (w + gap).min(area.width);
        return Layout {
            cover: Some(Rect::new(area.x, area.y + (area.height - h) / 2, w, h)),
            lines,
            text: Some(Rect::new(
                area.x + shift,
                area.y,
                area.width - shift,
                area.height,
            )),
        };
    }
    // High: the cover above, the text below it (what fits in half).
    let half = (area.height / 2).saturating_sub(gap / 2);
    lines.fit(env, area.width, half);
    let text_h = lines.height(env);
    let room = area.height.saturating_sub(text_h + gap / 2);
    // A cover too small to tell is left out rather than the text.
    if room < area.height / 3 || room < least_cover_width(env) * 3 / 2 {
        return Layout {
            cover: None,
            lines,
            text: Some(area),
        };
    }
    let (w, h) = cover_size(book, area.width, room);
    let total = h + gap / 2 + text_h;
    let top = area.y + (area.height - total.min(area.height)) / 2;
    Layout {
        cover: Some(Rect::new(super::place_x(area, w, align), top, w, h)),
        lines,
        text: Some(Rect::new(area.x, top + h + gap / 2, area.width, text_h)),
    }
}

fn draw<'b>(
    canvas: &mut Canvas<'_>,
    env: &Env<'b>,
    look: &WidgetLook,
    book: &'b Book,
    (covers, memo): (&Covers, &'b FitMemo),
) {
    let Layout {
        cover,
        mut lines,
        text,
    } = layout(env, look, book, memo);
    if let Some(at) = cover {
        draw_cover(canvas, env, look, at, book, covers);
    }
    if let Some(text) = text {
        lines.draw(canvas, env, text, widgets::align(env, WIDGET));
    }
}
