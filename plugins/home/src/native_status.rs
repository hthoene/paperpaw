//! The status line as a widget of the home screen: the status items the user
//! chose for its left, centre and right (any plugin's, as the reader's lines
//! have them), asked of the host with their values, laid out as interface 6
//! laid them (`native_status_layout`), the texts drawn here and the items their
//! owners draw composed into the places the line gives them (`ui.compose`). Or
//! a whole line another plugin draws, where the user chose one.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Rect,
    native::{
        Error, Host,
        callbacks::{DrawFrame, FrameRect},
        fonts::TextOrigin,
        registry::{PageCursor, Registry, StatusScope},
        ui::{Compose, StatusLine, Ui},
    },
};
use paperpaw_ui_kit::{
    Paint, geometry, mem, paint,
    text::{self as plain, copy},
    widget::{Cx, Frame, text::ink_baseline},
};

use crate::native_status_layout::{self as lay, Laid, Listed, Parts};

/// What the figures of a line are measured by: the line is centred on them.
const FIGURES: &str = "0123456789";
/// The most items asked for one line (the host answers eight at a time).
const MOST: usize = 16;
/// The items the registry tells of, at most.
const KNOWN: usize = 64;

/// The status declarations a line may list: what the registry says of each.
#[derive(Debug)]
struct Meta {
    id: String,
    plugin: String,
    priority: u8,
    /// Its least width, in thousandths of the text size.
    size: u32,
    tap: String,
}

/// The registry's status items among `wanted` (comma lists), in the registry's
/// order.
fn read_meta(host: &Host<'_>, wanted: &[&str; 3]) -> Result<Vec<Meta>, Error> {
    let registry = Registry::open(host)?;
    let found = read_pages(&registry, host, wanted);
    let released = registry.release(host);
    let found = found?;
    released?;
    Ok(found)
}

fn read_pages(
    registry: &Registry,
    host: &Host<'_>,
    wanted: &[&str; 3],
) -> Result<Vec<Meta>, Error> {
    let mut input = mem::buffer(256)?;
    let mut out = mem::buffer(mem::SERVICE_BUFFER)?;
    let listed = |id: &str| wanted.iter().any(|list| ids(list).any(|w| w == id));
    for _ in 0..3 {
        let mut found: Vec<Meta> = Vec::new();
        let mut cursor = PageCursor::start();
        let result = loop {
            match registry.status(host, StatusScope::Items, cursor, &mut input, &mut out) {
                Ok(page) => {
                    for row in page.rows() {
                        if !listed(row.id()) || found.len() >= KNOWN {
                            continue;
                        }
                        let meta = Meta {
                            id: copy(row.id())?,
                            plugin: copy(row.plugin())?,
                            priority: u8::try_from(row.priority()).unwrap_or(u8::MAX),
                            size: row.size(),
                            tap: copy(row.tap())?,
                        };
                        mem::push(&mut found, meta)?;
                    }
                    match page.end().next_cursor() {
                        Some(next) => cursor = next,
                        None => break Ok(()),
                    }
                }
                Err(error) => break Err(error),
            }
        };
        match result {
            Ok(()) => return Ok(found),
            // The registry changed while it was read: again from the start.
            Err(Error::State) => {}
            Err(error) => return Err(error),
        }
    }
    Err(Error::State)
}

/// The item ids of a list as stored in a setting: separated by commas.
fn ids(list: &str) -> impl Iterator<Item = &str> {
    list.split(',').map(str::trim).filter(|id| !id.is_empty())
}

/// An item with its value now.
#[derive(Debug)]
struct Shown<'a> {
    id: String,
    /// Its text; empty for a drawn value.
    text: String,
    /// Composed into its place where it is drawn.
    drawn: Option<Compose<'a>>,
}

