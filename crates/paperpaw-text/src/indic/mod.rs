//! Indic shaping: Bengali (Bangla, Assamese) and Devanagari (Hindi,
//! Marathi, Nepali, Sanskrit) set as their fonts draw them (#157), the
//! other Brahmic scripts of India and Sri Lanka the same way (#385,
//! [`brahmic`]: Gujarati, Gurmukhi, Oriya, Tamil, Telugu, Kannada,
//! Malayalam, Sinhala), and Khmer, Lao and Tibetan (#314, [`rare`]: their
//! own syllables and order, the same substitutions), and Burmese (#397,
//! [`myanmar`]).
//!
//! Text is cut into syllables ([`syllable_end`]): a consonant or vowel,
//! the consonants joined to it by a virama (hasanta, halant), its vowel
//! signs and its modifiers. Each syllable is shaped on its own, in the
//! manner of OpenType's Indic shaping for the second version of the
//! scripts (`bng2`, `dev2`):
//!
//! - the base consonant is found: the last one that has no form below or
//!   after the others (ra, ya, ba after a virama);
//! - ra and a virama at the start before another consonant are a reph;
//! - vowel signs drawn left of the consonants (ি ে ৈ, ि) move in front of
//!   them; the split vowels ো ৌ become their two parts;
//! - the pack's substitutions ([`crate::substitutions`]) apply feature by
//!   feature, each where it belongs: half forms before the base, forms
//!   below and after it after the base, the reph's at the start, the
//!   others everywhere; then the reph moves where it is drawn, and the
//!   presentation forms (`pres`, `abvs`, `blws`, `psts`, `haln` ...)
//!   apply.
//!
//! The keys of the result are the text's code points, or for glyphs that
//! have none (conjuncts, half forms, reph) the keys the pack's builder
//! gave them: [`GLYPHS`] and the font's glyph number, [`MARKS`] and it
//! for marks (drawn on the letter before them). A font without
//! substitutions (a scalable one, or a pack made before) gets the
//! reordering alone.

mod brahmic;
#[cfg(feature = "outline")]
mod clusters;
mod myanmar;
#[cfg(feature = "outline")]
pub(crate) mod pack;
mod rare;
#[cfg(test)]
mod tests;

use core::ops::Range;

use crate::anchors::Bytes;
use crate::substitutions::{Run, Substitutions};
use brahmic::Reph;

/// The first key of a glyph that has no code point: glyph `n` of the
/// font is `GLYPHS + n` (below [`MAX_GLYPH`]). Above Arabic's keys
/// (`crate::arabic::PRIVATE`, up to U+F0FFF).
#[cfg_attr(
    not(any(test, feature = "outline")),
    allow(dead_code, reason = "packs are made with the outline feature")
)]
pub(crate) const GLYPHS: u32 = 0xF_1000;
/// The first key of a mark that has no code point: `MARKS + n`.
pub(crate) const MARKS: u32 = 0xF_8000;
/// Glyph numbers a key is given for: below this.
pub(crate) const MAX_GLYPH: u32 = 0x7000;

/// The glyph number of a key without code point ([`GLYPHS`], [`MARKS`]).
#[cfg_attr(
    not(any(test, feature = "outline")),
    allow(dead_code, reason = "packs are made with the outline feature")
)]
pub(crate) fn glyph_of_key(key: char) -> Option<u16> {
    let k = u32::from(key);
    let n = if (GLYPHS..GLYPHS + MAX_GLYPH).contains(&k) {
        k - GLYPHS
    } else if (MARKS..MARKS + MAX_GLYPH).contains(&k) {
        k - MARKS
    } else {
        return None;
    };
    u16::try_from(n).ok()
}

/// The most characters a syllable takes; a longer run is cut.
const MAX_CHARS: usize = 16;
/// The most characters shaped together (whole syllables of a word).
const MAX_RUN: usize = 32;
/// The most keys those are shaped into.
pub(crate) const MAX_SHAPED: usize = 64;

const ZWNJ: char = '\u{200C}';
const ZWJ: char = '\u{200D}';

/// The scripts shaped here.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Script {
    Bengali,
    Devanagari,
    Khmer,
    Lao,
    Tibetan,
    Gujarati,
    Oriya,
    Gurmukhi,
    Tamil,
    Malayalam,
    Sinhala,
    Telugu,
    Kannada,
    Myanmar,
}

/// The scripts, in [`Script`]'s order.
const SCRIPTS: [Script; 14] = [
    Script::Bengali,
    Script::Devanagari,
    Script::Khmer,
    Script::Lao,
    Script::Tibetan,
    Script::Gujarati,
    Script::Oriya,
    Script::Gurmukhi,
    Script::Tamil,
    Script::Malayalam,
    Script::Sinhala,
    Script::Telugu,
    Script::Kannada,
    Script::Myanmar,
];

