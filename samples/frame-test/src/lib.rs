//! A sample that runs in real time ([ADR 0030]): a moving pattern at a
//! fixed pace, drawn with [`Canvas::blit`], and a fixed amount of work
//! for every frame, as a game or an emulator would do. It measures what
//! counts for such plugins and shows it:
//!
//! - the frames a second the screen shows ([`Env::panel`]);
//! - the work per frame, as hot code (`paperpaw_sdk::hot!`, in internal
//!   RAM where the device runs it from there) and as ordinary code;
//! - how long drawing the pattern takes;
//! - the heap free.
//!
//! The work is a small interpreter running a made-up program over its
//! memory: fetching, decoding, branching, much as an emulator's core.
//!
//! [ADR 0030]: https://github.com/hthoene/paperpaw/blob/main/docs/adr/0030-real-time-screens.md

#![no_std]

extern crate alloc;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::format;
use alloc::vec::Vec;
use core::time::Duration;

use paperpaw_core::Items;
use paperpaw_sdk::builtin::menus;
use paperpaw_sdk::memory::{self, Placement};
use paperpaw_sdk::realtime::{KeyEvent, ScreenMode};
use paperpaw_sdk::ui::{self, UiStyle};
use paperpaw_sdk::{
    ActionSpec, Bitmap, BitmapFormat, Blit, Canvas, Command, Cx, Dither, Env, Handled, Id, Instant,
    Manifest, MenuEntry, Params, Point, Rect, RefreshMode, RegisterError, Screen, ScreenSpec,
    Version,
};

pub const PLUGIN: Id = Id::new_static("frame-test");
pub const OPEN: Id = Id::new_static("frame-test.open");
pub const SCREEN: Id = Id::new_static("frame-test.screen");

/// The pattern's size, as a small handheld's screen, and its scale.
pub const WIDTH: u16 = 160;
pub const HEIGHT: u16 = 144;
const SCALE: u8 = 3;
/// The pace asked for.
pub const FRAME: Duration = Duration::from_millis(100);
/// The interpreter's steps a frame at first.
const STEPS: u32 = 20_000;
/// Its memory, and its program's length.
const MEMORY: usize = 8192;
const PROGRAM: usize = 256;
/// The memory it asks to have free, as an emulator would for its own
/// screen and memory (see `Env::room`).
pub const ROOM: u32 = 64 * 1024;
/// The four grays of the pattern, from white to black.
const PALETTE: [u8; 4] = [255, 170, 85, 0];

/// The plugin.
#[derive(Debug, Default)]
pub struct FrameTest;

static HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/en.txt")),
    ("de", include_str!("../how/de.txt")),
];

impl FrameTest {
    /// What it registers under.
    #[must_use]
    pub fn manifest(&self) -> Manifest {
        Manifest::new(PLUGIN, "Frame test", Version::new(0, 1, 0))
            .description("Runs in real time and measures it.")
            .how(&HOW)
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    /// Adds its action, menu entry, screen and texts.
    ///
    /// # Errors
    /// What the kernel refuses of them.
    pub fn register(&self, r: &mut Items) -> Result<(), RegisterError> {
        r.add_action(
            ActionSpec::new(OPEN, "frame-test.title").handler(|_, _, out| {
                out.push(Command::OpenScreen(SCREEN, Params::new()));
            }),
        )?;
        r.add_menu_entry(MenuEntry::new(menus::TOOLS, OPEN).order(95))?;
        r.add_icon(OPEN, Cow::Borrowed("device-gamepad"))?;
        r.add_screen(ScreenSpec::new(
            SCREEN,
            "frame-test.title",
            Box::new(|env, _| Box::new(FrameScreen::new(env))),
        ))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
        Ok(())
    }
}

/// The interpreter: eight registers, a program and memory to work on.
struct Machine {
    registers: [u8; 8],
    pc: usize,
    program: Vec<u8>,
    memory: Vec<u8>,
}

impl Machine {
    /// A made-up program of `PROGRAM` instructions, the same every time.
    fn new() -> Self {
        let mut seed = 0x1234_5678u32;
        let mut next = move || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 24) as u8
        };
        let program = (0..PROGRAM).map(|_| next()).collect();
        // Read by every step: kept in internal RAM where there is more
        // than one kind.
        let mut memory = memory::vec_in(Placement::FAST, MEMORY).unwrap_or_default();
        memory.resize(MEMORY, 0);
        Self {
            registers: [0; 8],
            pc: 0,
            program,
            memory,
        }
    }
}