/// The items of the three lists that the registry knows, in order, with their
/// values (an item that shows nothing is left out) and which part each is in.
fn values<'a>(
    host: &'a Host<'a>,
    ui: &Ui,
    meta: &[Meta],
    lists: &[&str; 3],
    buffers: &'a mut [Vec<u8>; 2],
) -> Result<(Vec<Shown<'a>>, [Vec<usize>; 3]), Error> {
    let mut asked: Vec<&str> = Vec::new();
    let mut parts: [Vec<&str>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    for (at, list) in lists.iter().enumerate() {
        for id in ids(list) {
            if meta.iter().any(|m| m.id == id) && asked.len() < MOST {
                mem::push(&mut asked, id)?;
                mem::push(&mut parts[at], id)?;
            }
        }
    }
    let mut shown: Vec<Shown<'a>> = Vec::new();
    let [first, second] = buffers;
    let mut scratch = [0u8; 256];
    let mut input = mem::buffer(mem::SERVICE_BUFFER)?;
    for (chunk, out) in asked.chunks(8).zip([first, second]) {
        let items = join(chunk)?;
        let page = ui.status_values(host, &items, &[], &mut scratch, &mut input, out)?;
        for row in page {
            let row = row?;
            if row.is_own() || row.is_hidden() {
                continue;
            }
            let drawn = row.draw_handle().map(Compose::StatusItem);
            // An empty value and no drawing hides the item.
            if drawn.is_none() && row.text().is_empty() {
                continue;
            }
            mem::push(
                &mut shown,
                Shown {
                    id: copy(row.item())?,
                    text: copy(row.text())?,
                    drawn,
                },
            )?;
        }
    }
    let mut at_part: [Vec<usize>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    for (part, listed) in parts.iter().enumerate() {
        for id in listed {
            if let Some(index) = shown.iter().position(|s| s.id == *id) {
                mem::push(&mut at_part[part], index)?;
            }
        }
    }
    Ok((shown, at_part))
}

/// `ids` separated by newlines, as `status_values` asks for them.
fn join(ids: &[&str]) -> Result<String, Error> {
    let mut out = String::new();
    let len: usize = ids.iter().map(|id| id.len() + 1).sum();
    out.try_reserve_exact(len).map_err(|_| Error::NoMemory)?;
    for (n, id) in ids.iter().enumerate() {
        if n > 0 {
            out.push('\n');
        }
        out.push_str(id);
    }
    Ok(out)
}

/// The height of the status line: a line of the small size and half a gap, and
/// the frame.
///
/// # Errors
/// Font refusals.
pub fn height(cx: &Cx<'_>) -> Result<u16, Error> {
    let style = cx.style();
    Ok(geometry::line_px(&cx.font, style.small)?
        .saturating_add(style.gap / 2)
        .saturating_add(cx.shape.frame_height()))
}

/// What the line shows, laid out in `area`.
struct Line<'a> {
    shown: Vec<Shown<'a>>,
    laid: Vec<Laid>,
    meta: Vec<Meta>,
}

fn lay_out<'a>(
    cx: &Cx<'a>,
    ui: &Ui,
    area: Rect,
    buffers: &'a mut [Vec<u8>; 2],
) -> Result<Line<'a>, Error> {
    let lists = [
        cx.settings.string("left", 1024)?,
        cx.settings.string("center", 1024)?,
        cx.settings.string("right", 1024)?,
    ];
    let wanted = [lists[0].as_str(), lists[1].as_str(), lists[2].as_str()];
    let meta = read_meta(cx.host, &wanted)?;
    let (shown, at_part) = values(cx.host, ui, &meta, &wanted, buffers)?;
    let size = cx.style().small;
    let mut parts: Parts = [Vec::new(), Vec::new(), Vec::new()];
    for (part, indices) in at_part.iter().enumerate() {
        for &index in indices {
            let Some(item) = shown.get(index) else {
                continue;
            };
            let known = meta.iter().find(|m| m.id == item.id);
            let least = known.map_or(0.0, |m| m.size as f32 * size / 1000.0);
            // A drawn value is as wide as its owner says it is at least (a symbol
            // is about as wide as it is high where it says nothing).
            let width = match item.drawn {
                Some(_) => least.max(size),
                None => cx.font.measure_text(size, &item.text)?.max(least),
            };
            let listed = Listed {
                index,
                priority: known.map_or(128, |m| m.priority),
                width,
                text: item.drawn.is_none(),
                least,
            };
            mem::push(&mut parts[part], listed)?;
        }
    }
    let laid = lay::layout(size, (f32::from(area.x), f32::from(area.width)), parts);
    Ok(Line { shown, laid, meta })
}

