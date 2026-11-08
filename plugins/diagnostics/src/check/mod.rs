//! The device check: one page for each part of the device (see [`Step`]), a
//! page at the end with what came of each, and the report written to the
//! plugin's data folder after each answer.
//!
//! Each page says what to do, shows what was measured, and ends in a row of
//! answers: *Fail*, *Skip*, *Pass*, the one the measurements suggest chosen.
//! Some pages have a row of tools above (a full refresh, the light again,
//! sleep now). Buttons, taps and the touch targets answer the buttons' and the
//! touch page by themselves.
//!
//! Buttons: left and right (or the page buttons) move between the controls,
//! *Select* chooses, *Back* leaves; the check goes on where it was when opened
//! again. The buttons page begins with *Start*: before, it is moved over and
//! left like the others; after, every button counts as the one asked for, and
//! holding another one gives that one up.
//!
//! What this module decides is plain data; the page is laid out in
//! [`view`] and replayed by the draw callback. Time goes by the host's ticks:
//! a deadline is compared with the time each tick brings.

use crate::steps::{self, About, Buttons, CardTest, Mark, Outcome, Results, Step};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Handled, Point,
    native::{
        Error, Host,
        callbacks::{DrawFrame, Input, InputGestureKind, KeyState},
        commands::{CommandSender, RedrawMode},
        events::Event,
        settings::{self, SettingValue},
        snapshot::capture_snapshot,
        storage::{self, Root},
        ui::{Question, Ui},
    },
};
use paperpaw_ui_kit::{
    mem::{SERVICE_BUFFER, buffer, copy_strings},
    text::{copy, format},
};

pub mod reads;
mod view;

pub use view::Control;

/// The Wi-Fi plugin's action that opens its page, to join a network.
const WIFI_PAGE: &str = "wifi.open";
/// How long a search for networks may take.
const SCAN_MS: u64 = 20_000;
/// How long each light level shows.
const LIGHT_MS: u64 = 1500;
/// How long the device may take to go to sleep once asked.
const SLEEP_WAIT_MS: u64 = 10_000;
/// The reader's own light while the light step changes it, beside the report:
/// set again from there if the check ends unforeseen (a reset, the power cut),
/// and removed once it is set again.
const LIGHT_FILE: &str = "device-check-light.txt";
/// The light's levels tried in turn: brightness and warmth in percent.
pub const LIGHT_LEVELS: [(i64, i64); 5] = [(0, 50), (50, 50), (100, 50), (100, 0), (100, 100)];
/// The card's test file, beside the report, and how it is written: in chunks
/// of 4 KiB, which small devices hold easily.
const CARD_FILE: &str = "device-check.bin";
/// The longest report read back.
const REPORT_CAP: usize = 16 * 1024;
const CARD_CHUNK: usize = 4096;
const CARD_CHUNKS: usize = 32;
const BRIGHTNESS: &str = "core.frontlight";
const WARMTH: &str = "core.frontlight-warmth";

/// The light step going on: the level shown and the reader's own values, set
/// again at the end.
#[derive(Debug, Clone, Copy)]
pub struct LightTest {
    pub level: usize,
    pub kept: (i64, i64),
}

/// The radio during the Wi-Fi step: whether it was off before, so it is
/// switched off again after, and whether the search was asked for.
#[derive(Debug, Clone, Copy)]
struct RadioUse {
    was_off: bool,
    scanned: bool,
}

/// The device check of one screen instance.
#[derive(Debug)]
pub struct Check {
    machine: reads::Machine,
    results: Results,
    /// The step shown; `None` at the end.
    step: Option<Step>,
    focus: usize,
    /// Whether the focus shows (once a button was used; at once without touch).
    shown: bool,
    /// Why the report could not be written, if it could not.
    unsaved: Option<String>,
    started: bool,
    flipped: bool,
    buttons: Buttons,
    /// Whether the buttons are being asked for: not before *Start*, so that a
    /// page of buttons can be moved over and left like any other.
    buttons_on: bool,
    /// A held key already counted (the next release is not a press).
    held: bool,
    tapped: [bool; steps::TOUCH_TARGETS.len()],
    light: Option<LightTest>,
    /// Charging seen and not charging seen, while on the battery page.
    charging_seen: [bool; 2],
    card: Option<CardTest>,
    /// Why the light step could not begin, if it could not.
    light_unkept: Option<String>,
    /// The reader's own light as the data folder kept it, set again at the start.
    own_light: Option<(i64, i64)>,
    /// Networks found; `None` while looking.
    networks: Option<usize>,
    radio: Option<RadioUse>,
    radio_now: reads::Radio,
    /// When looking for networks began.
    scan_from: u64,
    /// When the device was asked to sleep, while it has not gone to sleep.
    sleep_asked: Option<u64>,
    /// The next time something is due.
    timer: Timer,
    now: u64,
    page: view::PageView,
    width: u16,
    height: u16,
}