/// One step of the interpreter: fetch, decode, act, branch.
#[inline(always)]
fn step(m: &mut Machine) {
    let op = m.program[m.pc % PROGRAM];
    let a = usize::from(op & 7);
    let b = usize::from((op >> 3) & 7);
    let at = (usize::from(m.registers[a]) << 5 | usize::from(m.registers[b])) % MEMORY;
    match op >> 6 {
        0 => m.registers[a] = m.registers[a].wrapping_add(m.registers[b] ^ op),
        1 => m.registers[a] = m.memory[at],
        2 => m.memory[at] = m.registers[a].rotate_left(u32::from(op & 3)),
        _ if m.registers[a] & 1 == 0 => m.pc = m.pc.wrapping_add(usize::from(m.registers[b])),
        _ => {}
    }
    m.pc = m.pc.wrapping_add(1);
}

paperpaw_sdk::hot! {
    /// `steps` steps, from internal RAM where the device runs hot code
    /// there.
    #[inline(never)]
    fn run_hot(m: &mut Machine, steps: u32) -> u8 {
        for _ in 0..steps {
            step(m);
        }
        m.registers[0]
    }
}

/// The same, as ordinary code (from flash on the X4).
#[inline(never)]
fn run_cold(m: &mut Machine, steps: u32) -> u8 {
    for _ in 0..steps {
        step(m);
    }
    m.registers[0]
}

/// What was measured, shown below the pattern.
#[derive(Debug, Default, Clone, Copy)]
struct Measured {
    /// Microseconds of the last frame's work, hot and ordinary.
    hot_us: Option<u64>,
    cold_us: Option<u64>,
    /// Microseconds drawing the pattern took, in the last frame.
    blit_us: u64,
    /// Frames shown in the last second.
    fps: u32,
}

/// The screen: the pattern, the square, what was measured.
struct FrameScreen {
    style: UiStyle,
    machine: Machine,
    pixels: Vec<u8>,
    /// Frames drawn, the square's place, the work a frame, its kind.
    frame: u32,
    square: u16,
    steps: u32,
    hot: bool,
    measured: Measured,
    /// Drawing time added up over the bands of the frame being drawn.
    drawing_us: u64,
    /// When the frames of the last second were shown.
    shown: Vec<Instant>,
}

impl FrameScreen {
    fn new(env: &Env<'_>) -> Self {
        let bytes = BitmapFormat::GRAY2.stride(WIDTH) * usize::from(HEIGHT);
        let mut pixels = memory::vec_in(Placement::FAST, bytes).unwrap_or_default();
        pixels.resize(bytes, 0);
        Self {
            style: UiStyle::new(env),
            machine: Machine::new(),
            pixels,
            frame: 0,
            square: WIDTH / 2,
            steps: STEPS,
            hot: true,
            measured: Measured::default(),
            drawing_us: 0,
            shown: Vec::new(),
        }
    }

    /// The pattern's place: across the top, as wide as it can be at its
    /// scale.
    fn picture(env: &Env<'_>) -> Rect {
        let scale = u16::from(SCALE);
        let width = env.device().display.width;
        let x = width.saturating_sub(WIDTH * scale) / 2;
        Rect::new(x, 0, WIDTH * scale, HEIGHT * scale)
    }

    /// Where what was measured is written: below the pattern.
    fn notes(env: &Env<'_>) -> Rect {
        let display = env.device().display;
        let top = HEIGHT * u16::from(SCALE);
        Rect::new(0, top, display.width, display.height.saturating_sub(top))
    }

    /// Stripes moving with the frames, and the square.
    fn paint(&mut self) {
        let stride = BitmapFormat::GRAY2.stride(WIDTH);
        let shift = self.frame as usize;
        for y in 0..usize::from(HEIGHT) {
            let row = &mut self.pixels[y * stride..(y + 1) * stride];
            for (i, byte) in row.iter_mut().enumerate() {
                let mut packed = 0u8;
                for k in 0..4 {
                    let x = i * 4 + k;
                    let gray = ((x + y + shift) / 8) % 4;
                    packed |= (gray as u8) << (6 - 2 * k);
                }
                *byte = packed;
            }
        }
        let (left, top) = (usize::from(self.square.min(WIDTH - 16)), 64);
        for y in top..top + 16 {
            for x in left..left + 16 {
                let byte = &mut self.pixels[y * stride + x / 4];
                *byte |= 3 << (6 - 2 * (x % 4));
            }
        }
    }

