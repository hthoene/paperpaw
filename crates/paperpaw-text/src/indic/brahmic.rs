//! The Brahmic scripts shaped as Bengali and Devanagari are, beyond those
//! two (#385): what their characters are in a syllable, the parts of their
//! split vowel signs, and where their vowel signs and reph are drawn, as
//! HarfBuzz's Indic shaper places them.

use super::{Cat, POS_AFTER_MAIN, POS_AFTER_POST, POS_AFTER_SUB, POS_BEFORE_SUB, Script};

/// Where a script's reph is drawn: after the base, after the forms below
/// it, after all (the forms after it too), or at the end, before the
/// virama left standing before the base if there is one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Reph {
    AfterMain,
    AfterSub,
    AfterPost,
    End,
}

impl Script {
    /// Whether ra and a virama before `next` make a reph: before a
    /// consonant, else not (Malayalam's is a character of its own), or
    /// only before a joiner (Sinhala).
    pub(super) fn reph_before(self, next: Cat) -> bool {
        match self {
            Script::Malayalam => false,
            Script::Sinhala | Script::Telugu => next == Cat::Zwj,
            _ => next != Cat::Zwj,
        }
    }

    /// Where its reph goes.
    pub(super) const fn reph(self) -> Reph {
        match self {
            Script::Bengali => Reph::AfterSub,
            Script::Oriya | Script::Malayalam => Reph::AfterMain,
            Script::Tamil | Script::Telugu | Script::Kannada => Reph::AfterPost,
            _ => Reph::End,
        }
    }
}

/// What `ch` (of `script`, one shaped here) is in a syllable.
pub(super) fn category(script: Script, ch: char) -> Cat {
    match script {
        Script::Gujarati => gujarati(ch),
        Script::Oriya => oriya(ch),
        Script::Gurmukhi => gurmukhi(ch),
        Script::Tamil => tamil(ch),
        Script::Malayalam => malayalam(ch),
        Script::Sinhala => sinhala(ch),
        Script::Telugu => telugu(ch),
        Script::Kannada => kannada(ch),
        _ => Cat::Other,
    }
}

/// The parts a split vowel sign is drawn as (its canonical decomposition),
/// left first.
pub(super) fn parts(ch: char) -> Option<&'static [u32]> {
    Some(match ch {
        '\u{9CB}' => &[0x9C7, 0x9BE],
        '\u{9CC}' => &[0x9C7, 0x9D7],
        '\u{B48}' => &[0xB47, 0xB56],
        '\u{B4B}' => &[0xB47, 0xB3E],
        '\u{B4C}' => &[0xB47, 0xB57],
        '\u{BCA}' => &[0xBC6, 0xBBE],
        '\u{BCB}' => &[0xBC7, 0xBBE],
        '\u{BCC}' => &[0xBC6, 0xBD7],
        '\u{D4A}' => &[0xD46, 0xD3E],
        '\u{D4B}' => &[0xD47, 0xD3E],
        '\u{D4C}' => &[0xD46, 0xD57],
        '\u{DDA}' => &[0xDD9, 0xDCA],
        '\u{DDC}' => &[0xDD9, 0xDCF],
        '\u{DDD}' => &[0xDD9, 0xDCF, 0xDCA],
        '\u{DDE}' => &[0xDD9, 0xDDF],
        '\u{C48}' => &[0xC46, 0xC56],
        '\u{CC0}' => &[0xCBF, 0xCD5],
        '\u{CC7}' => &[0xCC6, 0xCD5],
        '\u{CC8}' => &[0xCC6, 0xCD6],
        '\u{CCA}' => &[0xCC6, 0xCC2],
        '\u{CCB}' => &[0xCC6, 0xCC2, 0xCD5],
        _ => return None,
    })
}

/// Where a vowel sign of category `cat` (right, above or below its
/// consonant), `ch`, is sorted to in a syllable of `script`.
pub(super) fn matra_position(script: Script, cat: Cat, ch: u32) -> u8 {
    match (script, cat) {
        (Script::Devanagari, _) => POS_AFTER_SUB,
        (Script::Bengali, Cat::BelowMatra) => POS_AFTER_SUB,
        (Script::Bengali, _) => POS_AFTER_POST,
        (Script::Oriya, Cat::AboveMatra) => POS_AFTER_MAIN,
        (Script::Oriya, Cat::BelowMatra) => POS_AFTER_SUB,
        (Script::Gujarati, Cat::AboveMatra) => POS_AFTER_SUB,
        (Script::Gujarati | Script::Gurmukhi | Script::Oriya, _) => POS_AFTER_POST,
        (Script::Tamil, Cat::AboveMatra) => POS_AFTER_SUB,
        (Script::Tamil | Script::Malayalam, _) => POS_AFTER_POST,
        (Script::Telugu, Cat::PostMatra) if ch > 0xC42 => POS_AFTER_SUB,
        (Script::Kannada, Cat::PostMatra) if (0xCC3..=0xCD6).contains(&ch) => POS_AFTER_SUB,
        (Script::Telugu | Script::Kannada, _) => POS_BEFORE_SUB,
        _ => POS_AFTER_SUB,
    }
}