impl Script {
    /// The script of `ch`, if it is one shaped here.
    pub(crate) fn of(ch: char) -> Option<Script> {
        match ch {
            '\u{980}'..='\u{9FF}' => Some(Script::Bengali),
            '\u{900}'..='\u{97F}' => Some(Script::Devanagari),
            '\u{1780}'..='\u{17FF}' => Some(Script::Khmer),
            '\u{E80}'..='\u{EFF}' => Some(Script::Lao),
            '\u{F00}'..='\u{FFF}' => Some(Script::Tibetan),
            '\u{A80}'..='\u{AFF}' => Some(Script::Gujarati),
            '\u{B00}'..='\u{B7F}' => Some(Script::Oriya),
            '\u{A00}'..='\u{A7F}' => Some(Script::Gurmukhi),
            '\u{B80}'..='\u{BFF}' => Some(Script::Tamil),
            '\u{D00}'..='\u{D7F}' => Some(Script::Malayalam),
            '\u{D80}'..='\u{DFF}' => Some(Script::Sinhala),
            '\u{C00}'..='\u{C7F}' => Some(Script::Telugu),
            '\u{C80}'..='\u{CFF}' => Some(Script::Kannada),
            '\u{1000}'..='\u{109F}' => Some(Script::Myanmar),
            _ => None,
        }
    }

    /// Its ISO 15924 code.
    pub(crate) const fn code(self) -> &'static str {
        match self {
            Script::Bengali => "Beng",
            Script::Devanagari => "Deva",
            Script::Khmer => "Khmr",
            Script::Lao => "Laoo",
            Script::Tibetan => "Tibt",
            Script::Gujarati => "Gujr",
            Script::Oriya => "Orya",
            Script::Gurmukhi => "Guru",
            Script::Tamil => "Taml",
            Script::Malayalam => "Mlym",
            Script::Sinhala => "Sinh",
            Script::Telugu => "Telu",
            Script::Kannada => "Knda",
            Script::Myanmar => "Mymr",
        }
    }

    /// Whether its syllables are found, ordered and given their forms as
    /// Bengali's and Devanagari's are (else as [`rare`] says).
    const fn is_brahmic(self) -> bool {
        !matches!(
            self,
            Script::Khmer | Script::Lao | Script::Tibetan | Script::Myanmar
        )
    }

    const fn ra(self) -> char {
        match self {
            Script::Bengali => '\u{9B0}',
            Script::Devanagari => '\u{930}',
            Script::Gujarati => '\u{AB0}',
            Script::Oriya => '\u{B30}',
            Script::Gurmukhi => '\u{A30}',
            Script::Tamil => '\u{BB0}',
            Script::Malayalam => '\u{D30}',
            Script::Sinhala => '\u{DBB}',
            Script::Telugu => '\u{C30}',
            Script::Kannada => '\u{CB0}',
            _ => '\0',
        }
    }

    const fn virama(self) -> u32 {
        match self {
            Script::Bengali => 0x9CD,
            Script::Devanagari => 0x94D,
            Script::Gujarati => 0xACD,
            Script::Oriya => 0xB4D,
            Script::Gurmukhi => 0xA4D,
            Script::Tamil => 0xBCD,
            Script::Malayalam => 0xD4D,
            Script::Sinhala => 0xDCA,
            Script::Telugu => 0xC4D,
            Script::Kannada => 0xCCD,
            _ => 0,
        }
    }
}

/// What a character is in a syllable.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Cat {
    /// Not part of a syllable: digits, punctuation, other scripts.
    Other,
    Consonant,
    /// An independent vowel: a syllable's base like a consonant.
    Vowel,
    Nukta,
    Virama,
    /// A vowel sign drawn left of its consonant.
    PreMatra,
    AboveMatra,
    BelowMatra,
    /// Right of it.
    PostMatra,
    /// A vowel sign of two parts, left and right (Bengali o, au).
    SplitMatra,
    /// Candrabindu, anusvara, visarga and the like.
    Modifier,
    Zwj,
    Zwnj,
    /// A medial consonant sign (Burmese ya, ra, wa, ha).
    Medial,
    /// A tone mark (Burmese dot below, visarga).
    Tone,
}

impl Cat {
    const fn index(self) -> u8 {
        self as u8
    }

    fn from_index(i: u8) -> Cat {
        CATS.get(usize::from(i)).copied().unwrap_or(Cat::Other)
    }

    const fn is_matra(self) -> bool {
        matches!(
            self,
            Cat::PreMatra | Cat::AboveMatra | Cat::BelowMatra | Cat::PostMatra | Cat::SplitMatra
        )
    }

    const fn is_base(self) -> bool {
        matches!(self, Cat::Consonant | Cat::Vowel)
    }
}

