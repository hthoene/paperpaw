//! *Books this year*: the books read to the end this year, towards a yearly
//! goal (off, 6, 12, 24, 36 or 52 books) as a bar, a ring or a dot a book, with
//! how the year goes ("2 ahead of plan") and the book finished last.
//!
//! A book counts in the year it was last read to the end: the statistics keep
//! when that was (`finished` in `books.ini`, from the event `book finished`).
//! Books finished before the statistics kept that, or marked as read in the
//! library, count in the year they were last opened, if the library has them
//! read to the end (asked for the 24 opened last at most).
//!
//! `books.ini` is streamed (one book at a time): only the count, the title of
//! the book finished last (cut to 80 bytes) and the at most 24 books without a
//! date that are to be looked up are held.
use crate::book_list::cut;
use crate::declarations::YEAR_STYLES;
use crate::files::DataFolder;
use crate::native_screens::utc_day;
use crate::native_widget_ring::{arc, disc, point_at};
use crate::native_widgets::{
    caption_room, caption_used, captioned, message, middle, mm, note_lines, number_if_narrow,
    pick_caption, shade, sized, stamp, text_at, text_size,
};
use crate::store::{self, Book};
use crate::texts::date_of;
use alloc::string::String;
use alloc::vec::Vec;
use core::cmp::Reverse;
use paperpaw_sdk::native::Error;
use paperpaw_sdk::native::formatting::Arg;
use paperpaw_sdk::native::library::{BookRef, Library};
use paperpaw_sdk::native::storage::DataStorage;
use paperpaw_sdk::{Point, Rect, StorageError};
use paperpaw_ui_kit::mem::{SERVICE_BUFFER, buffer};
use paperpaw_ui_kit::widget::text::Wrapped;
use paperpaw_ui_kit::widget::{Cx, Look};
use paperpaw_ui_kit::{Paint, geometry, paint, text};

/// Books looked up in the library at most (those opened last), for books
/// without a date of their own.
pub(crate) const LIBRARY_LOOKUPS: usize = 24;
/// Dots drawn at most.
const MOST_DOTS: u32 = 60;

/// What the widget shows.
#[derive(Debug, PartialEq, Eq)]
struct Shown {
    year: i32,
    done: u32,
    /// The title of the book finished last, cut to 80 bytes.
    last: Option<String>,
    /// Today's day of the year, and the year's days.
    ordinal: u32,
    length: u32,
}

/// What a year widget worked out, and what for (the stamp of the day and the
/// statistics' files): the books are read, and the library asked, again only
/// when that changed, never at each draw.
#[derive(Debug, Default)]
pub(crate) struct Cache(core::cell::RefCell<Option<(u64, Shown)>>);

/// Whether `year` has 366 days.
fn leap(year: i32) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

