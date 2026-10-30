//! The UltraChip displays: an 800 × 480 panel driven by a UC8179 or a
//! UC8279 controller over SPI (the X4 Pro and the X4 Classic have either,
//! by batch; see [`probe`](crate::probe)), with the same calls as
//! [`display`](crate::display), so the firmware drives any.
//!
//! The controller keeps two frames: the new one (`DTM2`) and the one
//! shown (`DTM1`), which a fast refresh compares with. Where there is the
//! memory for it (the X4 Pro's PSRAM), the driver keeps the frame too
//! ([`Frame::Kept`]): [`Display::write_rows`] draws PaperPaw's bands into
//! it, each refresh sends it whole, and after a refresh it goes into the
//! frame shown too, for the next fast one to compare with. Where there is
//! not ([`Frame::InController`], a device with the X4's 192 KB), the bands
//! go straight into the controller's frames as the SSD1677's do, each
//! through a window of the controller's memory (`PTL`, entered with
//! `PTIN` and left with `PTOUT`): into the new frame, the one shown or
//! both, as [`Planes`] says (laid out as for the SSD1677).
//!
//! The controller addresses 800 × 600 pixels (`TRES`); the panel shows
//! 480 of the lines: on a UC8179 the first, sent last line first, on a
//! UC8279 the last, in order. A byte holds 8 pixels of a line, the
//! leftmost in the highest bit, set for white. PaperPaw draws the screen
//! upright (480 × 800): the panel turned, the screen's row `y` the
//! controller's column `y`, its column `x` the controller's line `x` on a
//! UC8179 (as seen on the device: the other way it shows mirrored) and
//! `599 - x` on a UC8279.
//!
//! The refreshes use the panel's own waveforms (`PSR` 0x1F on a UC8179,
//! 0x17 on a UC8279), chosen by the temperature told (`TSSET`): 30 °C
//! drives every pixel (full and half), 90 °C the fast waveform, only where
//! the two frames differ. A UC8279 forgets its panel setting when its
//! power comes on (it is set again after) and refreshes fast only within
//! a window. These sequences are the ones the FreeInk SDK records for the
//! X4 Pro from its stock firmware (MIT licence), as Papyrix uses them.
//!
//! Four grays ([`Display::write_gray_rows`], [`Display::refresh_gray`])
//! come in two steps on a UC8179 with its frame kept, as in the FreeInk
//! SDK: the frame is shown in black and white first, every gray black;
//! then the SDK's gray waveforms (`PSR` 0x3F, from the LUT registers)
//! lighten the grays, chosen by a code in *old* and *new*: 00 not at all
//! (black), 11 a little (dark gray), 10 more (light gray); white, already
//! white, takes 11 too. The next refresh after them drives every pixel
//! back to black or white, as a half refresh if a fast one was asked (the
//! SDK's waveforms for a fast one leave the grays a little light). The
//! first step needs the frame kept, the UC8279 waveforms of its own that
//! are not known yet: otherwise grays are shown in black and white.

use alloc::vec;
use alloc::vec::Vec;

use embedded_hal::delay::DelayNs;
use embedded_hal::digital::{InputPin, OutputPin};
use embedded_hal::spi::SpiDevice;
use paperpaw_platform::{FrameView, PixelFormat, Rotation};

use crate::bands::{self, Window};
use crate::display::BUSY_TIMEOUT_MS;
pub use crate::display::{Error, PANEL_HEIGHT, PANEL_WIDTH, Planes, Refresh};
use crate::waveform::Direct;

/// The screen PaperPaw draws upright (see [`Display::size`] for it
/// turned).
pub const WIDTH: u16 = PANEL_HEIGHT;
pub const HEIGHT: u16 = PANEL_WIDTH;

/// Lines the controller addresses; those past the panel's are white.
const ADDRESSED_LINES: u16 = 600;
/// Bytes of a line.
const LINE: usize = PANEL_WIDTH as usize / 8;

/// The controller's commands used here.
mod cmd {
    pub const PANEL_SETTING: u8 = 0x00;
    pub const POWER_SETTING: u8 = 0x01;
    pub const POWER_OFF: u8 = 0x02;
    pub const POWER_OFF_SEQUENCE: u8 = 0x03;
    pub const POWER_ON: u8 = 0x04;
    pub const BOOSTER_SOFT_START: u8 = 0x06;
    pub const DEEP_SLEEP: u8 = 0x07;
    pub const OLD: u8 = 0x10;
    pub const REFRESH: u8 = 0x12;
    pub const NEW: u8 = 0x13;
    pub const PLL: u8 = 0x30;
    pub const VCOM_DATA_INTERVAL: u8 = 0x50;
    pub const RESOLUTION: u8 = 0x61;
    pub const GATE_SOURCE_START: u8 = 0x65;
    pub const VCOM_DC: u8 = 0x82;
    pub const PARTIAL_WINDOW: u8 = 0x90;
    pub const PARTIAL_IN: u8 = 0x91;
    pub const PARTIAL_OUT: u8 = 0x92;
    pub const CASCADE: u8 = 0xE0;
    pub const GATE_SCAN: u8 = 0xE1;
    pub const POWER_SAVING: u8 = 0xE3;
    pub const TEMPERATURE: u8 = 0xE5;
}

