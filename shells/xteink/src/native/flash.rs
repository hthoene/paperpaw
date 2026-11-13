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
#[cfg(feature = "ppn5")]
use paperpaw_native::header5::Header5;
#[cfg(feature = "ppn5")]
use paperpaw_native::index::Mark;
use paperpaw_native::index::{Index, Layout, Placement, Region, SECTOR};
#[cfg(feature = "ppn5")]
use paperpaw_native::stream5::{Cached, Relocator5, Written};
use paperpaw_native::{CODE_OFFSET, HEADER_LEN, Header, LINK_BASE, Relocator};
use paperpaw_platform::Storage;
use paperpaw_sdk::PluginProblem;
use paperpaw_sdk::native::CodeRoom;

use super::api::API;
use super::partition::{Behind, PAGE, behind_image, running_partition};

/// An app slot of PaperPaw's own layout (`partitions.csv`).
const OWN_SLOT: u32 = 0x7F_0000;

/// The block of the plugin file the lookups into its relocation words and
/// patches are served from: a sector of the card. Whether it saves enough
/// reads is to be measured on the device.
#[cfg(feature = "ppn5")]
const READ_BLOCK: usize = 512;

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

/// A format-5 plugin's image as [`Flash::put5`] placed it.
#[cfg(feature = "ppn5")]
#[derive(Debug, Clone, Copy)]
pub struct Placed5 {
    /// The data address of the image's first byte.
    pub base: u32,
    /// Where its hot code runs (0: in place, from flash).
    pub hot: u32,
    mark: Mark,
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
    /// [`Flash::finish`] ran: the index is written, nothing is placed or
    /// taken back from here (a hosted image stays where it is until a restart).
    finished: bool,
}

