//! The X4's display: a GDEQ0426T82 panel (800 × 480, black and white)
//! driven by an SSD1677 controller over SPI.
//!
//! The controller has two memories of the whole panel: the frame to show
//! (*new*) and the frame shown (*shown*), which a fast refresh compares
//! with, to move only the pixels that change. PaperPaw draws the screen
//! upright (480 × 800) in bands of rows, aligned to 8 (see
//! `KernelBuilder::row_align`); [`Display::write_rows`] writes each band
//! turned into one or both memories:
//!
//! - **Full** and **half** refreshes: the band goes into both, then
//!   [`Display::refresh`].
//! - **Fast** refreshes: the band goes into *new*, then
//!   [`Display::refresh`], then the same bands again into *shown*
//!   (`Kernel::rerender`), for the next one to compare with.
//!
//! So the firmware keeps no frame of its own: 48 KB of the 192 KB it has
//! stay free.
//!
//! The upright screen is the panel turned clockwise: the screen's row `y`
//! is the controller's column `y`, its column `x` the controller's row
//! `x` (its gates are wired bottom to top). A byte of the controller's
//! memory holds 8 pixels of a row (the most significant bit first; set
//! for white), so 8 rows of the screen.
//!
//! In four grays ([`Display::write_gray_rows`], [`Display::refresh_gray`]),
//! the two memories hold the two bits of each pixel's gray, shown with the
//! stock firmware's gray waveform; the next refresh then drives every
//! pixel again.
//!
//! Turned ([`Display::set_rotation`]), the screen is 800 × 480 on its side
//! or 480 × 800 upside down; its rows go into the controller's memory as
//! they lie on the panel (on its side, a row of the screen is a row of the
//! controller's).

use alloc::vec::Vec;

use embedded_hal::delay::DelayNs;
use embedded_hal::digital::{InputPin, OutputPin};
use embedded_hal::spi::SpiDevice;
use paperpaw_platform::{FrameView, PixelFormat, Rotation};

use crate::bands::{self, Window};
use crate::waveform;

/// The panel, as the controller addresses it.
pub const PANEL_WIDTH: u16 = 800;
pub const PANEL_HEIGHT: u16 = 480;
/// The screen PaperPaw draws upright (see [`Display::size`] for it
/// turned).
pub const WIDTH: u16 = PANEL_HEIGHT;
pub const HEIGHT: u16 = PANEL_WIDTH;

/// Longest wait for the controller, e.g. a full refresh (about 2 s, a
/// UC8179's 3.5 s): a busy line that sticks ends in an error, well before
/// the firmware's watchdog (20 s) would start the X4 again.
pub(crate) const BUSY_TIMEOUT_MS: u32 = 10_000;
/// The stock firmware's waveform for four grays (V3.1.9, as the FreeInk SDK
/// records it from the OpenX4 community SDK, MIT licence): 105 bytes of
/// phases, timing and frame rate, then the gate voltage, the three source
/// voltages and VCOM.
pub(crate) const GRAY_LUT: [u8; 110] = [
    0x00, 0x4A, 0x88, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, //
    0x80, 0x62, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, //
    0x88, 0x60, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, //
    0xA8, 0x44, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, //
    0x08, 0x0B, 0x02, 0x03, 0x00, //
    0x0C, 0x02, 0x07, 0x02, 0x00, //
    0x01, 0x00, 0x02, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x00, 0x01, //
    0x22, 0x22, 0x22, 0x22, 0x22, //
    0x17, 0x41, 0xA8, 0x32, 0x30,
];

/// How dark the two grays between black and white come out with
/// the stock gray waveform: its every level is first driven white, then black for 12
/// frames (black for 21), the grays then lightened by 2 (dark) or 4 (light)
/// frames of white, which leaves them faint beside the X4 Pro's (which
/// shows them black first and lightens them by one or two of its frames).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GrayTone {
    /// The stock firmware's waveform as it is: lightened by 2 and 4 frames.
    Stock,
    /// Lightened by 1 frame (dark gray) and 3 (light gray): as dark as
    /// the X4 Pro's beside it, the levels apart (chosen on the devices;
    /// not lightened at all, they came out darker than the Pro's, and
    /// close).
    #[default]
    Middle,
}

impl GrayTone {
    /// The waveform's second and third phase groups for light gray
    /// (*shown* 0 and *new* 1) and for dark gray (the other way round):
    /// four phases of two bits each, `01` driving black, `10` white, of
    /// 12, 2, 7 and 2 frames in the second group, 1, 0, 2 and 0 in the
    /// third.
    fn grays(self) -> [u8; 4] {
        // Light: second, third; dark: second, third.
        match self {
            GrayTone::Stock => [0x62, 0x00, 0x60, 0x00],
            GrayTone::Middle => [0x60, 0x80, 0x40, 0x80],
        }
    }

    /// The waveform with them.
    fn lut(self) -> [u8; 110] {
        let mut lut = GRAY_LUT;
        let [light, light_after, dark, dark_after] = self.grays();
        lut[11] = light;
        lut[12] = light_after;
        lut[21] = dark;
        lut[22] = dark_after;
        lut
    }
}

/// A way to show fast refreshes other than the panel's own, for trying
/// waveforms on a device (the waveform lab, `shells/xteink/src/lab.rs`).
/// Set with [`Display::set_experiment`]; only fast refreshes take it, and
/// the first after [`Display::init`] or after grays, which drive every
/// pixel, do not. Everything else (full, half, grays) keeps the stock
/// waveforms, so a full refresh cleans up after an experiment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Experiment {
    /// The panel's own fast waveform, as it picks it for a panel at this
    /// temperature, in degrees (0 to 90: the warm end is what the stock
    /// half refresh asks for, [`Refresh::Half`]): the controller's own
    /// tables, which the vendor made, faster the warmer they are meant for.
    Temperature(i8),
    /// A waveform made here ([`waveform::Direct`]), at the stock voltages.
    Direct(waveform::Direct),
}

/// The coolest and the warmest temperature [`Experiment::Temperature`]
/// takes.
pub const TEMPERATURE_RANGE: (i8, i8) = (0, 90);