/// The one place time is judged: a deadline against the time the host's
/// events bring. Everything that waits (the light levels, the Wi-Fi search,
/// the sleep wait, the card test) goes through it, so a finer timer from the
/// host replaces what calls [`Timer::due`] and nothing else.
#[derive(Debug, Default, Clone, Copy)]
struct Timer {
    deadline: Option<u64>,
}

impl Timer {
    /// Something is due at `at` (the host's milliseconds).
    fn arm(&mut self, at: u64) {
        self.deadline = Some(at);
    }

    fn clear(&mut self) {
        self.deadline = None;
    }

    /// Whether it is due at `now`.
    fn due(self, now: u64) -> bool {
        self.deadline.is_some_and(|due| now >= due)
    }
}

impl Check {
    /// A check on a screen of `width` by `height` pixels.
    ///
    /// # Errors
    /// The host's refusals and allocation failures.
    // Not inlined: the frames would add up past what the X4's stack watch allows.
    #[inline(never)]
    pub fn new(host: &Host<'_>, width: u16, height: u16) -> Result<Self, Error> {
        let machine = reads::machine(host)?;
        let (now, has_touch) = start_facts(host)?;
        let results = read_report(host)?;
        let buttons = Buttons::new(copy_strings(&machine.keys)?);
        let mut check = Self {
            results,
            step: None,
            focus: 0,
            shown: !has_touch,
            unsaved: None,
            started: false,
            flipped: false,
            buttons,
            buttons_on: false,
            held: false,
            tapped: [false; steps::TOUCH_TARGETS.len()],
            light: None,
            charging_seen: [false; 2],
            card: None,
            light_unkept: None,
            own_light: None,
            networks: None,
            radio: None,
            radio_now: reads::Radio::default(),
            scan_from: now,
            sleep_asked: None,
            timer: Timer::default(),
            now,
            page: view::PageView::default(),
            width,
            height,
            machine,
        };
        // Asleep when last seen: how PaperPaw started tells whether it woke or
        // was started again.
        if check.results.get(Step::Sleep) == Some(&Mark::Asleep) {
            use paperpaw_sdk::native::device::StartKind;
            let (outcome, note) = match reads::start(host)? {
                StartKind::PowerOn | StartKind::Wake => (Outcome::Pass, "woke"),
                StartKind::Restart => (Outcome::Fail, "started again instead of waking"),
                _ => (Outcome::Skip, "how it started is not known"),
            };
            check.mark(host, Step::Sleep, outcome, copy(note)?)?;
        }
        check.step = check.results.next_open();
        Ok(check)
    }

    /// The screen was resized.
    pub fn resize(&mut self, width: u16, height: u16) {
        self.width = width;
        self.height = height;
    }

    /// Marks `step` and writes the report.
    fn mark(
        &mut self,
        host: &Host<'_>,
        step: Step,
        outcome: Outcome,
        note: String,
    ) -> Result<(), Error> {
        self.results.set(step, Mark::Done(outcome, note));
        self.save(host)
    }

    // Not inlined: it holds a snapshot and the report's text.
    #[inline(never)]
    fn save(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let snapshot = capture_snapshot(host).ok();
        let when = snapshot
            .and_then(|s| s.unix_seconds())
            .map(|secs| paperpaw_sdk::time::UnixTime::from_secs(secs).iso8601());
        let about = About {
            name: &self.machine.name,
            id: &self.machine.id,
            dpi: self.machine.dpi,
            format: self.machine.format,
            version: env!("CARGO_PKG_VERSION"),
            when: when.as_deref(),
        };
        let report = self.results.report(&about)?;
        self.unsaved = storage::write(host, Root::Data, steps::REPORT, report.as_bytes())
            .err()
            .map(|e| format(format_args!("{e:?}")))
            .transpose()?;
        Ok(())
    }

