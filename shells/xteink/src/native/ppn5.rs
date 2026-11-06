//! The X4's plugin files for interface 7 (format 5, `abi/ppn5.md`), as far
//! as the core can take them today (feature `ppn5`, off by default).
//!
//! What this does, in the note's order (`Header5` and `stream5` of
//! `paperpaw_native` hold every rule and are tested on the host):
//!
//! 1. the header is read and checked against this firmware (steps 1 to 7);
//!    the file must be the one installed;
//! 2. the room is found and the image placed (ESP32-C3 only: the X4 Pro and
//!    the X4 Classic refuse every format-5 file at step 3, target 9 having
//!    no profile yet);
//! 3. the digest and every word to relocate are checked on the file, before
//!    a sector of the flash is touched (`Flash::put5`, step 8 and 9);
//! 4. the image is relocated for its place and written, a sector at a time,
//!    through a buffer of one sector: nothing is held in proportion to the
//!    file, the image or the number of words;
//! 5. the entry the image holds is checked against the header and the
//!    metadata record (`Header5::check_entry`, step 9).
//!
//! **Nothing of the plugin runs.** The core has no interface-7 plugin to
//! put the entry's `make` into yet (S2 and the event dispatch come later),
//! so a file that passes everything is reported as not runnable
//! ([`PluginProblem::Unsupported`]) and stays placed: the next start finds
//! it in the index and only checks its entry again. A file refused at any
//! step leaves nothing behind: the layout takes it back (room and the hot
//! code's part of the arena are free again), the index is written without
//! it, and nothing is allocated or mapped for it.

use alloc::boxed::Box;
use log::warn;
use paperpaw_core::CodeFile;
use paperpaw_native::MAGIC;
use paperpaw_native::header5::{HEADER_LEN, Header5, Host, Refusal};
use paperpaw_native::stream5::{ReadError, Source};
use paperpaw_platform::Storage;
use paperpaw_sdk::{Plugin, PluginProblem};

/// What this firmware is, to compare a file with: the X4 at the interface
/// minor the generated ABI says.
const HOST: Host = Host::x4(paperpaw_native::host_glue::MINOR);

/// The newest format a header may carry for [`is_format5`] to take the file
/// as one of ours: the reader below says what it is.
const FIRST_FORMAT: u16 = paperpaw_native::header5::FORMAT;

/// Whether `head` (the file's first bytes) is a plugin file of format 5 or
/// later, which is this module's to read; older ones are the format-4
/// loader's.
pub fn is_format5(head: &[u8]) -> bool {
    head.len() >= 6 && head[..4] == MAGIC && u16::from_le_bytes([head[4], head[5]]) >= FIRST_FORMAT
}

/// A file of the card read at offsets, for `stream5`. Every read tells the
/// watchdogs the work goes on: the digest of a whole file takes seconds.
pub struct StorageSource<'a> {
    storage: &'a dyn Storage,
    path: &'a str,
}

impl<'a> StorageSource<'a> {
    pub fn new(storage: &'a dyn Storage, path: &'a str) -> Self {
        Self { storage, path }
    }
}

impl Source for StorageSource<'_> {
    fn read_at(&self, at: u64, buf: &mut [u8]) -> Result<(), ReadError> {
        crate::power::feed();
        super::read_exact(self.storage, self.path, at, buf).map_err(|()| ReadError)
    }
}

/// What the plugin manager says for a header this firmware refuses.
fn refused(path: &str, head: &[u8], why: Refusal) -> PluginProblem {
    warn!("native: {path}: format 5: {why:?}");
    match why {
        Refusal::OlderInterface => PluginProblem::OlderInterface {
            built_for: head
                .get(8..10)
                .map_or(0, |b| u16::from_le_bytes([b[0], b[1]])),
        },
        Refusal::NewerFormat
        | Refusal::NewerInterface
        | Refusal::NeedsNewerMinor
        | Refusal::Reserved => PluginProblem::NeedsNewerPaperPaw,
        Refusal::UnknownChip | Refusal::OtherChip | Refusal::NoProfile => PluginProblem::OtherBuild,
        _ => PluginProblem::Damaged,
    }
}