/// The items to put on the canvas once the paint is replayed: each owner's drawn
/// value and where it goes.
#[derive(Debug)]
pub struct Placed<'a> {
    pub target: Compose<'a>,
    pub rect: FrameRect,
}

/// Draws the status line in `area` (inside its frame, where the shape has
/// painted it): the texts into `out`, the drawn values into `placed`; the whole
/// line another plugin draws where the user chose one.
///
/// # Errors
/// Font and host refusals, [`Error::NoMemory`].
pub fn draw<'a>(
    cx: &mut Cx<'a>,
    ui: &Ui,
    out: &mut Vec<Paint>,
    placed: &mut Vec<Placed<'a>>,
    buffers: &'a mut [Vec<u8>; 2],
    (area, color): (Rect, paperpaw_sdk::Color),
) -> Result<(), Error> {
    let size = cx.style().small;
    let chosen = cx.settings.string("line", 256)?;
    if !chosen.is_empty() {
        let (mut scratch, mut input, mut reply) = ([0u8; 256], mem::buffer(1024)?, [0u8; 256]);
        let line = ui.status_line(cx.host, &chosen, &[], &mut scratch, &mut input, &mut reply);
        // A line that is not there (its plugin gone), or is the caller's own: nothing.
        if let Ok(StatusLine::Draw(handle)) = line {
            mem::push(
                placed,
                Placed {
                    target: Compose::StatusLine(handle),
                    rect: paint::frame_rect(area),
                },
            )?;
        }
        return Ok(());
    }
    let line = lay_out(cx, ui, area, buffers)?;
    let baseline = ink_baseline(&cx.font, size, FIGURES, area)?;
    for laid in &line.laid {
        let Some(item) = line.shown.get(laid.index) else {
            continue;
        };
        match item.drawn {
            Some(target) => {
                let at = Rect::new(
                    geometry::clamp_u16(laid.x as i64),
                    area.y,
                    geometry::clamp_u16(laid.width as i64),
                    area.height,
                );
                mem::push(
                    placed,
                    Placed {
                        target,
                        rect: paint::frame_rect(at),
                    },
                )?;
            }
            None => {
                let text = plain::fit(&cx.font, size, &item.text, laid.width + 1.0)?;
                paint::push(
                    out,
                    Paint::Text {
                        text,
                        size,
                        origin: TextOrigin::new(laid.x, baseline),
                        color,
                        clip: Some(area),
                    },
                )?;
            }
        }
    }
    Ok(())
}

/// Where a tap on the line in `area` does something: the items with an action
/// of their owner's, as laid out, each with a fifth of the text size beside it,
/// in reading order. The target is the item's place in that order from 1.
///
/// # Errors
/// Font and host refusals, [`Error::NoMemory`].
pub fn targets(cx: &Cx<'_>, ui: &Ui, area: Rect) -> Result<Vec<(u32, Rect)>, Error> {
    let mut buffers = [
        mem::buffer(mem::SERVICE_BUFFER)?,
        mem::buffer(mem::SERVICE_BUFFER)?,
    ];
    let line = lay_out(cx, ui, area, &mut buffers)?;
    let mut tappable = tapped(&line);
    tappable.sort_unstable_by(|a, b| a.0.x.total_cmp(&b.0.x));
    let reach = cx.style().small * 0.2;
    let (left, right) = (f32::from(area.x), f32::from(area.x) + f32::from(area.width));
    let mut found = Vec::new();
    for (n, (laid, _)) in tappable.iter().enumerate() {
        let x0 = (laid.x - reach).max(left);
        let x1 = (laid.x + laid.width + reach).min(right);
        let rect = Rect::new(
            geometry::clamp_u16(x0 as i64),
            area.y,
            geometry::clamp_u16((x1 - x0).max(0.0) as i64),
            area.height,
        );
        mem::push(&mut found, (u32::try_from(n + 1).unwrap_or(u32::MAX), rect))?;
    }
    Ok(found)
}

