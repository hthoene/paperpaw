// The controller's set-up sequence and the waveform tables below (`init`,
// the `lut` module) derive from the FreeInk SDK's UC8253 driver for the
// Xteink X3 (`Uc8253X3Driver.cpp`, `Uc8253X3Luts.h`), which derives them
// from the OpenX4 E-Paper Community SDK. Both are under the MIT licence:
//
// Copyright (c) 2026 FreeInk
// Copyright (c) 2025 Open X4 E-Paper Contributors
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the
// "Software"), to deal in the Software without restriction, including
// without limitation the rights to use, copy, modify, merge, publish,
// distribute, sublicense, and/or sell copies of the Software, and to permit
// persons to whom the Software is furnished to do so, subject to the
// following conditions:
//
// The above copyright notice and this permission notice shall be included
// in all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
// MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN
// NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM,
// DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR
// OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE
// USE OR OTHER DEALINGS IN THE SOFTWARE.

//! The Xteink X3's display: a 792 × 528 panel driven by an UltraChip
//! UC8253 over SPI, with the same calls as [`uc8179`](crate::uc8179), so
//! the firmware drives it alike.
//!
//! The X3 has no memory for a frame of its own: the controller keeps it,
//! in two memories, the new frame (`DTM2`) and the one shown (`DTM1`),
//! which a fast refresh compares with. [`Display::write_rows`] writes
//! PaperPaw's bands straight into them, each through a window of the
//! controller's memory (`PTL`, entered with `PTIN` and left with
//! `PTOUT`), into the memories [`Planes`] names. After every refresh the
//! firmware writes the frame shown into both again, so the one shown is
//! there for the next fast refresh.
//!
//! The controller's memory is 528 lines (gates) of 792 columns (sources),
//! a byte 8 pixels of a line, the leftmost in the highest bit, set for
//! white: 52,272 bytes. PaperPaw draws the screen upright (528 × 792): the
//! panel turned clockwise, as the X4's, the screen's column `x` the
//! controller's line `x`, its row `y` the controller's column `y`.
//!
//! The panel's own memory of waveforms is blank, so every refresh loads
//! its waveforms into the controller's registers first (`0x20` to `0x24`:
//! VCOM, then for each pixel by its bit in the shown and the new frame,
//! white to white, black to white, white to black, black to black).
//! These are the FreeInk SDK's: full (its stock waveform), half (driving
//! every pixel to its new colour) and fast (only where the two frames
//! differ). The SDK drives a full refresh from a white frame shown; here
//! the full waveform is loaded for both of a new colour's registers, so
//! the frame shown does not matter and stays as it is. After a full
//! refresh the SDK settles the panel with a gentle pass before the next
//! fast one ("the first differential after a full garbles"); here that
//! pass comes at the start of the next fast refresh, from the frame shown.
//!
//! Newer X3s have a UC8279d instead (`Display::uc8279d`): the same
//! memories, window and command set, but its module's own memory of
//! waveforms and settings is blank too, so the whole set-up comes from
//! here (`init8279d`): power, booster and a 792 × 528 window; and its
//! waveforms are its own (its `lut8279d` module: one full, one fast),
//! sent with a one-byte `CDI`. Its refreshes compare the new frame with
//! the frame shown, as a fast one does here, and the first two after
//! [`Display::init`] use the full waveform. Its facts come from the
//! FreeInk SDK (`Uc8279X3Luts.h`, `docs/xteink-x3-uc8279-support.md`,
//! MIT, recovered there from the stock firmware).
//!
//! Four grays ([`Display::write_gray_rows`], [`Display::refresh_gray`])
//! are shown in black and white. The SDK's grays on the UC8253 first show
//! the frame in black and white, then the two bits of each gray in both
//! memories: that takes the frame twice, which the X3 does not keep, and
//! a waveform that prepares the panel for them taken from the stock
//! firmware, which is not used here.

use alloc::vec::Vec;

use embedded_hal::delay::DelayNs;
use embedded_hal::digital::{InputPin, OutputPin};
use embedded_hal::spi::SpiDevice;
use paperpaw_platform::{FrameView, PixelFormat, Rotation};

use crate::bands::{self, CHUNK};
use crate::display::BUSY_TIMEOUT_MS;
pub use crate::display::{Error, Planes, Refresh};

/// The panel, as the controller addresses it: columns (sources) and lines
/// (gates).
pub const PANEL_WIDTH: u16 = 792;
pub const PANEL_HEIGHT: u16 = 528;
/// The screen PaperPaw draws upright (see [`Display::size`] for it
/// turned).
pub const WIDTH: u16 = PANEL_HEIGHT;
pub const HEIGHT: u16 = PANEL_WIDTH;

/// Bytes of a line.
const LINE: usize = PANEL_WIDTH as usize / 8;
/// Bytes of a frame.
pub const PLANE_BYTES: usize = LINE * PANEL_HEIGHT as usize;

