//! The world clock: the time in up to three other places (of the time
//! zones PaperPaw knows), each with its name and whether it is already
//! tomorrow or still yesterday there ("Tokyo, tomorrow 18:41"); as rows
//! one below the other, or side by side in a wide row.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::time::{self, LocalTime, TIME_ZONES, TimeZone};
use paperpaw_sdk::ui::UiStyle;
use paperpaw_sdk::widgets::{self, Align, Drawn, Frame, Padding};
use paperpaw_sdk::{Arg, Canvas, Color, Env, FontHandle, Id, Rect, RegisterError, Registrar};

use paperpaw_sdk::SettingSpec;

use super::{chosen_name, dates, frame_height, line, ui};

const ID: Id = Id::new_static("widgets.world-clock");

/// No place.
const NONE: &str = "widgets.world-clock.none";
/// The settings of the places.
const PLACES: [&str; 3] = ["place-1", "place-2", "place-3"];
/// The values of the first place's setting: the ids of
/// [`TIME_ZONES`], written out to stay in the program (a test keeps
/// them the same).
const ZONES: [&str; 22] = [
    "core.tz.utc",
    "core.tz.honolulu",
    "core.tz.anchorage",
    "core.tz.los-angeles",
    "core.tz.phoenix",
    "core.tz.denver",
    "core.tz.chicago",
    "core.tz.new-york",
    "core.tz.sao-paulo",
    "core.tz.london",
    "core.tz.lisbon",
    "core.tz.berlin",
    "core.tz.helsinki",
    "core.tz.istanbul",
    "core.tz.moscow",
    "core.tz.dubai",
    "core.tz.india",
    "core.tz.bangkok",
    "core.tz.china",
    "core.tz.tokyo",
    "core.tz.sydney",
    "core.tz.auckland",
];
/// The values of the other places' settings: none, or one of [`ZONES`].
const ZONES_OR_NONE: [&str; 23] = {
    let mut values = [NONE; 23];
    let mut n = 0;
    while n < ZONES.len() {
        values[n + 1] = ZONES[n];
        n += 1;
    }
    values
};
/// The values of the layout setting: as fits, rows, side by side.
const LAYOUTS: [&str; 3] = [
    "widgets.layout.auto",
    "widgets.layout.rows",
    "widgets.layout.columns",
];

/// Its settings, kept in the program.
static SETTINGS: [SettingSpec; 15] = [
    widgets::align_spec("widgets.world-clock.align", ID, Align::Left),
    widgets::frame_spec("widgets.world-clock.frame", ID, Frame::None),
    widgets::padding_spec("widgets.world-clock.padding", ID, Padding::Auto),
    widgets::paper_spec("widgets.world-clock.paper", ID),
    widgets::ink_spec("widgets.world-clock.ink", ID),
    widgets::corners_spec("widgets.world-clock.corners", ID),
    widgets::frame_line_spec("widgets.world-clock.frame-line", ID),
    widgets::shadow_spec("widgets.world-clock.drop-shadow", ID),
    SettingSpec::choice(
        "widgets.world-clock.place-1",
        "widgets.setting.place-1",
        &ZONES,
        "core.tz.new-york",
        ID,
    )
    .description("widgets.world-clock.place.help"),
    SettingSpec::choice(
        "widgets.world-clock.place-2",
        "widgets.setting.place-2",
        &ZONES_OR_NONE,
        "core.tz.tokyo",
        ID,
    )
    .description("widgets.world-clock.place.help"),
    SettingSpec::choice(
        "widgets.world-clock.place-3",
        "widgets.setting.place-3",
        &ZONES_OR_NONE,
        NONE,
        ID,
    )
    .description("widgets.world-clock.place.help"),
    SettingSpec::choice(
        "widgets.world-clock.layout",
        "widgets.setting.layout",
        &LAYOUTS,
        LAYOUTS[0],
        ID,
    )
    .description("widgets.world-clock.layout.help"),
    widgets::part_spec(
        "widgets.world-clock.here",
        ID,
        "widgets.setting.here",
        false,
    ),
    widgets::part_spec(
        "widgets.world-clock.day",
        ID,
        "widgets.setting.day-difference",
        true,
    ),
    widgets::part_spec(
        "widgets.world-clock.offset",
        ID,
        "widgets.setting.offset",
        false,
    ),
];