/// The controller's commands used here.
mod cmd {
    pub const DRIVER_OUTPUT: u8 = 0x01;
    pub const GATE_VOLTAGE: u8 = 0x03;
    pub const SOURCE_VOLTAGE: u8 = 0x04;
    pub const BOOSTER_SOFT_START: u8 = 0x0C;
    pub const DEEP_SLEEP: u8 = 0x10;
    pub const DATA_ENTRY: u8 = 0x11;
    pub const SOFT_RESET: u8 = 0x12;
    pub const TEMPERATURE_SENSOR: u8 = 0x18;
    pub const WRITE_TEMPERATURE: u8 = 0x1A;
    pub const ACTIVATE: u8 = 0x20;
    pub const UPDATE_CONTROL_1: u8 = 0x21;
    pub const UPDATE_CONTROL_2: u8 = 0x22;
    pub const WRITE_NEW: u8 = 0x24;
    pub const WRITE_SHOWN: u8 = 0x26;
    pub const WRITE_VCOM: u8 = 0x2C;
    pub const WRITE_LUT: u8 = 0x32;
    pub const BORDER: u8 = 0x3C;
    pub const RAM_X_RANGE: u8 = 0x44;
    pub const RAM_Y_RANGE: u8 = 0x45;
    pub const FILL_NEW: u8 = 0x46;
    pub const FILL_SHOWN: u8 = 0x47;
    pub const RAM_X: u8 = 0x4E;
    pub const RAM_Y: u8 = 0x4F;
}

/// How the panel changes to the new frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refresh {
    /// Flashing, clears all ghosting (about 2 s).
    Full,
    /// Flashing less, warmer waveform (about 1.7 s).
    Half,
    /// Only the pixels that change (a few hundred ms); leaves some
    /// ghosting.
    Fast,
}

/// Which of the controller's memories a band goes into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Planes {
    pub new: bool,
    pub shown: bool,
}

