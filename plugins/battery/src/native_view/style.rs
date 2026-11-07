//! The stock UI's Battery geometry, read without an Env or UI adapter.

use alloc::string::String;
use paperpaw_sdk::{
    Color,
    native::{
        Error, Host,
        fonts::Font,
        settings::{self, SettingValue},
        snapshot::Snapshot,
    },
};

const UI_SIZE: &str = "core.ui-size";
const UI_ROW: &str = "core.ui-row";
const UI_TITLE: &str = "core.ui-title";
const UI_MARGIN: &str = "core.ui-margin";
const UI_LINES: &str = "core.ui-lines";
const UI_HEADER: &str = "core.ui-header";
const UI_SEPARATORS: &str = "core.ui-separators";
const UI_CONTRAST: &str = "core.ui-contrast";
const UI_PALETTE: &str = "core.ui-palette";

pub(super) fn boolean(host: &Host<'_>, key: &str) -> Result<bool, Error> {
    let mut bytes = [0; 32];
    Ok(matches!(
        settings::read_setting(host, key, &mut bytes)?,
        SettingValue::Bool(true)
    ))
}

fn integer(host: &Host<'_>, key: &str) -> Result<i64, Error> {
    let mut bytes = [0; 32];
    Ok(match settings::read_setting(host, key, &mut bytes)? {
        SettingValue::Int(value) => value,
        _ => 0,
    })
}

pub(super) fn setting_text(host: &Host<'_>, key: &str) -> Result<String, Error> {
    let mut bytes = [0; 512];
    Ok(match settings::read_setting(host, key, &mut bytes)? {
        SettingValue::Text(value) => String::from(value),
        _ => String::new(),
    })
}

pub(super) struct Style {
    pub text: f32,
    pub small: f32,
    pub title: f32,
    pub head: f32,
    pub row: u16,
    pub header: u16,
    pub heading: u16,
    pub margin: u16,
    pub gap: u16,
    pub line: u16,
    pub dark: bool,
    pub plain: bool,
    pub separators: String,
    pub ink: Color,
    pub paper: Color,
    pub muted: Color,
    pub note: Color,
    pub disabled: Color,
    pub rule: Color,
}

impl Style {
    pub(super) fn read(
        host: &Host<'_>,
        display: &Snapshot,
        font: &Font<'_>,
    ) -> Result<Self, Error> {
        let dpi = u32::from(display.display_dpi());
        let tenth = |value: u32| {
            let pixels = (value * dpi + 127) / 254;
            if pixels == 0 && value > 0 { 1 } else { pixels }
        };
        let pt = |points: f32| points * dpi as f32 / 72.0;
        let base = integer(host, UI_SIZE)?.clamp(6, 40) as f32;
        let text = pt(base);
        let small = pt(base * 0.75);
        let line_height = font.read_line_metrics(text)?.line_height;
        let rows = integer(host, UI_ROW)?.clamp(120, 300) as f32 / 100.0;
        let control_row = line_height * rows;
        let least_line = font.read_line_metrics(small * 0.7)?.line_height;
        let row = if 2.0 * least_line > control_row {
            if base < 10.0 {
                control_row.max(2.0 * least_line + tenth(6) as f32)
            } else {
                control_row.max(2.0 * font.read_line_metrics(small)?.line_height + tenth(20) as f32)
            }
        } else {
            control_row
        };
        let finger = if display.has_touch() { tenth(70) } else { 0 };
        let lift = finger.saturating_sub(row as u32);
        let header = setting_text(host, UI_HEADER)?;
        let strong = setting_text(host, UI_CONTRAST)? == "core.contrast.strong";
        let palette = setting_text(host, UI_PALETTE)?;
        let color = |name, default| palette_color(&palette, name, default);
        let faint = |name, default| {
            if strong {
                color("text", Color::BLACK)
            } else {
                color(name, default)
            }
        };
        let gap = narrow(tenth(20));
        Ok(Self {
            text,
            small,
            title: pt(base * 1.15),
            head: pt(base * integer(host, UI_TITLE)?.clamp(100, 200) as f32 / 100.0),
            row: narrow((row as u32).max(finger)),
            header: narrow((line_height * (rows + 0.1)) as u32 + lift),
            heading: (font.read_line_metrics(small)?.line_height * 1.25) as u16 + gap,
            margin: narrow(tenth(integer(host, UI_MARGIN)?.clamp(0, 15) as u32 * 10)),
            gap,
            line: narrow(tenth(integer(host, UI_LINES)?.clamp(1, 10) as u32).max(1)),
            dark: header == "core.header.dark",
            plain: header == "core.header.plain",
            separators: setting_text(host, UI_SEPARATORS)?,
            ink: color("text", Color::BLACK),
            paper: color("paper", Color::WHITE),
            muted: faint("muted", Color::gray(90)),
            note: faint("note", Color::gray(60)),
            disabled: color("disabled", Color::gray(150)),
            rule: color("rule", Color::gray(150)),
        })
    }
}

fn narrow(value: u32) -> u16 {
    u16::try_from(value).unwrap_or(u16::MAX)
}

fn palette_color(palette: &str, name: &str, default: Color) -> Color {
    let mut color = default;
    for pair in palette.split_ascii_whitespace() {
        if let Some((key, value)) = pair.split_once('=')
            && key == name
            && let Some(value) = parse_color(value)
        {
            color = value;
        }
    }
    color
}

fn parse_color(value: &str) -> Option<Color> {
    if let Some(hex) = value.strip_prefix('#') {
        if hex.len() != 6 || !hex.is_ascii() {
            return None;
        }
        Some(Color::rgb(
            u8::from_str_radix(&hex[..2], 16).ok()?,
            u8::from_str_radix(&hex[2..4], 16).ok()?,
            u8::from_str_radix(&hex[4..], 16).ok()?,
        ))
    } else if !value.is_empty()
        && value.len() <= 3
        && value.bytes().all(|byte| byte.is_ascii_digit())
    {
        value.parse().ok().map(Color::gray)
    } else {
        None
    }
}
