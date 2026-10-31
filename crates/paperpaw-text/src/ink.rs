//! How the text a font handle draws is inked: the tone of its edges, a
//! dither for black-and-white frames, and room for more.

use paperpaw_graphics::Dither;

use crate::ToneCurve;

/// The ink of text a font handle draws (`FontHandle::inked` in the SDK):
/// every caller that draws with the handle, labels, status lines, widgets
/// and a plugin's own screens, inks as the handle says.
///
/// A value of private fields: made with [`TextInk::DEFAULT`] and changed
/// with the `with_` methods. Bits of `flags` that a release does not know
/// are kept and mean "as before" where clear.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextInk {
    tone: ToneCurve,
    /// Reserved: 0 is what text does today, per bit.
    flags: u32,
    /// For the edges of letters in black-and-white frames. Stored, and not
    /// read by drawing yet: [`Dither::NONE`] is today's threshold.
    dither: Dither,
    /// Zero now.
    spare: [u8; 3],
}

impl TextInk {
    /// What text is drawn with unless told otherwise: smooth, no dither,
    /// no flags.
    pub const DEFAULT: TextInk = TextInk {
        tone: ToneCurve::SMOOTH,
        flags: 0,
        dither: Dither::NONE,
        spare: [0; 3],
    };

    /// The default with `tone`.
    pub const fn new(tone: ToneCurve) -> TextInk {
        TextInk {
            tone,
            ..TextInk::DEFAULT
        }
    }

    /// With `tone`.
    #[must_use]
    pub const fn with_tone(self, tone: ToneCurve) -> TextInk {
        TextInk { tone, ..self }
    }

    /// With `dither` for the edges of letters (stored; see the type).
    #[must_use]
    pub const fn with_dither(self, dither: Dither) -> TextInk {
        TextInk { dither, ..self }
    }

    /// With `flags`, unknown bits kept.
    #[must_use]
    pub const fn with_flags(self, flags: u32) -> TextInk {
        TextInk { flags, ..self }
    }

    /// The tone of the letters' edges.
    pub const fn tone(&self) -> ToneCurve {
        self.tone
    }

    /// The dither for black-and-white frames.
    pub const fn dither(&self) -> Dither {
        self.dither
    }

    /// The reserved flags.
    pub const fn flags(&self) -> u32 {
        self.flags
    }
}

impl Default for TextInk {
    fn default() -> Self {
        TextInk::DEFAULT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_is_what_text_does_today() {
        let ink = TextInk::DEFAULT;
        assert_eq!(ink.tone(), ToneCurve::SMOOTH);
        assert_eq!((ink.dither(), ink.flags()), (Dither::NONE, 0));
        assert_eq!(TextInk::default(), ink);
        assert_eq!(TextInk::new(ToneCurve::SMOOTH), ink);
    }

    #[test]
    fn each_with_changes_one_thing_and_keeps_unknown_flag_bits() {
        let ink = TextInk::DEFAULT
            .with_tone(ToneCurve::contrast(2))
            .with_flags(0x8000_0001);
        assert_eq!(ink.tone(), ToneCurve::contrast(2));
        assert_eq!(ink.flags(), 0x8000_0001);
        let ink = ink.with_dither(Dither::ORDERED);
        assert_eq!((ink.dither(), ink.flags()), (Dither::ORDERED, 0x8000_0001));
        assert_eq!(ink.tone(), ToneCurve::contrast(2));
    }

    #[test]
    fn it_is_24_bytes_on_a_32_bit_target() {
        assert!(core::mem::size_of::<TextInk>() <= 28);
    }
}
