//! The word clock: the time in words, to five minutes, in the reader's
//! language ("It is quarter past ten", "Es ist Viertel nach zehn"): as text,
//! as large as fits, or as a grid of letters with the words that tell the
//! time lit, the others faint.
//!
//! What it says comes from the translations, so a language brings its own:
//! `widgets.word-clock.<minute>` for each five minutes (with `{hour}` for the
//! hour it names, this one or the next as `widgets.word-clock.hours` says),
//! the hours' names (`widgets.word-clock.hour.<1-12>`), and the grid of
//! letters (`widgets.word-clock.grid`, its rows separated by spaces).
//! Quarters may be told towards the next hour ("viertel elf"), as some
//! regions say (`widgets.word-clock.next.<15|45>`).
use crate::native_draw::no_time;
use alloc::{format, string::String, vec::Vec};
use paperpaw_sdk::native::Host;
use paperpaw_sdk::native::fonts::TextOrigin;
use paperpaw_sdk::{
    Color, Point, Rect,
    native::{Error, formatting::Arg},
};
use paperpaw_ui_kit::widget::names::chosen_name;
use paperpaw_ui_kit::widget::{Cx, Settings, cx::local_time_now};
use paperpaw_ui_kit::{
    Paint, paint,
    text::{concat, copy},
    widget::{
        TextSize,
        layout::{balanced, cap_height, caption, caption_height, largest, line, words_in_lines},
        shape::{fill_circle, stroke_circle},
    },
};

/// The minute keys' prefix.
const KEY: &str = "widgets.word-clock";

/// Where the words of a language come from: the catalog of the host, or a
/// translator in a test.
pub trait Words {
    /// The text of `key`.
    ///
    /// # Errors
    /// The host's refusal and [`Error::NoMemory`].
    fn word(&mut self, key: &str) -> Result<String, Error>;
    /// The text of `key` with `{hour}` the hour's name.
    ///
    /// # Errors
    /// The host's refusal and [`Error::NoMemory`].
    fn say(&mut self, key: &str, hour: &str) -> Result<String, Error>;
}

impl Words for Cx<'_> {
    fn word(&mut self, key: &str) -> Result<String, Error> {
        self.tr(key)
    }

    fn say(&mut self, key: &str, hour: &str) -> Result<String, Error> {
        self.format(
            key,
            &[Arg::Text {
                name: "hour",
                value: hour,
            }],
        )
    }
}

/// The time told: the five minutes it says (rounded, or the last five
/// minutes where dots show the rest), and the minutes past them.
pub fn told(hour: u8, minute: u8, dots: bool) -> (u8, u8, u8) {
    if dots {
        return (hour, minute / 5 * 5, minute % 5);
    }
    let total = (u16::from(hour) * 60 + u16::from(minute) + 2) / 5 * 5 % (24 * 60);
    ((total / 60) as u8, (total % 60) as u8, 0)
}

/// `text` with its first letter a capital.
pub fn capitalised(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

pub fn upper(text: &str) -> String {
    text.chars().flat_map(char::to_uppercase).collect()
}

/// The letters of the grid lit by `words` (row, column): each word found
/// in a row, the words in order, the last one as late in the grid as it
/// can be (the hour comes last, below the minutes); `None` if a word is
/// not in it.
pub fn lit(grid: &[Vec<char>], words: &str) -> Option<Vec<(usize, usize)>> {
    let words: Vec<Vec<char>> = words
        .split_whitespace()
        .map(|w| {
            w.chars()
                .filter(|c| c.is_alphabetic())
                .flat_map(char::to_uppercase)
                .collect::<Vec<char>>()
        })
        .filter(|w| !w.is_empty())
        .collect();
    let columns = grid.iter().map(Vec::len).max().unwrap_or(0);
    // Positions counted through the rows; each word before the one after.
    let mut before = grid.len() * columns;
    let mut cells = Vec::new();
    for word in words.iter().rev() {
        let mut found = None;
        'rows: for (r, row) in grid.iter().enumerate().rev() {
            if row.len() < word.len() {
                continue;
            }
            for c in (0..=row.len() - word.len()).rev() {
                if r * columns + c + word.len() <= before && row[c..c + word.len()] == word[..] {
                    found = Some((r, c));
                    break 'rows;
                }
            }
        }
        let (r, c) = found?;
        before = r * columns + c;
        cells.extend((c..c + word.len()).map(|c| (r, c)));
    }
    Some(cells)
}

