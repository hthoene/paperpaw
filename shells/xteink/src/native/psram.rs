//! The X4 Pro: an image is read into PSRAM, relocated there and run from
//! there. esp-hal maps PSRAM through the MMU, whose entries serve the
//! instruction bus too: code seen at data address A runs at A +
//! `CODE_OFFSET`. No flash is written; plugins take PSRAM only, as much
//! as their images.

use core::alloc::Layout;

use esp_alloc::MemoryCapability;
use esp_hal::sha::{Sha, Sha256};
use paperpaw_native::{CODE_OFFSET, HEADER_LEN, Header, Relocator};
use paperpaw_platform::Storage;
use paperpaw_sdk::PluginProblem;
use paperpaw_sdk::native::CodeRoom;

use super::api::API;

#[cfg(feature = "ppn5")]
use alloc::vec::Vec;
#[cfg(feature = "ppn5")]
use log::{info, warn};
#[cfg(feature = "ppn5")]
use paperpaw_native::header5::Header5;
#[cfg(feature = "ppn5")]
use paperpaw_native::stream5::{Cached, Relocator5, Written};

/// The data cache's line: an image's memory is written back from it in
/// whole lines.
const CACHE_LINE: usize = 32;

unsafe extern "C" {
    fn rom_Cache_WriteBack_Addr(addr: u32, size: u32);
    fn Cache_Suspend_DCache_Autoload() -> u32;
    fn Cache_Resume_DCache_Autoload(value: u32);
    fn Cache_Invalidate_Addr(addr: u32, size: u32);
}

/// A format-5 plugin's image as [`Psram::put5`] placed it.
#[cfg(feature = "ppn5")]
#[derive(Debug, Clone, Copy)]
pub struct Placed5 {
    /// The data address of the image's first byte.
    pub base: u32,
    /// Where its hot code runs: always 0, target 9 has none.
    pub hot: u32,
    /// The block's size and alignment, to give it back.
    size: usize,
    align: usize,
}

/// A block of the file held for the small reads of the lookups (the same
/// size as the X4's).
#[cfg(feature = "ppn5")]
const READ_BLOCK: usize = 512;
/// The buffer one pass over the image uses.
#[cfg(feature = "ppn5")]
const PIECE: usize = 4096;

/// The address of the firmware's `PpHeap` table for a plugin's heap word
/// (`abi/heap-word.md`). Until the firmware has one, a plugin that has a
/// word is not loaded (as on the X4: `flash::heap_table`).
#[cfg(feature = "ppn5")]
fn heap_table() -> Option<u32> {
    Some(super::ppn5::heap_word())
}

/// Places images in PSRAM.
pub struct Psram {
    sha: Sha<'static>,
    /// Bytes the images placed at this start take, and how many.
    used: u64,
    placed: u32,
    /// [`Psram::finish`] ran: a hosted image stays until a restart, so nothing
    /// is placed or freed from here.
    finished: bool,
}

impl Psram {
    pub fn new(sha: esp_hal::peripherals::SHA<'static>) -> Self {
        Self {
            sha: Sha::new(sha),
            used: 0,
            placed: 0,
            finished: false,
        }
    }

