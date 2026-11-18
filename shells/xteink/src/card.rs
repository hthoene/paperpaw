//! The microSD card's blocks (read and written by [`paperpaw_x4::card`])
//! for PaperPaw's FAT storage.

use core::cell::RefCell;

use esp_hal::delay::Delay;
use esp_hal::gpio::Output;
use esp_hal::spi::master::Config as SpiConfig;
use esp_hal::time::Rate;
use log::{info, warn};
use paperpaw_core::CardTrouble;
use paperpaw_fat::{BLOCK, Blocks, DiskError, Restart};

use crate::board::{self, Bus, CardParts};
use crate::volume::Mounted;

/// The card's FAT file system as PaperPaw's storage, then the bus at full
/// speed, the card watched for being taken out and put back (see
/// `volume::mount_swappable`); without a card or one PaperPaw cannot read,
/// why.
// A frame of its own, off the stack before the kernel is built.
#[inline(never)]
pub fn mount(parts: CardParts) -> Result<Mounted, CardTrouble> {
    let CardParts {
        bus,
        spi,
        select,
        delay,
    } = parts;
    // A card wants clocks with nothing selected before it starts.
    let _ = bus.borrow_mut().write(&[0xFF; 10]);
    let sd = match paperpaw_x4::card::Card::new(bus, select, delay) {
        Ok(sd) => sd,
        Err(e) => {
            warn!("SD card: {e:?}");
            return Err(CardTrouble::Missing);
        }
    };
    full_speed(bus, spi);
    info!("SD card: {} MB", sd.blocks() >> 11);
    crate::volume::mount_swappable(Card { sd, bus, spi })
}

/// The bus at the speed it runs at once a card is started (the panel's
/// too: they share it).
fn full_speed(bus: &RefCell<Bus>, spi: SpiConfig) {
    let _ = bus
        .borrow_mut()
        .apply_config(&spi.with_frequency(Rate::from_mhz(board::SPI_MHZ)));
}

/// The card, as blocks; its bus and its settings at the starting speed,
/// to start it again.
pub struct Card {
    sd: paperpaw_x4::card::Card<'static, Bus, Output<'static>, Delay>,
    bus: &'static RefCell<Bus>,
    spi: SpiConfig,
}

impl Restart for Card {
    /// Started as at the start: the bus slow while it starts, as cards
    /// want, then at full speed again, whether it started or not.
    fn restart(&mut self) -> Result<(), DiskError> {
        let _ = self.bus.borrow_mut().apply_config(&self.spi);
        let _ = self.bus.borrow_mut().write(&[0xFF; 10]);
        let started = self.sd.restart();
        full_speed(self.bus, self.spi);
        match started {
            Ok(()) => {
                info!("SD card: started again, {} MB", self.sd.blocks() >> 11);
                Ok(())
            }
            Err(e) => {
                warn!("SD card: not started again: {e:?}");
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
        let blocks = data.as_chunks_mut::<BLOCK>().0;
        #[cfg(feature = "remote")]
        crate::traced::blocks(index, blocks.len() as u32, 0);
        self.sd.read(index, blocks).map_err(|_| DiskError)
    }

    fn write_blocks(&mut self, index: u32, data: &[u8]) -> Result<(), DiskError> {
        crate::power::working();
        let blocks = data.as_chunks::<BLOCK>().0;
        #[cfg(feature = "remote")]
        crate::traced::blocks(index, 0, blocks.len() as u32);
        self.sd.write(index, blocks).map_err(|_| DiskError)
    }

    fn blocks(&mut self) -> Result<u32, DiskError> {
        Ok(self.sd.blocks())
    }
}
