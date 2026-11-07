//! Keyboard layouts and the variants of letters (`é` on a long press on
//! `e`).
//!
//! The built-in layouts are static tables, so they cost no memory until
//! one is shown. The *automatic* layout is the built-in one of the UI
//! language, or else what its translation brings under `keyboard.letters`,
//! so a language pack works without code.
//!
//! Other plugins add layouts to the point [`POINT`] (`keyboard.layouts`),
//! a line each: `id; code; letter rows; variants`, e.g.
//! `dutch.layout; NL; qwertyuiop asdfghjkl zxcvbnm; e=éèë i=ïí`. The id is
//! also the translation key of its name; the code has at most
//! [`MAX_CODE`] characters; the rows are separated by spaces, as the
//! built-in ones, at most [`MAX_ROWS`] of at most [`MAX_KEYS`] letters;
//! the variants (optional) are a letter, `=` and its variants (at most
//! [`MAX_VARIANTS`]), shown before the built-in ones of the letter.

use alloc::borrow::Cow;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::Env;
use paperpaw_ui_kit::{mem::push, text::copy};

/// The setting value (and list entry) for the UI language's layout.
pub(crate) const AUTO: &str = "keyboard.layout.auto";

/// The point other plugins add layouts to (see the module's
/// documentation).
pub const POINT: &str = "keyboard.layouts";

/// Letter rows of an added layout, and letters in a row, at most: the
/// keyboard stays on the screen.
pub const MAX_ROWS: usize = 6;
pub const MAX_KEYS: usize = 16;
/// Characters of a layout's code, and variants of a letter, at most.
pub const MAX_CODE: usize = 4;
pub const MAX_VARIANTS: usize = 16;

/// Whether `c` shows nothing or turns text around (controls, marks of
/// direction, joiners): never a key.
fn invisible(c: char) -> bool {
    c.is_control()
        || matches!(c, '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{206F}' | '\u{FEFF}')
}

/// A layout chosen: the code on the key that switches layouts, the letter
/// rows, and the variants of letters it adds.
pub(crate) type Chosen = (String, Cow<'static, str>, Vec<(char, String)>);

/// A layout another plugin added (see [`POINT`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Added {
    pub id: String,
    pub code: String,
    pub letters: String,
    pub variants: Vec<(char, String)>,
}

/// The layouts other plugins added, in the order of the plugins; lines
/// that say too little are left out.
pub(crate) fn added(env: &Env<'_>) -> Vec<Added> {
    let mut added: Vec<Added> = Vec::new();
    for mut layout in env
        .services()
        .contributions(POINT)
        .flat_map(|(_, text)| text.lines())
        .filter_map(parse)
    {
        // Not one of the built-in ids, nor one added before.
        let id = layout.id.as_str();
        if id == AUTO || LAYOUTS.iter().any(|l| l.id == id) || added.iter().any(|l| l.id == id) {
            continue;
        }
        // Variants the fonts draw.
        for (_, variants) in &mut layout.variants {
            variants.retain(|c| drawable(env, c.encode_utf8(&mut [0; 4])));
        }
        layout.variants.retain(|(_, variants)| !variants.is_empty());
        added.push(layout);
    }
    added
}

/// A line of [`POINT`].
fn parse(line: &str) -> Option<Added> {
    let mut fields = line.split(';').map(str::trim);
    let id = fields.next().filter(|id| !id.is_empty())?;
    let code: String = fields
        .next()
        .unwrap_or_default()
        .chars()
        .filter(|c| !invisible(*c))
        .take(MAX_CODE)
        .collect();
    // At most so many rows of so many visible letters.
    let rows: Vec<String> = fields
        .next()
        .unwrap_or_default()
        .split_whitespace()
        .take(MAX_ROWS)
        .map(|row| {
            row.chars()
                .filter(|c| !invisible(*c))
                .take(MAX_KEYS)
                .collect::<String>()
        })
        .filter(|row| !row.is_empty())
        .collect();
    if code.is_empty() || rows.is_empty() {
        return None;
    }
    let variants = held(fields.next().unwrap_or_default());
    Some(Added {
        id: id.to_string(),
        code,
        letters: rows.join(" "),
        variants,
    })
}