    /// Reads the image of the plugin file at `path` into PSRAM, checked
    /// by its digest and relocated, and makes it runnable; its data
    /// address, and 0: its hot code runs in place (its calls could not
    /// reach internal RAM from PSRAM, which is fast through its cache
    /// already). `words` follow the image in the file.
    pub fn put(
        &mut self,
        storage: &dyn Storage,
        path: &str,
        header: &Header,
        words: &[u32],
    ) -> Result<(u32, u32), PluginProblem> {
        #[cfg(feature = "remote")]
        super::set_room(super::Room::Psram);
        let len = header.image_len as usize;
        let align = (header.align as usize).max(CACHE_LINE);
        let layout = Layout::from_size_align(len, align).map_err(|_| PluginProblem::Damaged)?;
        // SAFETY: not zero-sized (a header's image never is). Kept for
        // good: plugins are not unloaded while PaperPaw runs.
        let ptr = unsafe { esp_alloc::HEAP.alloc_caps(MemoryCapability::External.into(), layout) };
        if ptr.is_null() {
            return Err(PluginProblem::NoRoom);
        }
        self.used += layout.size().next_multiple_of(align) as u64;
        self.placed += 1;
        // SAFETY: `len` bytes just allocated, used only here.
        let image = unsafe { core::slice::from_raw_parts_mut(ptr, len) };
        let base = ptr as u32;
        let placed = fill(storage, path, header, words, image, base, &mut self.sha);
        if let Err(problem) = placed {
            // SAFETY: allocated above with this layout, nothing else
            // holds it.
            unsafe { alloc::alloc::dealloc(ptr, layout) };
            return Err(problem);
        }
        // Out of the data cache into PSRAM, and nothing old in the
        // instruction cache for the code's addresses.
        // SAFETY: ROM functions on memory this loader owns.
        unsafe {
            let autoload = Cache_Suspend_DCache_Autoload();
            rom_Cache_WriteBack_Addr(base, len as u32);
            Cache_Resume_DCache_Autoload(autoload);
            Cache_Invalidate_Addr(base + CODE_OFFSET, len as u32);
        }
        Ok((base, 0))
    }

    /// [`Psram::put`] for a format-5 file (`ppn5.rs`, target 9): the image is
    /// read into PSRAM through `paperpaw_native::stream5` as on the X4, and
    /// every check that can refuse the file (the digest, each word to
    /// relocate, the entry and the metadata as they will lie once placed)
    /// runs on the PSRAM copy's address before a byte of it is written. The
    /// image runs from where it is read to, through `CODE_OFFSET`; no flash
    /// is touched. A file refused at any point leaves the block freed.
    #[cfg(feature = "ppn5")]
    pub fn put5(
        &mut self,
        storage: &dyn Storage,
        path: &str,
        header: &Header5,
    ) -> Result<Placed5, PluginProblem> {
        if self.finished {
            warn!("native: {path}: placed after the start's last one: not loaded");
            return Err(PluginProblem::NoRoom);
        }
        #[cfg(feature = "remote")]
        super::set_room(super::Room::Psram);
        let table = heap_table();
        if header.heap_at.is_some() && table.is_none() {
            warn!("native: {path} has a heap word and this firmware has no heap table: not loaded");
            return Err(PluginProblem::Unsupported);
        }
        let len = (header.image_len as usize).next_multiple_of(4);
        let align = (header.align as usize).max(CACHE_LINE);
        let layout = Layout::from_size_align(len, align).map_err(|_| PluginProblem::Damaged)?;
        // The buffers first: a full heap is `NoRoom` before PSRAM is taken.
        let mut piece = Vec::new();
        piece
            .try_reserve_exact(PIECE)
            .map_err(|_| PluginProblem::NoRoom)?;
        piece.resize(PIECE, 0u8);
        let mut block = Vec::new();
        block
            .try_reserve_exact(READ_BLOCK)
            .map_err(|_| PluginProblem::NoRoom)?;
        block.resize(READ_BLOCK, 0u8);
        let size = storage.size(path).map_err(|_| PluginProblem::Damaged)?;
        // SAFETY: not zero-sized (a header's image never is). Kept for good
        // once the plugin ran: plugins are not unloaded while PaperPaw runs.
        let ptr = unsafe { esp_alloc::HEAP.alloc_caps(MemoryCapability::External.into(), layout) };
        if ptr.is_null() {
            return Err(PluginProblem::NoRoom);
        }
        let base = ptr as u32;
        let free = |ptr: *mut u8| {
            // SAFETY: allocated above with this layout; nothing else holds it
            // (it was written only through the slice below, now dropped).
            unsafe { alloc::alloc::dealloc(ptr, layout) };
        };
        let file = super::ppn5::StorageSource::new(storage, path);
        let src = Cached::new(&file, size, &mut block);
        let placed = (|| {
            let mut relocator = Relocator5::new(header, &src, base).map_err(super::problem)?;
            if let Some(table) = table {
                relocator = relocator.heap_word(table);
            }
            // Everything that can refuse the file, as the file reads now,
            // before the PSRAM block is written.
            relocator.verify(&mut piece).map_err(super::problem)?;
            relocator
                .check_entry(&super::ppn5::HOST, &mut piece)
                .map_err(|why| super::ppn5::entry_check_failed(path, why))?;
            // SAFETY: `len` bytes just allocated, used only here.
            let image = unsafe { core::slice::from_raw_parts_mut(ptr, len) };
            relocator
                .write_image(&mut piece[..], |done, words| {
                    let at = done as usize;
                    match image.get_mut(at..at + words.len()) {
                        Some(to) => {
                            to.copy_from_slice(words);
                            Ok(())
                        }
                        None => Err(PluginProblem::Damaged),
                    }
                })
                .map_err(|why| match why {
                    Written::File(problem) => super::problem(problem),
                    Written::Sink(problem) => problem,
                })
        })();
        if let Err(why) = placed {
            free(ptr);
            return Err(why);
        }
        // Out of the data cache into PSRAM, and nothing old in the
        // instruction cache for the code's addresses.
        // SAFETY: ROM functions on memory this loader owns.
        unsafe {
            let autoload = Cache_Suspend_DCache_Autoload();
            rom_Cache_WriteBack_Addr(base, len as u32);
            Cache_Resume_DCache_Autoload(autoload);
            Cache_Invalidate_Addr(base + CODE_OFFSET, len as u32);
        }
        self.used += layout.size().next_multiple_of(align) as u64;
        self.placed += 1;
        info!(
            "native: {path}: format 5, {} B in PSRAM at {base:#x}, {} words moved",
            header.image_len, header.relocs
        );
        Ok(Placed5 {
            base,
            hot: 0,
            size: len,
            align,
        })
    }

