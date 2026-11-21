//! The texts two rows open in the reader, as Beta 3 opened them: the
//! licences and the list of the plugins built in. Each is written to the
//! plugin's data folder and handed to the reader (`core.ui` hand-over with
//! root `0` and the path, `command.open` into the slot `core.reader`).
//!
//! The licences are kept deflated in the program, in parts of at most 8 KiB
//! (`build.rs`); the page's start is written first, then each part is
//! inflated and appended on its own, so the whole text is never on the heap.
use alloc::vec::Vec;
use paperpaw_sdk::{Storage, StorageError, native::Error};

/// The licences of this build's product, packed by `build.rs`.
static LICENCES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/licences.bin"));

/// Where the pages are written for the reader, in the plugin's data folder.
pub(crate) const LICENCES_FILE: &str = "licences.md";
pub(crate) const PLUGINS_FILE: &str = "plugins.md";

/// The licences' parts in order, each inflated when it is reached; a part
/// that cannot be read is an `Err` (and the last item).
fn parts(mut packed: &[u8]) -> impl Iterator<Item = Result<Vec<u8>, Error>> {
    core::iter::from_fn(move || {
        if packed.is_empty() {
            return None;
        }
        let part = next_part(packed);
        packed = match part {
            Ok((_, rest)) => rest,
            Err(_) => &[],
        };
        Some(part.map(|(text, _)| text))
    })
}

/// The first part of `packed`, inflated, and what follows it.
fn next_part(packed: &[u8]) -> Result<(Vec<u8>, &[u8]), Error> {
    let word = |at: usize| -> Result<usize, Error> {
        let bytes = packed.get(at..at + 4).ok_or(Error::Invalid)?;
        let bytes: [u8; 4] = bytes.try_into().map_err(|_| Error::Invalid)?;
        usize::try_from(u32::from_le_bytes(bytes)).map_err(|_| Error::Invalid)
    };
    let (deflated, length) = (word(0)?, word(4)?);
    let data = packed.get(8..8 + deflated).ok_or(Error::Invalid)?;
    let text = miniz_oxide::inflate::decompress_to_vec_with_limit(data, length)
        .map_err(|_| Error::Invalid)?;
    Ok((text, &packed[8 + deflated..]))
}

/// Writes the licences to [`LICENCES_FILE`] in `storage`: `start` (the
/// page's heading and introduction), then the licences.
///
/// # Errors
/// [`Error::NoMemory`] or [`Error::Io`] where the file cannot be written,
/// [`Error::Invalid`] for a part that cannot be inflated.
pub(crate) fn write_licences(storage: &dyn Storage, start: &str) -> Result<(), Error> {
    write_from(storage, start, LICENCES)
}

fn write_from(storage: &dyn Storage, start: &str, packed: &[u8]) -> Result<(), Error> {
    storage
        .write(LICENCES_FILE, start.as_bytes())
        .map_err(stored)?;
    for part in parts(packed) {
        storage.append(LICENCES_FILE, &part?).map_err(stored)?;
    }
    Ok(())
}

/// Writes `text` to [`PLUGINS_FILE`] in `storage`.
///
/// # Errors
/// [`Error::NoMemory`] or [`Error::Io`] where the file cannot be written.
pub(crate) fn write_plugins(storage: &dyn Storage, text: &str) -> Result<(), Error> {
    storage.write(PLUGINS_FILE, text.as_bytes()).map_err(stored)
}

fn stored(error: StorageError) -> Error {
    match error {
        StorageError::OutOfMemory => Error::NoMemory,
        _ => Error::Io,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::String;
    use paperpaw_sdk::MemoryStorage;

    fn read(storage: &MemoryStorage, path: &str) -> String {
        let mut bytes = alloc::vec![0; usize::try_from(storage.size(path).unwrap()).unwrap()];
        assert_eq!(storage.read_at(path, 0, &mut bytes).unwrap(), bytes.len());
        String::from_utf8(bytes).unwrap()
    }

    #[test]
    fn the_licences_are_written_whole_after_the_start() {
        let storage = MemoryStorage::new();
        write_licences(&storage, "# PaperPaw\n\nIntro\n\n").unwrap();
        let text = read(&storage, LICENCES_FILE);
        assert!(text.starts_with("# PaperPaw\n\nIntro\n\n## Notice\n"));
        // Every part came out: the text is the build's licences, unchanged.
        let source = if cfg!(target_os = "none") {
            include_str!("../data/licences-firmware.md")
        } else {
            include_str!("../data/licences-app.md")
        };
        assert_eq!(&text["# PaperPaw\n\nIntro\n\n".len()..], source);
        assert!(parts(LICENCES).count() > 1, "packed in more than one part");
    }

    #[test]
    fn a_broken_part_is_an_error_not_a_short_text() {
        let storage = MemoryStorage::new();
        let mut packed = LICENCES.to_vec();
        packed.truncate(packed.len() - 10);
        assert_eq!(write_from(&storage, "start", &packed), Err(Error::Invalid));
    }

    #[test]
    fn the_plugins_page_is_written() {
        let storage = MemoryStorage::new();
        write_plugins(&storage, "# Plugins built in\n\n- reader 1.0.0\n").unwrap();
        assert_eq!(
            read(&storage, PLUGINS_FILE),
            "# Plugins built in\n\n- reader 1.0.0\n"
        );
    }
}