/// Longest wait for the controller to start being busy after a command
/// (the SDK's): a command done before it is seen leaves no low edge.
const BUSY_EDGE_MS: u32 = 1000;
/// Longest wait for a refresh to start, before the CPU goes back to other
/// work (the SDK's).
const REFRESH_EDGE_MS: u32 = 50;
/// The rest after a refresh that is not fast, before the controller is
/// told more (the SDK's).
const SETTLE_MS: u32 = 200;

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
    pub const DATA_STOP: u8 = 0x11;
    pub const REFRESH: u8 = 0x12;
    pub const NEW: u8 = 0x13;
    pub const LUT_VCOM: u8 = 0x20;
    pub const PLL: u8 = 0x30;
    pub const VCOM_DATA_INTERVAL: u8 = 0x50;
    pub const RESOLUTION: u8 = 0x61;
    pub const GATE_SOURCE_START: u8 = 0x65;
    pub const VCOM_DC: u8 = 0x82;
    pub const PARTIAL_WINDOW: u8 = 0x90;
    pub const PARTIAL_IN: u8 = 0x91;
    pub const PARTIAL_OUT: u8 = 0x92;
    pub const LV_SELECTION: u8 = 0xE1;
}

/// Bytes of a waveform register; the tables give the first 12, the rest
/// are 0.
const LUT_LEN: usize = 42;

/// The first bytes of a waveform register.
type Lut = [u8; 12];

/// The FreeInk SDK's waveforms for the X3 (`Uc8253X3Luts.h`), by the
/// SDK's names.
mod lut {
    use super::Lut;

    /// `_normal` (lines 17 to 36): the SDK's pass after the first full
    /// refresh.
    pub const VCOM_NORMAL: Lut = [
        0x00, 0x06, 0x01, 0x06, 0x06, 0x01, 0x00, 0x04, 0x01, 0x01, 0x00, 0x01,
    ];
    pub const WW_NORMAL: Lut = [
        0x20, 0x06, 0x01, 0x06, 0x06, 0x01, 0x00, 0x04, 0x01, 0x01, 0x00, 0x01,
    ];
    pub const WB_NORMAL: Lut = [
        0x55, 0x06, 0x01, 0x06, 0x06, 0x01, 0x50, 0x04, 0x01, 0x01, 0x00, 0x01,
    ];

    /// `_half` (lines 39 to 58): every pixel driven to its new colour
    /// (`ww` and `bw` alike, `wb` and `bb` alike).
    pub const VCOM_HALF: Lut = [
        0x00, 0x06, 0x01, 0x06, 0x06, 0x01, 0x00, 0x04, 0x01, 0x01, 0x00, 0x01,
    ];
    pub const WW_HALF: Lut = [
        0xAA, 0x06, 0x01, 0x06, 0x06, 0x01, 0xA0, 0x04, 0x01, 0x01, 0x00, 0x01,
    ];
    pub const BW_HALF: Lut = WW_HALF;
    pub const WB_HALF: Lut = [
        0x55, 0x06, 0x01, 0x06, 0x06, 0x01, 0x50, 0x04, 0x01, 0x01, 0x00, 0x01,
    ];
    pub const BB_HALF: Lut = WB_HALF;

    /// `_fast` (lines 64 to 83): only where the two frames differ, the
    /// rest gently held.
    pub const VCOM_FAST: Lut = [
        0x00, 0x04, 0x02, 0x04, 0x04, 0x01, 0x00, 0x04, 0x01, 0x00, 0x00, 0x01,
    ];
    pub const WW_FAST: Lut = [
        0x20, 0x04, 0x02, 0x04, 0x04, 0x01, 0x00, 0x04, 0x01, 0x00, 0x00, 0x01,
    ];
    pub const BW_FAST: Lut = [
        0xAA, 0x04, 0x02, 0x04, 0x04, 0x01, 0x80, 0x04, 0x01, 0x00, 0x00, 0x01,
    ];
    pub const WB_FAST: Lut = [
        0x55, 0x04, 0x02, 0x04, 0x04, 0x01, 0x40, 0x04, 0x01, 0x00, 0x00, 0x01,
    ];
    pub const BB_FAST: Lut = [
        0x10, 0x04, 0x02, 0x04, 0x04, 0x01, 0x00, 0x04, 0x01, 0x00, 0x00, 0x01,
    ];

    /// `_full` (lines 86 to 105), from a white frame shown: white to
    /// white and white to black.
    pub const VCOM_FULL: Lut = [
        0x00, 0x18, 0x04, 0x0E, 0x0A, 0x01, 0x00, 0x0A, 0x00, 0x00, 0x00, 0x01,
    ];
    pub const WW_FULL: Lut = [
        0x4A, 0x18, 0x04, 0x0E, 0x0A, 0x01, 0x00, 0x0A, 0x00, 0x00, 0x00, 0x01,
    ];
    pub const WB_FULL: Lut = [
        0x04, 0x18, 0x04, 0x0E, 0x0A, 0x01, 0x40, 0x0A, 0x00, 0x00, 0x00, 0x01,
    ];
}

