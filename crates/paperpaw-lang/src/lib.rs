//! What a text says about its language and direction, as small values
//! that cross between a plugin and PaperPaw: [`Lang`] (a BCP 47 language
//! tag's language, script and region), [`Direction`] and [`WritingMode`].
//!
//! A [`Lang`] keeps indices into tables of this crate. They are part of
//! the plugin interface: their entries are only ever appended
//! (`tools/lang-tables.py`), so a plugin's copy and the firmware give one
//! tag the same value.

#![no_std]

#[rustfmt::skip]
mod tables;

#[cfg(test)]
mod tests;

/// The base direction of a paragraph or a piece of text.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Direction {
    /// From the text: its first strong character (UAX #9, rules P2 and
    /// P3), left to right where it has none.
    #[default]
    Auto,
    /// Left to right.
    Ltr,
    /// Right to left.
    Rtl,
}

/// How lines run on a page.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum WritingMode {
    /// Horizontal lines, top to bottom.
    #[default]
    HorizontalTb,
    /// Vertical lines, right to left (Chinese, Japanese).
    VerticalRl,
    /// Vertical lines, left to right (Mongolian).
    VerticalLr,
}

/// A language tag in 32 bits: the language (two or three letters), the
/// script and the region of a BCP 47 tag; variants and extensions are
/// dropped. [`Lang::NONE`] is "not known".
///
/// The bits: 0 to 14 the language (three letters a to z, five bits each,
/// the first lowest; 0 where a two-letter code has no third), 15 to 22
/// the script's index + 1, 23 to 31 the region's index + 1.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Lang(u32);

const SCRIPT_SHIFT: u32 = 15;
const REGION_SHIFT: u32 = 23;
const LANGUAGE_MASK: u32 = (1 << SCRIPT_SHIFT) - 1;
const SCRIPT_MASK: u32 = 0xFF;

impl Lang {
    /// No language known.
    pub const NONE: Lang = Lang(0);

    /// The language of a BCP 47 tag (`ar`, `fa-IR`, `sr-Latn-RS`,
    /// `zh-Hant`), with its script and region where it has them and they
    /// are known. Three-letter codes of languages that have a two-letter
    /// one, and deprecated codes, give the code that replaced them (`ara`
    /// and `ar`, `per` and `fa`, `iw` and `he` are one value). Anything
    /// that is not a language code gives [`Lang::NONE`].
    #[must_use]
    pub fn parse(tag: &str) -> Lang {
        let mut parts = tag.split(['-', '_']);
        let Some(language) = parts.next().and_then(language) else {
            return Lang::NONE;
        };
        let mut value = language;
        let mut script_seen = false;
        for part in parts {
            if !script_seen && part.len() == 4 && part.bytes().all(|b| b.is_ascii_alphabetic()) {
                script_seen = true;
                if let Some(i) = index_of(tables::SCRIPTS, &title(part)) {
                    value |= (i + 1) << SCRIPT_SHIFT;
                }
                continue;
            }
            if let Some(i) = region(part) {
                value |= (i + 1) << REGION_SHIFT;
            }
            // A region ends what a `Lang` keeps (variants, extensions).
            if matches!(part.len(), 2 | 3) {
                break;
            }
        }
        Lang(value)
    }

    /// The value as it crosses: for storing and comparing.
    #[must_use]
    pub const fn to_bits(self) -> u32 {
        self.0
    }

    /// The `Lang` of [`to_bits`](Lang::to_bits)' value; `None` where an
    /// index lies past this build's tables (a value of a newer PaperPaw).
    #[must_use]
    pub const fn from_bits(bits: u32) -> Option<Lang> {
        let script = (bits >> SCRIPT_SHIFT) & SCRIPT_MASK;
        let region = bits >> REGION_SHIFT;
        if script as usize > tables::SCRIPTS.len() || region as usize > tables::REGIONS.len() {
            return None;
        }
        Some(Lang(bits))
    }

    /// Whether no language is known.
    #[must_use]
    pub const fn is_none(self) -> bool {
        self.0 & LANGUAGE_MASK == 0
    }

    /// The language code (`ar`, `fa`, `ckb`), empty for [`Lang::NONE`].
    #[must_use]
    pub fn language(&self) -> LanguageCode {
        let mut code = [0u8; 3];
        let mut len = 0;
        for (i, slot) in code.iter_mut().enumerate() {
            let letter = (self.0 >> (5 * i)) & 0x1F;
            if letter == 0 {
                break;
            }
            // 1 to 26: letters (as written by `language`).
            *slot = b'a' + (letter as u8).saturating_sub(1).min(25);
            len += 1;
        }
        LanguageCode { code, len }
    }

