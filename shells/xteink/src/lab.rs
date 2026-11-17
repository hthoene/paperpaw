//! The waveform lab: a screen to try, on the device, how the panel is
//! driven (Tools, *Waveform lab*; for panels with an SSD1677 or a UC8179
//! controller, and only in the developer firmware, the feature
//! `waveform-lab`, which the `x4` and `x4pro` builds have).
//!
//! A test picture (stripes, squares, halves or lines) is flipped between
//! its two states, so that pixels change both ways, and each flip is shown
//! with the waveform chosen here instead of the panel's own fast one:
//!
//! - *Panel's own*: the stock fast refresh, to compare with.
//! - *Panel's, at a temperature*: the panel's own tables, made by the
//!   vendor, as the controller takes them for a panel at that temperature
//!   (an SSD1677: 0 to 90 degrees, the stock half refresh asks for 90; a
//!   UC8179: up to 120, its fast refresh asks for 90, which is the range
//!   of 80 to 90 degrees, and the ranges above are others). The warmer the
//!   faster, and the less complete at a cool panel.
//! - *Direct drive*: a waveform made here (`paperpaw_x4::waveform`): a
//!   pixel that changes is driven to its new colour for some frames, after
//!   a kick of the other colour for up to three. The panel's own voltages;
//!   the frames and the kick are what is tried.
//!
//! On a UC8179 (the X4 Pro) the direct drive has more: the frame rate
//! (the controller's `PLL` register, 20 to 200 Hz, its own 50), the group's
//! repeat (0 or 1; the datasheet says "0: 0 time", which may mean that the
//! group does not run at all, and then nothing is driven and the ghosts
//! pile up; the stock gray waveforms use 1, which the lab starts at), and the levels of white and black, assumed from the stock
//! gray waveform and turned round if the picture comes out the wrong way.
//!
//! The rows are the ordinary ones: a setting each (developer group, so they
//! are kept and are in the settings too), with `−` and `+` where there is
//! a touch screen, left and right on the buttons; *Select* or a tap on the
//! picture flips it. *Clean up* shows everything with the stock refresh
//! that drives every pixel, which clears what the experiments left behind.
//! The time the panel took for the last frame is told above the rows.
//!
//! The waveform chosen counts only while the lab is the screen shown, and
//! only for the picture's flips (the fast refreshes): the rows are shown
//! the panel's own way, so the lab stays readable whatever is tried. Leaving
//! it, or a restart, is the stock firmware again.

use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU8, Ordering};

use paperpaw_sdk::builtin::{actions, groups, menus};
use paperpaw_sdk::realtime::ScreenMode;
use paperpaw_sdk::ui::{self, ListView, RowPart, UiStyle};
use paperpaw_sdk::{
    Canvas, Color, Command, Cx, Env, FixedAction, Gesture, GestureKind, Handled, Id, Instances,
    MenuEntry, Params, Rect, RefreshMode, SettingExtra, SettingSpec, SettingValue, Settings,
};
use paperpaw_x4::display::{self, TEMPERATURE_RANGE};
use paperpaw_x4::panel::Experiment;
use paperpaw_x4::uc8179::{self, FRAME_RATES};
use paperpaw_x4::waveform::{Direct, MAX_FRAMES, MAX_KICK};

pub const SCREEN: Id = Id::new_static("x4.waveform-lab");
pub const OPEN: Id = Id::new_static("x4.open-waveform-lab");

/// The lab's settings: the waveform and its numbers.
const WAVEFORM: &str = "x4.lab.waveform";
const TEMPERATURE: &str = "x4.lab.temperature";
const FRAMES: &str = "x4.lab.frames";
const KICK: &str = "x4.lab.kick";
const REPEAT: &str = "x4.lab.repeat";
const HOLD: &str = "x4.lab.hold";
const RATE: &str = "x4.lab.rate";
const SWAPPED: &str = "x4.lab.swapped";

/// The choices of the waveform.
const OWN: &str = "x4.lab.waveform.panel";
const BY_TEMPERATURE: &str = "x4.lab.waveform.temperature";
const DIRECT: &str = "x4.lab.waveform.direct";

static BY_FIVE: SettingExtra = SettingExtra::new().step(5);
static BY_TEN: SettingExtra = SettingExtra::new().step(10);

