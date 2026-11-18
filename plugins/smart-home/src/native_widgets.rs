//! The four widgets: *Smart home value* (one entity's reading, large, with its
//! name), *Smart home group* (the entities of a room or all of them, a row
//! each), *Smart home switch* (a switch's state, a tap switches it) and *Smart
//! home room* (a room's readings in one line and its switches below).
//!
//! Each draws from the kept entities in whatever room its place has, measured
//! in pixels, not in cells: what fits, most wanted first. Nothing here asks for
//! a redraw: the plugin tells the host that the widgets changed when a state
//! did.
use crate::{
    declarations::{GROUP_TOKEN, ROOM_TOKEN, SWITCH_TOKEN, VALUE_TOKEN},
    model::{Entity, Kind, Reading, Store},
};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Rect,
    native::{Error, formatting::Arg},
};
use paperpaw_ui_kit::{
    Paint,
    geometry::line_px,
    widget::{Align, Cx, text::line as one_line},
};

/// The most rows a group lists, and the most switches a room card lists.
const GROUP_ROWS: usize = 8;
const ROOM_SWITCHES: usize = 4;
/// How long a setting naming an entity or a room is read, in bytes.
const NAME_CAP: usize = 100;

/// How `entity`'s state reads.
fn reading(cx: &mut Cx<'_>, entity: &Entity) -> Result<String, Error> {
    match entity.reading()? {
        Reading::Text(text) => Ok(text),
        Reading::Word(key) => cx.tr(key),
    }
}

/// How old the states are, in words, where the clock says.
fn age(cx: &mut Cx<'_>, store: &Store) -> Result<Option<String>, Error> {
    let (Some(now), true) = (cx.snapshot.unix_seconds(), store.synced > 0) else {
        return Ok(None);
    };
    let minutes = (now - store.synced).max(0) / 60;
    Ok(Some(if minutes < 1 {
        cx.tr("smart-home.age.now")?
    } else if minutes < 120 {
        cx.format("smart-home.age.minutes", &[Arg::Count(minutes)])?
    } else {
        cx.format(
            "smart-home.age.hours",
            &[Arg::Count((minutes / 60).min(99))],
        )?
    }))
}

/// How high the widget with `token` is in a place `width` wide.
///
/// # Errors
/// Font refusals, `NoMemory`.
pub fn height(cx: &mut Cx<'_>, token: u32, store: &Store) -> Result<u16, Error> {
    let line = line_px(&cx.font, cx.style().small)?;
    let rows = match token {
        VALUE_TOKEN | SWITCH_TOKEN => 3,
        GROUP_TOKEN => {
            let room = cx.settings.string("room", NAME_CAP)?;
            let n = store.in_room(&room).count().clamp(2, GROUP_ROWS);
            n + 2
        }
        ROOM_TOKEN => {
            let room = store.room_named(&cx.settings.string("room", NAME_CAP)?)?;
            let switches = store
                .in_room(&room)
                .filter(|e| e.kind() != Kind::Value)
                .count()
                .min(ROOM_SWITCHES);
            3 + switches
        }
        _ => return Err(Error::NotFound),
    };
    let rows = u16::try_from(rows).unwrap_or(3);
    Ok(cx
        .shape
        .frame_height()
        .saturating_add(line.saturating_mul(rows)))
}

/// Draws the widget with `token` in `area`.
///
/// # Errors
/// Font refusals, `NoMemory`.
pub fn draw(
    cx: &mut Cx<'_>,
    out: &mut Vec<Paint>,
    area: Rect,
    token: u32,
    store: &Store,
) -> Result<(), Error> {
    match token {
        VALUE_TOKEN => single(cx, out, area, store, Kind::Value),
        SWITCH_TOKEN => single(cx, out, area, store, Kind::Switch),
        GROUP_TOKEN => group(cx, out, area, store),
        ROOM_TOKEN => room(cx, out, area, store),
        _ => Err(Error::NotFound),
    }
}

/// Takes `height` pixels off the top of `rest`.
fn take(rest: &mut Rect, height: u16) -> Rect {
    let height = height.min(rest.height);
    let band = Rect::new(rest.x, rest.y, rest.width, height);
    rest.y = rest.y.saturating_add(height);
    rest.height -= height;
    band
}