/// The day of the year of `day` (1 on 1 January) and the year's days.
fn day_of_year(day: i32) -> (u32, u32) {
    const BEFORE: [u32; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    let d = date_of(day);
    let extra = u32::from(leap(d.year) && d.month > 2);
    let before = BEFORE
        .get(usize::from(d.month.clamp(1, 12) - 1))
        .copied()
        .unwrap_or(0);
    let ordinal = before + extra + u32::from(d.day);
    (ordinal, if leap(d.year) { 366 } else { 365 })
}

/// How far ahead of the plan (behind, if less than 0) `done` books are on day
/// `ordinal` of a year of `length` days, `goal` books spread evenly over it.
fn plan(done: u32, goal: u32, ordinal: u32, length: u32) -> i32 {
    let expected = u64::from(goal) * u64::from(ordinal) / u64::from(length.max(1));
    let expected = i32::try_from(expected).unwrap_or(i32::MAX);
    i32::try_from(done)
        .unwrap_or(i32::MAX)
        .saturating_sub(expected)
}

/// The year of a Unix moment (UTC until the system zone comes with the
/// snapshot).
fn year_of(unix: i64) -> i32 {
    let unix = unix.clamp(0, i64::from(u32::MAX)) as u32;
    date_of(utc_day(unix)).year
}

/// The book finished last, as far as known.
#[derive(Debug, PartialEq, Eq)]
struct Last {
    id: u16,
    at: i64,
    title: String,
}

/// A book without a finishing date, opened this year: to be looked up.
#[derive(Debug, PartialEq, Eq)]
struct Undated {
    id: u16,
    at: i64,
    path: String,
    title: String,
}

impl Undated {
    /// Newer first, ties by id.
    fn key(&self) -> (Reverse<i64>, u16) {
        (Reverse(self.at), self.id)
    }
}

/// The books finished in a year, counted as `books.ini` streams by.
#[derive(Debug)]
struct Tally {
    year: i32,
    count: u32,
    last: Option<Last>,
    /// The `LIBRARY_LOOKUPS` opened last, of the books without a date.
    undated: Vec<Undated>,
}

impl Tally {
    fn new(year: i32) -> Self {
        Self {
            year,
            count: 0,
            last: None,
            undated: Vec::new(),
        }
    }

    /// Takes in the next book of the file: it counts where it was finished
    /// this year; without a finishing date it is kept for a look in the
    /// library where it was opened this year (and has a path to look it up
    /// by).
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    fn add(&mut self, book: &Book) -> Result<(), Error> {
        if book.finished > 0 {
            if year_of(book.finished) == self.year {
                self.found(book.id, book.finished, &book.title, true)?;
            }
            return Ok(());
        }
        if book.last_open > 0 && !book.path.is_empty() && year_of(book.last_open) == self.year {
            self.consider(book)?;
        }
        Ok(())
    }

    /// A book finished (or read to the end) at `at`: counted, and the last
    /// one if no one is later (`by_id`: of two at the same moment the smaller
    /// id; else the one found first stays).
    fn found(&mut self, id: u16, at: i64, title: &str, by_id: bool) -> Result<(), Error> {
        self.count = self.count.saturating_add(1);
        let better = self
            .last
            .as_ref()
            .is_none_or(|last| at > last.at || (by_id && at == last.at && id < last.id));
        if better {
            let title = cut(title)?;
            self.last = Some(Last { id, at, title });
        }
        Ok(())
    }

    /// Keeps the book for a look in the library if it is among the
    /// `LIBRARY_LOOKUPS` opened last so far.
    fn consider(&mut self, book: &Book) -> Result<(), Error> {
        let key = (Reverse(book.last_open), book.id);
        let slot = if self.undated.len() < LIBRARY_LOOKUPS {
            self.undated.try_reserve(1).map_err(|_| Error::NoMemory)?;
            None
        } else {
            // The worst of those held is the one opened longest ago.
            let worst =
                (0..self.undated.len()).max_by_key(|&at| self.undated.get(at).map(Undated::key));
            match worst {
                Some(at) if self.undated.get(at).is_some_and(|w| key < w.key()) => Some(at),
                _ => return Ok(()),
            }
        };
        let kept = Undated {
            id: book.id,
            at: book.last_open,
            path: text::copy(&book.path)?,
            title: cut(&book.title)?,
        };
        match slot.and_then(|at| self.undated.get_mut(at)) {
            Some(held) => *held = kept,
            None => self.undated.push(kept),
        }
        Ok(())
    }

    /// The books finished this year and the one finished last, once the file
    /// is read: `read_to_end` says for a book's path whether the library has
    /// it read to the end; those it does count in the year they were last
    /// opened (asked of the most recently opened only).
    ///
    /// # Errors
    /// [`Error::NoMemory`], and what `read_to_end` returns.
    fn finish(
        mut self,
        mut read_to_end: impl FnMut(&str) -> Result<bool, Error>,
    ) -> Result<(u32, Option<String>), Error> {
        let mut undated = core::mem::take(&mut self.undated);
        paperpaw_sdk::sort::sort_by_key(&mut undated, Undated::key);
        for book in &undated {
            if read_to_end(&book.path)? {
                self.found(book.id, book.at, &book.title, false)?;
            }
        }
        Ok((self.count, self.last.map(|last| last.title)))
    }
}

/// The library, asked about books (opened once, on the first question). Where
/// it cannot be opened (the host without it, or the feature not granted) no
/// book is read to the end: only the books with a date count.
struct Lookups<'h, 'a> {
    host: &'h paperpaw_sdk::native::Host<'a>,
    open: Option<(Library, Vec<u8>, Vec<u8>)>,
    unavailable: bool,
}

impl<'h, 'a> Lookups<'h, 'a> {
    fn new(host: &'h paperpaw_sdk::native::Host<'a>) -> Self {
        Self {
            host,
            open: None,
            unavailable: false,
        }
    }

    /// Whether the library has the book at `path` read to the end (progress
    /// 100). A book it does not know, or cannot answer for, is not.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    fn read_to_end(&mut self, path: &str) -> Result<bool, Error> {
        if self.open.is_none() && !self.unavailable {
            match Library::open(self.host) {
                Ok(library) => {
                    let (input, out) = (buffer(SERVICE_BUFFER), buffer(SERVICE_BUFFER));
                    match (input, out) {
                        (Ok(input), Ok(out)) => self.open = Some((library, input, out)),
                        (_, _) => {
                            library.release(self.host).ok();
                            return Err(Error::NoMemory);
                        }
                    }
                }
                Err(Error::NoMemory) => return Err(Error::NoMemory),
                Err(_) => self.unavailable = true,
            }
        }
        let Some((library, input, out)) = self.open.as_mut() else {
            return Ok(false);
        };
        let Ok(book) = BookRef::path(path) else {
            return Ok(false);
        };
        match library.book(self.host, book, input, out) {
            Ok(book) => Ok(book.progress() == Some(100)),
            Err(Error::NoMemory) => Err(Error::NoMemory),
            Err(_) => Ok(false),
        }
    }

    /// Lets the service go (within the callback that opened it).
    fn release(self) {
        if let Some((library, _, _)) = self.open {
            library.release(self.host).ok();
        }
    }
}

