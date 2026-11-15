//! How letters are set: the space between them, the pairs set closer
//! (kerning) and the pairs joined into one (ligatures), as Beta 3 set them.
//! The host's text calls kern what they draw and measure at once, so the
//! layout learns a pair's kerning from the width of the pair against its
//! letters, kept in a small table; a ligature is the one letter U+FB00 to
//! U+FB04 the plugin draws in place of its letters, where the font has it.
use super::{
    Faces, Measure, Role, small_capital,
    tables::{Kerning, kerns, quantised},
};
use paperpaw_sdk::layout::Face;

/// The widest letter spacing, in percent of the text size (`core.letter-spacing`).
pub const MAX_SPACING: i8 = 20;
/// The tightest letter spacing.
pub const MIN_SPACING: i8 = -5;
/// A ligature of three letters or of `ff` is drawn where it takes the room of
/// its letters apart, within this many pixels (Beta 3's).
const LIGATURE_FIT: f32 = 0.5;

/// The reading settings of the type's letters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Letters {
    /// The space after each letter, in percent of the text size.
    pub spacing: i8,
    /// Pairs set closer as the font says.
    pub kerning: bool,
    /// `fi`, `fl` and `ff` joined where the font has the letter.
    pub ligatures: bool,
}

impl Letters {
    /// What the settings are until the user changes them: no spacing, kerning
    /// and ligatures on.
    pub const DEFAULT: Self = Self {
        spacing: 0,
        kerning: true,
        ligatures: true,
    };

    /// The settings as they were read, the spacing kept to what it allows.
    #[must_use]
    pub fn new(spacing: i64, kerning: bool, ligatures: bool) -> Self {
        let kept = spacing.clamp(i64::from(MIN_SPACING), i64::from(MAX_SPACING));
        Self {
            spacing: i8::try_from(kept).unwrap_or(0),
            kerning,
            ligatures,
        }
    }
}

/// The letter a pair is joined into, if it is one.
const fn ligature(left: char, right: char) -> Option<char> {
    match (left, right) {
        ('f', 'i') => Some('\u{fb01}'),
        ('f', 'l') => Some('\u{fb02}'),
        _ => None,
    }
}

/// The place of a ligature letter in the table of what the fonts have.
const fn ligature_index(c: char) -> usize {
    (c as u32 - 0xfb00) as usize
}

impl<M: Measure> Faces<M> {
    /// The same with the letters set as `letters` says.
    #[must_use]
    pub fn with_letters(mut self, letters: Letters) -> Self {
        self.spacing = self.base * f32::from(letters.spacing) / 100.0;
        self.letters = letters;
        self
    }

    /// The letters as they are set.
    #[must_use]
    pub const fn letters(&self) -> Letters {
        self.letters
    }

    /// The space after each letter of `face`: the reader's and the book's for
    /// the run (in fortieths of its size).
    #[must_use]
    pub fn spacing(&self, face: Face) -> f32 {
        match face.style.letter_spacing() {
            0 => self.spacing,
            steps => {
                let step =
                    self.size(face) / f32::from(paperpaw_sdk::document::LETTER_SPACING_STEPS);
                self.spacing + f32::from(steps) * step
            }
        }
    }

    /// Whether letters of `face` are joined: never where they are spaced (they
    /// are drawn one by one).
    #[must_use]
    pub fn joins(&self, face: Face) -> bool {
        self.letters.ligatures && self.letters.spacing == 0 && face.style.letter_spacing() == 0
    }

    /// Whether a word in `face` is drawn as the host draws a word (at once, its
    /// pairs kerned): kerned, not spaced, and without a letter that may join.
    #[must_use]
    pub fn drawn_at_once(&self, face: Face, word: &str) -> bool {
        self.letters.kerning
            && self.spacing(face).to_bits() == 0
            && !(self.joins(face) && word.contains('f'))
    }

    /// Whether the font of `role` has the ligature `c`, asked once.
    fn has(&self, role: Role, c: char) -> bool {
        let at = (role as usize * 5 + ligature_index(c)) * 2;
        let known = self.have.get() >> at & 3;
        if known != 0 {
            return known == 1;
        }
        let has = self.measure.covers(role, c);
        self.have
            .set(self.have.get() | (if has { 1u64 } else { 2 }) << at);
        has
    }

