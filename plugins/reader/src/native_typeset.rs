//! The type of book pages through the host's fonts: a layout [`Typesetter`]
//! that measures with the four reading fonts (`role:reading`, `-bold`,
//! `-italic`, `-bold-italic`: the host picks the file at each call, and the
//! missing ones fall back as `screens-input.md` 4 says). Widths of single
//! characters are kept in a small table, so a page does not ask the host for
//! the same letter twice. A host that refuses a measure is not an error of the
//! layout: the width is 0 and `failed` says so, for the view to show a notice
//! instead of a page it cannot trust. Small capitals, superscripts and kerning
//! are not set yet (`PORTING-7.md`).
use alloc::vec::Vec;
use core::cell::{Cell, RefCell};
use paperpaw_sdk::{
    layout::{Face, Typesetter},
    native::{Error, Host, fonts::Font},
};

/// Relative sizes of headings 1 to 6.
pub const HEADING_SCALE: [f32; 6] = [1.6, 1.4, 1.2, 1.1, 1.0, 1.0];
/// Entries of the table of widths (a power of two).
pub const CACHE_ENTRIES: usize = 256;

/// The four reading fonts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Role {
    /// Regular text.
    Reading,
    /// Bold text.
    Bold,
    /// Italic text.
    Italic,
    /// Bold italic text.
    BoldItalic,
}

impl Role {
    /// The key the host resolves.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Reading => "role:reading",
            Self::Bold => "role:reading-bold",
            Self::Italic => "role:reading-italic",
            Self::BoldItalic => "role:reading-bold-italic",
        }
    }

    /// The role a face is set in: by its style bits only, as Beta 3 chose the
    /// font (a heading is bold where the layout's style says so).
    #[must_use]
    pub fn of(face: Face) -> Self {
        match (face.style.bold(), face.style.italic()) {
            (false, false) => Self::Reading,
            (true, false) => Self::Bold,
            (false, true) => Self::Italic,
            (true, true) => Self::BoldItalic,
        }
    }
}

/// What the typesetter asks of the fonts.
pub trait Measure {
    /// The advance of `text` in `role` at `size` pixels.
    ///
    /// # Errors
    /// The host's refusal.
    fn width(&self, role: Role, size: f32, text: &str) -> Result<f32, Error>;

    /// Ascent and descent in `role` at `size` pixels.
    ///
    /// # Errors
    /// The host's refusal.
    fn metrics(&self, role: Role, size: f32) -> Result<(f32, f32), Error>;
}

impl<M: Measure + ?Sized> Measure for &M {
    fn width(&self, role: Role, size: f32, text: &str) -> Result<f32, Error> {
        (**self).width(role, size, text)
    }

    fn metrics(&self, role: Role, size: f32) -> Result<(f32, f32), Error> {
        (**self).metrics(role, size)
    }
}

/// The host's reading fonts, held for one callback.
#[derive(Debug)]
pub struct HostFonts<'h> {
    fonts: [Font<'h>; 4],
}

impl<'h> HostFonts<'h> {
    /// The four reading fonts of the host (four of the plugin's font handles).
    ///
    /// # Errors
    /// The host's refusal (the handles are released again).
    pub fn acquire(host: &'h Host<'h>) -> Result<Self, Error> {
        Ok(Self {
            fonts: [
                Font::acquire(host, Role::Reading.key())?,
                Font::acquire(host, Role::Bold.key())?,
                Font::acquire(host, Role::Italic.key())?,
                Font::acquire(host, Role::BoldItalic.key())?,
            ],
        })
    }

    /// The font of `role`.
    #[must_use]
    pub fn font_of(&self, role: Role) -> &Font<'h> {
        &self.fonts[role as usize]
    }

    /// Gives the handles back.
    ///
    /// # Errors
    /// The host's refusal (the others are released anyway).
    pub fn release(self) -> Result<(), Error> {
        let mut result = Ok(());
        for font in self.fonts {
            if let Err(error) = font.release() {
                result = Err(error);
            }
        }
        result
    }
}

impl Measure for HostFonts<'_> {
    fn width(&self, role: Role, size: f32, text: &str) -> Result<f32, Error> {
        self.font_of(role).measure_text(size, text)
    }

    fn metrics(&self, role: Role, size: f32) -> Result<(f32, f32), Error> {
        let metrics = self.font_of(role).read_line_metrics(size)?;
        Ok((metrics.ascent, metrics.descent))
    }
}

/// One entry of the table of widths: the key, 0 for none, and the width.
type Slot = (u64, f32);

/// A layout typesetter over `M`.
#[derive(Debug)]
pub struct Faces<M> {
    measure: M,
    /// The size of body text, in pixels.
    base: f32,
    /// How wide a space is, as a part of the font's own (the reader's word spacing).
    word_spacing: f32,
    /// Ascent and line height of body text: the reader's line spacing.
    body: (f32, f32),
    table: RefCell<Vec<Slot>>,
    failed: Cell<bool>,
}

