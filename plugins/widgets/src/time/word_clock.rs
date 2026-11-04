//! The word clock: the time in words, to five minutes, in the reader's
//! language ("It is quarter past ten", "Es ist Viertel nach zehn"): as
//! text, as large as fits, or as a grid of letters with the words that
//! tell the time lit, the others faint.
//!
//! What it says comes from the translations, so a language brings its
//! own: `widgets.word-clock.<minute>` for each five minutes (with `{hour}`
//! for the hour it names, this one or the next as
//! `widgets.word-clock.hours` says), the hours' names
//! (`widgets.word-clock.hour.<1-12>`), and the grid of letters
//! (`widgets.word-clock.grid`, its rows separated by spaces). Quarters
//! may be told towards the next hour ("viertel elf"), as some regions
//! say (`widgets.word-clock.next.<15|45>`).

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::time;
use paperpaw_sdk::widgets::{self, Align, Drawn, Frame, Padding, TextSize};
use paperpaw_sdk::{
    Arg, Canvas, Color, Env, FontHandle, Id, Point, Rect, RegisterError, Registrar, Translator,
};

use paperpaw_sdk::SettingSpec;

use super::{chosen_name, frame_height, line, ui};

const ID: Id = Id::new_static("widgets.word-clock");

/// The values of the style setting: text, a grid of letters.
const STYLES: [&str; 2] = [
    "widgets.word-clock.style.text",
    "widgets.word-clock.style.grid",
];
/// The values of the quarters setting: "quarter past ten", or towards the
/// next hour ("viertel elf").
const QUARTERS: [&str; 2] = [
    "widgets.word-clock.quarters.past",
    "widgets.word-clock.quarters.next",
];
/// The values of the lead setting ("It is"): left out, as part of the
/// sentence, as a small caption above it.
const LEADS: [&str; 3] = [
    "widgets.word-clock.lead.none",
    "widgets.word-clock.lead.same",
    "widgets.word-clock.lead.caption",
];

/// The minute keys' prefix.
const KEY: &str = "widgets.word-clock";

/// Its settings, kept in the program.
static SETTINGS: [SettingSpec; 11] = [
    widgets::align_spec("widgets.word-clock.align", ID, Align::Left),
    widgets::frame_spec("widgets.word-clock.frame", ID, Frame::None),
    widgets::padding_spec("widgets.word-clock.padding", ID, Padding::Auto),
    widgets::paper_spec("widgets.word-clock.paper", ID),
    widgets::ink_spec("widgets.word-clock.ink", ID),
    SettingSpec::choice(
        "widgets.word-clock.style",
        "widgets.setting.style",
        &STYLES,
        STYLES[0],
        ID,
    )
    .description("widgets.word-clock.style.help"),
    widgets::size_spec("widgets.word-clock.size", ID, TextSize::Fit, true),
    SettingSpec::choice(
        "widgets.word-clock.lead",
        "widgets.setting.word-clock-lead",
        &LEADS,
        LEADS[1],
        ID,
    )
    .description("widgets.word-clock.lead.help"),
    SettingSpec::choice(
        "widgets.word-clock.quarters",
        "widgets.setting.quarters",
        &QUARTERS,
        QUARTERS[0],
        ID,
    )
    .description("widgets.word-clock.quarters.help"),
    widgets::part_spec(
        "widgets.word-clock.capitals",
        ID,
        "widgets.setting.capitals",
        false,
    ),
    widgets::part_spec(
        "widgets.word-clock.dots",
        ID,
        "widgets.setting.minute-dots",
        false,
    ),
];

pub(super) fn register(r: &mut Registrar) -> Result<(), RegisterError> {
    super::add(
        r,
        {
            static SPEC: paperpaw_sdk::SettingGroupSpec =
                paperpaw_sdk::SettingGroupSpec::new(ID, "widgets.word-clock");
            &SPEC
        },
        Drawn::new(height, draw).shows(shows),
    )?;
    r.add_settings(&SETTINGS)
}

/// The time told: the five minutes it says (rounded, or the last five
/// minutes where dots show the rest), and the minutes past them.
fn told(hour: u8, minute: u8, dots: bool) -> (u8, u8, u8) {
    if dots {
        return (hour, minute / 5 * 5, minute % 5);
    }
    let total = (u16::from(hour) * 60 + u16::from(minute) + 2) / 5 * 5 % (24 * 60);
    ((total / 60) as u8, (total % 60) as u8, 0)
}