/// The settings the lab keeps (registered by `device`).
pub static SETTINGS: [SettingSpec; 8] = [
    SettingSpec::choice(
        WAVEFORM,
        "x4.lab.waveform",
        &[OWN, BY_TEMPERATURE, DIRECT],
        OWN,
        groups::DEVELOPER,
    ),
    SettingSpec::number(
        TEMPERATURE,
        "x4.lab.temperature",
        TEMPERATURE_RANGE.0 as i64,
        // The SSD1677 takes up to 90 (`TEMPERATURE_RANGE`); a UC8179's
        // ranges reach 127 degrees (the vendor's go to 127).
        120,
        25,
        groups::DEVELOPER,
    )
    .with_extra(&BY_FIVE),
    SettingSpec::number(
        FRAMES,
        "x4.lab.frames",
        1,
        MAX_FRAMES as i64,
        10,
        groups::DEVELOPER,
    ),
    SettingSpec::number(
        KICK,
        "x4.lab.kick",
        0,
        MAX_KICK as i64,
        0,
        groups::DEVELOPER,
    ),
    SettingSpec::number(REPEAT, "x4.lab.repeat", 0, 1, 1, groups::DEVELOPER),
    SettingSpec::number(
        RATE,
        "x4.lab.rate",
        FRAME_RATES[0].1 as i64,
        FRAME_RATES[FRAME_RATES.len() - 1].1 as i64,
        FRAME_RATES[uc8179::OWN_RATE].1 as i64,
        groups::DEVELOPER,
    )
    .with_extra(&BY_TEN),
    SettingSpec::switch(SWAPPED, "x4.lab.polarity", false, groups::DEVELOPER),
    SettingSpec::number(HOLD, "x4.lab.hold", 0, 3, 1, groups::DEVELOPER),
];

/// After this many flips without a clean-up the lab says to clean up, and
/// a picture that flips on its own stops after the second number.
const CLEAN_AFTER: u32 = 20;
const AUTO_STOPS_AFTER: u32 = 60;

/// What the panel can do: only the waveforms of the SSD1677 (the X4, some
/// X4 Pros) and of the UC8179 (the X4 Pro) can be tried.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Support {
    None,
    Ssd1677,
    Uc8179,
}

static SUPPORT: AtomicU8 = AtomicU8::new(0);

/// The firmware says what this panel takes (at start).
pub fn set_support(support: Support) {
    SUPPORT.store(
        match support {
            Support::None => 0,
            Support::Ssd1677 => 1,
            Support::Uc8179 => 2,
        },
        Ordering::Relaxed,
    );
}

fn support() -> Support {
    match SUPPORT.load(Ordering::Relaxed) {
        1 => Support::Ssd1677,
        2 => Support::Uc8179,
        _ => Support::None,
    }
}

/// The waveforms the panel offers, the panel's own first.
fn waveforms() -> &'static [&'static str] {
    match support() {
        Support::Ssd1677 => &[OWN, BY_TEMPERATURE, DIRECT],
        Support::Uc8179 => &[OWN, BY_TEMPERATURE, DIRECT],
        Support::None => &[OWN],
    }
}

/// Whether the lab is the screen shown (`active` is the screen on top).
pub fn is_open(active: Option<&Id>) -> bool {
    active == Some(&SCREEN)
}