    /// A frame: the work, then the pattern moved; asks for the next one.
    fn next_frame(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let clock = env.clock();
        let started = clock.micros();
        let result = if self.hot {
            run_hot(&mut self.machine, self.steps)
        } else {
            run_cold(&mut self.machine, self.steps)
        };
        if let (Some(started), Some(ended)) = (started, clock.micros()) {
            let took = Some(ended.saturating_sub(started));
            match self.hot {
                true => self.measured.hot_us = took,
                false => self.measured.cold_us = took,
            }
        }
        // The square jumps a little with the work's result, so the work
        // cannot be left out.
        self.square = (self.square + u16::from(result & 1)) % (WIDTH - 16);
        self.frame = self.frame.wrapping_add(1);
        self.paint();
        self.count_shown(&env);
        self.measured.blit_us = core::mem::take(&mut self.drawing_us);
        cx.request_redraw();
        cx.wake_at(env.now().add_millis(FRAME.as_millis() as u64));
    }

    /// Counts the frames shown in the last second, as the panel reports
    /// them.
    fn count_shown(&mut self, env: &Env<'_>) {
        if let Some(frame) = env.panel().last_frame()
            && self.shown.last() != Some(&frame.at())
        {
            self.shown.push(frame.at());
        }
        let now = env.now();
        self.shown.retain(|at| now.since(*at) < 1000);
        self.measured.fps = self.shown.len() as u32;
    }

    fn text(&self, env: &Env<'_>) -> alloc::string::String {
        let us = |value: Option<u64>| value.map_or_else(|| "-".into(), |v| format!("{v} us"));
        let heap = env
            .memory()
            .read()
            .map_or_else(|| "-".into(), |m| format!("{} KB", m.heap_free / 1024));
        let fastest = env.panel().fastest().map_or_else(
            || "-".into(),
            |(_, time)| format!("{} ms", time.as_millis()),
        );
        let room = env.room();
        let room = match (room.is_granted(), room.free()) {
            (_, None) => "room: not measured".into(),
            (true, Some(free)) => format!("room: {} KB free, granted", free / 1024),
            (false, Some(free)) => {
                format!("room: {} KB free of {} KB asked", free / 1024, ROOM / 1024)
            }
        };
        format!(
            "{} fps (refresh {fastest})\nwork {} steps: hot {}, ordinary {}{}\ndrawing {} us\nheap free {heap}\n{room}",
            self.measured.fps,
            self.steps,
            us(self.measured.hot_us),
            us(self.measured.cold_us),
            if self.hot { " (hot)" } else { " (ordinary)" },
            self.measured.blit_us,
        )
    }
}

impl Screen for FrameScreen {
    fn mode(&self) -> ScreenMode {
        ScreenMode::new()
            .full_screen()
            .raw_keys()
            .cleans_itself()
            .fastest()
            .grays(2)
            .frame_every(FRAME)
            .awake_for(Duration::from_secs(600))
            .memory(ROOM)
    }

    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let picture = Self::picture(env);
        if let Ok(bitmap) = Bitmap::new(BitmapFormat::GRAY2, WIDTH, HEIGHT, &self.pixels)
            .and_then(|b| b.palette(&PALETTE))
        {
            let clock = env.clock();
            let started = clock.micros();
            let at = Point::new(i32::from(picture.x), i32::from(picture.y));
            let blit = Blit::at(at).scale(SCALE).dither(Dither::ORDERED);
            canvas.blit(&bitmap, blit);
            if let (Some(started), Some(ended)) = (started, clock.micros()) {
                self.drawing_us += ended.saturating_sub(started);
            }
        }
        let text = self.text(env);
        let mut ui = ui::Ui::new(canvas, env, &self.style);
        let notes = Self::notes(env);
        let margin = self.style.margin();
        let area = Rect::new(
            notes.x + margin,
            notes.y + margin,
            notes.width.saturating_sub(2 * margin),
            notes.height.saturating_sub(2 * margin),
        );
        ui::Label::new(&text).wrap().draw(&mut ui, area);
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        cx.wake_at(cx.env().now());
    }

    fn on_timer(&mut self, cx: &mut Cx<'_, '_>) {
        self.next_frame(cx);
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        // Without raw keys (an older kernel): back leaves.
        if *action == paperpaw_sdk::builtin::actions::BACK {
            cx.command(Command::Back);
            return Handled::Yes;
        }
        Handled::No
    }

    fn on_key(&mut self, key: &KeyEvent, cx: &mut Cx<'_, '_>) -> Handled {
        if !key.is_press() {
            if key.name() == "back" {
                cx.command(Command::Back);
            }
            return Handled::Yes;
        }
        match key.name() {
            "left" => self.square = self.square.saturating_sub(8),
            "right" => self.square = (self.square + 8).min(WIDTH - 16),
            "up" => self.steps = self.steps.saturating_mul(2).min(1 << 22),
            "down" => self.steps = (self.steps / 2).max(1000),
            "confirm" => self.hot = !self.hot,
            "page-forward" => cx.request_refresh(RefreshMode::Full),
            "back" => {}
            _ => return Handled::No,
        }
        Handled::Yes
    }
}
