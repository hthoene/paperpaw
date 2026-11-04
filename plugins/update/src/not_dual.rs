//! A device taken for a dual-boot setup that is none (#224): the notice has
//! a button that says so. After a question that warns what it can cost, it
//! writes `dual-boot = off` in the `[firmware]` section of `paperpaw.ini`,
//! takes the firmware's note away and restarts, where the firmware decides
//! anew and installs again. Where the file says `off`, the page has the
//! way back: `dual-boot = auto`, with the same restart.
//!
//! The firmware reads `[firmware]` before the kernel exists and the kernel
//! keeps the section when it saves a setting, so the plugin writes the file
//! as the kernel does: read, one key changed (`config::set`), written.

use alloc::string::String;

use paperpaw_sdk::ask;
use paperpaw_sdk::config;
use paperpaw_sdk::event::Event;
use paperpaw_sdk::replies::Confirmed;
use paperpaw_sdk::{Command, Commands, Env, Storage, StorageError};

/// The file, its section and its key (`paperpaw_x4::dual_boot` reads them).
const FILE: &str = "paperpaw.ini";
const SECTION: &str = "firmware";
const KEY: &str = "dual-boot";
/// Largest file read, as the kernel reads it.
const MAX: usize = 64 * 1024;

/// The answer to the question below.
pub(crate) const ASKED: Event<Confirmed> = Event::new("update.not-dual-asked");

/// The question: what it does, and what it can cost.
pub(crate) fn question(env: &Env<'_>) -> Command {
    let tr = |key: &str| String::from(env.i18n().tr(key));
    ask::confirm(tr("update.not-dual.ask"), &ASKED)
        .text(tr("update.not-dual.warning"))
        .yes(tr("update.not-dual.yes"))
        .no(tr("update.not-dual.no"))
        .command()
}

/// The reader's answer: yes writes the choice and restarts.
pub(crate) fn answered(env: &Env<'_>, answer: &Confirmed, out: &mut Commands) {
    if answer.yes {
        choose(env, "off", out);
    }
}

/// Writes `dual-boot = value`, takes the firmware's note away (it is made
/// anew at the start) and restarts; says so where the file cannot be
/// written.
pub(crate) fn choose(env: &Env<'_>, value: &str, out: &mut Commands) {
    if write(env.storage(), value) {
        let _ = env.storage().remove(crate::dual::NOTE);
        out.push(Command::Restart);
    } else {
        out.push(Command::Notify(String::from(
            env.i18n().tr("update.not-dual.failed"),
        )));
    }
}

/// `dual-boot = value` in `paperpaw.ini`, the rest of the file as it was;
/// not written where the file cannot be read whole (another encoding, a
/// read that ends early) or where the change would make it larger than
/// the firmware and the kernel read ([`MAX`]).
fn write(storage: &dyn Storage, value: &str) -> bool {
    let Ok(source) = read_whole(storage) else {
        return false;
    };
    let source = source.unwrap_or_default();
    let updated = config::set(&source, SECTION, KEY, value);
    if updated == source {
        return true;
    }
    // A file the readers cut would lose its tail, and the choice with it.
    updated.len() <= MAX && storage.write(FILE, updated.as_bytes()).is_ok()
}

/// The file as text: `None` where there is none. An error where it cannot
/// be read, is not UTF-8, or where the read ends before the size the
/// storage gave (the part not read would be lost by a write).
fn read_whole(storage: &dyn Storage) -> Result<Option<String>, StorageError> {
    let size = match storage.size(FILE) {
        Ok(size) => size,
        Err(StorageError::NotFound) => return Ok(None),
        Err(e) => return Err(e),
    };
    let mut data = paperpaw_sdk::read_file(storage, FILE, MAX)?;
    if data.len() as u64 != size {
        return Err(StorageError::Io);
    }
    // The mark is left out; the file is written without it.
    if data.starts_with(b"\xEF\xBB\xBF") {
        data.drain(..3);
    }
    String::from_utf8(data)
        .map(Some)
        .map_err(|_| StorageError::InvalidData)
}

/// Whether the file says `dual-boot = off` (as the firmware reads the
/// spellings of it: off, false, no, 0).
pub(crate) fn is_off(storage: &dyn Storage) -> bool {
    let Ok(Some(source)) = paperpaw_sdk::files::load_text_for_update(storage, FILE, MAX) else {
        return false;
    };
    let mut off = false;
    for entry in config::parse(&source).flatten() {
        if entry.section == SECTION && entry.key == KEY {
            let value = entry.value.as_str();
            off = ["off", "false", "no", "0"]
                .iter()
                .any(|word| value.trim().eq_ignore_ascii_case(word));
        }
    }
    off
}