/// Which UltraChip controller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chip {
    Uc8179,
    Uc8279,
}

/// The frame rates a trial may ask the controller for (the `PLL` register's
/// codes and what they are in Hz; the datasheet has more, up to 200 Hz, none
/// is taken above 100 here), the controller's own 50 Hz fourth.
pub const FRAME_RATES: [(u8, u16); 13] = [
    (0x03, 20),
    (0x04, 30),
    (0x05, 40),
    (0x06, 50),
    (0x07, 60),
    (0x08, 70),
    (0x09, 80),
    (0x0A, 90),
    (0x0B, 100),
    (0x0C, 110),
    (0x0D, 130),
    (0x0E, 150),
    (0x0F, 200),
];
/// The index of the controller's own frame rate in [`FRAME_RATES`].
pub const OWN_RATE: usize = 3;

/// The voltages a waveform from the registers runs at: those the OTP's own
/// waveform uses (its range's header, [`otp::power`](crate::otp::power)),
/// which the registers hold only as the reset left them (VDH and VDL
/// 14.0 V, VCOM_DC next to nothing). As the codes of the registers: the gate
/// level (`VG_LVL`, 3 bits), `VDH_LVL`, `VDL_LVL` and `VDHR_LVL` (6 bits),
/// `VDCS` (7 bits).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Power {
    pub gate: u8,
    pub vdh: u8,
    pub vdl: u8,
    pub vdhr: u8,
    pub vcom: u8,
}

/// A way to show fast refreshes other than the panel's own, for trying
/// waveforms on a device (the waveform lab, `shells/xteink/src/lab.rs`);
/// UC8179 only. Set with [`Display::set_experiment`]; only fast refreshes
/// take it (not the first after [`Display::init`] or after grays, which
/// drive every pixel), so a half or full refresh cleans up after it. The
/// voltages are the controller's own (the registers' defaults), which are
/// not set here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Experiment {
    /// What a pixel that changes is driven with ([`Direct::uc8179_luts`]).
    pub drive: Direct,
    /// Which level gives white is turned round ([`Direct::uc8179_luts`]).
    pub swapped: bool,
    /// The group's times to repeat, 0 or 1.
    pub repeat: u8,
    /// Frames (0 to 3) a pixel that stays is driven to its own colour
    /// ([`Direct::uc8179_luts`]).
    pub hold: u8,
    /// The frame rate, an index of [`FRAME_RATES`].
    pub rate: usize,
    /// The panel's own waveform, the OTP's, at this temperature in degrees
    /// (what the controller takes for the temperature it is told, `TSSET`):
    /// then nothing above is used.
    pub own_at: Option<i8>,
}

/// Where the frame is kept between refreshes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Frame {
    /// In the driver (48 KB, 144 KB with grays) and the controller.
    Kept,
    /// In the controller alone.
    InController,
}

/// What differs between the two controllers.
struct Sequences {
    /// The panel setting at start, and for the panel's own waveforms.
    panel: [u8; 2],
    panel_otp: [u8; 2],
    /// Set again after the power comes on (the UC8279 forgets it).
    panel_after_power_on: bool,
    /// The border and data interval while refreshing fully and fast, and
    /// at rest.
    interval_full: &'static [u8],
    interval_fast: &'static [u8],
    interval_idle: Option<&'static [u8]>,
    /// The frame rate, if not the controller's own.
    pll: Option<u8>,
    /// The booster and power saving, if not the controller's own.
    booster: Option<[u8; 4]>,
    power_saving: Option<u8>,
    /// The first line the panel shows; its lines go in order, or last
    /// first.
    first_line: u16,
    lines_back: bool,
    /// A fast refresh needs a window: without one, nothing develops.
    fast_in_window: bool,
    /// Waveforms for four grays from black and white.
    gray: Option<&'static [[u8; 13]; 5]>,
}

const UC8179: Sequences = Sequences {
    panel: [0x3F, 0x0A],
    panel_otp: [0x1F, 0x0A],
    panel_after_power_on: false,
    interval_full: &[0x29, 0x07],
    interval_fast: &[0x29, 0x07],
    interval_idle: Some(&[0xA9, 0x07]),
    pll: None,
    booster: Some([0x25, 0x25, 0x3C, 0x25]),
    power_saving: Some(0x22),
    first_line: 0,
    lines_back: false,
    fast_in_window: false,
    gray: Some(&GRAY_LUTS),
};

const UC8279: Sequences = Sequences {
    panel: [0x37, 0x4D],
    panel_otp: [0x17, 0x4D],
    panel_after_power_on: true,
    interval_full: &[0x97],
    interval_fast: &[0xD7],
    interval_idle: None,
    pll: Some(0x0E),
    booster: None,
    power_saving: None,
    first_line: ADDRESSED_LINES - PANEL_HEIGHT,
    lines_back: true,
    fast_in_window: true,
    gray: None,
};

/// Bytes of a waveform register; the tables below give the first 12, the
/// rest are 0.
const LUT_LEN: usize = 42;