    /// How much `right` moves when it follows `left` set by the font alone: its
    /// kerning, as the host's text calls set a pair.
    fn host_kern(&self, role: Role, size: f32, (left, right): (char, char)) -> f32 {
        // A pair the tables cannot keep is not asked at every use.
        match self.tables.kerning(role, size, (left, right)) {
            Kerning::LeftOut => return 0.0,
            Kerning::Known(kern) => return kern,
            Kerning::Unknown => {}
        }
        let mut both = [0u8; 8];
        let len = left.encode_utf8(&mut both).len();
        let end = len + right.encode_utf8(&mut both[len..]).len();
        let pair = core::str::from_utf8(&both[..end]).unwrap_or("");
        let measured = self.measure.width(role, size, pair);
        let kern = match measured {
            Ok(width) if width.is_finite() => {
                width - self.width_of(role, size, left) - self.width_of(role, size, right)
            }
            _ => return self.refused(),
        };
        // A kern is a pixel or two; anything wider is a font that is not to be
        // believed. It is kept as the table keeps it, so a pair is the same
        // whether it was asked of the host or found.
        let kern = if kern.abs() < size { kern } else { 0.0 };
        let kern = f32::from(quantised(kern)) / 64.0;
        self.tables.keep_pair(role, size, (left, right), kern);
        kern
    }

    /// How much `right` moves when it follows `left` in `face`: the ligature's
    /// room where they join, else the font's kerning if it is on.
    fn pair(&self, face: Face, role: Role, size: f32, (left, right): (char, char)) -> f32 {
        if self.joins(face)
            && let Some(joined) = ligature(left, right)
            && self.has(role, joined)
        {
            let apart = self.width_of(role, size, left) + self.width_of(role, size, right);
            return self.width_of(role, size, joined) - apart;
        }
        match self.letters.kerning {
            true => self.host_kern(role, size, (left, right)),
            false => 0.0,
        }
    }

    /// The kerning of a pair as the host draws it in one run: the font's alone,
    /// whatever the settings say.
    #[must_use]
    pub fn font_kern(&self, face: Face, left: char, right: char) -> f32 {
        self.set_pair(face, (left, right), false)
    }

    /// What the layout adds between two letters of `face`.
    pub(super) fn set_pair(&self, face: Face, (left, right): (char, char), laid: bool) -> f32 {
        let wanted = !laid || self.letters.kerning || self.letters.ligatures;
        if !wanted
            || left == ' '
            || right == ' '
            || paperpaw_sdk::text::is_invisible(left)
            || paperpaw_sdk::text::is_invisible(right)
            || !kerns(left)
            || !kerns(right)
        {
            return 0.0;
        }
        let (role, size) = (Role::of(face), self.size(face));
        // In small capitals a pair of lower-case letters is a pair of capitals
        // at the smaller size; the others are drawn apart.
        if face.style.small_caps() && (left.is_lowercase() || right.is_lowercase()) {
            if left.is_lowercase() != right.is_lowercase() {
                return 0.0;
            }
            let (Some(last), Some(first)) =
                (left.to_uppercase().last(), right.to_uppercase().next())
            else {
                return 0.0;
            };
            let (_, small) = small_capital('a', true, size);
            return self.host_kern(role, small, (last, first));
        }
        match laid {
            true => self.pair(face, role, size, (left, right)),
            false => self.host_kern(role, size, (left, right)),
        }
    }

    /// The ligature the start of `text` (an `f`) is drawn as: its letter, the
    /// bytes of `text` it stands for and the width of them with the pairs
    /// between; `None` where the `f` is drawn alone (the font lacks the letter,
    /// one of three letters or `ff` would not take their room, or the next `f`
    /// joins the letter after it).
    #[must_use]
    pub fn ligature_at(&self, face: Face, text: &str) -> Option<(char, usize, f32)> {
        let (role, size) = (Role::of(face), self.size(face));
        let mut chars = text.chars();
        let (Some('f'), Some(second)) = (chars.next(), chars.next()) else {
            return None;
        };
        let width = |c: char| self.width_of(role, size, c);
        if let Some(joined) = ligature('f', second) {
            let both = width('f') + self.pair(face, role, size, ('f', second)) + width(second);
            return self.has(role, joined).then_some((joined, 2, both));
        }
        if second != 'f' {
            return None;
        }
        let two = width('f') + self.pair(face, role, size, ('f', 'f')) + width('f');
        let fits = |joined: char, wide: f32| {
            self.has(role, joined) && (width(joined) - wide).abs() <= LIGATURE_FIT
        };
        if let Some(third @ ('i' | 'l')) = chars.next() {
            let three = two + self.pair(face, role, size, ('f', third)) + width(third);
            let joined = if third == 'i' { '\u{fb03}' } else { '\u{fb04}' };
            if fits(joined, three) {
                return Some((joined, 3, three));
            }
            if ligature('f', third).is_some_and(|l| self.has(role, l)) {
                return None;
            }
        }
        fits('\u{fb00}', two).then_some(('\u{fb00}', 2, two))
    }

    /// The width of `c` in `face` without the space after it, small capitals
    /// included.
    #[must_use]
    pub fn bare_advance(&self, face: Face, c: char) -> f32 {
        let (shown, size) = small_capital(c, face.style.small_caps(), self.size(face));
        self.width_of(Role::of(face), size, shown)
    }
}