/// How a fast refresh is driven while the lab is open: as its settings say,
/// or (`None`) the panel's own.
pub fn experiment(active: Option<&Id>, settings: &Settings) -> Option<Experiment> {
    if !is_open(active) {
        return None;
    }
    let number = |id: &str| u8::try_from(settings.int(id).clamp(0, 255)).unwrap_or(u8::MAX);
    let drive = || Direct::new(number(FRAMES), number(KICK));
    match (support(), settings.text(WAVEFORM)) {
        (Support::Ssd1677, BY_TEMPERATURE) => Some(Experiment::Ssd1677(
            display::Experiment::Temperature(settings.int(TEMPERATURE).clamp(0, 127) as i8),
        )),
        (Support::Ssd1677, DIRECT) => {
            Some(Experiment::Ssd1677(display::Experiment::Direct(drive())))
        }
        (Support::Uc8179, BY_TEMPERATURE) => Some(Experiment::Uc8179(uc8179::Experiment {
            drive: drive(),
            swapped: false,
            repeat: 1,
            hold: 0,
            rate: uc8179::OWN_RATE,
            own_at: Some(settings.int(TEMPERATURE).clamp(-25, 127) as i8),
        })),
        (Support::Uc8179, DIRECT) => {
            let hz = settings.int(RATE);
            let rate = FRAME_RATES
                .iter()
                .position(|&(_, rate)| i64::from(rate) >= hz)
                .unwrap_or(FRAME_RATES.len() - 1);
            Some(Experiment::Uc8179(uc8179::Experiment {
                drive: drive(),
                swapped: settings.bool(SWAPPED),
                repeat: number(REPEAT).min(1),
                hold: number(HOLD).min(3),
                rate,
                own_at: None,
            }))
        }
        _ => None,
    }
}

pub static ACTIONS: [FixedAction; 1] = [FixedAction::new(OPEN, "x4.lab.title")
    .handler(|_, _, out| out.push(Command::OpenScreen(SCREEN, Params::new())))];

pub static ENTRIES: [MenuEntry; 1] = [MenuEntry::new(menus::TOOLS, OPEN).order(97)];

pub static SCREENS: [paperpaw_core::KernelScreen; 1] =
    [
        paperpaw_core::KernelScreen::new(SCREEN, "x4.lab.title", |env, _| Box::new(Lab::new(env)))
            .instances(Instances::SINGLE),
    ];

/// What a row of the lab does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Control {
    Waveform,
    Temperature,
    Frames,
    Kick,
    Repeat,
    Rate,
    Polarity,
    Hold,
    Picture,
    Auto,
    Clean,
}

impl Control {
    /// The setting that keeps its value, if it has one.
    fn setting(self) -> Option<&'static SettingSpec> {
        let index = match self {
            Control::Waveform => 0,
            Control::Temperature => 1,
            Control::Frames => 2,
            Control::Kick => 3,
            Control::Repeat => 4,
            Control::Rate => 5,
            Control::Polarity => 6,
            Control::Hold => 7,
            Control::Picture | Control::Auto | Control::Clean => return None,
        };
        Some(&SETTINGS[index])
    }

    /// Whether it is a number between `−` and `+`.
    fn is_number(self) -> bool {
        matches!(
            self,
            Control::Temperature
                | Control::Frames
                | Control::Kick
                | Control::Repeat
                | Control::Rate
                | Control::Hold
        )
    }

    fn title(self) -> &'static str {
        match self {
            Control::Waveform => "x4.lab.waveform",
            Control::Temperature => "x4.lab.temperature",
            Control::Frames => "x4.lab.frames",
            Control::Kick => "x4.lab.kick",
            Control::Repeat => "x4.lab.repeat",
            Control::Rate => "x4.lab.rate",
            Control::Polarity => "x4.lab.polarity",
            Control::Hold => "x4.lab.hold",
            Control::Picture => "x4.lab.picture",
            Control::Auto => "x4.lab.auto",
            Control::Clean => "x4.lab.clean",
        }
    }
}

/// The rows, by the waveform chosen and what the panel takes.
fn controls(waveform: &str) -> &'static [Control] {
    use Control::*;
    match (support(), waveform) {
        (_, BY_TEMPERATURE) if support() != Support::None => {
            &[Waveform, Temperature, Picture, Auto, Clean]
        }
        (Support::Ssd1677, DIRECT) => &[Waveform, Frames, Kick, Picture, Auto, Clean],
        (Support::Uc8179, DIRECT) => &[
            Waveform, Frames, Kick, Hold, Rate, Repeat, Polarity, Picture, Auto, Clean,
        ],
        _ => &[Waveform, Picture, Auto, Clean],
    }
}

const PICTURES: u8 = 4;
/// The most rows that show at a time.
const MOST_ROWS: u16 = 6;
/// The widest a number is, for the room of its `−` and `+`.
const ROOM: &str = "000 Hz";

struct Lab {
    style: UiStyle,
    header: ui::Header,
    list: ListView,
    picture: Rect,
    info: Rect,
    help: Rect,
    pattern: u8,
    flipped: bool,
    auto: bool,
    /// Flips since the screen was cleaned up.
    flips: u32,
}

