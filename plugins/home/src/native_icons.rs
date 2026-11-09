//! The icons of the dashboard: a tool's icon above its name (in the state its
//! plugin shows it: on, faint), the icons of the footer's actions and the
//! pencil.
//!
//! The characters come from the host (`icon.glyph`, looked up when the page is
//! laid out) and are drawn with the bound icon font. Where the host has none
//! (no icon font, a name it does not know) a tool shows its name alone and the
//! pencil its word.
use crate::native_source::HostSource;
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Rect,
    native::{
        Error, Host,
        fonts::{Font, IconGlyph, TextOrigin},
    },
};
use paperpaw_ui_kit::{Paint, Style, geometry, icons, mem, paint, text};

/// Lines a tool's name takes at most.
pub const NAME_LINES: usize = 3;
/// The icon of a footer action whose menu row names none.
const FALLBACK: &str = "puzzle";
/// The icon of the pencil.
const PENCIL: &str = "pencil";

/// How a tool is drawn: its icon (if the host has one) and its state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Face {
    pub glyph: Option<IconGlyph>,
    /// Shown as on: light on a dark ground.
    pub checked: bool,
    /// Else faint.
    pub enabled: bool,
}

/// The face of the tool of `action`.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn face(host: &Host<'_>, src: &HostSource, action: &str) -> Result<Face, Error> {
    let look = src.tool_look(action);
    let glyph = if look.icon.is_empty() {
        None
    } else {
        icons::glyph(host, &look.icon)?
    };
    Ok(Face {
        glyph,
        checked: look.checked,
        enabled: look.enabled,
    })
}

/// The icon of each of the first `count` footer actions: its own, else the
/// puzzle piece.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn footer(
    host: &Host<'_>,
    src: &HostSource,
    rects: &[Rect],
) -> Result<Vec<(Rect, Option<IconGlyph>)>, Error> {
    let mut found = Vec::new();
    for (at, rect) in rects.iter().enumerate() {
        let name = match src.footer_looks.get(at) {
            Some(look) if !look.icon.is_empty() => look.icon.as_str(),
            _ => FALLBACK,
        };
        mem::push(&mut found, (*rect, icons::glyph(host, name)?))?;
    }
    Ok(found)
}

/// The pencil's icon.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn pencil(host: &Host<'_>) -> Result<Option<IconGlyph>, Error> {
    icons::glyph(host, PENCIL)
}

/// The lines the name `name` takes in `cell` (at most [`NAME_LINES`]).
///
/// # Errors
/// Font refusals and [`Error::NoMemory`].
pub fn name_lines(
    font: &Font<'_>,
    style: &Style,
    cell: Rect,
    name: &str,
) -> Result<Vec<String>, Error> {
    let room = f32::from(cell.width.saturating_sub(style.gap)).max(1.0);
    text::wrap_with(
        |line| font.measure_text(style.small, line),
        name,
        room,
        NAME_LINES,
    )
}

/// The most lines any name takes among the tools of the same row (the cells
/// at the same top), for each of `lines` (the cells' names' lines, with the
/// top of each cell): names of tools side by side keep to one line.
#[must_use]
pub fn row_lines(cells: &[(u16, usize)]) -> Vec<usize> {
    cells
        .iter()
        .map(|(top, _)| {
            cells
                .iter()
                .filter(|(other, _)| other == top)
                .map(|(_, lines)| *lines)
                .max()
                .unwrap_or(0)
        })
        .collect()
}

/// The two fonts a tool is drawn with.
#[derive(Debug, Clone, Copy)]
pub struct Fonts<'a, 'f> {
    /// The interface font, for the name.
    pub ui: &'a Font<'f>,
    /// The icon font, where one is bound.
    pub icons: Option<&'a Font<'f>>,
    /// The circle's character, which frames every icon alike.
    pub circle: Option<&'a IconGlyph>,
}

/// What a tool shows: its name, its lines, its state.
#[derive(Debug)]
pub struct Tool<'a> {
    pub lines: &'a [String],
    pub face: Face,
    /// The most lines a name takes among the tools beside it.
    pub row_lines: usize,
}

