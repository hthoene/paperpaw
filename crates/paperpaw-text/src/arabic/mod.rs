//! Arabic shaping by substitution: each character of a text becomes one
//! glyph key, in logical order.
//!
//! A letter's key is its form where it stands: isolated, final, initial
//! or medial, by the joining types of its neighbours (`ArabicShaping.txt`).
//! The key is the letter's presentation form (U+FB50 to U+FEFF) where
//! Unicode has one, else a private key in plane 15 ([`PRIVATE`]) that a
//! font pack may hold; an isolated letter is the letter itself. Lam before
//! alef takes the ligature (U+FEF5 to U+FEFC) and the alef [`NO_GLYPH`],
//! so a text keeps one key per character and every byte range on it
//! stays valid. Marks keep their own key; directional and joining
//! controls draw nothing.
//!
//! The test `the_example` shapes "salam".

#[rustfmt::skip]
mod tables;

#[cfg(test)]
mod tests;

use core::str::Chars;

use crate::{bidi, is_invisible};

/// How a character joins its neighbours (`Joining_Type`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[allow(
    clippy::enum_variant_names,
    reason = "the names ArabicShaping.txt gives the types"
)]
#[non_exhaustive]
pub(crate) enum Joining {
    /// Joins neither side.
    NonJoining,
    /// Joins the character before it only (alef, dal, reh, waw).
    Right,
    /// Joins the character after it only.
    Left,
    /// Joins both sides (beh, seen, lam, meem).
    Dual,
    /// Makes both neighbours join (tatweel, ZWJ).
    Causing,
    /// Is passed over when joining (marks).
    Transparent,
}

const JOININGS: [Joining; 6] = [
    Joining::NonJoining,
    Joining::Right,
    Joining::Left,
    Joining::Dual,
    Joining::Causing,
    Joining::Transparent,
];

/// A letter's shape by where it stands in a word.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[non_exhaustive]
pub(crate) enum Form {
    /// Alone.
    Isolated,
    /// Joined to the letter before.
    Final,
    /// Joined to the letter after.
    Initial,
    /// Joined on both sides.
    Medial,
}

impl Form {
    const fn index(self) -> usize {
        match self {
            Form::Isolated => 0,
            Form::Final => 1,
            Form::Initial => 2,
            Form::Medial => 3,
        }
    }
}

/// The first private key: letter U+0600 + n in form f is
/// `PRIVATE + 4 * n + f`, f counted as [`Form`] lists them, for the
/// blocks U+0600 to U+08FF. Font packs and this module share it.
pub(crate) const PRIVATE: u32 = 0xF_0000;
/// The key of what draws nothing and takes no room: an alef inside a
/// lam-alef ligature, ZWJ, ZWNJ and the directional controls.
pub(crate) const NO_GLYPH: char = '\u{F0FFF}';
/// The blocks shaped here: Arabic, Arabic Supplement, Arabic Extended-B
/// and Arabic Extended-A.
const FIRST: u32 = 0x0600;
const LAST: u32 = 0x08FF;
const LAM: char = '\u{644}';

/// The joining type of `ch`. Outside the Arabic blocks: ZWJ causes
/// joining, marks are transparent, everything else joins nothing.
#[must_use]
pub(crate) fn joining(ch: char) -> Joining {
    let cp = u32::from(ch);
    if (FIRST..=LAST).contains(&cp) {
        let Ok(cp) = u16::try_from(cp) else {
            return Joining::NonJoining;
        };
        let at = match tables::JOINING_STARTS.binary_search(&cp) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        };
        return tables::JOINING_TYPES
            .get(at)
            .and_then(|&t| JOININGS.get(usize::from(t)))
            .copied()
            .unwrap_or(Joining::NonJoining);
    }
    match ch {
        '\u{200C}' => Joining::NonJoining,
        '\u{200D}' => Joining::Causing,
        // Zero width, and joining nothing across them, as they never did
        // (they are not in `is_invisible`'s old list, so the last arm
        // gave them NonJoining). Unicode's data says Transparent for these
        // (`DerivedJoiningType.txt` 17.0.0: `200B; T`, `2060..2064; T`), so
        // letters would join across them in a conforming shaper: this keeps
        // Arabic shaping as it was when they became invisible for line
        // breaking. A later switch to Unicode's behaviour is a plain
        // behaviour change (and a layout `VERSION` bump).
        '\u{200B}' | '\u{2060}'..='\u{2064}' => Joining::NonJoining,
        _ if is_invisible(ch) || bidi::class(ch) == bidi::Class::NSM => Joining::Transparent,
        _ => Joining::NonJoining,
    }
}

/// Whether a character of this type joins the one after it.
fn joins_next(j: Joining) -> bool {
    matches!(j, Joining::Dual | Joining::Left | Joining::Causing)
}

