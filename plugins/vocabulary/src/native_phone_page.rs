//! The phone's web page with its texts in the reader's language and its limits,
//! written into the plugin's data folder for `core.net` `respond` to send (an
//! answer made of memory holds 2,048 bytes; the page is some 16 KiB). Interface
//! 6 filled the same three marks in a buffer it read the page into
//! (`phone::page`).
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::native::storage::Root;
use paperpaw_sdk::native::{Error, Host};
use paperpaw_ui_kit::Messages;
use paperpaw_ui_kit::files::replace;
use paperpaw_ui_kit::text::write_to;

use crate::cards::MAX_TEXT;
use crate::native_phone_web::{MAX_IMPORT, PAGE_FILE, push_json};

/// The page, as the plugin carries it (it is also a file of the package for
/// interface 6, `data/phone.txt`: a package holds no `.html`).
const PAGE: &str = include_str!("../data/phone.txt");
/// Where the texts, the most bytes of an import and the most of a text go; they
/// come in this order in the page.
const STRINGS: &str = "/*STRINGS*/{}";
const IMPORT: &str = "/*MAX_IMPORT*/0";
const TEXT: &str = "/*MAX_TEXT*/0";

/// The names the page reads its texts by, and their catalog keys.
const TEXTS: [(&str, &str); 36] = [
    ("title", "vocabulary.web.title"),
    ("decks", "vocabulary.web.decks"),
    ("newDeck", "vocabulary.web.new-deck"),
    ("deckName", "vocabulary.web.deck-name"),
    ("add", "vocabulary.web.add"),
    ("save", "vocabulary.web.save"),
    ("cancel", "vocabulary.web.cancel"),
    ("rename", "vocabulary.web.rename"),
    ("delete", "vocabulary.web.delete"),
    ("deleteDeck", "vocabulary.web.delete-deck"),
    ("deleteCard", "vocabulary.web.delete-card"),
    ("cards", "vocabulary.web.cards"),
    ("noCards", "vocabulary.web.no-cards"),
    ("noDecks", "vocabulary.web.no-decks"),
    ("front", "vocabulary.web.front"),
    ("back", "vocabulary.web.back"),
    ("example", "vocabulary.web.example"),
    ("newCard", "vocabulary.web.new-card"),
    ("edit", "vocabulary.web.edit"),
    ("more", "vocabulary.web.more"),
    ("import", "vocabulary.web.import"),
    ("importHelp", "vocabulary.web.import-help"),
    ("paste", "vocabulary.web.paste"),
    ("header", "vocabulary.web.header"),
    ("file", "vocabulary.web.file"),
    ("send", "vocabulary.web.send"),
    ("read", "vocabulary.web.read"),
    ("imported", "vocabulary.web.imported"),
    ("failed", "vocabulary.web.failed"),
    ("newerAnki", "vocabulary.web.newer-anki"),
    ("noNotes", "vocabulary.web.no-notes"),
    ("tooLarge", "vocabulary.web.too-large"),
    ("wrongType", "vocabulary.web.wrong-type"),
    ("notUtf8", "vocabulary.web.not-utf8"),
    ("partial", "vocabulary.web.partial"),
    ("full", "vocabulary.web.full"),
];

/// The texts as an object literal: `{title:"…",decks:"…"}`.
fn strings(mut text: impl FnMut(&str) -> Result<String, Error>) -> Result<String, Error> {
    let mut json = String::new();
    json.try_reserve(4_096).map_err(|_| Error::NoMemory)?;
    json.push('{');
    for (i, (name, key)) in TEXTS.iter().enumerate() {
        if i > 0 {
            json.push(',');
        }
        json.push_str(name);
        json.push(':');
        push_json(&mut json, &text(key)?)?;
    }
    json.try_reserve(1).map_err(|_| Error::NoMemory)?;
    json.push('}');
    Ok(json)
}

/// The page with `json` and the limits where the marks are, made in one pass
/// (a value that holds a mark is not filled again).
fn assemble(json: &str) -> Result<Vec<u8>, Error> {
    let mut import = String::new();
    write_to(&mut import, format_args!("{MAX_IMPORT}"))?;
    let mut text = String::new();
    write_to(&mut text, format_args!("{MAX_TEXT}"))?;
    let mut page = Vec::new();
    page.try_reserve_exact(PAGE.len() + json.len() + import.len() + text.len())
        .map_err(|_| Error::NoMemory)?;
    let mut rest = PAGE;
    for (mark, value) in [(STRINGS, json), (IMPORT, &import), (TEXT, &text)] {
        let Some((before, after)) = rest.split_once(mark) else {
            continue;
        };
        page.extend_from_slice(before.as_bytes());
        page.extend_from_slice(value.as_bytes());
        rest = after;
    }
    page.extend_from_slice(rest.as_bytes());
    Ok(page)
}

/// Writes the page for the requests of this time of serving.
///
/// # Errors
/// What the texts and the storage refuse; `NoMemory`.
pub(crate) fn write(host: &Host<'_>) -> Result<(), Error> {
    let mut messages = Messages::new()?;
    let json = strings(|key| messages.copy(host, key))?;
    let page = assemble(&json)?;
    replace(host, Root::Data, PAGE_FILE, &page)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_marks_are_filled_in_order_and_nowhere_else() {
        let json = strings(|key| Ok(key.into())).unwrap();
        // A value that holds a mark is put in as it is.
        let page = assemble(&json.replace("vocabulary.web.title", "/*MAX_IMPORT*/0")).unwrap();
        let page = core::str::from_utf8(&page).unwrap();
        assert!(page.contains("title:\"/*MAX_IMPORT*/0\""));
        assert!(page.contains(&alloc::format!("{MAX_IMPORT}")));
        assert!(!page.contains(STRINGS) && !page.contains(TEXT));
    }

    #[test]
    fn the_page_has_its_marks_in_order() {
        let at = |mark: &str| PAGE.find(mark).unwrap();
        assert!(at(STRINGS) < at(IMPORT) && at(IMPORT) < at(TEXT));
    }

    /// Every text the page reads (`S.name`) is given.
    #[test]
    fn every_text_the_page_reads_is_given() {
        let mut read: Vec<&str> = Vec::new();
        for part in PAGE.split("S.").skip(1) {
            let end = part
                .find(|c: char| !c.is_ascii_alphanumeric())
                .unwrap_or(part.len());
            read.push(part.get(..end).unwrap());
        }
        let given: Vec<&str> = TEXTS.iter().map(|(name, _)| *name).collect();
        for name in &read {
            assert!(given.contains(name), "{name}");
        }
    }

    #[test]
    fn the_catalogs_have_every_text() {
        for source in [
            include_str!("../i18n/en.ini"),
            include_str!("../i18n/de.ini"),
        ] {
            for (_, key) in TEXTS {
                assert!(
                    source
                        .lines()
                        .any(|l| l.starts_with(&alloc::format!("{key} "))),
                    "{key}"
                );
            }
        }
    }
}
