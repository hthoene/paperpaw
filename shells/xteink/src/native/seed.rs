//! English hyphenation on the card (`paperpaw-default-packs`), at the first
//! start that finds none: the web flasher puts the pack there, but the
//! update of a firmware that had English built in (Beta 3) does not, and no
//! other way of installing would either.

use log::{info, warn};
use paperpaw_default_packs::{Seeded, seed_hyphenation};
use paperpaw_platform::Storage;

/// Puts the English hyphenation pack on `storage` if no earlier start dealt
/// with it, and says what happened.
// A frame of its own, off the stack before the kernel is built.
#[inline(never)]
pub fn hyphenation(storage: &dyn Storage) {
    match seed_hyphenation(storage) {
        Seeded::Kept => {}
        Seeded::Installed => info!("seed: English hyphenation put on the card"),
        Seeded::NoRoom => warn!("seed: no room for English hyphenation: tried again next start"),
        Seeded::Failed => {
            warn!("seed: English hyphenation could not be written: tried again next start")
        }
        _ => {}
    }
}