/// The UC8279d's waveforms (the FreeInk SDK's `Uc8279X3Luts.h`, `BW_GC`
/// and `BW_DU`), the first bytes of each register, the rest 0.
mod lut8279d {
    /// Full (and half): clears through the true old-to-new transition.
    pub const GC: [&[u8]; 5] = [
        &[0x01, 0x1A, 0x1A, 0x01, 0x00, 0x01, 0x01],
        &[0x01, 0x5A, 0x9A, 0x01, 0x00, 0x01, 0x01],
        &[0x01, 0x1A, 0x9A, 0x01, 0x00, 0x01, 0x01],
        &[0x01, 0x1A, 0x5A, 0x01, 0x00, 0x01, 0x01],
        &[0x01, 0x9A, 0x5A, 0x01, 0x00, 0x01, 0x01],
    ];
    /// Fast: drives only where the two frames differ.
    pub const DU: [&[u8]; 5] = [
        &[
            0x01, 0x07, 0x01, 0x06, 0x06, 0x01, 0x01, 0x01, 0x06, 0x01, 0x00, 0x00, 0x01,
        ],
        &[
            0x01, 0x07, 0x81, 0x06, 0x06, 0x01, 0x01, 0x01, 0x06, 0x01, 0x00, 0x00, 0x01,
        ],
        &[
            0x01, 0x87, 0x81, 0x86, 0x86, 0x01, 0x01, 0x01, 0x86, 0x01, 0x00, 0x00, 0x01,
        ],
        &[
            0x01, 0x47, 0x41, 0x46, 0x46, 0x01, 0x01, 0x01, 0x46, 0x01, 0x00, 0x00, 0x01,
        ],
        &[
            0x01, 0x07, 0x01, 0x06, 0x06, 0x01, 0x01, 0x01, 0x06, 0x01, 0x00, 0x00, 0x01,
        ],
    ];
    /// `CDI` of the first refresh after the controller's set-up, and of
    /// the later ones.
    pub const CDI_FIRST: u8 = 0x97;
    pub const CDI_LATER: u8 = 0xD7;
    /// Refreshes after the set-up that use the full waveform.
    pub const FULLS: u8 = 2;
    /// Its set-up after the reset (`FUN_42014ad4` of the stock firmware):
    /// command, data.
    pub const INIT: [(u8, &[u8]); 9] = [
        (0x00, &[0x3F, 0x4A]),
        (0x91, &[]),
        (
            0x90,
            &[0x00, 0x00, 0x03, 0x17, 0x00, 0x00, 0x02, 0x0F, 0x01],
        ),
        (0x03, &[0x20]),
        (0x01, &[0x43, 0x00, 0x78, 0x78, 0x17]),
        (0x82, &[0x24]),
        (0x06, &[0x25, 0x25, 0x3C]),
        (0x30, &[0x0F]),
        (0xE1, &[0x02]),
    ];
}

/// `table` with the frame counts of its phases (bytes 1 to 4 and 7 to 10 of
/// its two groups) scaled to `percent`, none that was there less than one
/// frame; what drives and what holds keep their proportions.
fn scaled(table: &[u8], percent: u8) -> [u8; LUT_LEN] {
    let mut out = [0u8; LUT_LEN];
    out[..table.len()].copy_from_slice(table);
    for group in [1..5, 7..11] {
        for byte in &mut out[group] {
            if *byte > 0 {
                let frames = (u16::from(*byte) * u16::from(percent) + 50) / 100;
                *byte = u8::try_from(frames.max(1)).unwrap_or(u8::MAX);
            }
        }
    }
    out
}

/// Waveforms for one refresh: the border and data interval (`CDI`), then
/// the registers VCOM, white to white, black to white, white to black,
/// black to black.
struct Bank {
    interval: [u8; 2],
    luts: [&'static [u8]; 5],
}

/// A full refresh: the SDK's full waveform from a white frame shown, here
/// by the new colour alone (the frame shown left as it is).
// Uncertain without a device: that the controller picks a register by the
// pixel's two bits alone, so this drives as the SDK's full refresh does.
const FULL: Bank = Bank {
    interval: [0x29, 0x07],
    luts: [
        &lut::VCOM_FULL,
        &lut::WW_FULL,
        &lut::WW_FULL,
        &lut::WB_FULL,
        &lut::WB_FULL,
    ],
};

const HALF: Bank = Bank {
    interval: [0xA9, 0x07],
    luts: [
        &lut::VCOM_HALF,
        &lut::WW_HALF,
        &lut::BW_HALF,
        &lut::WB_HALF,
        &lut::BB_HALF,
    ],
};

const FAST: Bank = Bank {
    interval: [0x29, 0x07],
    luts: [
        &lut::VCOM_FAST,
        &lut::WW_FAST,
        &lut::BW_FAST,
        &lut::WB_FAST,
        &lut::BB_FAST,
    ],
};

/// The SDK's pass after the first full refresh, from a white frame shown
/// to the frame: here by the colour shown alone, white held as white to
/// white, black as white to black.
const CONDITION: Bank = Bank {
    interval: [0xA9, 0x07],
    luts: [
        &lut::VCOM_NORMAL,
        &lut::WW_NORMAL,
        &lut::WB_NORMAL,
        &lut::WW_NORMAL,
        &lut::WB_NORMAL,
    ],
};

/// The SDK's settling after a full refresh, a fast refresh of the frame
/// to itself: here by the colour shown alone, white as white to white,
/// black as black to black.
const SETTLE: Bank = Bank {
    interval: [0x29, 0x07],
    luts: [
        &lut::VCOM_FAST,
        &lut::WW_FAST,
        &lut::BB_FAST,
        &lut::WW_FAST,
        &lut::BB_FAST,
    ],
};

/// Columns `x..x + width` (multiples of 8) of lines `y..y + height` of
/// the controller's memory, where a band goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Window {
    x: u16,
    width: u16,
    y: u16,
    height: u16,
}

