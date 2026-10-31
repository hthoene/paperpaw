//! The device check's screen: one page for each part of the device (see
//! [`Step`]), a page at the end with what came of each, and the report
//! written to the card after each answer.
//!
//! Each page says what to do, shows what was measured, and ends in a row
//! of answers: *Fail*, *Skip*, *Pass*, the one the measurements suggest
//! chosen. Some pages have a row of tools above (a full refresh, the
//! light again, sleep now). Buttons, taps and the touch targets answer
//! the buttons' and the touch page by themselves.
//!
//! Buttons: left and right (or the page buttons) move between the
//! controls, *Select* chooses, *Back* leaves; the check goes on where it
//! was when opened again. While the buttons are asked for, every button
//! counts as pressed; holding another one gives the one asked up.

use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::ask;
use paperpaw_sdk::builtin::{actions, events, settings};
use paperpaw_sdk::power::Start;
use paperpaw_sdk::replies::Confirmed;
use paperpaw_sdk::semantics::Role;
use paperpaw_sdk::ui::{self, UiStyle};
use paperpaw_sdk::wifi::WifiCommand;
use paperpaw_sdk::{
    Arg, Canvas, Color, Command, Cx, Env, Event, Gesture, GestureKind, Handled, Id, Instant,
    Params, PixelFormat, Point, Rect, RefreshMode, Screen, SettingValue,
};

use crate::steps::{self, About, Buttons, CardTest, Mark, Outcome, Results, Step};

/// The step's help read in a dialog (nothing to do then).
const HELP_READ: Event<Confirmed> = Event::new("diagnostics.check.help-read");
/// The Wi-Fi plugin's action that opens its page, to join a network.
const WIFI_PAGE: &str = "wifi.open";
/// How long a search for networks may take.
const SCAN_MS: u64 = 20_000;
/// How long each light level shows.
const LIGHT_MS: u64 = 1500;
/// How long the device may take to go to sleep once asked.
const SLEEP_WAIT_MS: u64 = 10_000;
/// The reader's own light while the light step changes it, beside the
/// report: set again from there if the check ends unforeseen (a reset,
/// the power cut), and removed once it is set again.
const LIGHT_FILE: &str = paperpaw_sdk::data_path!("device-check-light.txt");
/// The light's levels tried in turn: brightness and warmth in percent.
const LIGHT_LEVELS: [(i64, i64); 5] = [(0, 50), (50, 50), (100, 50), (100, 0), (100, 100)];
/// The card's test file, beside the report, and how it is written: in
/// chunks of 4 KiB, which small devices hold easily.
const CARD_FILE: &str = paperpaw_sdk::data_path!("device-check.bin");
/// The longest report read back.
const REPORT_CAP: usize = 16 * 1024;
const CARD_CHUNK: usize = 4096;
const CARD_CHUNKS: usize = 32;
/// The radius of a touch target, in millimetres.
const TARGET_MM: u32 = 5;

/// What a control on a page does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Control {
    Answer(Outcome),
    /// The step's title, where the page has no room for its help (the
    /// largest text on a small screen): the help whole, in a dialog.
    Help,
    /// The screen page: the pattern turned black for white, with a fast
    /// refresh, which shows ghosting.
    Flip,
    /// The screen page: a full refresh, which clears it.
    FullRefresh,
    /// The light page: its levels once more.
    LightAgain,
    /// The Wi-Fi page: the Wi-Fi plugin's page, to join a network.
    WifiPage,
    /// The sleep page: sleep now.
    SleepNow,
    /// The end: all steps again.
    StartOver,
    /// The end: close the check.
    Done,
}

impl Control {
    fn label(self) -> &'static str {
        match self {
            Control::Answer(Outcome::Pass) => "diagnostics.check.pass",
            Control::Answer(Outcome::Fail) => "diagnostics.check.fail",
            Control::Answer(Outcome::Skip) => "diagnostics.check.skip",
            Control::Help => "diagnostics.check.title",
            Control::Flip => "diagnostics.check.flip",
            Control::FullRefresh => "diagnostics.check.full-refresh",
            Control::LightAgain => "diagnostics.check.light-again",
            Control::WifiPage => "diagnostics.check.wifi-page",
            Control::SleepNow => "diagnostics.check.sleep-now",
            Control::StartOver => "diagnostics.check.start-over",
            Control::Done => "diagnostics.check.done",
        }
    }
}

/// The light step going on: the level shown and the reader's own values,
/// set again at the end.
#[derive(Debug, Clone, Copy)]
struct LightTest {
    level: usize,
    kept: (i64, i64),
}

/// The radio during the Wi-Fi step: whether it was off before, so it is
/// switched off again after, and whether the search was asked for.
#[derive(Debug, Clone, Copy)]
struct Radio {
    was_off: bool,
    scanned: bool,
}

/// Where things are on a page.
#[derive(Debug, Default, Clone)]
struct Layout {
    title: Rect,
    help: Rect,
    /// The help's text size: the small one, or smaller where the help
    /// would else run under the controls (the largest text on a small
    /// screen).
    help_size: f32,
    /// What the step shows: lines of values, the gray ramp.
    body: Rect,
    /// The controls with their places, tools first.
    controls: Vec<(Control, Rect)>,
}

