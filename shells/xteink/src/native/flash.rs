//! The X4: its chip has no PSRAM, and its RAM is too small to run
//! plugins from, so their code runs from flash. Images are written once
//! into the free end of the app partition the firmware runs from, behind
//! its image, one after the other; the index there
//! (`paperpaw_native::index`) says what lies where, so later starts only
//! map them. The region is mapped through MMU entries the firmware leaves
//! free, at the end of the table, so code and data of every plugin lie in
//! one window of addresses.
//!
//! - Updates are written into the other app partition: plugins in use are
//!   never erased under the code, and going back to the firmware before
//!   finds its own plugins behind it (the index holds the firmware it was
//!   made by).
//! - A bundle the web flasher wrote behind the image (`PPNB`) is left
//!   alone: the region is used once the bundle is installed.
//! - Hot code runs from internal RAM (`hot`): each image is relocated for
//!   its part of the arena, and its hot code copied there at every start.

use alloc::vec::Vec;

use esp_bootloader_esp_idf::partitions::FlashStorage;
use esp_hal::peripherals::MMU_TABLE;
use log::{info, warn};
use paperpaw_native::bundle::BUNDLE_MAGIC;
use paperpaw_native::index::{Index, Layout, Placement, Region, SECTOR};
use paperpaw_native::{CODE_OFFSET, HEADER_LEN, Header, LINK_BASE, Relocator};
use paperpaw_platform::Storage;
use paperpaw_sdk::PluginProblem;
use paperpaw_sdk::native::CodeRoom;

use super::api::API;
use super::partition::{Behind, PAGE, behind_image};

/// The MMU's entries (data and instruction bus share them).
const ENTRIES: u32 = 128;

unsafe extern "C" {
    fn Cache_Dbus_MMU_Set(
        ext_ram: u32,
        vaddr: u32,
        paddr: u32,
        psize: u32,
        num: u32,
        fixed: u32,
    ) -> i32;
    fn Cache_Invalidate_Addr(addr: u32, size: u32);
}

/// Places images in flash.
pub struct Flash {
    flash: crate::ota::Flash,
    /// Where they go, found and mapped for the first plugin (once safe
    /// mode counts the start): `None` before, `Some(None)` if there is no
    /// room (no free MMU entries, a bundle to install first).
    layout: Option<Option<Layout>>,
    /// The index sector was erased at this start.
    erased: bool,
    /// Plugins placed at this start.
    placed: u32,
}

impl Flash {
    /// Places images behind the running firmware, in `flash`.
    pub fn new(flash: crate::ota::Flash) -> Self {
        Self {
            flash,
            layout: None,
            erased: false,
            placed: 0,
        }
    }

    /// The plugins' region, found and mapped, with what its index says.
    fn layout(flash: &mut FlashStorage<'static>) -> Option<Layout> {
        match region(flash) {
            Ok((region, firmware)) => {
                #[cfg(feature = "remote")]
                super::set_room(super::Room::Flash {
                    start: region.start,
                    end: region.end,
                    base: region.base,
                });
                let found = read_index(flash, region.start);
                Some(Layout::new(
                    region,
                    firmware,
                    &API as *const _ as u32,
                    super::hot::arena(),
                    found,
                ))
            }
            Err(why) => {
                warn!("native: no room for code plugins: {why}");
                #[cfg(feature = "remote")]
                super::set_room(super::Room::None(why));
                None
            }
        }
    }