/// The waveforms for four grays (registers 0x20 to 0x24: VCOM, then by
/// *old* and *new* bit: 11, 01, 10, 00), from the panel showing every
/// gray black: 11 lightens one frame, 01 and 10 two, 00 none.
const GRAY_LUTS: [[u8; 13]; 5] = [
    [
        0x20, 0x00, 0x02, 0x02, 0x01, 0x01, 0x01, 0, 0, 0, 0, 0, 0x01,
    ],
    [
        0x21, 0x08, 0x02, 0x02, 0x01, 0x01, 0x01, 0, 0, 0, 0, 0, 0x01,
    ],
    [
        0x22, 0x20, 0x02, 0x02, 0x01, 0x01, 0x01, 0, 0, 0, 0, 0, 0x01,
    ],
    [
        0x23, 0x20, 0x02, 0x02, 0x01, 0x01, 0x01, 0, 0, 0, 0, 0, 0x01,
    ],
    [
        0x24, 0x00, 0x02, 0x02, 0x01, 0x01, 0x01, 0, 0, 0, 0, 0, 0x01,
    ],
];

/// A picture of the panel as [`Display`] keeps it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Plane {
    /// The frame in black and white.
    Frame,
    /// The grays' code in *old*: set but for black.
    Old,
    /// In *new*: set for dark gray and white.
    New,
}

/// How much of the frame kept goes to the controller for a fast refresh.
/// The refresh itself always covers the whole panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Transfer {
    /// The frame whole into *new* before, and into *old* after: what the
    /// stock firmware does.
    #[default]
    Whole,
    /// Only the area written since the last refresh, into both.
    Changed,
    /// Only that area into *new*; the controller's own copy of *new* into
    /// *old* after the refresh is left to make *old* the frame shown.
    Copied,
}

/// An area of the frame, in bytes of a line and lines (the last ones
/// included).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Area {
    first_byte: usize,
    last_byte: usize,
    first_line: usize,
    last_line: usize,
}

impl Area {
    fn join(self, other: Self) -> Self {
        Self {
            first_byte: self.first_byte.min(other.first_byte),
            last_byte: self.last_byte.max(other.last_byte),
            first_line: self.first_line.min(other.first_line),
            last_line: self.last_line.max(other.last_line),
        }
    }
}

/// What [`Display::wait`] does once the controller is done refreshing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Pending {
    /// Only this area of the frame went into *new* (none: nothing), and
    /// goes into *old*; not all of it if `None`.
    sent: Option<Option<Area>>,
    /// Leave the partial mode a fast refresh entered.
    partial: bool,
    power_off: bool,
    /// A trial set another frame rate: the controller's own again.
    restore_rate: bool,
}

/// The display, on its SPI device (which selects the chip) and pins.
#[derive(Debug)]
pub struct Display<SPI, DC, RST, BUSY, D> {
    spi: SPI,
    dc: DC,
    reset: RST,
    busy: BUSY,
    delay: D,
    chip: Chip,
    /// The frame rate is set ([`Sequences::pll`]); not where the stock
    /// firmware leaves the factory's ([`Display::with_factory_pll`]).
    set_pll: bool,
    /// The frame, line by line as the panel lies (480 lines of 100 bytes),
    /// if it is kept; empty if not.
    frame: Vec<u8>,
    /// The grays' codes for *old* and *new*, laid out as `frame`; empty
    /// until grays are first written.
    old: Vec<u8>,
    new: Vec<u8>,
    /// The panel shows grays: the next fast refresh drives them away.
    gray_shown: bool,
    /// The controller's *old* memory holds the frame shown, for a fast
    /// refresh to compare with.
    old_valid: bool,
    /// Both of the controller's memories hold the frame but for `dirty`.
    synced: bool,
    /// What was written to the frame since it went to the controller.
    dirty: Option<Area>,
    transfer: Transfer,
    /// Milliseconds spent waiting for the busy pin since
    /// [`Display::take_busy_ms`].
    busy_ms: u32,
    powered: bool,
    rotation: Rotation,
    pending: Option<Pending>,
    /// A band's bytes on their way, without a frame kept.
    buffer: Vec<u8>,
    /// What fast refreshes are shown with, if not the panel's own.
    experiment: Option<Experiment>,
    /// The voltages of a waveform from the registers, if known.
    trial_power: Option<Power>,
}

