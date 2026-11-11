//! The cover job's marker on the card, `.paperpaw/cover-job.txt`: the key of
//! the one book whose job is running (`doc_cover_job`), 16 hex digits.
//!
//! A format that crashes on a book resets the device; home then asks for the
//! same cover again and the job would crash it again, at every start. The
//! marker is written (and synced) when a job really starts, before its first
//! preparing step, and removed when the job ends however it ends. At the next
//! start a marker that is still there names the book the reset happened in:
//! the library counts it as failed (its failure ring is in memory only, this
//! one key is all the card holds) and the file is removed.
//!
//! A plain power loss while a job runs leaves the marker too: that book has no
//! cover job for one run, which is acceptable.
use super::storage::status;
use paperpaw_platform::Storage;

/// The file, in the data folder of the card.
pub(super) const FILE: &str = ".paperpaw/cover-job.txt";
const FOLDER: &str = ".paperpaw";
/// 16 hex digits of the key.
const LEN: usize = 16;

/// Writes the marker for the book with `key` and syncs it, so that a reset in
/// the job finds it. Where it cannot be written the job is not started.
pub(super) fn write(storage: &dyn Storage, key: u64) -> Result<(), i32> {
    // The folder is there on a card the host set up; a failure here is the
    // write's own.
    let _ = storage.create_dir(FOLDER);
    let mut text = [0_u8; LEN];
    for (i, digit) in text.iter_mut().enumerate() {
        let nibble = (key >> (60 - 4 * i)) & 0xf;
        *digit = b"0123456789abcdef"[nibble as usize];
    }
    storage.write(FILE, &text).map_err(status)?;
    storage.sync().map_err(status)
}

/// Removes the marker (the job has ended); nothing to say where it is gone.
pub(super) fn clear(storage: &dyn Storage) {
    let _ = storage.remove(FILE);
}

/// The key a marker left from before names, 0 for none (or a file that is
/// not one), and removes the file.
pub(super) fn take(storage: &dyn Storage) -> u64 {
    let Ok(size) = storage.size(FILE) else {
        return 0;
    };
    let mut text = [0_u8; LEN];
    // Exactly the 16 digits the host wrote; any other file names no book.
    let key = match storage.read_at(FILE, 0, &mut text) {
        Ok(LEN) if size == LEN as u64 => text.iter().try_fold(0_u64, |key, &digit| {
            char::from(digit)
                .to_digit(16)
                .map(|nibble| (key << 4) | u64::from(nibble))
        }),
        _ => None,
    };
    clear(storage);
    key.unwrap_or(0)
}