/// Whether a character of this type joins the one before it.
fn joins_previous(j: Joining) -> bool {
    matches!(j, Joining::Dual | Joining::Right | Joining::Causing)
}

/// The key of `letter` in `form`: its presentation form, a private key
/// where Unicode has none, or the letter itself when isolated, outside
/// the Arabic blocks, or for a letter that does not change its shape.
#[must_use]
pub(crate) fn form_key(letter: char, form: Form) -> char {
    if form == Form::Isolated {
        return letter;
    }
    let cp = u32::from(letter);
    if !(FIRST..=LAST).contains(&cp)
        || !matches!(
            joining(letter),
            Joining::Dual | Joining::Right | Joining::Left
        )
    {
        return letter;
    }
    if let Some(forms) = forms(letter) {
        let key = forms[form.index()];
        if key != 0 {
            return char::from_u32(u32::from(key)).unwrap_or(letter);
        }
    }
    let private = PRIVATE + 4 * (cp - FIRST) + form.index() as u32;
    char::from_u32(private).unwrap_or(letter)
}

fn forms(letter: char) -> Option<[u16; 4]> {
    let cp = u16::try_from(u32::from(letter)).ok()?;
    let at = tables::FORMS.binary_search_by_key(&cp, |f| f.0).ok()?;
    tables::FORMS.get(at).map(|f| f.1)
}

/// The lam-alef ligature with `alef`, joined to the letter before it or
/// not, if lam and this alef make one.
fn lam_alef(alef: char, joined: bool) -> Option<char> {
    let cp = u16::try_from(u32::from(alef)).ok()?;
    let at = tables::LAM_ALEF.binary_search_by_key(&cp, |l| l.0).ok()?;
    let key = tables::LAM_ALEF.get(at)?.1[usize::from(joined)];
    char::from_u32(u32::from(key))
}

/// The letter a key stands for and its form: what to draw where a font
/// has no glyph for the key. A lam-alef ligature gives lam; any other
/// character gives itself, isolated.
#[must_use]
pub(crate) fn letter_of(key: char) -> (char, Form) {
    let cp = u32::from(key);
    if (PRIVATE..PRIVATE + 4 * (LAST - FIRST + 1)).contains(&cp) {
        let n = cp - PRIVATE;
        let form = [Form::Isolated, Form::Final, Form::Initial, Form::Medial][(n % 4) as usize];
        return (char::from_u32(FIRST + n / 4).unwrap_or(key), form);
    }
    let Ok(short) = u16::try_from(cp) else {
        return (key, Form::Isolated);
    };
    if (0xFEF5..=0xFEFC).contains(&short) {
        let form = if short % 2 == 0 {
            Form::Final
        } else {
            Form::Isolated
        };
        return (LAM, form);
    }
    for &(letter, forms) in &tables::FORMS {
        if let Some(i) = forms.iter().position(|&f| f == short) {
            let form = [Form::Isolated, Form::Final, Form::Initial, Form::Medial][i];
            return (char::from_u32(u32::from(letter)).unwrap_or(key), form);
        }
    }
    (key, Form::Isolated)
}

/// The neighbours of a text that is part of a word: a run of another
/// style, or a line's part. Each is the nearest character outside the
/// text that is not a mark.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
#[non_exhaustive]
pub(crate) struct Context {
    /// The character before the text.
    pub(crate) before: Option<char>,
    /// The character after the text.
    pub(crate) after: Option<char>,
}

impl Context {
    /// A text with no neighbours: a whole paragraph or word.
    pub(crate) const NONE: Context = Context {
        before: None,
        after: None,
    };

    /// The neighbours `before` and `after`.
    #[must_use]
    pub(crate) const fn new(before: Option<char>, after: Option<char>) -> Context {
        Context { before, after }
    }
}

/// The keys of `text`, one per character, in logical order.
#[must_use]
pub(crate) fn keys(text: &str, context: Context) -> Keys<'_> {
    Keys {
        rest: text.chars(),
        before: context.before.map_or(Joining::NonJoining, joining),
        lam: context.before == Some(LAM),
        after: context.after,
    }
}

/// The keys of a text ([`keys`]).
#[derive(Clone, Debug)]
pub(crate) struct Keys<'a> {
    rest: Chars<'a>,
    /// The joining type of the last character that is not a mark.
    before: Joining,
    /// That character is a lam that took the ligature with the next one.
    lam: bool,
    after: Option<char>,
}

impl Keys<'_> {
    /// The joining type of the next character that is not a mark, and
    /// the character.
    fn next_letter(&self) -> (Joining, Option<char>) {
        let mut ahead = self.rest.clone();
        for ch in ahead.by_ref() {
            let j = joining(ch);
            if j != Joining::Transparent {
                return (j, Some(ch));
            }
        }
        (self.after.map_or(Joining::NonJoining, joining), self.after)
    }
}