    /// Shows `step`, or the end with `None`, after the steps that do not apply
    /// to this device were skipped; starts what the step measures.
    fn enter(&mut self, host: &Host<'_>, mut step: Option<Step>) -> Result<(), Error> {
        self.end_light(host);
        self.end_radio(host);
        while let Some(s) = step
            && let Some(missing) = s.missing(&self.machine.facts)
        {
            self.mark(host, s, Outcome::Skip, copy(missing)?)?;
            step = self.results.next_open();
        }
        self.step = step;
        self.flipped = false;
        self.buttons = Buttons::new(copy_strings(&self.machine.keys)?);
        self.buttons_on = false;
        self.held = false;
        self.tapped = [false; steps::TOUCH_TARGETS.len()];
        self.charging_seen = [false; 2];
        self.card = None;
        self.networks = None;
        self.timer.clear();
        match step {
            Some(Step::Light) => self.start_light(host)?,
            Some(Step::Battery) => self.see_charging(host),
            // The card is tried once the page shows it is.
            Some(Step::Card) => self.timer.arm(self.now),
            Some(Step::Wifi) => {
                // Until the radio is on, then while looking.
                self.scan_from = self.now;
                let radio = reads::radio(host).unwrap_or_default();
                self.radio_now = radio;
                self.radio = Some(RadioUse {
                    was_off: !radio.on,
                    scanned: radio.on,
                });
                // The radio looks for networks once it is on.
                let mut scratch = [0; 64];
                let mut commands = CommandSender::new(host, &mut scratch);
                if radio.on {
                    commands.wifi_scan()?;
                } else {
                    commands.wifi_on()?;
                }
                self.timer.arm(self.now + SCAN_MS);
            }
            _ => {}
        }
        self.prepare(host)?;
        self.focus = self.suggested_focus(host);
        self.redraw(host, RedrawMode::Full)
    }

    /// Answers the step shown with `outcome` and goes on.
    fn answer(&mut self, host: &Host<'_>, outcome: Outcome) -> Result<(), Error> {
        let Some(step) = self.step else {
            return Ok(());
        };
        // What was measured is noted whatever the answer: a part failed or
        // skipped is the better understood for it.
        let note = self.measured(host)?.1;
        self.mark(host, step, outcome, note)?;
        let next = self.results.next_open();
        self.enter(host, next)
    }

    /// The outcome the step's measurements suggest, and its note; no outcome
    /// while they go on.
    fn measured(&self, host: &Host<'_>) -> Result<(Option<Outcome>, String), Error> {
        let snapshot = capture_snapshot(host).ok();
        let (outcome, note) = match self.step {
            Some(Step::Screen) => {
                let levels = snapshot.map_or(16, |s| gray_levels(&s));
                (Outcome::Pass, format(format_args!("{levels} gray levels"))?)
            }
            Some(Step::Buttons) if self.buttons.asked().is_some() => {
                return Ok((None, String::new()));
            }
            Some(Step::Buttons) => self.buttons.verdict()?,
            Some(Step::Touch) if self.tapped.contains(&false) => {
                return Ok((None, String::new()));
            }
            Some(Step::Touch) => (Outcome::Pass, copy("corners and edges")?),
            Some(Step::Light) if self.light.is_some() => return Ok((None, String::new())),
            Some(Step::Light) => match &self.light_unkept {
                Some(e) => (
                    Outcome::Skip,
                    format(format_args!("own light not kept: {e}"))?,
                ),
                None => (Outcome::Pass, copy("brightness and warmth")?),
            },
            Some(Step::Battery) => steps::battery(
                snapshot.and_then(|s| reads::battery(&s)),
                self.charging_seen,
            )?,
            Some(Step::Card) => match &self.card {
                Some(test) => test.verdict()?,
                None => return Ok((None, String::new())),
            },
            Some(Step::Clock) => match snapshot.and_then(|s| s.unix_seconds()) {
                Some(secs) => (
                    Outcome::Pass,
                    copy(&paperpaw_sdk::time::UnixTime::from_secs(secs).iso8601())?,
                ),
                None => (Outcome::Skip, copy("time of day not known")?),
            },
            Some(Step::Wifi) => return self.wifi_measured(),
            Some(Step::Sleep) | None => return Ok((None, String::new())),
            Some(Step::Memory) => steps::memory(reads::memory(host).ok())?,
        };
        Ok((Some(outcome), note))
    }