/// Whether `ch`, a sign of category Modifier, takes room of its own (a
/// visarga, a spacing anusvara) rather than being drawn on its letter.
pub(super) fn spacing_modifier(ch: char) -> bool {
    matches!(
        ch,
        '\u{903}'
            | '\u{982}'
            | '\u{983}'
            | '\u{A03}'
            | '\u{A83}'
            | '\u{B02}'
            | '\u{B03}'
            | '\u{D02}'
            | '\u{D03}'
            | '\u{D82}'
            | '\u{D83}'
            | '\u{C01}'..='\u{C03}' | '\u{C82}' | '\u{C83}'
    )
}

fn gujarati(ch: char) -> Cat {
    match ch {
        '\u{A81}'..='\u{A83}' | '\u{AFA}'..='\u{AFF}' => Cat::Modifier,
        '\u{A85}'..='\u{A8D}'
        | '\u{A8F}'..='\u{A91}'
        | '\u{A93}'..='\u{A94}'
        | '\u{AE0}'..='\u{AE1}' => Cat::Vowel,
        '\u{A95}'..='\u{AA8}'
        | '\u{AAA}'..='\u{AB0}'
        | '\u{AB2}'..='\u{AB3}'
        | '\u{AB5}'..='\u{AB9}'
        | '\u{AF9}' => Cat::Consonant,
        '\u{ABC}' => Cat::Nukta,
        '\u{ABE}' | '\u{AC0}' | '\u{AC9}' | '\u{ACB}'..='\u{ACC}' => Cat::PostMatra,
        '\u{ABF}' => Cat::PreMatra,
        '\u{AC1}'..='\u{AC4}' | '\u{AE2}'..='\u{AE3}' => Cat::BelowMatra,
        '\u{AC5}' | '\u{AC7}'..='\u{AC8}' => Cat::AboveMatra,
        '\u{ACD}' => Cat::Virama,
        _ => Cat::Other,
    }
}

fn oriya(ch: char) -> Cat {
    match ch {
        '\u{B01}'..='\u{B03}' => Cat::Modifier,
        '\u{B05}'..='\u{B0C}'
        | '\u{B0F}'..='\u{B10}'
        | '\u{B13}'..='\u{B14}'
        | '\u{B60}'..='\u{B61}' => Cat::Vowel,
        '\u{B15}'..='\u{B28}'
        | '\u{B2A}'..='\u{B30}'
        | '\u{B32}'..='\u{B33}'
        | '\u{B35}'..='\u{B39}'
        | '\u{B5C}'..='\u{B5D}'
        | '\u{B5F}'
        | '\u{B71}' => Cat::Consonant,
        '\u{B3C}' => Cat::Nukta,
        '\u{B3E}' | '\u{B40}' | '\u{B57}' => Cat::PostMatra,
        '\u{B3F}' | '\u{B55}'..='\u{B56}' => Cat::AboveMatra,
        '\u{B41}'..='\u{B44}' | '\u{B62}'..='\u{B63}' => Cat::BelowMatra,
        '\u{B47}' => Cat::PreMatra,
        '\u{B48}' | '\u{B4B}'..='\u{B4C}' => Cat::SplitMatra,
        '\u{B4D}' => Cat::Virama,
        _ => Cat::Other,
    }
}

fn gurmukhi(ch: char) -> Cat {
    match ch {
        '\u{A01}'..='\u{A03}' | '\u{A51}' | '\u{A70}'..='\u{A71}' => Cat::Modifier,
        '\u{A05}'..='\u{A0A}'
        | '\u{A0F}'..='\u{A10}'
        | '\u{A13}'..='\u{A14}'
        | '\u{A72}'..='\u{A73}' => Cat::Vowel,
        '\u{A15}'..='\u{A28}'
        | '\u{A2A}'..='\u{A30}'
        | '\u{A32}'..='\u{A33}'
        | '\u{A35}'..='\u{A36}'
        | '\u{A38}'..='\u{A39}'
        | '\u{A59}'..='\u{A5C}'
        | '\u{A5E}' => Cat::Consonant,
        '\u{A3C}' => Cat::Nukta,
        '\u{A3E}' | '\u{A40}' => Cat::PostMatra,
        '\u{A3F}' => Cat::PreMatra,
        '\u{A41}'..='\u{A42}' | '\u{A75}' => Cat::BelowMatra,
        '\u{A47}'..='\u{A48}' | '\u{A4B}'..='\u{A4C}' => Cat::AboveMatra,
        '\u{A4D}' => Cat::Virama,
        _ => Cat::Other,
    }
}