pub(crate) struct Check {
    style: UiStyle,
    header: ui::Header,
    layout: Layout,
    results: Results,
    /// The step shown; `None` at the end.
    step: Option<Step>,
    /// The control focused, an index into the layout's.
    focus: usize,
    /// Whether the focus shows (once a button was used; at once without
    /// touch).
    shown: bool,
    /// Why the report could not be written, if it could not.
    unsaved: Option<String>,
    /// Whether the step was entered: at the first showing.
    started: bool,
    flipped: bool,
    buttons: Buttons,
    tapped: [bool; steps::TOUCH_TARGETS.len()],
    light: Option<LightTest>,
    /// Charging seen and not charging seen, while on the battery page.
    charging_seen: [bool; 2],
    card: Option<CardTest>,
    /// Why the light step could not begin, if it could not.
    light_unkept: Option<String>,
    /// The reader's own light as the card kept it, set again at the
    /// start: the settings may still hold a level tried.
    own_light: Option<(i64, i64)>,
    /// Networks found; `None` while looking.
    networks: Option<usize>,
    radio: Option<Radio>,
    /// When the radio was asked to come on, then when looking for
    /// networks began.
    scan_from: Instant,
    /// When the device was asked to sleep, while it has not gone to sleep.
    sleep_asked: Option<Instant>,
}

impl Check {
    pub(crate) fn new(env: &Env<'_>) -> Self {
        let style = UiStyle::new(env);
        let mut check = Self {
            header: ui::Header::new(env, &style, true),
            style,
            layout: Layout::default(),
            results: read_report(env),
            step: None,
            focus: 0,
            shown: ui::focus_shown(env),
            unsaved: None,
            started: false,
            flipped: false,
            buttons: Buttons::new(env.device().input.keys),
            tapped: [false; steps::TOUCH_TARGETS.len()],
            light: None,
            charging_seen: [false; 2],
            card: None,
            light_unkept: None,
            own_light: None,
            networks: None,
            radio: None,
            scan_from: env.now(),
            sleep_asked: None,
        };
        // Asleep when last seen: how PaperPaw started tells whether it
        // woke or was started again.
        if check.results.get(Step::Sleep) == Some(&Mark::Asleep) {
            let (outcome, note) = match env.started() {
                Start::PowerOn | Start::Wake => (Outcome::Pass, "woke"),
                Start::Restart => (Outcome::Fail, "started again instead of waking"),
                _ => (Outcome::Skip, "how it started is not known"),
            };
            check.mark(env, Step::Sleep, outcome, String::from(note));
        }
        check.step = check.results.next_open();
        check.layout = check.layout(env);
        check
    }

    /// Marks `step` and writes the report.
    fn mark(&mut self, env: &Env<'_>, step: Step, outcome: Outcome, note: String) {
        self.results.set(step, Mark::Done(outcome, note));
        self.save(env);
    }

    fn save(&mut self, env: &Env<'_>) {
        let when = env.status().clock.map(|c| {
            paperpaw_sdk::time::UnixTime::from_millis(c.unix_millis_at(env.now())).iso8601()
        });
        // PaperPaw's: its kernel's (this plugin has a version of its own).
        let version = env
            .plugins()
            .kernel()
            .map_or(paperpaw_sdk::SDK_VERSION, |k| k.manifest.version)
            .to_string();
        let about = About {
            device: env.device(),
            version: &version,
            when: when.as_deref(),
        };
        let report = self.results.report(&about);
        self.unsaved = env
            .storage()
            .write(steps::REPORT, report.as_bytes())
            .err()
            .map(|e| format!("{e:?}"));
    }