const CATS: [Cat; 15] = [
    Cat::Other,
    Cat::Consonant,
    Cat::Vowel,
    Cat::Nukta,
    Cat::Virama,
    Cat::PreMatra,
    Cat::AboveMatra,
    Cat::BelowMatra,
    Cat::PostMatra,
    Cat::SplitMatra,
    Cat::Modifier,
    Cat::Zwj,
    Cat::Zwnj,
    Cat::Medial,
    Cat::Tone,
];

/// What `ch` is in a syllable of `script`.
fn category(script: Script, ch: char) -> Cat {
    match ch {
        ZWJ => return Cat::Zwj,
        ZWNJ => return Cat::Zwnj,
        _ => {}
    }
    if Script::of(ch) != Some(script) {
        return Cat::Other;
    }
    match script {
        Script::Bengali => match ch {
            '\u{981}'..='\u{983}' | '\u{9FE}' => Cat::Modifier,
            '\u{985}'..='\u{98C}'
            | '\u{98F}'..='\u{990}'
            | '\u{993}'..='\u{994}'
            | '\u{9E0}'..='\u{9E1}' => Cat::Vowel,
            '\u{995}'..='\u{9A8}'
            | '\u{9AA}'..='\u{9B0}'
            | '\u{9B2}'
            | '\u{9B6}'..='\u{9B9}'
            | '\u{9CE}'
            | '\u{9DC}'..='\u{9DD}'
            | '\u{9DF}'
            | '\u{9F0}'..='\u{9F1}' => Cat::Consonant,
            '\u{9BC}' => Cat::Nukta,
            '\u{9BE}' | '\u{9C0}' | '\u{9D7}' => Cat::PostMatra,
            '\u{9BF}' | '\u{9C7}'..='\u{9C8}' => Cat::PreMatra,
            '\u{9C1}'..='\u{9C4}' | '\u{9E2}'..='\u{9E3}' => Cat::BelowMatra,
            '\u{9CB}'..='\u{9CC}' => Cat::SplitMatra,
            '\u{9CD}' => Cat::Virama,
            _ => Cat::Other,
        },
        Script::Devanagari => match ch {
            '\u{900}'..='\u{903}' | '\u{951}'..='\u{954}' => Cat::Modifier,
            '\u{904}'..='\u{914}' | '\u{960}'..='\u{961}' | '\u{972}'..='\u{977}' => Cat::Vowel,
            '\u{915}'..='\u{939}' | '\u{958}'..='\u{95F}' | '\u{978}'..='\u{97F}' => Cat::Consonant,
            '\u{93C}' => Cat::Nukta,
            '\u{93A}' | '\u{945}'..='\u{948}' | '\u{955}' => Cat::AboveMatra,
            '\u{93B}' | '\u{93E}' | '\u{940}' | '\u{949}'..='\u{94C}' | '\u{94F}' => Cat::PostMatra,
            '\u{93F}' | '\u{94E}' => Cat::PreMatra,
            '\u{941}'..='\u{944}' | '\u{956}'..='\u{957}' | '\u{962}'..='\u{963}' => {
                Cat::BelowMatra
            }
            '\u{94D}' => Cat::Virama,
            _ => Cat::Other,
        },
        Script::Khmer | Script::Lao | Script::Tibetan | Script::Myanmar => {
            rare::category(script, ch)
        }
        _ => brahmic::category(script, ch),
    }
}

/// Whether `text` has letters of a script shaped here: a quick look at
/// its bytes (in UTF-8, U+0900 to U+0DFF are `E0 A4` to `E0 B7`, Lao and
/// Tibetan U+0E80 to U+0FFF `E0 BA` to `E0 BF`, Khmer `E1 9E` and `E1 9F`,
/// Myanmar U+1000 to U+109F `E1 80` to `E1 82`).
pub fn has_indic(text: &str) -> bool {
    !text.is_ascii()
        && text.as_bytes().windows(2).any(|w| match w[0] {
            0xE0 => matches!(w[1], 0xA4..=0xB7 | 0xBA..=0xBF),
            0xE1 => matches!(w[1], 0x80..=0x82 | 0x9E..=0x9F),
            _ => false,
        })
}

/// Whether `text` has letters or signs of a script shaped here (not only
/// its dandas and digits): what the substitutions of a pack apply to.
pub(crate) fn has_letters(text: &str) -> bool {
    text.chars()
        .any(|c| Script::of(c).is_some_and(|s| category(s, c) != Cat::Other))
}

/// Whether `key` is drawn on the letter before it: a mark that takes no
/// room of its own (a vowel sign below or above, a nukta, a virama,
/// candrabindu), or a glyph without code point the pack calls a mark.
pub(crate) fn is_mark_key(key: char) -> bool {
    let k = u32::from(key);
    if (MARKS..MARKS + MAX_GLYPH).contains(&k) {
        return true;
    }
    let Some(script) = Script::of(key) else {
        return false;
    };
    if !script.is_brahmic() {
        return rare::is_mark(script, key, category(script, key));
    }
    match category(script, key) {
        Cat::Nukta | Cat::Virama | Cat::BelowMatra | Cat::AboveMatra => true,
        // Visarga and some anusvaras take room; candrabindu does not.
        Cat::Modifier => !brahmic::spacing_modifier(key),
        _ => false,
    }
}

