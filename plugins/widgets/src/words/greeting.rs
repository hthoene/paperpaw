//! A greeting by the time of day: "Good morning", "Good afternoon",
//! "Good evening", "Good night", with the reader's name if set ("Good
//! evening, Anna"), and the date below if wanted.

use alloc::format;
use alloc::string::String;

use paperpaw_sdk::setting::{Setting, Text};
use paperpaw_sdk::ui::{self, UiStyle};
use paperpaw_sdk::widgets::{self, Align, Drawn, FixedWidget, Frame, Padding, TextSize};
use paperpaw_sdk::{Arg, Canvas, Env, Id, Rect, RegisterError, Registrar, SettingSpec, time};

use super::frame_height;
use super::text::{self, FACES};

pub(super) const ID: Id = Id::new_static("widgets.greeting");
/// The reader's name, if greeted by it.
const NAME: Setting<Text> = Setting::new("widgets.greeting.name");
/// The line below the greeting: one of [`LINES`].
const LINE: Setting<Text> = Setting::new("widgets.greeting.line");
/// Nothing, or the date.
const LINES: [&str; 2] = ["widgets.greeting.line.none", "widgets.greeting.line.date"];
/// A sun or a moon before it, by the time of day.
const ICON: &str = "icon";
/// The font of the greeting: one of [`FACES`].
const FACE: Setting<Text> = Setting::new("widgets.greeting.face");

/// The greeting's settings, kept in the program.
static SETTINGS: [SettingSpec; 10] = [
    SettingSpec::text(
        "widgets.greeting.name",
        "widgets.greeting.setting.name",
        "",
        ID,
    )
    .description("widgets.greeting.setting.name.help"),
    SettingSpec::choice(
        "widgets.greeting.line",
        "widgets.greeting.setting.line",
        &LINES,
        LINES[1],
        ID,
    )
    .description("widgets.greeting.line.help"),
    widgets::part_spec(
        "widgets.greeting.icon",
        ID,
        "widgets.greeting.setting.icon",
        false,
    ),
    SettingSpec::choice(
        "widgets.greeting.face",
        "widgets.setting.font",
        &FACES,
        FACES[0],
        ID,
    )
    .description("widgets.setting.face-font.help"),
    widgets::size_spec("widgets.greeting.size", ID, TextSize::Fit, true),
    widgets::align_spec("widgets.greeting.align", ID, Align::Center),
    widgets::frame_spec("widgets.greeting.frame", ID, Frame::None),
    widgets::padding_spec("widgets.greeting.padding", ID, Padding::Auto),
    widgets::paper_spec("widgets.greeting.paper", ID),
    widgets::ink_spec("widgets.greeting.ink", ID),
];

pub(super) fn register(r: &mut Registrar) -> Result<(), RegisterError> {
    r.add_setting_group({
        static SPEC: paperpaw_sdk::SettingGroupSpec =
            paperpaw_sdk::SettingGroupSpec::new(ID, "widgets.greeting");
        &SPEC
    })?;
    r.add_settings(&SETTINGS)?;
    static WIDGET: [FixedWidget; 1] = [FixedWidget::drawn(
        ID,
        "widgets.greeting",
        Drawn::new(height, draw).shows(shows),
    )
    .category(widgets::categories::PERSONAL)];
    r.add_widgets(&WIDGET)
}

/// The part of the day an hour is in: morning from 5, afternoon from
/// 12, evening from 18, night from 22.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Part {
    Morning,
    Afternoon,
    Evening,
    Night,
}

impl Part {
    fn of(hour: u8) -> Self {
        match hour {
            5..=11 => Part::Morning,
            12..=17 => Part::Afternoon,
            18..=21 => Part::Evening,
            _ => Part::Night,
        }
    }

    /// The translation key of its greeting.
    fn key(self) -> &'static str {
        match self {
            Part::Morning => "widgets.greeting.morning",
            Part::Afternoon => "widgets.greeting.afternoon",
            Part::Evening => "widgets.greeting.evening",
            Part::Night => "widgets.greeting.night",
        }
    }

    /// Its icon: the sun by day, the moon by night.
    fn icon(self) -> &'static str {
        match self {
            Part::Morning | Part::Afternoon => "sun",
            Part::Evening | Part::Night => "moon",
        }
    }
}

