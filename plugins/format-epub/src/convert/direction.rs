//! The direction and language of blocks (#33): the `dir` and `lang`
//! attributes, CSS `direction`, and the book's language where they say
//! nothing; `left` and `right` alignment read against the direction;
//! inline text set apart by `dir`, `<bdi>` and `<bdo>`.

use alloc::vec::Vec;

use paperpaw_sdk::document::{Align, MAX_INLINE_LANGS};
use paperpaw_sdk::lang::{Direction, Lang};
use paperpaw_sdk::xml::{Attrs, decode, local_name};

use super::{Converter, MAX_MARGIN};

/// Pop directional isolate: ends an isolate.
const PDI: char = '\u{2069}';

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

/// How far a block is set in from the start and the end of its lines:
/// those of the blocks around it (`parent`) and its own at the left and
/// the right (`sides`), which are its end and its start where it runs
/// right to left. The body's own are the page's, set in the settings.
pub(crate) fn set_in(
    parent: (u16, u16),
    sides: (u16, u16),
    dir: Direction,
    body: bool,
) -> (u16, u16) {
    let (start, end) = match (body, dir) {
        (true, _) => (0, 0),
        (false, Direction::Rtl) => (sides.1, sides.0),
        (false, _) => sides,
    };
    (
        parent.0.saturating_add(start).min(MAX_MARGIN),
        parent.1.saturating_add(end).min(MAX_MARGIN),
    )
}

/// The controls (UAX #9) an inline element's text is set apart with, as
/// HTML does: opening and closing. `<bdo>` overrides the direction of its
/// letters; `<bdi>` and text with a `dir` attribute are isolated (CSS
/// `direction` alone does not set inline text apart). None for blocks
/// (`paragraphs`, table cells), which take the direction themselves.
pub(crate) fn inline_controls(
    tag: &str,
    attrs: &Attrs<'_>,
    paragraphs: bool,
) -> Option<(char, char)> {
    if paragraphs
        || matches!(
            tag,
            "td" | "th" | "tr" | "table" | "thead" | "tbody" | "tfoot"
        )
    {
        return None;
    }
    let dir = attrs
        .get("dir")
        .and_then(|dir| crate::css::parse_direction(&dir));
    match (tag, dir) {
        ("bdo", Some(Direction::Rtl)) => Some(('\u{202E}', '\u{202C}')),
        ("bdo", Some(Direction::Ltr)) => Some(('\u{202D}', '\u{202C}')),
        (_, Some(Direction::Rtl)) => Some(('\u{2067}', PDI)),
        (_, Some(Direction::Ltr)) => Some(('\u{2066}', PDI)),
        ("bdi", _) | (_, Some(_)) => Some(('\u{2068}', PDI)),
        _ => None,
    }
}

/// Spaces and controls that draw nothing: left out at a block's end, and
/// a block of them alone is none.
pub(crate) fn is_blank(c: char) -> bool {
    c.is_whitespace() || paperpaw_sdk::text::is_invisible(c)
}

impl Converter<'_> {
    /// Opens the text of an inline element with `open` (see
    /// [`inline_controls`]). A space before the element stays before it,
    /// outside: within, it would go to the other side of the text.
    pub(super) fn open_inline(&mut self, open: char) {
        if self.hidden > 0 {
            return;
        }
        if self.space && !self.text.is_empty() && !self.text.ends_with(['\n', ' ']) {
            self.origin = self.space_origin.take();
            self.push_styled(' ', self.space_style);
        }
        self.space = false;
        self.push(open);
    }

    /// Closes the text of the elements from the `open`th on, the innermost
    /// first; the closing control stays out of a block it did not open in
    /// only where that block is still empty.
    pub(super) fn close_inline(&mut self, open: usize) {
        if self.hidden > 0 || self.text.is_empty() {
            return;
        }
        for i in (open..self.stack.len()).rev() {
            if let Some(close) = self.stack.get(i).and_then(|f| f.closer) {
                self.push(close);
            }
        }
    }
}