    /// Writes the image of the plugin file at `path` into the region,
    /// relocated, unless it lies there already, and copies its hot code
    /// into the arena; its data address and where its hot code runs (0:
    /// in place). `words` follow the image in the file.
    pub fn put(
        &mut self,
        storage: &dyn Storage,
        path: &str,
        header: &Header,
        words: &[u32],
    ) -> Result<(u32, u32), PluginProblem> {
        let flash = &self.flash;
        let layout = self
            .layout
            .get_or_insert_with(|| Self::layout(&mut flash.borrow_mut()))
            .as_mut()
            .ok_or(PluginProblem::NoRoom)?;
        let placement = layout.place(header).map_err(|_| PluginProblem::NoRoom)?;
        self.placed += 1;
        if placement.present {
            info!("native: {path} lies in flash at {:#x}", placement.at);
            return copy_hot(header, &placement).map(|hot| (placement.base, hot));
        }
        // A sector at a time, for checking and for writing; written only
        // once checked whole, so a damaged file never gets into the index.
        let mut piece = Vec::new();
        piece
            .try_reserve_exact(SECTOR as usize)
            .map_err(|_| PluginProblem::NoRoom)?;
        piece.resize(SECTOR as usize, 0);
        check(storage, path, header, words, &mut piece)?;
        if placement.erase_index && !self.erased {
            let start = layout.region().start;
            self.flash
                .borrow_mut()
                .erase(start, start + SECTOR)
                .map_err(|e| written_not(path, e))?;
            self.erased = true;
        }
        // Erased at once, to the end of its last 64 KiB block: what lies
        // behind is free or written anew, and blocks erase faster than
        // their sectors.
        let end = (placement.at + header.image_len)
            .next_multiple_of(PAGE)
            .min(layout.region().end);
        self.flash
            .borrow_mut()
            .erase(placement.at, end)
            .map_err(|e| written_not(path, e))?;
        let mut relocator = Relocator::new(header, words, placement.base, &API as *const _ as u32)
            .map_err(super::problem)?
            .tables(super::api::tables());
        if placement.hot != 0 {
            relocator = relocator.hot_at(placement.hot).map_err(super::problem)?;
        }
        let mut done = 0;
        while done < header.image_len {
            crate::power::feed();
            let len = (header.image_len - done).min(SECTOR) as usize;
            let bytes = &mut piece[..len];
            super::read_exact(storage, path, HEADER_LEN as u64 + u64::from(done), bytes)
                .map_err(|_| PluginProblem::Damaged)?;
            relocator.apply(done, bytes).map_err(super::problem)?;
            // Whole words: the last piece filled up as erased flash.
            let words_len = len.next_multiple_of(4);
            piece[len..words_len].fill(0xFF);
            let at = placement.at + done;
            self.flash
                .borrow_mut()
                .write_nor(at, &piece[..words_len])
                .map_err(|e| written_not(path, e))?;
            done += len as u32;
        }
        // Nothing of what the region held before stays in the caches.
        let len = header.image_len.next_multiple_of(4);
        // SAFETY: ROM functions on addresses mapped to this region.
        unsafe {
            Cache_Invalidate_Addr(placement.base, len);
            Cache_Invalidate_Addr(placement.base + CODE_OFFSET, len);
        }
        info!(
            "native: {path} written into flash at {:#x} ({} sectors)",
            placement.at,
            header.image_len.div_ceil(SECTOR)
        );
        copy_hot(header, &placement).map(|hot| (placement.base, hot))
    }

    /// The room for plugins at this start: the region they were placed
    /// in, or, where none was placed or none could be, the one they would
    /// be (found, not mapped): little or nothing behind an image in a
    /// smaller slot (another firmware's partitions, CrossPoint's), which
    /// the store then says has no room. `None` where the running image
    /// cannot be measured.
    pub fn room(&self) -> Option<CodeRoom> {
        let (start, end, images_end) = match &self.layout {
            Some(Some(layout)) => {
                let region = layout.region();
                (region.start, region.end, layout.end())
            }
            Some(None) | None => {
                let behind = behind_image(&mut self.flash.borrow_mut()).ok()?;
                let (_, free) = free_entries();
                let first_page = behind.start / PAGE;
                let end = behind.end.min((first_page + free) * PAGE);
                (behind.start, end, behind.start)
            }
        };
        Some(CodeRoom::in_flash(start, end, images_end, self.placed))
    }

    /// Writes the index, if what lies in the region changed, and gives
    /// what hot code did not take of the arena to the heap.
    pub fn finish(&mut self) {
        let Some(Some(layout)) = &self.layout else {
            super::hot::release(0);
            return;
        };
        super::hot::release(layout.hot_used());
        let Some(index) = layout.index() else {
            return;
        };
        let start = layout.region().start;
        let mut bytes = index.to_bytes();
        bytes.resize(bytes.len().next_multiple_of(4), 0xFF);
        let mut flash = self.flash.borrow_mut();
        let written = flash
            .erase(start, start + SECTOR)
            .and_then(|()| flash.write_nor(start, &bytes));
        match written {
            Ok(()) => info!(
                "native: index of {} plugins written, {} KB of the region used",
                index.placed.len(),
                (layout.end() - start) / 1024
            ),
            Err(e) => warn!("native: the index: {e:?}"),
        }
    }
}

