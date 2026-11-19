//! The quick panel: big round switches, bars for the light and the text size,
//! and the dock. What it shows is the rows of the user's panel menu: a switch
//! setting is a round switch, an action a round button, a number setting a
//! bar; the light only where the device has one (its row is then not listed).
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Point,
    native::{Error, canvas::PartRole, fonts::IconGlyph},
};
use paperpaw_ui_kit::{icons, widget::Align};

use crate::{
    data::{Kind, Number, Row},
    dock,
    look::{B, Ui},
    model::{self, Target},
};

/// The action of the dock's panel icon.
pub const PANEL: &str = "core.quick-panel";

/// The most round switches (two rows of four) and bars.
pub const MOST_TOGGLES: usize = 8;
pub const MOST_BARS: usize = 3;

/// A round switch or button.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Toggle {
    pub row: Row,
}

/// A bar for a number setting.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bar {
    pub row: Row,
    pub number: Number,
}

/// What the panel shows, made of the rows of a menu once, and kept, with its
/// layout (made when it first draws, again only where the room or the text size
/// changed).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Model {
    pub toggles: Vec<Toggle>,
    pub bars: Vec<Bar>,
    layout: Option<Layout>,
}

impl Model {
    pub fn new(toggles: Vec<Toggle>, bars: Vec<Bar>) -> Self {
        Self {
            toggles,
            bars,
            layout: None,
        }
    }
}

/// What the layout was made for: the room, the text size and the small one.
type Key = ([i32; 4], u32, u32);

/// Where everything is, measured once per open: not again for each frame
/// (a frame is drawn a band at a time, and again at each focus move).
#[derive(Clone, Debug, PartialEq)]
struct Layout {
    key: Key,
    columns: usize,
    cell_w: i32,
    apart: i32,
    size: f32,
    radius: i32,
    row_h: i32,
    label: i32,
    area: B,
    bars: Vec<B>,
    /// The icons of the shown switches and of the bars, looked up once.
    toggle_icons: Vec<Option<IconGlyph>>,
    bar_icons: Vec<Option<IconGlyph>>,
}

/// The name of the icon a bar shows: the row's, else one by the setting.
fn bar_icon(row: &Row) -> &str {
    if row.icon.is_empty() {
        match row.item.as_str() {
            "core.frontlight" => "sun",
            _ => "text-size",
        }
    } else {
        &row.icon
    }
}

/// The text a bar shows at its right: the value, a percent where the range is
/// 0 to 100.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn value_text(number: &Number) -> Result<String, Error> {
    if number.min == 0 && number.max == 100 {
        paperpaw_ui_kit::text::format(format_args!("{} %", number.value))
    } else {
        paperpaw_ui_kit::text::format(format_args!("{}", number.value))
    }
}

/// A bar with an icon and a value: a thick pill filled from the left.
fn slider(ui: &mut Ui<'_, '_>, b: B, bar: &Bar, glyph: Option<IconGlyph>) -> Result<(), Error> {
    let (ink, paper) = (ui.ink, ui.paper);
    let radius = b.h / 2;
    let focused = ui.focused();
    ui.frame(b, radius, ui.stroke, ink)?;
    if focused {
        ui.ring(b, radius)?;
    }
    let inner = b.inset(ui.stroke + 3);
    let share = i32::from(model::permille(
        bar.number.value,
        bar.number.min,
        bar.number.max,
    ));
    let filled = inner.left((inner.w * share / 1000).max(inner.h));
    ui.plate(filled, inner.h / 2, ink)?;
    ui.icon_glyph(glyph, inner.h as f32 * 0.62, filled.left(inner.h), paper)?;
    let value = value_text(&bar.number)?;
    let size = (ui.u * 1.1).min(inner.h as f32 * 0.5);
    let text_tone = if inner.w * share / 1000 > inner.w - inner.h {
        paper
    } else {
        ink
    };
    ui.bold(
        B::new(inner.x, inner.y, inner.w - inner.h / 2, inner.h),
        Align::Right,
        size,
        &value,
        text_tone,
    )?;
    ui.zone(Target::Bar(0), b, PartRole::Slider, &bar.row.title)
}

/// The name of the icon a switch shows.
fn toggle_icon(row: &Row) -> &str {
    if row.icon.is_empty() {
        "adjustments-horizontal"
    } else {
        &row.icon
    }
}

