//! The UC8179 driver against a simulated controller.

use super::*;
use alloc::rc::Rc;
use core::cell::RefCell;
use embedded_hal::spi::{ErrorType, Operation};
use paperpaw_platform::PixelFormat;

/// The controller, simulated: what it was told, and its memories.
#[derive(Default)]
struct Controller {
    data: bool,
    command: u8,
    commands: Vec<u8>,
    new: Vec<u8>,
    old: Vec<u8>,
    /// The temperatures told, by refresh.
    temperatures: Vec<u8>,
    /// The memories at each refresh: *old*, then *new*.
    refreshed: Vec<(Vec<u8>, Vec<u8>)>,
    /// The memories as addressed at each refresh: *old*, then *new*.
    rams: Vec<(Vec<u8>, Vec<u8>)>,
    /// Each command with its data, in order.
    log: Vec<(u8, Vec<u8>)>,
    /// The memories as addressed (600 lines of 100 bytes), written in
    /// the window while in partial mode.
    ram_new: Vec<u8>,
    ram_old: Vec<u8>,
    partial: bool,
    window: [u8; 9],
    /// Where the next byte goes: line, byte of the line.
    cursor: (usize, usize),
}

impl Controller {
    fn byte(&mut self, b: u8) {
        if !self.data {
            self.command = b;
            self.commands.push(b);
            self.log.push((b, Vec::new()));
            match b {
                cmd::NEW => self.new.clear(),
                cmd::OLD => self.old.clear(),
                cmd::REFRESH => {
                    self.refreshed.push((self.old.clone(), self.new.clone()));
                    self.rams.push((self.ram_old.clone(), self.ram_new.clone()));
                }
                cmd::PARTIAL_IN => self.partial = true,
                cmd::PARTIAL_OUT => self.partial = false,
                _ => {}
            }
            if matches!(b, cmd::NEW | cmd::OLD) {
                let (first, _, line, _) = self.area();
                self.cursor = (line, first);
            }
            return;
        }
        if let Some((_, data)) = self.log.last_mut() {
            data.push(b);
        }
        match self.command {
            cmd::NEW | cmd::OLD => {
                if self.command == cmd::NEW {
                    self.new.push(b);
                } else {
                    self.old.push(b);
                }
                self.store(b);
            }
            cmd::TEMPERATURE => self.temperatures.push(b),
            cmd::PARTIAL_WINDOW => {
                let at = self.log.last().map_or(0, |(_, d)| d.len() - 1);
                self.window[at] = b;
            }
            _ => {}
        }
    }

    /// The bytes and lines written to: first and last byte, first and
    /// last line.
    fn area(&self) -> (usize, usize, usize, usize) {
        if !self.partial {
            return (0, LINE - 1, 0, usize::from(ADDRESSED_LINES) - 1);
        }
        let w = &self.window;
        let word = |high: u8, low: u8| usize::from(high) << 8 | usize::from(low);
        (
            word(w[0], w[1]) / 8,
            word(w[2], w[3]) / 8,
            word(w[4], w[5]),
            word(w[6], w[7]),
        )
    }

    fn store(&mut self, b: u8) {
        let (first, last, _, last_line) = self.area();
        let (line, at) = self.cursor;
        assert!(line <= last_line, "written past the window");
        if self.ram_new.is_empty() {
            let size = LINE * usize::from(ADDRESSED_LINES);
            self.ram_new = vec![0x55; size];
            self.ram_old = vec![0x55; size];
        }
        let ram = if self.command == cmd::NEW {
            &mut self.ram_new
        } else {
            &mut self.ram_old
        };
        ram[line * LINE + at] = b;
        self.cursor = if at == last {
            (line + 1, first)
        } else {
            (line, at + 1)
        };
    }

    /// The data of the last `command` sent.
    fn last(&self, command: u8) -> Option<&[u8]> {
        self.log
            .iter()
            .rev()
            .find(|(c, _)| *c == command)
            .map(|(_, d)| &d[..])
    }

    /// The commands from the last `from` on.
    fn since(&self, from: u8) -> Vec<u8> {
        let start = self.log.iter().rposition(|(c, _)| *c == from).unwrap_or(0);
        self.log[start..].iter().map(|(c, _)| *c).collect()
    }
}

struct Spi(Rc<RefCell<Controller>>);
struct Dc(Rc<RefCell<Controller>>);
struct Pin;
struct Delay;

