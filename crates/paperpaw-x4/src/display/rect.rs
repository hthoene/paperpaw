//! The SSD1677 changing a rectangle of the screen while its grays stay
//! (#218, see `shells/xteink/src/firmware/gray_clock.rs`).

use embedded_hal::delay::DelayNs;
use embedded_hal::digital::{InputPin, OutputPin};
use embedded_hal::spi::SpiDevice;
use paperpaw_platform::{FrameView, Rotation};

use super::{Display, Error, PANEL_HEIGHT, PANEL_WIDTH, Planes, Refresh, bands, cmd};

/// Where screen pixel `(x, y)` lies in the controller's memory, turned as
/// `rotation` says: its column and its row (see [`bands`]).
fn controller_of(rotation: Rotation, x: u16, y: u16) -> (u16, u16) {
    match rotation {
        Rotation::Deg90 => (PANEL_WIDTH - 1 - x, y),
        Rotation::Deg180 => (PANEL_WIDTH - 1 - y, PANEL_HEIGHT - 1 - x),
        Rotation::Deg270 => (x, PANEL_HEIGHT - 1 - y),
        _ => (y, x),
    }
}

/// The screen pixel at the controller's column and row: the other way
/// round of [`controller_of`].
fn screen_of(rotation: Rotation, column: u16, row: u16) -> (u16, u16) {
    match rotation {
        Rotation::Deg90 => (PANEL_WIDTH - 1 - column, row),
        Rotation::Deg180 => (PANEL_HEIGHT - 1 - row, PANEL_WIDTH - 1 - column),
        Rotation::Deg270 => (column, PANEL_HEIGHT - 1 - row),
        _ => (row, column),
    }
}

impl<SPI, DC, RST, BUSY, D> Display<SPI, DC, RST, BUSY, D>
where
    SPI: SpiDevice,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
    D: DelayNs,
{
    /// Starts a fast refresh that leaves the grays on the panel where both
    /// memories agree (#218): the caller made them the frame shown in black
    /// and white after the gray pass (`write_rows` into both), then wrote
    /// what changed ([`Display::write_rect`]). Only the pixels whose two
    /// bits differ are driven. Where the panel does not show grays, as
    /// [`Display::start_refresh`]; the grays count as shown after it, so
    /// that the next refresh of another kind drives every pixel.
    pub fn start_refresh_keeping_grays(&mut self, power_off: bool) -> Result<(), Error> {
        self.keep_grays = true;
        self.start_refresh(Refresh::Fast, power_off)
    }

    /// Writes the part of a band that lies in the screen rectangle
    /// `rect` (`x0, y0, x1, y1`, the end excluded) into `planes`, nothing of
    /// the rest of the memory (#218): the controller's window is the
    /// rectangle's, widened to whole bytes of its columns, where the pixels
    /// outside the rectangle are the band's as they are. `invert` writes
    /// the pixels inside the rectangle the other way round, so that a
    /// refresh drives all of them. The band as [`Display::write_rows`]
    /// takes it.
    pub fn write_rect(
        &mut self,
        top: u16,
        rows: FrameView<'_>,
        rect: (u16, u16, u16, u16),
        planes: Planes,
        invert: bool,
    ) -> Result<(), Error> {
        let (width, screen_height) = self.size();
        if !top.is_multiple_of(8) || top >= screen_height || rows.width != width || rows.height == 0
        {
            return Err(Error::Rows);
        }
        let end = top.saturating_add(rows.height).min(screen_height);
        let (x0, y0, x1, y1) = rect;
        let (x1, y0, y1) = (x1.min(width), y0.max(top), y1.min(end));
        if x0 >= x1 || y0 >= y1 {
            return Ok(());
        }
        let rotation = self.rotation;
        let (ca, ra) = controller_of(rotation, x0, y0);
        let (cb, rb) = controller_of(rotation, x1 - 1, y1 - 1);
        let (first_row, last_row) = (ra.min(rb), ra.max(rb));
        let first_column = ca.min(cb) & !7;
        let last_column = (ca.max(cb) | 7).min(PANEL_WIDTH - 1);
        let columns = last_column - first_column + 1;
        let light = bands::light(&rows);
        let pixel = |column: u16, row: u16| {
            let (x, y) = screen_of(rotation, column, row);
            if y < top || y >= end || x >= width {
                // Below the band's end: white.
                return true;
            }
            let light = light(x, y - top);
            if invert && x >= x0 && x < x1 && y >= y0 && y < y1 {
                !light
            } else {
                light
            }
        };
        for (on, write) in [
            (planes.new, cmd::WRITE_NEW),
            (planes.shown, cmd::WRITE_SHOWN),
        ] {
            if !on {
                continue;
            }
            self.window(first_column, columns, first_row, last_row - first_row + 1)?;
            self.command(write, &[])?;
            let Self {
                spi, dc, buffer, ..
            } = self;
            buffer.clear();
            for row in first_row..=last_row {
                for byte in 0..columns / 8 {
                    let mut bits = 0u8;
                    for bit in 0..8u16 {
                        if pixel(first_column + byte * 8 + bit, row) {
                            bits |= 0x80 >> bit;
                        }
                    }
                    buffer.push(bits);
                }
                if buffer.len() >= bands::CHUNK {
                    dc.set_high().map_err(|_| Error::Pin)?;
                    spi.write(buffer).map_err(|_| Error::Spi)?;
                    buffer.clear();
                }
            }
            if !buffer.is_empty() {
                dc.set_high().map_err(|_| Error::Pin)?;
                spi.write(buffer).map_err(|_| Error::Spi)?;
                buffer.clear();
            }
        }
        Ok(())
    }
}