    fn wifi_measured(&self) -> Result<(Option<Outcome>, String), Error> {
        let online = self.radio_now.online;
        let timed_out = self.now.saturating_sub(self.scan_from) >= SCAN_MS;
        let scanned = self.radio.is_none_or(|r| r.scanned);
        let fail = |note: Result<String, Error>| Ok((Some(Outcome::Fail), note?));
        match self.networks {
            // Joined, the network found before it looked or not.
            Some(n) if online => Ok((
                Some(Outcome::Pass),
                format(format_args!("{n} networks found, joined"))?,
            )),
            None if online => Ok((Some(Outcome::Pass), copy("joined")?)),
            None if !timed_out => Ok((None, String::new())),
            None if !scanned => fail(copy("the radio did not come on")),
            None | Some(0) => fail(copy("no networks found")),
            Some(n) => fail(format(format_args!("{n} networks found, not joined"))),
        }
    }

    /// The control focused at first: the answer a measurement suggests, else
    /// the first tool, else *Pass*. The screen page suggests *Pass* without
    /// measuring anything: it is judged by the tools (flip the pattern, refresh
    /// in full), so they come first.
    fn suggested_focus(&self, host: &Host<'_>) -> usize {
        let controls = &self.page.controls;
        let wanted = match (self.step, self.measured(host).ok().and_then(|m| m.0)) {
            (None, _) => Control::Done,
            (Some(Step::Screen), _) | (_, None) => controls
                .iter()
                .find(|c| c.0 != Control::Help)
                .map_or(Control::Answer(Outcome::Pass), |c| c.0),
            (_, Some(outcome)) => Control::Answer(outcome),
        };
        controls.iter().position(|c| c.0 == wanted).unwrap_or(0)
    }

    /// Chooses what control `control` stands for.
    fn choose(&mut self, host: &Host<'_>, control: Control) -> Result<(), Error> {
        match control {
            Control::Answer(outcome) => self.answer(host, outcome),
            Control::Help => self.help(host),
            Control::Flip => {
                self.flipped = !self.flipped;
                self.prepare(host)?;
                self.redraw(host, RedrawMode::Fast)
            }
            Control::FullRefresh => {
                self.prepare(host)?;
                self.redraw(host, RedrawMode::Full)
            }
            Control::StartButtons => {
                self.buttons_on = true;
                self.prepare(host)?;
                self.redraw(host, RedrawMode::Partial)
            }
            Control::LightAgain => {
                self.start_light(host)?;
                self.redraw_page(host)
            }
            Control::WifiPage => {
                let mut scratch = [0; 128];
                // Without the Wi-Fi plugin there is no page to open.
                match CommandSender::new(host, &mut scratch).run_action(WIFI_PAGE) {
                    Err(Error::NotFound) => Ok(()),
                    other => other,
                }
            }
            Control::SleepNow => {
                // Marked asleep when the device goes to sleep (suspend).
                self.sleep_asked = Some(self.now);
                self.timer.arm(self.now + SLEEP_WAIT_MS);
                let mut scratch = [0; 64];
                CommandSender::new(host, &mut scratch).sleep()
            }
            Control::StartOver => {
                self.results = Results::new()?;
                self.save(host)?;
                self.enter(host, Some(Step::ALL[0]))
            }
            Control::Done => {
                let mut scratch = [0; 64];
                CommandSender::new(host, &mut scratch).close_screen()
            }
        }
    }

    /// The step's help whole, in a dialog (nothing to do then).
    fn help(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let (title, text) = view::texts(host, self.step)?;
        let ui = Ui::open(host)?;
        let result = (|| {
            let question = Question::confirm(&title, &text)?.only_yes()?;
            let mut input = buffer(SERVICE_BUFFER)?;
            let mut out = buffer(SERVICE_BUFFER)?;
            ui.ask(host, question, &mut input, &mut out).map(|_| ())
        })();
        let released = ui.release(host);
        result.and(released)
    }

    fn set_light(host: &Host<'_>, (brightness, warmth): (i64, i64)) {
        let mut scratch = [0; 128];
        let mut commands = CommandSender::new(host, &mut scratch);
        // A refusal leaves the light as it is; the check goes on.
        let _ = commands.set_setting(BRIGHTNESS, SettingValue::Int(brightness));
        let _ = commands.set_setting(WARMTH, SettingValue::Int(warmth));
    }