impl Lab {
    fn new(env: &Env<'_>) -> Self {
        let style = UiStyle::new(env);
        let header = ui::Header::new(env, &style, true);
        let display = env.device().display;
        let (gap, row) = (style.gap(), style.row());
        let line = row * 3 / 4;
        let top = header.area().height + gap;
        let rows_height = MOST_ROWS * row;
        let help = Rect::new(0, display.height.saturating_sub(line), display.width, line);
        let rows_top = help.y.saturating_sub(rows_height);
        let info = Rect::new(0, rows_top.saturating_sub(line), display.width, line);
        let picture = Rect::new(
            0,
            top,
            display.width,
            info.y.saturating_sub(top + gap).max(row),
        );
        let rows = Rect::new(0, rows_top, display.width, rows_height);
        Self {
            list: ListView::new(rows, u32::from(row)).focus_on_keys(env),
            style,
            header,
            picture,
            info,
            help,
            pattern: 0,
            flipped: false,
            auto: false,
            flips: 0,
        }
    }

    fn controls(env: &Env<'_>) -> &'static [Control] {
        controls(env.settings().text(WAVEFORM))
    }

    fn focused(&self, env: &Env<'_>) -> Control {
        let controls = Self::controls(env);
        controls[self.list.focus().min(controls.len() - 1)]
    }

    /// The value of a row, as a setting shows it.
    fn value(&self, env: &Env<'_>, control: Control) -> String {
        let settings = env.settings();
        let tr = |key: &str| String::from(env.i18n().tr(key));
        match control {
            Control::Waveform => tr(settings.text(WAVEFORM)),
            Control::Temperature => format!("{} °C", settings.int(TEMPERATURE)),
            Control::Frames => format!("{}", settings.int(FRAMES)),
            Control::Kick => format!("{}", settings.int(KICK)),
            Control::Repeat => format!("{}", settings.int(REPEAT)),
            Control::Rate => format!("{} Hz", settings.int(RATE)),
            Control::Hold => format!("{}", settings.int(HOLD)),
            Control::Polarity => String::new(),
            Control::Picture => tr(match self.pattern {
                0 => "x4.lab.picture.stripes",
                1 => "x4.lab.picture.squares",
                2 => "x4.lab.picture.halves",
                _ => "x4.lab.picture.lines",
            }),
            Control::Auto => String::new(),
            Control::Clean => String::new(),
        }
    }

    /// The row as the standard rows draw it.
    fn row<'r>(
        &self,
        env: &Env<'_>,
        control: Control,
        title: &'r str,
        value: &'r str,
    ) -> ui::Row<'r> {
        let settings = env.settings();
        if control.is_number() {
            ui::Row::number(title, value, ROOM)
        } else if control == Control::Polarity {
            ui::Row::switch(title, settings.bool(SWAPPED))
        } else if control == Control::Auto {
            ui::Row::switch(title, self.auto)
        } else {
            ui::Row::value(title, value)
        }
    }

    /// The part below the header drawn anew and shown fast.
    fn redraw(&self, cx: &mut Cx<'_, '_>) {
        let top = self.picture.y;
        let below = Rect::new(
            0,
            top,
            self.picture.width,
            (self.help.bottom() as u16).saturating_sub(top),
        );
        cx.request_redraw_area(below);
        // The rows are shown the panel's own way (a partial refresh, not
        // the fast one the experiments take): a waveform that does not
        // develop would otherwise leave the lab itself unreadable.
        cx.request_refresh(RefreshMode::Partial);
    }

    /// Flips the picture: its black pixels white and the other way, with
    /// the line telling what the last frame took.
    fn flip(&mut self, cx: &mut Cx<'_, '_>) {
        self.flipped = !self.flipped;
        self.flips += 1;
        if self.auto && self.flips >= AUTO_STOPS_AFTER {
            self.auto = false;
        }
        let area = Rect::new(
            0,
            self.picture.y,
            self.picture.width,
            (self.info.bottom() as u16).saturating_sub(self.picture.y),
        );
        cx.request_redraw_area(area);
        cx.request_refresh(RefreshMode::Fast);
        if self.auto {
            cx.wake_at(cx.env().now().add_millis(1));
        }
    }

    /// Every pixel driven, the stock way: what the experiments left of
    /// the picture is cleared.
    fn clean(&mut self, cx: &mut Cx<'_, '_>) {
        self.flips = 0;
        self.flipped = false;
        self.auto = false;
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Full);
    }

    /// Changes a row by `step`: a setting as the settings page changes it,
    /// the picture, the flipping on its own.
    fn change(&mut self, control: Control, step: i64, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        match control {
            Control::Waveform => {
                let waveforms = waveforms();
                let at = waveforms
                    .iter()
                    .position(|&w| w == env.settings().text(WAVEFORM))
                    .unwrap_or(0) as i64;
                let next = waveforms[(at + waveforms.len() as i64 + step.signum())
                    .rem_euclid(waveforms.len() as i64)
                    as usize];
                cx.command(Command::SetSetting(
                    Id::new_static(WAVEFORM),
                    SettingValue::Text(next.into()),
                ));
                self.list.set_len(controls(next).len());
            }
            Control::Picture => {
                self.pattern = (i64::from(self.pattern) + i64::from(PICTURES) + step.signum())
                    .rem_euclid(i64::from(PICTURES)) as u8;
            }
            Control::Auto => {
                self.auto = !self.auto;
                if self.auto {
                    cx.wake_at(cx.env().now().add_millis(1));
                }
            }
            Control::Clean => {}
            _ => {
                let Some(spec) = control.setting() else {
                    return;
                };
                if let Some(value) = ui::next_value(&env, spec, step.signum()) {
                    cx.command(Command::SetSetting(spec.id.clone(), value));
                }
            }
        }
        self.redraw(cx);
    }
}