/// Makes the cache hold the figures for what the widget is drawn for (worked
/// out again only where the stamp changed); whether it holds them (not where
/// the device does not know the time). Nothing is copied: the draw reads them
/// in the cache.
fn refresh(cx: &Cx<'_>, cache: &Cache) -> Result<bool, Error> {
    let key = stamp(cx.host)?;
    if cache
        .0
        .borrow()
        .as_ref()
        .is_some_and(|(kept, _)| *kept == key)
    {
        return Ok(true);
    }
    let Some(made) = work_out(cx)? else {
        return Ok(false);
    };
    *cache.0.borrow_mut() = Some((key, made));
    Ok(true)
}

fn work_out(cx: &Cx<'_>) -> Result<Option<Shown>, Error> {
    let Some(today) = cx
        .snapshot
        .unix_seconds()
        .and_then(|unix| u32::try_from(unix).ok())
        .map(utc_day)
    else {
        return Ok(None);
    };
    let year = date_of(today).year;
    let storage = DataStorage::new(cx.host);
    let mut tally = Tally::new(year);
    let read = store::each_book(&DataFolder(&storage), |book| {
        tally.add(&book).map_err(|_| StorageError::TooLarge)
    });
    match read {
        Ok(()) => {}
        Err(StorageError::TooLarge) => return Err(Error::NoMemory),
        // A file that cannot be read shows as no books (as interface 6 did),
        // not as what was read of it.
        Err(_) => tally = Tally::new(year),
    }
    let mut lookups = Lookups::new(cx.host);
    let found = tally.finish(|path| lookups.read_to_end(path));
    lookups.release();
    let (done, last) = found?;
    let (ordinal, length) = day_of_year(today);
    Ok(Some(Shown {
        year,
        done,
        last,
        ordinal,
        length,
    }))
}

/// How the way to the goal shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Way {
    Bar,
    Ring,
    Dots,
}

fn way(cx: &Cx<'_>) -> Result<Way, Error> {
    cx.settings.with_text("style", |v| {
        if v == YEAR_STYLES[1] {
            Way::Ring
        } else if v == YEAR_STYLES[2] {
            Way::Dots
        } else {
            Way::Bar
        }
    })
}