    /// Shows `step`, or the end with `None`, after the steps that do not
    /// apply to this device were skipped; starts what the step measures.
    fn enter(&mut self, mut step: Option<Step>, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        self.end_light(cx);
        self.end_radio(cx);
        while let Some(s) = step
            && let Some(missing) = s.missing(env.device(), &env.caps())
        {
            self.mark(&env, s, Outcome::Skip, String::from(missing));
            step = self.results.next_open();
        }
        self.step = step;
        self.flipped = false;
        self.buttons = Buttons::new(env.device().input.keys);
        self.tapped = [false; steps::TOUCH_TARGETS.len()];
        self.charging_seen = [false; 2];
        self.card = None;
        self.networks = None;
        match step {
            Some(Step::Light) => self.start_light(cx),
            Some(Step::Battery) => self.see_charging(&env),
            // The card is tried once the page shows it is.
            Some(Step::Card) => cx.wake_at(env.now()),
            Some(Step::Wifi) => {
                // Until the radio is on, then while looking.
                self.scan_from = env.now();
                let on = env.wifi().is_some_and(|w| w.state.is_on());
                self.radio = Some(Radio {
                    was_off: !on,
                    scanned: on,
                });
                // The radio looks for networks once it is on.
                cx.command(Command::Wifi(if on {
                    WifiCommand::Scan
                } else {
                    WifiCommand::On
                }));
                cx.wake_at(env.now().add_millis(SCAN_MS));
            }
            _ => {}
        }
        self.layout = self.layout(&env);
        self.focus = self.suggested_focus(&env);
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Full);
    }

    /// Answers the step shown with `outcome` and goes on.
    fn answer(&mut self, outcome: Outcome, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let Some(step) = self.step else {
            return;
        };
        // What was measured is noted whatever the answer: a part failed
        // or skipped is the better understood for it.
        let note = self.measured(&env).1;
        self.mark(&env, step, outcome, note);
        self.enter(self.results.next_open(), cx);
    }

    /// The outcome the step's measurements suggest, and its note; no
    /// outcome while they go on.
    fn measured(&self, env: &Env<'_>) -> (Option<Outcome>, String) {
        let (outcome, note) = match self.step {
            Some(Step::Screen) => {
                let levels = gray_levels(env);
                (Outcome::Pass, format!("{levels} gray levels"))
            }
            Some(Step::Buttons) if self.buttons.asked().is_some() => return (None, String::new()),
            Some(Step::Buttons) => self.buttons.verdict(),
            Some(Step::Touch) if self.tapped.contains(&false) => return (None, String::new()),
            Some(Step::Touch) => (Outcome::Pass, String::from("corners and edges")),
            Some(Step::Light) if self.light.is_some() => return (None, String::new()),
            Some(Step::Light) => match &self.light_unkept {
                Some(e) => (Outcome::Skip, format!("own light not kept: {e}")),
                None => (Outcome::Pass, String::from("brightness and warmth")),
            },
            Some(Step::Battery) => steps::battery(env.status().battery, self.charging_seen),
            Some(Step::Card) => match &self.card {
                Some(test) => test.verdict(),
                None => return (None, String::new()),
            },
            Some(Step::Clock) => match env.status().clock {
                Some(c) => (
                    Outcome::Pass,
                    paperpaw_sdk::time::UnixTime::from_millis(c.unix_millis_at(env.now()))
                        .iso8601(),
                ),
                None => (Outcome::Skip, String::from("time of day not known")),
            },
            Some(Step::Wifi) => return self.wifi_measured(env),
            Some(Step::Sleep) => return (None, String::new()),
            Some(Step::Memory) => steps::memory(env.memory().read()),
            None => return (None, String::new()),
        };
        (Some(outcome), note)
    }

    fn wifi_measured(&self, env: &Env<'_>) -> (Option<Outcome>, String) {
        let online = env.wifi().is_some_and(|w| w.state.is_online());
        let timed_out = env.now().since(self.scan_from) >= SCAN_MS;
        let scanned = self.radio.is_none_or(|r| r.scanned);
        let fail = |note: String| (Some(Outcome::Fail), note);
        match self.networks {
            // Joined, the network found before it looked or not.
            Some(n) if online => (Some(Outcome::Pass), format!("{n} networks found, joined")),
            None if online => (Some(Outcome::Pass), String::from("joined")),
            None if !timed_out => (None, String::new()),
            None if !scanned => fail(String::from("the radio did not come on")),
            None | Some(0) => fail(String::from("no networks found")),
            Some(n) => fail(format!("{n} networks found, not joined")),
        }
    }

    /// The control focused at first: the answer suggested, else the first
    /// tool, else *Pass*.
    fn suggested_focus(&self, env: &Env<'_>) -> usize {
        let controls = &self.layout.controls;
        let wanted = match (self.step, self.measured(env).0) {
            (None, _) => Control::Done,
            (_, Some(outcome)) => Control::Answer(outcome),
            (_, None) => controls
                .iter()
                .find(|c| c.0 != Control::Help)
                .map_or(Control::Answer(Outcome::Pass), |c| c.0),
        };
        controls.iter().position(|c| c.0 == wanted).unwrap_or(0)
    }

    /// Chooses what control `control` stands for.
    fn choose(&mut self, control: Control, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        match control {
            Control::Answer(outcome) => self.answer(outcome, cx),
            Control::Help => {
                let (title, help) = texts(&env, self.step);
                cx.command(
                    ask::confirm(title, &HELP_READ)
                        .text(help)
                        .only_yes()
                        .command(),
                );
            }
            Control::Flip => {
                self.flipped = !self.flipped;
                cx.request_redraw_area(self.layout.body);
                cx.request_refresh(RefreshMode::Fast);
            }
            Control::FullRefresh => {
                cx.request_redraw();
                cx.request_refresh(RefreshMode::Full);
            }
            Control::LightAgain => self.start_light(cx),
            Control::WifiPage => cx.command(Command::run(Id::new_static(WIFI_PAGE))),
            Control::SleepNow => {
                // Marked asleep when the device goes to sleep (SUSPEND).
                self.sleep_asked = Some(env.now());
                cx.command(Command::Sleep);
                cx.wake_at(env.now().add_millis(SLEEP_WAIT_MS));
            }
            Control::StartOver => {
                self.results = Results::default();
                self.save(&env);
                self.enter(Some(Step::ALL[0]), cx);
            }
            Control::Done => cx.close(),
        }
    }

    fn start_light(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let kept = self.own_light.take().unwrap_or_else(|| {
            (
                env.settings().read(&settings::FRONTLIGHT),
                env.settings().read(&settings::FRONTLIGHT_WARMTH),
            )
        });
        // Kept on the card first: nothing is changed unless it is.
        let line = format!("{} {}\n", kept.0, kept.1);
        if let Err(e) = env.storage().write(LIGHT_FILE, line.as_bytes()) {
            self.light_unkept = Some(format!("{e:?}"));
            self.redraw_page(cx);
            return;
        }
        self.light_unkept = None;
        self.light = Some(LightTest { level: 0, kept });
        set_light(cx, LIGHT_LEVELS[0]);
        cx.wake_at(env.now().add_millis(LIGHT_MS));
    }

    /// Sets the reader's own light again from the card, where a check that
    /// ended unforeseen left it while trying the light.
    fn restore_light(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let Ok(text) = paperpaw_sdk::files::read_text(env.storage(), LIGHT_FILE, 64) else {
            return;
        };
        let mut numbers = text.split_whitespace().map(str::parse::<i64>);
        if let (Some(Ok(brightness)), Some(Ok(warmth))) = (numbers.next(), numbers.next()) {
            set_light(cx, (brightness, warmth));
            self.own_light = Some((brightness, warmth));
        }
        forget_light(&env);
    }

    /// The next light level, or the reader's own again after the last.
    fn step_light(&mut self, cx: &mut Cx<'_, '_>) {
        let Some(light) = &mut self.light else {
            return;
        };
        light.level += 1;
        match LIGHT_LEVELS.get(light.level) {
            Some(level) => {
                set_light(cx, *level);
                let now = cx.env().now();
                cx.wake_at(now.add_millis(LIGHT_MS));
            }
            None => self.end_light(cx),
        }
        self.redraw_page(cx);
    }

    /// Sets the reader's own light again, if the light step changed it.
    fn end_light(&mut self, cx: &mut Cx<'_, '_>) {
        if let Some(light) = self.light.take() {
            set_light(cx, light.kept);
            forget_light(cx.env());
        }
    }

    /// Switches the radio off again if the Wi-Fi step switched it on and
    /// nothing wants the network now.
    fn end_radio(&mut self, cx: &mut Cx<'_, '_>) {
        let wanted = cx.env().wifi().is_some_and(|w| w.wanted || w.busy);
        if self.radio.take().is_some_and(|r| r.was_off) && !wanted {
            cx.command(Command::Wifi(WifiCommand::Off));
        }
    }

    fn see_charging(&mut self, env: &Env<'_>) {
        if let Some(battery) = env.status().battery {
            self.charging_seen[usize::from(!battery.charging)] = true;
        }
    }

    fn run_card_test(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        self.card = Some(card_test(&env));
        self.redraw_page(cx);
    }

    /// The page anew, with its controls (the answer suggested may have
    /// changed) and the focus where the suggestion is.
    fn redraw_page(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        self.layout = self.layout(&env);
        self.focus = self.suggested_focus(&env);
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Partial);
    }

    fn move_focus(&mut self, delta: isize, cx: &mut Cx<'_, '_>) {
        let count = self.layout.controls.len();
        let before = self.focus;
        if !self.shown {
            self.shown = true;
        } else if count > 0 {
            self.focus = ui::step_focus(self.focus, delta, count);
        }
        // The controls the focus left and came to: buttons, and *Help*
        // as a row widened by the margin.
        let reach = |i: usize| {
            let rect = self.layout.controls.get(i)?.1;
            let wide = rect.grow(self.style.margin());
            Some(ui::button_reach(&self.style, rect).union(&Rect::new(
                wide.x,
                rect.y,
                wide.width,
                rect.height,
            )))
        };
        match (reach(before), reach(self.focus)) {
            (Some(left), Some(reached)) => ui::ListChange::focus(left, reached).request(cx),
            _ => {
                cx.request_redraw();
                cx.request_refresh(RefreshMode::Fast);
            }
        }
    }

    /// A tap on a touch target of the touch page.
    fn tap_target(&mut self, point: Point, cx: &mut Cx<'_, '_>) -> bool {
        let env = *cx.env();
        let radius = env.metrics().mm(TARGET_MM * 2) as i32;
        let hit = targets(&env)
            .iter()
            .position(|t| (point.x - t.x).abs() <= radius && (point.y - t.y).abs() <= radius);
        let Some(index) = hit else {
            return false;
        };
        if !self.tapped[index] {
            self.tapped[index] = true;
            if self.tapped.contains(&false) {
                cx.request_redraw();
                cx.request_refresh(RefreshMode::Fast);
            } else {
                self.answer(Outcome::Pass, cx);
            }
        }
        true
    }
}

