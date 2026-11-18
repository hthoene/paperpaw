//! The microSD card of the X4 Pro and the X4 Classic, on the ESP32-S3's
//! SD host (SDMMC, one data line), for PaperPaw's FAT storage: started as
//! the SD specification says, then read and written block by block
//! through a buffer in the chip's own RAM (its DMA does not reach the
//! PSRAM).

use alloc::boxed::Box;
use core::cell::UnsafeCell;

use esp_hal::Blocking;
use esp_hal::delay::Delay;
use esp_hal::dma::aligned::{DmaAlignedMut, InternalMemory};
use esp_hal::sdmmc::{
    BusWidth, CommandFlags, Config, ResponseLen, SdHostController, Slot, SlotConfig,
};
use esp_hal::time::{Duration, Instant};
use log::{info, warn};
use paperpaw_core::CardTrouble;
use paperpaw_fat::{BLOCK, Blocks, DiskError, Restart};

use crate::board::CardParts;
use crate::volume::Mounted;

/// The card's clock while it starts, and after (the default speed's most).
const START_HZ: u32 = 400_000;
const RUN_HZ: u32 = 20_000_000;
/// Blocks moved at once through the buffer.
const AT_ONCE: usize = 16;
/// How long a card may take to write blocks (the SD specification allows
/// SDXC cards 500 ms), with room.
const WRITE_MS: u64 = 1000;
/// Times a write is tried, the card ready again between: one the host
/// gave up on while the card was still busy goes through the next time.
/// The host waits for a busy card a number of polls, not a time (esp-hal's
/// `POLL_LIMIT`), some tens of milliseconds, less than a card may take; a
/// card that tidies up while the radio is busy can outlast it a few times
/// over.
const WRITE_TRIES: u32 = 6;
/// Between polls of a card that is still writing, in microseconds.
const POLL_US: u32 = 20;
/// Times the card is started: the X4 Classic's up to four, its power off
/// and on before each (the FreeInk SDK's `SdmmcBlockDevice`, MIT); the
/// X4 Pro's once.
#[cfg(feature = "x4-classic")]
const TRIES: u32 = 4;
#[cfg(not(feature = "x4-classic"))]
const TRIES: u32 = 1;

/// The buffer the SD host's DMA moves blocks through: in the chip's RAM
/// (a static, never on the heap, which reaches into PSRAM).
type Buffer = InternalMemory<[u8; BLOCK * AT_ONCE]>;
struct Shared(UnsafeCell<Buffer>);
// SAFETY: only the card takes it, once (`mount`), and uses it from the
// main loop alone.
unsafe impl Sync for Shared {}
static BUFFER: Shared = Shared(UnsafeCell::new(InternalMemory::new([0; BLOCK * AT_ONCE])));

type CardSlot = Slot<'static, 1, Blocking>;

/// The card's FAT file system as PaperPaw's storage, the card watched for
/// being taken out and put back (see `volume::mount_swappable`); without a
/// card or one PaperPaw cannot read, no storage.
// A frame of its own, off the stack before the kernel is built.
#[inline(never)]
pub fn mount(parts: CardParts) -> Result<Mounted, CardTrouble> {
    let CardParts {
        host,
        clock,
        command,
        data,
        delay,
        #[cfg(feature = "x4-classic")]
        mut power,
    } = parts;
    delay.delay_millis(20);
    let Ok(host) = SdHostController::new(host, Config::default()) else {
        warn!("SD card: no host");
        return Err(CardTrouble::Missing);
    };
    let host: &'static SdHostController<'static> = Box::leak(Box::new(host));
    let Ok(slot) = host.slot::<1>(SlotConfig::default()) else {
        warn!("SD card: no slot");
        return Err(CardTrouble::Missing);
    };
    let mut slot = slot.with_clk(clock).with_cmd(command).with_data0(data);
    let started = Card::start_tries(
        &mut slot,
        &delay,
        #[cfg(feature = "x4-classic")]
        &mut power,
    );
    // SAFETY: mounted once, so the buffer is taken once.
    let buffer = unsafe { &mut *BUFFER.0.get() };
    match started.map(|found| Card {
        slot,
        buffer,
        rca: found.rca,
        by_block: found.by_block,
        blocks: found.blocks,
        delay,
        #[cfg(feature = "x4-classic")]
        power,
    }) {
        Ok(card) => {
            info!("SD card: {} MB", card.blocks >> 11);
            crate::volume::mount_swappable(card)
        }
        Err(e) => {
            warn!("SD card: {e}");
            Err(CardTrouble::Missing)
        }
    }
}

/// A started card.
struct Card {
    slot: CardSlot,
    buffer: &'static mut Buffer,
    /// Its address, for the commands that name it.
    rca: u32,
    /// Addressed by block (SDHC, SDXC), else by byte.
    by_block: bool,
    blocks: u32,
    delay: Delay,
    /// The X4 Classic's card power, off and on before each start.
    #[cfg(feature = "x4-classic")]
    power: esp_hal::gpio::Output<'static>,
}

