//! *Export* on the list of words (interface 6: `WordsScreen::export`): all
//! words with their sentences, for flash card apps such as Anki, are written to
//! `Notes/Vocabulary.tsv` beside the library folder, in place of the file
//! there. The file is staged in the plugin's data folder (`export.part`,
//! [`store::export`]) and moved onto the card when whole, so a refusal (memory,
//! a full card) leaves the old file as it was and removes the staged one.

use alloc::string::String;

use paperpaw_sdk::Storage;
use paperpaw_sdk::native::files::Files;
use paperpaw_sdk::native::settings::{self, SettingValue};
use paperpaw_sdk::native::storage::{self, DataStorage, Root};
use paperpaw_sdk::native::{Error, Host};
use paperpaw_ui_kit::mem::buffer;
use paperpaw_ui_kit::text;

use crate::files::DataFolder;
use crate::native_review::storage_error;
use crate::store;

/// The file the export makes, in the folder `Notes`.
pub(crate) const NAME: &str = "Vocabulary.tsv";
/// The folder beside the library folder.
const NOTES: &str = "Notes";
/// The staged file in the data folder, and as the data layer names it.
const STAGED: &str = "export.part";
const STAGED_PATH: &str = ".paperpaw/plugins/vocabulary/export.part";
/// Where the library keeps books unless the setting says otherwise.
const DEFAULT_FOLDER: &str = "books";
/// The kernel's setting of the library folder.
const FOLDER_SETTING: &str = "core.library-folder";
/// The buffers of a files call.
const BUFFER: usize = 4096;

/// The folder exports go to: `Notes` beside the `library` folder (at the top of
/// the card where the library is).
///
/// # Errors
/// [`Error::NoMemory`].
pub(crate) fn notes_folder(library: &str) -> Result<String, Error> {
    match library.trim_matches('/').rsplit_once('/') {
        Some((parent, _)) if !parent.is_empty() => text::concat(&[parent, "/", NOTES]),
        _ => text::copy(NOTES),
    }
}

/// The library folder the user chose, without slashes around it.
fn library_folder(host: &Host<'_>) -> Result<String, Error> {
    let mut scratch = [0; 512];
    match settings::read_setting(host, FOLDER_SETTING, &mut scratch) {
        Ok(SettingValue::Text(text)) if !text.trim_matches('/').is_empty() => {
            text::copy(text.trim_matches('/'))
        }
        Ok(_) | Err(Error::Denied | Error::NotFound | Error::NoSpace) => text::copy(DEFAULT_FOLDER),
        Err(error) => Err(error),
    }
}

/// Writes all words to `Notes/Vocabulary.tsv`; the path it is at.
///
/// # Errors
/// [`Error::NoMemory`], [`Error::Full`] for a full card, [`Error::Io`] and the
/// host's other refusals. The old file is as it was.
pub(crate) fn run(host: &Host<'_>) -> Result<String, Error> {
    let folder = notes_folder(&library_folder(host)?)?;
    let path = text::concat(&[&folder, "/", NAME])?;
    let done = stage_and_move(host, &folder, &path);
    if done.is_err() {
        // A leftover is written anew by the next export; nothing to tell.
        let _ = storage::remove(host, Root::Data, STAGED);
    }
    done.map(|()| path)
}

fn stage_and_move(host: &Host<'_>, folder: &str, path: &str) -> Result<(), Error> {
    let data = DataStorage::new(host);
    store::export(&DataFolder(&data), STAGED_PATH).map_err(storage_error)?;
    data.sync().map_err(storage_error)?;
    let (mut input, mut out) = (buffer(BUFFER)?, buffer(BUFFER)?);
    let files = Files::open(host)?;
    let moved = put_in_place(host, &files, folder, path, &mut input, &mut out);
    // The service is released whatever the move did.
    let released = files.release(host);
    moved?;
    released
}

fn put_in_place(
    host: &Host<'_>,
    files: &Files,
    folder: &str,
    path: &str,
    input: &mut [u8],
    out: &mut [u8],
) -> Result<(), Error> {
    files.make_dir(host, Root::Files, folder, input, out)?;
    files.move_entry(
        host,
        Root::Data,
        STAGED,
        Root::Files,
        path,
        true,
        input,
        out,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_notes_folder_is_beside_the_library_folder() {
        for (library, notes) in [
            ("books", "Notes"),
            ("/books/", "Notes"),
            ("Library/books", "Library/Notes"),
            ("a/b/books", "a/b/Notes"),
            ("", "Notes"),
        ] {
            assert_eq!(notes_folder(library).unwrap(), notes, "{library}");
        }
    }
}