pub(super) fn register(r: &mut Registrar) -> Result<(), RegisterError> {
    super::add_settable(
        r,
        {
            static SPEC: paperpaw_sdk::SettingGroupSpec =
                paperpaw_sdk::SettingGroupSpec::new(ID, "widgets.world-clock");
            &SPEC
        },
        Drawn::new(height, draw).shows(super::minute_shows),
    )?;
    r.add_settings(&SETTINGS)
}

/// A place shown: its name, its time, how many days it is ahead of here,
/// and how many minutes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Place {
    pub name: String,
    pub time: LocalTime,
    pub days: i64,
    pub minutes: i64,
}

/// The place of time zone `rule` at `unix` seconds, here being `here`
/// seconds east of UTC.
pub(super) fn place(name: &str, rule: &str, unix: i64, here: i64) -> Option<Place> {
    let zone = TimeZone::parse(rule)?;
    let offset = i64::from(zone.offset_at(unix));
    let there = LocalTime::from_seconds(unix + offset);
    let local = LocalTime::from_seconds(unix + here);
    let day = |t: &LocalTime| dates::days_from_civil(t.year, t.month, t.day);
    Some(Place {
        // The first city of "Berlin, Vienna, Zurich, Paris, Rome".
        name: String::from(name.split(',').next().unwrap_or(name).trim()),
        time: there,
        days: day(&there) - day(&local),
        minutes: (offset - here) / 60,
    })
}

/// The places chosen, here first if wished.
fn places(env: &Env<'_>) -> Vec<Place> {
    let Some(unix) = time::unix_seconds(env) else {
        return Vec::new();
    };
    let here = time::local_seconds(env, unix) - unix;
    let mut places = Vec::new();
    if widgets::shows(env, ID.as_str(), "here")
        && let Some(now) = time::local_time(env)
    {
        places.push(Place {
            name: String::from(env.i18n().tr("widgets.world-clock.here")),
            time: now,
            days: 0,
            minutes: 0,
        });
    }
    for key in PLACES {
        let id = env.settings().text(&format!("{ID}.{key}"));
        if let Some((id, rule)) = TIME_ZONES.iter().find(|(z, _)| *z == id)
            && let Some(place) = place(env.i18n().tr(id), rule, unix, here)
        {
            places.push(place);
        }
    }
    places
}

/// "tomorrow", "yesterday" and "+7 h", as the settings want them.
fn note(env: &Env<'_>, place: &Place) -> String {
    let mut parts: Vec<String> = Vec::new();
    if widgets::shows(env, ID.as_str(), "day") {
        let key = match place.days {
            1.. => Some("widgets.world-clock.tomorrow"),
            ..=-1 => Some("widgets.world-clock.yesterday"),
            0 => None,
        };
        if let Some(key) = key {
            parts.push(String::from(env.i18n().tr(key)));
        }
    }
    if widgets::shows(env, ID.as_str(), "offset")
        && place.name != env.i18n().tr("widgets.world-clock.here")
    {
        parts.push(offset_text(env, place.minutes));
    }
    parts.join(" · ")
}

/// A difference of `minutes` in hours: "+7 h", "−5:30 h", "±0 h".
pub(super) fn offset_text(env: &Env<'_>, minutes: i64) -> String {
    let sign = match minutes {
        1.. => "+",
        0 => "±",
        _ => "−",
    };
    let (h, m) = (minutes.abs() / 60, minutes.abs() % 60);
    let number = if m == 0 {
        format!("{sign}{h}")
    } else {
        format!("{sign}{h}:{m:02}")
    };
    env.i18n().format(
        "widgets.world-clock.offset",
        &[("offset", Arg::Str(&env.i18n().localize_digits(&number)))],
    )
}

fn height(env: &Env<'_>, _: u16) -> u16 {
    let (style, _) = ui(env);
    let count = places(env).len().max(1) as u16;
    let row =
        widgets::line_height(env, style.title() * 1.3) + widgets::line_height(env, style.small());
    row * count + style.gap() * (count - 1) + frame_height(env, ID.as_str())
}