/// Where the syllable that starts at byte `start` of `text` ends; one
/// character for a character outside a syllable.
pub(crate) fn syllable_end(text: &str, start: usize) -> usize {
    let Some(rest) = text.get(start..) else {
        return text.len();
    };
    let mut chars = rest.char_indices().peekable();
    let Some((_, first)) = chars.next() else {
        return start;
    };
    let one = start + first.len_utf8();
    let Some(script) = Script::of(first) else {
        return one;
    };
    let cat = |ch: char| category(script, ch);
    if !script.is_brahmic() {
        return rare_syllable_end(rest, start, script, cat(first));
    }
    let mut end = one;
    let mut taken = 1;
    // Takes the next character where `want` says; its category.
    let mut take = |want: &dyn Fn(Cat) -> bool,
                    chars: &mut core::iter::Peekable<core::str::CharIndices<'_>>|
     -> Option<Cat> {
        let &(at, ch) = chars.peek()?;
        let c = cat(ch);
        if taken >= MAX_CHARS || !want(c) {
            return None;
        }
        chars.next();
        taken += 1;
        end = start + at + ch.len_utf8();
        Some(c)
    };
    match cat(first) {
        Cat::Consonant | Cat::Vowel => {
            take(&|c| c == Cat::Nukta, &mut chars);
            // Consonants joined by a virama, maybe with a joiner before or
            // after it.
            loop {
                let mut ahead = chars.clone().map(|(_, c)| cat(c));
                if (ahead.next(), ahead.next()) == (Some(Cat::Zwj), Some(Cat::Virama)) {
                    take(&|c| c == Cat::Zwj, &mut chars);
                }
                if take(&|c| c == Cat::Virama, &mut chars).is_none() {
                    break;
                }
                if take(&|c| c == Cat::Zwnj, &mut chars).is_some() {
                    break;
                }
                take(&|c| c == Cat::Zwj, &mut chars);
                if take(&|c| c == Cat::Consonant, &mut chars).is_none() {
                    break;
                }
                take(&|c| c == Cat::Nukta, &mut chars);
            }
            while take(&|c| c.is_matra() || c == Cat::Nukta, &mut chars).is_some() {}
            take(&|c| c == Cat::Virama, &mut chars);
            while take(&|c| c == Cat::Modifier, &mut chars).is_some() {}
        }
        Cat::Other => {}
        // A sign without a letter: with the signs after it.
        _ => {
            while take(
                &|c| c.is_matra() || matches!(c, Cat::Nukta | Cat::Virama | Cat::Modifier),
                &mut chars,
            )
            .is_some()
            {}
        }
    }
    end
}

/// [`syllable_end`] in Khmer, Lao or Tibetan: `rest` the text from the
/// syllable's first character (of category `first`) on.
fn rare_syllable_end(rest: &str, start: usize, script: Script, first: Cat) -> usize {
    let mut before = first;
    let mut end = start;
    for (n, (at, ch)) in rest.char_indices().enumerate() {
        let cat = category(script, ch);
        if n > 0
            && (n >= MAX_CHARS || first == Cat::Other || !rare::takes(script, first, before, cat))
        {
            break;
        }
        before = cat;
        end = start + at + ch.len_utf8();
    }
    end
}

/// Where the run of syllables shaped together that starts at byte
/// `start` ends: the syllables of one script that follow each other (a
/// word), whole, up to [`MAX_RUN`] characters; one character outside the
/// scripts shaped here.
pub(crate) fn run_end(text: &str, start: usize) -> usize {
    let script = text
        .get(start..)
        .and_then(|t| t.chars().next())
        .and_then(Script::of);
    let mut end = syllable_end(text, start);
    let Some(script) = script else {
        return end;
    };
    let mut chars = text.get(start..end).map_or(0, |t| t.chars().count());
    while let Some(next) = text.get(end..).and_then(|t| t.chars().next())
        && Script::of(next) == Some(script)
    {
        let after = syllable_end(text, end);
        let n = text.get(end..after).map_or(0, |t| t.chars().count());
        if after <= end || chars + n > MAX_RUN {
            break;
        }
        chars += n;
        end = after;
    }
    end
}