/// What it says at `hour`:`minute` (the five minutes told): the lead ("It
/// is") and the rest ("quarter past ten").
///
/// # Errors
/// The host's refusal and [`Error::NoMemory`].
pub fn sentence<W: Words>(
    words: &mut W,
    hour: u8,
    minute: u8,
    next: bool,
) -> Result<(String, String), Error> {
    let minute = minute / 5 * 5;
    let quarter = next && matches!(minute, 15 | 45);
    let key = if quarter {
        format!("{KEY}.next.{minute}")
    } else {
        format!("{KEY}.{minute}")
    };
    // Which hour it names: this one or the next ("half past ten", "halb
    // elf").
    let (hours, at) = if quarter {
        ("widgets.word-clock.next.hours", usize::from(minute / 30))
    } else {
        ("widgets.word-clock.hours", usize::from(minute / 5))
    };
    let ahead = words.word(hours)?.split_whitespace().nth(at) == Some("1");
    let named = if ahead { hour + 1 } else { hour };
    let name = if minute == 0 && named % 12 == 1 {
        copy(words.word("widgets.word-clock.oclock-one")?.trim())?
    } else {
        let h = match named % 12 {
            0 => 12,
            h => h,
        };
        copy(words.word(&format!("{KEY}.hour.{h}"))?.trim())?
    };
    let rest = words.say(&key, &name)?;
    Ok((copy(words.word("widgets.word-clock.lead")?.trim())?, rest))
}

/// The grid's rows, from the translations.
///
/// # Errors
/// The host's refusal and [`Error::NoMemory`].
pub fn grid<W: Words>(words: &mut W) -> Result<Vec<Vec<char>>, Error> {
    let text = words.word("widgets.word-clock.grid")?;
    let mut rows: Vec<Vec<char>> = Vec::new();
    for row in text.split_whitespace() {
        let mut letters: Vec<char> = Vec::new();
        letters
            .try_reserve(row.len())
            .map_err(|_| Error::NoMemory)?;
        letters.extend(row.chars().flat_map(char::to_uppercase));
        rows.try_reserve(1).map_err(|_| Error::NoMemory)?;
        rows.push(letters);
    }
    Ok(rows)
}

/// The whole sentence as the settings want it: with the lead or without, in
/// capitals or not.
fn text(cx: &mut Cx<'_>, hour: u8, minute: u8) -> Result<(Option<String>, String), Error> {
    let next = chosen_name(cx, "quarters")?.as_str() == "next";
    let (lead, rest) = sentence(cx, hour, minute, next)?;
    let capitals = cx.settings.shows("capitals")?;
    let case = |s: String| if capitals { upper(&s) } else { s };
    Ok(match chosen_name(cx, "lead")?.as_str() {
        "none" => (None, case(capitalised(&rest))),
        "caption" => (Some(case(lead)), case(capitalised(&rest))),
        _ => (None, case(format!("{lead} {rest}"))),
    })
}

/// The time alone, without the lead ("Quarter past ten"): what it says where
/// the whole sentence has no room.
fn time_alone(cx: &mut Cx<'_>, hour: u8, minute: u8) -> Result<String, Error> {
    let next = chosen_name(cx, "quarters")?.as_str() == "next";
    let (_, rest) = sentence(cx, hour, minute, next)?;
    let rest = capitalised(&rest);
    Ok(if cx.settings.shows("capitals")? {
        upper(&rest)
    } else {
        rest
    })
}