impl Screen for Check {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        canvas.clear(Color::WHITE);
        let style = self.style;
        let mut ui = ui::Ui::new(canvas, env, &style);
        if self.step != Some(Step::Touch) {
            // Which step, as a note: the header's page arrows would turn
            // nothing here.
            let step = self
                .step
                .map(|s| format!("{} / {}", s.index() + 1, Step::ALL.len()));
            let title = env.i18n().tr("diagnostics.check.title");
            self.header.draw(&mut ui, title, step.as_deref(), None);
        }
        let l = &self.layout;
        let (title, help) = texts(env, self.step);
        if !l.controls.iter().any(|c| c.0 == Control::Help) {
            paragraph(&mut ui, l.title, style.title(), &title, Color::BLACK);
        }
        if l.help.height > 0 {
            paragraph(
                &mut ui,
                l.help,
                l.help_size,
                &help,
                style.faint(ui::Ink::NOTE),
            );
        }
        match self.step {
            Some(Step::Screen) => draw_screen_test(ui.canvas(), env, l.body, self.flipped),
            Some(Step::Touch) => self.draw_targets(ui.canvas(), env),
            _ => draw_lines(&mut ui, l.body, &self.lines(env)),
        }
        // The answer suggested stands out; while buttons are asked for,
        // they choose nothing.
        let suggested = self.measured(env).0.map(Control::Answer);
        for (i, (control, rect)) in l.controls.iter().enumerate() {
            let focused = self.shown && i == self.focus && !self.takes_buttons();
            if *control == Control::Help {
                // A row, widened by its margin inside: its title lines up
                // with the text below.
                let inside = style.margin();
                let row = Rect::new(
                    rect.x.saturating_sub(inside),
                    rect.y,
                    rect.width + 2 * inside,
                    rect.height,
                );
                ui::Row::value(&title, "›")
                    .focused(focused)
                    .draw(&mut ui, row);
                continue;
            }
            let label = env.i18n().tr(control.label());
            let on = Some(*control) == suggested || *control == Control::Done;
            ui.button(*rect, label, on, focused);
        }
    }

    fn grays(&self) -> bool {
        // The gray levels as the panel shows them; the pattern turned
        // shows ghosting, with fast refreshes in black and white.
        self.step == Some(Step::Screen) && !self.flipped
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        if self.started {
            // Back from the Wi-Fi page: joined or not.
            self.redraw_page(cx);
        } else {
            self.started = true;
            self.restore_light(cx);
            self.enter(self.step, cx);
        }
    }

    fn on_close(&mut self, cx: &mut Cx<'_, '_>) {
        self.end_light(cx);
        self.end_radio(cx);
    }

    fn takes_buttons(&self) -> bool {
        self.step == Some(Step::Buttons) && self.buttons.asked().is_some()
    }

    fn on_button(&mut self, name: &str, long: bool, cx: &mut Cx<'_, '_>) -> Handled {
        if !self.takes_buttons() {
            return Handled::No;
        }
        if self.buttons.press(name, long) {
            if self.buttons.asked().is_some() {
                cx.request_redraw_area(self.layout.body);
                cx.request_refresh(RefreshMode::Fast);
            } else {
                let outcome = self.buttons.verdict().0;
                self.answer(outcome, cx);
            }
        }
        Handled::Yes
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let a = action;
        if [
            actions::LEFT,
            actions::UP,
            actions::PREVIOUS,
            actions::PREV_PAGE,
        ]
        .contains(a)
        {
            self.move_focus(-1, cx);
        } else if [
            actions::RIGHT,
            actions::DOWN,
            actions::NEXT,
            actions::NEXT_PAGE,
        ]
        .contains(a)
        {
            self.move_focus(1, cx);
        } else if *a == actions::CONFIRM {
            if !self.shown {
                self.shown = true;
                cx.request_redraw();
                cx.request_refresh(RefreshMode::Fast);
            } else if let Some((control, _)) = self.layout.controls.get(self.focus) {
                self.choose(*control, cx);
            }
        } else {
            return Handled::No;
        }
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if gesture.kind != GestureKind::Tap {
            return Handled::No;
        }
        self.shown = false;
        let touch = self.step == Some(Step::Touch);
        if touch && self.tap_target(gesture.start, cx) {
            return Handled::Yes;
        }
        let hit = self
            .layout
            .controls
            .iter()
            .position(|(_, r)| r.contains(gesture.start));
        if let Some(index) = hit {
            self.focus = index;
            let control = self.layout.controls[index].0;
            self.choose(control, cx);
            return Handled::Yes;
        }
        match self.header.tap(cx.env(), &self.style, gesture.start, None) {
            Some(action) if !touch => {
                cx.command(Command::run(action));
                Handled::Yes
            }
            _ => Handled::No,
        }
    }

    fn on_timer(&mut self, cx: &mut Cx<'_, '_>) {
        match self.step {
            Some(Step::Card) if self.card.is_none() => self.run_card_test(cx),
            Some(Step::Light) => self.step_light(cx),
            // The search took too long: the page says so.
            Some(Step::Wifi) => self.redraw_page(cx),
            Some(Step::Sleep)
                if self
                    .sleep_asked
                    .is_some_and(|at| cx.env().now().since(at) >= SLEEP_WAIT_MS) =>
            {
                let env = *cx.env();
                self.sleep_asked = None;
                let note = String::from("did not go to sleep");
                self.mark(&env, Step::Sleep, Outcome::Fail, note);
                self.enter(self.results.next_open(), cx);
            }
            _ => {}
        }
    }

    fn on_event(&mut self, event: &Id, _: &Params, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        match self.step {
            Some(Step::Battery)
                if event == events::STATUS_CHANGED.id()
                    || event == events::BATTERY_MEASURED.id() =>
            {
                self.see_charging(&env);
                self.redraw_page(cx);
            }
            Some(Step::Wifi) if event == events::WIFI_SCANNED.id() => {
                self.networks = env.wifi().map(|w| w.networks.len());
                self.redraw_page(cx);
            }
            Some(Step::Wifi) if event == events::WIFI_CHANGED.id() => {
                if let Some(radio) = &mut self.radio
                    && !radio.scanned
                    && env.wifi().is_some_and(|w| w.state.is_on())
                {
                    radio.scanned = true;
                    cx.command(Command::Wifi(WifiCommand::Scan));
                    // The search has its time from now.
                    self.scan_from = env.now();
                    cx.wake_at(env.now().add_millis(SCAN_MS));
                }
                self.redraw_page(cx);
            }
            Some(Step::Sleep) if event == events::SUSPEND.id() && self.sleep_asked.is_some() => {
                self.sleep_asked = None;
                self.results.set(Step::Sleep, Mark::Asleep);
                self.save(&env);
            }
            Some(Step::Sleep)
                if event == events::RESUME.id()
                    && self.results.get(Step::Sleep) == Some(&Mark::Asleep) =>
            {
                self.mark(&env, Step::Sleep, Outcome::Pass, String::from("woke"));
                self.enter(self.results.next_open(), cx);
            }
            _ => {}
        }
    }
}