/// Letters typed by holding another: pairs of a letter, `=` and them,
/// separated by spaces (`e=éèë i=ïí`).
fn held(pairs: &str) -> Vec<(char, String)> {
    pairs
        .split_whitespace()
        .filter_map(|pair| {
            let (base, variants) = pair.split_once('=')?;
            let mut chars = base.chars();
            let base = chars.next().filter(|_| chars.next().is_none())?;
            // As lower case letters are looked up.
            let base = base.to_lowercase().next().unwrap_or(base);
            let variants: String = variants
                .chars()
                .filter(|c| !invisible(*c))
                .take(MAX_VARIANTS)
                .collect();
            (!variants.is_empty()).then_some((base, variants))
        })
        .collect()
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

/// Whether the device's fonts draw every letter of `letters` and their
/// capitals: a layout in a script they lack (e.g. Cyrillic on the X4's
/// packs) is not offered, rather than shown as question marks.
pub(crate) fn drawable(env: &Env<'_>, letters: &str) -> bool {
    let Some(font) = paperpaw_sdk::ui::UiStyle::new(env).font(env) else {
        return true;
    };
    letters
        .chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(|c| core::iter::once(c).chain(c.to_uppercase()))
        .all(|c| font.has_char(c))
}

/// A built-in layout as chosen, with keys of their own for the letters
/// it holds where `keys`.
fn builtin(layout: &Layout, code: String, keys: bool) -> Chosen {
    (code, Cow::Borrowed(layout.rows(keys)), held(layout.held))
}

/// The layouts chosen in the setting `value` (ids separated by commas),
/// as their codes and letter rows; the UI language's layout if none.
/// Layouts the fonts cannot draw are left out. `keys`: the letters a
/// layout types by holding another get keys of their own (*Umlaut keys*).
pub(crate) fn chosen(env: &Env<'_>, value: &str, keys: bool) -> Vec<Chosen> {
    let mut layouts: Vec<Chosen> = Vec::new();
    let mut added: Option<Vec<Added>> = None;
    for id in value.split(',').map(str::trim).filter(|id| !id.is_empty()) {
        if id == AUTO {
            layouts.push(automatic(env, keys));
        } else if let Some(layout) = LAYOUTS
            .iter()
            .find(|l| l.id == id && drawable(env, l.rows(keys)))
        {
            layouts.push(builtin(layout, layout.code.to_string(), keys));
        } else if let Some(layout) = added
            .get_or_insert_with(|| self::added(env))
            .iter()
            .find(|l| l.id == id && drawable(env, &l.letters))
        {
            let letters = Cow::Owned(layout.letters.clone());
            layouts.push((layout.code.clone(), letters, layout.variants.clone()));
        }
    }
    if layouts.is_empty() {
        layouts.push(automatic(env, keys));
    }
    layouts
}

/// The UI language's layout: the built-in one for the language, or its
/// translation's (`keyboard.letters`, which falls back to English).
fn automatic(env: &Env<'_>, keys: bool) -> Chosen {
    let language = env.i18n().language();
    let base = language.split('-').next().unwrap_or(language);
    let code = base.to_uppercase();
    let builtin = LAYOUTS
        .iter()
        .find(|l| l.id.rsplit('.').next() == Some(base));
    match (builtin, env.i18n().get("keyboard.letters")) {
        (Some(layout), _) => self::builtin(layout, code, keys),
        (None, Some(letters)) => (code, Cow::Owned(letters.to_string()), Vec::new()),
        (None, None) => (code, Cow::Borrowed(LAYOUTS[0].letters), Vec::new()),
    }
}

/// The layouts chosen in the setting `value` for the native plugin: the same
/// as [`chosen`], from the language (`core.language`) and the translation's
/// own rows (`keyboard.letters`, empty for none) instead of an environment,
/// with fallible allocation. Layouts other plugins add come by the stage
/// `keyboard.layouts` (see `PORTING-7.md`).
pub(crate) fn chosen_for(
    language: &str,
    letters: &str,
    value: &str,
    keys: bool,
) -> Result<Vec<Chosen>, paperpaw_sdk::native::Error> {
    let mut layouts: Vec<Chosen> = Vec::new();
    for id in value.split(',').map(str::trim).filter(|id| !id.is_empty()) {
        let one = if id == AUTO {
            Some(automatic_for(language, letters, keys)?)
        } else {
            match LAYOUTS.iter().find(|l| l.id == id) {
                Some(l) => Some(builtin_for(l, copy(l.code)?, keys)?),
                None => None,
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
            for (_, letters) in held(layout.held) {
                assert!(letters.chars().all(|c| keys.contains(c)), "{}", layout.id);
            }
        }
        assert!(en.contains("keyboard.layout.auto ="));
    }

    #[test]
    fn added_layouts_are_read_from_their_lines() {
        let dutch = parse("dutch.layout; NL; qwertyuiop asdfghjkl zxcvbnm; e=éèë ij=x i=ïí");
        let dutch = dutch.unwrap();
        assert_eq!(
            (dutch.id.as_str(), dutch.code.as_str()),
            ("dutch.layout", "NL")
        );
        assert_eq!(dutch.letters, "qwertyuiop asdfghjkl zxcvbnm");
        // Two letters before `=` are no letter.
        assert_eq!(
            dutch.variants,
            [('e', String::from("éèë")), ('i', String::from("ïí"))]
        );
        assert!(parse("plain.layout; PL; abc").unwrap().variants.is_empty());
        // Capitals are looked up in lower case; too much is cut; what
        // shows nothing goes.
        let big = parse(&alloc::format!(
            "big.layout; LONGCODE; {}; A=\u{202E}áà{}",
            ["abcdefghijklmnopqrstuvwxyz"; 30].join(" "),
            "x".repeat(40)
        ))
        .unwrap();
        assert_eq!(big.code, "LONG");
        assert_eq!(big.letters.split(' ').count(), MAX_ROWS);
        assert!(
            big.letters
                .split(' ')
                .all(|row| row.chars().count() == MAX_KEYS)
        );
        assert_eq!(big.variants[0].0, 'a');
        assert_eq!(big.variants[0].1.chars().count(), MAX_VARIANTS);
        assert!(!big.variants[0].1.contains('\u{202E}'));
        assert_eq!(parse("id; \u{202E}; abc"), None);
        for line in ["", "only.id", "id; CODE", "; X; abc", "id;; abc"] {
            assert_eq!(parse(line), None, "{line}");
        }
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
        assert_eq!(
            held(de.held),
            [
                ('a', String::from("ä")),
                ('o', String::from("ö")),
                ('u', String::from("ü")),
                ('s', String::from("ß"))
            ]
        );
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
    /// The layouts the script packs bring (`tools/plugin-files/keyboards`,
    /// written to the packs' `points/keyboard.layouts.txt`) are lines the
    /// keyboard takes whole: their rows and keys within its limits, ids not
    /// built in, letters of their script, held letters that parse.
    #[test]
    fn the_layouts_of_the_script_packs_are_taken_whole() {
        let packs = [
            (
                include_str!("../../../tools/plugin-files/keyboards/arabic.txt"),
                '\u{600}'..='\u{6ff}',
                3,
            ),
            (
                include_str!("../../../tools/plugin-files/keyboards/hebrew.txt"),
                '\u{590}'..='\u{5ff}',
                1,
            ),
        ];
        let mut ids = Vec::new();
        for (text, script, count) in packs {
            let lines: Vec<&str> = text.lines().filter(|l| !l.starts_with('#')).collect();
            assert_eq!(lines.len(), count);
            for line in lines {
                let layout = parse(line).unwrap_or_else(|| panic!("{line}"));
                assert!(layout.id.starts_with("keyboard.layout."), "{}", layout.id);
                assert!(LAYOUTS.iter().all(|l| l.id != layout.id) && !ids.contains(&layout.id));
                ids.push(layout.id.clone());
                // Not cut short by the limits.
                let rows: Vec<&str> = layout.letters.split(' ').collect();
                assert_eq!(
                    rows.len(),
                    line.split(';').nth(2).unwrap().split_whitespace().count()
                );
                assert!(rows.len() <= MAX_ROWS);
                for row in rows {
                    assert!(row.chars().count() <= MAX_KEYS, "{}", layout.id);
                    assert!(row.chars().all(|c| script.contains(&c)), "{row}");
                }
                // Every variant is a letter of the script too, and of a
                // letter the layout types or a variant of one.
                assert!(!layout.variants.is_empty() || layout.id.ends_with(".he"));
                for (_, variants) in &layout.variants {
                    assert!(variants.chars().all(|c| script.contains(&c)), "{variants}");
                }
                // No letter twice in a layout.
                let all: Vec<char> = layout.letters.chars().filter(|c| *c != ' ').collect();
                let mut sorted = all.clone();
                sorted.sort_unstable();
                sorted.dedup();
                assert_eq!(sorted.len(), all.len(), "{}", layout.id);
            }
        }
        // Their names are in the packs' translations (see script-pack.py).
        let names = include_str!("../../../tools/script-pack.py");
        for id in &ids {
            assert!(names.contains(&alloc::format!("\"{id}\"")), "{id}");
        }
    }
}