/// The yearly goal, if one is set.
fn goal(cx: &Cx<'_>) -> Result<Option<u32>, Error> {
    cx.settings.with_text("goal", |v| {
        v.rsplit('.')
            .next()
            .and_then(|n| n.parse::<u32>().ok())
            .filter(|&g| g > 0)
    })
}

/// The line a text of `size` takes.
fn line(cx: &Cx<'_>, size: f32) -> Result<u16, Error> {
    geometry::line_px(&cx.font, size)
}

/// The room of the bar, a thick line.
fn bar_band(cx: &Cx<'_>) -> u16 {
    mm(cx, 30)
}

/// The room the way to the goal takes, if there is a goal.
fn band_height(cx: &Cx<'_>, goal: Option<u32>) -> Result<u16, Error> {
    if goal.is_none() {
        return Ok(0);
    }
    Ok(match way(cx)? {
        Way::Ring => line(cx, cx.style().title)?.saturating_mul(2),
        Way::Dots => line(cx, cx.style().small)?.saturating_mul(2),
        Way::Bar => bar_band(cx),
    })
}

/// The height the widget takes: the frame, the caption, the books, the way to
/// the goal and a line for each note that shows. The note on the last book is
/// counted where its part shows, whether a book was finished or not (the
/// height does not wait for the books: it does not change as one is read).
///
/// # Errors
/// Font and host refusals.
pub fn height(cx: &mut Cx<'_>, _width: u16) -> Result<u16, Error> {
    let goal = goal(cx)?;
    let size = cx.settings.size()?.text(cx.style());
    let notes = u16::from(goal.is_some() && cx.settings.shows("plan")?)
        + u16::from(cx.settings.shows("last")?);
    Ok(cx
        .shape
        .frame_height()
        .saturating_add(caption_room(cx)?)
        .saturating_add(line(cx, size)?)
        .saturating_add(band_height(cx, goal)?)
        .saturating_add(notes.saturating_mul(line(cx, cx.style().small)?)))
}

/// Draws the widget in `area`.
///
/// # Errors
/// Font and host refusals, [`Error::NoMemory`].
pub fn draw(
    cx: &mut Cx<'_>,
    out: &mut Vec<Paint>,
    area: Rect,
    slot: &crate::native_widgets::Slot,
) -> Result<(), Error> {
    let look = cx.shape.paint(out, area, 256)?;
    if !refresh(cx, &slot.year)? {
        return message(cx, out, &look, "stats.no-clock");
    }
    let kept = slot.year.0.borrow();
    let Some((_, shown)) = kept.as_ref() else {
        return Err(Error::State);
    };
    body(cx, out, &look, shown)
}

/// "3 of 12 books", or "3 books read" without a goal.
fn headline(cx: &mut Cx<'_>, shown: &Shown, goal: Option<u32>) -> Result<String, Error> {
    let done = Arg::Int {
        name: "done",
        value: i64::from(shown.done),
    };
    match goal {
        Some(goal) => cx.plural("stats.widget.year.of", i64::from(goal), &[done]),
        None => cx.plural("stats.widget.year.read", i64::from(shown.done), &[]),
    }
}

/// The notes below: how the year goes, the book finished last; each with
/// whether it may be shortened where it is too long (a book's title), else it
/// is left out.
fn notes(cx: &mut Cx<'_>, shown: &Shown, goal: Option<u32>) -> Result<Vec<(String, bool)>, Error> {
    let (plan_on, last_on) = (cx.settings.shows("plan")?, cx.settings.shows("last")?);
    let mut notes = Vec::new();
    notes.try_reserve_exact(2).map_err(|_| Error::NoMemory)?;
    if let Some(goal) = goal.filter(|_| plan_on) {
        let ahead = plan(shown.done, goal, shown.ordinal, shown.length);
        let note = if shown.done >= goal {
            cx.tr("stats.widget.year.reached")?
        } else if ahead > 0 {
            cx.plural("stats.widget.year.ahead", i64::from(ahead), &[])?
        } else if ahead < 0 {
            cx.plural("stats.widget.year.behind", -i64::from(ahead), &[])?
        } else {
            cx.tr("stats.widget.year.on-plan")?
        };
        notes.push((note, false));
    }
    if let Some(title) = shown.last.as_deref().filter(|_| last_on) {
        let note = cx.format(
            "stats.widget.year.last",
            &[Arg::Text {
                name: "title",
                value: title,
            }],
        )?;
        notes.push((note, true));
    }
    Ok(notes)
}