impl Check {
    fn layout(&self, env: &Env<'_>) -> Layout {
        let style = &self.style;
        let display = env.device().display;
        // The header's margin, so that everything lines up with its title.
        let margin = style.margin();
        let gap = style.gap();
        let width = display.width.saturating_sub(2 * margin);
        let (title_text, help_text) = texts(env, self.step);
        let lines = |size: f32, text: &str| -> u16 {
            let count = style
                .font(env)
                .map_or(1, |f| f.wrap(size, text, f32::from(width)).len().max(1));
            line_height(env, style, size) * count as u16
        };
        // The touch page has no header: its corners are for the targets,
        // and its text begins below them.
        let touch = self.step == Some(Step::Touch);
        let mut y = if touch {
            (env.metrics().mm(TARGET_MM * 2 + 2) + u32::from(style.gap())) as u16
        } else {
            self.header.area().y + self.header.area().height + gap * 2
        };
        let title = Rect::new(margin, y, width, lines(style.title(), &title_text));
        y += title.height + gap;

        let button = style.row();
        let answers = self.answers();
        let tools = self.tools(env);
        // The touch page keeps its edges free for the targets: its
        // answers go between the targets halfway down and those at the
        // bottom.
        let bottom = if touch {
            display.height / 4 * 3 + button / 2
        } else {
            display.height.saturating_sub(margin)
        };
        let answer_row = Rect::new(margin, bottom.saturating_sub(button), width, button);
        let tool_row = Rect::new(
            margin,
            answer_row.y.saturating_sub(gap * 2 + button),
            width,
            button,
        );
        let above = if tools.is_empty() {
            answer_row
        } else {
            tool_row
        };
        let body_bottom = if touch {
            display.height.saturating_sub(margin)
        } else {
            above.y.saturating_sub(gap * 2)
        };
        // The help whole, with a row's room at least left for the body.
        let room = body_bottom.saturating_sub(y + gap * 2 + button);
        let small = style.small();
        let help_size = paperpaw_sdk::widgets::largest_size(small, small * 0.6, |at| {
            lines(at, &help_text) <= room
        });
        // Where it does not fit so, the title opens it.
        let help = match help_size {
            Some(size) => Rect::new(margin, y, width, lines(size, &help_text)),
            None => Rect::new(margin, y, width, 0),
        };
        if help.height > 0 {
            y += help.height + gap * 2;
        } else {
            y += gap;
        }
        let help_size = help_size.unwrap_or(small);
        let body = Rect::new(margin, y, width, body_bottom.saturating_sub(y));
        let mut controls = Vec::new();
        if help.height == 0 {
            controls.push((Control::Help, title));
        }
        for (row, list) in [(tool_row, &tools), (answer_row, &answers)] {
            for (control, rect) in list.iter().zip(ui::columns(row, list.len(), gap)) {
                controls.push((*control, rect));
            }
        }
        Layout {
            title,
            help,
            help_size,
            body,
            controls,
        }
    }