/// The part of `text` around byte `at` that is drawn as one, as wide as
/// it is drawn, not as its characters: the syllables of a Bengali or
/// Devanagari word, which take forms by those around them; a Khmer, Lao
/// or Tibetan syllable (lines break between them, inside words). Outside
/// these scripts, the character at `at`.
pub fn shaped_at(text: &str, at: usize) -> Range<usize> {
    let at = at.min(text.len());
    let mut start = at;
    // Back to where the run of the script's characters starts (a word).
    let head = text.get(..at).unwrap_or_default();
    for (i, ch) in head.char_indices().rev().take(16 * MAX_CHARS) {
        if Script::of(ch).is_none() && !matches!(ch, ZWJ | ZWNJ) {
            break;
        }
        start = i;
    }
    let whole = text
        .get(at..)
        .and_then(|t| t.chars().next())
        .and_then(Script::of)
        .is_none_or(Script::is_brahmic);
    while start < text.len() {
        let end = match whole {
            true => run_end(text, start),
            false => syllable_end(text, start),
        };
        if end > at || end <= start {
            return start..end.max(start + 1).min(text.len());
        }
        start = end;
    }
    at..at
}

// Positions in a syllable, the order they are sorted in before the forms
// apply (as OpenType's Indic shaping numbers them).
const POS_REPH: u8 = 1;
const POS_PRE_M: u8 = 2;
const POS_PRE_C: u8 = 3;
const POS_BASE: u8 = 4;
const POS_AFTER_MAIN: u8 = 5;
const POS_BEFORE_SUB: u8 = 7;
const POS_BELOW_C: u8 = 8;
const POS_AFTER_SUB: u8 = 9;
const POS_POST_C: u8 = 11;
const POS_AFTER_POST: u8 = 12;
const POS_SMVD: u8 = 14;

// The masks: where a feature applies.
const ALL: u8 = 1;
const RPHF: u8 = 2;
const HALF: u8 = 4;
const POST: u8 = 8;
const INIT: u8 = 16;
/// Not a mask: the glyph is the reph.
const IS_REPH: u8 = 128;
/// Not a mask: the glyph may take a form drawn before the base (`pref`).
const IS_PREF: u8 = 32;

/// A glyph being shaped: what its character was, where it goes, where
/// features apply.
#[derive(Clone, Copy, Default, Debug)]
struct Info {
    cat: u8,
    pos: u8,
    mask: u8,
}

/// The keys of a syllable being shaped.
pub(crate) struct Glyphs {
    keys: [u32; MAX_SHAPED],
    info: [Info; MAX_SHAPED],
    len: usize,
    /// The mask of the feature applied now.
    mask: u8,
}

impl Default for Glyphs {
    fn default() -> Self {
        Glyphs {
            keys: [0; MAX_SHAPED],
            info: [Info::default(); MAX_SHAPED],
            len: 0,
            mask: ALL,
        }
    }
}

impl Glyphs {
    /// The keys, in the order they are drawn; joiners left out.
    pub(crate) fn keys(&self) -> impl Iterator<Item = char> + '_ {
        self.keys[..self.len]
            .iter()
            .filter_map(|&k| char::from_u32(k))
            .filter(|k| !matches!(*k, ZWJ | ZWNJ))
    }

    fn cat(&self, i: usize) -> Cat {
        self.info
            .get(i)
            .map_or(Cat::Other, |i| Cat::from_index(i.cat))
    }

    fn push(&mut self, key: u32, cat: Cat) {
        if self.len < MAX_SHAPED {
            self.keys[self.len] = key;
            self.info[self.len] = Info {
                cat: cat.index(),
                pos: 0,
                mask: ALL,
            };
            self.len += 1;
        }
    }

    /// Moves the glyph at `from` to `to` (both below `len`), those
    /// between one step over.
    fn move_glyph(&mut self, from: usize, to: usize) {
        if from >= self.len || to >= self.len || from == to {
            return;
        }
        let (key, info) = (self.keys[from], self.info[from]);
        if from < to {
            self.keys.copy_within(from + 1..=to, from);
            self.info.copy_within(from + 1..=to, from);
        } else {
            self.keys.copy_within(to..from, to + 1);
            self.info.copy_within(to..from, to + 1);
        }
        self.keys[to] = key;
        self.info[to] = info;
    }
}

impl Run for Glyphs {
    fn len(&self) -> usize {
        self.len
    }
    fn key(&self, i: usize) -> u32 {
        self.keys.get(i).copied().unwrap_or(0)
    }
    fn may(&self, i: usize) -> bool {
        self.info
            .get(i)
            .is_some_and(|info| info.mask & self.mask != 0)
    }
    fn set(&mut self, i: usize, key: u32) {
        if i < self.len {
            self.keys[i] = key;
        }
    }
    fn join(&mut self, i: usize, n: usize, key: u32) {
        if n == 0 || i >= self.len || n > self.len - i {
            return;
        }
        let mut info = self.info[i];
        if self.info[i..i + n].iter().any(|g| g.pos == POS_BASE) {
            info.pos = POS_BASE;
        }
        if self.mask == RPHF {
            info.mask |= IS_REPH;
        }
        self.keys[i] = key;
        self.info[i] = info;
        self.keys.copy_within(i + n..self.len, i + 1);
        self.info.copy_within(i + n..self.len, i + 1);
        self.len -= n - 1;
    }
    fn split(&mut self, i: usize, keys: &[u32]) -> bool {
        if keys.is_empty() || i >= self.len || self.len - 1 + keys.len() > MAX_SHAPED {
            return false;
        }
        let n = keys.len();
        let info = self.info[i];
        self.keys.copy_within(i + 1..self.len, i + n);
        self.info.copy_within(i + 1..self.len, i + n);
        self.keys[i..i + n].copy_from_slice(keys);
        self.info[i..i + n].fill(info);
        self.len += n - 1;
        true
    }
    fn bring(&mut self, from: usize, to: usize) {
        if to < from {
            self.move_glyph(from, to);
        }
    }
}