impl Iterator for Keys<'_> {
    type Item = char;

    fn next(&mut self) -> Option<char> {
        let ch = self.rest.next()?;
        let j = joining(ch);
        if j == Joining::Transparent {
            return Some(if is_invisible(ch) { NO_GLYPH } else { ch });
        }
        let ligated = core::mem::take(&mut self.lam);
        if ligated && lam_alef(ch, false).is_some() {
            self.before = Joining::Right;
            return Some(NO_GLYPH);
        }
        if is_invisible(ch) {
            self.before = j;
            return Some(NO_GLYPH);
        }
        let previous = joins_next(self.before) && joins_previous(j);
        let (next_type, next) = self.next_letter();
        if ch == LAM
            && let Some(key) = next.and_then(|alef| lam_alef(alef, previous))
        {
            self.before = j;
            self.lam = true;
            return Some(key);
        }
        let following = joins_next(j) && joins_previous(next_type);
        let form = match (previous, following) {
            (false, false) => Form::Isolated,
            (true, false) => Form::Final,
            (false, true) => Form::Initial,
            (true, true) => Form::Medial,
        };
        self.before = j;
        Some(form_key(ch, form))
    }
}

/// Where a mark goes on its letter.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[non_exhaustive]
pub(crate) enum Side {
    /// Over the letter.
    Above,
    /// Under the letter.
    Below,
}

/// A mark's place: its side, and its step away from the letter (0 is
/// next to it, 1 on top of the mark at 0, and so on).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[non_exhaustive]
pub(crate) struct Place {
    /// Over or under.
    pub(crate) side: Side,
    /// How many marks lie between it and the letter on its side.
    pub(crate) step: u8,
}

impl Place {
    /// A place on `side`, `step` marks away from the letter.
    #[must_use]
    pub(crate) const fn new(side: Side, step: u8) -> Place {
        Place { side, step }
    }
}

const SHADDA: char = '\u{651}';
const KASRA: char = '\u{650}';
const KASRATAN: char = '\u{64D}';

/// Whether `ch` is a mark that sits on the letter before it.
#[must_use]
pub(crate) fn is_mark(ch: char) -> bool {
    joining(ch) == Joining::Transparent
}

/// The places of the marks that follow one letter, in their order
/// (without font anchors: the order a Naskh font stacks them in). Over
/// the letter, shadda and hamza come first and the vowels on top of them;
/// a kasra with a shadda goes over the letter, under the shadda. Writes
/// `min(marks, places)` places and returns their count.
pub(crate) fn place_marks(marks: &[char], places: &mut [Place]) -> usize {
    let n = marks.len().min(places.len());
    let shadda = marks.contains(&SHADDA);
    let (mut above, mut below) = (0u8, 0u8);
    // Three passes over the marks: those under a shadda, those that sit
    // on the letter itself, then the others.
    for pass in 0..3 {
        for (mark, place) in marks.iter().zip(places.iter_mut()).take(n) {
            let under_shadda = shadda && matches!(*mark, KASRA | KASRATAN);
            let inner = matches!(*mark, SHADDA | '\u{654}' | '\u{653}');
            let wanted = if under_shadda {
                0
            } else if inner {
                1
            } else {
                2
            };
            if wanted != pass {
                continue;
            }
            if under_shadda || !below_mark(*mark) {
                *place = Place {
                    side: Side::Above,
                    step: above,
                };
                above = above.saturating_add(1);
            } else {
                *place = Place {
                    side: Side::Below,
                    step: below,
                };
                below = below.saturating_add(1);
            }
        }
    }
    n
}

fn below_mark(mark: char) -> bool {
    u16::try_from(u32::from(mark)).is_ok_and(|cp| tables::BELOW.binary_search(&cp).is_ok())
}

/// The character search compares instead of `ch`, or `None` for one it
/// leaves out: marks and tatweel go; alef with hamza or madda and alef
/// wasla are alef; alef maqsura and Farsi yeh are yeh; teh marbuta is
/// heh; keheh is kaf; Arabic-Indic and Persian digits are 0 to 9.
#[must_use]
pub(crate) fn search_fold(ch: char) -> Option<char> {
    if is_mark(ch) || ch == '\u{640}' || is_invisible(ch) {
        return None;
    }
    Some(match ch {
        '\u{622}' | '\u{623}' | '\u{625}' | '\u{671}' => '\u{627}',
        '\u{649}' | '\u{6CC}' => '\u{64A}',
        '\u{629}' => '\u{647}',
        '\u{6A9}' => '\u{643}',
        '\u{660}'..='\u{669}' => char::from_u32(u32::from(ch) - 0x660 + 0x30).unwrap_or(ch),
        '\u{6F0}'..='\u{6F9}' => char::from_u32(u32::from(ch) - 0x6F0 + 0x30).unwrap_or(ch),
        other => other,
    })
}
