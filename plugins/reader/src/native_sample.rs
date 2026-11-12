//! The sample widget `reader.sample` (Beta 3's `screen/sample.rs`): a sample
//! of text set as the reading settings say, the preview above them on their
//! settings page (`WidgetDecl::previews`, `core.group.reading`). Two short
//! paragraphs in the language of the interface (`reader.sample.1` and `.2`),
//! laid out with the reading screen's typesetter in the width of a page with
//! its side margins, two millimetres of room above and below, and drawn with
//! the page's ink. It is a fifth of the display high.
//!
//! Nothing is kept across frames: each draw reads the settings (the values on
//! show while one is chosen, as the host gives them), the texts and the fonts,
//! lays out and draws, and gives everything back. Its `core.widget` stage
//! calls (measure, place, targets) answer from the display alone.
use crate::{
    native_draw::{HostInk, draw_lines},
    native_look::Look,
    native_pages::{self, Reading},
    native_typeset::{Faces, HostFonts, Measure},
};
use alloc::string::String;
use paperpaw_sdk::{
    Color, NoStorage, Storage,
    document::{Align, Block, BlockKind, Document, DocumentError, Locator, Metadata, OffsetMap},
    layout::{LayoutContext, Page, Style, layout_page},
    native::{
        Error, Host,
        callbacks::{DrawFrame, DrawPurpose, FrameRect},
        formatting,
        reply::Reply,
        settings::{self, SettingValue},
        snapshot,
        ui::stages::WidgetCall,
    },
};
use paperpaw_ui_kit::mem::buffer;

/// The share of the display's height the sample takes: a few lines, and room
/// for the settings below it.
pub const SHARE: u16 = 5;
/// The room above and below the text, in tenths of a millimetre.
pub const PAD_TENTH_MM: u32 = 20;
/// The keys of the two paragraphs.
pub const TEXTS: [&str; 2] = ["reader.sample.1", "reader.sample.2"];
/// The room for one paragraph as the host copies it (a long translation in a
/// script of two or three bytes a letter fits).
pub const TEXT_BYTES: usize = 1024;

/// How high the sample is on a display `display_height` pixels high.
#[must_use]
pub const fn height(display_height: u16) -> u16 {
    display_height / SHARE
}

/// Where the text goes inside a frame `frame` (width, height) wide on a display
/// of `display` (width, height, dpi): the side margins of a page and the
/// width between them (no wider than the frame leaves), the room above and the
/// height left below it. `None` where no line fits.
#[must_use]
pub fn room(
    frame: (u32, u32),
    (width, height, dpi): (u16, u16, u16),
    reading: &Reading,
) -> Option<native_pages::Box> {
    // A page without status lines, as Beta 3's sample.
    let page = native_pages::content_box(width, height, dpi, &reading.with_status(0, 0))?;
    let pad = (PAD_TENTH_MM * u32::from(dpi) + 127) / 254;
    let wide = frame
        .0
        .checked_sub(2 * u32::from(page.x))?
        .min(u32::from(page.width));
    let high = frame.1.checked_sub(2 * pad)?;
    if wide < 16 || high < 1 {
        return None;
    }
    Some(native_pages::Box {
        x: page.x,
        y: u16::try_from(pad).ok()?,
        width: u16::try_from(wide).ok()?,
        height: u16::try_from(high).ok()?,
    })
}

/// The sample's text as a document: one section of paragraphs, one to a line.
#[derive(Debug, Default)]
pub struct SampleText {
    text: String,
    metadata: Metadata,
}

impl SampleText {
    /// The paragraphs `parts`, joined with a line break.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn of(parts: &[&str]) -> Result<Self, Error> {
        let len = parts.iter().map(|p| p.len() + 1).sum();
        let mut text = String::new();
        text.try_reserve_exact(len).map_err(|_| Error::NoMemory)?;
        for (n, part) in parts.iter().enumerate() {
            if n > 0 {
                text.push('\n');
            }
            text.push_str(part);
        }
        Ok(Self {
            text,
            metadata: Metadata::default(),
        })
    }

    /// The two paragraphs in the language of the interface.
    ///
    /// # Errors
    /// `NoMemory`, and the host's refusal to copy a text.
    pub fn read(host: &Host<'_>) -> Result<Self, Error> {
        let mut first = buffer(TEXT_BYTES)?;
        let mut second = buffer(TEXT_BYTES)?;
        let [one, two] = TEXTS;
        let one = formatting::copy_message(host, one, &mut first)?;
        let two = formatting::copy_message(host, two, &mut second)?;
        Self::of(&[one, two])
    }

    /// The paragraph at `offset`: its start, its end (after its line break) and
    /// its text. `None` past the text or inside a letter.
    #[must_use]
    pub fn paragraph(&self, offset: u32) -> Option<(u32, u32, &str)> {
        let at = usize::try_from(offset).ok()?;
        let before = self.text.get(..at)?;
        let after = self.text.get(at..)?;
        if after.is_empty() {
            return None;
        }
        let start = before.rfind('\n').map_or(0, |i| i + 1);
        let end = after.find('\n').map_or(self.text.len(), |i| at + i + 1);
        let body = self.text.get(start..end)?.trim_end_matches('\n');
        Some((u32::try_from(start).ok()?, u32::try_from(end).ok()?, body))
    }
}