/// The features that apply before the reph and the vowel signs find
/// their places, each with its mask; then all others (presentation forms)
/// everywhere, `init` at a word's start.
const BASIC: [(&[u8; 4], u8); 11] = [
    (b"nukt", ALL),
    (b"akhn", ALL),
    (b"rphf", RPHF),
    (b"rkrf", ALL),
    (b"pref", POST),
    (b"blwf", POST),
    (b"abvf", POST),
    (b"half", HALF),
    (b"pstf", POST),
    (b"vatu", ALL),
    (b"cjct", ALL),
];

/// Features that apply before the syllable is sorted.
const FIRST: [&[u8; 4]; 2] = [b"ccmp", b"locl"];

fn basic_mask(tag: &[u8; 4]) -> Option<u8> {
    BASIC.iter().find(|(t, _)| *t == tag).map(|(_, mask)| *mask)
}

/// Shapes syllables with a pack's substitutions (or none).
pub(crate) struct Shaper<'a, B: Bytes + ?Sized> {
    table: Option<Substitutions<'a, B>>,
    /// The scripts' numbers in the table (index in
    /// `paperpaw_lang::scripts()` and 1), in [`Script`]'s order.
    numbers: [u8; SCRIPTS.len()],
}

/// The number a table gives `script`.
fn number(script: Script) -> u8 {
    paperpaw_lang::scripts()
        .position(|s| s == script.code())
        .and_then(|i| u8::try_from(i + 1).ok())
        .unwrap_or(0)
}

impl<'a, B: Bytes + ?Sized> Shaper<'a, B> {
    pub(crate) fn new(table: Option<Substitutions<'a, B>>) -> Self {
        Shaper {
            table,
            numbers: SCRIPTS.map(number),
        }
    }

    fn number(&self, script: Script) -> u8 {
        self.numbers[script as usize]
    }

    /// Applies the features of `script` that `pick` gives a mask for, in
    /// the table's order.
    fn features(&self, script: Script, glyphs: &mut Glyphs, pick: impl Fn(&[u8; 4]) -> Option<u8>) {
        let Some(table) = self.table else {
            return;
        };
        let number = self.number(script);
        for feature in table.features() {
            if !feature.on_by_default() || !feature.applies(number, "") {
                continue;
            }
            if let Some(mask) = pick(&feature.tag) {
                glyphs.mask = mask;
                feature.apply(glyphs);
            }
        }
        glyphs.mask = ALL;
    }

    /// Whether the forms below or after a base (`blwf`, `pstf`) draw
    /// `virama` and `consonant` as something else: then that consonant is
    /// not the base.
    fn below_or_after(&self, script: Script, consonant: u32) -> bool {
        let mut pair = Glyphs::default();
        pair.push(script.virama(), Cat::Virama);
        pair.push(consonant, Cat::Consonant);
        self.features(script, &mut pair, |tag| {
            matches!(tag, b"blwf" | b"pstf" | b"pref").then_some(ALL)
        });
        pair.len != 2 || pair.keys[0] != script.virama() || pair.keys[1] != consonant
    }

    /// Whether ra and a virama make a reph in the table.
    fn has_reph(&self, script: Script) -> bool {
        let mut pair = Glyphs::default();
        pair.push(u32::from(script.ra()), Cat::Consonant);
        pair.push(script.virama(), Cat::Virama);
        self.features(script, &mut pair, |tag| (tag == b"rphf").then_some(ALL));
        pair.len != 2 || pair.keys[0] != u32::from(script.ra())
    }

    /// Shapes `run` (as [`run_end`] cuts them) into `out`: each syllable,
    /// then the presentation forms over them all; `word_start` where it
    /// begins a word.
    pub(crate) fn shape(&self, run: &str, word_start: bool, out: &mut Glyphs) {
        *out = Glyphs::default();
        let mut one = Glyphs::default();
        let mut at = 0;
        let mut script = None;
        while at < run.len() {
            let end = syllable_end(run, at);
            let Some(syllable) = run.get(at..end).filter(|s| !s.is_empty()) else {
                break;
            };
            script = script.or_else(|| syllable.chars().next().and_then(Script::of));
            self.syllable(syllable, word_start && at == 0, &mut one);
            for i in 0..one.len {
                if out.len < MAX_SHAPED {
                    out.keys[out.len] = one.keys[i];
                    out.info[out.len] = one.info[i];
                    out.len += 1;
                }
            }
            at = end;
        }
        if let Some(script) = script {
            let whole = matches!(script, Script::Lao | Script::Tibetan);
            self.features(script, out, |tag| match tag {
                b"init" => Some(INIT),
                _ if FIRST.contains(&tag) && whole => Some(ALL),
                _ if FIRST.contains(&tag) || basic_mask(tag).is_some() => None,
                _ => Some(ALL),
            });
        }
    }