#[expect(
    clippy::too_many_lines,
    reason = "longer than the limit from before it was set; split it when it is next changed"
)]
fn draw(canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
    let look = widgets::draw_frame(canvas, env, ID.as_str(), area);
    let (style, font) = ui(env);
    if time::unix_seconds(env).is_none() {
        super::draw_no_time(canvas, env, &look);
        return;
    }
    let Some(font) = font else {
        return;
    };
    let mut places = places(env);
    if places.is_empty() {
        widgets::draw_message(
            canvas,
            env,
            &look,
            env.i18n().tr("widgets.world-clock.empty"),
        );
        return;
    }
    let area = look.area;
    let gap = style.gap();
    let columns = match chosen_name(env, ID.as_str(), "layout") {
        "rows" => false,
        "columns" => true,
        _ => area.width / places.len() as u16 > area.height * 9 / 10 && places.len() > 1,
    };
    // Rows with the name beside the time where every name has room
    // there, else with the name above it.
    let stacked_in = |places: &[Place], row_h: u16| {
        !columns
            && places.iter().any(|place| {
                let clock = super::clock_text(env, &place.time);
                let size = time_size(&font, &clock, f32::from(row_h), f32::from(area.width))
                    .max(style.small());
                let name = name_size(&style, row_h);
                font.measure(size, &clock) + style.gap() as f32 + font.measure(name, &place.name)
                    > f32::from(area.width)
            })
    };
    // Where not all have room, those first chosen: each needs a line of
    // the small size, two where its name goes above its time.
    let small = line(&font, style.small());
    let room = if columns { area.width } else { area.height };
    let mut n = places.len();
    while n > 1 {
        let each = super::share(room, gap, n as u16);
        let least = if stacked_in(&places[..n], each) {
            2.0 * small
        } else {
            small
        };
        if f32::from(each) >= least {
            break;
        }
        n -= 1;
    }
    places.truncate(n);
    let n = n as u16;
    let row_h = super::share(area.height, gap, n);
    let stacked = stacked_in(&places, row_h);
    // Places side by side or stacked alike: the times as large as the
    // widest allows, room for the name and the notes where any has one;
    // where the height has no room for them, the notes are left out, then
    // the names.
    let common = {
        let any_note = !stacked && places.iter().any(|p| !note(env, p).is_empty());
        let (w, h) = if columns {
            (super::share(area.width, gap, n), area.height)
        } else {
            (area.width, row_h)
        };
        let mut notes: f32 = if any_note { 2.0 } else { 1.0 };
        while notes > 0.0 && small * notes + small > f32::from(h) {
            notes -= 1.0;
        }
        let room = (f32::from(h) - small * notes).max(small);
        let clocks: Vec<String> = places
            .iter()
            .map(|p| super::clock_text(env, &p.time))
            .collect();
        let fits = |size: f32| {
            clocks
                .iter()
                .all(|clock| font.measure(size, clock) <= f32::from(w) * 0.96)
        };
        let size = widgets::largest_size((room * 0.8).max(style.small()), style.small(), fits)
            .unwrap_or(style.small());
        (size, notes)
    };
    // Too flat for the name above the time: beside it, where it has room
    // there.
    let beside = columns
        && common.1 == 0.0
        && places.iter().all(|place| {
            let clock = super::clock_text(env, &place.time);
            let w = f32::from(super::share(area.width, gap, n));
            let time = time_size(&font, &clock, f32::from(area.height), w).max(style.small());
            let name = name_size(&style, area.height);
            font.measure(time, &clock) + style.gap() as f32 + font.measure(name, &place.name) <= w
        });
    for (i, place) in places.iter().enumerate() {
        let i = i as u16;
        let cell = if columns {
            let w = super::share(area.width, gap, n);
            Rect::new(area.x + i * (w + gap), area.y, w, area.height)
        } else {
            Rect::new(area.x, area.y + i * (row_h + gap), area.width, row_h)
        };
        if beside {
            draw_row(canvas, env, &font, &look, cell, place);
        } else if columns || stacked {
            draw_column(canvas, env, &font, &look, cell, place, stacked, common);
        } else {
            draw_row(canvas, env, &font, &look, cell, place);
        }
        // A fine line between places.
        if i > 0 && look.paper.luma() >= 128 {
            let thick = style.line();
            let back = (gap / 2 + thick / 2).min(gap);
            let between = if columns {
                Rect::new(cell.x - back, cell.y, thick.min(gap), cell.height)
            } else {
                Rect::new(cell.x, cell.y - back, cell.width, thick.min(gap))
            };
            draw_between(canvas, &style, between, look.ink);
        }
    }
}

