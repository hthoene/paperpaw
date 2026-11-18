//! *Your own fonts*: the part of the plugin's *How it works* that says how to
//! read with a font file of one's own, from after its heading on, as a page in
//! the reader (as Beta 3 showed it).
//!
//! The page is written to `own.txt` in the plugin's data folder (its title,
//! then the part, appended straight from the text kept in the program: no copy
//! of it on the heap) and handed to the reader (`core.ui` op 16, root `0` and
//! the path; `command.open` into the slot `core.reader`).
use alloc::string::String;
use paperpaw_sdk::{
    Storage,
    native::{
        Error, Host,
        commands::CommandSender,
        ui::{Notify, Open, Param, Ui},
    },
};
use paperpaw_ui_kit::{
    Messages,
    mem::{SERVICE_BUFFER, buffer},
    text::concat,
};

/// How it works, by language: the part this page shows is taken from it.
pub(crate) static HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/en.txt")),
    ("de", include_str!("../how/de.txt")),
];

/// Where the page is written for the reader, in the plugin's data folder.
pub(crate) const OWN_FILE: &str = "own.txt";

/// The part of the texts `how` that follows `heading` (the heading of the
/// language shown: only that language's text has it, and a language without
/// a text of its own shows the English heading), from its first word on.
pub(crate) fn part<'a>(how: &[(&str, &'a str)], heading: &str) -> Option<&'a str> {
    if heading.is_empty() {
        return None;
    }
    how.iter().find_map(|(_, text)| {
        let at = text.find(heading)?;
        text.get(at + heading.len()..).map(str::trim_start)
    })
}

/// Writes the page to [`OWN_FILE`]: `title`, an empty line, `part`, a line's end.
pub(crate) fn write(storage: &dyn Storage, title: &str, part: &str) -> Result<(), Error> {
    let start = concat(&[title, "\n\n"])?;
    let wrote = storage
        .write(OWN_FILE, start.as_bytes())
        .and_then(|()| storage.append(OWN_FILE, part.as_bytes()))
        .and_then(|()| storage.append(OWN_FILE, b"\n"));
    wrote.map_err(|error| match error {
        paperpaw_sdk::StorageError::OutOfMemory => Error::NoMemory,
        _ => Error::Io,
    })
}

/// Shows *Your own fonts* in the reader; where it cannot be (no text for it,
/// the card refused it), says that the help could not be shown, as Beta 3 did.
pub(crate) fn show(host: &Host<'_>, ui: &Ui) -> Result<(), Error> {
    let mut messages = Messages::new()?;
    let shown = open(host, ui, &mut messages);
    if shown.is_ok() {
        return Ok(());
    }
    let text = messages.copy(host, "fonts-browser.how-failed")?;
    let mut scratch = [0; 256];
    CommandSender::new(host, &mut scratch).notify(Notify::new(&text)?)
}

fn open(host: &Host<'_>, ui: &Ui, messages: &mut Messages) -> Result<(), Error> {
    let heading = messages.copy(host, "fonts-browser.own.heading")?;
    let part = part(&HOW, &heading).ok_or(Error::NotFound)?;
    let title: String = messages.copy(host, "fonts-browser.own")?;
    write(
        &paperpaw_sdk::native::storage::DataStorage::new(host),
        &title,
        part,
    )?;
    let params = [Param::new("root", "0")?, Param::new("path", OWN_FILE)?];
    let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
    let handover = ui.handover(host, &params, &mut input, &mut out)?;
    let mut scratch = [0; 128];
    CommandSender::new(host, &mut scratch).open(Open::slot("core.reader")?.handover(handover))
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_sdk::{DirEntry, MemoryStorage, StorageError};

    fn heading(language: &str) -> &'static str {
        match language {
            "de" => "EIGENE SCHRIFTEN",
            _ => "YOUR OWN FONTS",
        }
    }

    #[test]
    fn each_language_finds_its_own_part_and_only_there() {
        for (language, text) in HOW {
            let found = part(&HOW, heading(language)).expect(language);
            let at = text.find(heading(language)).expect(language);
            assert_eq!(found, text[at + heading(language).len()..].trim_start());
            assert!(!found.is_empty(), "{language}");
        }
        // Only the German text has the German heading, only the English the English.
        assert!(!HOW[0].1.contains(heading("de")));
        assert!(!HOW[1].1.contains(heading("en")));
    }

    #[test]
    fn a_heading_no_text_has_finds_nothing() {
        assert_eq!(part(&HOW, "VOS POLICES"), None);
        assert_eq!(part(&HOW, ""), None);
    }

    #[test]
    fn the_page_is_the_title_then_the_part() {
        let card = MemoryStorage::default();
        write(&card, "Your own fonts", "Put a font file.").unwrap();
        let mut buf = [0u8; 64];
        let n = card.read_at(OWN_FILE, 0, &mut buf).unwrap();
        assert_eq!(&buf[..n], b"Your own fonts\n\nPut a font file.\n");
        // Shown again, the page is written anew, not appended to.
        write(&card, "Eigene Schriften", "Leg eine Datei ab.").unwrap();
        let n = card.read_at(OWN_FILE, 0, &mut buf).unwrap();
        assert_eq!(&buf[..n], b"Eigene Schriften\n\nLeg eine Datei ab.\n");
    }

    /// A card that takes every write and keeps nothing (so it allocates nothing:
    /// the sweep counts only the page's own allocations).
    struct Null;
    impl Storage for Null {
        fn read_at(&self, _: &str, _: u64, _: &mut [u8]) -> Result<usize, StorageError> {
            Ok(0)
        }
        fn size(&self, _: &str) -> Result<u64, StorageError> {
            Ok(0)
        }
        fn write(&self, _: &str, _: &[u8]) -> Result<(), StorageError> {
            Ok(())
        }
        fn append(&self, _: &str, _: &[u8]) -> Result<(), StorageError> {
            Ok(())
        }
        fn list(&self, _: &str, _: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
            Ok(())
        }
        fn remove(&self, _: &str) -> Result<(), StorageError> {
            Ok(())
        }
    }

    #[test]
    fn a_refused_allocation_is_no_memory() {
        // One allocation: the title's line; the part itself is never copied.
        let made = crate::fail_alloc::sweep(|| write(&Null, "Your own fonts", HOW[0].1));
        assert_eq!(made, 1);
    }

    #[test]
    fn a_refused_write_is_an_error() {
        assert_eq!(
            write(&paperpaw_sdk::NoStorage, "Your own fonts", "part"),
            Err(Error::Io)
        );
    }
}