fn body(cx: &mut Cx<'_>, out: &mut Vec<Paint>, look: &Look, shown: &Shown) -> Result<(), Error> {
    let goal = goal(cx)?;
    let way = way(cx)?;
    // "Books in 2026", or where that is too long the year alone.
    let title = cx.format(
        "stats.widget.year.caption",
        &[Arg::Int {
            name: "year",
            value: i64::from(shown.year),
        }],
    )?;
    let year = text::format(format_args!("{}", shown.year))?;
    let caption = pick_caption(cx, look.area, &[&title, &year])?;
    let progress = goal.map(|g| shown.done.min(g) * 1000 / g.max(1));
    if way == Way::Ring && goal.is_some() {
        return ring_view(cx, out, look, (shown, caption), progress);
    }
    bar_view(cx, out, look, (shown, caption), (goal, way, progress))
}

/// The room of `area` below `caption`.
fn below(cx: &Cx<'_>, caption: Option<&str>, area: Rect) -> Result<Rect, Error> {
    let used = caption_used(cx, caption, area)?;
    Ok(Rect::new(
        area.x,
        area.y.saturating_add(used),
        area.width,
        area.height.saturating_sub(used),
    ))
}

/// The notes as lines in `width`: on two lines where they need them (left out
/// where not even two hold them), the last book's title on one.
fn wrapped_notes(
    cx: &mut Cx<'_>,
    notes: Vec<(String, bool)>,
    width: u16,
) -> Result<Vec<Wrapped>, Error> {
    let small_px = cx.style().small;
    let small = line(cx, small_px)?;
    let mut lines = Vec::new();
    lines
        .try_reserve_exact(notes.len())
        .map_err(|_| Error::NoMemory)?;
    for (note, may_cut) in notes {
        if may_cut {
            lines.push(Wrapped::new(
                &cx.font,
                &note,
                small_px,
                (width, small),
                false,
            )?);
        } else if let Some(two) = note_lines(cx, &note, (width, small.saturating_mul(2)), 2)? {
            lines.push(two);
        }
    }
    Ok(lines)
}

/// The height of notes together.
fn notes_height(cx: &Cx<'_>, notes: &[Wrapped]) -> Result<u16, Error> {
    let mut total = 0u16;
    for note in notes {
        total = total.saturating_add(note.height(&cx.font)?);
    }
    Ok(total)
}

