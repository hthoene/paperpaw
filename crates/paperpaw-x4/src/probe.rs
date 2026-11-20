//! Which display controller a device has, found at start before the SPI
//! bus takes the panel's pins: Xteink ships the same devices with an
//! SSD1677 or an UltraChip UC8179 or UC8279, batch by batch, and each
//! needs its own driver (an SSD1677's driver shows nothing on the others).
//!
//! The panel has no line of its own to answer on: its data line is read
//! back on the line the processor sends on, so [`probe`] drives the lines
//! itself (bit by bit, about 500 kHz), as the stock firmware does. It
//! asks the UltraChip controllers' status (`FLG`, 0x71) and revision
//! (`REV`, 0x70) and looks at the busy line at rest: the SSD1677 holds it
//! low when idle and answers neither (the line stays pulled up, 0xFF);
//! the UltraChips hold it high when idle and answer, the revision's third
//! byte naming the panel (a value its maker programs: 0x01 and 0x40 come
//! with a UC8179; 0x02, 0x03, 0x41, 0x42 and 0x67 to 0x69 with a UC8279).
//! An answer that fits none is no answer: the device keeps the driver it
//! always had.
//!
//! Sources (MIT licence, facts only): the FreeInk SDK's `XteinkDetect`
//! and Papyrix's `X4ClassicDisplayProbe`, which read the stock firmware's
//! probe and panel table; the UC8179's datasheet (`REV`, `FLG`).

use core::fmt;

use embedded_hal::delay::DelayNs;
use embedded_hal::digital::{InputPin, OutputPin};

/// The display controllers Xteink's devices come with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Controller {
    /// Solomon Systech's: the X4, the first X4 Pros.
    Ssd1677,
    /// UltraChip's: X4 Pros and X4 Classics of later batches.
    Uc8179,
    Uc8279,
    /// UltraChip's, on the X3 (792 × 528, its waveforms sent by the
    /// driver); not told apart by [`Probe::controller`], the X3's build
    /// takes it for granted (see [`Probe::x3_uc8279d`]).
    Uc8253,
    /// The controller of the Boox Picco's SE0400NQW47 panel, as its stock
    /// firmware calls it (see [`ssd2677`](crate::ssd2677)); told by
    /// [`Probe::picco_ssd2677`], the Picco's build asks it.
    #[cfg(feature = "picco")]
    Ssd2677,
}

impl Controller {
    pub fn name(self) -> &'static str {
        match self {
            Controller::Ssd1677 => "SSD1677",
            Controller::Uc8179 => "UC8179",
            Controller::Uc8279 => "UC8279",
            Controller::Uc8253 => "UC8253",
            #[cfg(feature = "picco")]
            Controller::Ssd2677 => "SSD2677",
        }
    }
}

impl fmt::Display for Controller {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// The revision's third byte of panels that come with a UC8279 (the
/// stock firmware's panel table); others with an answer are UC8179s.
const UC8279_PANELS: [u8; 7] = [0x02, 0x03, 0x41, 0x42, 0x67, 0x68, 0x69];

/// The commands asked.
const FLG: u8 = 0x71;
const REV: u8 = 0x70;
/// Reads the controller's OTP from its first byte on (a UC8179's: see
/// [`otp`](crate::otp)).
const ROTP: u8 = 0xA2;

/// How long after the reset the controller is asked.
const AFTER_RESET_MS: u32 = 30;
/// How long the reset is held: as tried on the X4 and the X4 Pro, and as
/// the stock firmware of the X4 Classic and the X3 holds it (the FreeInk
/// SDK's `XteinkDetect`, Papyrix's `X4ClassicDisplayProbe`).
pub const RESET_MS: u32 = 10;
pub const STOCK_RESET_MS: u32 = 50;
/// The longest an UltraChip that answered takes to come to rest (it loads
/// its settings after a reset): the stock firmware's limit.
const SETTLE_MS: u32 = 300;
/// Half a clock period.
const HALF_PERIOD_US: u32 = 1;

/// The panel's data line, which the controller drives while it answers.
pub trait DataLine: OutputPin + InputPin {
    /// Leaves the line to the controller: an input, pulled up.
    fn listen(&mut self);
    /// Drives the line again.
    fn drive(&mut self);
}

/// The panel's lines, before the SPI bus takes them.
#[derive(Debug)]
pub struct Lines<CLK, SDA, CS, DC, RST, BUSY> {
    pub clock: CLK,
    pub data: SDA,
    pub select: CS,
    pub dc: DC,
    pub reset: RST,
    pub busy: BUSY,
}

/// What the controller answered: the busy line at rest, its status and
/// its revision's first five bytes, as they came.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Probe {
    pub busy_high: bool,
    pub flg: u8,
    pub rev: [u8; 5],
}