/// What it says at `hour`:`minute` (the five minutes told): the lead
/// ("It is") and the rest ("quarter past ten").
pub(super) fn sentence(i18n: &Translator, hour: u8, minute: u8, next: bool) -> (String, String) {
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
    let ahead = i18n.tr(hours).split_whitespace().nth(at) == Some("1");
    let named = if ahead { hour + 1 } else { hour };
    let name: String = if minute == 0 && named % 12 == 1 {
        i18n.tr("widgets.word-clock.oclock-one").trim().into()
    } else {
        let h = match named % 12 {
            0 => 12,
            h => h,
        };
        i18n.tr(&format!("{KEY}.hour.{h}")).trim().into()
    };
    let rest = i18n.format(&key, &[("hour", Arg::Str(&name))]);
    (
        String::from(i18n.tr("widgets.word-clock.lead").trim()),
        rest,
    )
}

/// The whole sentence as the settings want it: with the lead or without,
/// in capitals or not.
fn text(env: &Env<'_>, hour: u8, minute: u8) -> (Option<String>, String) {
    let next = chosen_name(env, ID.as_str(), "quarters") == "next";
    let (lead, rest) = sentence(env.i18n(), hour, minute, next);
    let capitals = widgets::shows(env, ID.as_str(), "capitals");
    let case = |s: String| if capitals { upper(&s) } else { s };
    match chosen_name(env, ID.as_str(), "lead") {
        "none" => (None, case(capitalised(&rest))),
        "caption" => (Some(case(lead)), case(capitalised(&rest))),
        _ => (None, case(format!("{lead} {rest}"))),
    }
}

/// The time alone, without the lead ("Quarter past ten"): what it says
/// where the whole sentence has no room.
fn time_alone(env: &Env<'_>, hour: u8, minute: u8) -> String {
    let next = chosen_name(env, ID.as_str(), "quarters") == "next";
    let (_, rest) = sentence(env.i18n(), hour, minute, next);
    let rest = capitalised(&rest);
    if widgets::shows(env, ID.as_str(), "capitals") {
        upper(&rest)
    } else {
        rest
    }
}

