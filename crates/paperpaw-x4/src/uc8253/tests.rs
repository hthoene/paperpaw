//! The UC8253 driver against a simulated controller.

use super::*;
use alloc::rc::Rc;
use alloc::vec;
use core::cell::{Cell, RefCell};
use embedded_hal::spi::{ErrorType, Operation};

/// Lines the controller addresses (`TRES`).
const ADDRESSED_LINES: usize = 600;

/// The controller, simulated: what it was told, its memories and its
/// busy pin.
#[derive(Default)]
struct Controller {
    data: bool,
    command: u8,
    /// Each command with its data, in order.
    log: Vec<(u8, Vec<u8>)>,
    /// The memories as addressed (600 lines of 99 bytes).
    ram_new: Vec<u8>,
    ram_old: Vec<u8>,
    partial: bool,
    window: [u8; 9],
    /// Where the next byte goes: line, byte of the line.
    cursor: (usize, usize),
    /// Reads of the busy pin after a command that makes the
    /// controller busy: high ones first, then low ones (`u32::MAX`
    /// for ever).
    busy_pattern: (u32, u32),
    busy: (u32, u32),
    /// A command came while the controller was busy.
    interrupted: bool,
}

impl Controller {
    fn new(busy_pattern: (u32, u32)) -> Self {
        let size = LINE * ADDRESSED_LINES;
        Self {
            ram_new: vec![0x55; size],
            ram_old: vec![0x55; size],
            busy_pattern,
            ..Self::default()
        }
    }

