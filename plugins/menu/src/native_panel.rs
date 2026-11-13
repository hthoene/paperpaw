//! The quick panel without the host: which controls a menu becomes (switches
//! and buttons several to a row, numbers with `−` and `+`, choices with few
//! options as segments, anything else to open), where they go on the screen
//! (at its bottom or hanging from its top, in pages when they need more room
//! than the panel may take) and what tapping each one does. Everything
//! allocates fallibly; text widths come from the caller (`Measure`).
//!
//! Controls name their menu row by its index in the rows the panel was made
//! from, so nothing is copied twice.
use crate::native_model::{Kind, Row, SettingInfo, SettingKind};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{Rect, native::Error};
use paperpaw_ui_kit::{
    mem::push,
    text::{concat, copy},
};

/// Most switches or buttons in one row.
pub const PER_ROW: usize = 3;
/// Most options shown as segments; more open a list.
pub const MAX_SEGMENTS: usize = 4;
/// Percent of the screen the panel may cover at the bottom.
pub const SHARE_BOTTOM: u32 = 80;
/// Percent of it the quick panel may cover, hanging from the top: below it,
/// room is left to tap to close it.
pub const SHARE_TOP: u32 = 95;

/// Most rows of a menu the panel reads: each costs a part and, for a setting,
/// a registry call per read. The rest are left out.
pub const MAX_ROWS: usize = 64;

/// `rows` cut to [`MAX_ROWS`].
pub fn cap_rows(rows: &mut Vec<Row>) {
    rows.truncate(MAX_ROWS);
}

/// The width of a text in the panel's small font.
pub trait Measure {
    /// Pixels `text` needs on one line.
    fn width(&self, text: &str) -> f32;
}

/// What tapping a control does; `usize`s are rows of the menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Control {
    /// Switches a setting on or off.
    Toggle(usize),
    /// Changes a number by a step down (`-1`) or up (`1`).
    Step(usize, i8),
    /// Sets a choice to its option (second number).
    Option(usize, usize),
    /// Opens the setting's list, editor or page.
    Open(usize),
    /// Runs an action or opens a submenu.
    Run(usize),
}

impl Control {
    /// The menu row it is part of.
    #[must_use]
    pub const fn row(&self) -> usize {
        match *self {
            Control::Toggle(row)
            | Control::Step(row, _)
            | Control::Option(row, _)
            | Control::Open(row)
            | Control::Run(row) => row,
        }
    }
}

/// A row of the menu with what the panel needs to decide its control.
#[derive(Clone, Copy, Debug)]
pub struct Entry<'a> {
    /// The menu row.
    pub row: &'a Row,
    /// The setting it is, when it is one the registry knows.
    pub info: Option<&'a SettingInfo>,
    /// A choice's option labels (translated), read when it has two to
    /// [`MAX_SEGMENTS`] options; empty otherwise.
    pub options: &'a [String],
}

/// What a row of the panel shows, before it is placed.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Line {
    /// Switches or buttons side by side.
    Buttons(Vec<Control>),
    /// A number's `−` and `+`, placed by its value when drawn.
    Stepper(usize),
    /// A number asking for a slider on a touch screen: `−`, the track, its
    /// value and `+`.
    Slider(usize),
    /// A choice's options side by side (menu row, number of options).
    Segments(usize, usize),
    /// A setting opened in its list or editor.
    Open(usize),
}

/// The sizes the panel is laid out with.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct Geometry {
    /// The screen's.
    pub width: u16,
    /// The screen's.
    pub height: u16,
    /// Room around things.
    pub gap: u16,
    /// Between buttons side by side: wider on touch screens, so that a finger
    /// hits the one it means.
    pub spacing: u16,
    /// A row's height.
    pub row: u16,
    /// A button's height in a row (the rest is room above and below).
    pub button: u16,
    /// The line towards the rest of the screen.
    pub border: u16,
    /// The labels of settings, on the left of their rows.
    pub label: u16,
    /// The bar that turns pages, where there are several.
    pub pager: u16,
    /// The screen's margin at the sides.
    pub margin: u16,
    /// The interface runs right to left.
    pub rtl: bool,
    /// It hangs from the top of the screen.
    pub top: bool,
    /// The screen takes touch: numbers that ask for a slider show one.
    pub touch: bool,
}