/// The laid items whose owner declared an action of its own.
fn tapped<'l>(line: &'l Line<'_>) -> Vec<(Laid, &'l str)> {
    let mut found = Vec::new();
    for laid in &line.laid {
        let Some(item) = line.shown.get(laid.index) else {
            continue;
        };
        let Some(meta) = line.meta.iter().find(|m| m.id == item.id) else {
            continue;
        };
        let own = meta
            .tap
            .strip_prefix(meta.plugin.as_str())
            .is_some_and(|rest| rest.starts_with('.'));
        if own && found.try_reserve(1).is_ok() {
            found.push((*laid, meta.tap.as_str()));
        }
    }
    found
}

/// The action the tap on the item at `target` (from 1, as [`targets`]) runs.
///
/// # Errors
/// Font and host refusals, [`Error::NoMemory`].
pub fn action(cx: &Cx<'_>, ui: &Ui, area: Rect, target: u32) -> Result<Option<String>, Error> {
    let mut buffers = [
        mem::buffer(mem::SERVICE_BUFFER)?,
        mem::buffer(mem::SERVICE_BUFFER)?,
    ];
    let line = lay_out(cx, ui, area, &mut buffers)?;
    let mut tappable = tapped(&line);
    tappable.sort_unstable_by(|a, b| a.0.x.total_cmp(&b.0.x));
    let at = usize::try_from(target).unwrap_or(0).saturating_sub(1);
    tappable.get(at).map(|(_, tap)| copy(tap)).transpose()
}

/// What the line's texts show, to tell when it changed: the values of the items
/// of its parts (drawn values have none: the line is live, drawn each minute).
///
/// # Errors
/// The host's refusals and [`Error::NoMemory`].
pub fn shows(cx: &Cx<'_>, ui: &Ui) -> Result<u64, Error> {
    let mut buffers = [
        mem::buffer(mem::SERVICE_BUFFER)?,
        mem::buffer(mem::SERVICE_BUFFER)?,
    ];
    let lists = [
        cx.settings.string("left", 1024)?,
        cx.settings.string("center", 1024)?,
        cx.settings.string("right", 1024)?,
    ];
    let wanted = [lists[0].as_str(), lists[1].as_str(), lists[2].as_str()];
    let meta = read_meta(cx.host, &wanted)?;
    let (shown, _) = values(cx.host, ui, &meta, &wanted, &mut buffers)?;
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for item in &shown {
        for byte in item.id.bytes().chain(item.text.bytes()).chain([0xff]) {
            hash = (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
        }
    }
    Ok(hash)
}

/// Draws the status line instance `widget` in `area` (its widget rectangle):
/// the frame, the texts, then the values its owners draw, composed in.
///
/// # Errors
/// Font and host refusals, [`Error::NoMemory`].
pub fn draw_widget(
    host: &Host<'_>,
    ui: &Ui,
    frame: &mut DrawFrame<'_>,
    widget: &str,
    area: Rect,
) -> Result<(), Error> {
    // The buffers outlive the context: the drawn values borrow the host for the
    // callback and are composed in before it ends.
    let mut buffers = [
        mem::buffer(mem::SERVICE_BUFFER)?,
        mem::buffer(mem::SERVICE_BUFFER)?,
    ];
    let mut cx = Cx::open(host, widget)?;
    let mut out = Vec::new();
    let mut placed = Vec::new();
    let look = cx.shape.paint(&mut out, area, 256)?;
    // The widget's own ink where the reader set a look of it (a dark card, a paper,
    // a text colour); else the interface's note ink.
    let own = look.frame == Frame::Dark
        || cx.settings.is_chosen("paper")?
        || cx.settings.is_chosen("ink")?;
    let color = if own { look.faint } else { cx.style().note };
    // A host that does not answer the status calls (the compose family on the
    // X4 until its stack gate passes) leaves an empty line, not a failed draw.
    match draw(
        &mut cx,
        ui,
        &mut out,
        &mut placed,
        &mut buffers,
        (look.area, color),
    ) {
        Err(Error::Unsupported) => {}
        other => other?,
    }
    paint::replay(frame, &cx.font, out.iter())?;
    for item in &placed {
        // An owner that cannot draw now (busy, gone) leaves its place empty.
        Ui::compose(host, item.target, item.rect).ok();
    }
    Ok(())
}