    fn start_light(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let kept = self.own_light.take().unwrap_or_else(|| {
            let mut scratch = [0; 32];
            let mut read = |key| match settings::read_setting(host, key, &mut scratch) {
                Ok(SettingValue::Int(value)) => value,
                _ => 0,
            };
            (read(BRIGHTNESS), read(WARMTH))
        });
        // Kept on the card first: nothing is changed unless it is.
        let line = format(format_args!("{} {}\n", kept.0, kept.1))?;
        if let Err(e) = storage::write(host, Root::Data, LIGHT_FILE, line.as_bytes()) {
            self.light_unkept = Some(format(format_args!("{e:?}"))?);
            return Ok(());
        }
        self.light_unkept = None;
        self.light = Some(LightTest { level: 0, kept });
        Self::set_light(host, LIGHT_LEVELS[0]);
        self.timer.arm(self.now + LIGHT_MS);
        Ok(())
    }

    /// Sets the reader's own light again from the card, where a check that
    /// ended unforeseen left it while trying the light.
    fn restore_light(&mut self, host: &Host<'_>) {
        let mut bytes = [0; 64];
        let Ok(count) = storage::read_at(host, Root::Data, LIGHT_FILE, 0, &mut bytes) else {
            return;
        };
        let Ok(text) = core::str::from_utf8(&bytes[..count]) else {
            return;
        };
        let mut numbers = text.split_whitespace().map(str::parse::<i64>);
        if let (Some(Ok(brightness)), Some(Ok(warmth))) = (numbers.next(), numbers.next()) {
            Self::set_light(host, (brightness, warmth));
            self.own_light = Some((brightness, warmth));
        }
        let _ = storage::remove(host, Root::Data, LIGHT_FILE);
    }

    /// The next light level, or the reader's own again after the last.
    fn step_light(&mut self, host: &Host<'_>) {
        let Some(light) = &mut self.light else {
            return;
        };
        light.level += 1;
        match LIGHT_LEVELS.get(light.level) {
            Some(level) => {
                Self::set_light(host, *level);
                self.timer.arm(self.now + LIGHT_MS);
            }
            None => self.end_light(host),
        }
    }

    /// Sets the reader's own light again, if the light step changed it.
    fn end_light(&mut self, host: &Host<'_>) {
        if let Some(light) = self.light.take() {
            Self::set_light(host, light.kept);
            let _ = storage::remove(host, Root::Data, LIGHT_FILE);
        }
    }

    /// Switches the radio off again if the Wi-Fi step switched it on and
    /// nothing wants the network now.
    fn end_radio(&mut self, host: &Host<'_>) {
        let wanted = reads::radio(host).is_ok_and(|r| r.wanted || r.busy);
        if self.radio.take().is_some_and(|r| r.was_off) && !wanted {
            let mut scratch = [0; 64];
            let _ = CommandSender::new(host, &mut scratch).wifi_off();
        }
    }

    fn see_charging(&mut self, host: &Host<'_>) {
        if let Ok(snapshot) = capture_snapshot(host)
            && snapshot.battery_percent().is_some()
        {
            self.charging_seen[usize::from(!snapshot.is_charging())] = true;
        }
    }

    /// Writes 128 KiB to the data folder, reads them back and compares them,
    /// timed; the file goes again after.
    fn run_card_test(&mut self, host: &Host<'_>) {
        self.card = Some(card_test(host));
    }

    fn redraw(&self, host: &Host<'_>, mode: RedrawMode) -> Result<(), Error> {
        let mut scratch = [0; 64];
        CommandSender::new(host, &mut scratch).queue_redraw(mode)
    }

    /// The page anew, with its controls (the answer suggested may have
    /// changed) and the focus where the suggestion is.
    fn redraw_page(&mut self, host: &Host<'_>) -> Result<(), Error> {
        self.prepare(host)?;
        self.focus = self.suggested_focus(host);
        self.prepare(host)?;
        self.redraw(host, RedrawMode::Partial)
    }

    fn move_focus(&mut self, host: &Host<'_>, delta: isize) -> Result<(), Error> {
        let count = self.page.controls.len();
        if !self.shown {
            self.shown = true;
        } else if count > 0 {
            self.focus = step_focus(self.focus, delta, count);
        }
        self.prepare(host)?;
        self.redraw(host, RedrawMode::Fast)
    }