/// A row placed on the screen: an optional label on the left, then its
/// controls.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Placed {
    /// The whole row.
    pub area: Rect,
    /// The page it is on.
    pub page: usize,
    /// The setting row whose title is the label (a number's or a choice's).
    pub label: Option<usize>,
    /// The controls and where they are.
    pub controls: Vec<(Control, Rect)>,
    /// A slider's track, set by a tap on it (not a control: the keys move
    /// the number with its `−` and `+`).
    pub track: Option<Rect>,
}

/// Where the panel's parts go.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Layout {
    /// The rows, in order.
    pub rows: Vec<Placed>,
    /// The panel.
    pub region: Rect,
    /// The bar that turns pages, if there are pages.
    pub pager: Option<Rect>,
}

/// The text of a button for `row`: its label, the badge after it.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn button_text(row: &Row) -> Result<String, Error> {
    if row.badge.is_empty() {
        copy(&row.title)
    } else {
        concat(&[&row.title, " · ", &row.badge])
    }
}

/// `area` without the margins at its sides and a little above and below.
fn inset(area: Rect, g: &Geometry) -> Rect {
    let pad = area.height.saturating_sub(g.button) / 2;
    area.inset(g.margin, pad)
}

/// The part of a row after the label.
fn after_label(inner: Rect, g: &Geometry) -> Rect {
    Rect::new(
        inner.x.saturating_add(g.label),
        inner.y,
        inner.width.saturating_sub(g.label),
        inner.height,
    )
}

