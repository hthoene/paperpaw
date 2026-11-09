//! The direction and language of blocks (#33): the `dir` and `lang`
//! attributes, CSS `direction`, and the book's language where they say
//! nothing; `left` and `right` alignment read against the direction;
//! inline text set apart by `dir`, `<bdi>` and `<bdo>`.

use alloc::vec::Vec;

use paperpaw_sdk::document::{DocumentError, MAX_INLINE_LANGS};
use paperpaw_sdk::lang::{Direction, Lang};
use paperpaw_sdk::xml::{Attrs, local_name, try_decode};

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

/// Whether `lang` is written right to left (see [`Lang::direction`]).
pub(crate) fn is_rtl_language(lang: Lang) -> bool {
    lang.direction() == Direction::Rtl
}

/// The direction an element sets: its `dir` attribute, else its CSS
/// `direction` (`css`).
pub(crate) fn own_dir(
    attrs: &Attrs<'_>,
    css: Option<Direction>,
) -> Result<Option<Direction>, DocumentError> {
    let dir = attrs.try_get("dir").map_err(|_| crate::fallible::OOM)?;
    Ok(dir
        .and_then(|dir| crate::css::parse_direction(&dir))
        .or(css))
}

/// The language an element sets: `xml:lang` or `lang`.
pub(crate) fn own_lang(attrs: &Attrs<'_>) -> Result<Option<Lang>, DocumentError> {
    let Some((_, value)) = attrs.iter().find(|(name, _)| local_name(name) == "lang") else {
        return Ok(None);
    };
    let value = try_decode(value).map_err(|_| crate::fallible::OOM)?;
    Ok(Some(Lang::parse(&value)).filter(|lang| !lang.is_none()))
}

/// The place of `lang` among a block's inline languages, from 1 as
/// [`TextStyle`](paperpaw_sdk::document::TextStyle) keeps it: added where
/// it is new and there is room, else 0 (the block's own).
pub(crate) fn inline_index(langs: &mut Vec<Lang>, lang: Lang) -> Result<u8, DocumentError> {
    if let Some(i) = langs.iter().position(|l| *l == lang) {
        return Ok(u8::try_from(i + 1).unwrap_or(0));
    }
    if langs.len() >= usize::from(MAX_INLINE_LANGS) {
        return Ok(0);
    }
    crate::fallible::push(langs, lang)?;
    Ok(u8::try_from(langs.len()).unwrap_or(0))
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
) -> Result<Option<(char, char)>, DocumentError> {
    if paragraphs
        || matches!(
            tag,
            "td" | "th" | "tr" | "table" | "thead" | "tbody" | "tfoot"
        )
    {
        return Ok(None);
    }
    let dir = attrs.try_get("dir").map_err(|_| crate::fallible::OOM)?;
    let dir = dir.and_then(|dir| crate::css::parse_direction(&dir));
    Ok(match (tag, dir) {
        ("bdo", Some(Direction::Rtl)) => Some(('\u{202E}', '\u{202C}')),
        ("bdo", Some(Direction::Ltr)) => Some(('\u{202D}', '\u{202C}')),
        (_, Some(Direction::Rtl)) => Some(('\u{2067}', PDI)),
        (_, Some(Direction::Ltr)) => Some(('\u{2066}', PDI)),
        ("bdi", _) | (_, Some(_)) => Some(('\u{2068}', PDI)),
        _ => None,
    })
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
    pub(super) fn open_inline(&mut self, open: char) -> Result<(), DocumentError> {
        if self.hidden > 0 {
            return Ok(());
        }
        if self.space && !self.text.is_empty() && !self.text.ends_with(['\n', ' ']) {
            self.origin = self.space_origin.take();
            self.push_styled(' ', self.space_style)?;
        }
        self.space = false;
        self.push(open)
    }

    /// Closes the text of the elements from the `open`th on, the innermost
    /// first; the closing control stays out of a block it did not open in
    /// only where that block is still empty.
    pub(super) fn close_inline(&mut self, open: usize) -> Result<(), DocumentError> {
        if self.hidden > 0 || self.text.is_empty() {
            return Ok(());
        }
        for i in (open..self.stack.len()).rev() {
            if let Some(close) = self.stack.get(i).and_then(|f| f.closer) {
                self.push(close)?;
            }
        }
        Ok(())
    }
}