/// Loads the format-5 file `code` as far as the core takes it (see the
/// [module documentation](self)). `place` puts the image where it runs.
pub fn load(
    place: &mut super::Place,
    storage: &dyn Storage,
    code: &CodeFile<'_>,
) -> Result<Box<dyn Plugin>, PluginProblem> {
    let path = code.path;
    let started = crate::micros();
    let size = storage.size(path).map_err(|_| PluginProblem::Damaged)?;
    let mut head = [0u8; HEADER_LEN];
    let have = size.min(HEADER_LEN as u64) as usize;
    super::read_exact(storage, path, 0, &mut head[..have]).map_err(|()| PluginProblem::Damaged)?;
    let header =
        Header5::read(&head[..have], size, &HOST).map_err(|why| refused(path, &head, why))?;
    // The file installed, not one put in its folder since.
    if header.sha256 != code.sha256 {
        warn!("native: {path} is not the file installed");
        return Err(PluginProblem::Changed);
    }
    #[cfg(feature = "esp32c3")]
    {
        x4::verified(place, storage, code, &header, started)?;
        Err(PluginProblem::Unsupported)
    }
    #[cfg(not(feature = "esp32c3"))]
    {
        // Unreachable in practice: the X4 Pro's files are target 9, refused
        // by the header; any other chip is refused as another build.
        let _ = (place, started);
        Err(PluginProblem::OtherBuild)
    }
}

/// The ESP32-C3's part: the image is placed in flash and its entry read.
#[cfg(feature = "esp32c3")]
mod x4 {
    use alloc::vec::Vec;

    use log::{info, warn};
    use paperpaw_core::CodeFile;
    use paperpaw_native::header5::{
        EntryFacts, EntryRefusal, HEADER_LEN, Header5, RawEntry, RecordFacts,
    };
    use paperpaw_platform::Storage;
    use paperpaw_sdk::PluginProblem;

    use super::HOST;

    /// An entry the image holds that is not right: a damaged file.
    fn entry_refused(path: &str, why: EntryRefusal) -> PluginProblem {
        warn!("native: {path}: format 5: the entry: {why:?}");
        PluginProblem::Damaged
    }

    /// Places the image of `header`, checks its entry, and takes it back if
    /// the entry is refused.
    pub(super) fn verified(
        place: &mut crate::native::Place,
        storage: &dyn Storage,
        code: &CodeFile<'_>,
        header: &Header5,
        started: u64,
    ) -> Result<(), PluginProblem> {
        let path = code.path;
        // The metadata record as the file has it: at most 2 KiB, however large
        // the file.
        let mut record = Vec::new();
        record
            .try_reserve_exact(header.meta_len as usize)
            .map_err(|_| PluginProblem::NoRoom)?;
        record.resize(header.meta_len as usize, 0);
        crate::native::read_exact(
            storage,
            path,
            HEADER_LEN as u64 + u64::from(header.meta_at),
            &mut record,
        )
        .map_err(|()| PluginProblem::Damaged)?;
        let facts = RecordFacts::read(&record).ok_or_else(|| {
            warn!("native: {path}: format 5: the metadata record is not one this firmware reads");
            PluginProblem::Damaged
        })?;
        let placed = place.put5(storage, path, header)?;
        let checked = check_placed(path, header, &placed, &facts, &record);
        if let Err(why) = checked {
            place.take_back(placed);
            return Err(why);
        }
        info!(
            "native: {path}: format 5, {} B at {:#x}, {} words moved, {} patched; \
             placed and its entry checked in {} us; not run (no interface-7 plugin in the core yet)",
            header.image_len,
            placed.base,
            header.relocs,
            header.patches,
            crate::micros() - started
        );
        Ok(())
    }

    /// Step 9 on the image as it lies in flash: reads the entry and the
    /// metadata view where the header says they are, and gives the header what
    /// it found.
    fn check_placed(
        path: &str,
        header: &Header5,
        placed: &crate::native::flash::Placed5,
        facts: &RecordFacts,
        record: &[u8],
    ) -> Result<(), PluginProblem> {
        // SAFETY: the entry and the metadata lie in the image just placed
        // (`Header5::read` checked `entry + entry_bytes <= code_at <= image_len`
        // and `meta_at + meta_len <= code_at`), written, its cache lines
        // invalidated, in the region mapped at `placed.base`; read-only bytes.
        let (entry, metadata) = unsafe {
            (
                core::slice::from_raw_parts(
                    (placed.base + header.entry) as *const u8,
                    paperpaw_native::header5::X4_ENTRY_BYTES as usize,
                ),
                core::slice::from_raw_parts(
                    (placed.base + header.meta_at) as *const u8,
                    header.meta_len as usize,
                ),
            )
        };
        let raw = RawEntry::read(entry).ok_or(PluginProblem::Damaged)?;
        let found = EntryFacts {
            size: raw.size,
            abi_major: raw.abi_major,
            needs_minor: raw.needs_minor,
            metadata_ptr: raw.metadata_ptr,
            metadata_len: raw.metadata_len,
            metadata,
            make_at: header.make_offset(raw.make, placed.base, placed.hot),
        };
        header
            .check_entry(placed.base, &HOST, facts, record, &found)
            .map_err(|why| entry_refused(path, why))
    }
}
