//! "Library in numbers": the books, those read, being read and not read
//! yet, as large numbers in a row with small names below, and a bar of
//! the share read.
//!
//! The numbers are the library's: the books of the catalog, and the totals
//! of its finished and reading queries (no book is looked at). They change
//! with the catalog's generation, which moves when books come or go and when
//! one is first opened or finished.
use crate::native_books::{buffers, caption, with_library};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Color, Rect,
    native::{
        Error, Host,
        fonts::{Font, TextOrigin},
        library::{Library, Query, Show},
    },
};
use paperpaw_ui_kit::{
    Paint, geometry, mem, paint, text,
    widget::{
        Align, Cx, Look, TextSize, shape,
        text::{ink_baseline, largest_size, line as fit_row},
    },
};

/// The numbers that can show, in order: each a part and the key of its name.
const PARTS: [(&str, &str); 4] = [
    ("books", "widgets.library.books"),
    ("finished", "widgets.library.finished"),
    ("reading", "widgets.library.reading"),
    ("unread", "widgets.library.unread"),
];
/// The caption.
const CAPTION: &str = "widgets.library.caption";

/// The library counted.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Numbers {
    pub books: u32,
    pub read: u32,
    pub reading: u32,
    pub unread: u32,
}

impl Numbers {
    /// `books` books, of which `read` are finished and `reading` begun.
    #[must_use]
    pub const fn count(books: u32, read: u32, reading: u32) -> Numbers {
        // Books the catalog does not list still count as books.
        let known = read.saturating_add(reading);
        let books = if books > known { books } else { known };
        Numbers {
            books,
            read,
            reading,
            unread: books - known,
        }
    }

    /// The number of the part `name` (`books`, `finished`, `reading`, else
    /// `unread`).
    #[must_use]
    pub fn of(&self, name: &str) -> u32 {
        match name {
            "books" => self.books,
            "finished" => self.read,
            "reading" => self.reading,
            _ => self.unread,
        }
    }
}

/// The books the library has found: those it lists, those finished and
/// those being read, each by one query of which only the total is used.
///
/// # Errors
/// The host's refusals and [`Error::NoMemory`].
pub fn read(host: &Host<'_>) -> Result<Numbers, Error> {
    with_library(host, |library| {
        let (mut input, mut out) = buffers()?;
        let state = library.state(host, &mut input, &mut out)?;
        let read = total(host, library, Show::Finished, (&mut input, &mut out))?;
        let reading = total(host, library, Show::Reading, (&mut input, &mut out))?;
        Ok(Numbers::count(state.books(), read, reading))
    })
}

/// How many books `show` finds.
fn total(
    host: &Host<'_>,
    library: &Library,
    show: Show,
    (input, out): (&mut [u8], &mut [u8]),
) -> Result<u32, Error> {
    let page = library.query(host, &Query::new().show(show), input, out)?;
    Ok(page.end().total())
}

/// What the widget shows, to tell when it changed: the catalog's generation
/// and its books, never 0.
///
/// # Errors
/// The host's refusals.
pub fn shows(host: &Host<'_>, _widget: &str) -> Result<u64, Error> {
    with_library(host, |library| {
        let (mut input, mut out) = buffers()?;
        let state = library.state(host, &mut input, &mut out)?;
        let mut bytes = [0; 8];
        bytes[..4].copy_from_slice(&state.generation().to_le_bytes());
        bytes[4..].copy_from_slice(&state.books().to_le_bytes());
        Ok(crate::native_draw::fnv(1, &bytes))
    })
}

/// The height: the numbers two titles large, two small lines, a gap and the
/// frame.
///
/// # Errors
/// Font refusals.
pub fn height(cx: &mut Cx<'_>, _width: u16) -> Result<u16, Error> {
    let style = cx.style();
    let (title, small, gap) = (style.title, style.small, style.gap);
    let big = geometry::line_px(&cx.font, title * 2.0)?;
    let note = geometry::line_px(&cx.font, small)?;
    Ok(big
        .saturating_add(note.saturating_mul(2))
        .saturating_add(gap)
        .saturating_add(cx.shape.frame_height()))
}