/// `text` with its first letter a capital.
fn capitalised(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

fn upper(text: &str) -> String {
    text.chars().flat_map(char::to_uppercase).collect()
}

fn shows(env: &Env<'_>) -> String {
    let Some(t) = time::local_time(env) else {
        return String::new();
    };
    let (hour, minute, rest) = told(t.hour, t.minute, widgets::shows(env, ID.as_str(), "dots"));
    format!("{hour}:{minute}+{rest}")
}

fn height(env: &Env<'_>, width: u16) -> u16 {
    let (style, _) = ui(env);
    let lines = if chosen_name(env, ID.as_str(), "style") == "grid" {
        return width.min((u32::from(style.row()) * 5) as u16) + frame_height(env, ID.as_str());
    } else {
        3
    };
    let size = widgets::size(env, ID.as_str()).text(&style);
    widgets::line_height(env, size) * lines + frame_height(env, ID.as_str())
}

fn draw(canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
    let look = widgets::draw_frame(canvas, env, ID.as_str(), area);
    let (_, font) = ui(env);
    let Some(now) = time::local_time(env) else {
        super::draw_no_time(canvas, env, &look);
        return;
    };
    let Some(font) = font else {
        return;
    };
    let dots = widgets::shows(env, ID.as_str(), "dots");
    let (hour, minute, past) = told(now.hour, now.minute, dots);
    if chosen_name(env, ID.as_str(), "style") == "grid"
        && draw_grid(
            canvas,
            env,
            &font,
            &look,
            hour,
            minute,
            dots.then_some(past),
        )
    {
        return;
    }
    draw_text(
        canvas,
        env,
        &font,
        &look,
        hour,
        minute,
        dots.then_some(past),
    );
}

/// The time as text, as large as fits (or as set), its lines balanced.
#[allow(
    clippy::too_many_arguments,
    reason = "its inputs are separate things; a struct of them would serve this call alone"
)]
fn draw_text(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    font: &FontHandle<'_>,
    look: &widgets::WidgetLook,
    hour: u8,
    minute: u8,
    past: Option<u8>,
) {
    let (style, _) = ui(env);
    let mut area = look.area;
    let align = widgets::align(env, ID.as_str());
    let (lead, rest) = text(env, hour, minute);
    // Room for the dots below.
    let dot = (style.small() * 0.22).max(2.0) as u32;
    let dots_band = if past.is_some() { (dot * 6) as u16 } else { 0 };
    // The caption above, where it leaves a line of the small size.
    let least = line(font, style.small()) as u16;
    let caption = widgets::caption_height(env);
    if let Some(lead) = lead
        && area.height >= caption + least + dots_band
    {
        area = widgets::draw_caption(canvas, env, ID.as_str(), look, &lead);
    }
    let room = Rect::new(
        area.x,
        area.y,
        area.width,
        area.height.saturating_sub(dots_band),
    );
    // Where the sentence has no room whole even at the small size, the
    // time alone: "It is" says the least.
    let whole = |text: &str| {
        words_in_lines(font, style.small(), text, f32::from(room.width)).is_some_and(|lines| {
            line(font, style.small()) * lines.len() as f32 <= f32::from(room.height)
        })
    };
    let rest = if whole(&rest) {
        rest
    } else {
        time_alone(env, hour, minute)
    };
    let size = match widgets::size(env, ID.as_str()) {
        TextSize::Fit => largest(font, &rest, room, style.small(), f32::from(room.height)),
        size => largest(font, &rest, room, style.small(), size.text(&style) * 1.2),
    };
    let height = line(font, size);
    let mut lines = balanced(font, size, &rest, f32::from(room.width));
    // Where not even the small size fits: as many lines as there is room
    // for, the last one ending in "…".
    let most = ((f32::from(room.height) / height) as usize).max(1);
    if lines.len() > most {
        let rest = lines[most - 1..].join(" ");
        lines.truncate(most - 1);
        lines.push(paperpaw_sdk::ui::fit(
            font,
            size,
            &format!("{rest} …"),
            f32::from(room.width),
        ));
    }
    let total = height * lines.len() as f32;
    let top = f32::from(room.y) + (f32::from(room.height) - total).max(0.0) / 2.0;
    let metrics = font.line_metrics(size);
    for (n, text) in lines.iter().enumerate() {
        let y = top + height * n as f32;
        let baseline = y + (height + metrics.ascent - metrics.descent) / 2.0;
        let shown = paperpaw_sdk::ui::fit(font, size, text, f32::from(room.width));
        let x = align.x(room, font.measure(size, &shown));
        font.draw(canvas, size, x, baseline as i32, &shown, look.ink);
    }
    canvas.describe(paperpaw_sdk::semantics::Role::Text, &rest, room);
    if let Some(past) = past {
        // The minutes past the five, as dots below the text.
        let y = (top + total) as i32 + (dot * 3) as i32;
        let gap = (dot * 4) as i32;
        let width = gap * 3 + 2 * dot as i32;
        let x = align.x(area, width as f32) as i32 + dot as i32;
        for n in 0..4 {
            let center = Point::new(x + gap * n, y);
            if n < i32::from(past) {
                canvas.fill_circle(center, dot, look.ink);
            } else {
                canvas.stroke_circle(center, dot, 1, look.faint);
            }
        }
    }
}

/// The largest size, from `least` to `most`, at which `text` fits `room`
/// broken into lines between words (no word wider than a line), as it
/// is drawn; `least` where none does.
pub(super) fn largest(font: &FontHandle<'_>, text: &str, room: Rect, least: f32, most: f32) -> f32 {
    let width = f32::from(room.width);
    let fits = |size: f32| {
        words_in_lines(font, size, text, width)
            .is_some_and(|lines| line(font, size) * lines.len() as f32 <= f32::from(room.height))
    };
    widgets::largest_size(most.max(least), least, fits).unwrap_or(least)
}

/// The lines `text` breaks into at `size` in `width` pixels, between
/// words only; `None` where a word alone is wider.
fn words_in_lines(font: &FontHandle<'_>, size: f32, text: &str, width: f32) -> Option<Vec<String>> {
    let mut lines: Vec<String> = Vec::new();
    for word in text.split_whitespace() {
        if font.measure(size, word) > width {
            return None;
        }
        match lines.last_mut() {
            Some(last) if font.measure(size, &format!("{last} {word}")) <= width => {
                last.push(' ');
                last.push_str(word);
            }
            _ => lines.push(String::from(word)),
        }
    }
    Some(lines)
}

/// `text` broken into as many lines as `width` needs at `size`, between
/// words only, the lines as even as can be (not one long and one short).
/// A word wider than a line has a line to itself.
pub(super) fn balanced(font: &FontHandle<'_>, size: f32, text: &str, width: f32) -> Vec<String> {
    let lines = |width: f32| -> Option<Vec<String>> { words_in_lines(font, size, text, width) };
    let Some(first) = lines(width) else {
        return text.split_whitespace().map(String::from).collect();
    };
    let count = first.len();
    if count <= 1 {
        return first;
    }
    // The narrowest width that needs no more lines.
    let (mut low, mut high) = (width / count as f32, width);
    let mut best = first;
    for _ in 0..8 {
        let mid = (low + high) / 2.0;
        match lines(mid) {
            Some(narrow) if narrow.len() <= count => {
                high = mid;
                best = narrow;
            }
            _ => low = mid,
        }
    }
    best
}