/// Copies the hot code of the image placed as `placement` says, relocated
/// for its part of the arena, from the image mapped there; where it runs
/// (0: in place, from flash).
fn copy_hot(header: &Header, placement: &Placement) -> Result<u32, PluginProblem> {
    if placement.hot == 0 {
        return Ok(0);
    }
    let from = placement.base + header.hot_at;
    // SAFETY: the image's hot code, in the region mapped (its data
    // address), written and its cache lines invalidated before.
    if unsafe { super::hot::copy(from, placement.hot, header.hot_len) } {
        Ok(placement.hot)
    } else {
        Err(PluginProblem::NoRoom)
    }
}

/// The flash could not be written: said, and taken for no room (the
/// region cannot be used).
fn written_not(path: &str, error: impl core::fmt::Debug) -> PluginProblem {
    warn!("native: {path}: the flash: {error:?}");
    PluginProblem::NoRoom
}

/// Checks the file's digest, reading its image once, a `buf` at a time.
fn check(
    storage: &dyn Storage,
    path: &str,
    header: &Header,
    words: &[u32],
    buf: &mut [u8],
) -> Result<(), PluginProblem> {
    let mut sha = header.digest();
    let mut done = 0u32;
    while done < header.image_len {
        let len = (header.image_len - done).min(buf.len() as u32) as usize;
        super::read_exact(
            storage,
            path,
            HEADER_LEN as u64 + u64::from(done),
            &mut buf[..len],
        )
        .map_err(|_| PluginProblem::Damaged)?;
        sha.update(&buf[..len]);
        done += len as u32;
    }
    super::digest_words(&mut sha, words);
    if header.holds(sha) {
        Ok(())
    } else {
        Err(PluginProblem::Damaged)
    }
}

/// The region behind the running firmware's image, mapped, and the
/// firmware (the SHA-256 its image ends with); or why there is none.
fn region(flash: &mut FlashStorage<'static>) -> Result<(Region, [u8; 32]), &'static str> {
    let Behind {
        start,
        end,
        firmware,
    } = behind_image(flash)?;
    let mut head = [0u8; 4];
    flash
        .read(start, &mut head)
        .map_err(|_| "the flash cannot be read")?;
    if head == BUNDLE_MAGIC {
        return Err("a bundle of plugins waits to be installed");
    }
    // As many pages as the region spans, in the longest run of entries the
    // MMU leaves free.
    let first_page = start / PAGE;
    let (entry, free) = free_entries();
    let pages = (end.div_ceil(PAGE) - first_page).min(free);
    if pages == 0 {
        return Err("no MMU entries are free");
    }
    let vaddr = LINK_BASE + entry * PAGE;
    // SAFETY: entries found free, mapped to the region only this module
    // writes.
    let set = unsafe { Cache_Dbus_MMU_Set(0, vaddr, first_page * PAGE, 64, pages, 0) };
    if set != 0 {
        return Err("the MMU did not map the region");
    }
    // SAFETY: ROM functions on the addresses just mapped.
    unsafe {
        Cache_Invalidate_Addr(vaddr, pages * PAGE);
        Cache_Invalidate_Addr(vaddr + CODE_OFFSET, pages * PAGE);
    }
    let end = end.min((first_page + pages) * PAGE);
    let base = vaddr + (start - first_page * PAGE);
    info!(
        "native: the region {start:#x}..{end:#x} ({} KB) mapped at {base:#x}, MMU entries {entry}..{}",
        (end - start) / 1024,
        entry + pages
    );
    let region = Region::new(start, end, base).ok_or("the region is too small")?;
    Ok((region, firmware))
}

/// The longest run of MMU entries that map nothing (the later of two as
/// long): its first entry and its length. The firmware's own lie at the
/// table's start; the bootloader may leave one of its own at the end.
fn free_entries() -> (u32, u32) {
    let table = MMU_TABLE::regs();
    let mut best = (0, 0);
    let mut run = (0, 0);
    for i in 0..ENTRIES {
        if table.entry(i as usize).read().invalid().bit() {
            run = if run.1 == 0 {
                (i, 1)
            } else {
                (run.0, run.1 + 1)
            };
            if run.1 >= best.1 {
                best = run;
            }
        } else {
            run = (0, 0);
        }
    }
    best
}

/// The index at the region's start, if one is there.
fn read_index(flash: &mut FlashStorage<'static>, start: u32) -> Option<Index> {
    let mut head = [0u8; 8];
    flash.read(start, &mut head).ok()?;
    let len = Index::byte_len(&head)?;
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(len).ok()?;
    bytes.resize(len, 0);
    flash.read(start, &mut bytes).ok()?;
    Index::read(&bytes)
}