impl Planes {
    /// For a full or half refresh: both.
    pub const BOTH: Self = Self {
        new: true,
        shown: true,
    };
    /// For a fast refresh, before it.
    pub const NEW: Self = Self {
        new: true,
        shown: false,
    };
    /// After a fast refresh, the frame now shown.
    pub const SHOWN: Self = Self {
        new: false,
        shown: true,
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    Spi,
    Pin,
    /// The controller stayed busy too long.
    Timeout,
    /// Rows not aligned to 8, or not the (turned) screen's width.
    Rows,
}

/// The display, on its SPI device (which selects the chip) and pins.
#[derive(Debug)]
pub struct Display<SPI, DC, RST, BUSY, D> {
    spi: SPI,
    dc: DC,
    reset: RST,
    busy: BUSY,
    delay: D,
    /// The panel's power is on (after a refresh not followed by sleep).
    powered: bool,
    /// Nothing shown since [`Display::init`]: the controller does not know
    /// what the panel shows, so a fast refresh would compare with nothing.
    fresh: bool,
    /// How the screen PaperPaw draws is turned.
    rotation: Rotation,
    /// The panel shows grays ([`Display::refresh_gray`]): the next refresh
    /// drives every pixel.
    gray: bool,
    /// How dark the grays between come out.
    tone: GrayTone,
    /// How fast refreshes are shown, in place of the panel's own, while
    /// an experiment is on ([`Display::set_experiment`]).
    experiment: Option<Experiment>,
    buffer: Vec<u8>,
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
            powered: false,
            fresh: true,
            rotation: Rotation::Deg0,
            gray: false,
            tone: GrayTone::default(),
            experiment: None,
            buffer: Vec::new(),
        }
    }

    /// How the screen PaperPaw draws is turned, in quarter turns from
    /// upright as the setting says: [`Rotation::Deg90`] is the X4 turned
    /// clockwise (its buttons on the left), [`Rotation::Deg270`] the
    /// other way. The panel keeps what it shows until bands of the turned
    /// screen are written.
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

    /// Resets and sets up the controller, both memories white.
    pub fn init(&mut self) -> Result<(), Error> {
        for (high, ms) in [(true, 20), (false, 2), (true, 20)] {
            if high {
                self.reset.set_high()
            } else {
                self.reset.set_low()
            }
            .map_err(|_| Error::Pin)?;
            self.delay.delay_ms(ms);
        }
        self.command(cmd::SOFT_RESET, &[])?;
        self.wait()?;
        // The internal temperature sensor.
        self.command(cmd::TEMPERATURE_SENSOR, &[0x80])?;
        // The booster's soft start, as the panel wants it.
        // The stock firmware's sequences throughout (as the FreeInk SDK
        // records them for the X4): booster, border, and the updates below.
        self.command(cmd::BOOSTER_SOFT_START, &[0xAE, 0xC7, 0xC3, 0xC0, 0x80])?;
        // 480 gates, scanned interlaced.
        let gates = PANEL_HEIGHT - 1;
        self.command(
            cmd::DRIVER_OUTPUT,
            &[(gates % 256) as u8, (gates / 256) as u8, 0x02],
        )?;
        self.command(cmd::BORDER, &[0x80])?;
        self.window(0, PANEL_WIDTH, 0, PANEL_HEIGHT)?;
        for fill in [cmd::FILL_NEW, cmd::FILL_SHOWN] {
            self.command(fill, &[0xF7])?;
            self.wait()?;
        }
        self.fresh = true;
        Ok(())
    }

    /// The controller writes into columns `x..x + width` of rows
    /// `y..y + height`, row by row from the top.
    fn window(&mut self, x: u16, width: u16, y: u16, height: u16) -> Result<(), Error> {
        let (last_x, last_y) = (x + width - 1, y + height - 1);
        // X and then Y increase.
        self.command(cmd::DATA_ENTRY, &[0x03])?;
        self.command(
            cmd::RAM_X_RANGE,
            &[
                (x % 256) as u8,
                (x / 256) as u8,
                (last_x % 256) as u8,
                (last_x / 256) as u8,
            ],
        )?;
        self.command(
            cmd::RAM_Y_RANGE,
            &[
                (y % 256) as u8,
                (y / 256) as u8,
                (last_y % 256) as u8,
                (last_y / 256) as u8,
            ],
        )?;
        self.command(cmd::RAM_X, &[(x % 256) as u8, (x / 256) as u8])?;
        self.command(cmd::RAM_Y, &[(y % 256) as u8, (y / 256) as u8])
    }

    /// Writes screen rows `top..top + rows.height` (a band from
    /// `Kernel::render`, the screen's width as turned, starting at a
    /// multiple of 8) into `planes`. Pixels lighter than middle gray are
    /// white.
    pub fn write_rows(
        &mut self,
        top: u16,
        rows: FrameView<'_>,
        planes: Planes,
    ) -> Result<(), Error> {
        let (width, screen_height) = self.size();
        if !top.is_multiple_of(8) || top >= screen_height || rows.width != width || rows.height == 0
        {
            return Err(Error::Rows);
        }
        let height = rows.height.min(screen_height - top);
        // Light pixels show white, dark ones black.
        let light = bands::light(&rows);
        for (plane, write) in [
            (planes.new, cmd::WRITE_NEW),
            (planes.shown, cmd::WRITE_SHOWN),
        ] {
            if plane {
                // Rows past the band's end are white.
                self.write_plane(top, &rows, height, write, &light, true)?;
            }
        }
        Ok(())
    }

    /// Writes screen rows `top..top + rows.height` (as [`Display::write_rows`]
    /// takes them) in four grays, 2 bits a pixel ([`PixelFormat::Gray2`],
    /// 0 black, 3 white), into both memories: the low bit into *new*, the
    /// high one into *shown*, as the gray waveform reads them. Shown with
    /// [`Display::refresh_gray`].
    pub fn write_gray_rows(&mut self, top: u16, rows: FrameView<'_>) -> Result<(), Error> {
        let (width, screen_height) = self.size();
        if !top.is_multiple_of(8)
            || top >= screen_height
            || rows.width != width
            || rows.height == 0
            || rows.format != PixelFormat::Gray2
        {
            return Err(Error::Rows);
        }
        let height = rows.height.min(screen_height - top);
        // The waveform's levels run the other way: 3 is black.
        let level = |x: u16, y: u16| {
            let at = usize::from(y) * rows.stride + usize::from(x / 4);
            let byte = rows.data.get(at).copied().unwrap_or(0xFF);
            3 - ((byte >> (6 - 2 * (x % 4))) & 0b11)
        };
        let low = |x: u16, y: u16| level(x, y) & 1 != 0;
        let high = |x: u16, y: u16| level(x, y) & 2 != 0;
        // Rows past the band's end are white (0).
        self.write_plane(top, &rows, height, cmd::WRITE_NEW, &low, false)?;
        self.write_plane(top, &rows, height, cmd::WRITE_SHOWN, &high, false)
    }

    /// How dark the grays between come out from now on.
    pub fn set_gray_tone(&mut self, tone: GrayTone) {
        self.tone = tone;
    }

    /// Shows fast refreshes as `experiment` says from now on, or as the
    /// panel does with `None` (see [`Experiment`]).
    pub fn set_experiment(&mut self, experiment: Option<Experiment>) {
        self.experiment = experiment;
    }

    /// The experiment on, if any.
    pub fn experiment(&self) -> Option<Experiment> {
        self.experiment
    }

    /// Shows the grays written by [`Display::write_gray_rows`], with the stock
    /// firmware's gray waveform (about as long as a full refresh). The next
    /// refresh drives every pixel (a half refresh), as after
    /// [`Display::init`].
    pub fn refresh_gray(&mut self) -> Result<(), Error> {
        let lut = self.tone.lut();
        self.command(cmd::WRITE_LUT, &lut[..105])?;
        self.command(cmd::GATE_VOLTAGE, &lut[105..106])?;
        self.command(cmd::SOURCE_VOLTAGE, &lut[106..109])?;
        self.command(cmd::WRITE_VCOM, &lut[109..110])?;
        self.command(cmd::BORDER, &[0xC0])?;
        // Both memories count; the loaded waveform, not the panel's own.
        self.command(cmd::UPDATE_CONTROL_1, &[0x00])?;
        self.command(cmd::UPDATE_CONTROL_2, &[0xCC])?;
        self.command(cmd::ACTIVATE, &[])?;
        self.powered = true;
        self.fresh = false;
        self.gray = true;
        self.wait()
    }

    /// Writes one memory: the window for screen rows `top..top + height` as
    /// the screen is turned, then each pixel's bit (`bit`), `pad` for rows
    /// past the band's end (see [`bands::send`]).
    fn write_plane(
        &mut self,
        top: u16,
        rows: &FrameView<'_>,
        height: u16,
        write: u8,
        bit: &dyn Fn(u16, u16) -> bool,
        pad: bool,
    ) -> Result<(), Error> {
        let window = Window::of(self.rotation, top, height);
        self.window(window.x, window.width, window.y, window.height)?;
        self.command(write, &[])?;
        let Self {
            spi, dc, buffer, ..
        } = self;
        bands::send(&window, rows, height, bit, pad, buffer, &mut |bytes| {
            dc.set_high().map_err(|_| Error::Pin)?;
            spi.write(bytes).map_err(|_| Error::Spi)
        })
    }

    /// Shows the new frame; `power_off` turns the panel's power off after
    /// it, e.g. before sleeping. The first after [`Display::init`] drives
    /// every pixel in any case (a half refresh).
    ///
    /// The stock firmware's whole sequences, each powering up, loading the
    /// temperature and the waveform: full `F7`, half `D7` (a warm panel's
    /// waveform), fast `FC` (the panel's partial waveform). A weaker fast
    /// sequence (`1C`, without the temperature) left text on some pages
    /// behind: a page turned showed the page before under its number.
    pub fn refresh(&mut self, refresh: Refresh, power_off: bool) -> Result<(), Error> {
        self.start_refresh(refresh, power_off)?;
        self.wait()
    }

    /// Starts showing the new frame as [`Display::refresh`] does, without
    /// waiting for it: the processor works on meanwhile (the bus is free
    /// for the card). [`Display::wait`] before anything else is sent.
    pub fn start_refresh(&mut self, refresh: Refresh, power_off: bool) -> Result<(), Error> {
        // After grays, every pixel is driven back to black or white.
        let refresh = if self.fresh || (self.gray && refresh == Refresh::Fast) {
            Refresh::Half
        } else {
            refresh
        };
        self.fresh = false;
        self.gray = false;
        // A fast refresh compares with the frame shown; the others
        // drive every pixel.
        let compare = if refresh == Refresh::Fast { 0x00 } else { 0x40 };
        self.command(cmd::UPDATE_CONTROL_1, &[compare])?;
        self.command(cmd::BORDER, &[0xC0])?;
        let mut sequence = match refresh {
            Refresh::Full => 0xF7,
            Refresh::Half => {
                self.command(cmd::WRITE_TEMPERATURE, &[0x5A])?;
                0xD7
            }
            Refresh::Fast => match self.experiment {
                None => 0xFC,
                Some(experiment) => self.start_experiment(experiment)?,
            },
        };
        if power_off {
            sequence |= 0x03;
        }
        self.command(cmd::UPDATE_CONTROL_2, &[sequence])?;
        self.command(cmd::ACTIVATE, &[])?;
        // The full and half sequences power down at their end (0x03).
        self.powered = sequence & 0x03 == 0;
        Ok(())
    }

    /// What an experimental fast refresh sends before it is started, and
    /// the sequence that starts it (as `0xFC` for the panel's own).
    ///
    /// The panel's own waveform at a temperature: that temperature is
    /// written and not read from the sensor (the sequence without the bit
    /// that loads it, `0xDC`), so the controller picks the table of that
    /// range. A waveform of ours: loaded as the gray one is (`0xCC`, the
    /// loaded waveform and not the panel's own), at the voltages of the
    /// stock gray waveform, the ones this panel is driven with.
    fn start_experiment(&mut self, experiment: Experiment) -> Result<u8, Error> {
        match experiment {
            Experiment::Temperature(celsius) => {
                let (coolest, warmest) = TEMPERATURE_RANGE;
                let celsius = celsius.clamp(coolest, warmest);
                self.command(cmd::WRITE_TEMPERATURE, &[celsius as u8])?;
                Ok(0xDC)
            }
            Experiment::Direct(direct) => {
                let lut = direct.lut();
                let voltages = &GRAY_LUT[waveform::LUT_LEN..];
                self.command(cmd::WRITE_LUT, &lut)?;
                self.command(cmd::GATE_VOLTAGE, &voltages[..1])?;
                self.command(cmd::SOURCE_VOLTAGE, &voltages[1..4])?;
                self.command(cmd::WRITE_VCOM, &voltages[4..5])?;
                Ok(0xCC)
            }
        }
    }

    /// Turns the panel's power off after a refresh that left it on (a
    /// fast one), once nothing follows for a while: the picture stays,
    /// and the next refresh powers it up again (its sequence does).
    pub fn power_down(&mut self) -> Result<(), Error> {
        if self.powered {
            self.command(cmd::UPDATE_CONTROL_2, &[0x03])?;
            self.command(cmd::ACTIVATE, &[])?;
            self.wait()?;
            self.powered = false;
        }
        Ok(())
    }

    /// Whether the panel's power is on (after a fast refresh).
    pub fn powered(&self) -> bool {
        self.powered
    }

    /// Puts the controller into deep sleep, the picture staying; [`init`]
    /// wakes it again.
    ///
    /// [`init`]: Display::init
    pub fn sleep(&mut self) -> Result<(), Error> {
        if self.powered {
            self.command(cmd::UPDATE_CONTROL_1, &[0x40])?;
            self.command(cmd::UPDATE_CONTROL_2, &[0x03])?;
            self.command(cmd::ACTIVATE, &[])?;
            self.wait()?;
            self.powered = false;
        }
        self.command(cmd::DEEP_SLEEP, &[0x01])
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

    /// Whether a refresh started by [`Display::start_refresh`] still goes
    /// on, without waiting (its busy pin high): [`Display::wait`] once it
    /// is over.
    pub fn refreshing(&mut self) -> Result<bool, Error> {
        self.busy.is_high().map_err(|_| Error::Pin)
    }

    /// Waits while the controller is busy (its busy pin high), e.g. with a
    /// refresh started by [`Display::start_refresh`].
    pub fn wait(&mut self) -> Result<(), Error> {
        for _ in 0..BUSY_TIMEOUT_MS {
            if !self.busy.is_high().map_err(|_| Error::Pin)? {
                return Ok(());
            }
            self.delay.delay_ms(1);
        }
        Err(Error::Timeout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::rc::Rc;
    use alloc::vec;
    use core::cell::RefCell;
    use embedded_hal::spi::{ErrorType, Operation};
    use paperpaw_platform::PixelFormat;

    /// The controller, simulated: its two memories and what it was told.
    struct Controller {
        data: bool,
        command: u8,
        args: Vec<u8>,
        x_range: (u16, u16),
        y_range: (u16, u16),
        x: u16,
        y: u16,
        /// Rows of 100 bytes, white.
        new: Vec<u8>,
        shown: Vec<u8>,
        commands: Vec<u8>,
        /// The first argument each command had last.
        first: [u8; 256],
        refreshes: Vec<(u8, u8)>,
        /// What was sent as the waveform and as the temperature last.
        lut: Vec<u8>,
        temperature: Vec<u8>,
    }

    impl Controller {
        fn new() -> Rc<RefCell<Self>> {
            let size = usize::from(PANEL_WIDTH / 8) * usize::from(PANEL_HEIGHT);
            Rc::new(RefCell::new(Self {
                data: false,
                command: 0,
                args: Vec::new(),
                x_range: (0, 0),
                y_range: (0, 0),
                x: 0,
                y: 0,
                new: vec![0; size],
                shown: vec![0; size],
                commands: Vec::new(),
                first: [0; 256],
                refreshes: Vec::new(),
                lut: Vec::new(),
                temperature: Vec::new(),
            }))
        }

        fn byte(&mut self, b: u8) {
            if !self.data {
                self.command = b;
                self.args.clear();
                self.commands.push(b);
                match b {
                    cmd::WRITE_LUT => self.lut.clear(),
                    cmd::WRITE_TEMPERATURE => self.temperature.clear(),
                    _ => {}
                }
                if b == cmd::ACTIVATE {
                    let control_1 = self.first[usize::from(cmd::UPDATE_CONTROL_1)];
                    let control_2 = self.first[usize::from(cmd::UPDATE_CONTROL_2)];
                    self.refreshes.push((control_1, control_2));
                }
                return;
            }
            self.args.push(b);
            if self.args.len() == 1 {
                self.first[usize::from(self.command)] = b;
            }
            match self.command {
                cmd::WRITE_LUT => self.lut.push(b),
                cmd::WRITE_TEMPERATURE => self.temperature.push(b),
                _ => {}
            }
            let word = |a: &[u8], i: usize| u16::from(a[i]) | u16::from(a[i + 1]) << 8;
            match self.command {
                cmd::RAM_X_RANGE if self.args.len() == 4 => {
                    self.x_range = (word(&self.args, 0), word(&self.args, 2));
                }
                cmd::RAM_Y_RANGE if self.args.len() == 4 => {
                    self.y_range = (word(&self.args, 0), word(&self.args, 2));
                }
                cmd::RAM_X if self.args.len() == 2 => self.x = word(&self.args, 0),
                cmd::RAM_Y if self.args.len() == 2 => self.y = word(&self.args, 0),
                cmd::DATA_ENTRY => assert_eq!(b, 0x03, "X, then Y increase"),
                cmd::FILL_NEW => self.new.fill(0xFF),
                cmd::FILL_SHOWN => self.shown.fill(0xFF),
                cmd::WRITE_NEW | cmd::WRITE_SHOWN => {
                    let at = usize::from(self.y) * 100 + usize::from(self.x / 8);
                    if self.command == cmd::WRITE_NEW {
                        self.new[at] = b;
                    } else {
                        self.shown[at] = b;
                    }
                    self.x += 8;
                    if self.x > self.x_range.1 {
                        self.x = self.x_range.0;
                        self.y += 1;
                        assert!(self.y <= self.y_range.1 + 1);
                    }
                }
                _ => {}
            }
        }

        /// Whether the panel pixel at upright `(x, y)` is white in `plane`.
        fn white(plane: &[u8], x: u16, y: u16) -> bool {
            let (c, r) = panel_at(Rotation::Deg0, x, y);
            plane[usize::from(r) * 100 + usize::from(c / 8)] & (0x80 >> (c % 8)) != 0
        }
    }

    struct Spi(Rc<RefCell<Controller>>);
    struct Dc(Rc<RefCell<Controller>>);
    struct Pin;
    struct Busy;
    struct Delay;

    impl ErrorType for Spi {
        type Error = core::convert::Infallible;
    }
    impl SpiDevice for Spi {
        fn transaction(&mut self, operations: &mut [Operation<'_, u8>]) -> Result<(), Self::Error> {
            for op in operations {
                if let Operation::Write(bytes) = op {
                    for &b in bytes.iter() {
                        self.0.borrow_mut().byte(b);
                    }
                }
            }
            Ok(())
        }
    }
    impl embedded_hal::digital::ErrorType for Dc {
        type Error = core::convert::Infallible;
    }
    impl OutputPin for Dc {
        fn set_low(&mut self) -> Result<(), Self::Error> {
            self.0.borrow_mut().data = false;
            Ok(())
        }
        fn set_high(&mut self) -> Result<(), Self::Error> {
            self.0.borrow_mut().data = true;
            Ok(())
        }
    }
    impl embedded_hal::digital::ErrorType for Pin {
        type Error = core::convert::Infallible;
    }
    impl OutputPin for Pin {
        fn set_low(&mut self) -> Result<(), Self::Error> {
            Ok(())
        }
        fn set_high(&mut self) -> Result<(), Self::Error> {
            Ok(())
        }
    }
    impl embedded_hal::digital::ErrorType for Busy {
        type Error = core::convert::Infallible;
    }
    impl InputPin for Busy {
        fn is_high(&mut self) -> Result<bool, Self::Error> {
            Ok(false)
        }
        fn is_low(&mut self) -> Result<bool, Self::Error> {
            Ok(true)
        }
    }
    impl DelayNs for Delay {
        fn delay_ns(&mut self, _: u32) {}
    }

    /// A busy line that never ends.
    struct Stuck;
    impl embedded_hal::digital::ErrorType for Stuck {
        type Error = core::convert::Infallible;
    }
    impl InputPin for Stuck {
        fn is_high(&mut self) -> Result<bool, Self::Error> {
            Ok(true)
        }
        fn is_low(&mut self) -> Result<bool, Self::Error> {
            Ok(false)
        }
    }

    /// Counts the time waited, in nanoseconds.
    struct Clock(Rc<core::cell::Cell<u64>>);
    impl DelayNs for Clock {
        fn delay_ns(&mut self, ns: u32) {
            self.0.set(self.0.get() + u64::from(ns));
        }
    }

    #[test]
    fn a_stuck_busy_line_ends_before_the_watchdog() {
        let controller = Controller::new();
        let waited = Rc::new(core::cell::Cell::new(0));
        let mut display = Display::new(
            Spi(controller.clone()),
            Dc(controller),
            Pin,
            Stuck,
            Clock(waited.clone()),
        );
        assert!(matches!(display.wait(), Err(Error::Timeout)));
        let seconds = waited.get() / 1_000_000_000;
        assert!((5..20).contains(&seconds), "waited {seconds} s");
    }

    /// Where screen pixel `(x, y)` lies on the panel, turned: the
    /// controller's column and row.
    fn panel_at(rotation: Rotation, x: u16, y: u16) -> (u16, u16) {
        match rotation {
            Rotation::Deg0 => (y, x),
            Rotation::Deg90 => (PANEL_WIDTH - 1 - x, y),
            Rotation::Deg180 => (PANEL_WIDTH - 1 - y, PANEL_HEIGHT - 1 - x),
            Rotation::Deg270 => (x, PANEL_HEIGHT - 1 - y),
            _ => (y, x),
        }
    }

    /// An upright screen band in 2-bit gray, black where `ink` says.
    fn band(height: u16, ink: impl Fn(u16, u16) -> bool) -> Vec<u8> {
        band_of(WIDTH, height, ink)
    }

    /// A band `width` wide in 2-bit gray, black where `ink` says.
    fn band_of(width: u16, height: u16, ink: impl Fn(u16, u16) -> bool) -> Vec<u8> {
        let stride = usize::from(width) / 4;
        let mut data = vec![0xFF; stride * usize::from(height)];
        for y in 0..height {
            for x in 0..width {
                if ink(x, y) {
                    let at = usize::from(y) * stride + usize::from(x / 4);
                    data[at] &= !(0b11 << (6 - 2 * (x % 4)));
                }
            }
        }
        data
    }

    fn view(data: &[u8], height: u16) -> FrameView<'_> {
        FrameView::new(
            WIDTH,
            height,
            PixelFormat::Gray2,
            usize::from(WIDTH) / 4,
            data,
        )
    }

    #[test]
    fn eight_by_eight_turned() {
        let rows = [
            0b1000_0000,
            0b0100_0001,
            0,
            0xFF,
            0b0000_0001,
            0,
            0,
            0b1000_0000,
        ];
        let columns = bands::transpose8(rows);
        for (r, row) in rows.iter().enumerate() {
            for (c, column) in columns.iter().enumerate() {
                let row_bit = row >> (7 - c) & 1;
                let column_bit = column >> (7 - r) & 1;
                assert_eq!(row_bit, column_bit, "row {r}, column {c}");
            }
        }
    }

    #[test]
    fn black_and_white_bands_as_gray_ones() {
        // The same ink in black and white and in gray: the same planes,
        // also for a band whose height is not a multiple of 8.
        let ink = |x: u16, y: u16| (x * 7 + y * 3).is_multiple_of(5) || x == y;
        for height in [16, 13] {
            let gray = band(height, ink);
            let stride = usize::from(WIDTH) / 8;
            let mut mono = vec![0xFFu8; stride * usize::from(height)];
            for y in 0..height {
                for x in 0..WIDTH {
                    if ink(x, y) {
                        mono[usize::from(y) * stride + usize::from(x) / 8] &= !(0x80 >> (x % 8));
                    }
                }
            }
            let planes = |view: FrameView<'_>| {
                let controller = Controller::new();
                let mut display = Display::new(
                    Spi(controller.clone()),
                    Dc(controller.clone()),
                    Pin,
                    Busy,
                    Delay,
                );
                display.init().unwrap();
                display.write_rows(96, view, Planes::NEW).unwrap();
                controller.borrow().new.clone()
            };
            let mono_view = FrameView::new(WIDTH, height, PixelFormat::Mono1, stride, &mono);
            assert!(
                planes(view(&gray, height)) == planes(mono_view),
                "height {height}"
            );
        }
    }

    #[test]
    fn bands_go_in_turned_into_the_planes_asked() {
        let controller = Controller::new();
        let mut display = Display::new(
            Spi(controller.clone()),
            Dc(controller.clone()),
            Pin,
            Busy,
            Delay,
        );
        display.init().unwrap();
        // A band of 16 rows from row 96: ink on its diagonal.
        let ink = |x: u16, y: u16| x == y * 3;
        let data = band(16, ink);
        display
            .write_rows(96, view(&data, 16), Planes::NEW)
            .unwrap();
        let c = controller.borrow();
        for y in 96..112 {
            for x in 0..WIDTH {
                let expected = !ink(x, y - 96);
                assert_eq!(Controller::white(&c.new, x, y), expected, "({x}, {y})");
                assert!(Controller::white(&c.shown, x, y), "shown untouched");
            }
        }
        // Rows of other bands stay white.
        assert!(Controller::white(&c.new, 0, 95));
        assert!(Controller::white(&c.new, 0, 112));
        drop(c);
        // After a fast refresh, the same into the frame shown.
        display.refresh(Refresh::Fast, false).unwrap();
        display
            .write_rows(96, view(&data, 16), Planes::SHOWN)
            .unwrap();
        let c = controller.borrow();
        assert_eq!(c.new, c.shown);
    }

    #[test]
    fn the_kernels_bands_make_its_frame() {
        use paperpaw_core::Kernel;
        use paperpaw_platform::{
            DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, Rotation,
        };
        let device = DeviceInfo::new(
            "x4",
            "X4",
            DisplayInfo::new(
                WIDTH,
                HEIGHT,
                220,
                PixelFormat::Gray2,
                Rotation::Deg90,
                None,
            ),
            InputCaps::new(true, false, false, &[]),
            MemoryClass::Small,
        );
        let build = |banded: bool| {
            let mut builder = Kernel::builder(device);
            if banded {
                builder = builder.frame_rows(96).row_align(8);
            }
            let mut kernel = paperpaw_app::install(builder, paperpaw_app::Fonts::Packs)
                .build()
                .unwrap();
            kernel.start(Instant(0));
            kernel
        };
        // The whole frame at once, thresholded as the display does.
        let mut whole = build(false);
        let mut expected = vec![false; usize::from(WIDTH) * usize::from(HEIGHT)];
        whole.render(&mut |top, rows| {
            for y in 0..rows.height {
                for x in 0..rows.width {
                    let c = rows.pixel(x, y);
                    let white = u16::from(c.r) * 77 + u16::from(c.g) * 150 + u16::from(c.b) * 29
                        >= 128 * 256;
                    expected[usize::from(top + y) * usize::from(WIDTH) + usize::from(x)] = white;
                }
            }
        });
        assert!(expected.iter().any(|w| !w), "something is drawn");
        // In bands, into the controller.
        let controller = Controller::new();
        let mut display = Display::new(
            Spi(controller.clone()),
            Dc(controller.clone()),
            Pin,
            Busy,
            Delay,
        );
        display.init().unwrap();
        let mut banded = build(true);
        let mut bands = 0;
        banded.render(&mut |top, rows| {
            bands += 1;
            display.write_rows(top, rows, Planes::NEW).unwrap();
        });
        assert!(bands > 1);
        let c = controller.borrow();
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let at = usize::from(y) * usize::from(WIDTH) + usize::from(x);
                assert_eq!(Controller::white(&c.new, x, y), expected[at], "({x}, {y})");
            }
        }
    }

    #[test]
    fn turned_bands_lie_on_the_panel_as_turned() {
        // On its side is the X4 turned clockwise: the screen's top left
        // is the upright screen's bottom left.
        assert_eq!(
            panel_at(Rotation::Deg90, 0, 0),
            panel_at(Rotation::Deg0, 0, HEIGHT - 1)
        );
        assert_eq!(
            panel_at(Rotation::Deg270, 0, 0),
            panel_at(Rotation::Deg0, WIDTH - 1, 0)
        );
        let ink = |x: u16, y: u16| (x * 7 + y * 3).is_multiple_of(5) || x == y;
        for rotation in [
            Rotation::Deg0,
            Rotation::Deg90,
            Rotation::Deg180,
            Rotation::Deg270,
        ] {
            let controller = Controller::new();
            let mut display = Display::new(
                Spi(controller.clone()),
                Dc(controller.clone()),
                Pin,
                Busy,
                Delay,
            );
            display.init().unwrap();
            display.set_rotation(rotation);
            let (width, height) = display.size();
            assert_eq!(u32::from(width) * u32::from(height), 800 * 480);
            // In gray into one plane, in black and white into the other.
            let gray = band_of(width, 16, ink);
            let stride = usize::from(width) / 8;
            let mut mono = vec![0xFFu8; stride * 16];
            for y in 0..16 {
                for x in 0..width {
                    if ink(x, y) {
                        mono[usize::from(y) * stride + usize::from(x) / 8] &= !(0x80 >> (x % 8));
                    }
                }
            }
            let view = |data, format, stride| FrameView::new(width, 16, format, stride, data);
            display
                .write_rows(96, view(&gray, PixelFormat::Gray2, stride * 2), Planes::NEW)
                .unwrap();
            display
                .write_rows(96, view(&mono, PixelFormat::Mono1, stride), Planes::SHOWN)
                .unwrap();
            let c = controller.borrow();
            let white = |plane: &[u8], (c, r): (u16, u16)| {
                plane[usize::from(r) * 100 + usize::from(c / 8)] & (0x80 >> (c % 8)) != 0
            };
            for y in 96..112 {
                for x in 0..width {
                    let at = panel_at(rotation, x, y);
                    let expected = !ink(x, y - 96);
                    assert_eq!(white(&c.new, at), expected, "{rotation:?} ({x}, {y})");
                    assert_eq!(white(&c.shown, at), expected, "{rotation:?} ({x}, {y})");
                }
            }
            // The rest stays white.
            for (x, y) in [(0, 95), (width - 1, 112), (width / 2, height - 1)] {
                assert!(white(&c.new, panel_at(rotation, x, y)), "{rotation:?}");
            }
            drop(c);
            let wrong = view(&gray, PixelFormat::Gray2, stride * 2).with_width(height);
            assert_eq!(
                display.write_rows(96, wrong, Planes::NEW),
                Err(Error::Rows),
                "{rotation:?}: not the turned width"
            );
        }
    }

    #[test]
    fn four_grays_go_into_both_memories() {
        let controller = Controller::new();
        let mut display = Display::new(
            Spi(controller.clone()),
            Dc(controller.clone()),
            Pin,
            Busy,
            Delay,
        );
        display.init().unwrap();
        // Columns of the four grays, black on the left.
        let stride = usize::from(WIDTH) / 4;
        let mut data = vec![0u8; stride * 8];
        for y in 0..8 {
            for x in 0..WIDTH {
                let level = (x / 120) as u8;
                data[y * stride + usize::from(x / 4)] |= level << (6 - 2 * (x % 4));
            }
        }
        let rows = FrameView::new(WIDTH, 8, PixelFormat::Gray2, stride, &data);
        display.write_gray_rows(16, rows).unwrap();
        let c = controller.borrow();
        // The waveform's levels: black 11, dark gray 10, light gray 01,
        // white 00 (a set bit is "white" to the tests' reading).
        for (x, high, low) in [
            (0, true, true),
            (130, true, false),
            (250, false, true),
            (470, false, false),
        ] {
            assert_eq!(Controller::white(&c.new, x, 20), low, "low bit at {x}");
            assert_eq!(Controller::white(&c.shown, x, 20), high, "high bit at {x}");
        }
        drop(c);
        let mono = vec![0xFFu8; usize::from(WIDTH) / 8 * 8];
        let mono = FrameView::new(WIDTH, 8, PixelFormat::Mono1, usize::from(WIDTH) / 8, &mono);
        assert_eq!(
            display.write_gray_rows(16, mono),
            Err(Error::Rows),
            "not gray"
        );
        // The gray waveform loaded, both memories read; then a half refresh
        // drives every pixel again, also where a fast one was asked for.
        display.refresh(Refresh::Fast, false).unwrap();
        display.refresh_gray().unwrap();
        display.refresh(Refresh::Fast, false).unwrap();
        display.refresh(Refresh::Fast, false).unwrap();
        let c = controller.borrow();
        assert_eq!(c.first[usize::from(cmd::WRITE_LUT)], GRAY_LUT[0]);
        let sequences: Vec<(u8, u8)> = c.refreshes.clone();
        assert_eq!(
            sequences,
            [(0x40, 0xD7), (0x00, 0xCC), (0x40, 0xD7), (0x00, 0xFC)]
        );
    }

    #[test]
    fn the_tones_lighten_the_two_grays_less_after_their_black() {
        // Frames of white after the black, for a level's phase groups.
        let white = |lut: &[u8; 110], level: usize| {
            let frames = [[12, 2, 7, 2], [1, 0, 2, 0]];
            (0..2)
                .map(|group| {
                    let byte = lut[level * 10 + 1 + group];
                    (0..4)
                        .filter(|&p| (byte >> (6 - 2 * p)) & 0b11 == 0b10)
                        .map(|p| frames[group][p])
                        .sum::<u32>()
                })
                .sum::<u32>()
        };
        assert_eq!(GrayTone::Stock.lut(), GRAY_LUT);
        let lightened = |tone: GrayTone| (white(&tone.lut(), 2), white(&tone.lut(), 1));
        assert_eq!(lightened(GrayTone::Stock), (2, 4));
        assert_eq!(lightened(GrayTone::Middle), (1, 3));
        let lut = GrayTone::Middle.lut();
        let changed: Vec<usize> = (0..lut.len()).filter(|&i| lut[i] != GRAY_LUT[i]).collect();
        assert_eq!(changed, [11, 12, 21, 22]);
        // The 12 frames of black stay first.
        assert_eq!((lut[11] >> 6, lut[21] >> 6), (0b01, 0b01));
    }

    #[test]
    fn a_short_last_band_is_white_below() {
        let controller = Controller::new();
        let mut display = Display::new(
            Spi(controller.clone()),
            Dc(controller.clone()),
            Pin,
            Busy,
            Delay,
        );
        display.init().unwrap();
        let data = band(5, |_, _| true);
        display
            .write_rows(792, view(&data, 5), Planes::BOTH)
            .unwrap();
        let c = controller.borrow();
        assert!(!Controller::white(&c.new, 10, 796));
        assert!(Controller::white(&c.new, 10, 797));
        assert!(!Controller::white(&c.shown, 10, 792));
        drop(c);
        assert_eq!(
            display.write_rows(4, view(&data, 5), Planes::BOTH),
            Err(Error::Rows),
            "not aligned"
        );
    }

    #[test]
    fn refreshes_power_off_when_asked() {
        let controller = Controller::new();
        let mut display = Display::new(
            Spi(controller.clone()),
            Dc(controller.clone()),
            Pin,
            Busy,
            Delay,
        );
        display.init().unwrap();
        // The first drives every pixel whatever is asked; a fast one keeps
        // the panel powered, sleep powers it down first.
        display.refresh(Refresh::Fast, false).unwrap();
        display.refresh(Refresh::Fast, false).unwrap();
        display.refresh(Refresh::Fast, true).unwrap();
        display.sleep().unwrap();
        let c = controller.borrow();
        let sequences: Vec<u8> = c.refreshes.iter().map(|&(_, two)| two).collect();
        assert_eq!(sequences, [0xD7, 0xFC, 0xFF]);
        assert_eq!(c.commands.last(), Some(&cmd::DEEP_SLEEP));
    }

    #[test]
    fn a_fast_refresh_leaves_the_power_on_until_powered_down() {
        let controller = Controller::new();
        let mut display = Display::new(
            Spi(controller.clone()),
            Dc(controller.clone()),
            Pin,
            Busy,
            Delay,
        );
        display.init().unwrap();
        display.refresh(Refresh::Fast, false).unwrap();
        assert!(!display.powered(), "the first is a half one");
        display.refresh(Refresh::Fast, false).unwrap();
        assert!(display.powered());
        display.power_down().unwrap();
        assert!(!display.powered());
        // Once is enough.
        display.power_down().unwrap();
        let sequences: Vec<u8> = controller
            .borrow()
            .refreshes
            .iter()
            .map(|&(_, two)| two)
            .collect();
        assert_eq!(sequences, [0xD7, 0xFC, 0x03]);
    }

    type Shown = (Rc<RefCell<Controller>>, Display<Spi, Dc, Pin, Busy, Delay>);

    /// A display that has shown two frames (the first is a half refresh),
    /// so the next fast refresh is one.
    fn shown_twice() -> Shown {
        let controller = Controller::new();
        let mut display = Display::new(
            Spi(controller.clone()),
            Dc(controller.clone()),
            Pin,
            Busy,
            Delay,
        );
        display.init().unwrap();
        display.refresh(Refresh::Fast, false).unwrap();
        display.refresh(Refresh::Fast, false).unwrap();
        (controller, display)
    }

    #[test]
    fn an_experiment_at_a_temperature_writes_it_and_loads_the_panels_waveform() {
        let (controller, mut display) = shown_twice();
        display.set_experiment(Some(Experiment::Temperature(45)));
        display.refresh(Refresh::Fast, false).unwrap();
        // The temperature is not read from the sensor (bit 5 of the
        // sequence), the panel's own table is loaded (bit 4).
        let c = controller.borrow();
        assert_eq!(c.temperature, [45]);
        assert_eq!(c.refreshes.last(), Some(&(0x00, 0xDC)));
        assert!(!c.commands.ends_with(&[cmd::WRITE_LUT]));
    }

    #[test]
    fn an_experiment_keeps_to_the_temperatures_of_the_stock_firmware() {
        let (controller, mut display) = shown_twice();
        for (asked, written) in [(-30, 0), (0, 0), (25, 25), (90, 90), (120, 90)] {
            display.set_experiment(Some(Experiment::Temperature(asked)));
            display.refresh(Refresh::Fast, false).unwrap();
            assert_eq!(controller.borrow().temperature, [written as u8], "{asked}");
        }
    }

    #[test]
    fn a_direct_experiment_loads_its_waveform_at_the_stock_voltages() {
        let (controller, mut display) = shown_twice();
        let direct = waveform::Direct::new(6, 1);
        display.set_experiment(Some(Experiment::Direct(direct)));
        display.refresh(Refresh::Fast, false).unwrap();
        let c = controller.borrow();
        assert_eq!(c.lut, direct.lut());
        // Loaded as the gray waveform is: the loaded one, not the panel's.
        assert_eq!(c.refreshes.last(), Some(&(0x00, 0xCC)));
        // The voltages are the stock gray waveform's, not new ones.
        assert_eq!(c.first[usize::from(cmd::GATE_VOLTAGE)], GRAY_LUT[105]);
        assert_eq!(c.first[usize::from(cmd::SOURCE_VOLTAGE)], GRAY_LUT[106]);
        assert_eq!(c.first[usize::from(cmd::WRITE_VCOM)], GRAY_LUT[109]);
        assert!(display.powered(), "left on, as after a fast refresh");
    }

    #[test]
    fn only_fast_refreshes_take_an_experiment() {
        let (controller, mut display) = shown_twice();
        display.set_experiment(Some(Experiment::Direct(waveform::Direct::new(4, 0))));
        display.refresh(Refresh::Half, false).unwrap();
        display.refresh(Refresh::Full, false).unwrap();
        display.refresh_gray().unwrap();
        let sequences: Vec<u8> = controller
            .borrow()
            .refreshes
            .iter()
            .map(|&(_, two)| two)
            .collect();
        // The two fast ones before, then the stock half and full, the grays.
        assert_eq!(sequences, [0xD7, 0xFC, 0xD7, 0xF7, 0xCC]);
        // And the stock fast again once the experiment is off.
        display.set_experiment(None);
        display.refresh(Refresh::Fast, false).unwrap();
        display.refresh(Refresh::Fast, false).unwrap();
        let last: Vec<u8> = controller
            .borrow()
            .refreshes
            .iter()
            .rev()
            .take(2)
            .map(|&(_, two)| two)
            .collect();
        assert_eq!(last, [0xFC, 0xD7], "grays are followed by a half refresh");
    }

    #[test]
    fn the_first_refresh_is_not_an_experiment() {
        let controller = Controller::new();
        let mut display = Display::new(
            Spi(controller.clone()),
            Dc(controller.clone()),
            Pin,
            Busy,
            Delay,
        );
        display.init().unwrap();
        display.set_experiment(Some(Experiment::Temperature(60)));
        display.refresh(Refresh::Fast, false).unwrap();
        // Nothing is known to be shown yet: every pixel is driven, the
        // stock way.
        assert_eq!(controller.borrow().refreshes, [(0x40, 0xD7)]);
    }

    #[test]
    fn the_stock_sequences() {
        let controller = Controller::new();
        let mut display = Display::new(
            Spi(controller.clone()),
            Dc(controller.clone()),
            Pin,
            Busy,
            Delay,
        );
        display.init().unwrap();
        // The first is a half whatever is asked; full, half and fast as the
        // stock firmware sends them.
        for refresh in [
            Refresh::Fast,
            Refresh::Full,
            Refresh::Half,
            Refresh::Fast,
            Refresh::Fast,
        ] {
            display.refresh(refresh, false).unwrap();
        }
        let c = controller.borrow();
        let sequences: Vec<u8> = c.refreshes.iter().map(|&(_, two)| two).collect();
        assert_eq!(sequences, [0xD7, 0xF7, 0xD7, 0xFC, 0xFC]);
    }
}