/// The grid's rows, from the translations.
fn grid(i18n: &Translator) -> Vec<Vec<char>> {
    i18n.tr("widgets.word-clock.grid")
        .split_whitespace()
        .map(|row| row.chars().flat_map(char::to_uppercase).collect())
        .collect()
}

/// The letters of the grid lit by `words` (row, column): each word found
/// in a row, the words in order, the last one as late in the grid as it
/// can be (the hour comes last, below the minutes); `None` if a word is
/// not in it.
pub(super) fn lit(grid: &[Vec<char>], words: &str) -> Option<Vec<(usize, usize)>> {
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

/// The time as a grid of letters, the words lit; `false` if the language
/// has no grid that tells it (the text is drawn then).
#[allow(
    clippy::too_many_arguments,
    reason = "its inputs are separate things; a struct of them would serve this call alone"
)]
fn draw_grid(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    font: &FontHandle<'_>,
    look: &widgets::WidgetLook,
    hour: u8,
    minute: u8,
    past: Option<u8>,
) -> bool {
    let next = chosen_name(env, ID.as_str(), "quarters") == "next";
    let (lead, rest) = sentence(env.i18n(), hour, minute, next);
    let rows = grid(env.i18n());
    let Some(cells) = lit(&rows, &format!("{lead} {rest}")) else {
        return false;
    };
    let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
    if columns == 0 {
        return false;
    }
    let area = look.area;
    // Dots at the corners take half a cell round the grid.
    let margins = if past.is_some() { 1.0 } else { 0.0 };
    let cell_w = f32::from(area.width) / (columns as f32 + margins);
    let cell_h = f32::from(area.height) / (rows.len() as f32 + margins);
    // Cells about as high as wide.
    let cell_w = cell_w.min(cell_h * 1.5);
    let cell_h = cell_h.min(cell_w * 1.25);
    let width = cell_w * (columns as f32 + margins);
    let height = cell_h * (rows.len() as f32 + margins);
    let align = widgets::align(env, ID.as_str());
    let left = align.x(area, width) + cell_w * margins / 2.0;
    let top = f32::from(area.y) + (f32::from(area.height) - height) / 2.0 + cell_h * margins / 2.0;
    let size = cell_w.min(cell_h) * 0.6;
    // Where the font has no letters that small, the text instead.
    if font.measure(size, "W") > cell_w * 0.95 || super::cap_height(font, size) > cell_h * 0.8 {
        return false;
    }
    let bolder = (size / 20.0) as u16;
    let one_bit = env.caps().black_and_white;
    let unlit = if one_bit {
        look.ink
    } else {
        // A quarter of the way from the paper to the ink.
        let luma = (u16::from(look.ink.luma()) + 3 * u16::from(look.paper.luma())) / 4;
        Color::gray(luma as u8)
    };
    let mut buffer = [0u8; 4];
    for (r, row) in rows.iter().enumerate() {
        for (c, letter) in row.iter().enumerate() {
            let on = cells.contains(&(r, c));
            let x = left + cell_w * (c as f32 + 0.5);
            let y = top + cell_h * (r as f32 + 0.5);
            let text = letter.encode_utf8(&mut buffer);
            let width = font.measure(size, text);
            let baseline = (y + super::cap_height(font, size) / 2.0) as i32;
            let (color, extra) = if on { (look.ink, bolder) } else { (unlit, 0) };
            font.draw_bolder(canvas, size, x - width / 2.0, baseline, text, color, extra);
            if !on && one_bit {
                // Where the panel has no grays, half the letter's dots.
                let cell = Rect::new(
                    (x - cell_w / 2.0).max(0.0) as u16,
                    (y - cell_h / 2.0).max(0.0) as u16,
                    cell_w as u16 + 1,
                    cell_h as u16 + 1,
                );
                halve(canvas, cell, look.paper);
            }
        }
    }
    let sentence = format!("{lead} {rest}");
    let at = Rect::new(left as u16, top as u16, width as u16, height as u16);
    canvas.describe(paperpaw_sdk::semantics::Role::Text, &sentence, at);
    if let Some(past) = past {
        let dot = (cell_w.min(cell_h) * 0.12).max(2.0) as u32;
        let (l, t) = (left - cell_w * 0.25, top - cell_h * 0.25);
        let (r, b) = (
            left + cell_w * (columns as f32 + 0.25),
            top + cell_h * (rows.len() as f32 + 0.25),
        );
        let corners = [(l, t), (r, t), (r, b), (l, b)];
        for (n, (x, y)) in corners.into_iter().enumerate() {
            let center = Point::new(x as i32, y as i32);
            if n < usize::from(past) {
                canvas.fill_circle(center, dot, look.ink);
            } else if !one_bit {
                canvas.fill_circle(center, dot, unlit);
            }
        }
    }
    true
}

