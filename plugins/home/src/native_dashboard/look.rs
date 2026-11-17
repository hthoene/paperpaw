//! The look a page is drawn in: the style, and the lines of the names of its
//! tools wrapped in it. Read once with the page and again where a setting or the
//! registry changed ([`Dashboard::forget_looks`]), not at every draw (the X4
//! draws a frame in five bands, each a call of `draw`).
use alloc::vec::Vec;
use core::cell::RefCell;

use super::{Cell, Dashboard, Error, Font, Host, Laid, Made, Rect, Snapshot, Style, Ui, icons};
use paperpaw_sdk::native::snapshot;
use paperpaw_ui_kit::mem::push;

/// The status line of a page, made and placed.
pub(super) type StatusLine = Option<(Made, Rect)>;

/// The style a page is drawn in, and the lines of each tool's name with the top
/// of its cell (hyphenation asks, text measures).
#[derive(Debug)]
pub(super) struct Look {
    pub(super) style: Style,
    pub(super) wrapped: Vec<(u16, icons::Name)>,
    /// What drawing each tool measured (one per tool, in the order of
    /// `wrapped`), kept for the later bands and draws of the page: they ask the
    /// host the same questions again otherwise.
    pub(super) shapes: RefCell<Vec<Option<icons::Geometry>>>,
    /// Whether the tools show their names.
    tool_labels: bool,
    /// Whether the rest is what the settings say now.
    pub(super) ok: bool,
}

impl Look {
    /// Reads it, in a vector of one: the look is large, and room for it may be
    /// short ([`Error::NoMemory`]).
    #[inline(never)]
    fn read(
        host: &Host<'_>,
        (font, display): (&Font<'_>, &Snapshot),
        (cells, tool_labels): (&[Cell], bool),
    ) -> Result<Vec<Self>, Error> {
        let style = Style::read(host, display, font)?;
        let (wrapped, refused) = tool_lines(host, font, &style, cells, tool_labels)?;
        let mut held = Vec::new();
        held.try_reserve_exact(1).map_err(|_| Error::NoMemory)?;
        held.push(Self {
            style,
            wrapped,
            shapes: RefCell::new(Vec::new()),
            tool_labels,
            // Hyphenation refused: the names are wrapped again at the next draw.
            ok: !refused,
        });
        Ok(held)
    }
}

impl Look {
    /// What drawing tool number `tool` measures: kept from the first draw of it
    /// in this look (see [`Look::shapes`]), measured now where it is not or no
    /// longer says what the draw needs.
    ///
    /// # Errors
    /// Font refusals and [`Error::NoMemory`].
    pub(super) fn shape_of(
        &self,
        tool: usize,
        fonts: &icons::Fonts<'_, '_>,
        cell: Rect,
        shown: &icons::Tool<'_>,
    ) -> Result<icons::Geometry, Error> {
        let kept = self.shapes.borrow().get(tool).copied().flatten();
        if let Some(shape) = kept.filter(|shape| shape.fits(fonts, shown)) {
            return Ok(shape);
        }
        let shape = icons::measure_tool(fonts, &self.style, cell, shown)?;
        let mut shapes = self.shapes.borrow_mut();
        if shapes.len() <= tool {
            let more = tool + 1 - shapes.len();
            shapes
                .try_reserve_exact(more)
                .map_err(|_| Error::NoMemory)?;
            shapes.resize(tool + 1, None);
        }
        if let Some(at) = shapes.get_mut(tool) {
            *at = Some(shape);
        }
        Ok(shape)
    }
}

impl Dashboard {
    /// The status line above the grid and the look of the page; the status line
    /// made is let go where the look fails.
    pub(super) fn status_and_look(
        host: &Host<'_>,
        ui: &Ui,
        (font, display): (&Font<'_>, &Snapshot),
        (cells, tool_labels, strip): (&[Cell], bool, Option<Rect>),
    ) -> Result<(StatusLine, Vec<Look>), Error> {
        let status = Self::status_bar(host, ui, strip)?;
        match Look::read(host, (font, display), (cells, tool_labels)) {
            Ok(look) => Ok((status, look)),
            Err(error) => {
                if let Some((made, _)) = status {
                    made.release(host, ui);
                }
                Err(error)
            }
        }
    }

    /// The look read again where a setting or the registry changed since it was
    /// read.
    pub(super) fn refresh_look(
        &self,
        host: &Host<'_>,
        font: &Font<'_>,
        laid: &Laid,
    ) -> Result<(), Error> {
        let tool_labels = match laid.look.borrow().first() {
            Some(look) if look.ok => return Ok(()),
            Some(look) => look.tool_labels,
            None => false,
        };
        let display = snapshot::capture_snapshot(host)?;
        let fresh = Look::read(host, (font, &display), (&laid.cells, tool_labels))?;
        *laid.look.borrow_mut() = fresh;
        Ok(())
    }
}

/// The lines of the name of each tool shown (with names), with the top of its
/// cell, in the order of the cells that are tools, and whether the host refused
/// a hyphenation (then the lines are not to be kept).
fn tool_lines(
    host: &Host<'_>,
    font: &Font<'_>,
    style: &Style,
    cells: &[Cell],
    tool_labels: bool,
) -> Result<(Vec<(u16, icons::Name)>, bool), Error> {
    let mut lines = Vec::new();
    let hyphens = icons::Hyphens::OFF;
    let refused = core::cell::Cell::new(false);
    let mut breaks = |word: &str, out: &mut _| hyphens.breaks_noting(host, word, out, &refused);
    for cell in cells {
        if cell.face.is_none() {
            continue;
        }
        let own = if tool_labels {
            icons::name_lines(font, style, cell.rect, &cell.name, &mut breaks)?
        } else {
            icons::Name {
                size: style.small,
                lines: Vec::new(),
            }
        };
        push(&mut lines, (cell.rect.y, own))?;
    }
    Ok((lines, refused.get()))
}
