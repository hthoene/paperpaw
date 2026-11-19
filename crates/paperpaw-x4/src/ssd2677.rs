//! The Boox Picco's SE0400NQW47 panel (800 × 480, black and white), whose
//! controller its stock firmware calls "SSD2677": UltraChip-style commands
//! (not the SSD1677's), its busy line low while it works, and no waveforms
//! of its own: the firmware uploads one before every refresh
//! ([`ssd2677_luts`](crate::ssd2677_luts)).
//!
//! A frame goes out whole as two bits a pixel in one run of command 0x10:
//! the high bit what the glass shows, the low bit the new frame (1 for
//! white); the waveform drives each pixel by that pair. So the driver
//! keeps both frames (48 KB each, in the Picco's PSRAM) and the packed run
//! (96 KB), laid out as the X4 Pro's kept frame is
//! ([`uc8179`](crate::uc8179)): 480 lines of 100 bytes, the upright
//! screen's left column the last line.
//!
//! The start-up list, the commands, the packing and the order of a refresh
//! are the FreeInk SDK's `Ssd2677Driver` (MIT licence, Copyright (c) 2026
//! FreeInk), read there from the stock firmware (build 2912). Not tried on
//! a device by PaperPaw. Grays are not shown: the picture goes out in
//! black and white.

use alloc::vec;
use alloc::vec::Vec;

use embedded_hal::delay::DelayNs;
use embedded_hal::digital::{InputPin, OutputPin};
use embedded_hal::spi::SpiDevice;
use paperpaw_platform::{FrameView, PixelFormat, Rotation};

use crate::bands;
use crate::display::{BUSY_TIMEOUT_MS, Error, PANEL_HEIGHT, Planes, Refresh};
use crate::ssd2677_luts as luts;
use crate::uc8179::{HEIGHT, LINE, WIDTH, panel_place, put, upright};

/// The controller's commands used here.
mod cmd {
    pub const POWER_OFF: u8 = 0x02;
    pub const POWER_ON: u8 = 0x04;
    pub const DEEP_SLEEP: u8 = 0x07;
    pub const DATA: u8 = 0x10;
    pub const REFRESH: u8 = 0x12;
    pub const LUT: u8 = 0x20;
}

/// What the stock firmware sends at start, in its order: the panel
/// setting (10 ms after it), the window, power, booster, the data
/// interval, the voltages, frame rate and VCOM.
const START: [(u8, &[u8]); 12] = [
    (0x00, &[0x27, 0x09]),
    (
        0x83,
        &[0x00, 0x00, 0x03, 0x1F, 0x00, 0xC8, 0x02, 0xA8, 0x01],
    ),
    (0x01, &[0x07, 0xF0, 0x22, 0x20, 0x78, 0x22]),
    (0x06, &[0x0F, 0x8B, 0x93, 0xC1]),
    (0xE7, &[0xC1]),
    (0x50, &[0x77]),
    (0x62, &[0x98, 0x98, 0x98, 0x75, 0xCA, 0xB2, 0x98, 0x7E]),
    (0xE0, &[0x10]),
    (0x65, &[0x00, 0x00, 0x00, 0x00]),
    (0x30, &[0x04]),
    (0x82, &[0x28]),
    (0xE9, &[0x01]),
];

/// A frame's bytes.
const FRAME: usize = LINE * PANEL_HEIGHT as usize;

/// Four bits spread into the even bits of a byte.
const SPREAD: [u8; 16] = [
    0x00, 0x01, 0x04, 0x05, 0x10, 0x11, 0x14, 0x15, 0x40, 0x41, 0x44, 0x45, 0x50, 0x51, 0x54, 0x55,
];

/// Eight pixels of the frame shown and of the new one as the controller
/// takes them: two bits each, the shown one's the higher.
fn pack(shown: u8, new: u8) -> [u8; 2] {
    let pair = |shown: u8, new: u8| (SPREAD[usize::from(shown)] << 1) | SPREAD[usize::from(new)];
    [pair(shown >> 4, new >> 4), pair(shown & 15, new & 15)]
}