/// The width of the labels: as wide as the longest needs, at most half the
/// row. Numbers and choices carry labels; switches do not. Labels are set in
/// the text font: `text` measures with it (buttons and segments are measured
/// with the small font, as `lines` does).
#[must_use]
pub fn label_width(entries: &[Entry<'_>], text: &dyn Measure, g: &Geometry) -> u16 {
    let longest = entries
        .iter()
        .filter(|e| listed(e))
        .filter(|e| e.info.is_some_and(|i| i.kind != SettingKind::Bool))
        .map(|e| text.width(&e.row.title))
        .fold(0.0, f32::max);
    let half = g.width.saturating_sub(2 * g.margin) / 2;
    (longest as u16)
        .saturating_add(g.gap.saturating_mul(2))
        .min(half)
}

/// Whether a choice's options fit side by side in `area`, readably.
fn fits_segments(options: &[String], area: Rect, gap: u16, measure: &dyn Measure) -> bool {
    if options.is_empty() || options.len() > MAX_SEGMENTS {
        return false;
    }
    let cell = f32::from(area.width) / options.len() as f32 - 2.0 * f32::from(gap);
    options.iter().all(|o| measure.width(o) <= cell)
}

/// Whether the panel lists the entry: not a heading, and a setting only
/// while its `shown_if` holds and it has a choice to make.
fn listed(entry: &Entry<'_>) -> bool {
    entry.row.kind != Kind::Section && entry.info.is_none_or(SettingInfo::listed)
}

/// The control of a row that is a button or a switch, else none.
fn control_of(index: usize, entry: &Entry<'_>) -> Option<Control> {
    let plain = entry.info.filter(|i| i.editor_kind == 0);
    match (plain.map(|i| i.kind), entry.info) {
        (Some(SettingKind::Bool), _) => Some(Control::Toggle(index)),
        (None, None) => Some(Control::Run(index)),
        _ => None,
    }
}

/// Shares `group` out over as few rows as the widest label allows, as evenly
/// filled as can be (four buttons two and two, not three and one).
fn flush(
    group: &mut Vec<Control>,
    lines: &mut Vec<Line>,
    entries: &[Entry<'_>],
    measure: &dyn Measure,
    g: &Geometry,
) -> Result<(), Error> {
    if group.is_empty() {
        return Ok(());
    }
    let inner = f32::from(g.width.saturating_sub(2 * g.margin));
    let mut texts: Vec<String> = Vec::new();
    texts
        .try_reserve_exact(group.len())
        .map_err(|_| Error::NoMemory)?;
    for control in group.iter() {
        let row = entries.get(control.row()).ok_or(Error::State)?.row;
        texts.push(button_text(row)?);
    }
    let per_row = (1..=PER_ROW.min(group.len()))
        .rev()
        .find(|&n| {
            let cell = inner / n as f32 - 2.0 * f32::from(g.gap);
            texts.iter().all(|t| measure.width(t) <= cell)
        })
        .unwrap_or(1);
    let rows = group.len().div_ceil(per_row);
    let n = group.len().div_ceil(rows);
    for chunk in group.chunks(n) {
        let mut one = Vec::new();
        one.try_reserve_exact(chunk.len())
            .map_err(|_| Error::NoMemory)?;
        one.extend_from_slice(chunk);
        push(lines, Line::Buttons(one))?;
    }
    group.clear();
    Ok(())
}

/// What the panel shows of `entries`, row by row: switches and buttons
/// several to a row (as many as stay readable; switches and buttons not
/// mixed), numbers with their `−` and `+`, choices with few options as
/// segments, anything else to open. Headings and settings that are not
/// shown now (`SettingInfo::listed`) are left out.
///
/// `measure` is the small font's; the labels' width ([`label_width`]) is
/// the caller's, set in `g.label`.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn lines(
    entries: &[Entry<'_>],
    measure: &dyn Measure,
    g: &Geometry,
) -> Result<Vec<Line>, Error> {
    let rest = after_label(inset(Rect::new(0, 0, g.width, g.row), g), g);
    let mut lines = Vec::new();
    let mut group: Vec<Control> = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        if !listed(entry) {
            continue;
        }
        if let Some(control) = control_of(index, entry) {
            // Switches and buttons share rows, but not with each other; those
            // that follow each other are shared out evenly among rows.
            let same = group.first().is_some_and(|first| {
                matches!(first, Control::Toggle(_)) == matches!(control, Control::Toggle(_))
            });
            if !group.is_empty() && !same {
                flush(&mut group, &mut lines, entries, measure, g)?;
            }
            push(&mut group, control)?;
            continue;
        }
        flush(&mut group, &mut lines, entries, measure, g)?;
        let plain = entry.info.filter(|i| i.editor_kind == 0);
        let line = match plain.map(|i| i.kind) {
            Some(SettingKind::Int) if g.touch && plain.is_some_and(SettingInfo::slider) => {
                Line::Slider(index)
            }
            Some(SettingKind::Int) => Line::Stepper(index),
            Some(SettingKind::Choice) if fits_segments(entry.options, rest, g.gap, measure) => {
                Line::Segments(index, entry.options.len())
            }
            _ => Line::Open(index),
        };
        push(&mut lines, line)?;
    }
    flush(&mut group, &mut lines, entries, measure, g)?;
    Ok(lines)
}

/// `count` columns of `area` side by side, `gap` apart, as wide as each other
/// to a pixel (what dividing leaves goes a pixel each to the last ones), so
/// the last ends where `area` does. In a right-to-left interface the first
/// is the one at the right.
fn columns(area: Rect, count: usize, g: &Geometry) -> Result<Vec<Rect>, Error> {
    let Ok(n) = u16::try_from(count) else {
        return Ok(Vec::new());
    };
    if n == 0 {
        return Ok(Vec::new());
    }
    let room = area.width.saturating_sub(g.spacing.saturating_mul(n - 1));
    let (width, left) = (room / n, room % n);
    let mut cells = Vec::new();
    cells
        .try_reserve_exact(usize::from(n))
        .map_err(|_| Error::NoMemory)?;
    let mut x = area.x;
    for i in 0..n {
        let w = width + u16::from(i >= n - left);
        cells.push(Rect::new(x, area.y, w, area.height));
        x = x.saturating_add(w).saturating_add(g.spacing);
    }
    if g.rtl {
        cells.reverse();
    }
    Ok(cells)
}

/// The controls of `line` in its row, `area`.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn place(line: &Line, area: Rect, g: &Geometry) -> Result<Placed, Error> {
    let inner = inset(area, g);
    let rest = after_label(inner, g);
    let mut controls = Vec::new();
    let mut label = None;
    let mut track = None;
    match line {
        Line::Buttons(group) => {
            let cells = columns(inner, group.len(), g)?;
            controls
                .try_reserve_exact(group.len())
                .map_err(|_| Error::NoMemory)?;
            controls.extend(group.iter().copied().zip(cells));
        }
        Line::Stepper(row) => {
            label = Some(*row);
            controls.try_reserve_exact(2).map_err(|_| Error::NoMemory)?;
            // [−] value [+] after the label; the value is drawn between.
            let side = rest.height.min(rest.width / 3);
            let minus = Rect::new(rest.x, rest.y, side, rest.height);
            let plus = Rect::new(
                rest.x.saturating_add(rest.width.saturating_sub(side)),
                rest.y,
                side,
                rest.height,
            );
            controls.push((Control::Step(*row, -1), minus));
            controls.push((Control::Step(*row, 1), plus));
        }
        Line::Slider(row) => {
            label = Some(*row);
            controls.try_reserve_exact(2).map_err(|_| Error::NoMemory)?;
            // [−] track value [+]: the value has the room of a button.
            let side = rest.height.min(rest.width / 5);
            let minus = Rect::new(rest.x, rest.y, side, rest.height);
            let plus = Rect::new(
                rest.x.saturating_add(rest.width.saturating_sub(side)),
                rest.y,
                side,
                rest.height,
            );
            let from = minus.x.saturating_add(minus.width).saturating_add(g.gap);
            let to = plus.x.saturating_sub(side).saturating_sub(g.gap);
            let width = to.saturating_sub(from);
            controls.push((Control::Step(*row, -1), minus));
            controls.push((Control::Step(*row, 1), plus));
            track = Some(Rect::new(from, rest.y, width, rest.height));
        }
        Line::Segments(row, count) => {
            label = Some(*row);
            let cells = columns(rest, *count, g)?;
            controls
                .try_reserve_exact(cells.len())
                .map_err(|_| Error::NoMemory)?;
            controls.extend(
                cells
                    .into_iter()
                    .enumerate()
                    .map(|(option, cell)| (Control::Option(*row, option), cell)),
            );
        }
        // The whole row, as in lists.
        Line::Open(row) => {
            label = Some(*row);
            controls.try_reserve_exact(1).map_err(|_| Error::NoMemory)?;
            controls.push((Control::Open(*row), area));
        }
    }
    Ok(Placed {
        area,
        page: 0,
        label,
        controls,
        track,
    })
}