/// The books done, the way to the goal (a bar or dots) and the notes below,
/// together in the middle.
#[inline(never)]
fn bar_view(
    cx: &mut Cx<'_>,
    out: &mut Vec<Paint>,
    look: &Look,
    (shown, mut caption): (&Shown, Option<&str>),
    (goal, way, progress): (Option<u32>, Way, Option<u32>),
) -> Result<(), Error> {
    let (small_px, text_px) = (cx.style().small, cx.style().text);
    let mut area = below(cx, caption, look.area)?;
    let small = line(cx, small_px)?;
    let align = cx.settings.align()?;
    let all = notes(cx, shown, goal)?;
    let mut notes = wrapped_notes(cx, all, area.width)?;
    let headline = headline(cx, shown, goal)?;
    // Notes as fit below the rest, the plan first.
    let dots = way == Way::Dots;
    let band = match goal {
        Some(_) if dots => small,
        Some(_) => bar_band(cx),
        None => 0,
    };
    let least = line(cx, text_px)?;
    // As many notes as fit `height` with the rest.
    let kept = |cx: &Cx<'_>, notes: &[Wrapped], height: u16| -> Result<usize, Error> {
        let mut kept = 0;
        let mut used = band.saturating_add(least);
        for note in notes {
            used = used.saturating_add(note.height(&cx.font)?);
            if used > height {
                break;
            }
            kept += 1;
        }
        Ok(kept)
    };
    // The caption goes before a note.
    if caption.is_some() && kept(cx, &notes, look.area.height)? > kept(cx, &notes, area.height)? {
        caption = None;
        area = look.area;
    }
    notes.truncate(kept(cx, &notes, area.height)?);
    let notes_h = notes_height(cx, &notes)?;
    let room = area.height.saturating_sub(band.saturating_add(notes_h));
    let size = text_size(cx, &headline, area.width, room)?;
    let big = line(cx, size)?;
    let band = if dots {
        // The dots take the room left, up to three lines of text.
        area.height
            .saturating_sub(big.saturating_add(notes_h))
            .min(small.saturating_mul(3))
    } else {
        band
    };
    let block = captioned(
        cx,
        out,
        look,
        caption,
        big.saturating_add(band).saturating_add(notes_h),
    )?;
    let row = |y: u16, h: u16| Rect::new(area.x, y, area.width, h);
    // Too wide: "3 / 12", or the number alone.
    let headline = match goal {
        Some(g) if cx.font.measure_text(size, &headline)? > f32::from(area.width) => {
            text::format(format_args!("{} / {g}", shown.done))?
        }
        _ => headline,
    };
    let headline = text::fit(
        &cx.font,
        size,
        number_if_narrow(cx, size, &headline, area.width)?,
        f32::from(area.width),
    )?;
    let x = align.x(area, cx.font.measure_text(size, &headline)?);
    text_at(cx, out, &headline, (size, look.ink), (x, row(block.y, big)))?;
    let mut y = block.y.saturating_add(big);
    if let Some(goal) = goal {
        if dots {
            draw_dots(cx, out, look, row(y, band), shown.done, goal)?;
        } else {
            draw_bar(cx, out, look, row(y, band), progress.unwrap_or(0))?;
        }
        y = y.saturating_add(band);
    }
    for note in &notes {
        let h = note.height(&cx.font)?;
        note.draw(out, &cx.font, row(y, h), align, look.faint)?;
        y = y.saturating_add(h);
    }
    Ok(())
}

/// The bar: a track across `band` and over it the share done (`progress` in
/// thousandths), as round as it is thick.
fn draw_bar(
    cx: &Cx<'_>,
    out: &mut Vec<Paint>,
    look: &Look,
    band: Rect,
    progress: u32,
) -> Result<(), Error> {
    let height = mm(cx, 10).max(4);
    let track = Rect::new(
        band.x,
        band.y
            .saturating_add(band.height.saturating_sub(height) / 2),
        band.width,
        height,
    );
    let radius = u32::from(height / 2);
    paint::push(out, Paint::Focus(track, radius, None, shade(look.paper, 1)))?;
    let filled = geometry::clamp_u16(i64::from(band.width) * i64::from(progress) / 1000);
    if filled > 0 {
        let bar = Rect::new(track.x, track.y, filled.max(height), height);
        paint::push(out, Paint::Focus(bar, radius, None, look.ink))?;
    }
    Ok(())
}

/// A circle of `radius` around `center`: filled, or as a line `stroke` thick.
fn circle(
    out: &mut Vec<Paint>,
    center: Point,
    radius: u32,
    stroke: Option<u32>,
    color: paperpaw_sdk::Color,
) -> Result<(), Error> {
    let at = Rect::new(
        geometry::clamp_u16(i64::from(center.x) - i64::from(radius)),
        geometry::clamp_u16(i64::from(center.y) - i64::from(radius)),
        geometry::clamp_u16(i64::from(radius) * 2),
        geometry::clamp_u16(i64::from(radius) * 2),
    );
    paint::push(out, Paint::Focus(at, radius, stroke, color))
}

/// The rows (up to 6) and the dot size that make the dots of `goal` books the
/// largest in a band `w` wide and `h` high: `(rows, size)`.
fn dot_rows(goal: u32, (w, h): (u32, u32)) -> (u32, u32) {
    let n = goal.clamp(1, MOST_DOTS);
    let mut best = (1, 0);
    for rows in 1..=n.min(6) {
        let cols = n.div_ceil(rows);
        let size = (w / cols).min(h / rows);
        if size > best.1 {
            best = (rows, size);
        }
    }
    best
}