/// The parts that show and their names.
fn parts(cx: &mut Cx<'_>) -> Result<Vec<(&'static str, String)>, Error> {
    let mut shown = Vec::new();
    for (part, key) in PARTS {
        if cx.settings.shows(part)? {
            let name = cx.tr(key)?;
            mem::push(&mut shown, (part, name))?;
        }
    }
    Ok(shown)
}

/// A number as decimal digits.
fn digits(value: u32) -> Result<String, Error> {
    let mut scratch = [0; 20];
    text::copy(text::decimal(value as usize, &mut scratch))
}

/// How the numbers are laid out in a place: the first `shown` of them, in
/// `columns`, the numbers `size` large, with the bar or not; or, where no
/// number fits above its name, each beside its name on a line.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Laid {
    shown: usize,
    columns: u16,
    size: f32,
    bar: bool,
    /// The numbers beside their names, small, on one line.
    beside: bool,
}

/// What the font makes of the digits and of the names.
#[derive(Debug, Clone, Copy)]
struct Inks {
    /// The digits' ink height for the ascent.
    digits: f32,
    /// The names' ink: its top above the baseline and its height.
    names: (i32, u16),
}

/// The ink of `0123456789` for the font's ascent, and of the `labels`.
fn inks(font: &Font<'_>, (text_size, small): (f32, f32), labels: &str) -> Result<Inks, Error> {
    let ascent = geometry::line_metrics(font, text_size)?.0.max(1.0);
    let digits = match font.ink(text_size, "0123456789")? {
        Some(ink) => {
            let (top, bottom) = geometry::check_ink(ink.top, ink.bottom)?;
            (bottom - top) as f32 / ascent
        }
        None => 0.72,
    };
    let names = match font.ink(small, labels)? {
        Some(ink) => {
            let (top, bottom) = geometry::check_ink(ink.top, ink.bottom)?;
            (top, (bottom - top) as u16)
        }
        None => (-(small as i32), small as u16),
    };
    Ok(Inks { digits, names })
}

/// The height of the digits' ink at `size`, for a font whose digits are
/// `digits` of its ascent high.
fn digits_height(font: &Font<'_>, size: f32, digits: f32) -> Result<u16, Error> {
    Ok((geometry::line_metrics(font, size)?.0 * digits + 0.99) as u16)
}

/// What a layout is made of: the place and the sizes of the interface.
struct Place<'a, 'f> {
    font: &'a Font<'f>,
    area: Rect,
    text: f32,
    small: f32,
    gap: u16,
    line: u16,
    /// The size the setting asks for, else the most the place holds.
    max: f32,
    bar: bool,
}