    fn answers(&self) -> Vec<Control> {
        match self.step {
            None => alloc::vec![Control::StartOver, Control::Done],
            Some(_) => Outcome::ALL.into_iter().map(Control::Answer).collect(),
        }
    }

    fn tools(&self, env: &Env<'_>) -> Vec<Control> {
        match self.step {
            Some(Step::Screen) => alloc::vec![Control::Flip, Control::FullRefresh],
            Some(Step::Light) if self.light.is_none() => alloc::vec![Control::LightAgain],
            Some(Step::Wifi) if env.actions().contains(WIFI_PAGE) => alloc::vec![Control::WifiPage],
            Some(Step::Sleep) => alloc::vec![Control::SleepNow],
            _ => Vec::new(),
        }
    }

    /// The lines of values a page shows: a name and its value each.
    fn lines(&self, env: &Env<'_>) -> Vec<(String, String)> {
        let tr = |key: &str| env.i18n().tr(key).to_string();
        let mut lines = Vec::new();
        match self.step {
            Some(Step::Buttons) => {
                let asked = self.buttons.asked();
                for name in &self.buttons.names {
                    let state = if self.buttons.pressed.contains(name) {
                        "diagnostics.check.button.pressed"
                    } else if self.buttons.failed.contains(name) {
                        "diagnostics.check.button.failed"
                    } else if asked == Some(*name) {
                        "diagnostics.check.button.asked"
                    } else {
                        "diagnostics.check.button.waiting"
                    };
                    lines.push((button_name(env, name), tr(state)));
                }
            }
            Some(Step::Light) => {
                let level = self.light.and_then(|l| LIGHT_LEVELS.get(l.level));
                if let Some((brightness, warmth)) = level {
                    lines.push((
                        tr("diagnostics.check.brightness"),
                        format!("{brightness} %"),
                    ));
                    lines.push((tr("diagnostics.check.warmth"), format!("{warmth} %")));
                }
            }
            Some(Step::Battery) => battery_lines(env, &mut lines),
            Some(Step::Card) => self.card_lines(env, &mut lines),
            Some(Step::Clock) => {
                let text = paperpaw_sdk::time::clock_text(env);
                let date = paperpaw_sdk::time::date_text(env);
                let unknown = || tr("diagnostics.check.unknown");
                lines.push((tr("diagnostics.check.time"), text.unwrap_or_else(unknown)));
                lines.push((tr("diagnostics.check.date"), date.unwrap_or_else(unknown)));
            }
            Some(Step::Wifi) => self.wifi_lines(env, &mut lines),
            Some(Step::Memory) => memory_lines(env, &mut lines),
            None => self.summary_lines(env, &mut lines),
            _ => {}
        }
        lines
    }

    fn card_lines(&self, env: &Env<'_>, lines: &mut Vec<(String, String)>) {
        let tr = |key: &str| env.i18n().tr(key).to_string();
        let Some(test) = &self.card else {
            lines.push((tr("diagnostics.check.measuring"), String::new()));
            return;
        };
        if let Some(error) = &test.error {
            lines.push((tr("diagnostics.check.error"), error.clone()));
            return;
        }
        let speed = |us: Option<u64>| {
            us.map_or_else(
                || tr("diagnostics.check.unknown"),
                |us| format!("{} KiB/s", steps::speed(test.bytes, us)),
            )
        };
        lines.push((tr("diagnostics.check.write"), speed(test.write_us)));
        lines.push((tr("diagnostics.check.read"), speed(test.read_us)));
    }