/// A dot for each book of the goal in `band`, filled for those read, in as few
/// rows as let them be large.
fn draw_dots(
    cx: &Cx<'_>,
    out: &mut Vec<Paint>,
    look: &Look,
    band: Rect,
    done: u32,
    goal: u32,
) -> Result<(), Error> {
    let n = goal.clamp(1, MOST_DOTS);
    let (w, h) = (u32::from(band.width), u32::from(band.height));
    let (rows, size) = dot_rows(goal, (w, h));
    if size < 4 {
        return Ok(());
    }
    let cols = n.div_ceil(rows);
    let radius = (size * 2 / 5).max(2);
    let thin = u32::from(cx.style().line).max(1);
    let align = cx.settings.align()?;
    let left = align.x(band, (cols * size) as f32) as i32;
    let top = i32::from(band.y) + (h.saturating_sub(rows * size) / 2) as i32;
    for i in 0..n {
        let (row, col) = (i / cols, i % cols);
        let center = Point::new(
            left + (col * size + size / 2) as i32,
            top + (row * size + size / 2) as i32,
        );
        if i < done {
            circle(out, center, radius, None, look.ink)?;
        } else {
            circle(out, center, radius, Some(thin), look.faint)?;
        }
    }
    Ok(())
}

/// A ring into the square `at`, `thick` pixels wide: a thin track all round,
/// over it `progress` (thousandths) with round ends.
fn draw_gauge(
    cx: &Cx<'_>,
    out: &mut Vec<Paint>,
    look: &Look,
    at: Rect,
    progress: Option<u32>,
    thick: u32,
) -> Result<(), Error> {
    let d = u32::from(at.width.min(at.height));
    let radius = d / 2;
    if radius < 4 {
        return Ok(());
    }
    let thick = thick.clamp(1, radius);
    let center = Point::new(
        i32::from(at.x) + radius as i32,
        i32::from(at.y) + radius as i32,
    );
    // The track, thin, in the middle of where the ring is drawn.
    let track = u32::from(cx.style().line).max(1).min(thick);
    let track_radius = radius - (thick - track) / 2;
    arc(
        out,
        (center, track_radius, track),
        (0, 360),
        shade(look.paper, 2),
    )?;
    if let Some(progress) = progress.filter(|p| *p > 0) {
        let done = (360 * progress.min(1000) / 1000).max(1);
        arc(out, (center, radius, thick), (0, done), look.ink)?;
        // Round ends, but for a whole ring.
        if done < 360 {
            let mid = (radius - thick / 2) as i32;
            for degrees in [0, done as i32] {
                disc(out, point_at(center, mid, degrees), thick, look.ink)?;
            }
        }
    }
    Ok(())
}