impl Window {
    /// The whole panel.
    const PANEL: Self = Self {
        x: 0,
        width: PANEL_WIDTH,
        y: 0,
        height: PANEL_HEIGHT,
    };

    /// The window of screen rows `top..top + height` turned as `rotation`
    /// says.
    fn of(rotation: Rotation, top: u16, height: u16) -> Self {
        match rotation {
            // Upright, a band is columns of the controller's (whole bytes).
            Rotation::Deg0 | Rotation::Deg180 => {
                let width = height.div_ceil(8) * 8;
                let x = if rotation == Rotation::Deg180 {
                    PANEL_WIDTH - top - width
                } else {
                    top
                };
                Self {
                    x,
                    width,
                    y: 0,
                    height: PANEL_HEIGHT,
                }
            }
            // On its side, lines.
            _ => {
                let y = if rotation == Rotation::Deg90 {
                    top
                } else {
                    PANEL_HEIGHT - top - height
                };
                Self {
                    x: 0,
                    width: PANEL_WIDTH,
                    y,
                    height,
                }
            }
        }
    }

    /// The window as `PTL` takes it, the lines outside scanned too.
    fn command(&self) -> [u8; 9] {
        let (last_x, last_y) = (self.x + self.width - 1, self.y + self.height - 1);
        [
            (self.x >> 8) as u8,
            (self.x & 0xF8) as u8,
            (last_x >> 8) as u8,
            (last_x & 0xF8) as u8 | 0x07,
            (self.y >> 8) as u8,
            self.y as u8,
            (last_y >> 8) as u8,
            last_y as u8,
            0x01,
        ]
    }
}