/// The part of the day now; the afternoon where the time is not known.
fn part(env: &Env<'_>) -> Part {
    time::local_time(env).map_or(Part::Afternoon, |t| Part::of(t.hour))
}

/// "Good evening", or "Good evening, Anna" with a name.
fn greeting(env: &Env<'_>, part: Part) -> String {
    let name = env.settings().read(&NAME).trim();
    if name.is_empty() {
        String::from(env.i18n().tr(part.key()))
    } else {
        env.i18n().format(
            &format!("{}.named", part.key()),
            &[("name", Arg::Str(name))],
        )
    }
}

/// The line below it: the date, or nothing.
fn below(env: &Env<'_>) -> Option<String> {
    if env.settings().read(&LINE) == LINES[0] {
        return None;
    }
    time::date_text(env)
}

/// `date` as it fits: whole, or without its weekday ("Wednesday, 14
/// October" gives "14 October").
fn short_date(date: &str, fits: impl Fn(&str) -> bool) -> &str {
    match date.split_once(", ") {
        Some((_, short)) if !fits(date) => short,
        _ => date,
    }
}

/// What it shows: drawn again when that changes.
fn shows(env: &Env<'_>) -> String {
    format!(
        "{}|{}",
        greeting(env, part(env)),
        below(env).unwrap_or_default()
    )
}

fn height(env: &Env<'_>, _: u16) -> u16 {
    let style = UiStyle::new(env);
    let size = match widgets::size(env, ID.as_str()) {
        TextSize::Fit => style.title() * 1.6,
        size => size.text(&style),
    };
    let line = below(env).map_or(0, |_| widgets::line_height(env, style.small()));
    widgets::line_height(env, size) + line + frame_height(env, ID.as_str())
}