/// The goal as a ring with the books read inside, the notes beside it (or
/// below, where it is not wide).
#[inline(never)]
fn ring_view(
    cx: &mut Cx<'_>,
    out: &mut Vec<Paint>,
    look: &Look,
    (shown, caption): (&Shown, Option<&str>),
    progress: Option<u32>,
) -> Result<(), Error> {
    let (small_px, gap) = (cx.style().small, cx.style().gap);
    let area = below(cx, caption, look.area)?;
    let small = line(cx, small_px)?;
    let align = cx.settings.align()?;
    let goal = goal(cx)?;
    // Beside or below the ring, the notes that fit a line whole (the last
    // book's title may be shortened).
    let mut lines: Vec<String> = Vec::new();
    for (note, may_cut) in notes(cx, shown, goal)? {
        if may_cut || cx.font.measure_text(small_px, &note)? <= f32::from(area.width) {
            lines.try_reserve(1).map_err(|_| Error::NoMemory)?;
            lines.push(note);
        }
    }
    let count = u16::try_from(lines.len()).unwrap_or(u16::MAX);
    let wide = area.width >= area.height.saturating_mul(2);
    let notes_h = if wide { 0 } else { count.saturating_mul(small) };
    let d = if wide {
        area.height.min(area.width / 3)
    } else {
        area.width.min(area.height.saturating_sub(notes_h))
    };
    let block = captioned(cx, out, look, caption, d.saturating_add(notes_h))?;
    // Wide: the ring and the notes beside it placed together, as the
    // alignment says.
    let beside = if wide {
        let room = f32::from(area.width.saturating_sub(d.saturating_add(gap)));
        let mut widest = 0.0_f32;
        for note in &lines {
            widest = widest.max(cx.font.measure_text(small_px, note)?.min(room));
        }
        widest as u16
    } else {
        0
    };
    let x = if wide && beside > 0 {
        align.x(
            area,
            f32::from(d.saturating_add(gap).saturating_add(beside)),
        ) as u16
    } else {
        align.x(area, f32::from(d)) as u16
    };
    let at = Rect::new(x, block.y, d, d);
    let thick = (u32::from(d) / 12).max(3);
    draw_gauge(cx, out, look, at, progress, thick)?;
    ring_inside(cx, out, look, at, shown.done, thick)?;
    // The notes.
    let (text_area, top) = if wide {
        let left = x.saturating_add(d).saturating_add(gap);
        let text_area = Rect::new(
            left,
            area.y,
            area.x.saturating_add(area.width).saturating_sub(left),
            area.height,
        );
        let top = middle(text_area, count.saturating_mul(small)).y;
        (text_area, top)
    } else {
        (area, block.y.saturating_add(d))
    };
    for (n, note) in lines.iter().enumerate() {
        let shown = text::fit(&cx.font, small_px, note, f32::from(text_area.width))?;
        let nx = if wide {
            f32::from(text_area.x)
        } else {
            align.x(text_area, cx.font.measure_text(small_px, &shown)?)
        };
        let step = u16::try_from(n).unwrap_or(u16::MAX).saturating_mul(small);
        let row = Rect::new(
            text_area.x,
            top.saturating_add(step),
            text_area.width,
            small,
        );
        text_at(cx, out, &shown, (small_px, look.faint), (nx, row))?;
    }
    Ok(())
}

/// The count of books inside the ring at `at`, "of 12" below it where there
/// is room.
fn ring_inside(
    cx: &mut Cx<'_>,
    out: &mut Vec<Paint>,
    look: &Look,
    at: Rect,
    done: u32,
    thick: u32,
) -> Result<(), Error> {
    let (title_px, small_px, text_px) = (cx.style().title, cx.style().small, cx.style().text);
    let small = line(cx, small_px)?;
    let d = at.width.min(at.height);
    let inner = d.saturating_sub(geometry::clamp_u16(i64::from(thick) * 2 + 4));
    let count = text::format(format_args!("{done}"))?;
    let size = sized(
        cx,
        "00",
        (inner / 2, inner.saturating_mul(2) / 5),
        (title_px * 3.0, small_px),
    )?
    .max(text_px);
    let big = line(cx, size)?;
    let of = match goal(cx)? {
        Some(g) => Some(cx.format(
            "stats.widget.year.of-goal",
            &[Arg::Int {
                name: "goal",
                value: i64::from(g),
            }],
        )?),
        None => None,
    };
    let of_h = if of.is_some() && big.saturating_add(small) <= inner {
        small
    } else {
        0
    };
    let inside = middle(at, big.saturating_add(of_h));
    let center = f32::from(at.x) + f32::from(d) / 2.0;
    let w = cx.font.measure_text(size, &count)?;
    text_at(
        cx,
        out,
        &count,
        (size, look.ink),
        (center - w / 2.0, Rect::new(at.x, inside.y, d, big)),
    )?;
    if let Some(of) = of.filter(|_| of_h > 0) {
        let w = cx.font.measure_text(small_px, &of)?;
        let row = Rect::new(at.x, inside.y.saturating_add(big), d, small);
        text_at(
            cx,
            out,
            &of,
            (small_px, look.faint),
            (center - w / 2.0, row),
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