impl ErrorType for Spi {
    type Error = core::convert::Infallible;
}
impl SpiDevice for Spi {
    fn transaction(&mut self, ops: &mut [Operation<'_, u8>]) -> Result<(), Self::Error> {
        for op in ops {
            if let Operation::Write(bytes) = op {
                for b in bytes.iter() {
                    self.0.borrow_mut().byte(*b);
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
impl InputPin for Pin {
    fn is_high(&mut self) -> Result<bool, Self::Error> {
        Ok(true)
    }
    fn is_low(&mut self) -> Result<bool, Self::Error> {
        Ok(false)
    }
}
impl DelayNs for Delay {
    fn delay_ns(&mut self, _: u32) {}
}

type Simulated = Display<Spi, Dc, Pin, Pin, Delay>;

fn display() -> (Simulated, Rc<RefCell<Controller>>) {
    display_of(Chip::Uc8179, Frame::Kept)
}

fn display_of(chip: Chip, frame: Frame) -> (Simulated, Rc<RefCell<Controller>>) {
    let c = Rc::new(RefCell::new(Controller::default()));
    let mut d = Display::new(chip, frame, Spi(c.clone()), Dc(c.clone()), Pin, Pin, Delay);
    d.init().unwrap();
    (d, c)
}

/// A band of the upright screen, all black but for one white pixel.
fn band_with(x: u16, y: u16) -> Vec<u8> {
    let stride = usize::from(WIDTH / 8);
    let mut data = vec![0u8; stride * 8];
    data[usize::from(y) * stride + usize::from(x / 8)] |= 0x80 >> (x % 8);
    data
}

fn view(data: &[u8], height: u16) -> FrameView<'_> {
    FrameView::new(
        WIDTH,
        height,
        PixelFormat::Mono1,
        usize::from(WIDTH / 8),
        data,
    )
}

#[test]
fn frames_are_sent_whole_last_line_first() {
    let (mut d, c) = display();
    // Screen pixel (5, 3) white: the panel's line 474, column 3.
    let band = band_with(5, 3);
    d.write_rows(0, view(&band, 8), Planes::BOTH).unwrap();
    d.refresh(Refresh::Full, false).unwrap();
    let c = c.borrow();
    assert_eq!(c.new.len(), LINE * usize::from(ADDRESSED_LINES));
    // Line 474 of 480 is sent as the sixth.
    let line = 5;
    assert_eq!(c.new[line * LINE] & 0x10, 0x10);
    assert_eq!(c.new[line * LINE] & 0x08, 0);
    // The lines past the panel are white.
    assert!(
        c.new[usize::from(PANEL_HEIGHT) * LINE..]
            .iter()
            .all(|b| *b == 0xFF)
    );
    // After the refresh the frame is the old one too.
    assert_eq!(c.old, c.new);
}

/// With the frame kept, the bands written as new are shown and then
/// in both frames after every kind of refresh, waited for or not:
/// nothing needs to be sent again.
#[test]
fn a_kept_frame_is_in_both_frames_after_a_refresh() {
    let (mut d, c) = display();
    assert!(d.keeps_frame());
    let mut before = Vec::new();
    for (x, refresh) in [Refresh::Full, Refresh::Half, Refresh::Fast, Refresh::Fast]
        .into_iter()
        .enumerate()
    {
        let band = band_with(x as u16, 0);
        d.write_rows(8, view(&band, 8), Planes::NEW).unwrap();
        let kept = d.frame.clone();
        if x % 2 == 0 {
            d.refresh(refresh, false).unwrap();
        } else {
            d.start_refresh(refresh, false).unwrap();
            d.wait().unwrap();
        }
        let c = c.borrow();
        let shown = &c.refreshed.last().unwrap().1;
        assert_ne!(*shown, before);
        assert_eq!(c.new, *shown);
        assert_eq!(c.old, *shown);
        before.clone_from(shown);
        drop(c);
        // Sending the band again into both frames changes nothing.
        d.write_rows(8, view(&band, 8), Planes::BOTH).unwrap();
        assert_eq!(d.frame, kept);
    }
    let (alone, _) = display_of(Chip::Uc8179, Frame::InController);
    assert!(!alone.keeps_frame());
}

#[test]
fn a_fast_refresh_needs_a_frame_shown() {
    let (mut d, c) = display();
    let band = band_with(0, 0);
    d.write_rows(0, view(&band, 8), Planes::NEW).unwrap();
    // The first after init drives every pixel.
    d.refresh(Refresh::Fast, false).unwrap();
    d.refresh(Refresh::Fast, false).unwrap();
    let c = c.borrow();
    assert_eq!(c.temperatures, [0x1E, 0x5A]);
    assert!(c.commands.contains(&cmd::PARTIAL_IN));
    assert!(c.commands.contains(&cmd::PARTIAL_OUT));
}

#[test]
fn a_trial_loads_its_waveforms_for_a_fast_refresh_and_only_for_that() {
    let (mut d, c) = display();
    let band = band_with(0, 0);
    d.write_rows(0, view(&band, 8), Planes::NEW).unwrap();
    // The first refresh drives every pixel: no trial.
    d.set_experiment(Some(Experiment {
        drive: Direct::new(2, 1),
        swapped: false,
        repeat: 0,
        hold: 0,
        rate: 4,
        own_at: None,
    }));
    d.refresh(Refresh::Fast, false).unwrap();
    assert!(c.borrow().last(0x22).is_none());
    d.refresh(Refresh::Fast, false).unwrap();
    {
        let c = c.borrow();
        // The panel takes its waveforms from the registers, loads the
        // five, and asks the frame rate (60 Hz, code 7).
        assert_eq!(c.last(cmd::PANEL_SETTING), Some(&UC8179.panel[..]));
        for register in [0x20, 0x21, 0x22, 0x23, 0x24] {
            assert_eq!(c.last(register).map(<[u8]>::len), Some(LUT_LEN));
        }
        assert_eq!(
            c.last(0x22).map(|d| d[..6].to_vec()),
            Some(vec![0b01_10_10_00, 1, 0, 2, 0, 0])
        );
        // Asked, and the controller's own (50 Hz, code 6) given back
        // once the refresh ended.
        let rates: Vec<u8> = c
            .log
            .iter()
            .filter(|(command, _)| *command == cmd::PLL)
            .map(|(_, data)| data[0])
            .collect();
        assert_eq!(rates, [0x07, 0x06]);
    }
    // A half refresh after it is the panel's own again.
    let before = c.borrow().log.len();
    d.refresh(Refresh::Half, false).unwrap();
    let c = c.borrow();
    let after: Vec<&(u8, Vec<u8>)> = c.log[before..].iter().collect();
    assert!(
        after
            .iter()
            .all(|(command, _)| !(0x20..=0x24).contains(command))
    );
    assert!(
        after
            .iter()
            .any(|(command, data)| *command == cmd::PANEL_SETTING && data == &UC8179.panel_otp)
    );
}

#[test]
fn the_own_frame_rate_is_not_asked_and_other_rates_are_given_back() {
    let (mut d, c) = display();
    let band = band_with(0, 0);
    d.write_rows(0, view(&band, 8), Planes::NEW).unwrap();
    d.refresh(Refresh::Fast, false).unwrap();
    let trial = |rate| Experiment {
        drive: Direct::new(1, 0),
        swapped: false,
        repeat: 0,
        hold: 0,
        rate,
        own_at: None,
    };
    d.set_experiment(Some(trial(OWN_RATE)));
    d.refresh(Refresh::Fast, false).unwrap();
    let plls = |c: &Controller| {
        c.log
            .iter()
            .filter(|(command, _)| *command == cmd::PLL)
            .count()
    };
    let at_start = plls(&c.borrow());
    d.set_experiment(Some(trial(8)));
    d.refresh(Refresh::Fast, false).unwrap();
    // Asked, then given back: the controller's own again.
    assert_eq!(plls(&c.borrow()), at_start + 2);
    assert_eq!(c.borrow().last(cmd::PLL), Some(&[0x06][..]));
}

#[test]
fn a_trial_runs_at_the_otps_voltages_when_they_are_known() {
    let (mut d, c) = display();
    let band = band_with(0, 0);
    d.write_rows(0, view(&band, 8), Planes::NEW).unwrap();
    d.refresh(Refresh::Fast, false).unwrap();
    d.set_trial_power(Some(Power {
        gate: 7,
        vdh: 0x3F,
        vdl: 0x3F,
        vdhr: 0x0D,
        vcom: 0x1A,
    }));
    d.set_experiment(Some(Experiment {
        drive: Direct::new(3, 0),
        swapped: false,
        repeat: 1,
        hold: 1,
        rate: OWN_RATE,
        own_at: None,
    }));
    d.refresh(Refresh::Fast, false).unwrap();
    let c = c.borrow();
    assert_eq!(
        c.last(cmd::POWER_SETTING),
        Some(&[0x07, 0x17, 0x3F, 0x3F, 0x0D][..])
    );
    assert_eq!(c.last(cmd::VCOM_DC), Some(&[0x1A][..]));
}

#[test]
fn the_panels_own_waveform_at_a_temperature_keeps_the_otp_and_forces_it() {
    let (mut d, c) = display();
    let band = band_with(0, 0);
    d.write_rows(0, view(&band, 8), Planes::NEW).unwrap();
    d.refresh(Refresh::Fast, false).unwrap();
    d.set_experiment(Some(Experiment {
        drive: Direct::new(3, 0),
        swapped: false,
        repeat: 1,
        hold: 0,
        rate: 8,
        own_at: Some(95),
    }));
    d.refresh(Refresh::Fast, false).unwrap();
    let c = c.borrow();
    // The OTP's waveform (REG = 0), the temperature as told, no
    // registers loaded, no frame rate asked.
    assert_eq!(c.last(cmd::PANEL_SETTING), Some(&UC8179.panel_otp[..]));
    assert_eq!(c.temperatures.last(), Some(&95));
    assert!(c.last(0x22).is_none());
    assert!(c.last(cmd::PLL).is_none() || c.last(cmd::PLL) == Some(&[0x06][..]));
}

#[test]
fn only_a_uc8179_takes_a_trial() {
    let (mut d, _) = display_of(Chip::Uc8279, Frame::Kept);
    assert!(!d.set_experiment(None));
    let (mut d, _) = display();
    assert!(d.set_experiment(None));
}

/// A gray band of the upright screen, white but for its first four
/// pixels: black, dark gray, light gray, white.
fn gray_band() -> Vec<u8> {
    let mut data = vec![0xFF; usize::from(WIDTH / 4) * 8];
    data[0] = 0b00_01_10_11;
    data
}

fn gray_view(data: &[u8]) -> FrameView<'_> {
    FrameView::new(WIDTH, 8, PixelFormat::Gray2, usize::from(WIDTH / 4), data)
}

/// The bit of the upright screen's pixel `(x, 0)` in memory as sent:
/// the panel's line `479 - x`, sent as the `x`th, its first bit.
fn bit(memory: &[u8], x: usize) -> u8 {
    memory[x * LINE] >> 7
}

#[test]
fn grays_are_lightened_from_black() {
    let (mut d, c) = display();
    let band = gray_band();
    d.write_gray_rows(0, gray_view(&band)).unwrap();
    d.refresh_gray().unwrap();
    let c = c.borrow();
    let [shown, (old, new)] = &c.refreshed[..] else {
        panic!("two refreshes: {}", c.refreshed.len());
    };
    // First the frame in black and white: only white white.
    let levels = 0..4;
    assert_eq!(
        levels.clone().map(|x| bit(&shown.1, x)).collect::<Vec<_>>(),
        [0, 0, 0, 1]
    );
    // Then the codes: black 00, dark gray 11, light gray 10, white 11.
    assert_eq!(
        levels.clone().map(|x| bit(old, x)).collect::<Vec<_>>(),
        [0, 1, 1, 1]
    );
    assert_eq!(
        levels.map(|x| bit(new, x)).collect::<Vec<_>>(),
        [0, 1, 0, 1]
    );
    // With waveforms of our own.
    assert!((0x20..=0x24).all(|r| c.commands.contains(&r)));
    // After: both memories the frame in black and white.
    assert_eq!(c.old, c.new);
    assert_eq!(c.new, shown.1);
}

/// A display that shows a picture, so that the next fast refresh is one,
/// with a quick experiment set, then the grays written.
fn quick_then_grays() -> (Simulated, Rc<RefCell<Controller>>) {
    let (mut d, c) = display();
    let band = band_with(5, 3);
    d.write_rows(0, view(&band, 8), Planes::BOTH).unwrap();
    d.refresh(Refresh::Full, false).unwrap();
    d.set_experiment(Some(Experiment {
        drive: Direct::new(6, 0),
        swapped: false,
        repeat: 1,
        hold: 3,
        rate: 8,
        own_at: None,
    }));
    let gray = gray_band();
    d.write_gray_rows(0, gray_view(&gray)).unwrap();
    (d, c)
}

#[test]
fn the_grays_start_from_the_panels_own_fast_refresh_not_an_experiment() {
    let (mut d, c) = quick_then_grays();
    d.refresh_gray().unwrap();
    // The full refresh, the fast one before the grays at the temperature
    // of the stock fast waveform, no frame rate set for it.
    assert_eq!(c.borrow().temperatures, [0x1E, 0x5A]);
    assert!(c.borrow().last(cmd::PLL).is_none());
    // The experiment is there for the next one.
    assert!(d.experiment.is_some());
}

#[test]
fn a_cleaning_screen_drives_every_pixel_before_the_grays() {
    let (mut d, c) = quick_then_grays();
    d.refresh_gray_clean().unwrap();
    // Both at the temperature of a half refresh.
    assert_eq!(c.borrow().temperatures, [0x1E, 0x1E]);
    assert!((0x20..=0x24).all(|r| c.borrow().commands.contains(&r)));
    assert!(d.experiment.is_some());
}

#[test]
fn the_refresh_after_grays_drives_every_pixel() {
    let (mut d, c) = display();
    let band = gray_band();
    d.write_gray_rows(0, gray_view(&band)).unwrap();
    d.refresh_gray().unwrap();
    d.refresh(Refresh::Fast, false).unwrap();
    d.refresh(Refresh::Fast, false).unwrap();
    // Fast (the first after init: full), the grays, then a half
    // refresh in place of a fast one, then fast again.
    assert_eq!(c.borrow().temperatures, [0x1E, 0x1E, 0x5A]);
}

#[test]
fn turned_screens_land_upright() {
    for rotation in [
        Rotation::Deg0,
        Rotation::Deg90,
        Rotation::Deg180,
        Rotation::Deg270,
    ] {
        let (w, h) = match rotation {
            Rotation::Deg0 | Rotation::Deg180 => (WIDTH, HEIGHT),
            _ => (HEIGHT, WIDTH),
        };
        for (x, y) in [(0, 0), (w - 1, 0), (0, h - 1), (w - 1, h - 1)] {
            let (sx, sy) = upright(rotation, x, y);
            assert!(sx < WIDTH && sy < HEIGHT, "{rotation:?} {x},{y}");
            assert_eq!(turned(rotation, sx, sy), (x, y), "{rotation:?}");
        }
    }
}

/// The upright screen's pixel `(x, y)` in the controller's memory as
/// addressed: its line and column. Without the frame kept, the screen
/// turns as the X4's (see `bands`).
fn addressed(chip: Chip, rotation: Rotation, x: u16, y: u16) -> (usize, u16) {
    // Where the SSD1677 has it: its row and column.
    let (column, row) = match rotation {
        Rotation::Deg0 => (y, x),
        Rotation::Deg90 => (PANEL_WIDTH - 1 - x, y),
        Rotation::Deg180 => (PANEL_WIDTH - 1 - y, PANEL_HEIGHT - 1 - x),
        Rotation::Deg270 => (x, PANEL_HEIGHT - 1 - y),
        _ => (y, x),
    };
    let line = match chip {
        Chip::Uc8179 => row,
        Chip::Uc8279 => ADDRESSED_LINES - 1 - row,
    };
    (usize::from(line), column)
}

fn white(ram: &[u8], (line, column): (usize, u16)) -> bool {
    ram[line * LINE + usize::from(column / 8)] & (0x80 >> (column % 8)) != 0
}

#[test]
fn a_uc8279_shows_its_last_lines_in_order() {
    let (mut d, c) = display_of(Chip::Uc8279, Frame::Kept);
    // Screen pixel (5, 3) white: frame line 474, sent after the 120
    // lines the panel does not show.
    let band = band_with(5, 3);
    d.write_rows(0, view(&band, 8), Planes::BOTH).unwrap();
    d.refresh(Refresh::Full, false).unwrap();
    let c = c.borrow();
    assert!(c.new[..120 * LINE].iter().all(|b| *b == 0xFF));
    let line = 120 + 474;
    assert_eq!(c.new[line * LINE] & 0x10, 0x10);
    assert_eq!(c.new[line * LINE] & 0x08, 0);
    // Where the bands go without a frame: the same line.
    assert_eq!(addressed(Chip::Uc8279, Rotation::Deg0, 5, 3), (594, 3));
    assert_eq!(addressed(Chip::Uc8179, Rotation::Deg0, 5, 3), (5, 3));
}

#[test]
fn an_x4_classic_keeps_its_factory_frame_rate() {
    let c = Rc::new(RefCell::new(Controller::default()));
    let mut d = Display::new(
        Chip::Uc8279,
        Frame::Kept,
        Spi(c.clone()),
        Dc(c.clone()),
        Pin,
        Pin,
        Delay,
    )
    .with_factory_pll();
    d.init().unwrap();
    assert_eq!(c.borrow().last(cmd::PLL), None);
    assert_eq!(c.borrow().log[0], (cmd::PANEL_SETTING, vec![0x37, 0x4D]));
}

#[test]
fn a_uc8279_is_set_again_after_its_power_comes_on() {
    let (mut d, c) = display_of(Chip::Uc8279, Frame::Kept);
    assert_eq!(c.borrow().log[0], (cmd::PANEL_SETTING, vec![0x37, 0x4D]));
    assert_eq!(c.borrow().last(cmd::PLL), Some(&[0x0E][..]));
    d.refresh(Refresh::Full, false).unwrap();
    assert_eq!(
        c.borrow().since(cmd::VCOM_DATA_INTERVAL),
        [
            cmd::VCOM_DATA_INTERVAL,
            cmd::CASCADE,
            cmd::TEMPERATURE,
            cmd::POWER_ON,
            cmd::PANEL_SETTING,
            cmd::REFRESH,
            cmd::OLD,
        ]
    );
    assert_eq!(c.borrow().last(cmd::VCOM_DATA_INTERVAL), Some(&[0x97][..]));
    // Fast: within a window of the lines the panel shows.
    d.refresh(Refresh::Fast, false).unwrap();
    let c = c.borrow();
    assert_eq!(c.temperatures, [0x1E, 0x5A]);
    assert_eq!(c.last(cmd::VCOM_DATA_INTERVAL), Some(&[0xD7][..]));
    assert_eq!(
        c.last(cmd::PARTIAL_WINDOW),
        Some(&[0x00, 0x00, 0x03, 0x1F, 0x00, 0x78, 0x02, 0x57, 0x01][..])
    );
    assert_eq!(c.last(cmd::PANEL_SETTING), Some(&[0x17, 0x4D][..]));
    let order = c.since(cmd::POWER_OFF_SEQUENCE);
    let at = |command| order.iter().position(|&c| c == command).unwrap();
    assert!(at(cmd::PARTIAL_IN) < at(cmd::PARTIAL_WINDOW));
    assert!(at(cmd::PARTIAL_WINDOW) < at(cmd::PANEL_SETTING));
    assert!(at(cmd::PANEL_SETTING) < at(cmd::REFRESH));
    assert!(at(cmd::REFRESH) < at(cmd::PARTIAL_OUT));
}

#[test]
fn grays_without_their_waveforms_are_black_and_white() {
    for (chip, frame) in [
        (Chip::Uc8279, Frame::Kept),
        (Chip::Uc8179, Frame::InController),
    ] {
        let (mut d, c) = display_of(chip, frame);
        let band = gray_band();
        d.write_gray_rows(0, gray_view(&band)).unwrap();
        d.refresh_gray().unwrap();
        let c = c.borrow();
        // One refresh, every pixel driven, no waveforms of our own.
        assert_eq!(c.refreshed.len(), 1, "{chip:?} {frame:?}");
        assert_eq!(c.temperatures, [0x1E]);
        assert!(!c.commands.contains(&0x21));
        // Black and dark gray black, light gray and white white.
        let levels: Vec<bool> = (0..4)
            .map(|x| white(&c.ram_new, addressed(chip, Rotation::Deg0, x, 0)))
            .collect();
        assert_eq!(levels, [false, false, true, true], "{chip:?} {frame:?}");
    }
}

#[test]
fn without_a_frame_bands_go_into_the_controller_through_windows() {
    let ink = |x: u16, y: u16| (x * 7 + y * 3).is_multiple_of(5) || x == y;
    for chip in [Chip::Uc8179, Chip::Uc8279] {
        for rotation in [
            Rotation::Deg0,
            Rotation::Deg90,
            Rotation::Deg180,
            Rotation::Deg270,
        ] {
            let (mut d, c) = display_of(chip, Frame::InController);
            d.set_rotation(rotation);
            let (width, height) = d.size();
            // Black and white into the new frame, gray into the one
            // shown.
            let stride = usize::from(width) / 8;
            let mut mono = vec![0xFFu8; stride * 16];
            let gray_stride = usize::from(width) / 4;
            let mut gray = vec![0xFFu8; gray_stride * 16];
            for y in 0..16 {
                for x in 0..width {
                    if ink(x, y) {
                        let y = usize::from(y);
                        mono[y * stride + usize::from(x) / 8] &= !(0x80 >> (x % 8));
                        gray[y * gray_stride + usize::from(x) / 4] &= !(0b11 << (6 - 2 * (x % 4)));
                    }
                }
            }
            let view = |data, format, stride| FrameView::new(width, 16, format, stride, data);
            let mono = view(&mono, PixelFormat::Mono1, stride);
            d.write_rows(96, mono, Planes::NEW).unwrap();
            let gray = view(&gray, PixelFormat::Gray2, gray_stride);
            d.write_rows(96, gray, Planes::SHOWN).unwrap();
            let c = c.borrow();
            assert!(!c.partial, "partial mode left");
            for y in 96..112 {
                for x in 0..width {
                    let at = addressed(chip, rotation, x, y);
                    let expected = !ink(x, y - 96);
                    let pixel = (chip, rotation, x, y);
                    assert_eq!(white(&c.ram_new, at), expected, "{pixel:?}");
                    assert_eq!(white(&c.ram_old, at), expected, "{pixel:?}");
                }
            }
            // The rest stays white, as made at the start.
            for (x, y) in [(0, 95), (width - 1, 112), (width / 2, height - 1)] {
                let at = addressed(chip, rotation, x, y);
                assert!(white(&c.ram_new, at), "{chip:?} {rotation:?}");
            }
            drop(c);
            assert_eq!(
                d.write_rows(4, mono, Planes::NEW),
                Err(Error::Rows),
                "not aligned"
            );
        }
    }
}

#[test]
fn without_a_frame_refreshes_send_no_frame() {
    let (mut d, c) = display_of(Chip::Uc8179, Frame::InController);
    let band = band_with(0, 0);
    d.write_rows(0, view(&band, 8), Planes::BOTH).unwrap();
    // The first, full: the frame shown made white; nothing sent after.
    d.refresh(Refresh::Fast, false).unwrap();
    assert_eq!(
        c.borrow().since(cmd::REFRESH),
        [cmd::REFRESH, cmd::VCOM_DATA_INTERVAL]
    );
    assert!(c.borrow().refreshed[0].0.iter().all(|b| *b == 0xFF));
    // Fast, within the panel's lines (not the last band's window).
    d.write_rows(0, view(&band, 8), Planes::NEW).unwrap();
    d.refresh(Refresh::Fast, false).unwrap();
    let c = c.borrow();
    assert_eq!(c.temperatures, [0x1E, 0x5A]);
    assert_eq!(
        c.last(cmd::PARTIAL_WINDOW),
        Some(&[0x00, 0x00, 0x03, 0x1F, 0x00, 0x00, 0x01, 0xDF, 0x01][..])
    );
    let after = c.since(cmd::REFRESH);
    assert!(!after.contains(&cmd::OLD) && !after.contains(&cmd::NEW));
}

/// A display with a picture shown (one pixel white), set to send
/// `transfer`, and the controller's counts of bytes since.
fn shown_with(transfer: Transfer) -> (Simulated, Rc<RefCell<Controller>>) {
    let (mut d, c) = display();
    d.set_transfer(transfer);
    let band = band_with(5, 3);
    d.write_rows(0, view(&band, 8), Planes::BOTH).unwrap();
    d.refresh(Refresh::Full, false).unwrap();
    (d, c)
}

/// Writes the next picture's band (rows 16 to 24, a pixel white) and
/// shows it fast.
fn then_fast(d: &mut Simulated) {
    let band = band_with(40, 3);
    d.write_rows(16, view(&band, 8), Planes::NEW).unwrap();
    d.refresh(Refresh::Fast, false).unwrap();
}

#[test]
fn changed_rows_leave_the_controller_with_the_same_frames() {
    let (mut whole, w) = shown_with(Transfer::Whole);
    then_fast(&mut whole);
    let (mut changed, c) = shown_with(Transfer::Changed);
    then_fast(&mut changed);
    let (w, c) = (w.borrow(), c.borrow());
    // At the refresh, and after it, both memories as with the whole
    // frame sent.
    assert_eq!(c.rams.last(), w.rams.last());
    assert_eq!(c.ram_old, w.ram_old);
    assert_eq!(c.ram_new, w.ram_new);
    // The refresh is over the panel's lines whole, the area only sent:
    // a band of the upright screen is 8 columns of the 480 lines.
    assert_eq!(c.new.len(), 480);
    assert_eq!(c.old.len(), 480);
    assert_eq!(
        c.last(cmd::PARTIAL_WINDOW),
        Some(&[0x00, 0x10, 0x00, 0x17, 0x00, 0x00, 0x01, 0xDF, 0x01][..])
    );
    assert_eq!(w.new.len(), LINE * usize::from(ADDRESSED_LINES));
    let refresh = c.log.iter().rposition(|(c, _)| *c == cmd::REFRESH).unwrap();
    let window = c.log[..refresh]
        .iter()
        .rfind(|(c, _)| *c == cmd::PARTIAL_WINDOW)
        .map(|(_, d)| &d[..]);
    assert_eq!(
        window,
        Some(&[0x00, 0x00, 0x03, 0x1F, 0x00, 0x00, 0x01, 0xDF, 0x01][..])
    );
    // The window of *old* is left again.
    assert_eq!(c.since(cmd::REFRESH).last(), Some(&cmd::PARTIAL_OUT));
}

#[test]
fn a_copied_frame_is_not_sent_again() {
    let (mut d, c) = shown_with(Transfer::Copied);
    then_fast(&mut d);
    let c = c.borrow();
    assert_eq!(c.new.len(), 480);
    let after = c.since(cmd::REFRESH);
    assert!(!after.contains(&cmd::OLD) && !after.contains(&cmd::NEW));
}

#[test]
fn a_fast_refresh_of_nothing_sends_nothing() {
    let (mut d, c) = shown_with(Transfer::Changed);
    d.refresh(Refresh::Fast, false).unwrap();
    let c = c.borrow();
    let after = c.since(cmd::VCOM_DATA_INTERVAL);
    assert!(!after.contains(&cmd::OLD) && !after.contains(&cmd::NEW));
}

#[test]
fn what_is_not_known_to_be_in_the_controller_goes_whole() {
    let whole = LINE * usize::from(ADDRESSED_LINES);
    // After a start.
    let (mut d, c) = display();
    d.set_transfer(Transfer::Changed);
    let band = band_with(5, 3);
    d.write_rows(0, view(&band, 8), Planes::BOTH).unwrap();
    d.refresh(Refresh::Fast, false).unwrap();
    assert_eq!(c.borrow().old.len(), whole);
    // After grays.
    let (mut d, c) = shown_with(Transfer::Changed);
    let gray = gray_band();
    d.write_gray_rows(0, gray_view(&gray)).unwrap();
    d.refresh_gray().unwrap();
    then_fast(&mut d);
    assert_eq!(c.borrow().new.len(), whole);
    // After sleep.
    let (mut d, c) = shown_with(Transfer::Changed);
    d.sleep().unwrap();
    d.init().unwrap();
    then_fast(&mut d);
    assert_eq!(c.borrow().new.len(), whole);
    // Then the area again.
    then_fast(&mut d);
    assert_eq!(c.borrow().new.len(), 480);
}

#[test]
fn the_rotated_screen_sends_its_rows() {
    let (mut d, c) = shown_with(Transfer::Changed);
    d.set_rotation(Rotation::Deg90);
    let (mut w, wc) = shown_with(Transfer::Whole);
    w.set_rotation(Rotation::Deg90);
    for d in [&mut d, &mut w] {
        let stride = usize::from(HEIGHT / 8);
        let data = vec![0x0F; stride * 8];
        let rows = FrameView::new(HEIGHT, 8, PixelFormat::Mono1, stride, &data);
        d.write_rows(24, rows, Planes::NEW).unwrap();
        d.refresh(Refresh::Fast, false).unwrap();
    }
    // The turned screen's band is eight of the panel's 800 columns...
    // across all of them: 8 lines of 100 bytes.
    assert_eq!(c.borrow().new.len(), 8 * LINE);
    assert_eq!(c.borrow().ram_old, wc.borrow().ram_old);
    assert_eq!(c.borrow().ram_new, wc.borrow().ram_new);
}

#[test]
fn partial_windows_are_in_whole_bytes() {
    assert_eq!(
        partial_window(96, 16, 120, 480),
        [0x00, 0x60, 0x00, 0x6F, 0x00, 0x78, 0x02, 0x57, 0x01]
    );
}