/// Paints every other dot of `area` in `paper`, so that what is drawn
/// there looks half as dark: gray where the panel has none.
fn halve(canvas: &mut Canvas<'_>, area: Rect, paper: Color) {
    let (x0, y0) = (i32::from(area.x), i32::from(area.y));
    canvas.draw_mask(
        x0,
        y0,
        u32::from(area.width),
        u32::from(area.height),
        paper,
        |x, y| {
            if (x as i32 + x0 + y as i32 + y0) % 2 == 1 {
                255
            } else {
                0
            }
        },
    );
}

/// Draws widget `id` in each of a few small places, with its default
/// settings, and says whether any dot outside its place changed.
#[cfg(test)]
pub(super) fn drawn_outside(id: &str) -> Option<Rect> {
    use paperpaw_graphics::Framebuffer;
    use paperpaw_sdk::PixelFormat;

    let h = crate::harness::Harness::new(paperpaw_sdk::MemoryStorage::new());
    let env = h.env();
    let display = crate::harness::DEVICE.display;
    let mut fb = Framebuffer::new(display.width, display.height, PixelFormat::Gray4).unwrap();
    let places = [
        Rect::new(100, 100, 50, 50),
        Rect::new(100, 100, 80, 50),
        Rect::new(100, 100, 90, 60),
        Rect::new(100, 100, 60, 200),
        Rect::new(100, 100, 200, 60),
        Rect::new(100, 100, 300, 120),
    ];
    for area in places {
        let all = fb.bounds();
        fb.canvas().fill(all, Color::WHITE);
        let mut widget = env.widgets().make(id, &env).unwrap();
        widget.place(&env, area);
        widget.draw(&mut fb.canvas(), &env, area);
        let view = fb.view();
        for y in 0..display.height {
            for x in 0..display.width {
                let inside = x >= area.x
                    && x < area.x + area.width
                    && y >= area.y
                    && y < area.y + area.height;
                if !inside && view.pixel(x, y) != Color::WHITE {
                    return Some(area);
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_i18n::Catalog;

    #[test]
    fn words_are_never_split_and_nothing_is_drawn_outside() {
        let h = crate::harness::Harness::new(paperpaw_sdk::MemoryStorage::new());
        let env = h.env();
        let font = paperpaw_sdk::ui::UiStyle::new(&env).font(&env).unwrap();
        let text = "It is quarter to eleven";
        for width in [30.0, 60.0, 90.0, 140.0, 400.0] {
            for size in [20.0, 28.0, 36.0] {
                let lines = balanced(&font, size, text, width);
                assert_eq!(lines.join(" "), text, "{width} at {size}");
            }
        }
        // In a narrow, high place: as large as fits without a word cut.
        let room = Rect::new(0, 0, 70, 400);
        let size = largest(&font, text, room, 10.0, 400.0);
        assert!(font.measure(size, "quarter") <= 70.0, "{size}");
        assert_eq!(drawn_outside(ID.as_str()), None);
    }

    fn translator(language: &str) -> Translator {
        let mut t = Translator::new();
        t.add(Catalog::parse(include_str!("../../i18n/en.ini")).unwrap());
        t.add(Catalog::parse(include_str!("../../i18n/de.ini")).unwrap());
        t.set_language(language);
        t
    }

    fn says(t: &Translator, hour: u8, minute: u8, next: bool) -> String {
        let (h, m, _) = told(hour, minute, false);
        let (lead, rest) = sentence(t, h, m, next);
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
            let rows = grid(&t);
            assert!(rows.len() >= 8, "{language}");
            for next in [false, true] {
                for hour in 0..24 {
                    for minute in (0..60).step_by(5) {
                        let (lead, rest) = sentence(&t, hour, minute, next);
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
        let rows = grid(&en);
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
mod minutes_tests;