impl Document for SampleText {
    fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    fn sections(&self) -> u32 {
        1
    }

    fn section_len(&self, _: u32) -> u32 {
        u32::try_from(self.text.len()).unwrap_or(u32::MAX)
    }

    fn block_at(&self, _: &dyn Storage, at: Locator) -> Result<Option<Block>, DocumentError> {
        if at.section > 0 {
            return Ok(None);
        }
        let Some((start, end, body)) = self.paragraph(at.offset) else {
            return Ok(None);
        };
        let mut text = String::new();
        text.try_reserve_exact(body.len())
            .map_err(|_| DocumentError::OutOfMemory)?;
        text.push_str(body);
        Ok(Some(
            Block::new(BlockKind::Paragraph, start, end, text)
                .map(OffsetMap::IDENTITY)
                .align(Align::Auto),
        ))
    }
}

/// The sample laid out in `room` with `faces`, body text `base` pixels high,
/// justified as `justify` says.
///
/// # Errors
/// `NoMemory` where the layout ran out of it, else `Invalid`.
#[inline(never)]
pub fn lay_out<M: Measure>(
    faces: &Faces<M>,
    text: &SampleText,
    room: native_pages::Box,
    (base, justify): (f32, bool),
) -> Result<Page, Error> {
    let style = Style::new(0.0, 0.0, base, justify);
    let cx = LayoutContext::new(text, &NoStorage, f32::from(room.width), &style, faces);
    layout_page(&cx, Locator::new(0, 0), f32::from(room.height)).map_err(|error| match error {
        DocumentError::OutOfMemory => Error::NoMemory,
        _ => Error::Invalid,
    })
}

fn integer(host: &Host<'_>, key: &str) -> Result<i64, Error> {
    let mut bytes = [0; 32];
    Ok(match settings::read_setting(host, key, &mut bytes)? {
        SettingValue::Int(value) => value,
        _ => 0,
    })
}

/// The reading settings now (the values on show while one is chosen).
fn reading(host: &Host<'_>) -> Result<Reading, Error> {
    let mut bytes = [0; 32];
    let justify = matches!(
        settings::read_setting(host, "core.justify", &mut bytes)?,
        SettingValue::Bool(true)
    );
    Ok(Reading::new(
        integer(host, "core.font-size")?,
        integer(host, "core.line-spacing")?,
        integer(host, "core.word-spacing")?,
        integer(host, "core.margin")?,
        integer(host, "core.margin-ends")?,
        justify,
    ))
}

/// Draws the sample in the widget's frame. Any other purpose draws nothing.
///
/// # Errors
/// The host's refusals, `NoMemory`.
#[inline(never)]
pub fn draw(host: &Host<'_>, frame: &mut DrawFrame<'_>) -> Result<(), Error> {
    if frame.purpose != DrawPurpose::Widget {
        return Ok(());
    }
    let display = snapshot::capture_snapshot(host)?;
    let reading = reading(host)?;
    let rect = frame.rect;
    let Some(room) = room(
        (rect.width, rect.height),
        (
            display.display_width(),
            display.display_height(),
            display.display_dpi(),
        ),
        &reading,
    ) else {
        return Ok(());
    };
    let text = SampleText::read(host)?;
    let look = Look::read(host, display.display_gray_levels())?;
    let base = reading.size_pt * f32::from(display.display_dpi()) / 72.0;
    let fonts = HostFonts::acquire(host)?;
    let drawn = draw_with(frame, &fonts, (&text, &reading, base), (room, look));
    let released = fonts.release();
    drawn.and(released)
}

#[inline(never)]
fn draw_with(
    frame: &mut DrawFrame<'_>,
    fonts: &HostFonts<'_>,
    (text, reading, base): (&SampleText, &Reading, f32),
    (room, look): (native_pages::Box, Look),
) -> Result<(), Error> {
    let faces =
        Faces::new(fonts, base, reading.spacing_factor())?.word_spacing(reading.word_factor());
    let page = lay_out(&faces, text, room, (base, reading.justify))?;
    let rect: FrameRect = frame.rect;
    let canvas = frame.draw_canvas()?;
    let mut ink = HostInk::new(fonts, &canvas, Color::BLACK, look);
    let origin = (
        rect.x as f32 + f32::from(room.x),
        rect.y as f32 + f32::from(room.y),
    );
    draw_lines(&mut ink, &faces, &page, origin)
}

/// Answers a `core.widget` stage call: the height (a fifth of the display, the
/// least the same), the place taken, and no targets (a tap on it does nothing).
///
/// # Errors
/// A call that is not the widget's, the host's refusal.
pub fn stage(host: &Host<'_>, payload: &[u8], reply: &mut Reply<'_>) -> Result<(), Error> {
    let call = WidgetCall::decode(payload)?;
    let mut out = [0; 64];
    let written = match call {
        WidgetCall::Measure { .. } => {
            let display = snapshot::capture_snapshot(host)?;
            let high = u32::from(height(display.display_height()));
            call.reply_size(high, high, &mut out)?
        }
        WidgetCall::Place { .. } => call.reply_place(&mut out)?,
        WidgetCall::Targets { .. } => call.reply_targets(&[], &mut out)?,
        _ => return Err(Error::Unsupported),
    };
    reply.write(out.get(..written).ok_or(Error::NoSpace)?)
}

#[cfg(test)]
mod tests;