impl paperpaw_core::Layer for Lab {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let mut ui = ui::Ui::new(canvas, env, &style);
        let controls = Self::controls(env);
        self.header.draw(
            &mut ui,
            env.i18n().tr("x4.lab.title"),
            None,
            self.list.paging(controls.len()),
        );
        if support() == Support::None {
            let area = Rect::new(0, self.picture.y, self.picture.width, self.picture.height);
            ui.message(area, env.i18n().tr("x4.lab.unsupported"));
            return;
        }
        draw_picture(ui.canvas(), self.picture, self.pattern, self.flipped);
        let tr = |key: &str| String::from(env.i18n().tr(key));
        let took = match env.panel().last_frame() {
            Some(frame) => format!(
                "{}: {} ms · {}: {}",
                tr("x4.lab.previous"),
                frame.shown().as_millis(),
                tr("x4.lab.flips"),
                self.flips
            ),
            None => tr("x4.lab.none"),
        };
        ui.note_line(self.info, &took);
        let rows: Vec<(Control, String, String)> = controls
            .iter()
            .map(|&control| {
                (
                    control,
                    String::from(env.i18n().tr(control.title())),
                    self.value(env, control),
                )
            })
            .collect();
        self.list
            .draw(&mut ui, rows.len(), |ui, index, rect, focused| {
                let (control, title, value) = &rows[index];
                if control.is_number() && focused {
                    // As the settings page draws a number on the focus: the
                    // row on black, its − and + as buttons.
                    ui::draw_stepper_lines(
                        ui.canvas(),
                        env,
                        &style,
                        ui::RowLines::one(rect),
                        title,
                        value,
                        ROOM,
                        false,
                    );
                } else {
                    self.row(env, *control, title, value)
                        .focused(focused)
                        .draw(ui, rect);
                }
            });
        let note = tr(if self.flips >= CLEAN_AFTER {
            "x4.lab.clean-soon"
        } else if env.device().input.touch {
            "x4.lab.help.touch"
        } else {
            "x4.lab.help"
        });
        ui.note_line(self.help, &note);
    }

    fn mode(&self) -> ScreenMode {
        // The refreshes are the lab's own: the kernel does not add its
        // cleaning ones.
        ScreenMode::new().cleans_itself()
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        if support() == Support::None {
            return Handled::No;
        }
        let env = *cx.env();
        let len = Self::controls(&env).len();
        if *action == actions::LEFT {
            let control = self.focused(&env);
            self.change(control, -1, cx);
        } else if *action == actions::RIGHT {
            let control = self.focused(&env);
            self.change(control, 1, cx);
        } else if *action == actions::CONFIRM {
            match self.focused(&env) {
                Control::Auto => self.change(Control::Auto, 1, cx),
                Control::Clean => self.clean(cx),
                _ => self.flip(cx),
            }
        } else if *action == actions::UP {
            self.list.move_focus(-1, len).request(cx);
        } else if *action == actions::DOWN {
            self.list.move_focus(1, len).request(cx);
        } else if *action == actions::PREV_PAGE {
            self.list
                .move_focus(-(self.list.rows() as isize), len)
                .request(cx);
        } else if *action == actions::NEXT_PAGE {
            self.list
                .move_focus(self.list.rows() as isize, len)
                .request(cx);
        } else {
            return Handled::No;
        }
        Handled::Yes
    }

    /// By touch, as the settings are: the header's arrows and back; a tap
    /// on the picture flips it; on a row's `−` or `+` changes its number, on
    /// a switch or a choice changes it, on *Clean up* cleans.
    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if gesture.kind != GestureKind::Tap {
            return Handled::No;
        }
        let env = *cx.env();
        let controls = Self::controls(&env);
        let paging = self.list.paging(controls.len());
        if let Some(action) = self.header.tap(&env, &self.style, gesture.start, paging) {
            cx.command(Command::run(action));
            return Handled::Yes;
        }
        if support() == Support::None {
            return Handled::No;
        }
        if self.picture.contains(gesture.start) {
            self.flip(cx);
            return Handled::Yes;
        }
        let Some(index) = self.list.hit(gesture.start, controls.len()) else {
            return Handled::No;
        };
        self.list.set_shown(true);
        self.list.set_focus(index);
        let control = controls[index];
        let rect = self.list.row_rect(index % self.list.rows());
        let title = String::from(env.i18n().tr(control.title()));
        let value = self.value(&env, control);
        let part =
            self.row(&env, control, &title, &value)
                .hit(&env, &self.style, rect, gesture.start);
        match (control, part) {
            (Control::Clean, _) => self.clean(cx),
            (_, Some(RowPart::Minus)) => self.change(control, -1, cx),
            (_, Some(RowPart::Plus)) => self.change(control, 1, cx),
            (c, _) if c.is_number() => self.redraw(cx),
            _ => self.change(control, 1, cx),
        }
        Handled::Yes
    }

    fn on_timer(&mut self, cx: &mut Cx<'_, '_>) {
        if self.auto {
            self.flip(cx);
        }
    }
}