    /// A tap on a touch target of the touch page.
    fn tap_target(&mut self, host: &Host<'_>, point: Point) -> Result<bool, Error> {
        let radius = i32::try_from(self.page.metrics().mm(view::TARGET_MM * 2)).unwrap_or(i32::MAX);
        let hit = self
            .page
            .targets(self.width, self.height)
            .iter()
            .position(|t| (point.x - t.x).abs() <= radius && (point.y - t.y).abs() <= radius);
        let Some(index) = hit else {
            return Ok(false);
        };
        if !self.tapped[index] {
            self.tapped[index] = true;
            if self.tapped.contains(&false) {
                self.prepare(host)?;
                self.redraw(host, RedrawMode::Fast)?;
            } else {
                self.answer(host, Outcome::Pass)?;
            }
        }
        Ok(true)
    }

    fn takes_buttons(&self) -> bool {
        self.step == Some(Step::Buttons) && self.buttons_on && self.buttons.asked().is_some()
    }

    /// Lays the page out for what is now.
    fn prepare(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let body = view::Body {
            step: self.step,
            flipped: self.flipped,
            buttons: &self.buttons,
            buttons_on: self.buttons_on,
            tapped: &self.tapped,
            light: self.light,
            card: self.card.as_ref(),
            networks: self.networks,
            radio: self.radio_now,
            results: &self.results,
            unsaved: self.unsaved.as_deref(),
            wifi_page: true,
        };
        let suggested = self.measured(host).ok().and_then(|m| m.0);
        self.page = view::build(
            host,
            &body,
            suggested,
            self.width,
            self.height,
            (self.shown && !self.takes_buttons()).then_some(self.focus),
        )?;
        Ok(())
    }

    /// The screen is shown for the first time, or again after the Wi-Fi page.
    ///
    /// # Errors
    /// The host's refusals.
    // Not inlined: the frames would add up past what the X4's stack watch allows.
    #[inline(never)]
    pub fn shown(&mut self, host: &Host<'_>) -> Result<(), Error> {
        self.now = capture_snapshot(host)?.instant().0;
        if self.started {
            // Back from the Wi-Fi page: joined or not.
            self.radio_now = reads::radio(host).unwrap_or(self.radio_now);
            self.redraw_page(host)
        } else {
            self.started = true;
            self.restore_light(host);
            let step = self.step;
            self.enter(host, step)
        }
    }

    /// The screen closed.
    // Not inlined: the frames would add up past what the X4's stack watch allows.
    #[inline(never)]
    pub fn closed(&mut self, host: &Host<'_>) {
        self.end_light(host);
        self.end_radio(host);
    }

    /// Input to the screen on top.
    ///
    /// # Errors
    /// The host's refusals.
    pub fn input(&mut self, host: &Host<'_>, input: Input<'_>) -> Result<Handled, Error> {
        match input {
            Input::Key { now_ms, key, state } if self.takes_buttons() => {
                self.now = now_ms;
                self.key(host, key, state)?;
                Ok(Handled::Yes)
            }
            Input::Action { now_ms, action } => {
                self.now = now_ms;
                self.action(host, action)
            }
            Input::Gesture {
                now_ms,
                position,
                kind: InputGestureKind::Tap,
            } => {
                self.now = now_ms;
                self.tap(host, position)
            }
            _ => Ok(Handled::No),
        }
    }

    /// A button while the buttons are asked for: pressed when released, held
    /// when it repeats.
    fn key(&mut self, host: &Host<'_>, key: &str, state: KeyState) -> Result<(), Error> {
        let (counts, long) = match state {
            KeyState::Press => return Ok(()),
            KeyState::Repeat if self.held => return Ok(()),
            KeyState::Repeat => {
                self.held = true;
                (true, true)
            }
            _ => (!core::mem::take(&mut self.held), false),
        };
        if counts && self.buttons.press(key, long)? {
            if self.buttons.asked().is_some() {
                self.prepare(host)?;
                self.redraw(host, RedrawMode::Fast)?;
            } else {
                let outcome = self.buttons.verdict()?.0;
                self.answer(host, outcome)?;
            }
        }
        Ok(())
    }