/// Draws a tool in `cell`: its icon above its name, centred as a group; the
/// cell filled dark where it is on, the ink faint where it is not enabled.
///
/// # Errors
/// Font refusals and [`Error::NoMemory`].
pub fn draw_tool(
    out: &mut Vec<Paint>,
    fonts: &Fonts<'_, '_>,
    style: &Style,
    cell: Rect,
    tool: &Tool<'_>,
) -> Result<(), Error> {
    let face = tool.face;
    let faint = style.note;
    let ink = match (face.checked, face.enabled) {
        (true, _) => style.paper,
        (false, true) => style.ink,
        (false, false) => faint,
    };
    if face.checked {
        let back = if face.enabled { style.ink } else { faint };
        paint::push(out, Paint::Focus(cell, style.corner, None, back))?;
    }
    let font = fonts.ui;
    let named = !tool.lines.is_empty();
    let inner = i32::from(style.gap / 2);
    // The name's capitals above its baseline, its descenders below (by the ink
    // of letters that reach as far, so that names side by side line up).
    let (cap, descent) = if named {
        (
            match font.ink(style.small, "H")? {
                Some(ink) => -ink.top,
                None => (style.small * 0.7) as i32,
            },
            match font.ink(style.small, "g")? {
                Some(ink) => ink.bottom,
                None => (style.small * 0.25) as i32,
            },
        )
    } else {
        (0, 0)
    };
    let step = i32::from(geometry::line_px(font, style.small)?);
    let more = i32::try_from(tool.row_lines.max(tool.lines.len()).saturating_sub(1))
        .unwrap_or(0)
        .saturating_mul(step);
    let text_room = if named {
        inner
            .saturating_add(cap)
            .saturating_add(descent)
            .saturating_add(more)
    } else {
        0
    };
    let middle = f32::from(cell.x) + f32::from(cell.width) / 2.0;
    let drawn = match (fonts.icons, face.glyph.as_ref()) {
        (Some(icon_font), Some(glyph)) => {
            let room = (i32::from(cell.height) - text_room).max(1) as f32;
            let frame = icons::frame(icon_font, (fonts.circle, glyph), style.title, room)?;
            let group = frame.height().saturating_add(text_room);
            let y = i32::from(cell.y) + (i32::from(cell.height) - group).div_euclid(2);
            icons::draw(out, icon_font, glyph, &frame, (middle, y), ink)?;
            Some((y, frame.height()))
        }
        _ => None,
    };
    if !named {
        return Ok(());
    }
    let Some((y, height)) = drawn else {
        // The name alone, in the middle of the cell.
        let line = geometry::line_px(font, style.small)?;
        let total = u32::from(line) * u32::try_from(tool.lines.len()).unwrap_or(0);
        let mut top = u32::from(cell.y) + u32::from(cell.height).saturating_sub(total) / 2;
        for shown in tool.lines {
            let wide = font.measure_text(style.small, shown)?;
            let x = f32::from(cell.x) + (f32::from(cell.width) - wide).max(0.0) / 2.0;
            let row = Rect::new(
                cell.x,
                geometry::clamp_u16(i64::from(top)),
                cell.width,
                line,
            );
            text::label(out, font, shown, style.small, x, row, ink, None)?;
            top = top.saturating_add(u32::from(line));
        }
        return Ok(());
    };
    // The name below the icon.
    let baseline = y
        .saturating_add(height)
        .saturating_add(inner)
        .saturating_add(cap);
    for (n, shown) in tool.lines.iter().enumerate() {
        let wide = font.measure_text(style.small, shown)?;
        let x = f32::from(cell.x) + (f32::from(cell.width) - wide).max(0.0) / 2.0;
        let at = i32::try_from(n).unwrap_or(0).saturating_mul(step);
        paint::push(
            out,
            Paint::Text {
                text: text::copy(shown)?,
                size: style.small,
                origin: TextOrigin::new(x, baseline.saturating_add(at)),
                color: ink,
                clip: None,
            },
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tools_side_by_side_keep_to_the_most_lines_of_their_row() {
        // Two rows: tops 10 and 90.
        let cells = [(10, 1), (10, 3), (90, 2), (10, 2), (90, 1)];
        assert_eq!(row_lines(&cells), [3, 3, 2, 3, 2]);
        assert!(row_lines(&[]).is_empty());
    }
}