#[expect(
    clippy::too_many_lines,
    reason = "longer than the limit from before it was set; split it when it is next changed"
)]
fn draw(canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
    let style = UiStyle::new(env);
    let (Some(ui_font), Some(font)) =
        (style.font(env), text::font(env, env.settings().read(&FACE)))
    else {
        return;
    };
    let mut ui = ui::Ui::new(canvas, env, &style);
    let look = widgets::draw_frame(ui.canvas(), env, ID.as_str(), area);
    let area = look.area;
    let part = part(env);
    let width = f32::from(area.width);
    // The date below, where it fits whole (without its weekday if need
    // be) and leaves a line for the greeting.
    let small_line = widgets::line_height(env, style.small());
    let below = below(env).and_then(|date| {
        let fits = |text: &str| ui_font.measure(style.small(), text) <= width;
        let short = String::from(short_date(&date, fits));
        let room = area.height.saturating_sub(small_line);
        (fits(&short) && room >= text::line_height(&font, style.small()) as u16).then_some(short)
    });
    let small = below.as_ref().map_or(0, |_| small_line);
    let room = f32::from(area.height.saturating_sub(small));
    // The icon is as high as a capital, a space before the text.
    let icon_width = |icon: bool, size: f32| {
        // At the size the text is drawn at (a pack's largest at most).
        let size = font.size_for(size, "");
        if icon {
            ui::icon_width(env, part.icon(), size) + size * 0.3
        } else {
            0.0
        }
    };
    // Where it is narrow, without the name, then without the icon.
    let mut icon = widgets::shows(env, ID.as_str(), ICON) && ui::has_icons(env);
    let mut text = greeting(env, part);
    let fits = |text: &str, icon: bool| {
        font.measure(style.small(), text) + icon_width(icon, style.small()) <= width
    };
    if !fits(&text, icon) {
        text = String::from(env.i18n().tr(part.key()));
    }
    if !fits(&text, icon) {
        icon = false;
    }
    let icon_width = |size: f32| icon_width(icon, size);
    // As large as set, or as fits: on one line as large as the width
    // and the height left by the date allow.
    let (max, min) = match widgets::size(env, ID.as_str()) {
        // As large as the room holds, a line of it checked below.
        TextSize::Fit => (room.max(style.small()), style.small()),
        size => (size.text(&style), size.text(&style).min(style.small())),
    };
    let one_line = widgets::largest_size(max, min, |size| {
        font.measure(size, &text) + icon_width(size) <= width
            && text::line_height(&font, size) <= room
    });
    // Where it has no room on one line: in two, broken between words,
    // without the icon; else shortened on one.
    let (size, lines, icon) = match one_line {
        Some(size) => (size, alloc::vec![text], icon),
        None => {
            let two = widgets::largest_size(max, min, |size| {
                let (lines, too_wide) = crate::time::break_words(&font, size, &text, width);
                !too_wide
                    && lines.len() <= 2
                    && text::line_height(&font, size) * lines.len() as f32 <= room
            });
            match two {
                Some(size) => {
                    let (lines, _) = crate::time::break_words(&font, size, &text, width);
                    (size, lines, false)
                }
                // Shortened on one line; nothing where not even that
                // has room.
                None if text::line_height(&font, min) <= f32::from(area.height) => {
                    let shown = ui::fit(&font, min, &text, width);
                    if font.measure(min, &shown) > width {
                        return;
                    }
                    (min, alloc::vec![shown], false)
                }
                None => return,
            }
        }
    };
    let align = widgets::align(env, ID.as_str());
    if !icon {
        // The greeting and the date centred together by their ink.
        let mut parts = alloc::vec![crate::time::Part {
            lines: lines.iter().map(String::as_str).collect(),
            size,
            color: look.ink,
            font: None,
        }];
        if let Some(below) = &below {
            parts.push(crate::time::Part {
                lines: alloc::vec![below.as_str()],
                size: style.small(),
                color: look.faint,
                font: Some(&ui_font),
            });
        }
        let gap = style.gap() as f32 / 2.0;
        crate::time::draw_stack(ui.canvas(), &font, area, align, &parts, gap);
        return;
    }
    let icon_width = |size: f32| if icon { icon_width(size) } else { 0.0 };
    let big = text::line_height(&font, size) as u16;
    let count = lines.len() as u16;
    let top = area.y + area.height.saturating_sub(big * count + small) / 2;
    for (n, shown) in lines.iter().enumerate() {
        let total = icon_width(size) + font.measure(size, shown);
        let x = align.x(area, total);
        let row = Rect::new(area.x, top + big * n as u16, area.width, big);
        if icon {
            let icon_size = font.size_for(size, shown) * 0.9;
            ui.icon(part.icon(), icon_size, x, row, look.ink);
        }
        ui::Label::new(shown)
            .font(font)
            .px(size)
            .color(look.ink)
            .draw_from(&mut ui, x + icon_width(size), row);
    }
    if let Some(below) = below {
        let x = align.x(area, ui_font.measure(style.small(), &below));
        let row = Rect::new(area.x, top + big * count, area.width, small);
        ui::Label::new(&below)
            .px(style.small())
            .color(look.faint)
            .draw_from(&mut ui, x, row);
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn its_settings_are_those_read() {
        let ids: alloc::vec::Vec<&str> = super::SETTINGS.iter().map(|s| s.id.as_str()).collect();
        for read in [
            super::NAME.as_str(),
            super::LINE.as_str(),
            super::FACE.as_str(),
            "widgets.greeting.icon",
        ] {
            assert!(ids.contains(&read), "{read}");
        }
        for look in ["size", "align", "frame"] {
            let id = alloc::format!("{}.{look}", super::ID);
            assert!(ids.contains(&id.as_str()), "{id}");
        }
        assert!(super::SETTINGS.iter().all(|s| s.group == super::ID));
    }
    use super::*;

    #[test]
    fn greetings_follow_the_hours() {
        let parts: alloc::vec::Vec<Part> = (0..24).map(Part::of).collect();
        assert_eq!(parts[4], Part::Night);
        assert_eq!(parts[5], Part::Morning);
        assert_eq!(parts[11], Part::Morning);
        assert_eq!(parts[12], Part::Afternoon);
        assert_eq!(parts[17], Part::Afternoon);
        assert_eq!(parts[18], Part::Evening);
        assert_eq!(parts[21], Part::Evening);
        assert_eq!(parts[22], Part::Night);
        assert_eq!(parts[0], Part::Night);
        assert_eq!(Part::Evening.key(), "widgets.greeting.evening");
        assert_eq!(Part::Morning.icon(), "sun");
        assert_eq!(Part::Night.icon(), "moon");
    }

    #[test]
    fn dates_are_shortened_to_fit() {
        let date = "Wednesday, 14 October";
        assert_eq!(short_date(date, |_| true), date);
        assert_eq!(short_date(date, |_| false), "14 October");
        assert_eq!(short_date("14 October", |_| false), "14 October");
    }
}
