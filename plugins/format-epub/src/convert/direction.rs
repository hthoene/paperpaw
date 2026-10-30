//! The direction and language of blocks (#33): the `dir` and `lang`
//! attributes, CSS `direction`, and the book's language where they say
//! nothing; `left` and `right` alignment read against the direction.

use alloc::vec::Vec;

use paperpaw_sdk::document::{Align, MAX_INLINE_LANGS};
use paperpaw_sdk::lang::{Direction, Lang};
use paperpaw_sdk::xml::{Attrs, decode, local_name};

/// What a book's chapters start with: its language, and the direction of
/// its text where nothing else says it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct BookText {
    pub lang: Lang,
    pub dir: Direction,
}

impl BookText {
    /// For a book in `language` (its `dc:language`): right to left where
    /// that language is written so, else from each block's text.
    pub(crate) fn of(language: Option<&str>) -> BookText {
        let lang = language.map_or(Lang::NONE, Lang::parse);
        let dir = match is_rtl_language(lang) {
            true => Direction::Rtl,
            false => Direction::Auto,
        };
        BookText { lang, dir }
    }
}

/// Whether `lang` is written right to left: by its script where it names
/// one, else by its language.
pub(crate) fn is_rtl_language(lang: Lang) -> bool {
    match lang.script() {
        Some(script) => matches!(
            script,
            "Arab" | "Hebr" | "Syrc" | "Thaa" | "Nkoo" | "Adlm" | "Rohg" | "Mand" | "Samr"
        ),
        None => matches!(
            lang.language().as_str(),
            "ar" | "he" | "fa" | "ur" | "yi" | "ps" | "sd" | "ckb" | "dv" | "ug" | "syr"
        ),
    }
}

/// The direction an element sets: its `dir` attribute, else its CSS
/// `direction` (`css`).
pub(crate) fn own_dir(attrs: &Attrs<'_>, css: Option<Direction>) -> Option<Direction> {
    attrs
        .get("dir")
        .and_then(|dir| crate::css::parse_direction(&dir))
        .or(css)
}

/// The language an element sets: `xml:lang` or `lang`.
pub(crate) fn own_lang(attrs: &Attrs<'_>) -> Option<Lang> {
    attrs
        .iter()
        .find(|(name, _)| local_name(name) == "lang")
        .map(|(_, value)| Lang::parse(&decode(value)))
        .filter(|lang| !lang.is_none())
}

/// `align` of a block running right to left (`rtl`) or not: given as
/// `left` or `right` (`sides`), the start of a right-to-left line is at
/// the right.
pub(crate) fn resolve_align(align: Align, sides: bool, rtl: bool) -> Align {
    match (sides && rtl, align) {
        (true, Align::Start) => Align::End,
        (true, Align::End) => Align::Start,
        _ => align,
    }
}

/// The place of `lang` among a block's inline languages, from 1 as
/// [`TextStyle`](paperpaw_sdk::document::TextStyle) keeps it: added where
/// it is new and there is room, else 0 (the block's own).
pub(crate) fn inline_index(langs: &mut Vec<Lang>, lang: Lang) -> u8 {
    if let Some(i) = langs.iter().position(|l| *l == lang) {
        return u8::try_from(i + 1).unwrap_or(0);
    }
    if langs.len() >= usize::from(MAX_INLINE_LANGS) {
        return 0;
    }
    langs.push(lang);
    u8::try_from(langs.len()).unwrap_or(0)
}

/// A direction as the cache keeps it.
pub(crate) const fn dir_bits(dir: Direction) -> u8 {
    match dir {
        Direction::Ltr => 1,
        Direction::Rtl => 2,
        _ => 0,
    }
}

/// A direction from the cache.
pub(crate) const fn dir_from_bits(bits: u8) -> Direction {
    match bits {
        1 => Direction::Ltr,
        2 => Direction::Rtl,
        _ => Direction::Auto,
    }
}
