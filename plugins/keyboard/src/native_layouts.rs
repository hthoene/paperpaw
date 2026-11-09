//! Layouts other plugins add, on interface 7: a plugin subscribes to the stage
//! `keyboard.layouts@1` (`stages/keyboard.layouts@1.def`, codec `src/stages.rs`)
//! and answers each run with up to [`MOST_PER_PLUGIN`] `layouts.layout`
//! records. The keyboard runs the stage when it needs the layouts (the dialog,
//! the chooser) and takes the rules of interface 6's point `keyboard.layouts`
//! for a layout: the id is also the translation key of its name, the code has
//! at most [`MAX_CODE`] visible characters, the letter rows are separated by
//! spaces (at most [`MAX_ROWS`] of at most [`MAX_KEYS`] letters), and the
//! variants are pairs `e=éèë` (at most [`MAX_VARIANTS`] a letter). Every
//! allocation is fallible.
use alloc::borrow::Cow;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::native::{Error, Host, stages::Stage};
use paperpaw_ui_kit::{
    mem::{buffer, push},
    text::copy,
};

use crate::layouts::{
    AUTO, Added, Chosen, LAYOUTS, MAX_CODE, MAX_KEYS, MAX_ROWS, MAX_VARIANTS, invisible,
};
use crate::stages::keyboard_layouts_v1 as schema;

/// Layouts added at most, from all plugins: the chooser stays a list a
/// person reads.
pub const MOST: usize = 16;
/// Layouts one plugin may add in one answer.
pub const MOST_PER_PLUGIN: usize = schema::REPLY_ROWS;

/// The layout the fields of a record make, by the rules of the point
/// `keyboard.layouts`: `None` for one that says too little (no id, no code
/// letters, no letter rows).
///
/// # Errors
/// [`Error::NoMemory`].
pub fn from_fields(
    id: &str,
    code: &str,
    rows: &str,
    variants: &str,
) -> Result<Option<Added>, Error> {
    let id = id.trim();
    if id.is_empty() {
        return Ok(None);
    }
    let mut code_text = String::new();
    code_text
        .try_reserve(code.len().min(4 * MAX_CODE))
        .map_err(|_| Error::NoMemory)?;
    code_text.extend(
        code.trim()
            .chars()
            .filter(|c| !invisible(*c))
            .take(MAX_CODE),
    );
    // At most so many rows of so many visible letters, kept with their
    // single spaces between.
    let mut letters = String::new();
    letters
        .try_reserve(rows.len().min(4 * MAX_ROWS * (MAX_KEYS + 1)))
        .map_err(|_| Error::NoMemory)?;
    let mut count = 0;
    for row in rows.split_whitespace().take(MAX_ROWS) {
        let before = letters.len();
        if count > 0 {
            letters.push(' ');
        }
        let start = letters.len();
        letters.extend(row.chars().filter(|c| !invisible(*c)).take(MAX_KEYS));
        if letters.len() == start {
            letters.truncate(before);
        } else {
            count += 1;
        }
    }
    if code_text.is_empty() || letters.is_empty() {
        return Ok(None);
    }
    Ok(Some(Added {
        id: copy(id)?,
        code: code_text,
        letters,
        variants: held(variants)?,
    }))
}

/// Letters typed by holding another: pairs of a letter, `=` and them,
/// separated by spaces (`e=éèë i=ïí`); a pair whose base is not one letter
/// or that has nothing to type is left out; capitals are looked up in lower
/// case.
fn held(pairs: &str) -> Result<Vec<(char, String)>, Error> {
    let mut found: Vec<(char, String)> = Vec::new();
    for pair in pairs.split_whitespace() {
        let Some((base, letters)) = pair.split_once('=') else {
            continue;
        };
        let mut chars = base.chars();
        let Some(base) = chars.next().filter(|_| chars.next().is_none()) else {
            continue;
        };
        let base = base.to_lowercase().next().unwrap_or(base);
        let mut variants = String::new();
        variants
            .try_reserve(letters.len().min(4 * MAX_VARIANTS))
            .map_err(|_| Error::NoMemory)?;
        variants.extend(
            letters
                .chars()
                .filter(|c| !invisible(*c))
                .take(MAX_VARIANTS),
        );
        if !variants.is_empty() {
            push(&mut found, (base, variants))?;
        }
    }
    Ok(found)
}

