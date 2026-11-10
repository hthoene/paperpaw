//! Keyboard layouts and the variants of letters (`é` on a long press on
//! `e`).
//!
//! The built-in layouts are static tables, so they cost no memory until
//! one is shown. The *automatic* layout is the built-in one of the UI
//! language, or else what its translation brings under `keyboard.letters`,
//! so a language pack works without code.
//!
//! Other plugins add layouts through the stage `keyboard.layouts@1` (see
//! `native_layouts`): an id (also the translation key of its name), a code of
//! at most [`MAX_CODE`] characters, letter rows separated by spaces as the
//! built-in ones (at most [`MAX_ROWS`] of at most [`MAX_KEYS`] letters), and
//! optional variants, a letter, `=` and its variants (at most
//! [`MAX_VARIANTS`]), shown before the built-in ones of the letter.

use alloc::borrow::Cow;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_ui_kit::{mem::push, text::copy};

/// The setting value (and list entry) for the UI language's layout.
pub(crate) const AUTO: &str = "keyboard.layout.auto";

/// Letter rows of an added layout, and letters in a row, at most: the
/// keyboard stays on the screen.
pub const MAX_ROWS: usize = 6;
pub const MAX_KEYS: usize = 16;
/// Characters of a layout's code, and variants of a letter, at most.
pub const MAX_CODE: usize = 4;
pub const MAX_VARIANTS: usize = 16;

/// Whether `c` shows nothing or turns text around (controls, marks of
/// direction, joiners): never a key.
pub(crate) fn invisible(c: char) -> bool {
    c.is_control()
        || matches!(c, '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{206F}' | '\u{FEFF}')
}

/// A layout chosen: the code on the key that switches layouts, the letter
/// rows, and the variants of letters it adds.
pub(crate) type Chosen = (String, Cow<'static, str>, Vec<(char, String)>);

/// A layout another plugin added (see the module's documentation).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Added {
    pub id: String,
    pub code: String,
    pub letters: String,
    pub variants: Vec<(char, String)>,
}

/// A built-in layout: its id (also the translation key of its name), the
/// short code on the key that switches layouts, and the letter rows
/// separated by spaces; letters it types by holding another (`a=ä`, as
/// added layouts give them, shown first), and its rows with keys of their
/// own for those (the setting *Umlaut keys*).
pub(crate) struct Layout {
    pub id: &'static str,
    pub code: &'static str,
    pub letters: &'static str,
    pub held: &'static str,
    pub keys: Option<&'static str>,
}

impl Layout {
    const fn new(id: &'static str, code: &'static str, letters: &'static str) -> Self {
        Self {
            id,
            code,
            letters,
            held: "",
            keys: None,
        }
    }

    /// Its letter rows, with keys of their own for the held letters where
    /// `keys` (and it has such rows).
    fn rows(&self, keys: bool) -> &'static str {
        match self.keys {
            Some(rows) if keys => rows,
            _ => self.letters,
        }
    }
}

pub(crate) const LAYOUTS: [Layout; 12] = [
    Layout::new("keyboard.layout.en", "EN", "qwertyuiop asdfghjkl zxcvbnm"),
    // Ten keys a row, as English: ä, ö, ü and ß by holding a, o, u and s.
    Layout {
        id: "keyboard.layout.de",
        code: "DE",
        letters: "qwertzuiop asdfghjkl yxcvbnm",
        held: "a=ä o=ö u=ü s=ß",
        keys: Some("qwertzuiopü asdfghjklöä yxcvbnmß"),
    },
    Layout::new("keyboard.layout.fr", "FR", "azertyuiop qsdfghjklm wxcvbn"),
    Layout::new("keyboard.layout.es", "ES", "qwertyuiop asdfghjklñ zxcvbnm"),
    Layout::new("keyboard.layout.pt", "PT", "qwertyuiop asdfghjklç zxcvbnm"),
    Layout::new(
        "keyboard.layout.sv",
        "SV",
        "qwertyuiopå asdfghjklöä zxcvbnm",
    ),
    Layout::new(
        "keyboard.layout.da",
        "DA",
        "qwertyuiopå asdfghjklæø zxcvbnm",
    ),
    Layout::new("keyboard.layout.cs", "CS", "qwertzuiopú asdfghjklů yxcvbnm"),
    Layout::new(
        "keyboard.layout.tr",
        "TR",
        "qwertyuıopğü asdfghjklşi zxcvbnmöç",
    ),
    Layout::new(
        "keyboard.layout.ru",
        "RU",
        "йцукенгшщзх фывапролджэ ячсмитьбю",
    ),
    Layout::new(
        "keyboard.layout.uk",
        "UK",
        "йцукенгшщзхї фівапролджє ячсмитьбю",
    ),
    Layout::new("keyboard.layout.el", "EL", "ςερτυθιοπ ασδφγηξκλ ζχψωβνμ"),
];