/// Places `lines` on the screen: at its bottom (or top), at most the share of
/// it high, in pages if they need more.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn layout(lines: &[Line], g: &Geometry) -> Result<Layout, Error> {
    let gap = g.gap;
    let share = if g.top { SHARE_TOP } else { SHARE_BOTTOM };
    let room = (u32::from(g.height) * share / 100) as u16;
    let room = room.saturating_sub(g.border.saturating_add(2 * gap));
    // Too many rows: pages, with a bar to turn them.
    let paged = lines.len() as u32 * u32::from(g.row) > u32::from(room);
    let bar = if paged { g.pager } else { 0 };
    let room = room.saturating_sub(bar);
    // Each row's page and where on it, rows filling a page in turn.
    let per_page = usize::from((room / g.row.max(1)).max(1));
    let tallest = lines.len().min(per_page) as u16 * g.row;
    let height = g
        .border
        .saturating_add(2 * gap)
        .saturating_add(tallest.max(g.row))
        .saturating_add(bar)
        .min(g.height);
    // From the screen's top edge: a band left there showed the screen below,
    // a status line or a page's first line cut.
    let region = if g.top {
        Rect::new(0, 0, g.width, height)
    } else {
        Rect::new(0, g.height - height, g.width, height)
    };
    // The border on the side towards the rest of the screen.
    let inner_top = if g.top {
        region.y
    } else {
        region.y.saturating_add(g.border)
    };
    let mut rows = Vec::new();
    rows.try_reserve_exact(lines.len())
        .map_err(|_| Error::NoMemory)?;
    for (index, line) in lines.iter().enumerate() {
        let (page, at) = (index / per_page, (index % per_page) as u16);
        let y = inner_top
            .saturating_add(gap)
            .saturating_add(at.saturating_mul(g.row));
        let mut row = place(line, Rect::new(0, y, g.width, g.row), g)?;
        row.page = page;
        rows.push(row);
    }
    let pager = paged.then(|| {
        let bottom = if g.top { g.border } else { 0 };
        let y = region
            .y
            .saturating_add(region.height)
            .saturating_sub(bar)
            .saturating_sub(bottom);
        Rect::new(0, y, g.width, bar)
    });
    Ok(Layout {
        rows,
        region,
        pager,
    })
}

impl Layout {
    /// How many pages the rows take.
    #[must_use]
    pub fn pages(&self) -> usize {
        self.rows.last().map_or(1, |row| row.page + 1)
    }

    /// Every control in reading order, with its place and its page.
    pub fn controls(&self) -> impl Iterator<Item = (&Control, Rect, usize)> {
        self.rows
            .iter()
            .flat_map(|row| row.controls.iter().map(|(c, r)| (c, *r, row.page)))
    }

    /// How many controls there are.
    #[must_use]
    pub fn count(&self) -> usize {
        self.rows.iter().map(|row| row.controls.len()).sum()
    }