/// Adds `layout` unless it names a built-in layout or one added before, or
/// [`MOST`] are added.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn accept(added: &mut Vec<Added>, layout: Added) -> Result<(), Error> {
    let id = layout.id.as_str();
    if added.len() >= MOST
        || id == AUTO
        || LAYOUTS.iter().any(|l| l.id == id)
        || added.iter().any(|l| l.id == id)
    {
        return Ok(());
    }
    push(added, layout)
}

/// A layout as chosen: its code, its letters and the variants it adds.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn chosen_of(layout: &Added) -> Result<Chosen, Error> {
    let mut variants: Vec<(char, String)> = Vec::new();
    variants
        .try_reserve_exact(layout.variants.len())
        .map_err(|_| Error::NoMemory)?;
    for (base, letters) in &layout.variants {
        variants.push((*base, copy(letters)?));
    }
    Ok((
        copy(&layout.code)?,
        Cow::Owned(copy(&layout.letters)?),
        variants,
    ))
}

/// Reads the answers of one run of the stage: every record of every
/// subscriber that answered, in stage order, at most [`MOST_PER_PLUGIN`] a
/// plugin and [`MOST`] in all. A subscriber that fails or answers something
/// else adds none (the keyboard is the user's, not theirs).
///
/// # Errors
/// [`Error::NoMemory`] and the host's refusal of the run itself.
pub fn collect(host: &Host<'_>, language: &str) -> Result<Vec<Added>, Error> {
    let mut input = buffer(schema::MAX_INPUT)?;
    let length = schema::input_encode(
        &[schema::Record::LayoutsAsk(schema::LayoutsAsk::new(
            language
                .get(..language.len().min(schema::LAYOUTS_ASK_LANGUAGE_MAX_BYTES))
                .unwrap_or(""),
        ))],
        &mut input,
    )
    .map_err(|_| Error::Invalid)?;
    let mut out = buffer(Stage::out_len(schema::MAX_REPLY as u32))?;
    let stage = Stage::new(host, schema::KEY)?;
    let mut run = stage.run(&input[..length], &mut out);
    let mut added: Vec<Added> = Vec::new();
    while added.len() < MOST {
        let Some(reply) = run.next()? else { break };
        if reply.status().is_err() {
            continue;
        }
        let Ok(records) = schema::reply_decode(reply.payload()) else {
            continue;
        };
        let mut taken = 0;
        for record in records {
            if taken == MOST_PER_PLUGIN {
                break;
            }
            if let Ok(schema::Record::LayoutsLayout(layout)) = record {
                taken += 1;
                if let Some(layout) =
                    from_fields(layout.id, layout.code, layout.letters, layout.variants)?
                {
                    accept(&mut added, layout)?;
                }
            }
        }
    }
    Ok(added)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layouts::parse;

    /// The fields as the line of the point says them.
    fn line(id: &str, code: &str, rows: &str, variants: &str) -> String {
        alloc::format!("{id}; {code}; {rows}; {variants}")
    }

    #[test]
    fn the_rules_are_those_of_the_point_in_interface_6() {
        let long_rows = ["abcdefghijklmnopqrstuvwxyz"; 30].join(" ");
        let long_variants = alloc::format!("A=\u{202E}áà{} e=éèë ij=x i=ïí o=", "x".repeat(40));
        let samples = [
            (
                "dutch.layout",
                "NL",
                "qwertyuiop asdfghjkl zxcvbnm",
                "e=éèë ij=x i=ïí",
            ),
            ("plain.layout", "PL", "abc", ""),
            (
                "big.layout",
                "LONGCODE",
                long_rows.as_str(),
                long_variants.as_str(),
            ),
            ("odd.layout", "X\u{200B}Y", "ab \u{202E} cd   ef", "é=e"),
            ("a", "A", "z", "z=z"),
        ];
        for (id, code, rows, variants) in samples {
            let old = parse(&line(id, code, rows, variants));
            let new = from_fields(id, code, rows, variants).unwrap();
            assert_eq!(new, old, "{id}");
        }
        for (id, code, rows) in [
            ("", "X", "abc"),
            ("id", "", "abc"),
            ("id", "\u{202E}", "abc"),
            ("id", "X", ""),
            ("id", "X", "\u{200B}"),
        ] {
            assert_eq!(
                from_fields(id, code, rows, "").unwrap(),
                None,
                "{id}{code}{rows}"
            );
        }
    }

    #[test]
    fn built_in_ids_and_repeats_are_not_added_again() {
        let mut added = Vec::new();
        let make = |id: &str| from_fields(id, "X", "abc", "").unwrap().unwrap();
        for id in [
            AUTO,
            "keyboard.layout.de",
            "dutch.layout",
            "dutch.layout",
            "other.layout",
        ] {
            accept(&mut added, make(id)).unwrap();
        }
        let ids: Vec<&str> = added.iter().map(|l| l.id.as_str()).collect();
        assert_eq!(ids, ["dutch.layout", "other.layout"]);
        for n in 0..MOST + 3 {
            accept(&mut added, make(&alloc::format!("more.{n}"))).unwrap();
        }
        assert_eq!(added.len(), MOST);
    }

    #[test]
    fn an_added_layout_is_chosen_by_its_id() {
        let dutch = from_fields(
            "dutch.layout",
            "NL",
            "qwertyuiop asdfghjkl zxcvbnm",
            "e=éèë",
        )
        .unwrap()
        .unwrap();
        let chosen = crate::layouts::chosen_with(
            "en",
            "",
            "dutch.layout, keyboard.layout.de, unknown.layout",
            false,
            core::slice::from_ref(&dutch),
        )
        .unwrap();
        assert_eq!(chosen.len(), 2);
        assert_eq!(chosen[0].0, "NL");
        assert_eq!(chosen[0].1, "qwertyuiop asdfghjkl zxcvbnm");
        assert_eq!(chosen[0].2, [('e', String::from("éèë"))]);
        assert_eq!(chosen[1].0, "DE");
        // Without it the id is not a layout and the language's one stands in.
        let none = crate::layouts::chosen_with("en", "", "dutch.layout", false, &[]).unwrap();
        assert_eq!(none[0].0, "EN");
    }

    #[test]
    fn the_largest_answer_fits_the_replys_bounds_and_reads_back() {
        let id = "i".repeat(schema::LAYOUTS_LAYOUT_ID_MAX_BYTES);
        let code = "c".repeat(schema::LAYOUTS_LAYOUT_CODE_MAX_BYTES);
        let letters = "l".repeat(schema::LAYOUTS_LAYOUT_LETTERS_MAX_BYTES);
        let variants = "v".repeat(schema::LAYOUTS_LAYOUT_VARIANTS_MAX_BYTES);
        let one = schema::Record::LayoutsLayout(schema::LayoutsLayout::new(
            &id, &code, &letters, &variants,
        ));
        let mut out = alloc::vec![0; schema::MAX_REPLY];
        let used = schema::reply_encode(&[one, one, one], &mut out).unwrap();
        let records = schema::reply_decode(&out[..used]).unwrap();
        assert_eq!(records.len(), schema::REPLY_ROWS);
        // A fourth is more than a subscriber may answer.
        assert!(schema::reply_encode(&[one, one, one, one], &mut out).is_err());
        // What a subscriber's answer says is read by the rules of the point.
        let mut small = alloc::vec![0; 512];
        let used = schema::reply_encode(
            &[schema::Record::LayoutsLayout(schema::LayoutsLayout::new(
                "dutch.layout",
                "NL",
                "qwertyuiop asdfghjkl zxcvbnm",
                "e=éèë",
            ))],
            &mut small,
        )
        .unwrap();
        let mut found = Vec::new();
        for record in schema::reply_decode(&small[..used]).unwrap() {
            let schema::Record::LayoutsLayout(l) = record.unwrap() else {
                panic!()
            };
            found.push(
                from_fields(l.id, l.code, l.letters, l.variants)
                    .unwrap()
                    .unwrap(),
            );
        }
        assert_eq!(found[0].id, "dutch.layout");
        assert_eq!(found[0].variants, [('e', String::from("éèë"))]);
    }

    #[test]
    fn allocation_refusals_end_in_no_memory() {
        use crate::fail_alloc::sweep;
        let layout = from_fields("a.layout", "A", "abc def", "e=éèë")
            .unwrap()
            .unwrap();
        sweep(|| from_fields("a.layout", "A", "abc def", "e=éèë ï=i").map(|l| l.is_some()));
        sweep(|| {
            let mut added = Vec::new();
            accept(
                &mut added,
                from_fields("b.layout", "B", "abc", "")?.unwrap(),
            )?;
            Ok(added.len())
        });
        sweep(|| chosen_of(&layout).map(|c| c.0.len()));
    }
}
