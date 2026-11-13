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

/// What room a name leaves at the sides of its cell, as interface 6 left.
const SIDE: u16 = 4;
/// The least share of the small size a name shrinks to, to fit one line.
const LEAST: f32 = 0.85;
/// How much smaller each size tried to fit a name on one line is.
const SIZE_STEP: f32 = 0.5;

/// A tool's name as it is drawn: its size and its lines.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Name {
    pub size: f32,
    pub lines: Vec<String>,
}

/// The name `name` as it fits `cell` (see [`fit_name`]).
///
/// # Errors
/// Font refusals and [`Error::NoMemory`].
pub fn name_lines(font: &Font<'_>, style: &Style, cell: Rect, name: &str) -> Result<Name, Error> {
    let room = f32::from(cell.width.saturating_sub(SIDE)).max(1.0);
    fit_name(
        |size, line| font.measure_text(size, line),
        style.small,
        name,
        room,
    )
}

/// `name` in `room` as interface 6 fitted a tool's name: on one line, a
/// little smaller where that is what it takes (down to `LEAST` of
/// `small`); else on up to [`NAME_LINES`] lines broken between words or
/// after a hyphen written in one, never inside a word; else on one line
/// that ends in "…". `measure` gives a text's width at a size.
///
/// # Errors
/// Errors of `measure` and [`Error::NoMemory`].
pub fn fit_name(
    mut measure: impl FnMut(f32, &str) -> Result<f32, Error>,
    small: f32,
    name: &str,
    room: f32,
) -> Result<Name, Error> {
    let name = name.trim();
    if name.is_empty() {
        return Ok(Name {
            size: small,
            lines: Vec::new(),
        });
    }
    let least = small * LEAST;
    let mut size = small;
    while size >= least {
        if measure(size, name)? <= room {
            let mut lines = Vec::new();
            mem::push(&mut lines, text::copy(name)?)?;
            return Ok(Name { size, lines });
        }
        size -= SIZE_STEP;
    }
    for size in [small, least] {
        if let Some(lines) = whole_words(|line| measure(size, line), name, room)? {
            return Ok(Name { size, lines });
        }
    }
    let lines = text::wrap_with(|line| measure(least, line), name, room, 1)?;
    Ok(Name { size: least, lines })
}

/// `name` broken between its words, or after a hyphen inside one, into at
/// most [`NAME_LINES`] lines no wider than `room`; `None` where a part is
/// wider than a line or the lines are too few.
fn whole_words(
    mut measure: impl FnMut(&str) -> Result<f32, Error>,
    name: &str,
    room: f32,
) -> Result<Option<Vec<String>>, Error> {
    let mut lines: Vec<String> = Vec::new();
    let mut line = String::new();
    for word in name.split_whitespace() {
        let mut first = true;
        for part in word.split_inclusive('-') {
            let glue = if first && !line.is_empty() { " " } else { "" };
            first = false;
            let joined = text::concat(&[&line, glue, part])?;
            if measure(&joined)? <= room {
                line = joined;
                continue;
            }
            if measure(part)? > room {
                return Ok(None);
            }
            if !line.is_empty() {
                mem::push(&mut lines, core::mem::take(&mut line))?;
            }
            line = text::copy(part)?;
        }
    }
    if !line.is_empty() {
        mem::push(&mut lines, line)?;
    }
    Ok((lines.len() <= NAME_LINES).then_some(lines))
}

/// The most lines any name takes among the tools of the same row (the cells
/// at the same top), for each of `cells` (the top of each cell and its name's
/// lines): names of tools side by side keep to one line. One pass over the
/// cells and one over the distinct rows, which are few.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn row_lines(cells: &[(u16, usize)]) -> Result<Vec<usize>, Error> {
    let mut rows: Vec<(u16, usize)> = Vec::new();
    for (top, lines) in cells {
        if let Some(row) = rows.iter_mut().find(|(at, _)| at == top) {
            row.1 = row.1.max(*lines);
        } else {
            mem::push(&mut rows, (*top, *lines))?;
        }
    }
    let mut out = Vec::new();
    out.try_reserve_exact(cells.len())
        .map_err(|_| Error::NoMemory)?;
    for (top, _) in cells {
        let most = rows
            .iter()
            .find(|(at, _)| at == top)
            .map_or(0, |(_, lines)| *lines);
        out.push(most);
    }
    Ok(out)
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
    /// The size its name is drawn at.
    pub size: f32,
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
            let wide = font.measure_text(tool.size, shown)?;
            let x = f32::from(cell.x) + (f32::from(cell.width) - wide).max(0.0) / 2.0;
            let row = Rect::new(
                cell.x,
                geometry::clamp_u16(i64::from(top)),
                cell.width,
                line,
            );
            text::label(out, font, shown, tool.size, x, row, ink, None)?;
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
        let wide = font.measure_text(tool.size, shown)?;
        let x = f32::from(cell.x) + (f32::from(cell.width) - wide).max(0.0) / 2.0;
        let at = i32::try_from(n).unwrap_or(0).saturating_mul(step);
        paint::push(
            out,
            Paint::Text {
                text: text::copy(shown)?,
                size: tool.size,
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

    /// Every letter half as wide as the size: `name` fitted in 80 pixels at 17.
    fn fitted(name: &str) -> Name {
        let measure = |size: f32, line: &str| Ok(line.chars().count() as f32 * size * 0.5);
        fit_name(measure, 17.0, name, 80.0).unwrap()
    }

    #[test]
    fn a_name_a_little_too_wide_shrinks_onto_one_line() {
        // #257 (x4-drawer-de): "Bibliothe" over "k".
        let name = fitted("Bibliothek");
        assert_eq!(name.lines, ["Bibliothek"]);
        assert!(
            name.size < 17.0 && name.size >= 17.0 * LEAST,
            "{}",
            name.size
        );
        assert_eq!(fitted("News").size, 17.0);
    }

    #[test]
    fn a_name_breaks_between_words_or_after_its_hyphen_never_inside_a_word() {
        assert_eq!(fitted("Notizen-Tresor").lines, ["Notizen-", "Tresor"]);
        assert_eq!(fitted("Plugin-Store").lines, ["Plugin-", "Store"]);
        assert_eq!(
            fitted("Wi-Fi transfer files").lines,
            ["Wi-Fi", "transfer", "files"]
        );
        // A word no line holds is cut short on one line, marked as cut.
        let long = fitted("Einstellungen");
        assert_eq!(long.lines.len(), 1, "{:?}", long.lines);
        assert!(long.lines[0].ends_with('…'), "{:?}", long.lines);
        assert!(fitted("").lines.is_empty());
    }

    #[test]
    fn tools_side_by_side_keep_to_the_most_lines_of_their_row() {
        // Two rows: tops 10 and 90.
        let cells = [(10, 1), (10, 3), (90, 2), (10, 2), (90, 1)];
        assert_eq!(row_lines(&cells).unwrap(), [3, 3, 2, 3, 2]);
        assert!(row_lines(&[]).unwrap().is_empty());
    }
}