    /// Takes back a block [`Psram::put5`] placed and the caller refused after
    /// all (its entry or what it needs before `make`).
    #[cfg(feature = "ppn5")]
    pub fn take_back(&mut self, placed: Placed5) {
        if self.finished {
            return;
        }
        if let Ok(layout) = Layout::from_size_align(placed.size, placed.align) {
            // SAFETY: the block `put5` allocated with this layout; the caller
            // refused it before `make`, so nothing runs from it or holds it.
            unsafe { alloc::alloc::dealloc(placed.base as *mut u8, layout) };
            self.used = self
                .used
                .saturating_sub(layout.size().next_multiple_of(placed.align) as u64);
            self.placed = self.placed.saturating_sub(1);
        }
    }

    /// Nothing to do once all are placed.
    pub fn finish(&mut self) {
        self.finished = true;
    }

    /// The room for plugins at this start: what they took, and the PSRAM
    /// free beside it (shared with everything else kept there).
    pub fn room(&self) -> Option<CodeRoom> {
        let free = esp_alloc::HEAP.free_caps(MemoryCapability::External.into()) as u64;
        Some(CodeRoom::in_psram(self.used, free, self.placed))
    }
}

/// Reads the image into `image`, checks the file's digest and relocates
/// it for `base`.
fn fill(
    storage: &dyn Storage,
    path: &str,
    header: &Header,
    words: &[u32],
    image: &mut [u8],
    base: u32,
    engine: &mut Sha<'static>,
) -> Result<(), PluginProblem> {
    let reading = crate::micros();
    super::read_exact(storage, path, HEADER_LEN as u64, image)
        .map_err(|_| PluginProblem::Damaged)?;
    let hashing = crate::micros();
    let mut sha = engine.start::<Sha256>();
    let mut feed = |mut bytes: &[u8]| {
        while !bytes.is_empty() {
            bytes = nb::block!(sha.update(bytes)).unwrap_or_else(|never| match never {});
        }
    };
    feed(&header.digest_prefix());
    feed(image);
    for word in words {
        feed(&word.to_le_bytes());
    }
    let mut digest = [0; 32];
    nb::block!(sha.finish(&mut digest)).unwrap_or_else(|never| match never {});
    if digest != header.sha256 {
        return Err(PluginProblem::Damaged);
    }
    let relocating = crate::micros();
    let api = &API as *const _ as u32;
    Relocator::new(header, words, base, api)
        .and_then(|relocator| relocator.apply(0, image))
        .map_err(super::problem)?;
    log::debug!(
        "native phases: {path}: read {} us, hash {} us, relocate {} us",
        hashing - reading,
        relocating - hashing,
        crate::micros() - relocating,
    );
    Ok(())
}