/// Letters and signs with variants, and the variants (for lower case;
/// capitals are their upper case).
const VARIANTS: [(char, &str); 30] = [
    ('a', "àáâäãåāæą"),
    ('c', "çćč"),
    ('d', "ďđð"),
    ('e', "èéêëēėęě"),
    ('g', "ğ"),
    ('i', "îïíīįìı"),
    ('l', "łľ"),
    ('n', "ñńň"),
    ('o', "ôöòóõøōœő"),
    ('r', "ř"),
    ('s', "ßśšş"),
    ('t', "ťþ"),
    ('u', "ûüùúūůű"),
    ('y', "ÿý"),
    ('z', "žźż"),
    ('е', "ё"),
    ('ь', "ъ"),
    ('г', "ґ"),
    ('и', "ії"),
    ('α', "ά"),
    ('ε', "έ"),
    ('η', "ή"),
    ('ι', "ίϊΐ"),
    ('ο', "ό"),
    ('υ', "ύϋΰ"),
    ('ω', "ώ"),
    ('-', "–—"),
    ('"', "„“”«»"),
    ('\'', "‚‘’‹›"),
    ('.', "…"),
];

/// The variants of `c` (lower case), if it has any.
pub(crate) fn variants(c: char) -> Option<&'static str> {
    VARIANTS
        .iter()
        .find(|(base, _)| *base == c)
        .map(|(_, v)| *v)
}

/// The layouts chosen in the setting `value` (ids separated by commas), as
/// their codes and letter rows: from the language (`core.language`) and the
/// translation's own rows (`keyboard.letters`, empty for none), with fallible
/// allocation. Layouts other plugins add come by the stage `keyboard.layouts`
/// (see [`chosen_with`]).
#[cfg(test)]
pub(crate) fn chosen_for(
    language: &str,
    letters: &str,
    value: &str,
    keys: bool,
) -> Result<Vec<Chosen>, paperpaw_sdk::native::Error> {
    chosen_with(language, letters, value, keys, &[])
}

/// [`chosen_for`] with the layouts other plugins added (see
/// `native_layouts`): an id in `value` that is none of the built-in ones
/// takes the one added with it. Every layout is offered.
#[cfg(test)]
pub(crate) fn chosen_with(
    language: &str,
    letters: &str,
    value: &str,
    keys: bool,
    added: &[Added],
) -> Result<Vec<Chosen>, paperpaw_sdk::native::Error> {
    chosen_drawn(language, letters, value, keys, added, &|_| true)
}

/// Answers whether the fonts draw every character of a text (`text.covers`).
pub(crate) type Drawn<'a> = &'a dyn Fn(&str) -> bool;

/// Whether the fonts draw every letter of `letters` and their capitals
/// (spaces apart): a layout in a script they lack (e.g. Cyrillic on the X4's
/// packs) is not offered, rather than shown as question marks. Asks in
/// pieces of 256 bytes and allocates nothing.
pub(crate) fn all_drawn(drawn: Drawn<'_>, letters: &str) -> bool {
    let mut piece = [0_u8; 256];
    let mut used = 0;
    let wanted = letters
        .chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(|c| core::iter::once(c).chain(c.to_uppercase()));
    for c in wanted {
        let size = c.len_utf8();
        if used + size > piece.len() {
            let Ok(text) = core::str::from_utf8(&piece[..used]) else {
                return true;
            };
            if !drawn(text) {
                return false;
            }
            used = 0;
        }
        c.encode_utf8(&mut piece[used..]);
        used += size;
    }
    match core::str::from_utf8(&piece[..used]) {
        Ok(text) => used == 0 || drawn(text),
        Err(_) => true,
    }
}