/// What it shows, to tell when it changed: the five minutes told.
///
/// # Errors
/// Host refusals.
pub fn shows(host: &Host<'_>, widget: &str) -> Result<u64, Error> {
    let Some(t) = local_time_now(host)? else {
        return Ok(0);
    };
    let (hour, minute, rest) = told(t.hour, t.minute, Settings::new(host, widget).shows("dots")?);
    Ok(u64::from(hour) * 10_000 + u64::from(minute) * 100 + u64::from(rest) + 1)
}

/// The height: the grid as high as wide, or three lines of text.
///
/// # Errors
/// Font and host refusals.
pub fn height(cx: &mut Cx<'_>, width: u16) -> Result<u16, Error> {
    if chosen_name(cx, "style")?.as_str() == "grid" {
        let rows = (u32::from(cx.style().row) * 5) as u16;
        return Ok(width.min(rows).saturating_add(cx.shape.frame_height()));
    }
    let size = cx.settings.size()?.text(cx.style());
    let line = paperpaw_ui_kit::geometry::line_px(&cx.font, size)?;
    Ok(line
        .saturating_mul(3)
        .saturating_add(cx.shape.frame_height()))
}

/// Draws the widget in `area`.
///
/// # Errors
/// Font and host refusals, [`Error::NoMemory`].
pub fn draw(cx: &mut Cx<'_>, out: &mut Vec<Paint>, area: Rect) -> Result<(), Error> {
    let look = cx.shape.paint(out, area, 256)?;
    let Some(now) = cx.local_time()? else {
        return no_time(cx, out, &look);
    };
    let dots = cx.settings.shows("dots")?;
    let (hour, minute, past) = told(now.hour, now.minute, dots);
    let told = Told {
        hour,
        minute,
        past: dots.then_some(past),
    };
    if chosen_name(cx, "style")?.as_str() == "grid" && draw_grid(cx, out, &look, &told)? {
        return Ok(());
    }
    draw_text(cx, out, &look, &told)
}

/// The time told: the hour and the five minutes, and the minutes past them
/// where dots show them.
struct Told {
    hour: u8,
    minute: u8,
    past: Option<u8>,
}