fn tamil(ch: char) -> Cat {
    match ch {
        '\u{B82}' => Cat::Modifier,
        '\u{B85}'..='\u{B8A}' | '\u{B8E}'..='\u{B90}' | '\u{B92}'..='\u{B94}' => Cat::Vowel,
        '\u{B95}'
        | '\u{B99}'..='\u{B9A}'
        | '\u{B9C}'
        | '\u{B9E}'..='\u{B9F}'
        | '\u{BA3}'..='\u{BA4}'
        | '\u{BA8}'..='\u{BAA}'
        | '\u{BAE}'..='\u{BB9}' => Cat::Consonant,
        '\u{BBE}'..='\u{BBF}' | '\u{BC1}'..='\u{BC2}' | '\u{BD7}' => Cat::PostMatra,
        '\u{BC0}' => Cat::AboveMatra,
        '\u{BC6}'..='\u{BC8}' => Cat::PreMatra,
        '\u{BCA}'..='\u{BCC}' => Cat::SplitMatra,
        '\u{BCD}' => Cat::Virama,
        _ => Cat::Other,
    }
}

/// Malayalam: the chillus (U+0D7A to U+0D7F) and the dot reph stand alone.
fn malayalam(ch: char) -> Cat {
    match ch {
        '\u{D00}'..='\u{D03}' | '\u{D3B}'..='\u{D3C}' => Cat::Modifier,
        '\u{D05}'..='\u{D0C}'
        | '\u{D0E}'..='\u{D10}'
        | '\u{D12}'..='\u{D14}'
        | '\u{D5F}'..='\u{D61}' => Cat::Vowel,
        '\u{D15}'..='\u{D3A}' => Cat::Consonant,
        '\u{D3E}'..='\u{D40}' | '\u{D57}' => Cat::PostMatra,
        '\u{D41}'..='\u{D44}' | '\u{D62}'..='\u{D63}' => Cat::BelowMatra,
        '\u{D46}'..='\u{D48}' => Cat::PreMatra,
        '\u{D4A}'..='\u{D4C}' => Cat::SplitMatra,
        '\u{D4D}' => Cat::Virama,
        _ => Cat::Other,
    }
}

fn sinhala(ch: char) -> Cat {
    match ch {
        '\u{D81}'..='\u{D83}' => Cat::Modifier,
        '\u{D85}'..='\u{D96}' => Cat::Vowel,
        '\u{D9A}'..='\u{DB1}' | '\u{DB3}'..='\u{DBB}' | '\u{DBD}' | '\u{DC0}'..='\u{DC6}' => {
            Cat::Consonant
        }
        '\u{DCA}' => Cat::Virama,
        '\u{DCF}'..='\u{DD1}' | '\u{DD8}' | '\u{DDF}' | '\u{DF2}'..='\u{DF3}' => Cat::PostMatra,
        '\u{DD2}'..='\u{DD3}' => Cat::AboveMatra,
        '\u{DD4}' | '\u{DD6}' => Cat::BelowMatra,
        '\u{DD9}' | '\u{DDB}' => Cat::PreMatra,
        '\u{DDA}' | '\u{DDC}'..='\u{DDE}' => Cat::SplitMatra,
        _ => Cat::Other,
    }
}

fn telugu(ch: char) -> Cat {
    match ch {
        '\u{C00}'..='\u{C04}' => Cat::Modifier,
        '\u{C05}'..='\u{C0C}'
        | '\u{C0E}'..='\u{C10}'
        | '\u{C12}'..='\u{C14}'
        | '\u{C60}'..='\u{C61}' => Cat::Vowel,
        '\u{C15}'..='\u{C28}' | '\u{C2A}'..='\u{C39}' | '\u{C58}'..='\u{C5A}' => Cat::Consonant,
        '\u{C3C}' => Cat::Nukta,
        '\u{C3E}'..='\u{C40}' | '\u{C46}'..='\u{C47}' | '\u{C4A}'..='\u{C4C}' | '\u{C55}' => {
            Cat::AboveMatra
        }
        '\u{C41}'..='\u{C44}' => Cat::PostMatra,
        '\u{C48}' => Cat::SplitMatra,
        '\u{C56}' | '\u{C62}'..='\u{C63}' => Cat::BelowMatra,
        '\u{C4D}' => Cat::Virama,
        _ => Cat::Other,
    }
}

fn kannada(ch: char) -> Cat {
    match ch {
        '\u{C81}'..='\u{C83}' => Cat::Modifier,
        '\u{C85}'..='\u{C8C}'
        | '\u{C8E}'..='\u{C90}'
        | '\u{C92}'..='\u{C94}'
        | '\u{CE0}'..='\u{CE1}' => Cat::Vowel,
        '\u{C95}'..='\u{CA8}'
        | '\u{CAA}'..='\u{CB3}'
        | '\u{CB5}'..='\u{CB9}'
        | '\u{CDD}'..='\u{CDE}' => Cat::Consonant,
        '\u{CBC}' => Cat::Nukta,
        '\u{CBE}' | '\u{CC1}'..='\u{CC4}' | '\u{CD5}'..='\u{CD6}' => Cat::PostMatra,
        '\u{CBF}' | '\u{CC6}' | '\u{CCC}' => Cat::AboveMatra,
        '\u{CC0}' | '\u{CC7}'..='\u{CC8}' | '\u{CCA}'..='\u{CCB}' => Cat::SplitMatra,
        '\u{CE2}'..='\u{CE3}' => Cat::BelowMatra,
        '\u{CCD}' => Cat::Virama,
        _ => Cat::Other,
    }
}