/// What starting a card found out.
struct Found {
    rca: u32,
    by_block: bool,
    blocks: u32,
}

const NONE: CommandFlags = CommandFlags {
    wait_complete: false,
    stop_abort: false,
    busy: false,
};

impl Card {
    /// Starts the card in `slot` as often as the device tries ([`TRIES`]),
    /// the X4 Classic's power off and on before each.
    fn start_tries(
        slot: &mut CardSlot,
        delay: &Delay,
        #[cfg(feature = "x4-classic")] power: &mut esp_hal::gpio::Output<'static>,
    ) -> Result<Found, &'static str> {
        let mut started = Err("not started");
        for attempt in 1..=TRIES {
            #[cfg(feature = "x4-classic")]
            crate::board::power_card(power, delay);
            started = Card::start(slot, delay);
            match started {
                Err(e) if attempt < TRIES => warn!("SD card: {e}, started again"),
                _ => break,
            }
        }
        started
    }

    /// Starts the card in `slot`, as the SD specification says.
    fn start(slot: &mut CardSlot, delay: &Delay) -> Result<Found, &'static str> {
        slot.set_bus_low_level(BusWidth::Bit1, START_HZ)
            .map_err(|_| "bus")?;
        slot.send_init_sequence().map_err(|_| "no clock")?;
        let _ = slot.command_blocking(0, 0, ResponseLen::None, false, NONE);
        // Version 2 cards echo the pattern.
        let v2 = matches!(
            slot.command_blocking(8, 0x1AA, ResponseLen::Short, true, NONE),
            Ok(r) if r[0] & 0xFFF == 0x1AA
        );
        let mut ocr = 0;
        for _ in 0..200 {
            slot.command_blocking(55, 0, ResponseLen::Short, true, NONE)
                .map_err(|_| "no card")?;
            let capacity = if v2 { 0x4000_0000 } else { 0 };
            let r = slot
                .command_blocking(41, 0x0030_0000 | capacity, ResponseLen::Short, false, NONE)
                .map_err(|_| "no answer")?;
            if r[0] & 0x8000_0000 != 0 {
                ocr = r[0];
                break;
            }
            delay.delay_millis(10);
        }
        if ocr == 0 {
            return Err("never ready");
        }
        let by_block = ocr & 0x4000_0000 != 0;
        slot.command_blocking(2, 0, ResponseLen::Long, false, NONE)
            .map_err(|_| "no identity")?;
        let r = slot
            .command_blocking(3, 0, ResponseLen::Short, true, NONE)
            .map_err(|_| "no address")?;
        let rca = r[0] & 0xFFFF_0000;
        let csd = slot
            .command_blocking(9, rca, ResponseLen::Long, false, NONE)
            .map_err(|_| "no size")?;
        let blocks = blocks_of(csd);
        let busy = CommandFlags { busy: true, ..NONE };
        slot.command_blocking(7, rca, ResponseLen::Short, true, busy)
            .map_err(|_| "not selected")?;
        if !by_block {
            slot.command_blocking(16, BLOCK as u32, ResponseLen::Short, true, NONE)
                .map_err(|_| "block size")?;
        }
        slot.set_bus_low_level(BusWidth::Bit1, RUN_HZ)
            .map_err(|_| "speed")?;
        Ok(Found {
            rca,
            by_block,
            blocks,
        })
    }

    fn address(&self, index: u32) -> u32 {
        if self.by_block {
            index
        } else {
            index * BLOCK as u32
        }
    }

    /// The card's state (its status, bits 12..9), if it answers.
    fn state(&mut self) -> Option<u32> {
        let r = self
            .slot
            .command_blocking(13, self.rca, ResponseLen::Short, true, NONE)
            .ok()?;
        Some((r[0] >> 9) & 0xF)
    }

    /// Waits until the card is ready for the next transfer (after a write).
    fn ready(&mut self) -> Result<(), DiskError> {
        let since = Instant::now();
        loop {
            let r = self
                .slot
                .command_blocking(13, self.rca, ResponseLen::Short, true, NONE)
                .map_err(|_| DiskError)?;
            // Ready for data, in the transfer state.
            if r[0] & 0x100 != 0 && (r[0] >> 9) & 0xF == 4 {
                return Ok(());
            }
            if since.elapsed() > Duration::from_millis(WRITE_MS) {
                return Err(DiskError);
            }
            self.delay.delay_micros(POLL_US);
        }
    }
}