    /// Shapes `syllable` (as [`syllable_end`] cuts them) into `out` up to
    /// its presentation forms.
    fn syllable(&self, syllable: &str, word_start: bool, out: &mut Glyphs) {
        *out = Glyphs::default();
        let Some(script) = syllable.chars().next().and_then(Script::of) else {
            for ch in syllable.chars() {
                out.push(u32::from(ch), Cat::Other);
            }
            return;
        };
        if !script.is_brahmic() {
            for ch in syllable.chars() {
                rare::push(script, ch, out);
            }
            // Lao's and Tibetan's forms look across syllables: over the run.
            if script == Script::Khmer {
                self.features(script, out, |tag| FIRST.contains(&tag).then_some(ALL));
                rare::khmer_order(out);
                self.features(script, out, basic_mask);
            } else if script == Script::Myanmar {
                self.features(script, out, |tag| FIRST.contains(&tag).then_some(ALL));
                myanmar::order(out);
                self.features(script, out, |tag| {
                    matches!(tag, b"rphf" | b"pref" | b"blwf" | b"pstf").then_some(ALL)
                });
            }
            return;
        }
        for ch in syllable.chars() {
            // The split vowels as their parts.
            match brahmic::parts(ch) {
                Some(parts) => {
                    for &part in parts {
                        let cat = char::from_u32(part).map_or(Cat::Other, |c| category(script, c));
                        out.push(part, cat);
                    }
                }
                None => out.push(u32::from(ch), category(script, ch)),
            }
        }
        self.features(script, out, |tag| FIRST.contains(&tag).then_some(ALL));
        self.place(script, out);
        // Sorted by position, stably.
        for i in 1..out.len {
            let mut j = i;
            while j > 0 && out.info[j - 1].pos > out.info[j].pos {
                out.keys.swap(j - 1, j);
                out.info.swap(j - 1, j);
                j -= 1;
            }
        }
        self.features(script, out, basic_mask);
        self.reorder(script, out);
        if word_start && out.info[0].pos == POS_PRE_M {
            out.info[0].mask |= INIT;
        }
    }

    /// Finds the reph and the base, and gives every glyph its position and
    /// masks.
    fn place(&self, script: Script, out: &mut Glyphs) {
        let n = out.len;
        let virama = |out: &Glyphs, i: usize| out.cat(i) == Cat::Virama;
        let reph = n > 2
            && out.keys[0] == u32::from(script.ra())
            && virama(out, 1)
            && script.reph_before(out.cat(2))
            && (2..n).any(|i| out.cat(i) == Cat::Consonant)
            && self.has_reph(script);
        let first = if reph { 2 } else { 0 };
        // The consonants end where the vowel signs begin.
        let end = (first..n)
            .find(|&i| out.cat(i).is_matra() || out.cat(i) == Cat::Modifier)
            .unwrap_or(n);
        let mut base = (first..end).rev().find(|&i| out.cat(i).is_base());
        while let Some(b) = base {
            // The virama before it (a joiner may stand between).
            let mut v = b;
            while v > first && matches!(out.cat(v - 1), Cat::Zwj | Cat::Nukta) {
                v -= 1;
            }
            if v == first || !virama(out, v - 1) || self.table.is_none() {
                break;
            }
            if !self.below_or_after(script, out.keys[b]) {
                break;
            }
            match (first..v - 1).rev().find(|&i| out.cat(i).is_base()) {
                Some(before) => base = Some(before),
                None => break,
            }
        }
        let base = base.unwrap_or(first.min(n.saturating_sub(1)));
        let (mut post, mut pref) = (POS_POST_C, 0);
        for i in (0..n).rev() {
            let cat = out.cat(i);
            let info = &mut out.info[i];
            (info.pos, info.mask) = match cat {
                _ if reph && i < 2 => (POS_REPH, ALL | RPHF),
                Cat::PreMatra => (POS_PRE_M, ALL),
                Cat::SplitMatra | Cat::PostMatra | Cat::AboveMatra | Cat::BelowMatra => {
                    (brahmic::matra_position(script, cat, out.keys[i]), ALL)
                }
                // Oriya's candrabindu goes where a vowel sign above it does.
                Cat::Modifier if script == Script::Oriya && out.keys[i] == 0xB01 => {
                    (POS_AFTER_MAIN, ALL)
                }
                Cat::Modifier => (POS_SMVD, ALL),
                _ if i >= end => (POS_AFTER_SUB, ALL),
                _ if i < base => (POS_PRE_C, ALL | HALF),
                _ if i == base => (POS_BASE, ALL),
                // After the base: a consonant and the virama before it go
                // together, below or after.
                _ => {
                    if cat == Cat::Consonant {
                        post = match self.takes(script, out.keys[i], b"blwf", true) {
                            true => POS_BELOW_C,
                            false => POS_POST_C,
                        };
                        pref = match self.takes(script, out.keys[i], b"pref", false) {
                            true => IS_PREF,
                            false => 0,
                        };
                    }
                    (post, ALL | POST | pref)
                }
            };
        }
        // A nukta goes where its letter goes.
        for i in 1..n {
            if out.cat(i) == Cat::Nukta {
                out.info[i] = Info {
                    cat: out.info[i].cat,
                    ..out.info[i - 1]
                };
            }
        }
    }