/// A refresh started and not yet waited for.
#[derive(Debug, Clone, Copy)]
struct Pending {
    power_off: bool,
}

/// The display, on its SPI device (which selects the chip) and pins.
#[derive(Debug)]
pub struct Display<SPI, DC, RST, BUSY, D> {
    spi: SPI,
    dc: DC,
    reset: RST,
    busy: BUSY,
    delay: D,
    /// The frame to show, and what the glass shows.
    frame: Vec<u8>,
    shown: Vec<u8>,
    /// Both, packed for the controller.
    packed: Vec<u8>,
    /// Nothing shown since [`Display::init`]: the glass holds whatever it
    /// held, so it is driven white first and the frame shown in full.
    fresh: bool,
    powered: bool,
    rotation: Rotation,
    pending: Option<Pending>,
    /// LAB ONLY (#40): which waveform a fast refresh takes.
    table: Table,
}

/// LAB ONLY (#40): which of the stock firmware's waveforms a fast refresh
/// (a page turn) is shown with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Table {
    /// The stock firmware's choice: the page waveform, or the one for small
    /// changes where an eighth of the frame or less changed.
    #[default]
    Auto,
    /// Always the waveform for small changes, the power kept on (faster).
    Small,
    /// Always the full waveform (slowest, cleanest).
    Full,
}

