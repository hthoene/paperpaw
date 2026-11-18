//! One export, start to end: where it goes, the staged file, the library read
//! for it, and the file put in place. See [`crate::native_export`] for the
//! walk over the library.
use crate::model::{self, Sink};
use crate::native_export::{Exported, RESTARTS, export};
use crate::native_library::{BUFFER, HostSource};
use crate::native_staged::Staged;
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::native::{
    Error, Host,
    files::Files,
    library::Library,
    settings::{self, SettingValue},
};

/// Where the library keeps books unless the setting says otherwise.
const DEFAULT_FOLDER: &str = "books";
/// The kernel's setting of the library folder.
const FOLDER_SETTING: &str = "core.library-folder";

/// The library folder the user chose, without slashes around it.
fn library_folder(host: &Host<'_>) -> Result<String, Error> {
    let mut scratch = [0; 512];
    match settings::read_setting(host, FOLDER_SETTING, &mut scratch) {
        Ok(SettingValue::Text(text)) if !text.trim_matches('/').is_empty() => {
            model::copy(text.trim_matches('/'))
        }
        Ok(_) | Err(Error::Denied | Error::NotFound | Error::NoSpace) => {
            model::copy(DEFAULT_FOLDER)
        }
        Err(error) => Err(error),
    }
}

fn buffer() -> Result<Vec<u8>, Error> {
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(BUFFER)
        .map_err(|_| Error::NoMemory)?;
    bytes.resize(BUFFER, 0);
    Ok(bytes)
}

/// Writes the notes of every book to `Exports/My Clippings.txt` beside the
/// library folder, in place of the file there; what was made and the path.
/// With no notes nothing is written and the old file stays.
///
/// The time of a note is written in UTC until the system time zone comes with
/// the snapshot (draft 25); interface 6 wrote local time.
///
/// # Errors
/// `State` where the library kept changing, `NoMemory`, `Full` for a full
/// card; the host's other refusals. The old file is as it was.
pub fn run(host: &Host<'_>, library: &Library) -> Result<(Exported, String), Error> {
    let folder = model::export_folder(&library_folder(host)?)?;
    let mut path = String::new();
    path.put(&folder)?;
    path.put("/")?;
    path.put(model::NAME)?;
    let files = Files::open(host)?;
    let done = attempts(host, library, &files, &folder, &path);
    // The service is released whatever the export did.
    let released = files.release(host);
    let done = done?;
    released?;
    Ok((done, path))
}

fn attempts(
    host: &Host<'_>,
    library: &Library,
    files: &Files,
    folder: &str,
    path: &str,
) -> Result<Exported, Error> {
    let (mut input, mut out) = (buffer()?, buffer()?);
    let mut tries = 0;
    loop {
        let mut staged = Staged::new(host)?;
        let mut source = HostSource::for_export(host, library)?;
        let done = match export(&mut source, &mut staged, &|seconds| seconds) {
            Err(Error::State) if tries < RESTARTS => {
                tries += 1;
                continue;
            }
            other => other?,
        };
        // The library's buffers are not needed to put the file in place.
        drop(source);
        if done.notes == 0 {
            staged.discard();
        } else {
            staged.put_in_place(files, folder, path, &mut input, &mut out)?;
        }
        return Ok(done);
    }
}