/// Measures the panel in `area` for `model`.
fn measure(ui: &Ui<'_, '_>, model: &Model, area: B, key: Key) -> Result<Layout, Error> {
    let apart = ui.gap.max(2 * ui.stroke + 10);
    let toggles = &model.toggles[..model.toggles.len().min(MOST_TOGGLES)];
    let bars = &model.bars[..model.bars.len().min(MOST_BARS)];
    // Four to a row, three where a name would not fit a column.
    let mut columns = 4;
    let mut widest = 0.0f32;
    for toggle in toggles {
        widest = widest.max(ui.font.measure_text(ui.small, &toggle.row.title)?);
    }
    if widest > ((area.w - 3 * apart / 2) / 4 - 10) as f32 {
        columns = 3;
    }
    let toggles = &toggles[..toggles.len().min(columns * 2)];
    let rows = toggles.len().div_ceil(columns) as i32;
    let cell_w = (area.w - (columns as i32 - 1) * apart / 2) / columns as i32;
    // One size for every name.
    let mut names: Vec<&str> = Vec::new();
    for toggle in toggles {
        paperpaw_ui_kit::mem::push(&mut names, toggle.row.title.as_str())?;
    }
    let size = ui.common(&names, ui.small, cell_w - 8)?;
    let label = (size * 1.9) as i32;
    // The bars at least `bar_min` each; the switches take the rest.
    let bar_min = ((ui.u * 1.6) as i32 + 2 * ui.stroke + 8).max(64);
    let bars_h = bars.len() as i32 * (bar_min + apart);
    let room = (area.h - bars_h) / rows.max(1) - label - 12 - apart / 2;
    let radius = (cell_w * 2 / 5).min(room / 2).max(24);
    let row_h = 2 * radius + label + 12 + apart / 2;
    // The bars share the room below the switches.
    let bar_area = B::new(
        area.x,
        area.y + rows * row_h,
        area.w,
        area.h - rows * row_h,
    );
    let count = bars.len() as i32;
    let mut boxes = Vec::new();
    for index in 0..bars.len() {
        let b = bar_area.row(index as i32, count.max(1), apart);
        paperpaw_ui_kit::mem::push(&mut boxes, b.centred(b.w, b.h.min(bar_min * 3 / 2)))?;
    }
    let mut toggle_icons = Vec::new();
    for toggle in toggles {
        let glyph = icons::glyph(ui.host, toggle_icon(&toggle.row))?;
        paperpaw_ui_kit::mem::push(&mut toggle_icons, glyph)?;
    }
    let mut bar_icons = Vec::new();
    for bar in bars {
        paperpaw_ui_kit::mem::push(&mut bar_icons, icons::glyph(ui.host, bar_icon(&bar.row))?)?;
    }
    Ok(Layout {
        key,
        columns,
        cell_w,
        apart,
        size,
        radius,
        row_h,
        label,
        area,
        bars: boxes,
        toggle_icons,
        bar_icons,
    })
}

/// Draws the panel into `ui`.
///
/// # Errors
/// [`Error::NoMemory`] and the host's refusals.
pub fn draw(
    ui: &mut Ui<'_, '_>,
    model: &mut Model,
    title: &str,
    dock_rows: &[Row],
) -> Result<(), Error> {
    let top = ui.header(title, "")?;
    let rest = ui
        .screen
        .above(dock::height(ui) + dock::edge(ui), ui.gap / 2);
    let area = B::new(
        rest.x + ui.pad,
        top.y + ui.pad / 2,
        rest.w - 2 * ui.pad,
        rest.y + rest.h - top.y - ui.pad / 2,
    );
    let key = (
        [area.x, area.y, area.w, area.h],
        ui.u.to_bits(),
        ui.small.to_bits(),
    );
    if model.layout.as_ref().map(|l| l.key) != Some(key) {
        model.layout = Some(measure(ui, model, area, key)?);
    }
    let Some(layout) = model.layout.as_ref() else {
        return Err(Error::State);
    };
    let ink = ui.ink;
    let Layout {
        columns,
        cell_w,
        apart,
        size,
        radius,
        row_h,
        label,
        ..
    } = *layout;
    for (index, (toggle, glyph)) in model
        .toggles
        .iter()
        .zip(&layout.toggle_icons)
        .enumerate()
    {
        let (row_at, col_at) = ((index / columns) as i32, (index % columns) as i32);
        let cell = B::new(
            area.x + col_at * (cell_w + apart / 2),
            area.y + row_at * row_h,
            cell_w,
            row_h,
        );
        let centre = Point {
            x: cell.x + cell.w / 2,
            y: cell.y + radius + 4,
        };
        let around = B::new(centre.x - radius, centre.y - radius, 2 * radius, 2 * radius);
        let focused = ui.focused();
        ui.disc_glyph(centre, radius, *glyph, toggle.row.on)?;
        if focused {
            ui.ring(around, radius)?;
        }
        let under = B::new(cell.x, cell.y + 2 * radius + 12, cell.w, label);
        ui.text(under, Align::Center, size, &toggle.row.title, ink)?;
        ui.zone(
            Target::Toggle(index),
            B::new(cell.x, cell.y, cell.w, 2 * radius + label + 12),
            if toggle.row.kind == Kind::Setting {
                PartRole::Switch
            } else {
                PartRole::Button
            },
            &toggle.row.title,
        )?;
    }
    for (index, ((bar, b), glyph)) in model
        .bars
        .iter()
        .zip(&layout.bars)
        .zip(&layout.bar_icons)
        .enumerate()
    {
        let before = ui.zones.len();
        slider(ui, *b, bar, *glyph)?;
        if let Some(zone) = ui.zones.get_mut(before) {
            zone.target = Target::Bar(index);
        }
    }
    dock::draw(ui, dock_rows, PANEL)?;
    Ok(())
}
