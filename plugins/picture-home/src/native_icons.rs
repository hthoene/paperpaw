//! The icons of the dashboard, as interface 6 drew them: a tool's icon above
//! its name (in the state its plugin shows it: on, faint), the pencil, and while
//! editing the plus of *Add* and the tick of *Done*. Copied from `plugins/home`'s
//! `native_icons.rs` and adapted (the two plugins are separate crates): this
//! screen has no footer actions, and names of tools side by side are not kept
//! to one height (the interface-6 grid of this plugin did not either).
//!
//! The characters come from the host (`icon.glyph`, looked up when the page is
//! laid out) and are drawn with the icon font bound to `role:icons`. Where the
//! host has none (no icon font bound, a name its set lacks) a tool shows its
//! name alone and the buttons their words.
use crate::native_source::HostSource;
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Color, Rect,
    native::{
        Error, Host,
        fonts::{Font, IconGlyph, TextOrigin},
    },
};
use paperpaw_ui_kit::{Paint, Style, geometry, icons, paint, text};

/// Lines a tool's name takes at most.
pub const NAME_LINES: usize = 3;
/// The icons of the footer's buttons, by the names interface 6 drew them with.
const PENCIL: &str = "pencil";
const CHECK: &str = "check";
const PLUS: &str = "plus";
/// The icon whose ink frames every tool's icon alike.
pub const CIRCLE: &str = "circle";

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

/// The icons of the footer's buttons; each `None` where the host has none.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Buttons {
    /// The pencil, which starts editing.
    pub pencil: Option<IconGlyph>,
    /// The tick of *Done*, while editing.
    pub check: Option<IconGlyph>,
    /// The plus of *Add*, while editing.
    pub plus: Option<IconGlyph>,
}

impl Buttons {
    /// The icons the footer shows: the pencil where it is on and not editing,
    /// the plus and the tick while editing.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn read(host: &Host<'_>, pencil: bool, editing: bool) -> Result<Self, Error> {
        let named = |on: bool, name: &str| -> Result<Option<IconGlyph>, Error> {
            if on {
                icons::glyph(host, name)
            } else {
                Ok(None)
            }
        };
        Ok(Self {
            pencil: named(pencil && !editing, PENCIL)?,
            check: named(editing, CHECK)?,
            plus: named(editing, PLUS)?,
        })
    }

    /// The icon of the pencil's place: the tick while editing, else the pencil.
    #[must_use]
    pub const fn pencil_place(&self, editing: bool) -> Option<&IconGlyph> {
        if editing {
            self.check.as_ref()
        } else {
            self.pencil.as_ref()
        }
    }
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

/// The ink of a tool and what it sits on where it shows on, from the style's
/// `(ink, paper, note)`: light on dark where it is on, faint where it is not
/// enabled.
#[must_use]
pub const fn colours_of(
    face: Face,
    (ink, paper, note): (Color, Color, Color),
) -> (Color, Option<Color>) {
    let back = if face.enabled { ink } else { note };
    match (face.checked, face.enabled) {
        (true, _) => (paper, Some(back)),
        (false, true) => (ink, None),
        (false, false) => (note, None),
    }
}

/// Draws a tool in `cell`: its icon above its name, centred as a group; the
/// cell filled dark where it is on, the ink faint where it is not enabled.
///
/// # Errors
/// Font refusals and [`Error::NoMemory`].
#[inline(never)]
pub fn draw_tool(
    out: &mut Vec<Paint>,
    fonts: &Fonts<'_, '_>,
    style: &Style,
    cell: Rect,
    (lines, face): (&[String], Face),
) -> Result<(), Error> {
    let (ink, back) = colours_of(face, (style.ink, style.paper, style.note));
    if let Some(back) = back {
        paint::push(out, Paint::Focus(cell, style.corner, None, back))?;
    }
    let font = fonts.ui;
    let named = !lines.is_empty();
    let inner = i32::from(style.gap / 2);
    // The name's capitals above its baseline, its descenders below.
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
    let more = i32::try_from(lines.len().saturating_sub(1))
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
            let room = i32::from(cell.height).saturating_sub(text_room).max(1) as f32;
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
        return name(out, font, style, cell, (lines, ink));
    };
    // The name below the icon.
    let baseline = y
        .saturating_add(height)
        .saturating_add(inner)
        .saturating_add(cap);
    for (n, shown) in lines.iter().enumerate() {
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

/// Lines of a name alone, centred in `cell` both ways (a tool without an icon,
/// a button's word).
///
/// # Errors
/// Font refusals and [`Error::NoMemory`].
pub fn name(
    out: &mut Vec<Paint>,
    font: &Font<'_>,
    style: &Style,
    cell: Rect,
    (lines, ink): (&[String], Color),
) -> Result<(), Error> {
    let line = geometry::line_px(font, style.small)?;
    let total = u32::from(line).saturating_mul(u32::try_from(lines.len()).unwrap_or(0));
    let mut top = u32::from(cell.y) + u32::from(cell.height).saturating_sub(total) / 2;
    for shown in lines {
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
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn a_tool_is_drawn_light_on_dark_when_on_and_faint_when_not_enabled() {
        let (ink, paper, note) = (Color::BLACK, Color::WHITE, Color::rgb(0x80, 0x80, 0x80));
        let face = |checked, enabled| Face {
            glyph: None,
            checked,
            enabled,
        };
        let of = |checked, enabled| colours_of(face(checked, enabled), (ink, paper, note));
        assert_eq!(of(false, true), (ink, None));
        assert_eq!(of(false, false), (note, None));
        assert_eq!(of(true, true), (paper, Some(ink)));
        assert_eq!(of(true, false), (paper, Some(note)));
    }

    #[test]
    fn the_pencils_place_shows_the_tick_while_editing() {
        let none = Buttons::default();
        assert_eq!(none.pencil_place(false), None);
        assert_eq!(none.pencil_place(true), None);
    }

    #[test]
    fn the_buttons_icons_are_those_interface_6_drew() {
        assert_eq!(
            (PENCIL, CHECK, PLUS, CIRCLE),
            ("pencil", "check", "plus", "circle")
        );
    }
}