/// What [`Display::wait`] does once the controller is done refreshing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Pending {
    refresh: Refresh,
    /// The controller was seen busy: only its end is waited for.
    started: bool,
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
    /// The controller's *old* memory holds the frame shown, for a fast
    /// refresh to compare with.
    old_valid: bool,
    /// A full refresh was last: the panel is settled before the next fast
    /// one.
    settle: bool,
    /// The first full refresh since [`Display::init`] is not settled yet:
    /// the panel gets the SDK's pass after it too.
    condition: bool,
    powered: bool,
    rotation: Rotation,
    pending: Option<Pending>,
    /// LAB ONLY (#40): the percentage the fast waveform's frame counts are
    /// scaled to (`None`: the SDK's own).
    fast_percent: Option<u8>,
    /// A band's bytes on their way.
    buffer: Vec<u8>,
    /// The controller is a UC8279d (see [`Display::uc8279d`]).
    uc8279d: bool,
    /// A UC8279d: refreshes still to come with the full waveform.
    fulls_left: u8,
    /// A UC8279d: no refresh was done since [`Display::init`] (its `CDI`).
    first_refresh: bool,
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
            old_valid: false,
            settle: false,
            condition: false,
            powered: false,
            rotation: Rotation::Deg0,
            pending: None,
            fast_percent: None,
            buffer: Vec::new(),
            uc8279d: false,
            fulls_left: 0,
            first_refresh: false,
        }
    }

    /// Drives a UC8279d (the newer X3's controller) instead of a UC8253.
    #[must_use]
    pub fn uc8279d(mut self) -> Self {
        self.uc8279d = true;
        self
    }

    /// Whether this drives a UC8279d.
    pub fn is_uc8279d(&self) -> bool {
        self.uc8279d
    }

    /// LAB ONLY (#40): shows fast refreshes with the SDK's fast waveform
    /// whose frame counts are scaled to `percent` of its own (100: as it is;
    /// 50: half the frames, faster, more ghosting; 150: longer, cleaner),
    /// or as it is where `None`. Kept to 25 to 200.
    pub fn set_fast_percent(&mut self, percent: Option<u8>) {
        self.fast_percent = percent.map(|p| p.clamp(25, 200));
    }

    /// How the screen PaperPaw draws is turned: [`Rotation::Deg90`] is
    /// the X3 turned clockwise (the upright screen's left side on top), as
    /// the X4.
    // Uncertain without a device: which way the X3's panel lies in it, so
    // whether upright and turned are the right way round and not mirrored.
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

    /// Resets and sets up the controller (the SDK's `begin`, lines 115 to
    /// 160 of `Uc8253X3Driver.cpp`) and makes both its frames white; the
    /// next refresh drives every pixel.
    pub fn init(&mut self) -> Result<(), Error> {
        // The SDK's reset, with 50 ms more for the X3 to settle.
        for (high, ms) in [(true, 10), (false, 10), (true, 60)] {
            if high {
                self.reset.set_high()
            } else {
                self.reset.set_low()
            }
            .map_err(|_| Error::Pin)?;
            self.delay.delay_ms(ms);
        }
        if self.uc8279d {
            return self.init8279d();
        }
        self.wait_idle()?;
        self.command(cmd::PANEL_SETTING, &[0x3F, 0x0A])?;
        // 792 sources, 600 gates, as the SDK sets it.
        // Uncertain without a device: the panel shows 528 of the 600 lines
        // addressed; as in the SDK, the first, the frame written from line 0.
        self.command(cmd::RESOLUTION, &[0x03, 0x18, 0x02, 0x58])?;
        self.command(cmd::GATE_SOURCE_START, &[0x00, 0x00, 0x00, 0x00])?;
        self.command(cmd::POWER_OFF_SEQUENCE, &[0x20])?;
        self.command(cmd::POWER_SETTING, &[0x07, 0x17, 0x3F, 0x3F, 0x17])?;
        self.command(cmd::VCOM_DC, &[0x24])?;
        self.command(cmd::BOOSTER_SOFT_START, &[0x25, 0x25, 0x3C, 0x37])?;
        self.command(cmd::PLL, &[0x09])?;
        self.command(cmd::LV_SELECTION, &[0x02])?;
        // The controller does not clear its memories itself.
        self.fill(cmd::OLD, 0xFF)?;
        self.command(cmd::DATA_STOP, &[])?;
        self.fill(cmd::NEW, 0xFF)?;
        self.command(cmd::DATA_STOP, &[])?;
        self.old_valid = false;
        self.settle = false;
        self.condition = true;
        self.powered = false;
        self.pending = None;
        Ok(())
    }

    /// [`Display::init`] of a UC8279d: its blank module needs the whole
    /// script (the stock firmware's), then both memories white inside its
    /// window.
    fn init8279d(&mut self) -> Result<(), Error> {
        // Idle after a reset within 300 ms as the stock firmware's probe
        // waits; a controller that does not say so is told on regardless.
        for _ in 0..300 {
            if self.busy.is_high().map_err(|_| Error::Pin)? {
                break;
            }
            self.delay.delay_ms(1);
        }
        for (command, data) in lut8279d::INIT {
            self.command(command, data)?;
        }
        self.fill(cmd::OLD, 0xFF)?;
        self.command(cmd::DATA_STOP, &[])?;
        self.fill(cmd::NEW, 0xFF)?;
        self.command(cmd::DATA_STOP, &[])?;
        self.command(cmd::PARTIAL_OUT, &[])?;
        self.old_valid = false;
        self.settle = false;
        self.condition = false;
        self.powered = false;
        self.pending = None;
        self.fulls_left = lut8279d::FULLS;
        self.first_refresh = true;
        Ok(())
    }

    /// Writes screen rows `top..top + rows.height` (a band from
    /// `Kernel::render`, the screen's width as turned, starting at a row
    /// that is a multiple of 8) into the controller's frames `planes`
    /// names. Pixels lighter than middle gray are white.
    // Uncertain without a device: that a window of some of the columns
    // (upright, each band) takes the bytes as the SDK's windows of whole
    // lines do.
    pub fn write_rows(
        &mut self,
        top: u16,
        rows: FrameView<'_>,
        planes: Planes,
    ) -> Result<(), Error> {
        let (width, screen_height) = self.size();
        if top >= screen_height || rows.width != width || rows.height == 0 || !top.is_multiple_of(8)
        {
            return Err(Error::Rows);
        }
        let height = rows.height.min(screen_height - top);
        let window = Window::of(self.rotation, top, height);
        let area = window.command();
        let rotation = self.rotation;
        let light = bands::light(&rows);
        for (plane, write) in [(planes.new, cmd::NEW), (planes.shown, cmd::OLD)] {
            if !plane {
                continue;
            }
            self.command(cmd::PARTIAL_IN, &[])?;
            self.command(cmd::PARTIAL_WINDOW, &area)?;
            self.command(write, &[])?;
            let Self {
                spi, dc, buffer, ..
            } = self;
            send_band(
                rotation,
                &window,
                &rows,
                height,
                &light,
                buffer,
                &mut |bytes| {
                    dc.set_high().map_err(|_| Error::Pin)?;
                    spi.write(bytes).map_err(|_| Error::Spi)
                },
            )?;
            self.command(cmd::PARTIAL_OUT, &[])?;
        }
        Ok(())
    }

    /// Writes screen rows in four grays (2 bits a pixel, 0 black, 3
    /// white; `top` and `rows` as for [`Display::write_rows`]): shown in
    /// black and white (see the module's notes), as by
    /// [`Display::write_rows`] into both frames, dark gray black, light
    /// gray white.
    pub fn write_gray_rows(&mut self, top: u16, rows: FrameView<'_>) -> Result<(), Error> {
        if rows.format != PixelFormat::Gray2 {
            return Err(Error::Rows);
        }
        self.write_rows(top, rows, Planes::BOTH)
    }

    /// Shows what [`Display::write_gray_rows`] wrote, with a half refresh
    /// (in black and white).
    pub fn refresh_gray(&mut self) -> Result<(), Error> {
        self.refresh(Refresh::Half, false)
    }

    /// Shows the frame; `power_off` turns the panel's power off after it.
    pub fn refresh(&mut self, refresh: Refresh, power_off: bool) -> Result<(), Error> {
        self.start_refresh(refresh, power_off)?;
        self.wait()
    }

    /// Starts showing the frame as [`Display::refresh`] does, without
    /// waiting for the panel: [`Display::wait`] before anything else. A
    /// fast refresh after a full one settles the panel first (a fast
    /// refresh's time more, waited for here).
    pub fn start_refresh(&mut self, refresh: Refresh, power_off: bool) -> Result<(), Error> {
        self.wait()?;
        if self.uc8279d {
            return self.start_refresh8279d(refresh, power_off);
        }
        // As in the SDK: every pixel driven without the frame shown, and
        // as a half refresh after the panel's power was off (a waking panel
        // needs the stronger waveform; a full refresh stays full).
        let refresh = if !self.old_valid {
            Refresh::Full
        } else if refresh == Refresh::Fast && !self.powered && !power_off {
            Refresh::Half
        } else {
            refresh
        };
        if refresh == Refresh::Fast && self.settle {
            if self.condition {
                self.pass(&CONDITION)?;
            }
            self.pass(&SETTLE)?;
            self.settle = false;
            self.condition = false;
        }
        let bank = match refresh {
            Refresh::Full => &FULL,
            Refresh::Half => &HALF,
            Refresh::Fast => &FAST,
        };
        match self.fast_percent {
            Some(percent) if refresh == Refresh::Fast => self.load_scaled(bank, percent)?,
            _ => self.load(bank)?,
        }
        self.power_on()?;
        self.window_in()?;
        self.command(cmd::REFRESH, &[])?;
        // Seen busy, the CPU goes back to other work; if not yet,
        // [`Display::wait`] waits for both of its phases.
        let started = self.busy_starts(REFRESH_EDGE_MS)?;
        self.pending = Some(Pending {
            refresh,
            started,
            power_off,
        });
        Ok(())
    }

    /// [`Display::start_refresh`] of a UC8279d: the full waveform for a
    /// full or half refresh, for the first ones after the set-up and for
    /// one without the frame shown; the fast waveform else.
    fn start_refresh8279d(&mut self, refresh: Refresh, power_off: bool) -> Result<(), Error> {
        let refresh = if refresh == Refresh::Fast && (!self.old_valid || self.fulls_left > 0) {
            Refresh::Half
        } else {
            refresh
        };
        let bank = if refresh == Refresh::Fast {
            &lut8279d::DU
        } else {
            &lut8279d::GC
        };
        let cdi = if self.first_refresh {
            lut8279d::CDI_FIRST
        } else {
            lut8279d::CDI_LATER
        };
        self.command(cmd::VCOM_DATA_INTERVAL, &[cdi])?;
        self.load_luts(bank, 100)?;
        self.power_on()?;
        self.window_in()?;
        self.command(cmd::REFRESH, &[])?;
        let started = self.busy_starts(REFRESH_EDGE_MS)?;
        self.pending = Some(Pending {
            refresh,
            started,
            power_off,
        });
        Ok(())
    }

    /// Whether a refresh started by [`Display::start_refresh`] still goes
    /// on, without waiting (its busy pin low, or not seen busy yet):
    /// [`Display::wait`] once it is over, which ends it.
    pub fn refreshing(&mut self) -> Result<bool, Error> {
        let Some(pending) = self.pending.as_mut() else {
            return Ok(false);
        };
        let busy = self.busy.is_low().map_err(|_| Error::Pin)?;
        pending.started |= busy;
        Ok(busy || !pending.started)
    }

    /// Waits for a refresh started by [`Display::start_refresh`] to end.
    pub fn wait(&mut self) -> Result<(), Error> {
        self.wait_with(|| {})
    }

    /// As [`Self::wait`], calling `meanwhile` each round of the wait (a
    /// millisecond and what it takes), e.g. to serve the radio.
    pub fn wait_with(&mut self, mut meanwhile: impl FnMut()) -> Result<(), Error> {
        let Some(pending) = self.pending.take() else {
            return Ok(());
        };
        let done = if pending.started {
            self.wait_idle_with(&mut meanwhile)
        } else {
            self.wait_busy_with(&mut meanwhile)
        };
        self.command(cmd::PARTIAL_OUT, &[])?;
        if let Err(e) = done {
            self.old_valid = false;
            return Err(e);
        }
        if self.uc8279d {
            // The stock firmware's `CDI` for the refreshes after it.
            self.command(cmd::VCOM_DATA_INTERVAL, &[lut8279d::CDI_LATER])?;
            self.first_refresh = false;
            if pending.refresh != Refresh::Fast {
                self.fulls_left = self.fulls_left.saturating_sub(1);
            }
        } else if pending.refresh != Refresh::Fast {
            self.delay.delay_ms(SETTLE_MS);
        }
        match pending.refresh {
            Refresh::Full => self.settle = true,
            Refresh::Half => {
                self.settle = false;
                self.condition = false;
            }
            Refresh::Fast => {}
        }
        // The firmware writes the frame shown into both frames after every
        // refresh.
        self.old_valid = true;
        if pending.power_off {
            self.power_off()?;
        }
        Ok(())
    }

    /// Turns the panel's power off once nothing follows for a while: the
    /// picture stays, and the next refresh powers it up again.
    pub fn power_down(&mut self) -> Result<(), Error> {
        self.wait()?;
        if self.powered {
            self.power_off()?;
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
            self.power_off()?;
        }
        self.old_valid = false;
        self.settle = false;
        self.command(cmd::DEEP_SLEEP, &[0xA5])
    }

    /// A refresh of the frame shown to itself with `bank`, waited for.
    fn pass(&mut self, bank: &Bank) -> Result<(), Error> {
        self.load(bank)?;
        self.power_on()?;
        self.window_in()?;
        self.command(cmd::REFRESH, &[])?;
        let done = self.wait_busy();
        self.command(cmd::PARTIAL_OUT, &[])?;
        if done.is_err() {
            self.old_valid = false;
        }
        done
    }

    /// Enters the partial mode with the whole panel as its window: the
    /// bands leave the last one's (as the SDK refreshes after writing in
    /// windows, lines 104 to 113 and 490 to 498 of `Uc8253X3Driver.cpp`).
    // Uncertain without a device: whether a refresh outside the partial
    // mode would cover the whole panel too.
    fn window_in(&mut self) -> Result<(), Error> {
        self.command(cmd::PARTIAL_IN, &[])?;
        self.command(cmd::PARTIAL_WINDOW, &Window::PANEL.command())
    }

    /// Loads a refresh's waveforms into their registers.
    fn load(&mut self, bank: &Bank) -> Result<(), Error> {
        self.command(cmd::VCOM_DATA_INTERVAL, &bank.interval)?;
        self.load_luts(&bank.luts, 100)
    }

    /// [`Display::load`] with the frame counts of every phase scaled to
    /// `percent` (LAB ONLY, #40; see [`scaled`]).
    fn load_scaled(&mut self, bank: &Bank, percent: u8) -> Result<(), Error> {
        self.command(cmd::VCOM_DATA_INTERVAL, &bank.interval)?;
        self.load_luts(&bank.luts, percent)
    }

    /// Sends five waveform tables (VCOM, white to white, black to white,
    /// white to black, black to black) to their registers, padded with 0s,
    /// their frame counts scaled to `percent` where that is not 100.
    fn load_luts(&mut self, luts: &[&[u8]; 5], percent: u8) -> Result<(), Error> {
        for (register, table) in (cmd::LUT_VCOM..).zip(luts) {
            let lut = if percent == 100 {
                let mut lut = [0u8; LUT_LEN];
                lut[..table.len()].copy_from_slice(table);
                lut
            } else {
                scaled(table, percent)
            };
            self.command(register, &lut)?;
        }
        Ok(())
    }

    /// Turns the panel's power on, if it is not: a powered controller
    /// told again gives no busy edge, and the wait would take its longest.
    fn power_on(&mut self) -> Result<(), Error> {
        if !self.powered {
            self.command(cmd::POWER_ON, &[])?;
            self.wait_busy()?;
            self.powered = true;
        }
        Ok(())
    }

    fn power_off(&mut self) -> Result<(), Error> {
        self.command(cmd::POWER_OFF, &[])?;
        self.wait_busy()?;
        self.powered = false;
        Ok(())
    }

    /// Waits for the controller to finish what it was told: the busy pin
    /// goes low when it starts, then high when it is done (the SDK's
    /// `X3TwoPhase`, lines 359 to 377 of `EpdBus.cpp`). Without a low
    /// edge within [`BUSY_EDGE_MS`], it was done before it was seen.
    fn wait_busy(&mut self) -> Result<(), Error> {
        self.wait_busy_with(&mut || {})
    }

    fn wait_busy_with(&mut self, meanwhile: &mut dyn FnMut()) -> Result<(), Error> {
        if self.busy_starts(BUSY_EDGE_MS)? {
            self.wait_idle_with(meanwhile)
        } else {
            Ok(())
        }
    }

    /// Whether the controller is busy (its busy pin low) within `ms`.
    fn busy_starts(&mut self, ms: u32) -> Result<bool, Error> {
        for _ in 0..ms {
            if self.busy.is_low().map_err(|_| Error::Pin)? {
                return Ok(true);
            }
            self.delay.delay_ms(1);
        }
        self.busy.is_low().map_err(|_| Error::Pin)
    }

    /// Waits while the controller is busy (its busy pin low).
    fn wait_idle(&mut self) -> Result<(), Error> {
        self.wait_idle_with(&mut || {})
    }

    fn wait_idle_with(&mut self, meanwhile: &mut dyn FnMut()) -> Result<(), Error> {
        for _ in 0..BUSY_TIMEOUT_MS {
            if self.busy.is_high().map_err(|_| Error::Pin)? {
                return Ok(());
            }
            meanwhile();
            self.delay.delay_ms(1);
        }
        Err(Error::Timeout)
    }

    /// Fills memory `command` with `value`, the panel's lines.
    fn fill(&mut self, command: u8, value: u8) -> Result<(), Error> {
        self.command(command, &[])?;
        self.dc.set_high().map_err(|_| Error::Pin)?;
        let line = [value; LINE];
        for _ in 0..PANEL_HEIGHT {
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

/// Sends the `height` rows of a band into `window` (from [`Window::of`]),
/// line by line, each from its first column: black and white rows the
/// quick way, others pixel by pixel through `light`; rows past the band's
/// end white. Gathered in `buffer`, passed to `send` a chunk at a time.
fn send_band(
    rotation: Rotation,
    window: &Window,
    rows: &FrameView<'_>,
    height: u16,
    light: &dyn Fn(u16, u16) -> bool,
    buffer: &mut Vec<u8>,
    send: &mut dyn FnMut(&[u8]) -> Result<(), Error>,
) -> Result<(), Error> {
    buffer.clear();
    let mono = rows.format == PixelFormat::Mono1;
    match rotation {
        Rotation::Deg0 | Rotation::Deg180 if mono => {
            upright_mono(rotation, rows, height, buffer, send)
        }
        Rotation::Deg0 | Rotation::Deg180 => upright(rotation, window, height, light, buffer, send),
        _ => side(rotation, rows, height, light, buffer, send),
    }
}

/// Sends what `buffer` holds.
fn flush(
    buffer: &mut Vec<u8>,
    send: &mut dyn FnMut(&[u8]) -> Result<(), Error>,
) -> Result<(), Error> {
    if buffer.is_empty() {
        return Ok(());
    }
    send(buffer)?;
    buffer.clear();
    Ok(())
}

/// An upright band (or upside down), pixel by pixel: each column of the
/// screen is a line of the controller's, the top of the band in the
/// window's first column (upside down, its bottom).
fn upright(
    rotation: Rotation,
    window: &Window,
    height: u16,
    light: &dyn Fn(u16, u16) -> bool,
    buffer: &mut Vec<u8>,
    send: &mut dyn FnMut(&[u8]) -> Result<(), Error>,
) -> Result<(), Error> {
    let flip = rotation == Rotation::Deg180;
    for line in 0..PANEL_HEIGHT {
        let x = if flip { WIDTH - 1 - line } else { line };
        for byte in 0..window.width / 8 {
            let mut bits = 0u8;
            for bit in 0..8u16 {
                let i = byte * 8 + bit;
                let y = if flip { window.width - 1 - i } else { i };
                if y >= height || light(x, y) {
                    bits |= 0x80 >> bit;
                }
            }
            buffer.push(bits);
        }
        if buffer.len() >= CHUNK {
            flush(buffer, send)?;
        }
    }
    flush(buffer, send)
}

/// [`upright`] for black and white: eight rows by eight columns at a
/// time, turned with a few shifts.
fn upright_mono(
    rotation: Rotation,
    rows: &FrameView<'_>,
    height: u16,
    buffer: &mut Vec<u8>,
    send: &mut dyn FnMut(&[u8]) -> Result<(), Error>,
) -> Result<(), Error> {
    let flip = rotation == Rotation::Deg180;
    let bytes = usize::from(height.div_ceil(8));
    // Each of eight columns' bytes, top to bottom.
    let mut columns = [0u8; 8 * (HEIGHT as usize / 8)];
    let blocks = usize::from(WIDTH) / 8;
    for i in 0..blocks {
        let block = if flip { blocks - 1 - i } else { i };
        for group in 0..bytes {
            let mut eight = [0xFFu8; 8];
            for (r, row) in eight.iter_mut().enumerate() {
                let y = group * 8 + r;
                if y < usize::from(height) {
                    *row = rows
                        .data
                        .get(y * rows.stride + block)
                        .copied()
                        .unwrap_or(0xFF);
                }
            }
            for (c, byte) in bands::transpose8(eight).into_iter().enumerate() {
                columns[c * bytes + group] = byte;
            }
        }
        for c in 0..8 {
            let c = if flip { 7 - c } else { c };
            let column = &columns[c * bytes..(c + 1) * bytes];
            if flip {
                buffer.extend(column.iter().rev().map(|b| b.reverse_bits()));
            } else {
                buffer.extend_from_slice(column);
            }
        }
        if buffer.len() >= CHUNK {
            flush(buffer, send)?;
        }
    }
    flush(buffer, send)
}

/// A band of the screen on its side: each of its rows is a line of the
/// controller's, right to left (turned clockwise), or left to right from
/// the band's last row.
fn side(
    rotation: Rotation,
    rows: &FrameView<'_>,
    height: u16,
    light: &dyn Fn(u16, u16) -> bool,
    buffer: &mut Vec<u8>,
    send: &mut dyn FnMut(&[u8]) -> Result<(), Error>,
) -> Result<(), Error> {
    let clockwise = rotation == Rotation::Deg90;
    for i in 0..height {
        let y = if clockwise { i } else { height - 1 - i };
        let at = usize::from(y) * rows.stride;
        match rows.data.get(at..at + LINE) {
            // Black and white: each byte the other way, or as it is.
            Some(row) if rows.format == PixelFormat::Mono1 => {
                if clockwise {
                    buffer.extend(row.iter().rev().map(|b| b.reverse_bits()));
                } else {
                    buffer.extend_from_slice(row);
                }
            }
            _ => {
                for byte in 0..LINE {
                    let mut bits = 0u8;
                    for bit in 0..8 {
                        let column = (byte * 8 + bit) as u16;
                        let x = if clockwise {
                            PANEL_WIDTH - 1 - column
                        } else {
                            column
                        };
                        if light(x, y) {
                            bits |= 0x80 >> bit;
                        }
                    }
                    buffer.push(bits);
                }
            }
        }
        if buffer.len() >= CHUNK {
            flush(buffer, send)?;
        }
    }
    flush(buffer, send)
}

#[cfg(test)]
mod tests;