impl<SPI, DC, RST, BUSY, D> Display<SPI, DC, RST, BUSY, D>
where
    SPI: SpiDevice,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
    D: DelayNs,
{
    pub fn new(spi: SPI, dc: DC, reset: RST, busy: BUSY, delay: D) -> Self {
        Self {
            spi,
            dc,
            reset,
            busy,
            delay,
            frame: vec![0xFF; FRAME],
            shown: vec![0xFF; FRAME],
            packed: vec![0xFF; 2 * FRAME],
            fresh: true,
            powered: false,
            rotation: Rotation::Deg0,
            pending: None,
            table: Table::Auto,
        }
    }

    /// LAB ONLY (#40): the waveform fast refreshes take.
    pub fn set_table(&mut self, table: Table) {
        self.table = table;
    }

    /// How the screen PaperPaw draws is turned, as the X4 Pro's kept frame
    /// turns ([`uc8179::Display::set_rotation`](crate::uc8179::Display::set_rotation)).
    pub fn set_rotation(&mut self, rotation: Rotation) {
        self.rotation = rotation;
    }

    pub fn rotation(&self) -> Rotation {
        self.rotation
    }

    /// The width and height of the screen PaperPaw draws, turned.
    pub fn size(&self) -> (u16, u16) {
        match self.rotation {
            Rotation::Deg0 | Rotation::Deg180 => (WIDTH, HEIGHT),
            _ => (HEIGHT, WIDTH),
        }
    }

    /// Resets and sets up the controller; the next refresh cleans the
    /// glass and drives every pixel.
    pub fn init(&mut self) -> Result<(), Error> {
        for (high, ms) in [(true, 10), (false, 10), (true, 20)] {
            if high {
                self.reset.set_high()
            } else {
                self.reset.set_low()
            }
            .map_err(|_| Error::Pin)?;
            self.delay.delay_ms(ms);
        }
        self.wait_ready(&mut || {})?;
        for (command, data) in START {
            self.command(command, data)?;
            if command == 0x00 {
                self.delay.delay_ms(10);
            }
        }
        self.wait_ready(&mut || {})?;
        self.fresh = true;
        self.powered = false;
        self.pending = None;
        Ok(())
    }

    /// Writes screen rows `top..top + rows.height` (a band from
    /// `Kernel::render`, the screen's width as turned) into the frame; it
    /// goes to the controller whole at the refresh. `planes` only say
    /// whether the band is new: what goes only into *shown* after a fast
    /// refresh is kept here already.
    pub fn write_rows(
        &mut self,
        top: u16,
        rows: FrameView<'_>,
        planes: Planes,
    ) -> Result<(), Error> {
        if !planes.new {
            return Ok(());
        }
        let (width, screen_height) = self.size();
        if top >= screen_height || rows.width != width || rows.height == 0 {
            return Err(Error::Rows);
        }
        let height = rows.height.min(screen_height - top);
        let light = bands::light(&rows);
        for y in 0..height {
            for x in 0..width {
                let (sx, sy) = upright(self.rotation, x, top + y);
                let (column, line) = panel_place(sx, sy);
                let at = usize::from(line) * LINE + usize::from(column / 8);
                put(&mut self.frame, (at, 0x80 >> (column % 8)), light(x, y));
            }
        }
        Ok(())
    }

    /// Rows in four grays, shown in black and white: this driver has no
    /// waveform for grays.
    pub fn write_gray_rows(&mut self, top: u16, rows: FrameView<'_>) -> Result<(), Error> {
        if rows.format != PixelFormat::Gray2 {
            return Err(Error::Rows);
        }
        self.write_rows(top, rows, Planes::BOTH)
    }

    /// Shows what [`Display::write_gray_rows`] wrote: every pixel driven,
    /// in black and white.
    pub fn refresh_gray(&mut self) -> Result<(), Error> {
        self.refresh(Refresh::Half, false)
    }

    /// Shows the frame; `power_off` turns the panel's power off after it.
    pub fn refresh(&mut self, refresh: Refresh, power_off: bool) -> Result<(), Error> {
        self.start_refresh(refresh, power_off)?;
        self.wait()
    }

    /// Starts showing the frame as [`Display::refresh`] does, without
    /// waiting for the panel: [`Display::wait`] before anything else.
    ///
    /// As the stock firmware shows it: the first frame after the start
    /// over a glass driven white, in full. A fast refresh that changes
    /// more than an eighth of the frame's bytes with its page waveform,
    /// the power off after it; a smaller one with the waveform for small
    /// changes, the power kept on.
    pub fn start_refresh(&mut self, refresh: Refresh, power_off: bool) -> Result<(), Error> {
        self.wait()?;
        let mut refresh = refresh;
        if self.fresh {
            // Every pixel taken as black, so each gets the drive to white.
            self.shown.fill(0x00);
            self.frame_out(true)?;
            self.show(&luts::FULL)?;
            self.wait_ready(&mut || {})?;
            self.power_off()?;
            self.shown.fill(0xFF);
            self.fresh = false;
            refresh = Refresh::Full;
        }
        let (lut, keep_power) = match refresh {
            Refresh::Full | Refresh::Half => (&luts::FULL, false),
            Refresh::Fast if self.table == Table::Small => (&luts::SMALL, true),
            Refresh::Fast if self.table == Table::Full => (&luts::FULL, false),
            Refresh::Fast => {
                let changed = self
                    .frame
                    .iter()
                    .zip(&self.shown)
                    .filter(|(new, shown)| new != shown)
                    .count();
                if changed > FRAME / 8 {
                    (&luts::FAST, false)
                } else {
                    (&luts::SMALL, true)
                }
            }
        };
        self.frame_out(false)?;
        self.show(lut)?;
        self.pending = Some(Pending {
            power_off: power_off || !keep_power,
        });
        Ok(())
    }

    /// Sends both frames packed (the new one all white if `white`).
    fn frame_out(&mut self, white: bool) -> Result<(), Error> {
        for (i, pair) in self.packed.as_chunks_mut::<2>().0.iter_mut().enumerate() {
            let new = if white { 0xFF } else { self.frame[i] };
            *pair = pack(self.shown[i], new);
        }
        self.dc.set_low().map_err(|_| Error::Pin)?;
        self.spi.write(&[cmd::DATA]).map_err(|_| Error::Spi)?;
        self.dc.set_high().map_err(|_| Error::Pin)?;
        self.spi.write(&self.packed).map_err(|_| Error::Spi)
    }

    /// Loads `lut`, turns the power on if it is off and starts the
    /// refresh.
    fn show(&mut self, lut: &[u8; luts::LEN]) -> Result<(), Error> {
        self.command(cmd::LUT, lut)?;
        if !self.powered {
            self.command(cmd::POWER_ON, &[])?;
            self.wait_ready(&mut || {})?;
            self.powered = true;
        }
        self.command(cmd::REFRESH, &[0x00])
    }

    fn power_off(&mut self) -> Result<(), Error> {
        if self.powered {
            self.command(cmd::POWER_OFF, &[0x00])?;
            self.wait_ready(&mut || {})?;
            self.powered = false;
        }
        Ok(())
    }

    /// Turns the panel's power off once nothing follows for a while: the
    /// picture stays, and the next refresh powers it up again.
    pub fn power_down(&mut self) -> Result<(), Error> {
        self.wait()?;
        self.power_off()
    }

    /// Whether the panel's power is on.
    pub fn powered(&self) -> bool {
        self.powered
    }

    /// Puts the controller into deep sleep, the picture staying;
    /// [`Display::init`] wakes it again.
    pub fn sleep(&mut self) -> Result<(), Error> {
        self.wait()?;
        self.power_off()?;
        self.fresh = true;
        self.command(cmd::DEEP_SLEEP, &[0xA5])
    }

    /// Whether a refresh started by [`Display::start_refresh`] still goes
    /// on, without waiting (its busy pin low).
    pub fn refreshing(&mut self) -> Result<bool, Error> {
        if self.pending.is_none() {
            return Ok(false);
        }
        self.busy.is_low().map_err(|_| Error::Pin)
    }

    /// Waits for a refresh started by [`Display::start_refresh`] to end,
    /// then keeps the frame as the one shown.
    pub fn wait(&mut self) -> Result<(), Error> {
        self.wait_with(|| {})
    }

    /// As [`Self::wait`], calling `meanwhile` each round of the wait (a
    /// millisecond and what it takes), e.g. to serve the radio.
    pub fn wait_with(&mut self, mut meanwhile: impl FnMut()) -> Result<(), Error> {
        let Some(pending) = self.pending.take() else {
            return Ok(());
        };
        if let Err(e) = self.wait_ready(&mut meanwhile) {
            // What the glass shows is not known: cleaned at the next one.
            self.fresh = true;
            return Err(e);
        }
        self.shown.copy_from_slice(&self.frame);
        if pending.power_off {
            self.power_off()?;
        }
        Ok(())
    }

    /// Waits while the controller is busy (its busy pin low).
    fn wait_ready(&mut self, meanwhile: &mut dyn FnMut()) -> Result<(), Error> {
        for _ in 0..BUSY_TIMEOUT_MS {
            if self.busy.is_high().map_err(|_| Error::Pin)? {
                return Ok(());
            }
            meanwhile();
            self.delay.delay_ms(1);
        }
        Err(Error::Timeout)
    }

    fn command(&mut self, command: u8, data: &[u8]) -> Result<(), Error> {
        self.dc.set_low().map_err(|_| Error::Pin)?;
        self.spi.write(&[command]).map_err(|_| Error::Spi)?;
        if !data.is_empty() {
            self.dc.set_high().map_err(|_| Error::Pin)?;
            self.spi.write(data).map_err(|_| Error::Spi)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// As the stock firmware packs them (the FreeInk SDK's `packPixels`):
    /// the frame shown in the higher bit of each pair, the first pixel first.
    #[test]
    fn two_frames_pack_into_pairs() {
        assert_eq!(pack(0xFF, 0xFF), [0xFF, 0xFF]);
        assert_eq!(pack(0x00, 0x00), [0x00, 0x00]);
        // Shown white, new black: 10 for each pixel.
        assert_eq!(pack(0xFF, 0x00), [0xAA, 0xAA]);
        assert_eq!(pack(0x00, 0xFF), [0x55, 0x55]);
        // Only the first pixel white in the new frame.
        assert_eq!(pack(0x00, 0x80), [0x40, 0x00]);
        assert_eq!(pack(0x01, 0x00), [0x00, 0x02]);
    }

    #[test]
    fn the_waveforms_are_whole_tables() {
        for lut in [&luts::FULL, &luts::FAST, &luts::SMALL] {
            assert_eq!(lut[..5], [0x00, 0x78, 0x78, 0x78, 0x78]);
            assert_eq!(lut[6], 0x08);
        }
    }
}