    /// The direction text in it is written in: right to left for the
    /// scripts written so (Arabic, Hebrew, Syriac, Thaana, N'Ko and
    /// others), by its script where it names one, else by its language
    /// (Arabic, Hebrew, Persian, Urdu, Yiddish, Pashto, Sindhi, Sorani,
    /// Dhivehi, Uyghur, Syriac); else left to right. [`Direction::Auto`]
    /// for [`Lang::NONE`].
    #[must_use]
    pub fn direction(&self) -> Direction {
        if self.is_none() && self.script().is_none() {
            return Direction::Auto;
        }
        let rtl = match self.script() {
            Some(script) => matches!(
                script,
                "Arab" | "Hebr" | "Syrc" | "Thaa" | "Nkoo" | "Adlm" | "Rohg" | "Mand" | "Samr"
            ),
            None => matches!(
                self.language().as_str(),
                "ar" | "he" | "fa" | "ur" | "yi" | "ps" | "sd" | "ckb" | "dv" | "ug" | "syr"
            ),
        };
        if rtl { Direction::Rtl } else { Direction::Ltr }
    }

    /// The script (ISO 15924, `Arab`), if the tag named a known one.
    #[must_use]
    pub fn script(&self) -> Option<&'static str> {
        let i = ((self.0 >> SCRIPT_SHIFT) & SCRIPT_MASK) as usize;
        let code = tables::SCRIPTS.get(i.checked_sub(1)?)?;
        core::str::from_utf8(code).ok()
    }

    /// The region (`IR`, `419`), if the tag named a known one.
    #[must_use]
    pub fn region(&self) -> Option<&'static str> {
        let i = (self.0 >> REGION_SHIFT) as usize;
        let code = tables::REGIONS.get(i.checked_sub(1)?)?;
        core::str::from_utf8(code).ok().map(str::trim_end)
    }

    /// This tag with `script` (ISO 15924) where it names none.
    #[must_use]
    pub fn or_script(self, script: &str) -> Lang {
        if self.script().is_some() {
            return self;
        }
        match index_of(tables::SCRIPTS, &title(script)) {
            Some(i) => Lang(self.0 | ((i + 1) << SCRIPT_SHIFT)),
            None => self,
        }
    }
}

/// A language code of a [`Lang`]: two or three lowercase letters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LanguageCode {
    code: [u8; 3],
    len: usize,
}

impl LanguageCode {
    /// The code (`ar`), empty for no language.
    #[must_use]
    pub fn as_str(&self) -> &str {
        core::str::from_utf8(self.code.get(..self.len).unwrap_or_default()).unwrap_or_default()
    }
}

/// The language bits of a code of two or three ASCII letters, canonical.
fn language(code: &str) -> Option<u32> {
    if !matches!(code.len(), 2 | 3) || !code.bytes().all(|b| b.is_ascii_alphabetic()) {
        return None;
    }
    let mut lower = [0u8; 3];
    for (slot, b) in lower.iter_mut().zip(code.bytes()) {
        *slot = b.to_ascii_lowercase();
    }
    let lower = lower.get(..code.len())?;
    let canonical = core::str::from_utf8(lower).ok().map(|code| {
        tables::CANONICAL
            .binary_search_by(|(from, _)| (*from).cmp(code))
            .ok()
            .and_then(|i| tables::CANONICAL.get(i))
            .map_or(code, |(_, to)| *to)
    })?;
    let mut bits = 0;
    for (i, b) in canonical.bytes().enumerate() {
        bits |= u32::from(b - b'a' + 1) << (5 * i);
    }
    Some(bits)
}

/// The index of a region subtag (two letters or three digits).
fn region(part: &str) -> Option<u32> {
    let key: [u8; 3] = match part.as_bytes() {
        [a, b] if a.is_ascii_alphabetic() && b.is_ascii_alphabetic() => {
            [a.to_ascii_uppercase(), b.to_ascii_uppercase(), b' ']
        }
        [a, b, c] if part.bytes().all(|d| d.is_ascii_digit()) => [*a, *b, *c],
        _ => return None,
    };
    index_of(tables::REGIONS, &key)
}

/// `code` as ISO 15924 writes it: the first letter upper case.
fn title(code: &str) -> [u8; 4] {
    let mut out = [0u8; 4];
    for (i, (slot, b)) in out.iter_mut().zip(code.bytes()).enumerate() {
        *slot = if i == 0 {
            b.to_ascii_uppercase()
        } else {
            b.to_ascii_lowercase()
        };
    }
    out
}

fn index_of<const N: usize>(table: &[[u8; N]], key: &[u8; N]) -> Option<u32> {
    table
        .iter()
        .position(|entry| entry == key)
        .and_then(|i| u32::try_from(i).ok())
}

/// The script table, in order: what the plugin interface pins
/// (`interface/lang.list`).
pub fn scripts() -> impl Iterator<Item = &'static str> {
    tables::SCRIPTS
        .iter()
        .filter_map(|s| core::str::from_utf8(s).ok())
}

/// The region table, in order: what the plugin interface pins.
pub fn regions() -> impl Iterator<Item = &'static str> {
    tables::REGIONS
        .iter()
        .filter_map(|r| core::str::from_utf8(r).ok().map(str::trim_end))
}