impl Probe {
    /// The controller these answers name, if they name one surely: two of
    /// three signs for an UltraChip (the busy line high at rest, a status
    /// that can be one, a revision that is not one byte repeated), or all
    /// of an SSD1677's (the busy line low, no answer).
    pub fn controller(&self) -> Option<Controller> {
        let flg_fits = self.flg != 0x00 && self.flg != 0xFF && self.flg & 0x01 != 0;
        let rev_fits = self.rev.iter().any(|&b| b != self.rev[0]);
        let signs = [self.busy_high, flg_fits, rev_fits];
        if signs.iter().filter(|&&s| s).count() >= 2 {
            return Some(if rev_fits && UC8279_PANELS.contains(&self.rev[2]) {
                Controller::Uc8279
            } else {
                Controller::Uc8179
            });
        }
        let silent = self.flg == 0xFF && self.rev == [0xFF; 5];
        (!self.busy_high && silent).then_some(Controller::Ssd1677)
    }

    /// On an X3: whether it has the newer UC8279d (the revision's third
    /// byte 0x66, as the FreeInk SDK's `XteinkDetect` reads it).
    pub fn x3_uc8279d(&self) -> bool {
        self.rev[2] == 0x66
    }

    /// On a Boox Picco: whether its panel is the SE0400NQW47 with the
    /// "SSD2677" (its revision begins 07 01 01, as the stock firmware and
    /// the FreeInk SDK's `piccoProbePanel` tell it); its other panels have
    /// an SSD1677.
    pub fn picco_ssd2677(&self) -> bool {
        self.rev[..3] == [0x07, 0x01, 0x01]
    }
}

/// The controller the factory wrote down for the panel (the factory NVS's
/// `hw_calib`, `screenType`, see [`nvs`](crate::nvs)), as the X4
/// Classic's stock firmware reads it: 1 and 0x0B a UC8179, 2 and 0x0C a
/// UC8279, 3 an SSD1677 (the FreeInk SDK's `XteinkDetect`, Papyrix's
/// `X4ClassicPanelPolicy`, MIT); other values name none.
pub fn factory_controller(screen_type: i64) -> Option<Controller> {
    match screen_type {
        1 | 0x0B => Some(Controller::Uc8179),
        2 | 0x0C => Some(Controller::Uc8279),
        3 => Some(Controller::Ssd1677),
        _ => None,
    }
}

impl fmt::Display for Probe {
    /// For the log, all a bug report needs: e.g. `UC8179 (FLG 13, REV 00
    /// 00 01 FF FF, BUSY high at rest)`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.controller() {
            Some(controller) => write!(f, "{controller}")?,
            None => f.write_str("unknown")?,
        }
        write!(f, " (FLG {:02X}, REV", self.flg)?;
        for byte in self.rev {
            write!(f, " {byte:02X}")?;
        }
        let busy = if self.busy_high { "high" } else { "low" };
        write!(f, ", BUSY {busy} at rest)")
    }
}

/// Resets the controller (held `reset_ms`, [`RESET_MS`] or
/// [`STOCK_RESET_MS`]) and asks which it is; the lines are left as they
/// were set up (the data line driven, the chip not selected). Takes some
/// 30 ms more, or up to 300 ms more for an UltraChip still starting.
pub fn probe<CLK, SDA, CS, DC, RST, BUSY, D>(
    lines: &mut Lines<CLK, SDA, CS, DC, RST, BUSY>,
    delay: &mut D,
    reset_ms: u32,
) -> Probe
where
    CLK: OutputPin,
    SDA: DataLine,
    CS: OutputPin,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
    D: DelayNs,
{
    let _ = lines.select.set_high();
    let _ = lines.clock.set_low();
    let _ = lines.dc.set_low();
    let _ = lines.data.set_low();
    for (high, ms) in [(true, 10), (false, reset_ms), (true, AFTER_RESET_MS)] {
        let _ = if high {
            lines.reset.set_high()
        } else {
            lines.reset.set_low()
        };
        delay.delay_ms(ms);
    }
    let mut probe = ask(lines, delay);
    // An UltraChip still loading its settings: asked again at rest.
    if probe.controller() != Some(Controller::Ssd1677) && !probe.busy_high {
        for _ in 0..SETTLE_MS {
            if lines.busy.is_high().unwrap_or(false) {
                probe = ask(lines, delay);
                break;
            }
            delay.delay_ms(1);
        }
    }
    probe
}