/// The time as text, as large as fits (or as set), its lines balanced.
#[allow(clippy::too_many_lines, reason = "one drawing")]
fn draw_text(
    cx: &mut Cx<'_>,
    out: &mut Vec<Paint>,
    look: &paperpaw_ui_kit::widget::Look,
    told: &Told,
) -> Result<(), Error> {
    let (small_size, gap) = (cx.style().small, cx.style().gap);
    let mut area = look.area;
    let align = cx.settings.align()?;
    let (lead, rest) = text(cx, told.hour, told.minute)?;
    // Room for the dots below.
    let dot = (small_size * 0.22).max(2.0) as u32;
    let dots_band = if told.past.is_some() {
        (dot * 6) as u16
    } else {
        0
    };
    // The caption above, where it leaves a line of the small size.
    let least = line(&cx.font, small_size)? as u16;
    let caption_h = caption_height(&cx.font, small_size, gap)?;
    if let Some(lead) = &lead
        && area.height >= caption_h + least + dots_band
    {
        area = caption(
            out,
            &cx.font,
            (small_size, gap),
            (area, align, look.faint),
            lead,
        )?;
    }
    let room = Rect::new(
        area.x,
        area.y,
        area.width,
        area.height.saturating_sub(dots_band),
    );
    // Where the sentence has no room whole even at the small size, the time
    // alone: "It is" says the least.
    let whole = match words_in_lines(&cx.font, small_size, &rest, f32::from(room.width))? {
        Some(lines) => line(&cx.font, small_size)? * lines.len() as f32 <= f32::from(room.height),
        None => false,
    };
    let rest = if whole {
        rest
    } else {
        time_alone(cx, told.hour, told.minute)?
    };
    let size = match cx.settings.size()? {
        TextSize::Fit => largest(
            &cx.font,
            &rest,
            (room.width, room.height),
            (small_size, f32::from(room.height)),
        )?,
        size => largest(
            &cx.font,
            &rest,
            (room.width, room.height),
            (small_size, size.text(cx.style()) * 1.2),
        )?,
    };
    let height = line(&cx.font, size)?;
    let mut lines = balanced(&cx.font, size, &rest, f32::from(room.width))?;
    // Where not even the small size fits: as many lines as there is room for,
    // the last one ending in "…".
    let most = ((f32::from(room.height) / height) as usize).max(1);
    if lines.len() > most {
        let mut tail = String::new();
        for (n, held) in lines.iter().skip(most - 1).enumerate() {
            tail.try_reserve(held.len() + 4)
                .map_err(|_| Error::NoMemory)?;
            if n > 0 {
                tail.push(' ');
            }
            tail.push_str(held);
        }
        tail.push_str(" …");
        lines.truncate(most - 1);
        lines.try_reserve(1).map_err(|_| Error::NoMemory)?;
        lines.push(paperpaw_ui_kit::text::fit(
            &cx.font,
            size,
            &tail,
            f32::from(room.width),
        )?);
    }
    let total = height * lines.len() as f32;
    let top = f32::from(room.y) + (f32::from(room.height) - total).max(0.0) / 2.0;
    let (ascent, descent, _) = paperpaw_ui_kit::geometry::line_metrics(&cx.font, size)?;
    for (n, held) in lines.iter().enumerate() {
        let y = top + height * n as f32;
        let baseline = y + (height + ascent - descent) / 2.0;
        let shown = paperpaw_ui_kit::text::fit(&cx.font, size, held, f32::from(room.width))?;
        let x = align.x(room, cx.font.measure_text(size, &shown)?);
        paint::push(
            out,
            Paint::Text {
                text: shown,
                size,
                origin: TextOrigin::new(x, baseline as i32),
                color: look.ink,
                clip: Some(paperpaw_ui_kit::widget::text::ink_room(room)),
            },
        )?;
    }
    if let Some(past) = told.past {
        // The minutes past the five, as dots below the text.
        let y = (top + total) as i32 + (dot * 3) as i32;
        let gap = (dot * 4) as i32;
        let width = gap * 3 + 2 * dot as i32;
        let x = align.x(area, width as f32) as i32 + dot as i32;
        for n in 0..4 {
            let centre = Point::new(x + gap * n, y);
            if n < i32::from(past) {
                fill_circle(out, centre, dot, look.ink)?;
            } else {
                stroke_circle(out, centre, dot, 1, look.faint)?;
            }
        }
    }
    Ok(())
}