/// [`chosen_with`] where `drawn` says what the fonts draw: a layout whose
/// letters they cannot draw is left out (the layouts of the setting that
/// remain, or the automatic one), and so are the variants of an added
/// layout they cannot draw.
pub(crate) fn chosen_drawn(
    language: &str,
    letters: &str,
    value: &str,
    keys: bool,
    added: &[Added],
    drawn: Drawn<'_>,
) -> Result<Vec<Chosen>, paperpaw_sdk::native::Error> {
    let mut layouts: Vec<Chosen> = Vec::new();
    for id in value.split(',').map(str::trim).filter(|id| !id.is_empty()) {
        let one = if id == AUTO {
            Some(automatic_for(language, letters, keys)?)
        } else {
            match LAYOUTS.iter().find(|l| l.id == id) {
                Some(l) if all_drawn(drawn, l.rows(keys)) => {
                    Some(builtin_for(l, copy(l.code)?, keys)?)
                }
                Some(_) => None,
                None => match added.iter().find(|l| l.id == id) {
                    Some(l) if all_drawn(drawn, &l.letters) => {
                        let mut one = crate::native_layouts::chosen_of(l)?;
                        for (_, variants) in &mut one.2 {
                            // The whole string first; a letter at a time only
                            // where some are not drawn.
                            if !all_drawn(drawn, variants) {
                                variants.retain(|c| all_drawn(drawn, c.encode_utf8(&mut [0; 4])));
                            }
                        }
                        one.2.retain(|(_, variants)| !variants.is_empty());
                        Some(one)
                    }
                    _ => None,
                },
            }
        };
        if let Some(one) = one {
            push(&mut layouts, one)?;
        }
    }
    if layouts.is_empty() {
        push(&mut layouts, automatic_for(language, letters, keys)?)?;
    }
    Ok(layouts)
}

/// [`builtin`] with fallible allocation.
fn builtin_for(
    layout: &Layout,
    code: String,
    keys: bool,
) -> Result<Chosen, paperpaw_sdk::native::Error> {
    let mut variants: Vec<(char, String)> = Vec::new();
    for pair in layout.held.split_whitespace() {
        let Some((base, letters)) = pair.split_once('=') else {
            continue;
        };
        let mut chars = base.chars();
        let Some(base) = chars.next().filter(|_| chars.next().is_none()) else {
            continue;
        };
        let base = base.to_lowercase().next().unwrap_or(base);
        push(&mut variants, (base, copy(letters)?))?;
    }
    Ok((code, Cow::Borrowed(layout.rows(keys)), variants))
}