/// The numbers laid out in the place: all of them where they fit with their
/// names whole, else fewer (the last left out first); as many in a row as
/// makes them largest, the bar kept where it costs little.
fn lay_out(
    place: &Place<'_, '_>,
    parts: &[(&'static str, String)],
    numbers: &Numbers,
) -> Result<Option<Laid>, Error> {
    let font = place.font;
    let (small, gap, area) = (place.small, place.gap, place.area);
    let shown = parts.len();
    if shown == 0 {
        return Ok(None);
    }
    let labels = {
        let mut all = String::new();
        for (_, name) in parts {
            all.try_reserve(name.len()).map_err(|_| Error::NoMemory)?;
            all.push_str(name);
        }
        all
    };
    let inks = inks(font, (place.text, small), &labels)?;
    let names_h = inks.names.1;
    let bar_room = (place.line * 3).max(4) + gap;
    let wanted_bar = place.bar && numbers.books > 0;
    let mut widest_label = 0.0_f32;
    let mut widest = String::new();
    for (part, name) in parts {
        widest_label = widest_label.max(font.measure_text(small, name)?);
        let number = digits(numbers.of(part))?;
        if number.len() > widest.len() {
            widest = number;
        }
    }
    let mut best: Option<(f32, Laid)> = None;
    let mut tried = [0_u16; 3];
    for (n, columns) in [shown, shown.div_ceil(2), 1].into_iter().enumerate() {
        let columns = u16::try_from(columns).unwrap_or(u16::MAX);
        if tried[..n].contains(&columns) {
            continue;
        }
        tried[n] = columns;
        let column = area.width / columns;
        // The names whole, a little room between neighbours, else
        // shortened: that counts against it, the more the shorter.
        let between = if columns > 1 { gap / 2 } else { 0 };
        let room = f32::from(column.saturating_sub(between));
        let names = if widest_label <= room {
            1.0
        } else {
            0.5 * room / widest_label.max(1.0)
        };
        let rows = u16::try_from(shown).unwrap_or(u16::MAX).div_ceil(columns);
        for bar in [wanted_bar, false] {
            let fixed = if bar { bar_room } else { 0 } + gap * (rows - 1);
            let row = area.height.saturating_sub(fixed) / rows;
            let size = largest_size(place.max, small, |s| {
                // A pixel or two to spare: the ink is measured from the
                // ascent here, as drawn there.
                let fits = |height: u16| {
                    height
                        .saturating_add(gap / 2)
                        .saturating_add(names_h)
                        .saturating_add(2)
                        <= row
                };
                if !fits(digits_height(font, s, inks.digits)?)
                    || font.measure_text(s, &widest)? > f32::from(column) * 0.8
                {
                    return Ok(false);
                }
                Ok(match font.ink(s, "0123456789")? {
                    Some(ink) => {
                        let (top, bottom) = geometry::check_ink(ink.top, ink.bottom)?;
                        fits((bottom - top) as u16)
                    }
                    None => true,
                })
            })?;
            let Some(size) = size else {
                continue;
            };
            // Names shortened cost much, the bar a little.
            let mut score = size * names;
            if bar != wanted_bar {
                score *= 0.9;
            }
            if best.is_none_or(|(b, _)| score > b) {
                let laid = Laid {
                    shown,
                    columns,
                    size,
                    bar,
                    beside: false,
                };
                best = Some((score, laid));
            }
        }
    }
    if let Some((_, laid)) = best {
        return Ok(Some(laid));
    }
    // No number fits above its name: each beside it on a line (or the
    // number alone where the name does not fit).
    Ok(Some(Laid {
        shown,
        columns: u16::try_from(shown).unwrap_or(u16::MAX),
        size: small,
        bar: false,
        beside: true,
    }))
}

/// Draws the widget in `area`.
///
/// # Errors
/// Font and host refusals, [`Error::NoMemory`].
pub fn draw(cx: &mut Cx<'_>, out: &mut Vec<Paint>, area: Rect) -> Result<(), Error> {
    let look = cx.shape.paint(out, area, 256)?;
    let numbers = read(cx.host)?;
    let rest = caption(cx, out, &look, CAPTION)?;
    let parts = parts(cx)?;
    let style = cx.style();
    let max = match cx.settings.size()? {
        // As large as the place holds, its height checked below.
        TextSize::Fit => f32::from(rest.height).max(style.small),
        other => other.text(style) * 1.4,
    };
    let place = Place {
        font: &cx.font,
        area: rest,
        text: style.text,
        small: style.small,
        gap: style.gap,
        line: style.line,
        max,
        bar: cx.settings.shows("bar")?,
    };
    let Some(laid) = lay_out(&place, &parts, &numbers)? else {
        return Ok(());
    };
    let align = cx.settings.align()?;
    draw_laid(out, &place, (&look, align), (&parts, &numbers, &laid))
}

/// What a drawing needs of the colours and the parts.
type Content<'a> = (&'a [(&'static str, String)], &'a Numbers, &'a Laid);

/// The numbers beside their names, each on its own place of one line.
fn draw_beside(
    out: &mut Vec<Paint>,
    place: &Place<'_, '_>,
    (look, align): (&Look, Align),
    (parts, numbers, laid): Content<'_>,
) -> Result<(), Error> {
    let (font, small, area) = (place.font, place.small, place.area);
    let column = area.width / laid.columns.max(1);
    let left = align.x(area, f32::from(column * laid.columns)) as u16;
    for (i, (part, name)) in parts.iter().take(laid.shown).enumerate() {
        let cell = Rect::new(
            left.saturating_add((i as u16).saturating_mul(column)),
            area.y,
            column,
            area.height,
        );
        let number = digits(numbers.of(part))?;
        let whole = text::concat(&[&number, " ", name])?;
        if font.measure_text(small, &whole)? > f32::from(cell.width) {
            // Not even one with its name: the number alone, smaller where it
            // must be, nothing where even that is too wide.
            let least = small * 0.5;
            if font.measure_text(least, &number)? <= f32::from(cell.width) {
                fit_row(
                    out,
                    font,
                    cell,
                    Align::Center,
                    (small, least),
                    &number,
                    look.ink,
                )?;
            }
            continue;
        }
        let x = Align::Center.x(cell, font.measure_text(small, &whole)?);
        let baseline = ink_baseline(font, small, &whole, cell)?;
        let prefix = text::concat(&[&number, " "])?;
        let after = x + font.measure_text(small, &prefix)?;
        push_text(out, number, (small, x, baseline), look.ink)?;
        push_text(out, text::copy(name)?, (small, after, baseline), look.faint)?;
    }
    Ok(())
}

/// Pushes the text `shown` at `size`, from `x` on `baseline`.
fn push_text(
    out: &mut Vec<Paint>,
    shown: String,
    (size, x, baseline): (f32, f32, i32),
    color: Color,
) -> Result<(), Error> {
    paint::push(
        out,
        Paint::Text {
            text: shown,
            size,
            origin: TextOrigin::new(x, baseline),
            color,
            clip: None,
        },
    )
}

/// The numbers as `laid`, each above its name (left out where the name would
/// say less than three letters), and the bar.
fn draw_laid(
    out: &mut Vec<Paint>,
    place: &Place<'_, '_>,
    (look, align): (&Look, Align),
    content: Content<'_>,
) -> Result<(), Error> {
    let (parts, numbers, laid) = content;
    if laid.beside {
        return draw_beside(out, place, (look, align), content);
    }
    let (font, small, area, gap) = (place.font, place.small, place.area, place.gap);
    let shown = parts.get(..laid.shown.min(parts.len())).unwrap_or(&[]);
    let column = area.width / laid.columns.max(1);
    let used = column * laid.columns;
    let left = align.x(area, f32::from(used)) as u16;
    let labels = {
        let mut all = String::new();
        for (_, name) in shown {
            all.try_reserve(name.len()).map_err(|_| Error::NoMemory)?;
            all.push_str(name);
        }
        all
    };
    let inks = inks(font, (place.text, small), &labels)?;
    let (names_top, names_ink) = inks.names;
    // The names in their columns, a little room from their neighbours; none
    // where one would keep less than three letters (a row of "B…" says
    // nothing), the numbers alone then.
    let between = if laid.columns > 1 { gap / 2 } else { 0 };
    let room = f32::from(column.saturating_sub(between));
    let mut names = Vec::new();
    names.try_reserve(shown.len()).map_err(|_| Error::NoMemory)?;
    let mut named = true;
    for (_, name) in shown {
        let fitted = text::fit(font, small, name, room)?;
        named &= fitted == *name || fitted.chars().count() > 3;
        names.push(fitted);
    }
    let names_h = if named { names_ink } else { 0 };
    let size = laid.size;
    // Each row: the digits' ink, half a gap, the names' ink; a gap between
    // rows and above the bar; all of it in the middle.
    let ink = match font.ink(size, "0123456789")? {
        Some(ink) => geometry::check_ink(ink.top, ink.bottom)?,
        None => (-(size as i32), 0),
    };
    let digits_h = (ink.1 - ink.0) as u16;
    let row_h = digits_h + if named { gap / 2 + names_h } else { 0 };
    let rows = u16::try_from(laid.shown).unwrap_or(u16::MAX).div_ceil(laid.columns.max(1));
    let bar_h = (place.line * 3).max(4);
    let total = row_h * rows + gap * (rows - 1) + if laid.bar { gap + bar_h } else { 0 };
    let top = area.y + area.height.saturating_sub(total) / 2;
    for (i, ((part, _), name)) in shown.iter().zip(&names).enumerate() {
        let (col, row) = (i as u16 % laid.columns, i as u16 / laid.columns);
        let y = top + row * (row_h + gap);
        // A last row with fewer in the middle under the others.
        let in_row = (shown.len() as u16 - row * laid.columns).min(laid.columns);
        let shift = (laid.columns - in_row) * column / 2;
        let cell = Rect::new(left + shift + col * column, y, column, row_h);
        let number = digits(numbers.of(part))?;
        let x = Align::Center.x(cell, font.measure_text(size, &number)?);
        push_text(out, number, (size, x, i32::from(y) - ink.0), look.ink)?;
        let label_w = font.measure_text(small, name)?;
        if named && label_w <= f32::from(column) {
            let x = Align::Center.x(cell, label_w);
            let baseline = i32::from(y + digits_h + gap / 2) - names_top;
            push_text(out, text::copy(name)?, (small, x, baseline), look.faint)?;
        }
    }
    if laid.bar {
        let inset = column / 6;
        let line = Rect::new(
            left + inset,
            top + row_h * rows + gap * (rows - 1) + gap,
            used.saturating_sub(2 * inset),
            bar_h,
        );
        draw_share(out, look, line, numbers)?;
    }
    Ok(())
}

/// A bar of the books: those read dark, those being read shaded, the rest a
/// fine line.
fn draw_share(
    out: &mut Vec<Paint>,
    look: &Look,
    area: Rect,
    numbers: &Numbers,
) -> Result<(), Error> {
    let fine = 1u16.clamp(1, area.height.max(1));
    shape::fill(
        out,
        Rect::new(area.x, area.y + (area.height - fine) / 2, area.width, fine),
        look.ink,
    )?;
    let share = |n: u32| {
        (u64::from(area.width) * u64::from(n) / u64::from(numbers.books.max(1))) as u16
    };
    let read = share(numbers.read);
    let reading = share(numbers.reading);
    if reading > 0 {
        let at = Rect::new(
            area.x + read,
            area.y,
            reading.min(area.width - read),
            area.height,
        );
        // Shaded in interface 6 (a dither); a plain gray here, as the
        // battery's low charge.
        let shade = if look.paper.luma() < 128 {
            Color::gray(140)
        } else {
            Color::gray(150)
        };
        paint::push(out, Paint::Focus(at, u32::from(area.height / 2), None, shade))?;
    }
    if read > 0 {
        let at = Rect::new(
            area.x,
            area.y,
            read.max(area.height).min(area.width),
            area.height,
        );
        paint::push(
            out,
            Paint::Focus(at, u32::from(area.height / 2), None, look.ink),
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_library_is_counted() {
        let numbers = Numbers::count(10, 3, 2);
        assert_eq!(
            numbers,
            Numbers {
                books: 10,
                read: 3,
                reading: 2,
                unread: 5
            }
        );
        assert_eq!(numbers.of("books"), 10);
        assert_eq!(numbers.of("finished"), 3);
        assert_eq!(numbers.of("reading"), 2);
        assert_eq!(numbers.of("unread"), 5);
    }

    #[test]
    fn books_opened_outside_the_library_still_count() {
        let numbers = Numbers::count(1, 1, 1);
        assert_eq!((numbers.books, numbers.unread), (2, 0));
        assert_eq!(Numbers::count(0, 0, 0), Numbers::default());
    }

    #[test]
    fn no_count_overflows() {
        let numbers = Numbers::count(u32::MAX, u32::MAX, u32::MAX);
        assert_eq!(numbers.unread, 0);
    }
}
