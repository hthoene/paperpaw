//! Stock UI geometry read through typed native settings and font calls.

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
const UI_PALETTE: &str = "core.ui-palette";

fn integer(host: &Host<'_>, key: &str) -> Result<i64, Error> {
    let mut bytes = [0; 32];
    Ok(match settings::read_setting(host, key, &mut bytes)? {
        SettingValue::Int(value) => value,
        _ => 0,
    })
}

pub fn setting_text(host: &Host<'_>, key: &str) -> Result<String, Error> {
    let mut bytes = [0; 512];
    Ok(match settings::read_setting(host, key, &mut bytes)? {
        SettingValue::Text(value) => crate::text::copy(value)?,
        _ => String::new(),
    })
}

#[derive(Debug)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "the stock look is a set of switches"
)]
pub struct Style {
    pub corner: u32,
    pub pill: bool,
    pub ring: bool,
    pub separator_ink: Color,
    pub text: f32,
    pub small: f32,
    pub title: f32,
    pub head: f32,
    pub row: u16,
    pub header: u16,
    pub margin: u16,
    pub gap: u16,
    pub line: u16,
    pub dark: bool,
    pub plain: bool,
    pub separators: String,
    pub ink: Color,
    pub paper: Color,
    pub disabled: Color,
    pub rule: Color,
    /// Secondary text.
    pub muted: Color,
    /// Notes and footers.
    pub note: Color,
    /// Height of a section heading line.
    pub heading: u16,
}

impl Style {
    pub fn read(host: &Host<'_>, display: &Snapshot, font: &Font<'_>) -> Result<Self, Error> {
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
        let palette = setting_text(host, UI_PALETTE)?;
        let color = |name, default| palette_color(&palette, name, default);
        let strong = setting_text(host, "core.ui-contrast")? == "core.contrast.strong";
        let faint = |name, default| {
            if strong {
                color("text", Color::BLACK)
            } else {
                color(name, default)
            }
        };
        let focus = setting_text(host, "core.ui-focus")?;
        // The soft-focus shade depends on DEV display gray levels, which the
        // current snapshot facade does not expose. Never silently draw Fill.
        if focus == "core.focus.soft" {
            return Err(Error::Unsupported);
        }
        let corners = setting_text(host, "core.ui-corners")?;
        let corner = match corners.as_str() {
            "core.corners.small" => tenth(8),
            "core.corners.round" => tenth(20),
            "core.corners.soft" => tenth(35),
            _ => 0,
        };
        let gap = narrow(tenth(20));
        Ok(Self {
            corner,
            pill: matches!(corners.as_str(), "core.corners.round" | "core.corners.soft"),
            ring: focus == "core.focus.ring",
            separator_ink: if setting_text(host, UI_SEPARATORS)? == "core.separators.inset" {
                color("line-faint", Color::gray(210))
            } else {
                color("line", Color::gray(190))
            },
            text,
            small,
            title: pt(base * 1.15),
            head: pt(base * integer(host, UI_TITLE)?.clamp(100, 200) as f32 / 100.0),
            row: narrow((row as u32).max(finger)),
            header: narrow((line_height * (rows + 0.1)) as u32 + lift),
            margin: narrow(tenth(integer(host, UI_MARGIN)?.clamp(0, 15) as u32 * 10)),
            gap,
            line: narrow(tenth(integer(host, UI_LINES)?.clamp(1, 10) as u32).max(1)),
            dark: header == "core.header.dark",
            plain: header == "core.header.plain",
            separators: setting_text(host, UI_SEPARATORS)?,
            ink: color("text", Color::BLACK),
            paper: color("paper", Color::WHITE),
            disabled: color("disabled", Color::gray(150)),
            rule: color("rule", Color::gray(150)),
            muted: faint("muted", Color::gray(90)),
            note: faint("note", Color::gray(60)),
            heading: (font.read_line_metrics(small)?.line_height * 1.25) as u16 + gap,
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