    /// Whether `consonant` after a virama takes the form of feature `tag`
    /// (`blwf`: below the base, else after it; `pref`: before it);
    /// `without` where the pack has no substitutions.
    fn takes(&self, script: Script, consonant: u32, tag: &[u8; 4], without: bool) -> bool {
        let Some(table) = self.table else {
            return without;
        };
        let mut pair = Glyphs::default();
        pair.push(script.virama(), Cat::Virama);
        pair.push(consonant, Cat::Consonant);
        let number = self.number(script);
        for feature in table.features() {
            if feature.tag == *tag && feature.applies(number, "") {
                feature.apply(&mut pair);
            }
        }
        pair.len != 2 || pair.keys[1] != consonant
    }

    /// A consonant's form drawn before the base (`pref`, Malayalam's ra
    /// after a virama) moves there, after a vowel sign drawn left.
    fn reorder_pref(out: &mut Glyphs) {
        let Some(base) = (0..out.len).find(|&i| out.info[i].pos == POS_BASE) else {
            return;
        };
        let formed = |key: u32| char::from_u32(key).is_some_and(|k| glyph_of_key(k).is_some());
        if let Some(i) =
            (base + 1..out.len).find(|&i| out.info[i].mask & IS_PREF != 0 && formed(out.keys[i]))
        {
            out.move_glyph(i, base);
        }
    }

    /// After the forms: the reph and a vowel sign drawn left move where
    /// they are drawn.
    fn reorder(&self, script: Script, out: &mut Glyphs) {
        let virama = |out: &Glyphs, i: usize| out.keys[i] == script.virama();
        let base = (0..out.len).find(|&i| out.info[i].pos == POS_BASE);
        if out.len > 1 && out.info[0].mask & IS_REPH != 0 {
            let base = base.unwrap_or(1).max(1);
            // After a virama left standing before the base, else as the
            // script draws it.
            let left = match script.reph() {
                Reph::AfterPost => None,
                _ => (1..base).find(|&i| virama(out, i)),
            };
            let target = match left {
                Some(v) if matches!(out.cat(v + 1), Cat::Zwj | Cat::Zwnj) => v + 1,
                Some(v) => v,
                None => match script.reph() {
                    Reph::AfterMain => {
                        let mut t = base;
                        while t + 1 < out.len && out.info[t + 1].pos <= POS_AFTER_MAIN {
                            t += 1;
                        }
                        t
                    }
                    Reph::AfterSub => {
                        let mut t = base;
                        while t + 1 < out.len
                            && !matches!(
                                out.info[t + 1].pos,
                                POS_POST_C | POS_AFTER_POST | POS_SMVD
                            )
                        {
                            t += 1;
                        }
                        t
                    }
                    _ => {
                        let mut t = out.len - 1;
                        while t > base && out.info[t].pos == POS_SMVD {
                            t -= 1;
                        }
                        t
                    }
                },
            };
            out.move_glyph(0, target.min(out.len - 1));
        }
        // A vowel sign drawn left: after the last virama left standing
        // before the base, so that it is drawn by the consonant it is read
        // with.
        let base = (0..out.len).find(|&i| out.info[i].pos == POS_BASE);
        let Some(base) = base else {
            return;
        };
        let pre: usize = (0..base)
            .take_while(|&i| out.info[i].pos == POS_PRE_M || out.info[i].mask & IS_REPH != 0)
            .count();
        if let Some(matra) = (0..pre).rev().find(|&i| out.info[i].pos == POS_PRE_M)
            && let Some(v) = (matra + 1..base).rev().find(|&i| virama(out, i))
        {
            let to = match out.cat(v + 1) {
                Cat::Zwj | Cat::Zwnj => v + 1,
                _ => v,
            };
            out.move_glyph(matra, to);
        }
        Self::reorder_pref(out);
    }
}