impl Flash {
    /// Places images behind the running firmware, in `flash`.
    pub fn new(flash: crate::ota::Flash) -> Self {
        Self {
            flash,
            layout: None,
            erased: false,
            placed: 0,
            finished: false,
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
            return copy_hot(header.hot_at, header.hot_len, &placement)
                .map(|hot| (placement.base, hot));
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
            .map_err(super::problem)?;
        if placement.hot != 0 {
            relocator = relocator.hot_at(placement.hot).map_err(super::problem)?;
        }
        // The families' words get the firmware's tables with the rest.
        let tables = super::api::tables();
        let mut done = 0;
        while done < header.image_len {
            crate::power::feed();
            let len = (header.image_len - done).min(SECTOR) as usize;
            let bytes = &mut piece[..len];
            super::read_exact(storage, path, HEADER_LEN as u64 + u64::from(done), bytes)
                .map_err(|_| PluginProblem::Damaged)?;
            relocator
                .apply_with(done, bytes, &tables)
                .map_err(super::problem)?;
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
        copy_hot(header.hot_at, header.hot_len, &placement).map(|hot| (placement.base, hot))
    }

    /// [`Flash::put`] for a format-5 file (`ppn5.rs`): the same room, the
    /// same index, written in the same order, but the file is read through
    /// `paperpaw_native::stream5` (no words held in memory), and every check
    /// that can refuse it (the digest and each word to relocate) runs
    /// before the flash is touched. A file refused at any point after it
    /// was placed is taken back from the layout, so the index never lists
    /// it and the room and the hot code's part of the arena are free again;
    /// what was written lies behind the index's end, unreferenced.
    #[cfg(feature = "ppn5")]
    pub fn put5(
        &mut self,
        storage: &dyn Storage,
        path: &str,
        header: &Header5,
    ) -> Result<Placed5, PluginProblem> {
        if self.finished {
            warn!("native: {path}: placed after the index was written: not loaded");
            return Err(PluginProblem::NoRoom);
        }
        let flash = &self.flash;
        let layout = self
            .layout
            .get_or_insert_with(|| Self::layout(&mut flash.borrow_mut()))
            .as_mut()
            .ok_or(PluginProblem::NoRoom)?;
        let mark = layout.mark();
        let placement = layout
            .place_parts(
                header.sha256,
                header.image_len,
                header.align,
                header.hot_len,
            )
            .map_err(|_| PluginProblem::NoRoom)?;
        self.placed += 1;
        let mut touched = false;
        let written = Self::write5(
            flash,
            (&mut self.erased, &mut touched),
            layout,
            storage,
            path,
            header,
            &placement,
        );
        match written {
            Ok(hot) => Ok(Placed5 {
                base: placement.base,
                hot,
                mark,
            }),
            Err(why) => {
                // Refused before a sector was erased: the layout is as it
                // was, and so is what the next plugins cost. After: the
                // index is written anew.
                if touched {
                    layout.rewind(mark);
                } else {
                    layout.forget(mark);
                }
                self.placed -= 1;
                Err(why)
            }
        }
    }

    /// Takes back a plugin [`Flash::put5`] placed and the caller refused
    /// after all (its entry): the layout forgets it.
    #[cfg(feature = "ppn5")]
    pub fn take_back(&mut self, placed: Placed5) {
        if self.finished {
            return;
        }
        if let Some(Some(layout)) = &mut self.layout {
            layout.rewind(placed.mark);
            self.placed = self.placed.saturating_sub(1);
        }
    }

    #[cfg(feature = "ppn5")]
    fn write5(
        flash: &crate::ota::Flash,
        (erased, touched): (&mut bool, &mut bool),
        layout: &Layout,
        storage: &dyn Storage,
        path: &str,
        header: &Header5,
        placement: &Placement,
    ) -> Result<u32, PluginProblem> {
        // The host heap word (`abi/heap-word.md`): the address of the
        // firmware's `PpHeap` table, written into the plugin's word while
        // it is placed. Until the firmware has such a table a plugin that
        // has a word cannot be given one: not loaded, nothing touched.
        let table = heap_table();
        if header.heap_at.is_some() && table.is_none() {
            warn!("native: {path} has a heap word and this firmware has no heap table: not loaded");
            return Err(PluginProblem::Unsupported);
        }
        if placement.present {
            info!("native: {path} lies in flash at {:#x}", placement.at);
            check_heap_word(path, header, placement, table)?;
            return copy_hot(header.hot_at, header.hot_len, placement);
        }
        let mut piece = Vec::new();
        piece
            .try_reserve_exact(SECTOR as usize)
            .map_err(|_| PluginProblem::NoRoom)?;
        piece.resize(SECTOR as usize, 0);
        // The lookups into the relocation words and the patches are tiny
        // reads; the block of the file read last answers the next ones.
        let mut block = Vec::new();
        block
            .try_reserve_exact(READ_BLOCK)
            .map_err(|_| PluginProblem::NoRoom)?;
        block.resize(READ_BLOCK, 0);
        let size = storage.size(path).map_err(|_| PluginProblem::Damaged)?;
        let file = super::ppn5::StorageSource::new(storage, path);
        let src = Cached::new(&file, size, &mut block);
        let mut relocator =
            Relocator5::new(header, &src, placement.base).map_err(super::problem)?;
        if placement.hot != 0 {
            relocator = relocator.hot_at(placement.hot).map_err(super::problem)?;
        }
        // The word is filled in the piece buffer after its piece is
        // relocated, before the sector is programmed: the same pass as the
        // rest of the sector, never rewritten in place.
        if let Some(table) = table {
            relocator = relocator.heap_word(table);
        }
        // Everything that can refuse the file, on the file as read now and
        // before a sector of the flash is erased: the digest, each word to
        // relocate, and the entry and metadata as they will lie once placed.
        // A file refused here costs no write, now or at any later start.
        relocator.verify(&mut piece).map_err(super::problem)?;
        relocator
            .check_entry(&super::ppn5::HOST, &mut piece)
            .map_err(|why| super::ppn5::entry_check_failed(path, why))?;
        *touched = true;
        if placement.erase_index && !*erased {
            let start = layout.region().start;
            flash
                .borrow_mut()
                .erase(start, start + SECTOR)
                .map_err(|e| written_not(path, e))?;
            *erased = true;
        }
        let end = (placement.at + header.image_len)
            .next_multiple_of(PAGE)
            .min(layout.region().end);
        flash
            .borrow_mut()
            .erase(placement.at, end)
            .map_err(|e| written_not(path, e))?;
        // The write pass hashes each piece as it reads it and the lists
        // again after the image: what lands in the flash is what the
        // header's digest covers, whatever the card answers now.
        relocator
            .write_image(&mut piece[..], |done, words| {
                flash
                    .borrow_mut()
                    .write_nor(placement.at + done, words)
                    .map_err(|e| written_not(path, e))
            })
            .map_err(|why| match why {
                Written::File(problem) => super::problem(problem),
                Written::Sink(problem) => problem,
            })?;
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
        check_heap_word(path, header, placement, table)?;
        copy_hot(header.hot_at, header.hot_len, placement)
    }

    /// The room for plugins at this start: the region they were placed
    /// in, or, where none was placed or none could be, the one they would
    /// be (found, not mapped): little or nothing behind an image in a
    /// smaller slot (another firmware's partitions, CrossPoint's), which
    /// the store then says has no room. `None` where the running image
    /// cannot be measured.
    pub fn room(&self) -> Option<CodeRoom> {
        let (start, end, images_end, free_total) = match &self.layout {
            Some(Some(layout)) => {
                let region = layout.region();
                // The entries the region itself holds are free again for a
                // full flash.
                let mapped = region.end.div_ceil(PAGE) - region.start / PAGE;
                (
                    region.start,
                    region.end,
                    layout.end(),
                    free_entries().1 + mapped,
                )
            }
            Some(None) | None => {
                let behind = behind_image(&mut self.flash.borrow_mut()).ok()?;
                let (_, free) = free_entries();
                let first_page = behind.start / PAGE;
                let end = behind.end.min((first_page + free) * PAGE);
                (behind.start, end, behind.start, free)
            }
        };
        let room = CodeRoom::in_flash(start, end, images_end, self.placed);
        // A slot smaller than PaperPaw's own (a CrossPoint SD install):
        // the store says a full flash gives more, as much as the MMU's
        // free entries let plugins use of its larger slot, not the raw
        // difference.
        Some(match running_partition(&mut self.flash.borrow_mut()) {
            Ok((offset, slot)) if slot < OWN_SLOT => {
                let len = paperpaw_x4::flash_room::full_flash_len(
                    start, offset, OWN_SLOT, free_total, PAGE,
                );
                let full = CodeRoom::in_flash(start, start.saturating_add(len), start, 0);
                let growth = u32::try_from(full.total.saturating_sub(room.total)).unwrap_or(0);
                room.in_slot(slot, slot.saturating_add(growth))
            }
            _ => room,
        })
    }

    /// Writes the index, if what lies in the region changed, and gives
    /// what hot code did not take of the arena to the heap.
    pub fn finish(&mut self) {
        self.finished = true;
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

/// The address of the firmware's `PpHeap` table for plugins' heap words
/// (`abi/heap-word.md`), or `None` while the firmware has none. It is the
/// same for the whole run and is written into a placed image only while the
/// image is placed; the index is keyed by the firmware build, so a firmware
/// with another table re-places every plugin.
#[cfg(feature = "ppn5")]
fn heap_table() -> Option<u32> {
    Some(super::ppn5::heap_word())
}

/// Before `make`: the word placed in flash for `header` holds the address of
/// this firmware's table. A plugin without a word has nothing to check. A
/// word that differs (a damaged or foreign region: the build-keyed index
/// already re-placed everything it would) is `Damaged`, and the plugin is
/// never run.
#[cfg(feature = "ppn5")]
fn check_heap_word(
    path: &str,
    header: &Header5,
    placement: &Placement,
    table: Option<u32>,
) -> Result<(), PluginProblem> {
    let Some(at) = header.heap_at else {
        return Ok(());
    };
    let placed = placement
        .base
        .checked_add(at)
        .filter(|_| at.checked_add(4).is_some_and(|end| end <= header.code_at));
    let (Some(placed), Some(table)) = (placed, table) else {
        return Err(PluginProblem::Damaged);
    };
    // SAFETY: `at` is a 4-aligned word of the image's read-only data
    // (`Header5::read` checked it), which the MMU maps at `placement.base`
    // for the whole session; the cache was invalidated after a write.
    let found = unsafe { core::ptr::read_volatile(placed as *const u32) };
    if found != table {
        warn!("native: {path}: the heap word holds {found:#x}, not {table:#x}: not loaded");
        return Err(PluginProblem::Damaged);
    }
    Ok(())
}

/// Copies the hot code of the image placed as `placement` says, relocated
/// for its part of the arena, from the image mapped there; where it runs
/// (0: in place, from flash).
fn copy_hot(hot_at: u32, hot_len: u32, placement: &Placement) -> Result<u32, PluginProblem> {
    if placement.hot == 0 {
        return Ok(0);
    }
    let from = placement.base + hot_at;
    // SAFETY: the image's hot code, in the region mapped (its data
    // address), written and its cache lines invalidated before.
    if unsafe { super::hot::copy(from, placement.hot, hot_len) } {
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