/// Reads `out.len()` bytes of a UC8179's OTP from its start, on the lines as
/// [`probe`] leaves them (some 4 µs a bit: a 3 KB bank takes about 100 ms).
/// What comes back is the controller's to say: [`otp::describe`](crate::otp::describe)
/// tells what it makes of it.
pub fn read_otp<CLK, SDA, CS, DC, RST, BUSY, D>(
    lines: &mut Lines<CLK, SDA, CS, DC, RST, BUSY>,
    delay: &mut D,
    out: &mut [u8],
) where
    CLK: OutputPin,
    SDA: DataLine,
    CS: OutputPin,
    DC: OutputPin,
    D: DelayNs,
{
    read(lines, delay, ROTP, out);
}

fn ask<CLK, SDA, CS, DC, RST, BUSY, D>(
    lines: &mut Lines<CLK, SDA, CS, DC, RST, BUSY>,
    delay: &mut D,
) -> Probe
where
    CLK: OutputPin,
    SDA: DataLine,
    CS: OutputPin,
    DC: OutputPin,
    BUSY: InputPin,
    D: DelayNs,
{
    let mut flg = [0];
    read(lines, delay, FLG, &mut flg);
    let mut rev = [0; 5];
    read(lines, delay, REV, &mut rev);
    Probe {
        busy_high: lines.busy.is_high().unwrap_or(false),
        flg: flg[0],
        rev,
    }
}