    fn action(&mut self, host: &Host<'_>, action: &str) -> Result<Handled, Error> {
        match action {
            "core.left" | "core.up" | "core.previous" | "core.prev-page" => {
                self.move_focus(host, -1)?;
            }
            "core.right" | "core.down" | "core.next" | "core.next-page" => {
                self.move_focus(host, 1)?;
            }
            "core.confirm" => {
                if !self.shown {
                    self.shown = true;
                    self.prepare(host)?;
                    self.redraw(host, RedrawMode::Fast)?;
                } else if let Some((control, _)) = self.page.controls.get(self.focus).copied() {
                    self.choose(host, control)?;
                }
            }
            _ => return Ok(Handled::No),
        }
        Ok(Handled::Yes)
    }

    fn tap(&mut self, host: &Host<'_>, point: Point) -> Result<Handled, Error> {
        self.shown = false;
        let touch = self.step == Some(Step::Touch);
        if touch && self.tap_target(host, point)? {
            return Ok(Handled::Yes);
        }
        let hit = self
            .page
            .controls
            .iter()
            .position(|(_, r)| r.contains(point));
        if let Some(index) = hit {
            self.focus = index;
            let control = self.page.controls[index].0;
            self.choose(host, control)?;
            return Ok(Handled::Yes);
        }
        match self.page.header.action(point) {
            Some(action) if !touch => {
                let mut scratch = [0; 128];
                CommandSender::new(host, &mut scratch).run_action(action)?;
                Ok(Handled::Yes)
            }
            _ => Ok(Handled::No),
        }
    }

    /// Events the check follows: ticks (the time), the battery, the radio and
    /// sleeping.
    ///
    /// # Errors
    /// The host's refusals.
    // Not inlined: the frames would add up past what the X4's stack watch allows.
    #[inline(never)]
    pub fn event(&mut self, host: &Host<'_>, event: &Event<'_>) -> Result<(), Error> {
        match *event {
            Event::Tick { now_ms } => {
                self.now = now_ms;
                if self.timer.due(now_ms) {
                    self.timer.clear();
                    self.when_due(host)?;
                }
                Ok(())
            }
            Event::BatteryMeasured { now_ms } | Event::ChargeChanged { now_ms, .. }
                if self.step == Some(Step::Battery) =>
            {
                self.now = now_ms;
                self.see_charging(host);
                self.redraw_page(host)
            }
            Event::WifiChanged { now_ms, .. } if self.step == Some(Step::Wifi) => {
                self.now = now_ms;
                self.radio_now = reads::radio(host).unwrap_or(self.radio_now);
                self.networks = reads::networks(host)
                    .ok()
                    .filter(|_| self.radio.is_none_or(|r| r.scanned));
                if let Some(radio) = &mut self.radio
                    && !radio.scanned
                    && self.radio_now.on
                {
                    radio.scanned = true;
                    let mut scratch = [0; 64];
                    CommandSender::new(host, &mut scratch).wifi_scan()?;
                    // The search has its time from now.
                    self.scan_from = now_ms;
                    self.timer.arm(now_ms + SCAN_MS);
                }
                self.redraw_page(host)
            }
            Event::Suspend { now_ms }
                if self.step == Some(Step::Sleep) && self.sleep_asked.is_some() =>
            {
                self.now = now_ms;
                self.sleep_asked = None;
                self.timer.clear();
                self.results.set(Step::Sleep, Mark::Asleep);
                self.save(host)
            }
            Event::Resume { now_ms }
                if self.step == Some(Step::Sleep)
                    && self.results.get(Step::Sleep) == Some(&Mark::Asleep) =>
            {
                self.now = now_ms;
                self.mark(host, Step::Sleep, Outcome::Pass, copy("woke")?)?;
                let next = self.results.next_open();
                self.enter(host, next)
            }
            _ => Ok(()),
        }
    }

    fn when_due(&mut self, host: &Host<'_>) -> Result<(), Error> {
        match self.step {
            Some(Step::Card) if self.card.is_none() => {
                self.run_card_test(host);
                self.redraw_page(host)
            }
            Some(Step::Light) => {
                self.step_light(host);
                self.redraw_page(host)
            }
            // The search took too long: the page says so.
            Some(Step::Wifi) => {
                self.radio_now = reads::radio(host).unwrap_or(self.radio_now);
                self.networks = reads::networks(host).ok();
                self.redraw_page(host)
            }
            Some(Step::Sleep)
                if self
                    .sleep_asked
                    .is_some_and(|at| self.now.saturating_sub(at) >= SLEEP_WAIT_MS) =>
            {
                self.sleep_asked = None;
                self.mark(
                    host,
                    Step::Sleep,
                    Outcome::Fail,
                    copy("did not go to sleep")?,
                )?;
                let next = self.results.next_open();
                self.enter(host, next)
            }
            _ => Ok(()),
        }
    }