/// [`automatic`] from a language and the translation's rows.
fn automatic_for(
    language: &str,
    letters: &str,
    keys: bool,
) -> Result<Chosen, paperpaw_sdk::native::Error> {
    let base = language.split('-').next().unwrap_or(language);
    let mut code = String::new();
    code.try_reserve(base.len() * 2)
        .map_err(|_| paperpaw_sdk::native::Error::NoMemory)?;
    code.extend(base.chars().flat_map(char::to_uppercase));
    let layout = LAYOUTS
        .iter()
        .find(|l| l.id.rsplit('.').next() == Some(base));
    Ok(match layout {
        Some(layout) => builtin_for(layout, code, keys)?,
        None if !letters.is_empty() => (code, Cow::Owned(copy(letters)?), Vec::new()),
        None => (code, Cow::Borrowed(LAYOUTS[0].letters), Vec::new()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layouts_have_names_and_rows() {
        let en = include_str!("../i18n/en.ini");
        let de = include_str!("../i18n/de.ini");
        for layout in &LAYOUTS {
            let key = alloc::format!("{} =", layout.id);
            assert!(en.contains(&key) && de.contains(&key), "{}", layout.id);
            assert_eq!(layout.letters.split(' ').count(), 3, "{}", layout.id);
            // Every letter held has a key of its own in its other rows.
            let keys = layout.rows(true);
            assert_eq!(keys.split(' ').count(), 3, "{}", layout.id);
            for pair in layout.held.split_whitespace() {
                let letters = pair.split_once('=').unwrap().1;
                assert!(letters.chars().all(|c| keys.contains(c)), "{}", layout.id);
            }
        }
        assert!(en.contains("keyboard.layout.auto ="));
    }

    /// German has ten keys a row as English, ä, ö, ü and ß by holding a,
    /// o, u and s (first there); with *Umlaut keys* they are keys.
    #[test]
    fn german_holds_its_umlauts_or_has_keys_for_them() {
        let de = LAYOUTS.iter().find(|l| l.code == "DE").unwrap();
        let widths = |rows: &str| {
            rows.split(' ')
                .map(|r| r.chars().count())
                .collect::<Vec<_>>()
        };
        assert_eq!(widths(de.rows(false)), [10, 9, 7]);
        assert_eq!(widths(de.rows(true)), [11, 11, 8]);
        assert_eq!(de.held, "a=ä o=ö u=ü s=ß");
        assert!(
            LAYOUTS
                .iter()
                .filter(|l| l.code != "DE")
                .all(|l| l.rows(true) == l.letters)
        );
    }

    #[test]
    fn letters_have_variants() {
        assert_eq!(variants('e'), Some("èéêëēėęě"));
        assert!(variants('e').unwrap().contains('é'));
        assert_eq!(variants('q'), None);
        // Every variant is one character in upper case too, or kept.
        for (_, variants) in VARIANTS {
            assert!(!variants.is_empty());
        }
    }
    /// What a font without Cyrillic and Greek draws.
    fn latin_only(text: &str) -> bool {
        text.chars().all(|c| !matches!(c, '\u{370}'..='\u{4FF}'))
    }

    #[test]
    fn layouts_the_fonts_cannot_draw_are_left_out() {
        let all = "keyboard.layout.en, keyboard.layout.el, keyboard.layout.de";
        let kept = chosen_drawn("en", "", all, false, &[], &latin_only).unwrap();
        let codes: Vec<&str> = kept.iter().map(|l| l.0.as_str()).collect();
        assert_eq!(codes, ["EN", "DE"]);
        // Nothing left: the automatic layout.
        let none = chosen_drawn("de", "", "keyboard.layout.el", false, &[], &latin_only).unwrap();
        assert_eq!(none.len(), 1);
        assert_eq!(none[0].0, "DE");
        // A font that draws everything keeps them all.
        let every = chosen_drawn("en", "", all, false, &[], &|_| true).unwrap();
        assert_eq!(every.len(), 3);
    }

    #[test]
    fn variants_of_an_added_layout_the_fonts_cannot_draw_are_left_out() {
        let added = Added {
            id: String::from("mixed.layout"),
            code: String::from("MX"),
            letters: String::from("abc def"),
            variants: alloc::vec![('a', String::from("áαà")), ('d', String::from("δ"))],
        };
        let kept = chosen_drawn("en", "", "mixed.layout", false, &[added], &latin_only).unwrap();
        // `a` keeps its Latin variants, `d` has none left.
        assert_eq!(kept[0].2, [('a', String::from("áà"))]);
    }

    #[test]
    fn variants_all_drawn_are_asked_as_one_string() {
        use core::cell::Cell;
        let calls = Cell::new(0);
        let count = |_: &str| {
            calls.set(calls.get() + 1);
            true
        };
        let added = Added {
            id: String::from("many.layout"),
            code: String::from("MY"),
            letters: String::from("abc"),
            variants: alloc::vec![
                ('a', String::from("áàâäãå")),
                ('e', String::from("éèêë")),
                ('i', String::from("íìîï")),
            ],
        };
        let kept = chosen_drawn("en", "", "many.layout", false, &[added], &count).unwrap();
        assert_eq!(kept[0].2.len(), 3);
        // One question for the letters and one for each letter's variants.
        assert_eq!(calls.get(), 4);
    }

    #[test]
    fn an_added_layout_with_letters_the_fonts_lack_is_left_out() {
        let added = Added {
            id: String::from("greek.layout"),
            code: String::from("GR"),
            letters: String::from("αβγ δεζ"),
            variants: Vec::new(),
        };
        let kept = chosen_drawn("de", "", "greek.layout", false, &[added], &latin_only).unwrap();
        assert_eq!(kept[0].0, "DE");
    }

    #[test]
    fn letters_are_asked_in_pieces_with_their_capitals_and_without_spaces() {
        use core::cell::RefCell;
        let asked = RefCell::new(Vec::<String>::new());
        let record = |text: &str| {
            asked.borrow_mut().push(String::from(text));
            true
        };
        assert!(all_drawn(&record, "ab c"));
        assert_eq!(asked.borrow().as_slice(), ["aAbBcC"]);
        asked.borrow_mut().clear();
        // Past one piece (256 bytes) the rest follows; nothing is cut inside a
        // character.
        let long = "ж".repeat(100);
        assert!(all_drawn(&record, &long));
        let pieces = asked.borrow();
        assert!(pieces.len() >= 2);
        assert!(pieces.iter().all(|p| p.len() <= 256));
        assert_eq!(pieces.concat().chars().count(), 200);
        // The first refusal ends the asking.
        let calls = RefCell::new(0);
        let refuse = |_: &str| {
            *calls.borrow_mut() += 1;
            false
        };
        assert!(!all_drawn(&refuse, &long));
        assert_eq!(*calls.borrow(), 1);
        assert!(all_drawn(&refuse, "  "));
    }
}