/// The fine line between places: light gray, or on a panel of black and
/// white alone every third dot of it, counted from its start, so that it
/// looks alike wherever it falls (a shaded gray there would be dotted in
/// one place and solid in another).
fn draw_between(canvas: &mut Canvas<'_>, style: &UiStyle, area: Rect, ink: Color) {
    if !style.black_and_white() {
        canvas.fill_shaded(area, Color::gray(200));
        return;
    }
    let along_x = area.width >= area.height;
    canvas.draw_mask(
        i32::from(area.x),
        i32::from(area.y),
        u32::from(area.width),
        u32::from(area.height),
        ink,
        |x, y| {
            let at = if along_x { x } else { y };
            if at % 3 == 0 { 255 } else { 0 }
        },
    );
}

/// The size of a place's name beside its time in a row `height` high.
fn name_size(style: &UiStyle, height: u16) -> f32 {
    style.text().min(f32::from(height) * 0.4).max(style.small())
}

/// A place as a row: its name and note on the left, the time large on
/// the right.
fn draw_row(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    font: &FontHandle<'_>,
    look: &widgets::WidgetLook,
    cell: Rect,
    place: &Place,
) {
    let (style, _) = ui(env);
    let clock = super::clock_text(env, &place.time);
    let note = note(env, place);
    let width = f32::from(cell.width);
    let size = time_size(font, &clock, f32::from(cell.height), width).max(style.small());
    let time_width = font.measure(size, &clock);
    let big = line(font, size).min(f32::from(cell.height));
    let time_row = Rect::new(
        cell.x,
        cell.y + ((f32::from(cell.height) - big) / 2.0) as u16,
        cell.width,
        big as u16,
    );
    let flipped = widgets::align(env, ID.as_str()) == Align::Right;
    let (time_align, name_x) = if flipped {
        (
            Align::Left,
            cell.x + (time_width + style.gap() as f32) as u16,
        )
    } else {
        (Align::Right, cell.x)
    };
    super::label(canvas, font, size, time_align, time_row, &clock, look.ink);
    // The name and its note, together in the middle of the row.
    let name_room = Rect::new(
        name_x,
        cell.y,
        (width - time_width - style.gap() as f32).max(0.0) as u16,
        cell.height,
    );
    let name_size = name_size(&style, cell.height);
    let name_line = line(font, name_size);
    let note_line = if note.is_empty() {
        0.0
    } else {
        line(font, style.small())
    };
    let two = name_line + note_line <= f32::from(cell.height);
    let name_align = if flipped { Align::Right } else { Align::Left };
    if two {
        let top = f32::from(cell.y) + (f32::from(cell.height) - name_line - note_line) / 2.0;
        let row = Rect::new(name_room.x, top as u16, name_room.width, name_line as u16);
        super::label(
            canvas,
            font,
            name_size,
            name_align,
            row,
            &place.name,
            look.ink,
        );
        if !note.is_empty() {
            let row = Rect::new(
                name_room.x,
                (top + name_line) as u16,
                name_room.width,
                note_line as u16,
            );
            super::label(
                canvas,
                font,
                style.small(),
                name_align,
                row,
                &note,
                look.faint,
            );
        }
    } else {
        let text = if note.is_empty() {
            place.name.clone()
        } else {
            format!("{}, {note}", place.name)
        };
        super::label(
            canvas,
            font,
            style.small(),
            name_align,
            name_room,
            &text,
            look.ink,
        );
    }
}

/// The size of the time in a row `height` high and `width` wide: up to
/// half the width, and most of the height.
fn time_size(font: &FontHandle<'_>, clock: &str, height: f32, width: f32) -> f32 {
    let fits = |size: f32| font.measure(size, clock) <= width * 0.5;
    widgets::largest_size(height * 0.62, 1.0, fits).unwrap_or(1.0)
}