    /// The slider track at `point` on `page`: its row (a menu row) and place.
    #[must_use]
    pub fn track_at(&self, page: usize, point: paperpaw_sdk::Point) -> Option<(usize, Rect)> {
        self.rows
            .iter()
            .filter(|row| row.page == page)
            .find_map(|row| {
                let track = row.track.filter(|t| t.contains(point))?;
                Some((row.label?, track))
            })
    }

    /// The page with control `index` (in reading order); the first page for
    /// one that is not there.
    #[must_use]
    pub fn page_of(&self, index: usize) -> usize {
        self.controls().nth(index).map_or(0, |(_, _, page)| page)
    }

    /// The row holding control `index`.
    #[must_use]
    pub fn row_of(&self, index: usize) -> Option<Rect> {
        let mut count = 0;
        self.rows.iter().find_map(|row| {
            count += row.controls.len();
            (index < count).then_some(row.area)
        })
    }

    /// The control at `point` on `page`, with its number in reading order.
    #[must_use]
    pub fn hit(&self, page: usize, point: paperpaw_sdk::Point) -> Option<(usize, Control)> {
        self.controls()
            .enumerate()
            .find(|(_, (_, rect, on))| *on == page && rect.contains(point))
            .map(|(index, (control, _, _))| (index, *control))
    }
}

/// The radius of a slider's knob on `track`; the knob's centre goes along
/// the track less that at both ends.
#[must_use]
pub fn knob_radius(track: Rect) -> u16 {
    track.height / 3
}

/// The number a tap at pixel `x` of a slider's `track` sets, over `min..=max`
/// in steps of `step` (1 where 0), the nearest step; a tap beyond an end sets
/// that end.
#[must_use]
pub fn value_at(track: Rect, x: i32, (min, max, step): (i64, i64, i64)) -> i64 {
    // In u64 without a product that can overflow: a card plugin declares any
    // range, up to the whole of i64, and 128-bit helpers are not wanted on
    // the X4. The range's `span / run` is shared out first.
    let max = max.max(min);
    let span = max.abs_diff(min);
    let radius = i64::from(knob_radius(track));
    let run = (i64::from(track.width) - 2 * radius).max(1);
    let along = (i64::from(x) - i64::from(track.x) - radius).clamp(0, run);
    let (run, along) = (run as u64, along as u64);
    let (whole, rest) = (span / run, span % run);
    let offset = whole * along + (rest * along + run / 2) / run;
    let step = u64::try_from(step).unwrap_or(1).max(1);
    let offset = (offset.saturating_add(step / 2) / step)
        .saturating_mul(step)
        .min(span);
    min.checked_add_unsigned(offset).unwrap_or(max)
}

/// Where the knob of a slider on `track` is, at `value`: its centre's pixel.
#[must_use]
pub fn knob_x(track: Rect, value: i64, (min, max): (i64, i64)) -> i32 {
    let max = max.max(min);
    let radius = i64::from(knob_radius(track));
    let run = (i64::from(track.width) - 2 * radius).max(0) as u64;
    let span = max.abs_diff(min);
    let offset = value.clamp(min, max).abs_diff(min);
    // A pixel's worth is enough: both are scaled down to 32 bits first, so
    // that `run * offset` (a pixel count under 2^16) cannot overflow.
    let shift = (u64::BITS - span.leading_zeros()).saturating_sub(32);
    let (span, offset) = (span >> shift, offset >> shift);
    let along = (run * offset).checked_div(span).unwrap_or(0);
    let at = i64::from(track.x) + radius + along as i64;
    i32::try_from(at).unwrap_or(0)
}

/// Where the focus goes from `from` (`None`: nowhere yet) one control forward
/// (`delta` 1) or back (`-1`), round the ends, past the controls `enabled`
/// refuses unless none is enabled (then it goes one on all the same).
/// `None` when there are no controls.
#[must_use]
pub fn step_focus(
    count: usize,
    from: Option<usize>,
    delta: isize,
    enabled: impl Fn(usize) -> bool,
) -> Option<usize> {
    if count == 0 {
        return None;
    }
    // From nowhere: forward the first, back the last (interface 6 went back
    // to the one before the last).
    let current = from.map_or(if delta < 0 { count as isize } else { -1 }, |f| f as isize);
    let step = |at: isize| (at + delta).rem_euclid(count as isize) as usize;
    let first = step(current);
    let mut focus = first;
    for _ in 0..count {
        if enabled(focus) {
            return Some(focus);
        }
        focus = step(focus as isize);
    }
    Some(first)
}

#[cfg(test)]
mod tests;