impl<SPI, DC, RST, BUSY, D> Display<SPI, DC, RST, BUSY, D>
where
    SPI: SpiDevice,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
    D: DelayNs,
{
    pub fn new(
        chip: Chip,
        frame: Frame,
        spi: SPI,
        dc: DC,
        reset: RST,
        busy: BUSY,
        delay: D,
    ) -> Self {
        Self {
            spi,
            dc,
            reset,
            busy,
            delay,
            chip,
            set_pll: true,
            frame: match frame {
                Frame::Kept => vec![0xFF; LINE * usize::from(PANEL_HEIGHT)],
                Frame::InController => Vec::new(),
            },
            old: Vec::new(),
            new: Vec::new(),
            gray_shown: false,
            old_valid: false,
            synced: false,
            dirty: None,
            transfer: Transfer::Whole,
            busy_ms: 0,
            powered: false,
            rotation: Rotation::Deg0,
            pending: None,
            buffer: Vec::new(),
            experiment: None,
            trial_power: None,
        }
    }

    /// The voltages a waveform from the registers runs at (see [`Power`]);
    /// without, the controller's reset values.
    pub fn set_trial_power(&mut self, power: Option<Power>) {
        self.trial_power = power;
    }

    /// The milliseconds spent waiting for the controller since the last
    /// call: after [`Display::start_refresh`] what the setup waited for,
    /// after [`Display::wait`] mostly the waveform.
    pub fn take_busy_ms(&mut self) -> u32 {
        core::mem::take(&mut self.busy_ms)
    }

    /// Sets how much of the frame fast refreshes send (see [`Transfer`]).
    pub fn set_transfer(&mut self, transfer: Transfer) {
        self.transfer = transfer;
    }

    /// Shows fast refreshes as `experiment` says (or the panel's own where
    /// `None`); `false`, and nothing set, where the controller is not a
    /// UC8179.
    pub fn set_experiment(&mut self, experiment: Option<Experiment>) -> bool {
        if self.chip != Chip::Uc8179 {
            return false;
        }
        self.experiment = experiment;
        true
    }

    /// Keeps the frame rate the controller has from the factory: the X4
    /// Classic's stock firmware never sets it.
    #[must_use]
    pub fn with_factory_pll(mut self) -> Self {
        self.set_pll = false;
        self
    }

    pub fn chip(&self) -> Chip {
        self.chip
    }

    fn sequences(&self) -> &'static Sequences {
        match self.chip {
            Chip::Uc8179 => &UC8179,
            Chip::Uc8279 => &UC8279,
        }
    }

    fn kept(&self) -> bool {
        !self.frame.is_empty()
    }

    /// Whether the driver keeps the frame ([`Frame::Kept`]): every
    /// refresh then leaves it in both of the controller's frames, with no
    /// need to send it again.
    pub fn keeps_frame(&self) -> bool {
        self.kept()
    }

    /// How the screen PaperPaw draws is turned: [`Rotation::Deg90`] with
    /// the frame kept is the X4 Pro turned anticlockwise (the right side
    /// of the upright screen on top), without it the X4 turned clockwise
    /// (as [`display::Display::set_rotation`](crate::display::Display::set_rotation)).
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

    /// Resets and sets up the controller; the next refresh drives every
    /// pixel. Without the frame kept, both of the controller's frames are
    /// made white.
    pub fn init(&mut self) -> Result<(), Error> {
        for (high, ms) in [(true, 2), (false, 50), (true, 30)] {
            if high {
                self.reset.set_high()
            } else {
                self.reset.set_low()
            }
            .map_err(|_| Error::Pin)?;
            self.delay.delay_ms(ms);
        }
        self.wait_ready()?;
        let s = self.sequences();
        // The panel's own waveforms from its memory; 800 × 600 addressed.
        self.command(cmd::PANEL_SETTING, &s.panel)?;
        self.command(cmd::RESOLUTION, &[0x03, 0x20, 0x02, 0x58])?;
        self.command(cmd::GATE_SOURCE_START, &[0x00, 0x00, 0x00, 0x00])?;
        self.command(cmd::POWER_OFF_SEQUENCE, &[0x20])?;
        if let Some(booster) = s.booster {
            self.command(cmd::BOOSTER_SOFT_START, &booster)?;
        }
        if let Some(pll) = s.pll.filter(|_| self.set_pll) {
            self.command(cmd::PLL, &[pll])?;
        }
        self.command(cmd::GATE_SCAN, &[0x02])?;
        if let Some(power_saving) = s.power_saving {
            self.command(cmd::POWER_SAVING, &[power_saving])?;
        }
        if !self.kept() {
            self.fill(cmd::OLD, 0xFF)?;
            self.fill(cmd::NEW, 0xFF)?;
        }
        self.old_valid = false;
        self.synced = false;
        self.dirty = None;
        self.gray_shown = false;
        self.powered = false;
        self.pending = None;
        Ok(())
    }

    /// Writes screen rows `top..top + rows.height` (a band from
    /// `Kernel::render`, the screen's width as turned) into the frame.
    /// Pixels lighter than middle gray are white. With the frame kept,
    /// it is sent whole at the refresh, and `planes` only say whether the
    /// band is new: what goes only into *shown* after a fast refresh is
    /// there already. Without, the band goes into the controller's
    /// frames `planes` names, starting at a row that is a multiple of 8.
    pub fn write_rows(
        &mut self,
        top: u16,
        rows: FrameView<'_>,
        planes: Planes,
    ) -> Result<(), Error> {
        let (width, screen_height) = self.size();
        let kept = self.kept();
        if kept && !planes.new {
            return Ok(());
        }
        if top >= screen_height
            || rows.width != width
            || rows.height == 0
            || (!kept && !top.is_multiple_of(8))
        {
            return Err(Error::Rows);
        }
        let height = rows.height.min(screen_height - top);
        let light = bands::light(&rows);
        if !kept {
            return self.write_band(top, &rows, height, planes, &light);
        }
        for y in 0..height {
            for x in 0..width {
                let place = self.place(x, top + y);
                put(&mut self.frame, place, light(x, y));
            }
        }
        let corners = [(0, top), (width - 1, top)]
            .into_iter()
            .chain([(0, top + height - 1), (width - 1, top + height - 1)]);
        let area = corners
            .map(|(x, y)| {
                let (at, _) = self.place(x, y);
                Area {
                    first_byte: at % LINE,
                    last_byte: at % LINE,
                    first_line: at / LINE,
                    last_line: at / LINE,
                }
            })
            .reduce(Area::join);
        self.dirty = match (self.dirty, area) {
            (Some(dirty), Some(area)) => Some(dirty.join(area)),
            (dirty, area) => dirty.or(area),
        };
        Ok(())
    }

    /// Writes a band into the controller's frames, through the window of
    /// its memory the band covers (rows past the band's end white).
    fn write_band(
        &mut self,
        top: u16,
        rows: &FrameView<'_>,
        height: u16,
        planes: Planes,
        light: &dyn Fn(u16, u16) -> bool,
    ) -> Result<(), Error> {
        let s = self.sequences();
        let mut window = Window::of(self.rotation, top, height);
        // The window's lines on a controller whose lines run the other way.
        let first = if s.lines_back {
            window.rows_back = true;
            s.first_line + PANEL_HEIGHT - window.y - window.height
        } else {
            s.first_line + window.y
        };
        let area = partial_window(window.x, window.width, first, window.height);
        for (plane, write) in [(planes.new, cmd::NEW), (planes.shown, cmd::OLD)] {
            if !plane {
                continue;
            }
            self.command(cmd::PARTIAL_IN, &[])?;
            self.command(cmd::PARTIAL_WINDOW, &area)?;
            self.wait_ready()?;
            self.command(write, &[])?;
            let Self {
                spi, dc, buffer, ..
            } = self;
            bands::send(&window, rows, height, light, true, buffer, &mut |bytes| {
                dc.set_high().map_err(|_| Error::Pin)?;
                spi.write(bytes).map_err(|_| Error::Spi)
            })?;
            self.command(cmd::PARTIAL_OUT, &[])?;
        }
        Ok(())
    }

    /// The byte and bit of the turned screen's pixel `(x, y)` in a plane.
    fn place(&self, x: u16, y: u16) -> (usize, u8) {
        let (sx, sy) = upright(self.rotation, x, y);
        let (column, line) = panel_place(sx, sy);
        let at = usize::from(line) * LINE + usize::from(column / 8);
        (at, 0x80 >> (column % 8))
    }

    /// Writes screen rows in four grays (2 bits a pixel, 0 black, 3
    /// white; `top` and `rows` as for [`Display::write_rows`]), shown with
    /// [`Display::refresh_gray`]. The frame gets them in black and white,
    /// only white white. Where grays cannot be shown, as by
    /// [`Display::write_rows`] into both frames.
    pub fn write_gray_rows(&mut self, top: u16, rows: FrameView<'_>) -> Result<(), Error> {
        let (width, screen_height) = self.size();
        if top >= screen_height
            || rows.width != width
            || rows.height == 0
            || rows.format != PixelFormat::Gray2
        {
            return Err(Error::Rows);
        }
        if !self.grays() {
            return self.write_rows(top, rows, Planes::BOTH);
        }
        if self.old.is_empty() {
            self.old = vec![0; self.frame.len()];
            self.new = vec![0; self.frame.len()];
        }
        // The frame changes unseen: the controller gets it whole.
        self.synced = false;
        let height = rows.height.min(screen_height - top);
        for y in 0..height {
            for x in 0..width {
                let at = usize::from(y) * rows.stride + usize::from(x / 4);
                let byte = rows.data.get(at).copied().unwrap_or(0xFF);
                let level = (byte >> (6 - 2 * (x % 4))) & 0b11;
                let place = self.place(x, top + y);
                put(&mut self.frame, place, level == 3);
                put(&mut self.old, place, level != 0);
                put(&mut self.new, place, level & 1 != 0);
            }
        }
        Ok(())
    }

    /// Whether four grays can be shown: with the frame kept and the
    /// controller's waveforms for them known.
    fn grays(&self) -> bool {
        self.kept() && self.sequences().gray.is_some()
    }

    /// Shows what [`Display::write_gray_rows`] wrote: the frame in black
    /// and white with a fast refresh, then the grays lightened from it
    /// (about a second more). The next fast refresh drives every pixel.
    /// Where grays cannot be shown, a half refresh.
    pub fn refresh_gray(&mut self) -> Result<(), Error> {
        self.gray_from(Refresh::Fast)
    }

    /// As [`Display::refresh_gray`], from a half refresh in place of the
    /// fast one: every pixel driven, the ghosts of what was shown gone
    /// (about a second and a half more). For a screen that cleans, such as
    /// the sleep screen.
    pub fn refresh_gray_clean(&mut self) -> Result<(), Error> {
        self.gray_from(Refresh::Half)
    }

    /// Shows the grays after the black and white frame has been shown by
    /// `first`, with the panel's own waveform (not an experiment's, which
    /// leaves ghosts that the grays would be put over).
    fn gray_from(&mut self, first: Refresh) -> Result<(), Error> {
        let Some(luts) = self.sequences().gray.filter(|_| self.grays()) else {
            return self.refresh(Refresh::Half, false);
        };
        if self.old.is_empty() {
            return self.refresh(Refresh::Half, false);
        }
        let experiment = self.experiment.take();
        let first = self.refresh(first, false);
        self.experiment = experiment;
        first?;
        self.send(cmd::OLD, Plane::Old, false)?;
        self.send(cmd::NEW, Plane::New, false)?;
        self.command(cmd::PANEL_SETTING, &UC8179.panel)?;
        self.load_luts(luts)?;
        self.command(cmd::VCOM_DATA_INTERVAL, UC8179.interval_full)?;
        self.power_on()?;
        self.command(cmd::REFRESH, &[])?;
        let done = self.wait_ready();
        self.idle_interval()?;
        if let Err(e) = done {
            self.old_valid = false;
            return Err(e);
        }
        // Both memories the frame in black and white, the grays black:
        // what the next fast refresh starts from.
        self.send(cmd::OLD, Plane::Frame, false)?;
        self.send(cmd::NEW, Plane::Frame, false)?;
        self.old_valid = true;
        self.synced = true;
        self.dirty = None;
        self.gray_shown = true;
        Ok(())
    }

    /// Shows the frame; `power_off` turns the panel's power off after it.
    pub fn refresh(&mut self, refresh: Refresh, power_off: bool) -> Result<(), Error> {
        self.start_refresh(refresh, power_off)?;
        self.wait()
    }

    /// Starts showing the frame as [`Display::refresh`] does, without
    /// waiting for the panel: [`Display::wait`] before anything else.
    pub fn start_refresh(&mut self, refresh: Refresh, power_off: bool) -> Result<(), Error> {
        self.wait()?;
        let s = self.sequences();
        // A fast refresh compares with the frame shown: without it, every
        // pixel is driven; after grays, as a half refresh.
        let fast = refresh == Refresh::Fast && self.old_valid && !self.gray_shown;
        let refresh = if refresh == Refresh::Fast && self.gray_shown {
            Refresh::Half
        } else {
            refresh
        };
        self.gray_shown = false;
        // Kept, the frame goes whole into the new frame, and into the one
        // shown inverted for a half refresh (every pixel changes), white
        // for a full one. Without, the bands are there; the frame shown
        // is made white for a full refresh.
        let kept = self.kept();
        let dirty = self.dirty.take();
        let synced = core::mem::replace(&mut self.synced, false);
        // Only what changed goes over, where both memories hold the
        // frame shown (and the area is in the lines in order the UC8179
        // takes).
        let sent = (fast && synced && self.transfer != Transfer::Whole && !s.lines_back && kept)
            .then_some(dirty);
        match sent {
            Some(Some(area)) => self.send_area(cmd::NEW, area)?,
            Some(None) => {}
            None if kept => self.send(cmd::NEW, Plane::Frame, false)?,
            None => {}
        }
        match refresh {
            _ if fast => {}
            Refresh::Half if kept => self.send(cmd::OLD, Plane::Frame, true)?,
            Refresh::Half => {}
            _ => self.fill(cmd::OLD, 0xFF)?,
        }
        let trial = self
            .experiment
            .filter(|_| fast && self.chip == Chip::Uc8179);
        let interval = if fast {
            s.interval_fast
        } else {
            s.interval_full
        };
        self.command(cmd::VCOM_DATA_INTERVAL, interval)?;
        self.command(cmd::CASCADE, &[0x02])?;
        let temperature = match trial.and_then(|trial| trial.own_at) {
            Some(own) if fast => own as u8,
            _ if fast => 0x5A,
            _ => 0x1E,
        };
        self.command(cmd::TEMPERATURE, &[temperature])?;
        if !s.panel_after_power_on {
            match trial {
                // Its own waveforms, from the registers (as the grays).
                Some(trial) if trial.own_at.is_none() => self.load_trial(&trial)?,
                _ => self.command(cmd::PANEL_SETTING, &s.panel_otp)?,
            }
        }
        if fast {
            self.command(cmd::POWER_OFF_SEQUENCE, &[0x20])?;
            self.command(cmd::GATE_SCAN, &[0x02])?;
        }
        self.power_on()?;
        if fast {
            self.command(cmd::PARTIAL_IN, &[])?;
            // The panel's lines whole: without the frame kept, the window
            // of the last band would be left.
            if s.fast_in_window || !kept || sent.is_some() {
                let window = partial_window(0, PANEL_WIDTH, s.first_line, PANEL_HEIGHT);
                self.command(cmd::PARTIAL_WINDOW, &window)?;
                self.wait_ready()?;
            }
        }
        if s.panel_after_power_on {
            self.command(cmd::PANEL_SETTING, &s.panel_otp)?;
        }
        self.command(cmd::REFRESH, &[])?;
        self.pending = Some(Pending {
            sent,
            partial: fast,
            power_off,
            restore_rate: trial
                .is_some_and(|trial| trial.own_at.is_none() && trial.rate != OWN_RATE),
        });
        Ok(())
    }

    /// Sets the panel to take its waveforms from the registers, loads
    /// those of `trial` and its frame rate.
    fn load_trial(&mut self, trial: &Experiment) -> Result<(), Error> {
        let s = self.sequences();
        self.command(cmd::PANEL_SETTING, &s.panel)?;
        if let Some(power) = self.trial_power {
            // The voltages of the OTP's own waveform, not the reset's.
            self.command(
                cmd::POWER_SETTING,
                &[0x07, 0x10 | power.gate, power.vdh, power.vdl, power.vdhr],
            )?;
            self.command(cmd::VCOM_DC, &[power.vcom])?;
        }
        self.load_luts(
            &trial
                .drive
                .uc8179_luts(trial.swapped, trial.repeat, trial.hold),
        )?;
        let rate = FRAME_RATES[trial.rate.min(FRAME_RATES.len() - 1)].0;
        if trial.rate != OWN_RATE {
            self.command(cmd::PLL, &[rate])?;
        }
        Ok(())
    }

    /// Loads waveforms into their registers.
    fn load_luts(&mut self, luts: &[[u8; 13]; 5]) -> Result<(), Error> {
        let mut lut = [0u8; LUT_LEN];
        for row in luts {
            lut[..12].copy_from_slice(&row[1..]);
            self.command(row[0], &lut)?;
        }
        Ok(())
    }

    /// Turns the panel's power on, if it is not.
    fn power_on(&mut self) -> Result<(), Error> {
        if !self.powered {
            self.command(cmd::POWER_ON, &[])?;
            self.wait_ready()?;
            self.powered = true;
        }
        Ok(())
    }

    /// The border and data interval at rest, where the controller has one.
    fn idle_interval(&mut self) -> Result<(), Error> {
        match self.sequences().interval_idle {
            Some(interval) => self.command(cmd::VCOM_DATA_INTERVAL, interval),
            None => Ok(()),
        }
    }

    /// Turns the panel's power off once nothing follows for a while: the
    /// picture stays, and the next refresh powers it up again.
    pub fn power_down(&mut self) -> Result<(), Error> {
        self.wait()?;
        if self.powered {
            self.command(cmd::POWER_OFF, &[])?;
            self.wait_ready()?;
            self.powered = false;
        }
        Ok(())
    }

    /// Whether the panel's power is on.
    pub fn powered(&self) -> bool {
        self.powered
    }

    /// Puts the controller into deep sleep, the picture staying;
    /// [`Display::init`] wakes it again.
    pub fn sleep(&mut self) -> Result<(), Error> {
        self.wait()?;
        if self.powered {
            self.command(cmd::POWER_OFF, &[])?;
            self.wait_ready()?;
            self.powered = false;
        }
        self.old_valid = false;
        self.synced = false;
        self.gray_shown = false;
        self.command(cmd::DEEP_SLEEP, &[0xA5])
    }

    /// Whether a refresh started by [`Display::start_refresh`] still goes
    /// on, without waiting (its busy pin low): [`Display::wait`] once it
    /// is over, which ends it.
    pub fn refreshing(&mut self) -> Result<bool, Error> {
        if self.pending.is_none() {
            return Ok(false);
        }
        self.busy.is_low().map_err(|_| Error::Pin)
    }

    /// Waits for a refresh started by [`Display::start_refresh`] to end,
    /// then keeps the frame as the one shown (the controller copies it
    /// too, but the frame kept goes again, as the stock firmware sends
    /// it).
    pub fn wait(&mut self) -> Result<(), Error> {
        let Some(pending) = self.pending.take() else {
            return Ok(());
        };
        let done = self.wait_ready();
        if pending.partial {
            self.command(cmd::PARTIAL_OUT, &[])?;
        }
        if pending.restore_rate {
            self.command(cmd::PLL, &[FRAME_RATES[OWN_RATE].0])?;
        }
        self.idle_interval()?;
        if let Err(e) = done {
            self.old_valid = false;
            return Err(e);
        }
        match pending.sent {
            // The controller's copy of *new* is *old* now.
            Some(_) if self.transfer == Transfer::Copied => {}
            Some(Some(area)) => self.send_area(cmd::OLD, area)?,
            Some(None) => {}
            None if self.kept() => self.send(cmd::OLD, Plane::Frame, false)?,
            None => {}
        }
        self.old_valid = true;
        self.synced = self.kept();
        if pending.power_off {
            self.command(cmd::POWER_OFF, &[])?;
            self.wait_ready()?;
            self.powered = false;
        }
        Ok(())
    }

    /// Waits while the controller is busy (its busy pin low).
    fn wait_ready(&mut self) -> Result<(), Error> {
        for _ in 0..BUSY_TIMEOUT_MS {
            if self.busy.is_high().map_err(|_| Error::Pin)? {
                return Ok(());
            }
            self.delay.delay_ms(1);
            self.busy_ms += 1;
        }
        Err(Error::Timeout)
    }

    /// Sends `plane` into memory `command`, inverted if asked, its lines
    /// where the panel shows them (on a UC8179 last first, then the lines
    /// past the panel white; on a UC8279 those first, then in order).
    fn send(&mut self, command: u8, plane: Plane, invert: bool) -> Result<(), Error> {
        self.command(command, &[])?;
        self.dc.set_high().map_err(|_| Error::Pin)?;
        let s = self.sequences();
        let white = [0xFF; LINE];
        let blank = ADDRESSED_LINES - PANEL_HEIGHT;
        if s.lines_back {
            for _ in 0..blank {
                self.spi.write(&white).map_err(|_| Error::Spi)?;
            }
        }
        let plane = match plane {
            Plane::Frame => &self.frame,
            Plane::Old => &self.old,
            Plane::New => &self.new,
        };
        let mut line = [0u8; LINE];
        for i in 0..usize::from(PANEL_HEIGHT) {
            let index = if s.lines_back {
                i
            } else {
                usize::from(PANEL_HEIGHT) - 1 - i
            };
            let from = &plane[index * LINE..(index + 1) * LINE];
            if invert {
                for (to, byte) in line.iter_mut().zip(from) {
                    *to = !byte;
                }
                self.spi.write(&line).map_err(|_| Error::Spi)?;
            } else {
                self.spi.write(from).map_err(|_| Error::Spi)?;
            }
        }
        if !s.lines_back {
            for _ in 0..blank {
                self.spi.write(&white).map_err(|_| Error::Spi)?;
            }
        }
        Ok(())
    }

    /// Sends the lines and bytes of `area` of the frame into memory
    /// `command`, through a window of the controller's memory (the
    /// UC8179's lines last first, as [`Display::send`] has them).
    fn send_area(&mut self, command: u8, area: Area) -> Result<(), Error> {
        let width = (area.last_byte - area.first_byte + 1) * 8;
        let lines = area.last_line - area.first_line + 1;
        let window = partial_window(
            (area.first_byte * 8) as u16,
            width as u16,
            PANEL_HEIGHT - 1 - area.last_line as u16,
            lines as u16,
        );
        self.command(cmd::PARTIAL_IN, &[])?;
        self.command(cmd::PARTIAL_WINDOW, &window)?;
        self.wait_ready()?;
        self.command(command, &[])?;
        self.dc.set_high().map_err(|_| Error::Pin)?;
        let Self {
            spi, frame, buffer, ..
        } = self;
        buffer.clear();
        for line in (area.first_line..=area.last_line).rev() {
            let at = line * LINE;
            buffer.extend_from_slice(&frame[at + area.first_byte..=at + area.last_byte]);
            if buffer.len() >= bands::CHUNK {
                spi.write(buffer).map_err(|_| Error::Spi)?;
                buffer.clear();
            }
        }
        if !buffer.is_empty() {
            spi.write(buffer).map_err(|_| Error::Spi)?;
            buffer.clear();
        }
        self.command(cmd::PARTIAL_OUT, &[])
    }

    /// Fills memory `command` with `value`.
    fn fill(&mut self, command: u8, value: u8) -> Result<(), Error> {
        self.command(command, &[])?;
        self.dc.set_high().map_err(|_| Error::Pin)?;
        let line = [value; LINE];
        for _ in 0..ADDRESSED_LINES {
            self.spi.write(&line).map_err(|_| Error::Spi)?;
        }
        Ok(())
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

/// The window of the controller's memory (`PTL`) of columns `x..x +
/// width` (multiples of 8) and lines `y..y + height`, the lines outside
/// scanned too.
fn partial_window(x: u16, width: u16, y: u16, height: u16) -> [u8; 9] {
    let (last_x, last_y) = (x + width - 1, y + height - 1);
    [
        (x >> 8) as u8,
        (x & 0xF8) as u8,
        (last_x >> 8) as u8,
        (last_x & 0xF8) as u8 | 0x07,
        (y >> 8) as u8,
        y as u8,
        (last_y >> 8) as u8,
        last_y as u8,
        0x01,
    ]
}

/// Sets the bit at `place` (from [`Display::place`]) of `plane`, or
/// clears it.
fn put(plane: &mut [u8], (at, bit): (usize, u8), on: bool) {
    if let Some(byte) = plane.get_mut(at) {
        if on {
            *byte |= bit;
        } else {
            *byte &= !bit;
        }
    }
}

/// The upright screen's pixel of the turned screen's `(x, y)`, with the
/// frame kept (the X4 Pro's turns).
fn upright(rotation: Rotation, x: u16, y: u16) -> (u16, u16) {
    match rotation {
        Rotation::Deg0 => (x, y),
        // Turned clockwise: the turned screen's top is the upright one's
        // right side.
        Rotation::Deg90 => (WIDTH - 1 - y, x),
        Rotation::Deg180 => (WIDTH - 1 - x, HEIGHT - 1 - y),
        Rotation::Deg270 => (y, HEIGHT - 1 - x),
        _ => (x, y),
    }
}

/// The turned screen's pixel at the upright screen's `(x, y)`: where a
/// touch on the upright screen lands on the screen PaperPaw draws.
pub fn turned(rotation: Rotation, x: u16, y: u16) -> (u16, u16) {
    match rotation {
        Rotation::Deg0 => (x, y),
        Rotation::Deg90 => (y, WIDTH - 1 - x),
        Rotation::Deg180 => (WIDTH - 1 - x, HEIGHT - 1 - y),
        Rotation::Deg270 => (HEIGHT - 1 - y, x),
        _ => (x, y),
    }
}

/// Where the upright screen's pixel `(x, y)` lies in the frame: its
/// column (bit) and line (the screen's left column the panel's last line).
fn panel_place(x: u16, y: u16) -> (u16, u16) {
    (y, WIDTH - 1 - x)
}

#[cfg(test)]
mod tests;