    fn wifi_lines(&self, env: &Env<'_>, lines: &mut Vec<(String, String)>) {
        let tr = |key: &str| env.i18n().tr(key).to_string();
        let found = match self.networks {
            Some(n) => n.to_string(),
            None => tr("diagnostics.check.looking"),
        };
        lines.push((tr("diagnostics.check.networks"), found));
        let online = env.wifi().is_some_and(|w| w.state.is_online());
        lines.push((tr("diagnostics.check.joined"), yes_no(env, online)));
    }

    fn summary_lines(&self, env: &Env<'_>, lines: &mut Vec<(String, String)>) {
        for step in Step::ALL {
            let outcome = self
                .results
                .outcome(step)
                .map_or("", |o| env.i18n().tr(Control::Answer(o).label()));
            lines.push((step_title(env, step), outcome.to_string()));
        }
        // Where the report is, the help says; here only that it is not.
        if let Some(error) = &self.unsaved {
            let report = env.i18n().tr("diagnostics.check.report").to_string();
            let unsaved = env
                .i18n()
                .format("diagnostics.check.unsaved", &[("error", Arg::Str(error))]);
            lines.push((report, unsaved));
        }
    }

    fn draw_targets(&self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let radius = env.metrics().mm(TARGET_MM);
        let line = env.metrics().tenth_mm(5);
        for (target, tapped) in targets(env).iter().zip(self.tapped) {
            let (x, y) = (target.x - radius as i32, target.y - radius as i32);
            let d = radius * 2;
            if tapped {
                canvas.fill_round_rect(x, y, d, d, radius, Color::BLACK);
            } else {
                canvas.stroke_round_rect(x, y, d, d, line, radius, Color::BLACK);
            }
        }
    }
}

/// The report written before, to go on from; empty where there is none.
fn read_report(env: &Env<'_>) -> Results {
    paperpaw_sdk::files::read_text(env.storage(), steps::REPORT, REPORT_CAP)
        .map_or_else(|_| Results::default(), |text| Results::parse(&text))
}

/// Writes 128 KiB to the card, reads them back and compares them, timed
/// where the device has a clock; the file goes again after.
fn card_test(env: &Env<'_>) -> CardTest {
    let bytes = CARD_CHUNK * CARD_CHUNKS;
    let mut test = CardTest {
        bytes,
        write_us: None,
        read_us: None,
        error: None,
    };
    let path = CARD_FILE;
    let chunk: Vec<u8> = (0..CARD_CHUNK).map(|i| (i % 251) as u8).collect();
    let storage = env.storage();
    let started = env.clock().micros();
    let written = storage
        .write(path, &chunk)
        .and_then(|()| (1..CARD_CHUNKS).try_for_each(|_| storage.append(path, &chunk)));
    let wrote = env.clock().micros();
    if let Err(e) = written {
        test.error = Some(format!("write: {e:?}"));
        let _removed = storage.remove(path);
        return test;
    }
    let mut back = alloc::vec![0u8; CARD_CHUNK];
    let mut same = true;
    for i in 0..CARD_CHUNKS {
        match storage.read_at(path, (i * CARD_CHUNK) as u64, &mut back) {
            Ok(n) if n == CARD_CHUNK => same &= back == chunk,
            Ok(_) => same = false,
            Err(e) => {
                test.error = Some(format!("read: {e:?}"));
                break;
            }
        }
    }
    let read = env.clock().micros();
    if let Err(e) = storage.remove(path) {
        test.error.get_or_insert(format!("remove: {e:?}"));
    }
    if !same {
        test.error.get_or_insert(String::from("read back differs"));
    }
    if let (Some(a), Some(b), Some(c)) = (started, wrote, read) {
        test.write_us = Some(b - a);
        test.read_us = Some(c - b);
    }
    test
}

/// Removes the reader's own light kept on the card, once it is set again.
fn forget_light(env: &Env<'_>) {
    #[expect(
        clippy::let_underscore_must_use,
        reason = "the file is gone or stays; kept, it only sets the reader's own light once more"
    )]
    let _ = env.storage().remove(LIGHT_FILE);
}

fn set_light(cx: &mut Cx<'_, '_>, (brightness, warmth): (i64, i64)) {
    cx.command(Command::SetSetting(
        settings::FRONTLIGHT.id().clone(),
        SettingValue::Int(brightness),
    ));
    cx.command(Command::SetSetting(
        settings::FRONTLIGHT_WARMTH.id().clone(),
        SettingValue::Int(warmth),
    ));
}

/// The centres of the touch targets on the screen.
fn targets(env: &Env<'_>) -> [Point; steps::TOUCH_TARGETS.len()] {
    let display = env.device().display;
    let inset = env.metrics().mm(TARGET_MM + 2) as i32;
    let span =
        |size: u16, tenths: u8| inset + (i32::from(size) - 2 * inset) * i32::from(tenths) / 10;
    steps::TOUCH_TARGETS.map(|(x, y)| Point::new(span(display.width, x), span(display.height, y)))
}

/// The gray levels the screen page shows: a panel of black and white
/// shows the page in four grays, as it shows pictures.
fn gray_levels(env: &Env<'_>) -> u32 {
    let format = match env.device().display.format {
        PixelFormat::Mono1 => PixelFormat::Gray2,
        format => format,
    };
    format.gray_levels().unwrap_or(16).clamp(2, 16) as u32
}

/// A step's title and what to do on its page; the end's at the end.
fn texts(env: &Env<'_>, step: Option<Step>) -> (String, String) {
    let key = step.map_or("end", Step::key);
    (
        env.i18n()
            .tr(&format!("diagnostics.check.{key}.title"))
            .to_string(),
        env.i18n()
            .tr(&format!("diagnostics.check.{key}.help"))
            .to_string(),
    )
}