/// The time as a grid of letters, the words lit; `false` if the language has
/// no grid that tells it (the text is drawn then).
#[allow(clippy::too_many_lines, reason = "one drawing")]
fn draw_grid(
    cx: &mut Cx<'_>,
    out: &mut Vec<Paint>,
    look: &paperpaw_ui_kit::widget::Look,
    told: &Told,
) -> Result<bool, Error> {
    let next = chosen_name(cx, "quarters")?.as_str() == "next";
    let (lead, rest) = sentence(cx, told.hour, told.minute, next)?;
    let rows = grid(cx)?;
    let Some(cells) = lit(&rows, &concat(&[&lead, " ", &rest])?) else {
        return Ok(false);
    };
    let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
    if columns == 0 {
        return Ok(false);
    }
    let area = look.area;
    // Dots at the corners take half a cell round the grid.
    let margins = if told.past.is_some() { 1.0 } else { 0.0 };
    let cell_w = f32::from(area.width) / (columns as f32 + margins);
    let cell_h = f32::from(area.height) / (rows.len() as f32 + margins);
    // Cells about as high as wide.
    let cell_w = cell_w.min(cell_h * 1.5);
    let cell_h = cell_h.min(cell_w * 1.25);
    let width = cell_w * (columns as f32 + margins);
    let height = cell_h * (rows.len() as f32 + margins);
    let align = cx.settings.align()?;
    let left = align.x(area, width) + cell_w * margins / 2.0;
    let top = f32::from(area.y) + (f32::from(area.height) - height) / 2.0 + cell_h * margins / 2.0;
    let size = cell_w.min(cell_h) * 0.6;
    // Where the font has no letters that small, the text instead.
    if cx.font.measure_text(size, "W")? > cell_w * 0.95
        || cap_height(&cx.font, size)? > cell_h * 0.8
    {
        return Ok(false);
    }
    // A quarter of the way from the paper to the ink. (On a panel of black
    // and white alone the interface-6 widget halved the letter's dots; the
    // snapshot does not tell the grays yet.)
    let luma = (u16::from(look.ink.luma()) + 3 * u16::from(look.paper.luma())) / 4;
    let unlit = Color::gray(luma as u8);
    let cap = cap_height(&cx.font, size)?;
    for (r, row) in rows.iter().enumerate() {
        for (c, letter) in row.iter().enumerate() {
            let on = cells.contains(&(r, c));
            let x = left + cell_w * (c as f32 + 0.5);
            let y = top + cell_h * (r as f32 + 0.5);
            let mut buffer = [0u8; 4];
            let shown = letter.encode_utf8(&mut buffer);
            let wide = cx.font.measure_text(size, &*shown)?;
            let origin = TextOrigin::new(x - wide / 2.0, (y + cap / 2.0) as i32);
            let text = copy(&*shown)?;
            if on {
                // The lit letters in the bold face, where the interface-6
                // widget drew them a little heavier.
                paint::push(
                    out,
                    Paint::Bold {
                        text,
                        size,
                        origin,
                        color: look.ink,
                    },
                )?;
            } else {
                paint::push(
                    out,
                    Paint::Text {
                        text,
                        size,
                        origin,
                        color: unlit,
                        clip: None,
                    },
                )?;
            }
        }
    }
    if let Some(past) = told.past {
        let dot = (cell_w.min(cell_h) * 0.12).max(2.0) as u32;
        let (l, t) = (left - cell_w * 0.25, top - cell_h * 0.25);
        let (r, b) = (
            left + cell_w * (columns as f32 + 0.25),
            top + cell_h * (rows.len() as f32 + 0.25),
        );
        let corners = [(l, t), (r, t), (r, b), (l, b)];
        for (n, (x, y)) in corners.into_iter().enumerate() {
            let centre = Point::new(x as i32, y as i32);
            if n < usize::from(past) {
                fill_circle(out, centre, dot, look.ink)?;
            } else {
                fill_circle(out, centre, dot, unlit)?;
            }
        }
    }
    Ok(true)
}

/// A translator standing for the host's catalog in the tests.
#[cfg(test)]
pub struct Fake<'a>(pub &'a paperpaw_sdk::Translator);