/// A word in the middle of a widget that has nothing to show.
fn nothing(cx: &mut Cx<'_>, out: &mut Vec<Paint>, area: Rect, key: &str) -> Result<(), Error> {
    let look = cx.shape.paint(out, area, 256)?;
    let small = cx.style().small;
    let text = cx.tr(key)?;
    one_line(
        out,
        &cx.font,
        look.area,
        Align::Center,
        (small, small * 0.6),
        &text,
        look.faint,
    )?;
    Ok(())
}

/// One entity: its name small above, its reading large.
fn single(
    cx: &mut Cx<'_>,
    out: &mut Vec<Paint>,
    area: Rect,
    store: &Store,
    kind: Kind,
) -> Result<(), Error> {
    let named = cx.settings.string("entity", NAME_CAP)?;
    let Some(entity) = store.pick(&named, kind) else {
        return nothing(cx, out, area, "smart-home.widget.none");
    };
    let look = cx.shape.paint(out, area, 256)?;
    let mut rest = look.area;
    let small = cx.style().small;
    let line = line_px(&cx.font, small)?;
    let align = cx.settings.align()?;
    if cx.settings.shows("name")? && rest.height >= line.saturating_mul(2) {
        let band = take(&mut rest, line);
        one_line(
            out,
            &cx.font,
            band,
            align,
            (small, small * 0.6),
            entity.title(),
            look.faint,
        )?;
    }
    let ago = if kind == Kind::Value
        && cx.settings.shows("age")?
        && rest.height >= line.saturating_mul(3)
    {
        age(cx, store)?
    } else {
        None
    };
    let below = if ago.is_some() { line } else { 0 };
    let height = rest.height.saturating_sub(below);
    let main = take(&mut rest, height);
    let text = reading(cx, entity)?;
    let color = if entity.is_unknown() {
        look.faint
    } else {
        look.ink
    };
    one_line(
        out,
        &cx.font,
        main,
        align,
        (f32::from(main.height), small),
        &text,
        color,
    )?;
    if let Some(ago) = ago {
        let band = take(&mut rest, line);
        one_line(
            out,
            &cx.font,
            band,
            align,
            (small, small * 0.6),
            &ago,
            look.faint,
        )?;
    }
    Ok(())
}

/// A name at the left and its reading at the right of one band.
fn row(
    cx: &Cx<'_>,
    out: &mut Vec<Paint>,
    band: Rect,
    (name, value): (&str, &str),
    (ink, faint): (paperpaw_sdk::Color, paperpaw_sdk::Color),
) -> Result<(), Error> {
    let small = cx.style().small;
    let wide = cx.font.measure_text(small, value)?;
    let share = f32::from(band.width) * 0.55;
    let value_w = u16::try_from(wide.min(share) as u32 + 1)
        .unwrap_or(band.width)
        .min(band.width);
    let gap = band.width.min(8).min(band.width.saturating_sub(value_w));
    let name_band = Rect::new(
        band.x,
        band.y,
        band.width.saturating_sub(value_w + gap),
        band.height,
    );
    let value_band = Rect::new(
        band.x.saturating_add(band.width.saturating_sub(value_w)),
        band.y,
        value_w,
        band.height,
    );
    one_line(
        out,
        &cx.font,
        name_band,
        Align::Left,
        (small, small * 0.6),
        name,
        faint,
    )?;
    one_line(
        out,
        &cx.font,
        value_band,
        Align::Right,
        (small, small * 0.6),
        value,
        ink,
    )?;
    Ok(())
}