/// Sends `command`, then reads `out.len()` bytes back on the data line,
/// each bit after the clock rises (the controller moves on as it falls).
fn read<CLK, SDA, CS, DC, RST, BUSY, D>(
    lines: &mut Lines<CLK, SDA, CS, DC, RST, BUSY>,
    delay: &mut D,
    command: u8,
    out: &mut [u8],
) where
    CLK: OutputPin,
    SDA: DataLine,
    CS: OutputPin,
    DC: OutputPin,
    D: DelayNs,
{
    let _ = lines.dc.set_low();
    let _ = lines.select.set_low();
    delay.delay_us(HALF_PERIOD_US);
    for bit in (0..8).rev() {
        let _ = if command >> bit & 1 != 0 {
            lines.data.set_high()
        } else {
            lines.data.set_low()
        };
        delay.delay_us(HALF_PERIOD_US);
        let _ = lines.clock.set_high();
        delay.delay_us(HALF_PERIOD_US);
        let _ = lines.clock.set_low();
    }
    let _ = lines.dc.set_high();
    lines.data.listen();
    delay.delay_us(HALF_PERIOD_US);
    for byte in out.iter_mut() {
        *byte = 0;
        for _ in 0..8 {
            let _ = lines.clock.set_high();
            delay.delay_us(HALF_PERIOD_US);
            let bit = lines.data.is_high().unwrap_or(true);
            *byte = *byte << 1 | u8::from(bit);
            let _ = lines.clock.set_low();
            delay.delay_us(HALF_PERIOD_US);
        }
    }
    let _ = lines.select.set_high();
    lines.data.drive();
    let _ = lines.data.set_low();
    let _ = lines.dc.set_low();
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::rc::Rc;
    use alloc::string::ToString;
    use alloc::vec::Vec;
    use core::cell::RefCell;
    use core::convert::Infallible;
    use embedded_hal::digital::ErrorType;

    /// A panel, simulated on its lines.
    #[derive(Debug, Default)]
    struct Panel {
        /// What it answers to `FLG` and `REV`; `None`: nothing (an
        /// SSD1677, the line pulled up).
        flg: Option<u8>,
        rev: Option<[u8; 5]>,
        /// Its busy line high at rest (an UltraChip's).
        idle_high: bool,
        /// How long it stays busy after a reset (ms).
        starting_ms: u32,
        /// The line read low without an answer (pulled down, or nothing
        /// there): the pull-up does not win.
        line_low: bool,
        // The lines and the time.
        now_us: u64,
        reset_at_us: u64,
        select: bool,
        dc: bool,
        clock: bool,
        data: bool,
        listening: bool,
        shift: u8,
        bits: u8,
        answer: Vec<u8>,
        at: usize,
        /// Bytes received while selected, with DC low: the commands.
        commands: Vec<u8>,
    }

    impl Panel {
        fn busy_ms(&self) -> u64 {
            (self.now_us - self.reset_at_us) / 1000
        }

        fn busy_line(&self) -> bool {
            let starting = self.busy_ms() < u64::from(self.starting_ms);
            // Busy: an SSD1677 high, an UltraChip low.
            if starting {
                !self.idle_high
            } else {
                self.idle_high
            }
        }

        fn clock(&mut self, high: bool) {
            let rising = high && !self.clock;
            let falling = !high && self.clock;
            self.clock = high;
            if self.select {
                return;
            }
            if rising && !self.listening && !self.dc {
                self.shift = self.shift << 1 | u8::from(self.data);
                self.bits += 1;
                if self.bits == 8 {
                    self.bits = 0;
                    self.commands.push(self.shift);
                    let starting = self.busy_ms() < u64::from(self.starting_ms);
                    self.answer = match self.shift {
                        // Its busy flag low while starting.
                        FLG => self
                            .flg
                            .map(|f| f & !u8::from(starting))
                            .into_iter()
                            .collect(),
                        REV => self.rev.map(Vec::from).unwrap_or_default(),
                        _ => Vec::new(),
                    };
                    self.at = 0;
                }
            }
            if falling && self.listening {
                self.at += 1;
            }
        }

        fn data_line(&self) -> bool {
            match self.answer.get(self.at / 8) {
                Some(byte) if self.listening && !self.select => byte >> (7 - self.at % 8) & 1 != 0,
                _ => !self.line_low,
            }
        }
    }

    type Shared = Rc<RefCell<Panel>>;
    #[derive(Clone, Copy)]
    enum Line {
        Clock,
        Data,
        Select,
        Dc,
        Reset,
        Busy,
    }
    struct Pin(Shared, Line);
    struct Delay(Shared);

    impl ErrorType for Pin {
        type Error = Infallible;
    }
    impl OutputPin for Pin {
        fn set_low(&mut self) -> Result<(), Infallible> {
            self.set(false);
            Ok(())
        }
        fn set_high(&mut self) -> Result<(), Infallible> {
            self.set(true);
            Ok(())
        }
    }
    impl Pin {
        fn set(&mut self, high: bool) {
            let mut p = self.0.borrow_mut();
            match self.1 {
                Line::Clock => p.clock(high),
                Line::Data => {
                    assert!(!p.listening, "the data line driven while listening");
                    p.data = high;
                }
                Line::Select => {
                    p.select = high;
                    if high {
                        p.bits = 0;
                        p.answer.clear();
                    }
                }
                Line::Dc => p.dc = high,
                // It starts as the reset is let go.
                Line::Reset => p.reset_at_us = p.now_us,
                Line::Busy => unreachable!(),
            }
        }
    }
    impl InputPin for Pin {
        fn is_high(&mut self) -> Result<bool, Infallible> {
            let p = self.0.borrow();
            Ok(match self.1 {
                Line::Busy => p.busy_line(),
                Line::Data => p.data_line(),
                _ => unreachable!(),
            })
        }
        fn is_low(&mut self) -> Result<bool, Infallible> {
            self.is_high().map(|h| !h)
        }
    }
    impl DataLine for Pin {
        fn listen(&mut self) {
            self.0.borrow_mut().listening = true;
        }
        fn drive(&mut self) {
            self.0.borrow_mut().listening = false;
        }
    }
    impl DelayNs for Delay {
        fn delay_ns(&mut self, ns: u32) {
            self.0.borrow_mut().now_us += u64::from(ns.div_ceil(1000));
        }
    }

    fn run(panel: Panel) -> (Probe, Shared) {
        run_held(panel, RESET_MS)
    }

    fn run_held(panel: Panel, reset_ms: u32) -> (Probe, Shared) {
        let shared = Rc::new(RefCell::new(panel));
        let pin = |line| Pin(shared.clone(), line);
        let mut lines = Lines {
            clock: pin(Line::Clock),
            data: pin(Line::Data),
            select: pin(Line::Select),
            dc: pin(Line::Dc),
            reset: pin(Line::Reset),
            busy: pin(Line::Busy),
        };
        let probe = probe(&mut lines, &mut Delay(shared.clone()), reset_ms);
        (probe, shared)
    }

    fn ssd1677() -> Panel {
        Panel {
            starting_ms: 5,
            ..Panel::default()
        }
    }

    fn uc81(rev: [u8; 5]) -> Panel {
        Panel {
            flg: Some(0x13),
            rev: Some(rev),
            idle_high: true,
            starting_ms: 20,
            ..Panel::default()
        }
    }

    #[test]
    fn an_ssd1677_answers_nothing_its_busy_line_low() {
        let (probe, panel) = run(ssd1677());
        assert_eq!(probe.controller(), Some(Controller::Ssd1677));
        assert_eq!(probe.flg, 0xFF);
        assert_eq!(probe.rev, [0xFF; 5]);
        assert_eq!(panel.borrow().commands, [FLG, REV]);
        // Quick: no wait for an UltraChip.
        assert!(panel.borrow().now_us < 60_000);
    }

    #[test]
    fn ultrachips_answer_their_panel() {
        // As an X4 Pro answered.
        let (probe, _) = run(uc81([0x00, 0x00, 0x01, 0xFF, 0xFF]));
        assert_eq!(probe.controller(), Some(Controller::Uc8179));
        assert_eq!(
            probe.to_string(),
            "UC8179 (FLG 13, REV 00 00 01 FF FF, BUSY high at rest)"
        );
        for panel in UC8279_PANELS {
            let (probe, _) = run(uc81([0x00, 0x00, panel, 0xFF, 0xFF]));
            assert_eq!(probe.controller(), Some(Controller::Uc8279), "{panel:02X}");
        }
        let (probe, _) = run(uc81([0x00, 0x00, 0x40, 0xFF, 0xFF]));
        assert_eq!(probe.controller(), Some(Controller::Uc8179));
    }

    #[test]
    fn an_ultrachip_still_starting_is_asked_again_at_rest() {
        let (probe, panel) = run(Panel {
            starting_ms: 150,
            ..uc81([0x00, 0x00, 0x68, 0xFF, 0xFF])
        });
        assert!(probe.busy_high);
        assert_eq!(probe.flg, 0x13);
        assert_eq!(probe.controller(), Some(Controller::Uc8279));
        assert_eq!(panel.borrow().commands, [FLG, REV, FLG, REV]);
    }

    #[test]
    fn an_ultrachip_without_a_panel_value_is_a_uc8179() {
        // Its revision unprogrammed (all FF): the status and the busy line.
        let (probe, _) = run(uc81([0xFF; 5]));
        assert_eq!(probe.controller(), Some(Controller::Uc8179));
    }

    #[test]
    fn answers_that_fit_nothing_name_nothing() {
        // Nothing on the lines: all low.
        let (probe, _) = run(Panel {
            line_low: true,
            ..Panel::default()
        });
        assert_eq!(probe.controller(), None);
        assert_eq!(
            probe.to_string(),
            "unknown (FLG 00, REV 00 00 00 00 00, BUSY low at rest)"
        );
        // The busy line high, but no answer: neither surely.
        let (probe, _) = run(Panel {
            idle_high: true,
            ..Panel::default()
        });
        assert_eq!(probe.controller(), None);
        // A status alone is not enough.
        let one = Probe {
            busy_high: false,
            flg: 0x13,
            rev: [0xFF; 5],
        };
        assert_eq!(one.controller(), None);
    }

    #[test]
    fn the_stock_reset_is_held_longer() {
        let (probe, panel) = run_held(ssd1677(), STOCK_RESET_MS);
        assert_eq!(probe.controller(), Some(Controller::Ssd1677));
        assert!(panel.borrow().now_us >= 90_000);
    }

    #[test]
    fn the_factory_names_its_controller() {
        assert_eq!(factory_controller(1), Some(Controller::Uc8179));
        assert_eq!(factory_controller(0x0C), Some(Controller::Uc8279));
        assert_eq!(factory_controller(3), Some(Controller::Ssd1677));
        assert_eq!(factory_controller(0), None);
        assert_eq!(factory_controller(-1), None);
    }

    #[test]
    fn the_lines_are_left_as_set_up() {
        let (_, panel) = run(uc81([0x00, 0x00, 0x01, 0xFF, 0xFF]));
        let p = panel.borrow();
        assert!(p.select && !p.listening && !p.clock);
    }

    #[test]
    fn an_x3_with_the_newer_controller_is_told_by_the_third_revision_byte() {
        let newer = Probe {
            busy_high: true,
            flg: 0x13,
            rev: [0x00, 0x00, 0x66, 0xFF, 0xFF],
        };
        let older = Probe {
            rev: [0x00, 0x00, 0x01, 0xFF, 0xFF],
            ..newer
        };
        let silent = Probe {
            busy_high: false,
            flg: 0xFF,
            rev: [0xFF; 5],
        };
        assert!(newer.x3_uc8279d());
        assert!(!older.x3_uc8279d());
        assert!(!silent.x3_uc8279d());
    }
}