/// The test picture in `area`: `pattern` in its two states (`flipped`
/// swaps black and white), with a frame round it.
fn draw_picture(canvas: &mut Canvas<'_>, area: Rect, pattern: u8, flipped: bool) {
    let (ink, paper) = if flipped {
        (Color::WHITE, Color::BLACK)
    } else {
        (Color::BLACK, Color::WHITE)
    };
    canvas.fill(area, paper);
    let (x, y) = (i32::from(area.x), i32::from(area.y));
    let (width, height) = (u32::from(area.width), u32::from(area.height));
    match pattern {
        // Vertical stripes.
        0 => {
            let stripe = 24;
            let mut at = 0;
            while at < width {
                canvas.fill_rect(x + at as i32, y, stripe.min(width - at), height, ink);
                at += 2 * stripe;
            }
        }
        // Squares.
        1 => {
            let square = 24;
            for row in 0..height.div_ceil(square) {
                for column in 0..width.div_ceil(square) {
                    if (row + column) % 2 == 0 {
                        canvas.fill_rect(
                            x + (column * square) as i32,
                            y + (row * square) as i32,
                            square.min(width - column * square),
                            square.min(height - row * square),
                            ink,
                        );
                    }
                }
            }
        }
        // Left half and right half.
        2 => canvas.fill_rect(x, y, width / 2, height, ink),
        // Thin lines, as text has them.
        _ => {
            let mut at = 0;
            while at < height {
                canvas.fill_rect(x, y + at as i32, width, 2.min(height - at), ink);
                at += 8;
            }
        }
    }
    // A frame, so that the edge shows in both states.
    canvas.stroke_rect(x, y, width, height, 2, Color::BLACK);
}