/// A place as a column: its name small above, the time large, the note
/// below (`compact`: beside the name); the time `size` large, room
/// left for `notes` lines of small text (as for the other places, so
/// that they line up).
#[allow(
    clippy::too_many_arguments,
    reason = "its inputs are separate things; a struct of them would serve this call alone"
)]
fn draw_column(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    font: &FontHandle<'_>,
    look: &widgets::WidgetLook,
    cell: Rect,
    place: &Place,
    compact: bool,
    (size, notes): (f32, f32),
) {
    let (style, _) = ui(env);
    let align = widgets::align(env, ID.as_str());
    let clock = super::clock_text(env, &place.time);
    let mut note = note(env, place);
    let mut name = place.name.clone();
    if compact && !note.is_empty() {
        // One line above: "Tokyo, tomorrow".
        name = format!("{name}, {note}");
        note = String::new();
    }
    let small = line(font, style.small());
    let room = (f32::from(cell.height) - small * notes).max(small);
    let big = line(font, size).min(room);
    let total = small * notes + big;
    let mut y = f32::from(cell.y) + (f32::from(cell.height) - total).max(0.0) / 2.0;
    let row = |y: f32, h: f32| Rect::new(cell.x, y as u16, cell.width, h as u16);
    // The name where there is room for it (see `draw`), the note too.
    if notes >= 1.0 {
        super::label(
            canvas,
            font,
            style.small(),
            align,
            row(y, small),
            &name,
            look.faint,
        );
        y += small;
    }
    super::label(canvas, font, size, align, row(y, big), &clock, look.ink);
    y += big;
    if notes >= 2.0 && !note.is_empty() {
        super::label(
            canvas,
            font,
            style.small(),
            align,
            row(y, small),
            &note,
            look.faint,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn places_neither_overlap_nor_leave_their_place() {
        use paperpaw_graphics::Framebuffer;
        use paperpaw_sdk::PixelFormat;

        assert_eq!(super::super::word_clock::drawn_outside(ID.as_str()), None);
        let h = crate::harness::Harness::new(paperpaw_sdk::MemoryStorage::new());
        let env = h.env();
        let display = crate::harness::DEVICE.display;
        let mut fb = Framebuffer::new(display.width, display.height, PixelFormat::Gray4).unwrap();
        for area in [
            Rect::new(10, 10, 74, 53),
            Rect::new(10, 10, 190, 53),
            Rect::new(10, 10, 190, 150),
            Rect::new(10, 10, 74, 300),
        ] {
            let mut described = Vec::new();
            draw(&mut fb.canvas().describing(&mut described), &env, area);
            let parts: Vec<Rect> = described.iter().map(|d| d.area).collect();
            assert!(!parts.is_empty(), "{area:?}");
            for (i, a) in parts.iter().enumerate() {
                for b in &parts[i + 1..] {
                    assert!(a.intersection(b).is_none(), "{area:?}: {a:?} over {b:?}");
                }
            }
        }
    }

    /// Wednesday, 14 October 2026, 10:41 in Berlin (summer time).
    const NOW: i64 = 1_791_967_260;
    const BERLIN: i64 = 7200;

    #[test]
    fn the_places_offered_are_the_time_zones() {
        let zones: Vec<&str> = TIME_ZONES.iter().map(|(id, _)| *id).collect();
        assert_eq!(ZONES[..], zones[..]);
        assert_eq!(ZONES_OR_NONE[0], NONE);
        assert_eq!(ZONES_OR_NONE[1..], zones[..]);
    }

    #[test]
    fn places_are_ahead_or_behind() {
        let here = LocalTime::from_seconds(NOW + BERLIN);
        assert_eq!((here.day, here.hour, here.minute), (14, 10, 41));
        let tokyo = place("Tokyo, Seoul", "JST-9", NOW, BERLIN).unwrap();
        assert_eq!(tokyo.name, "Tokyo");
        assert_eq!(
            (tokyo.time.hour, tokyo.time.minute, tokyo.days),
            (17, 41, 0)
        );
        assert_eq!(tokyo.minutes, 7 * 60);
        let new_york = place("New York", "EST5EDT,M3.2.0,M11.1.0", NOW, BERLIN).unwrap();
        assert_eq!(
            (new_york.time.hour, new_york.days, new_york.minutes),
            (4, 0, -360)
        );
        // Late in the evening, Auckland is in tomorrow already.
        let evening = NOW + 12 * 3600;
        let auckland = place("Auckland", "NZST-12NZDT,M9.5.0,M4.1.0/3", evening, BERLIN).unwrap();
        assert_eq!((auckland.time.day, auckland.days), (15, 1));
        // Early in the morning, Honolulu is still in yesterday.
        let morning = NOW - 9 * 3600;
        let honolulu = place("Honolulu", "HST10", morning, BERLIN).unwrap();
        assert_eq!(honolulu.days, -1);
        let india = place("India", "IST-5:30", NOW, BERLIN).unwrap();
        assert_eq!(india.minutes, 210);
    }
}