    /// Draws the page.
    ///
    /// # Errors
    /// The host's refusals.
    pub fn draw(&self, host: &Host<'_>, frame: &mut DrawFrame<'_>) -> Result<(), Error> {
        self.page.draw(host, frame)
    }
}

/// The host's milliseconds now and whether the device has touch, read from a
/// snapshot that is gone again (it is too big to keep in a caller's frame).
#[inline(never)]
fn start_facts(host: &Host<'_>) -> Result<(u64, bool), Error> {
    let snapshot = capture_snapshot(host)?;
    Ok((snapshot.instant().0, snapshot.has_touch()))
}

/// The report written before, to go on from; empty where there is none.
#[inline(never)]
fn read_report(host: &Host<'_>) -> Result<Results, Error> {
    let mut bytes = Vec::new();
    if bytes.try_reserve_exact(REPORT_CAP).is_err() {
        return Results::new();
    }
    bytes.resize(REPORT_CAP.min(storage::MAX_DATA), 0);
    storage::read_at(host, Root::Data, steps::REPORT, 0, &mut bytes)
        .ok()
        .and_then(|count| core::str::from_utf8(bytes.get(..count)?).ok())
        .and_then(|text| Results::parse(text).ok())
        .map_or_else(Results::new, Ok)
}

/// Writes 128 KiB, reads them back and compares them, timed where the device
/// has a clock; the file goes again after.
fn card_test(host: &Host<'_>) -> CardTest {
    let bytes = CARD_CHUNK * CARD_CHUNKS;
    let mut test = CardTest {
        bytes,
        write_us: None,
        read_us: None,
        error: None,
    };
    let mut chunk = Vec::new();
    if chunk.try_reserve_exact(CARD_CHUNK * 2).is_err() {
        test.error = copy("no memory").ok();
        return test;
    }
    chunk.extend((0..CARD_CHUNK).map(|i| (i % 251) as u8));
    chunk.resize(CARD_CHUNK * 2, 0);
    let (pattern, back) = chunk.split_at_mut(CARD_CHUNK);
    let clock = || paperpaw_sdk::native::clock::micros(host).ok();
    let started = clock();
    let written = storage::write(host, Root::Data, CARD_FILE, pattern).and_then(|()| {
        (1..CARD_CHUNKS).try_for_each(|_| storage::append(host, Root::Data, CARD_FILE, pattern))
    });
    let wrote = clock();
    if let Err(e) = written {
        test.error = format(format_args!("write: {e:?}")).ok();
        let _ = storage::remove(host, Root::Data, CARD_FILE);
        return test;
    }
    let mut same = true;
    for i in 0..CARD_CHUNKS {
        match storage::read_at(host, Root::Data, CARD_FILE, (i * CARD_CHUNK) as u64, back) {
            Ok(n) if n == CARD_CHUNK => same &= back == pattern,
            Ok(_) => same = false,
            Err(e) => {
                test.error = format(format_args!("read: {e:?}")).ok();
                break;
            }
        }
    }
    let read = clock();
    if let Err(e) = storage::remove(host, Root::Data, CARD_FILE)
        && let Ok(note) = format(format_args!("remove: {e:?}"))
    {
        test.error.get_or_insert(note);
    }
    if !same && let Ok(note) = copy("read back differs") {
        test.error.get_or_insert(note);
    }
    if let (Some(a), Some(b), Some(c)) = (started, wrote, read) {
        test.write_us = Some(b.saturating_sub(a));
        test.read_us = Some(c.saturating_sub(b));
    }
    test
}

/// The gray levels the screen page shows: a panel of black and white shows the
/// page in four grays, as it shows pictures.
pub fn gray_levels(snapshot: &paperpaw_sdk::native::snapshot::Snapshot) -> u32 {
    match snapshot.display_gray_levels() {
        Some(2) => 4,
        Some(levels) => u32::from(levels),
        None => 16,
    }
    .clamp(2, 16)
}

/// `focus` moved by `delta` within `count` controls, wrapping round.
#[must_use]
pub fn step_focus(focus: usize, delta: isize, count: usize) -> usize {
    if count == 0 {
        return 0;
    }
    let count = count as isize;
    (focus as isize + delta).rem_euclid(count) as usize
}

#[cfg(test)]
mod tests;
