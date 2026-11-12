//! The page's own paint below the widgets: each item's plate (and its frame and
//! corner while editing), the tools with their icons and names, and the footer
//! (the dots, on a pill over the picture, the pencil or *Done* and *Add* with
//! their icons, the words where the host has no icon), as interface 6 drew them.
use super::*;
use crate::native_icons::Fonts;

impl Dashboard {
    /// Paints the items' plates and the tools, then the footer, into `below`;
    /// `plated`: the picture shows behind the page.
    #[inline(never)]
    pub(super) fn paint_page(
        &self,
        below: &mut Vec<Paint>,
        fonts: &Fonts<'_, '_>,
        style: &Style,
        (laid, plated): (&Laid, bool),
    ) -> Result<(), Error> {
        for cell in &laid.cells {
            if let Some(plate) = laid.plate {
                paint::push(below, Paint::Focus(cell.rect, style.corner, None, plate))?;
            }
            if self.editing {
                // The cells show while editing.
                let line = Some(u32::from(style.line).max(1));
                paint::push(
                    below,
                    Paint::Focus(cell.rect, style.corner, line, style.ink),
                )?;
                self.corner_mark(below, style, cell.rect, laid.handle)?;
            }
            if let (What::Tool(_), Some(face)) = (&cell.what, cell.face) {
                let lines = icons::name_lines(fonts.ui, style, cell.rect, &cell.name)?;
                icons::draw_tool(below, fonts, style, cell.rect, (&lines, face))?;
            }
        }
        self.paint_footer(below, fonts, style, (laid, plated))
    }

    /// The dots (on a pill over the picture), the pencil (on a disc of paper
    /// over the picture) or *Done*, and *Add*.
    fn paint_footer(
        &self,
        below: &mut Vec<Paint>,
        fonts: &Fonts<'_, '_>,
        style: &Style,
        (laid, plated): (&Laid, bool),
    ) -> Result<(), Error> {
        if let (Some(nav), true) = (self.nav, laid.dots || self.editing) {
            let footer = laid.frame.footer;
            if plated && let Some((rect, radius)) = dash::pill(footer, style.gap, laid.pages) {
                paint::push(below, Paint::Focus(rect, radius, None, PAPER))?;
            }
            for dot in dash::dots(footer, style.gap, laid.pages, nav.page) {
                self::dot(below, &dot, style)?;
            }
        }
        let size = style.title;
        if let Some((rect, word)) = &laid.pencil {
            match (fonts.icons, laid.icons.pencil_place(self.editing)) {
                (Some(icon_font), Some(glyph)) => {
                    // As interface 6: the tick in black, the pencil faint, on
                    // paper over the picture.
                    let ink = if self.editing {
                        Color::BLACK
                    } else {
                        if plated {
                            paint::push(below, disc(*rect))?;
                        }
                        style.note
                    };
                    paperpaw_ui_kit::icons::draw_centred(
                        below, icon_font, glyph, size, *rect, ink,
                    )?;
                }
                _ => word_in(below, fonts.ui, style, *rect, word)?,
            }
        }
        if let Some(rect) = laid.plus {
            match (fonts.icons, laid.icons.plus.as_ref()) {
                (Some(icon_font), Some(glyph)) => paperpaw_ui_kit::icons::draw_centred(
                    below,
                    icon_font,
                    glyph,
                    size,
                    rect,
                    Color::BLACK,
                )?,
                _ => word_in(below, fonts.ui, style, rect, "+")?,
            }
        }
        Ok(())
    }
}

/// The paper the pill and the pencil's disc are, over a picture (interface 6:
/// white).
const PAPER: Color = Color::WHITE;

/// The disc of paper under the pencil over a picture: as large as the button's
/// shorter side, in its middle.
fn disc(area: Rect) -> Paint {
    let side = area.width.min(area.height);
    let rect = Rect::new(
        area.x.saturating_add((area.width - side) / 2),
        area.y.saturating_add((area.height - side) / 2),
        side,
        side,
    );
    Paint::Focus(rect, u32::from(side / 2), None, PAPER)
}

/// A button's word, wrapped and centred in it.
fn word_in(
    out: &mut Vec<Paint>,
    font: &Font<'_>,
    style: &Style,
    area: Rect,
    word: &str,
) -> Result<(), Error> {
    let lines = icons::name_lines(font, style, area, word)?;
    icons::name(out, font, style, area, (&lines, style.ink))
}

/// A dot of the page indicator.
fn dot(paint: &mut Vec<Paint>, dot: &dash::Dot, style: &Style) -> Result<(), Error> {
    let size = clamp_u32(dot.size);
    let rect = Rect::new(clamp_i32(dot.x), clamp_i32(dot.y), size, size);
    let radius = dot.size / 2;
    let op = if dot.current {
        Paint::Focus(rect, radius, None, style.ink)
    } else {
        Paint::Focus(rect, radius, Some(u32::from(style.line).max(1)), style.ink)
    };
    paint::push(paint, op)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pencils_disc_is_round_and_in_the_middle_of_its_button() {
        assert!(matches!(
            disc(Rect::new(400, 740, 60, 60)),
            Paint::Focus(rect, 30, None, colour) if rect == Rect::new(400, 740, 60, 60) && colour == PAPER
        ));
        assert!(matches!(
            disc(Rect::new(300, 740, 120, 60)),
            Paint::Focus(rect, 30, None, colour) if rect == Rect::new(330, 740, 60, 60) && colour == PAPER
        ));
    }
}
