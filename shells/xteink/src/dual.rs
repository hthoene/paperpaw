//! Dual boot, the card's side (see `paperpaw_x4::dual_boot`): once the card
//! is there, `paperpaw.ini` says what the reader chose, the bootloader says
//! whether it carries the marker, and what is declared is told to updates
//! (nothing installs in the other firmware's partition from then on) and to
//! plugins, by a note on the card they read.

use log::{info, warn};
use paperpaw_platform::Storage;
use paperpaw_x4::dual_boot::{self, Choice, Evidence, IniScan, Source};

use crate::ota::{self, Ota};

/// The file the kernel reads (`paperpaw.ini`).
const CONFIG: &str = "paperpaw.ini";
/// The kernel reads no more than this much of it; so do we.
const CONFIG_MAX: u64 = 64 * 1024;
/// Read a piece at a time.
const PIECE: usize = 1024;

/// Milliseconds since the start.
fn millis() -> u64 {
    esp_hal::time::Instant::now()
        .duration_since_epoch()
        .as_millis()
}

/// What `paperpaw.ini` says, `Auto` where it says nothing or cannot be read.
#[cold]
#[inline(never)]
fn choice(storage: &dyn Storage) -> Choice {
    let mut scan = IniScan::default();
    let mut piece = [0u8; PIECE];
    let mut at = 0u64;
    while at < CONFIG_MAX {
        match storage.read_at(CONFIG, at, &mut piece) {
            Ok(0) => break,
            Ok(n) => {
                scan.feed(&piece[..n]);
                at += n as u64;
            }
            // No file, a card that cannot be read: nothing chosen.
            Err(_) => break,
        }
    }
    scan.finish()
}

/// Settles the start. The reader's choice and the bootloader are looked at
/// (`decide`) at every start, the card there: a setup declared is noted on
/// the card for the update page and the plugins (`.paperpaw/boot.txt`, also
/// where updates are not available at all, as under a dualbooter that made
/// finding the running slot fail before, #167), and it is known before
/// anything is written to the OTA data (an update to put on trial, one
/// undone or failed), which a declared setup never has. Whether the setup
/// is declared, for what else must not touch the other partition.
pub fn settle(ota: &mut Option<Ota>, flash: &ota::Flash, storage: &dyn Storage) -> bool {
    let source = decide(flash, ota.as_ref().map(Ota::slot), storage);
    // Done at once (an update this firmware installed): nothing more.
    if let Some(ota) = ota
        && !ota.settled()
    {
        ota.settle(source);
    }
    source.is_some()
}

/// Before an install: whether the setup is declared a dual-boot one, looked
/// at anew (the reader may have written `dual-boot` since the start, or a
/// tool set the bootloader up). Where it is, nothing is installed.
pub fn guard(ota: &mut Ota, storage: &dyn Storage) -> bool {
    if ota.dual().is_none() {
        let source = decide(ota.flash(), Some(ota.slot()), storage);
        ota.declare(source);
    }
    ota.dual().is_some()
}

/// What the reader's choice in `paperpaw.ini` and the bootloader's marker
/// (looked for only where the reader did not decide) make of this setup,
/// and the note for plugins to match.
#[cold]
#[inline(never)]
fn decide(flash: &ota::Flash, slot: Option<u32>, storage: &dyn Storage) -> Option<Source> {
    let began = millis();
    let choice = choice(storage);
    let read = millis();
    let marker = dual_boot::needs_marker(choice)
        .then(|| ota::marker(flash))
        .flatten();
    info!(
        "dual boot: the ini took {} ms, the bootloader {} ms",
        read - began,
        millis() - read
    );
    let source = dual_boot::decide(Evidence { choice, marker });
    match source {
        Some(source) => info!(
            "dual boot: declared by the {}: nothing is installed in the other slot",
            source.name()
        ),
        None if choice == Choice::Off => info!("dual boot: switched off in {CONFIG}"),
        None => {}
    }
    note(storage, source.map(|source| dual_boot::note(source, slot)));
    source
}

/// The note on the card, only where it changes.
#[cold]
#[inline(never)]
fn note(storage: &dyn Storage, text: Option<alloc::string::String>) {
    let mut head = [0u8; 64];
    let now = storage
        .read_at(dual_boot::NOTE, 0, &mut head)
        .ok()
        .map(|n| &head[..n]);
    match text {
        Some(text) if now != Some(text.as_bytes()) => {
            let _ = storage.create_dir(".paperpaw");
            if let Err(e) = storage.write(dual_boot::NOTE, text.as_bytes()) {
                warn!("dual boot: the note was not written: {e:?}");
            }
        }
        None if now.is_some() => {
            let _ = storage.remove(dual_boot::NOTE);
        }
        _ => {}
    }
}