    fn byte(&mut self, b: u8) {
        if !self.data {
            if self.busy != (0, 0) {
                self.interrupted = true;
            }
            self.command = b;
            self.log.push((b, Vec::new()));
            match b {
                cmd::PARTIAL_IN => self.partial = true,
                cmd::PARTIAL_OUT => self.partial = false,
                cmd::POWER_ON | cmd::POWER_OFF | cmd::REFRESH => {
                    self.busy = self.busy_pattern;
                }
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
            cmd::NEW | cmd::OLD => self.store(b),
            cmd::PARTIAL_WINDOW => {
                let at = self.log.last().map_or(0, |(_, d)| d.len() - 1);
                self.window[at] = b;
            }
            _ => {}
        }
    }

    fn read_busy(&mut self) -> bool {
        let (high, low) = &mut self.busy;
        if *high > 0 {
            *high -= 1;
            true
        } else if *low > 0 {
            if *low != u32::MAX {
                *low -= 1;
            }
            false
        } else {
            true
        }
    }

    /// The bytes and lines written to: first and last byte, first and
    /// last line.
    fn area(&self) -> (usize, usize, usize, usize) {
        if !self.partial {
            return (0, LINE - 1, 0, ADDRESSED_LINES - 1);
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

    /// The data of every `command` sent from the `from`th logged on.
    fn data_of(&self, command: u8, from: usize) -> Vec<&[u8]> {
        self.log[from..]
            .iter()
            .filter(|(c, _)| *c == command)
            .map(|(_, d)| &d[..])
            .collect()
    }
}

struct Spi(Rc<RefCell<Controller>>);
struct Dc(Rc<RefCell<Controller>>);
struct Busy(Rc<RefCell<Controller>>);
struct Pin;
/// Counts the time waited, in nanoseconds.
struct Delay(Rc<Cell<u64>>);

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
impl embedded_hal::digital::ErrorType for Busy {
    type Error = core::convert::Infallible;
}
impl InputPin for Busy {
    fn is_high(&mut self) -> Result<bool, Self::Error> {
        Ok(self.0.borrow_mut().read_busy())
    }
    fn is_low(&mut self) -> Result<bool, Self::Error> {
        Ok(!self.0.borrow_mut().read_busy())
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
impl DelayNs for Delay {
    fn delay_ns(&mut self, ns: u32) {
        self.0.set(self.0.get() + u64::from(ns));
    }
}

type Simulated = Display<Spi, Dc, Pin, Busy, Delay>;

struct Bench {
    display: Simulated,
    controller: Rc<RefCell<Controller>>,
    waited_ns: Rc<Cell<u64>>,
}

impl Bench {
    fn waited_ms(&self) -> u64 {
        self.waited_ns.get() / 1_000_000
    }
}

/// A display set up, its busy pin high for a read, then low for two.
fn display() -> (Simulated, Rc<RefCell<Controller>>) {
    let b = bench((1, 2));
    (b.display, b.controller)
}

fn bench(busy_pattern: (u32, u32)) -> Bench {
    let c = Rc::new(RefCell::new(Controller::new(busy_pattern)));
    let waited_ns = Rc::new(Cell::new(0));
    let mut display = Display::new(
        Spi(c.clone()),
        Dc(c.clone()),
        Pin,
        Busy(c.clone()),
        Delay(waited_ns.clone()),
    );
    display.init().unwrap();
    Bench {
        display,
        controller: c,
        waited_ns,
    }
}

/// A waveform register's data: the table, then zeros.
fn register(table: &Lut) -> Vec<u8> {
    let mut data = vec![0; LUT_LEN];
    data[..12].copy_from_slice(table);
    data
}

const ROTATIONS: [Rotation; 4] = [
    Rotation::Deg0,
    Rotation::Deg90,
    Rotation::Deg180,
    Rotation::Deg270,
];

/// Where the turned screen's pixel `(x, y)` lies in the controller's
/// memory: its line and column.
fn addressed(rotation: Rotation, x: u16, y: u16) -> (usize, u16) {
    // The upright screen's pixel.
    let (ux, uy) = match rotation {
        Rotation::Deg0 => (x, y),
        Rotation::Deg90 => (y, HEIGHT - 1 - x),
        Rotation::Deg180 => (WIDTH - 1 - x, HEIGHT - 1 - y),
        Rotation::Deg270 => (WIDTH - 1 - y, x),
        _ => (x, y),
    };
    (usize::from(ux), uy)
}

fn white(ram: &[u8], (line, column): (usize, u16)) -> bool {
    ram[line * LINE + usize::from(column / 8)] & (0x80 >> (column % 8)) != 0
}

#[test]
fn init_sends_the_sdk_sequence_and_white_frames() {
    let (_, c) = display();
    let c = c.borrow();
    let expected: [(u8, &[u8]); 9] = [
        (cmd::PANEL_SETTING, &[0x3F, 0x0A]),
        (cmd::RESOLUTION, &[0x03, 0x18, 0x02, 0x58]),
        (cmd::GATE_SOURCE_START, &[0x00, 0x00, 0x00, 0x00]),
        (cmd::POWER_OFF_SEQUENCE, &[0x20]),
        (cmd::POWER_SETTING, &[0x07, 0x17, 0x3F, 0x3F, 0x17]),
        (cmd::VCOM_DC, &[0x24]),
        (cmd::BOOSTER_SOFT_START, &[0x25, 0x25, 0x3C, 0x37]),
        (cmd::PLL, &[0x09]),
        (cmd::LV_SELECTION, &[0x02]),
    ];
    for (i, (command, data)) in expected.iter().enumerate() {
        assert_eq!(c.log[i], (*command, data.to_vec()), "command {i}");
    }
    let rest: Vec<u8> = c.log[9..].iter().map(|(c, _)| *c).collect();
    assert_eq!(rest, [cmd::OLD, cmd::DATA_STOP, cmd::NEW, cmd::DATA_STOP]);
    for (_, data) in c.log[9..].iter().step_by(2) {
        assert_eq!(data.len(), PLANE_BYTES);
        assert!(data.iter().all(|b| *b == 0xFF));
    }
}

#[test]
fn the_size_turns() {
    let (mut d, _) = display();
    for (rotation, size) in
        ROTATIONS
            .into_iter()
            .zip([(528, 792), (792, 528), (528, 792), (792, 528)])
    {
        d.set_rotation(rotation);
        assert_eq!(d.rotation(), rotation);
        assert_eq!(d.size(), size, "{rotation:?}");
    }
}

#[test]
fn refreshes_load_the_sdk_waveforms() {
    let (mut d, c) = display();
    // The first after init: full, whatever was asked.
    d.refresh(Refresh::Fast, false).unwrap();
    {
        let c = c.borrow();
        assert_eq!(c.data_of(cmd::VCOM_DATA_INTERVAL, 0), [&[0x29, 0x07]]);
        // By the new colour alone: white as white to white, black as
        // white to black.
        let tables = [
            &lut::VCOM_FULL,
            &lut::WW_FULL,
            &lut::WW_FULL,
            &lut::WB_FULL,
            &lut::WB_FULL,
        ];
        for (at, table) in (0x20..=0x24).zip(tables) {
            assert_eq!(c.data_of(at, 0), [&register(table)[..]]);
        }
        assert_eq!(
            c.data_of(cmd::PARTIAL_WINDOW, 0),
            [&[0x00, 0x00, 0x03, 0x17, 0x00, 0x00, 0x02, 0x0F, 0x01]]
        );
    }
    // The next fast one: the SDK's pass after the first full refresh,
    // the settling, then fast.
    let from = c.borrow().log.len();
    d.refresh(Refresh::Fast, false).unwrap();
    {
        let c = c.borrow();
        assert_eq!(
            c.data_of(cmd::VCOM_DATA_INTERVAL, from),
            [&[0xA9, 0x07], &[0x29, 0x07], &[0x29, 0x07]]
        );
        assert_eq!(c.data_of(cmd::REFRESH, from).len(), 3);
        let fast = c.data_of(0x22, from);
        assert_eq!(fast[0], &register(&lut::WB_NORMAL)[..]);
        assert_eq!(fast[1], &register(&lut::BB_FAST)[..]);
        assert_eq!(fast[2], &register(&lut::BW_FAST)[..]);
        // As in `Uc8253X3Luts.h`: the fast waveform's black to white.
        assert_eq!(
            fast[2][..12],
            [
                0xAA, 0x04, 0x02, 0x04, 0x04, 0x01, 0x80, 0x04, 0x01, 0x00, 0x00, 0x01
            ]
        );
        assert!(fast[2][12..].iter().all(|b| *b == 0));
    }
    // Then fast alone; a half one with its own waveforms.
    let from = c.borrow().log.len();
    d.refresh(Refresh::Fast, false).unwrap();
    assert_eq!(c.borrow().data_of(cmd::REFRESH, from).len(), 1);
    let from = c.borrow().log.len();
    d.refresh(Refresh::Half, false).unwrap();
    let c = c.borrow();
    assert_eq!(c.data_of(cmd::VCOM_DATA_INTERVAL, from), [&[0xA9, 0x07]]);
    assert_eq!(c.data_of(0x21, from), [&register(&lut::WW_HALF)[..]]);
    assert_eq!(c.data_of(0x24, from), [&register(&lut::BB_HALF)[..]]);
}

#[test]
fn a_waking_panel_gets_a_half_refresh() {
    let (mut d, c) = display();
    d.refresh(Refresh::Full, false).unwrap();
    d.power_down().unwrap();
    assert!(!d.powered());
    let from = c.borrow().log.len();
    d.refresh(Refresh::Fast, false).unwrap();
    assert!(d.powered());
    let c = c.borrow();
    // Half: no settling (a half refresh drives every pixel).
    assert_eq!(c.data_of(cmd::VCOM_DATA_INTERVAL, from), [&[0xA9, 0x07]]);
    assert_eq!(c.data_of(cmd::POWER_ON, from).len(), 1);
}

/// A band of the turned screen: black where `ink` says, in black and
/// white or four grays.
fn band(width: u16, height: u16, format: PixelFormat, ink: &dyn Fn(u16, u16) -> bool) -> Vec<u8> {
    let bpp = usize::from(format.bits_per_pixel());
    let stride = usize::from(width) * bpp / 8;
    let mut data = vec![0xFFu8; stride * usize::from(height)];
    for y in 0..height {
        for x in 0..width {
            if ink(x, y) {
                let bit = usize::from(x) * bpp;
                let at = usize::from(y) * stride + bit / 8;
                let mask = (((1u16 << bpp) - 1) as u8) << (8 - bpp - bit % 8);
                data[at] &= !mask;
            }
        }
    }
    data
}

fn view(data: &[u8], width: u16, height: u16, format: PixelFormat) -> FrameView<'_> {
    FrameView::new(
        width,
        height,
        format,
        data.len() / usize::from(height),
        data,
    )
}

#[test]
fn a_whole_screen_is_a_frame_in_each_memory() {
    let ink = |x: u16, y: u16| (x + 2 * y).is_multiple_of(7);
    for rotation in ROTATIONS {
        let (mut d, c) = display();
        d.set_rotation(rotation);
        let (width, height) = d.size();
        let from = c.borrow().log.len();
        for top in (0..height).step_by(40) {
            let rows = 40.min(height - top);
            let data = band(width, rows, PixelFormat::Mono1, &|x, y| ink(x, top + y));
            let rows = view(&data, width, rows, PixelFormat::Mono1);
            d.write_rows(top, rows, Planes::BOTH).unwrap();
        }
        let c = c.borrow();
        for memory in [cmd::NEW, cmd::OLD] {
            let sent: usize = c.data_of(memory, from).iter().map(|d| d.len()).sum();
            assert_eq!(sent, PLANE_BYTES, "{rotation:?}");
        }
        for y in 0..height {
            for x in 0..width {
                let at = addressed(rotation, x, y);
                assert_eq!(white(&c.ram_new, at), !ink(x, y), "{rotation:?} {x},{y}");
                assert_eq!(white(&c.ram_old, at), !ink(x, y), "{rotation:?} {x},{y}");
            }
        }
    }
}

#[test]
fn bands_land_where_the_turn_says() {
    let ink = |x: u16, y: u16| (x * 7 + y * 3).is_multiple_of(5) || x == y;
    for rotation in ROTATIONS {
        for format in [PixelFormat::Mono1, PixelFormat::Gray2] {
            let (mut d, c) = display();
            d.set_rotation(rotation);
            let (width, height) = d.size();
            // A band of 13 rows: the last byte of each line padded.
            let data = band(width, 13, format, &ink);
            let rows = view(&data, width, 13, format);
            d.write_rows(96, rows, Planes::NEW).unwrap();
            let c = c.borrow();
            assert!(!c.partial, "partial mode left");
            for y in 96..109 {
                for x in 0..width {
                    let at = addressed(rotation, x, y);
                    let pixel = (rotation, format, x, y);
                    assert_eq!(white(&c.ram_new, at), !ink(x, y - 96), "{pixel:?}");
                }
            }
            // The rest stays white, as made at the start; the frame
            // shown untouched.
            for (x, y) in [(0, 95), (width - 1, 109), (width / 2, height - 1)] {
                let at = addressed(rotation, x, y);
                assert!(white(&c.ram_new, at), "{rotation:?} {x},{y}");
            }
            assert!(c.ram_old[..PLANE_BYTES].iter().all(|b| *b == 0xFF));
            drop(c);
            assert_eq!(
                d.write_rows(4, rows, Planes::NEW),
                Err(Error::Rows),
                "not aligned"
            );
        }
    }
}

#[test]
fn grays_are_black_and_white() {
    let (mut d, c) = display();
    // Black, dark gray, light gray, white.
    let mut data = vec![0xFFu8; usize::from(WIDTH) / 4 * 8];
    data[0] = 0b00_01_10_11;
    let rows = view(&data, WIDTH, 8, PixelFormat::Gray2);
    d.write_gray_rows(0, rows).unwrap();
    let from = c.borrow().log.len();
    d.refresh_gray().unwrap();
    let c = c.borrow();
    let levels: Vec<bool> = (0..4)
        .map(|x| white(&c.ram_new, addressed(Rotation::Deg0, x, 0)))
        .collect();
    assert_eq!(levels, [false, false, true, true]);
    assert_eq!(
        levels,
        (0..4)
            .map(|x| white(&c.ram_old, (x, 0)))
            .collect::<Vec<_>>()
    );
    // One refresh (the first after init, so full).
    assert_eq!(c.data_of(cmd::REFRESH, from).len(), 1);
    drop(c);
    let mono = band(WIDTH, 8, PixelFormat::Mono1, &|_, _| false);
    let mono = view(&mono, WIDTH, 8, PixelFormat::Mono1);
    assert_eq!(d.write_gray_rows(0, mono), Err(Error::Rows));
}

#[test]
fn the_busy_pin_is_waited_for_in_two_phases() {
    // High for a while after each command, then low for a while: every
    // command after waits for both.
    let mut b = bench((5, 7));
    b.display.refresh(Refresh::Full, true).unwrap();
    b.display.refresh(Refresh::Fast, false).unwrap();
    b.display.sleep().unwrap();
    let c = b.controller.borrow();
    assert!(!c.interrupted, "told more while busy");
    assert_eq!(c.busy, (0, 0));
}

#[test]
fn a_busy_pin_that_never_falls_is_not_waited_for_long() {
    // A command done before it was seen: about a second a wait.
    let mut b = bench((0, 0));
    b.display.refresh(Refresh::Full, false).unwrap();
    // Power on, then the refresh.
    assert!(
        b.waited_ms() <= 2 * u64::from(BUSY_EDGE_MS) + 500,
        "{}",
        b.waited_ms()
    );
}

#[test]
fn work_is_done_in_the_rounds_of_a_wait_that_goes_on() {
    let mut b = bench((1, 300));
    b.display.start_refresh(Refresh::Fast, false).unwrap();
    let mut rounds = 0u32;
    b.display.wait_with(|| rounds += 1).unwrap();
    // Busy for about 300 readings: a round for each.
    assert!(rounds >= 200, "{rounds}");
    // And none for a wait with nothing to wait for.
    let mut more = 0u32;
    b.display.wait_with(|| more += 1).unwrap();
    assert_eq!(more, 0);
}

#[test]
fn a_busy_pin_that_sticks_ends_in_an_error() {
    let mut b = bench((0, u32::MAX));
    assert_eq!(b.display.refresh(Refresh::Full, false), Err(Error::Timeout));
    let limit = u64::from(BUSY_TIMEOUT_MS + BUSY_EDGE_MS) + 500;
    assert!(b.waited_ms() <= limit, "{}", b.waited_ms());
    // Not stuck after: the next refresh drives every pixel.
    b.controller.borrow_mut().busy_pattern = (1, 2);
    b.controller.borrow_mut().busy = (0, 0);
    let from = b.controller.borrow().log.len();
    b.display.refresh(Refresh::Fast, false).unwrap();
    let c = b.controller.borrow();
    assert_eq!(c.data_of(0x21, from), [&register(&lut::WW_FULL)[..]]);
}

#[test]
fn partial_windows_are_in_whole_bytes() {
    assert_eq!(
        Window::of(Rotation::Deg0, 96, 13).command(),
        [0x00, 0x60, 0x00, 0x6F, 0x00, 0x00, 0x02, 0x0F, 0x01]
    );
    assert_eq!(
        Window::of(Rotation::Deg180, 0, 8).command(),
        [0x03, 0x10, 0x03, 0x17, 0x00, 0x00, 0x02, 0x0F, 0x01]
    );
    assert_eq!(
        Window::of(Rotation::Deg270, 0, 40).command(),
        [0x00, 0x00, 0x03, 0x17, 0x01, 0xE8, 0x02, 0x0F, 0x01]
    );
}

#[test]
fn a_uc8279d_is_unshown_until_a_refresh_ends() {
    // The busy pin sticks: its first refresh times out, and the module
    // counts as one this build cannot show on (an update on trial goes back).
    let c = Rc::new(RefCell::new(Controller::new((0, u32::MAX))));
    let mut panel = Display::new(
        Spi(c.clone()),
        Dc(c.clone()),
        Pin,
        Busy(c.clone()),
        Delay(Rc::new(Cell::new(0))),
    )
    .uc8279d();
    panel.init().unwrap();
    assert!(panel.uc8279d_unshown());
    assert_eq!(panel.refresh(Refresh::Full, false), Err(Error::Timeout));
    assert!(panel.uc8279d_unshown());
    // A refresh that ends: shown.
    c.borrow_mut().busy_pattern = (1, 2);
    c.borrow_mut().busy = (0, 0);
    panel.refresh(Refresh::Full, false).unwrap();
    assert!(!panel.uc8279d_unshown());
    // A UC8253 is never one.
    assert!(!display().0.uc8279d_unshown());
}