fn step_title(env: &Env<'_>, step: Step) -> String {
    texts(env, Some(step)).0
}

/// A button's name as the reader knows it; its name on the device where
/// there is no translation.
fn button_name(env: &Env<'_>, name: &str) -> String {
    let key = format!("diagnostics.button.{name}");
    env.i18n().get(&key).unwrap_or(name).to_string()
}

fn yes_no(env: &Env<'_>, yes: bool) -> String {
    let key = if yes {
        "diagnostics.check.yes"
    } else {
        "diagnostics.check.no"
    };
    env.i18n().tr(key).to_string()
}

fn battery_lines(env: &Env<'_>, lines: &mut Vec<(String, String)>) {
    let tr = |key: &str| env.i18n().tr(key).to_string();
    let Some(b) = env.status().battery else {
        lines.push((tr("diagnostics.check.no-battery"), String::new()));
        return;
    };
    lines.push((tr("diagnostics.check.charge"), format!("{} %", b.percent)));
    if let Some(mv) = b.millivolts {
        lines.push((tr("diagnostics.check.voltage"), format!("{mv} mV")));
    }
    lines.push((tr("diagnostics.check.charging"), yes_no(env, b.charging)));
}

fn memory_lines(env: &Env<'_>, lines: &mut Vec<(String, String)>) {
    let tr = |key: &str| env.i18n().tr(key).to_string();
    let Some(m) = env.memory().read() else {
        lines.push((tr("diagnostics.check.unknown"), String::new()));
        return;
    };
    let of = |free: u32, size: u32| {
        env.i18n().format(
            "diagnostics.check.free-of",
            &[
                ("free", Arg::Int(i64::from(free / 1024))),
                ("size", Arg::Int(i64::from(size / 1024))),
            ],
        )
    };
    lines.push((tr("diagnostics.check.heap"), of(m.heap_free, m.heap_size)));
    if let (Some(free), Some(size)) = (m.stack_free, m.stack_size) {
        lines.push((
            tr("diagnostics.check.stack"),
            format!("{free} B / {size} B"),
        ));
    }
}

fn line_height(env: &Env<'_>, style: &UiStyle, size: f32) -> u16 {
    style
        .font(env)
        .map_or(size * 1.3, |f| f.line_metrics(size).line_height) as u16
}

/// Draws `text` wrapped into `area`, as many lines as fit.
fn paragraph(ui: &mut ui::Ui<'_>, area: Rect, size: f32, text: &str, color: Color) {
    let Some(font) = ui.font() else {
        return;
    };
    ui.canvas().describe(Role::Text, text, area);
    let height = font.line_metrics(size).line_height as u16;
    let mut top = area.y;
    for range in font.wrap(size, text, f32::from(area.width)) {
        if top + height > area.y + area.height {
            break;
        }
        let row = Rect::new(area.x, top, area.width, height);
        ui::Label::new(&text[range])
            .px(size)
            .color(color)
            .draw_from(ui, f32::from(area.x), row);
        top += height;
    }
}

/// Lines of a name and its value, one a row, as many as fit.
fn draw_lines(ui: &mut ui::Ui<'_>, area: Rect, lines: &[(String, String)]) {
    let (env, style) = (ui.env(), ui.style());
    // A row each, closer where they do not fit so (the end's list).
    let line = line_height(env, style, style.small());
    let count = u16::try_from(lines.len()).unwrap_or(u16::MAX).max(1);
    let row = (style.row()).min(area.height / count).max(line);
    // A row keeps a margin inside: widened by it, its text lines up with
    // the text above.
    let inside = style.margin();
    let mut top = area.y;
    for (name, value) in lines {
        if top + row > area.y + area.height {
            break;
        }
        let rect = Rect::new(
            area.x.saturating_sub(inside),
            top,
            area.width + 2 * inside,
            row,
        );
        ui::Row::value(name, value).draw(ui, rect);
        top += row;
    }
}

/// The screen's test: a stripe for each gray level, and below it a
/// checkerboard, turned black for white with `flipped`.
fn draw_screen_test(canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect, flipped: bool) {
    let levels = gray_levels(env);
    let (x, y, w) = (i32::from(area.x), i32::from(area.y), u32::from(area.width));
    let ramp = u32::from(area.height) / 3;
    for level in 0..levels {
        let x0 = x + (w * level / levels) as i32;
        let x1 = x + (w * (level + 1) / levels) as i32;
        let gray = (level * 255 / (levels - 1)) as u8;
        canvas.fill_rect(x0, y, (x1 - x0) as u32, ramp, Color::gray(gray));
    }
    canvas.stroke_rect(x, y, w, ramp, env.metrics().tenth_mm(3), Color::BLACK);
    // Squares of about 6 mm, as many as fill the width exactly.
    let columns = (w / env.metrics().mm(6).max(4)).max(1);
    let cell = w / columns;
    let top = y + ramp as i32 + env.metrics().mm(3) as i32;
    let bottom = i32::from(area.y) + i32::from(area.height);
    let mut row = 0;
    let mut cy = top;
    while cy + cell as i32 <= bottom {
        for col in 0..columns {
            let black = (row + col) % 2 == 0;
            let color = if black != flipped {
                Color::BLACK
            } else {
                Color::WHITE
            };
            let cx = x + (col * cell) as i32;
            canvas.fill_rect(cx, cy, cell, cell, color);
        }
        cy += cell as i32;
        row += 1;
    }
}

/// The check's screen, for `ScreenSpec`.
pub(crate) fn make(env: &Env<'_>) -> Box<dyn Screen> {
    Box::new(Check::new(env))
}