/// The entities of a room (or all), a row each, as many as fit.
fn group(cx: &mut Cx<'_>, out: &mut Vec<Paint>, area: Rect, store: &Store) -> Result<(), Error> {
    let room = cx.settings.string("room", NAME_CAP)?;
    let shown: Vec<&Entity> = store.in_room(&room).collect();
    if shown.is_empty() {
        return nothing(cx, out, area, "smart-home.widget.none");
    }
    let look = cx.shape.paint(out, area, 256)?;
    let mut rest = look.area;
    let small = cx.style().small;
    let line = line_px(&cx.font, small)?;
    let align = cx.settings.align()?;
    if cx.settings.shows("title")? && rest.height >= line.saturating_mul(3) {
        let title = if room.is_empty() {
            cx.tr("smart-home.title")?
        } else {
            room.clone()
        };
        let band = take(&mut rest, line);
        one_line(
            out,
            &cx.font,
            band,
            align,
            (small, small * 0.6),
            &title,
            look.ink,
        )?;
    }
    let ago = if cx.settings.shows("age")? && rest.height >= line.saturating_mul(4) {
        age(cx, store)?
    } else {
        None
    };
    let below = if ago.is_some() { line } else { 0 };
    let slots = usize::from(rest.height.saturating_sub(below) / line.max(1));
    let (listed, more) = if shown.len() > slots && slots > 1 {
        (slots - 1, shown.len() - (slots - 1))
    } else {
        (shown.len().min(slots), 0)
    };
    for entity in shown.iter().take(listed.min(GROUP_ROWS)) {
        let band = take(&mut rest, line);
        let text = reading(cx, entity)?;
        row(
            cx,
            out,
            band,
            (entity.title(), &text),
            (look.ink, look.faint),
        )?;
    }
    if more > 0 {
        let text = cx.format(
            "smart-home.widget.more",
            &[Arg::Count(i64::try_from(more).unwrap_or(0))],
        )?;
        let band = take(&mut rest, line);
        one_line(
            out,
            &cx.font,
            band,
            align,
            (small, small * 0.6),
            &text,
            look.faint,
        )?;
    }
    if let Some(ago) = ago {
        let height = rest.height.min(line);
        let band = take(&mut rest, height);
        one_line(
            out,
            &cx.font,
            band,
            align,
            (small, small * 0.6),
            &ago,
            look.faint,
        )?;
    }
    Ok(())
}

/// A room card: the room's name, its readings in one large line, its switches.
fn room(cx: &mut Cx<'_>, out: &mut Vec<Paint>, area: Rect, store: &Store) -> Result<(), Error> {
    let name = store.room_named(&cx.settings.string("room", NAME_CAP)?)?;
    let members: Vec<&Entity> = store.in_room(&name).collect();
    if members.is_empty() {
        return nothing(cx, out, area, "smart-home.widget.none");
    }
    let look = cx.shape.paint(out, area, 256)?;
    let mut rest = look.area;
    let small = cx.style().small;
    let line = line_px(&cx.font, small)?;
    let align = cx.settings.align()?;
    let switches: Vec<&Entity> = members
        .iter()
        .copied()
        .filter(|e| e.kind() != Kind::Value)
        .collect();
    let listed = if cx.settings.shows("switches")? {
        switches
            .len()
            .min(ROOM_SWITCHES)
            .min(usize::from(rest.height / line.max(1)).saturating_sub(3))
    } else {
        0
    };
    let title = if name.is_empty() {
        cx.tr("smart-home.title")?
    } else {
        paperpaw_ui_kit::text::copy(&name)?
    };
    let band = take(&mut rest, line);
    one_line(
        out,
        &cx.font,
        band,
        align,
        (small, small * 0.6),
        &title,
        look.faint,
    )?;
    // The readings in one line: "21.5 °C · 48 %".
    let mut readings = String::new();
    for e in members.iter().filter(|e| e.kind() == Kind::Value) {
        if e.is_unknown() {
            continue;
        }
        if !readings.is_empty() {
            readings.push_str(" · ");
        }
        readings.push_str(&reading(cx, e)?);
    }
    if readings.is_empty() {
        readings = cx.tr("smart-home.state.unknown")?;
    }
    let below = u16::try_from(listed).unwrap_or(0).saturating_mul(line);
    let height = rest.height.saturating_sub(below);
    let main = take(&mut rest, height);
    one_line(
        out,
        &cx.font,
        main,
        align,
        (f32::from(main.height), small),
        &readings,
        look.ink,
    )?;
    for e in switches.iter().take(listed) {
        let band = take(&mut rest, line);
        let text = reading(cx, e)?;
        row(cx, out, band, (e.title(), &text), (look.ink, look.faint))?;
    }
    Ok(())
}