impl<M: Measure> Faces<M> {
    /// Body text of `base` pixels with the line spacing `spacing` (1.0 is the
    /// size; at least the font's own distance, below 1.0 that much of it).
    ///
    /// # Errors
    /// `Invalid` for a size or spacing that is not positive and finite, or a
    /// font that cannot be measured; `NoMemory` for the table.
    pub fn new(measure: M, base: f32, spacing: f32) -> Result<Self, Error> {
        if !(base.is_finite() && base > 0.0 && spacing.is_finite() && spacing > 0.0) {
            return Err(Error::Invalid);
        }
        let (ascent, descent) = measure.metrics(Role::Reading, base)?;
        let natural = ascent + descent;
        let line_height = (base * spacing).max(natural * spacing.min(1.0));
        let body = (ascent + (line_height - natural) / 2.0, line_height);
        if !(body.0.is_finite() && body.1.is_finite()) {
            return Err(Error::Invalid);
        }
        let mut table = Vec::new();
        table
            .try_reserve_exact(CACHE_ENTRIES)
            .map_err(|_| Error::NoMemory)?;
        table.resize(CACHE_ENTRIES, (0, 0.0));
        Ok(Self {
            measure,
            base,
            word_spacing: 1.0,
            body,
            table: RefCell::new(table),
            failed: Cell::new(false),
        })
    }

    /// The same with spaces `factor` of the font's own width (0.5 to 2.0, as the
    /// reader's word spacing setting allows).
    #[must_use]
    pub fn word_spacing(mut self, factor: f32) -> Self {
        self.word_spacing = if factor.is_finite() {
            factor.clamp(0.5, 2.0)
        } else {
            1.0
        };
        self
    }

    /// Whether the host refused a measure since the last [`take_failed`](Self::take_failed).
    #[must_use]
    pub fn failed(&self) -> bool {
        self.failed.get()
    }

    /// [`failed`](Self::failed), and forgets it.
    pub fn take_failed(&self) -> bool {
        self.failed.replace(false)
    }

    /// The fonts and sizes this typesetter measures with.
    #[must_use]
    pub const fn measure(&self) -> &M {
        &self.measure
    }

    /// The size in pixels a face is set at: the book's percent wins, else a
    /// heading's level, else the reader's size.
    #[must_use]
    pub fn size(&self, face: Face) -> f32 {
        match (face.size, face.heading) {
            (percent, _) if percent > 0 => self.base * f32::from(percent) / 100.0,
            (_, 0) => self.base,
            (_, level) => {
                let at = usize::from(level).clamp(1, HEADING_SCALE.len()) - 1;
                self.base * HEADING_SCALE[at]
            }
        }
    }

    fn refused(&self) -> f32 {
        self.failed.set(true);
        0.0
    }
}

const fn slot(key: u64) -> usize {
    (key.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 56) as usize & (CACHE_ENTRIES - 1)
}

impl<M: Measure> Typesetter for Faces<M> {
    fn advance(&self, face: Face, c: char) -> f32 {
        // What is not drawn takes no room (zero-width and control characters).
        if paperpaw_sdk::text::is_invisible(c) {
            return 0.0;
        }
        let (role, size) = (Role::of(face), self.size(face));
        let key = u64::from(c) | (role as u64) << 24 | u64::from(size.to_bits()) << 32 | 1 << 63;
        let at = slot(key);
        if let Ok(table) = self.table.try_borrow()
            && let Some(&(found, width)) = table.get(at)
            && found == key
        {
            return width;
        }
        let mut buffer = [0u8; 4];
        let width = match self.measure.width(role, size, c.encode_utf8(&mut buffer)) {
            Ok(width) if width.is_finite() => {
                if c == ' ' {
                    width * self.word_spacing
                } else {
                    width
                }
            }
            _ => return self.refused(),
        };
        if let Ok(mut table) = self.table.try_borrow_mut()
            && let Some(entry) = table.get_mut(at)
        {
            *entry = (key, width);
        }
        width
    }

    /// Body text keeps the reader's ascent and line height, scaled by the
    /// book's percent; a heading is at least 1.2 of its size high with the
    /// extra shared above and below.
    fn line(&self, face: Face) -> (f32, f32) {
        if face.heading == 0 {
            let scale = match face.size {
                0 => 1.0,
                percent => f32::from(percent) / 100.0,
            };
            return (self.body.0 * scale, self.body.1 * scale);
        }
        let size = self.size(face);
        match self.measure.metrics(Role::of(face), size) {
            Ok((ascent, descent)) if ascent.is_finite() && descent.is_finite() => {
                let natural = ascent + descent;
                let line_height = (size * 1.2).max(natural);
                (ascent + (line_height - natural) / 2.0, line_height)
            }
            _ => {
                self.refused();
                // A line that cannot be measured is 1.2 of its size high, so a page still moves on.
                (size * 0.8, size * 1.2)
            }
        }
    }
}

#[cfg(test)]
mod tests;
