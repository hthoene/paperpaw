//! What an empty list says: a picture, a title and a few lines, centred.
use super::ItemList;
use crate::{Paint, Style, paint, text};
use alloc::vec::Vec;
use paperpaw_sdk::{
    Rect,
    native::{
        Error, Host,
        callbacks::FrameRect,
        commands::{CommandSender, RedrawMode},
        fonts::{Font, IconGlyph, TextOrigin},
    },
};

pub(super) fn draw_empty(
    paint: &mut Vec<Paint>,
    font: &Font<'_>,
    style: &Style,
    area: Rect,
    (title, text): (&str, &str),
    icon: Option<IconGlyph>,
) -> Result<(), Error> {
    let room = f32::from(area.width.saturating_sub(4 * style.margin)).max(1.0);
    let head = crate::geometry::line_px(font, style.title)?;
    let line = crate::geometry::line_px(font, style.small)?;
    // What the room under the rows holds (little at the largest text on a
    // small screen): the title, then the lines of the text that fit, then
    // the icon.
    if area.height < head {
        return Ok(());
    }
    let under = area.height - head;
    let most = usize::from(under.saturating_sub(style.gap) / line.max(1)).min(6);
    let lines = text::wrap_with(
        |line| font.measure_text(style.small, line),
        text,
        room,
        most,
    )?;
    let words = if lines.is_empty() {
        0
    } else {
        style.gap + line * lines.len() as u16
    };
    // The icon over the title, twice as large as it, with a gap under it.
    let size = style.title * 2.0;
    let icon = icon.filter(|_| (size as u16).saturating_add(style.gap) <= under - words);
    let above = icon.map_or(0, |_| (size as u16).saturating_add(style.gap));
    let total = crate::geometry::clamp_u16(
        i64::from(above)
            + i64::from(head)
            + i64::from(style.gap)
            + i64::from(line) * lines.len() as i64,
    );
    let mut y = area.y.saturating_add(area.height.saturating_sub(total) / 2);
    if let Some(glyph) = icon {
        let side = size as u16;
        let x = area.x.saturating_add(area.width.saturating_sub(side) / 2);
        paint::push(
            paint,
            Paint::Icon {
                glyph,
                size,
                origin: TextOrigin::new(f32::from(x), i32::from(y) + (size * 0.86) as i32),
                color: style.ink,
            },
        )?;
        y = y.saturating_add(above);
    }
    let centred = |font: &Font<'_>, size: f32, text: &str| -> Result<f32, Error> {
        Ok((f32::from(area.width) - font.measure_text(size, text)?).max(0.0) / 2.0)
    };
    let fitted = text::fit(font, style.title, title, room)?;
    let x = centred(font, style.title, &fitted)?;
    text::label(
        paint,
        font,
        &fitted,
        style.title,
        x,
        Rect::new(0, y, area.width, head),
        style.ink,
        None,
    )?;
    y = y.saturating_add(head.saturating_add(style.gap));
    for text in &lines {
        let x = centred(font, style.small, text)?;
        text::label(
            paint,
            font,
            text,
            style.small,
            x,
            Rect::new(0, y, area.width, line),
            style.muted,
            None,
        )?;
        y = y.saturating_add(line);
    }
    Ok(())
}

/// A title, a bar `permille` thousandths full with `label` under it, and the
/// text, centred in `area`. The room of the bar and its label is returned: it
/// is all that changes while the job runs.
#[inline(never)]
pub(super) fn draw_progress(
    paint: &mut Vec<Paint>,
    font: &Font<'_>,
    style: &Style,
    area: Rect,
    progress: &Progress<'_>,
) -> Result<Rect, Error> {
    let Progress {
        title,
        label,
        permille,
        text,
    } = *progress;
    let room = f32::from(area.width.saturating_sub(4 * style.margin)).max(1.0);
    let lines = text::wrap_with(|line| font.measure_text(style.small, line), text, room, 6)?;
    let head = crate::geometry::line_px(font, style.title)?;
    let line = crate::geometry::line_px(font, style.small)?;
    let words = crate::geometry::line_px(font, style.text)?;
    let high = (style.row / 4).max(10);
    let total = crate::geometry::clamp_u16(
        i64::from(head)
            + i64::from(style.gap) * 3
            + i64::from(high)
            + i64::from(words)
            + i64::from(line) * lines.len() as i64,
    );
    let mut y = area.y.saturating_add(area.height.saturating_sub(total) / 2);
    let centred = |font: &Font<'_>, size: f32, text: &str| -> Result<f32, Error> {
        Ok((f32::from(area.width) - font.measure_text(size, text)?).max(0.0) / 2.0)
    };
    let fitted = text::fit(font, style.title, title, room)?;
    let x = centred(font, style.title, &fitted)?;
    text::label(
        paint,
        font,
        &fitted,
        style.title,
        x,
        Rect::new(0, y, area.width, head),
        style.ink,
        None,
    )?;
    y = y.saturating_add(head.saturating_add(style.gap.saturating_mul(2)));
    // The bar: the one every screen draws.
    let wide = area.width.saturating_sub(4 * style.margin).max(1);
    let bar = Rect::new(
        area.x.saturating_add((area.width - wide) / 2),
        y,
        wide,
        high,
    );
    crate::parts::progress_bar(paint, style, bar, permille, style.ink)?;
    y = y.saturating_add(high.saturating_add(style.gap));
    if !label.is_empty() {
        let x = centred(font, style.text, label)?;
        text::label(
            paint,
            font,
            label,
            style.text,
            x,
            Rect::new(0, y, area.width, words),
            style.ink,
            None,
        )?;
    }
    let band = Rect::new(
        area.x,
        bar.y.saturating_sub(2),
        area.width,
        high.saturating_add(style.gap)
            .saturating_add(words)
            .saturating_add(4),
    );
    y = y.saturating_add(words.saturating_add(style.gap));
    for text in &lines {
        let x = centred(font, style.small, text)?;
        text::label(
            paint,
            font,
            text,
            style.small,
            x,
            Rect::new(0, y, area.width, line),
            style.muted,
            None,
        )?;
        y = y.saturating_add(line);
    }
    Ok(band)
}

impl ItemList {
    /// Asks for the bar of the [`super::Footer::Progress`] drawn last, and its label,
    /// to be drawn again alone (a partial refresh of that room): `false`, and
    /// nothing asked, where the last frame drew none.
    ///
    /// # Errors
    /// Propagates command queueing errors.
    pub fn redraw_progress(&self, host: &Host<'_>) -> Result<bool, Error> {
        let Spot::Band(band) = self.spot else {
            return Ok(false);
        };
        let mut scratch = [0; 64];
        let area = FrameRect::new(
            i32::from(band.x).saturating_sub(self.origin.0),
            i32::from(band.y).saturating_sub(self.origin.1),
            u32::from(band.width),
            u32::from(band.height),
        );
        CommandSender::new(host, &mut scratch).queue_redraw_area(RedrawMode::Partial, area)?;
        Ok(true)
    }
}

/// What [`super::Footer::Progress`] shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress<'a> {
    /// Said larger.
    pub title: &'a str,
    /// The share done in words, e.g. `25 %`; empty for none.
    pub label: &'a str,
    /// How much is done, in thousandths.
    pub permille: u16,
    /// Said in small letters, wrapped.
    pub text: &'a str,
}

/// A room of the last frame that the list remembers.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) enum Spot {
    #[default]
    None,
    /// The room above the rows.
    Head(Rect),
    /// The bar and label of a [`super::Footer::Progress`] (kept where there is no head).
    Band(Rect),
}