#[cfg(test)]
impl Words for Fake<'_> {
    fn word(&mut self, key: &str) -> Result<String, Error> {
        Ok(String::from(self.0.tr(key)))
    }

    fn say(&mut self, key: &str, hour: &str) -> Result<String, Error> {
        Ok(self
            .0
            .format(key, &[("hour", paperpaw_sdk::Arg::Str(hour))]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_i18n::Catalog;
    use paperpaw_sdk::Translator;

    pub(super) fn translator(language: &str) -> Translator {
        let mut t = Translator::new();
        t.add(Catalog::parse(include_str!("../i18n/en.ini")).unwrap());
        t.add(Catalog::parse(include_str!("../i18n/de.ini")).unwrap());
        t.set_language(language);
        t
    }

    fn says(t: &Translator, hour: u8, minute: u8, next: bool) -> String {
        let (h, m, _) = told(hour, minute, false);
        let (lead, rest) = sentence(&mut Fake(t), h, m, next).unwrap();
        format!("{lead} {rest}")
    }

    #[test]
    fn english_times_in_words() {
        let en = translator("en");
        for (h, m, text) in [
            (10, 0, "It is ten o'clock"),
            (10, 2, "It is ten o'clock"),
            (10, 3, "It is five past ten"),
            (10, 10, "It is ten past ten"),
            (10, 15, "It is quarter past ten"),
            (10, 20, "It is twenty past ten"),
            (10, 25, "It is twenty-five past ten"),
            (10, 30, "It is half past ten"),
            (10, 35, "It is twenty-five to eleven"),
            (10, 41, "It is twenty to eleven"),
            (10, 43, "It is quarter to eleven"),
            (10, 50, "It is ten to eleven"),
            (10, 55, "It is five to eleven"),
            (10, 58, "It is eleven o'clock"),
            (0, 0, "It is twelve o'clock"),
            (12, 45, "It is quarter to one"),
            (13, 0, "It is one o'clock"),
            (23, 58, "It is twelve o'clock"),
        ] {
            assert_eq!(says(&en, h, m, false), text, "{h}:{m}");
        }
        // English quarters are the same either way.
        assert_eq!(says(&en, 10, 15, true), "It is quarter past ten");
    }

    #[test]
    fn german_times_in_words() {
        let de = translator("de");
        for (h, m, text) in [
            (10, 0, "Es ist zehn Uhr"),
            (10, 5, "Es ist fünf nach zehn"),
            (10, 10, "Es ist zehn nach zehn"),
            (10, 15, "Es ist Viertel nach zehn"),
            (10, 20, "Es ist zwanzig nach zehn"),
            (10, 25, "Es ist fünf vor halb elf"),
            (10, 30, "Es ist halb elf"),
            (10, 35, "Es ist fünf nach halb elf"),
            (10, 40, "Es ist zwanzig vor elf"),
            (10, 45, "Es ist Viertel vor elf"),
            (10, 50, "Es ist zehn vor elf"),
            (10, 55, "Es ist fünf vor elf"),
            (1, 0, "Es ist ein Uhr"),
            (13, 5, "Es ist fünf nach eins"),
            (12, 30, "Es ist halb eins"),
            (0, 0, "Es ist zwölf Uhr"),
        ] {
            assert_eq!(says(&de, h, m, false), text, "{h}:{m}");
        }
        // Quarters towards the next hour, as some regions say.
        assert_eq!(says(&de, 10, 15, true), "Es ist viertel elf");
        assert_eq!(says(&de, 10, 45, true), "Es ist dreiviertel elf");
        assert_eq!(says(&de, 10, 30, true), "Es ist halb elf");
    }

    #[test]
    fn dots_tell_the_minutes_past() {
        assert_eq!(told(10, 41, true), (10, 40, 1));
        assert_eq!(told(10, 44, true), (10, 40, 4));
        assert_eq!(told(10, 44, false), (10, 45, 0));
        assert_eq!(told(23, 59, false), (0, 0, 0));
    }

    #[test]
    fn every_time_is_in_the_grid() {
        for language in ["en", "de"] {
            let t = translator(language);
            let rows = grid(&mut Fake(&t)).unwrap();
            assert!(rows.len() >= 8, "{language}");
            for next in [false, true] {
                for hour in 0..24 {
                    for minute in (0..60).step_by(5) {
                        let (lead, rest) = sentence(&mut Fake(&t), hour, minute, next).unwrap();
                        let words = format!("{lead} {rest}");
                        assert!(lit(&rows, &words).is_some(), "{language}: {words}");
                    }
                }
            }
        }
    }

    #[test]
    fn the_hour_is_lit_below_the_minutes() {
        let en = translator("en");
        let rows = grid(&mut Fake(&en)).unwrap();
        // "ten past ten": the first "ten" in the minutes, the second the
        // hour in the last row.
        let cells = lit(&rows, "It is ten past ten").unwrap();
        let last = rows.len() - 1;
        assert!(cells.iter().any(|&(r, _)| r == last));
        assert!(cells.iter().any(|&(r, _)| r == 3));
        // "five o'clock": the hour's five, not the minutes'.
        let cells = lit(&rows, "It is five o'clock").unwrap();
        assert!(!cells.iter().any(|&(r, _)| r == 2));
        assert!(lit(&rows, "It is teatime").is_none());
    }

    #[test]
    fn capitals_and_case() {
        assert_eq!(capitalised("viertel elf"), "Viertel elf");
        assert_eq!(upper("fünf vor zwölf"), "FÜNF VOR ZWÖLF");
    }
}

#[cfg(test)]
#[path = "native_word_clock/minutes_tests.rs"]
mod minutes_tests;