/// The card's size in blocks, from its CSD (the four words as the host
/// gives them: bits 127..96 first).
fn blocks_of(csd: [u32; 4]) -> u32 {
    // The host shifts the 136-bit answer: word 0 holds bits 31..0 of the
    // CSD without its CRC. The structure version is in bits 127..126.
    let bit = |n: u32| (csd[(n / 32) as usize] >> (n % 32)) & 1;
    let bits = |high: u32, low: u32| (low..=high).rev().fold(0u32, |v, n| (v << 1) | bit(n));
    match bits(127, 126) {
        // CSD 2.0: (C_SIZE + 1) Ã— 512 KiB.
        1 => (bits(69, 48) + 1).saturating_mul(1024),
        // CSD 1.0: (C_SIZE + 1) Ã— 2^(C_SIZE_MULT + 2) Ã— 2^READ_BL_LEN bytes.
        _ => {
            let c_size = bits(73, 62);
            let mult = bits(49, 47);
            let read_bl_len = bits(83, 80);
            let bytes = u64::from(c_size + 1) << (mult + 2 + read_bl_len);
            (bytes / BLOCK as u64) as u32
        }
    }
}

impl Restart for Card {
    /// Started as at the start (a card put in waits for that), the card
    /// in the slot now taken as another: its address and size anew.
    fn restart(&mut self) -> Result<(), DiskError> {
        let started = Card::start_tries(
            &mut self.slot,
            &self.delay,
            #[cfg(feature = "x4-classic")]
            &mut self.power,
        );
        match started {
            Ok(found) => {
                self.rca = found.rca;
                self.by_block = found.by_block;
                self.blocks = found.blocks;
                info!("SD card: started again, {} MB", self.blocks >> 11);
                Ok(())
            }
            Err(e) => {
                warn!("SD card: not started again: {e}");
                Err(DiskError)
            }
        }
    }
}

impl Blocks for Card {
    fn read_block(&mut self, index: u32, block: &mut [u8; BLOCK]) -> Result<(), DiskError> {
        self.read_blocks(index, block)
    }

    fn write_block(&mut self, index: u32, block: &[u8; BLOCK]) -> Result<(), DiskError> {
        self.write_blocks(index, block)
    }

    fn read_blocks(&mut self, index: u32, data: &mut [u8]) -> Result<(), DiskError> {
        // Long work on the card must not look hung to the watchdogs.
        crate::power::working();
        #[cfg(feature = "remote")]
        crate::traced::blocks(index, (data.len() / BLOCK) as u32, 0);
        let mut at = index;
        for chunk in data.chunks_mut(BLOCK * AT_ONCE) {
            let count = chunk.len() / BLOCK;
            let command = if count == 1 { 17 } else { 18 };
            let address = self.address(at);
            let mut aligned = self.buffer.get_mut();
            let into = DmaAlignedMut::new(&mut aligned[..chunk.len()]).map_err(|_| DiskError)?;
            self.slot
                .read_blocks_blocking(command, address, into, BLOCK as u16, count as u32)
                .map_err(|_| DiskError)?;
            chunk.copy_from_slice(&aligned[..chunk.len()]);
            at += count as u32;
        }
        Ok(())
    }

    fn write_blocks(&mut self, index: u32, data: &[u8]) -> Result<(), DiskError> {
        crate::power::working();
        #[cfg(feature = "remote")]
        crate::traced::blocks(index, 0, (data.len() / BLOCK) as u32);
        let mut at = index;
        for chunk in data.chunks(BLOCK * AT_ONCE) {
            let count = chunk.len() / BLOCK;
            let command = if count == 1 { 24 } else { 25 };
            let address = self.address(at);
            let mut tries = 0;
            loop {
                let began = Instant::now();
                let written = {
                    let mut aligned = self.buffer.get_mut();
                    aligned[..chunk.len()].copy_from_slice(chunk);
                    let from =
                        DmaAlignedMut::new(&mut aligned[..chunk.len()]).map_err(|_| DiskError)?;
                    self.slot.write_blocks_blocking(
                        command,
                        address,
                        from,
                        BLOCK as u16,
                        count as u32,
                    )
                };
                // Cut short: the card is told the transfer ended, where it
                // still waits for blocks (receiving, 6) or sends (data, 5).
                if written.is_err() && (count > 1 || matches!(self.state(), Some(5 | 6))) {
                    let busy = CommandFlags { busy: true, ..NONE };
                    let _ = self
                        .slot
                        .command_blocking(12, 0, ResponseLen::Short, true, busy);
                }
                // Written, or not: the card may still be busy writing
                // (some take longer than the host waits for them), and
                // takes nothing new until it is done.
                let ready = self.ready();
                let ms = began.elapsed().as_millis();
                tries += 1;
                match written {
                    Ok(_) if ready.is_ok() => break,
                    Ok(_) => warn!(
                        "card: not ready {ms} ms after writing {count} blocks at {at}, try {tries}"
                    ),
                    Err(e) => warn!(
                        "card: writing {count} blocks at {at}: {e:?} after {ms} ms, try {tries}"
                    ),
                }
                if tries == WRITE_TRIES {
                    return Err(DiskError);
                }
            }
            at += count as u32;
        }
        Ok(())
    }

    fn blocks(&mut self) -> Result<u32, DiskError> {
        Ok(self.blocks)
    }
}
