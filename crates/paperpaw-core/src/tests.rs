//! Kernel behaviour tests: slots, priorities, key map layers, commands,
//! settings and transactional plugin registration.

extern crate std;

use crate::{Place, Turn};
use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::rc::Rc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cell::RefCell;

use crate::Ask;
use crate::Staging;
use crate::kit::action::ActionSpec;
use paperpaw_platform::{
    ButtonState, Color, DeviceInfo, DisplayInfo, InputCaps, InputEvent, Instant, PixelFormat,
    Point, PointerEvent, PointerPhase, PointerTool, Rect, RefreshMode, Rotation,
};
use paperpaw_sdk::builtin::{actions, screens, settings, slots};
use paperpaw_sdk::{
    Canvas, Handled, Id, Manifest, Priority, RegisterError, SettingKind, SettingSpec, SettingValue,
    Version,
};

use crate::{HttpCall, Io, Kernel, ServerCall, SpeechCall, WarningSource};

/// What the shell is asked, one kind at a time, as the parts keep it: the
/// tests of each part look at its own.
impl Kernel {
    /// A plugin's request, as [`Ask::Http`] makes it.
    fn request(&mut self, request: paperpaw_sdk::HttpRequest) {
        self.output.commands.push(Ask::http(request));
        self.run_commands();
    }

    fn take_requests(&mut self) -> Vec<HttpCall> {
        if self.network_ready() {
            self.io.http.take()
        } else {
            Vec::new()
        }
    }

    fn take_server_calls(&mut self) -> Vec<ServerCall> {
        if self.network_ready() {
            self.io.servers.take()
        } else {
            Vec::new()
        }
    }

    fn take_speech(&mut self) -> Vec<SpeechCall> {
        self.io.speech.take()
    }

    fn take_wifi_calls(&mut self) -> Vec<paperpaw_sdk::wifi::WifiCommand> {
        core::mem::take(&mut self.io.wifi_calls)
    }
}

pub(crate) const DEVICE: DeviceInfo = DeviceInfo::new(
    "test",
    "Test device",
    DisplayInfo::new(90, 90, 100, PixelFormat::Gray4, Rotation::Deg0, None),
    InputCaps::new(true, true, false, &[]),
    paperpaw_platform::MemoryClass::Large,
);

pub(crate) type Log = Rc<RefCell<Vec<String>>>;

/// A configurable plugin providing one screen bound to the home slot.
pub(crate) struct TestPlugin {
    id: &'static str,
    priority: Priority,
    log: Log,
    /// Registers an id outside its namespace after its other contributions.
    misbehave: bool,
    keymap: Option<&'static str>,
}

impl TestPlugin {
    fn new(id: &'static str, log: &Log) -> Self {
        Self {
            id,
            priority: Priority::DEFAULT,
            log: log.clone(),
            misbehave: false,
            keymap: None,
        }
    }

    fn screen_id(&self) -> Id {
        Id::parse(&alloc::format!("{}.screen", self.id)).unwrap()
    }
}

impl TestPlugin {
    pub(crate) fn manifest(&self) -> Manifest {
        Manifest::new(Id::parse(self.id).unwrap(), "Test", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }
}

fn test_plugin(kept: TestPlugin) -> impl Fn(&mut Staging) -> Result<(), RegisterError> {
    move |r| {
        let log = kept.log.clone();
        let name = kept.id;
        r.add_fake(crate::testing::FakeSpec::new(
            kept.screen_id(),
            "test.screen",
            Box::new(move |_, _| {
                Box::new(TestScreen {
                    name,
                    log: log.clone(),
                })
            }),
        ))?;
        r.bind_slot(slots::HOME, kept.screen_id(), kept.priority)?;
        r.add_action(ActionSpec::new(
            Id::parse(&alloc::format!("{}.special", kept.id)).unwrap(),
            "test.special",
        ))?;
        r.add_setting(SettingSpec::new(
            Id::parse(&alloc::format!("{}.level", kept.id)).unwrap(),
            "test.level",
            SettingKind::Int { min: 1, max: 5 },
            SettingValue::Int(2),
            paperpaw_sdk::builtin::groups::OTHER,
        ))?;
        if let Some(keymap) = kept.keymap {
            r.add_keymap(Cow::Borrowed(keymap))?;
        }
        if kept.misbehave {
            r.add_action(ActionSpec::new(Id::new_static("core.stolen"), ""))?;
        }
        Ok(())
    }
}

struct TestScreen {
    name: &'static str,
    log: Log,
}

impl crate::testing::Fake for TestScreen {
    fn draw(&mut self, canvas: &mut Canvas<'_>, _: &Place<'_>) {
        canvas.fill_rect(0, 0, 10, 10, Color::BLACK);
    }

    fn on_action(&mut self, action: &Id, cx: &mut Turn<'_, '_>) -> Handled {
        self.log
            .borrow_mut()
            .push(alloc::format!("{}:{}", self.name, action));
        if *action == actions::NEXT_PAGE {
            cx.request_redraw_area(Rect::new(20, 20, 5, 5));
            cx.request_refresh(RefreshMode::Partial);
            return Handled::Yes;
        }
        if action.namespace() == self.name || *action == actions::CONFIRM {
            return Handled::Yes;
        }
        Handled::No
    }
}

/// Draws what changed, like a shell after each call.
fn render(kernel: &mut Kernel) {
    kernel.render(&mut |_, _| {});
}

/// A short press; the effects of what it did, on the press (a button
/// without a long press) or as it is let go.
fn press(kernel: &mut Kernel, at: u64, name: &'static str) -> crate::Effects {
    let pressed = kernel.handle(
        Instant(at),
        InputEvent::Button {
            name,
            state: ButtonState::Pressed,
        },
    );
    let released = kernel.handle(
        Instant(at + 50),
        InputEvent::Button {
            name,
            state: ButtonState::Released,
        },
    );
    render(kernel);
    if pressed.refresh().is_some() || pressed.sleep() || pressed.restart() {
        pressed
    } else {
        released
    }
}

fn tap(kernel: &mut Kernel, at: u64, x: i32, y: i32) -> crate::Effects {
    let event = |phase| {
        InputEvent::Pointer(PointerEvent::new(
            0,
            phase,
            Point::new(x, y),
            PointerTool::Finger,
            1000,
        ))
    };
    kernel.handle(Instant(at), event(PointerPhase::Down));
    let effects = kernel.handle(Instant(at + 60), event(PointerPhase::Up));
    render(kernel);
    effects
}

pub(crate) fn log() -> Log {
    Rc::new(RefCell::new(Vec::new()))
}

std::thread_local! {
    /// The log this test's handlers write to ([`heard`]): a handler is a
    /// plain function, it holds no log of its own.
    static HEARD: RefCell<Option<Log>> = const { RefCell::new(None) };
}

/// Makes `log` the one [`heard`] writes to, for the test running.
pub(crate) fn hear_into(log: &Log) {
    HEARD.with(|heard| *heard.borrow_mut() = Some(Rc::clone(log)));
}

/// Writes `line` to the log of [`hear_into`], if one.
pub(crate) fn heard(line: impl Into<String>) {
    HEARD.with(|heard| {
        if let Some(log) = heard.borrow().as_ref() {
            log.borrow_mut().push(line.into());
        }
    });
}

#[test]
fn empty_home_shows_placeholder() {
    let mut kernel = Kernel::builder(DEVICE).build().unwrap();
    let effects = kernel.start(Instant(0));
    assert_eq!(kernel.active_screen(), Some(&screens::PLACEHOLDER));
    assert_eq!(
        effects.refresh(),
        Some((Rect::new(0, 0, 90, 90), RefreshMode::Full))
    );
    assert!(kernel.take_warnings().is_empty());
}

#[test]
fn highest_priority_wins_and_user_config_overrides() {
    let log = log();
    let mut low = TestPlugin::new("low", &log);
    low.priority = Priority::FALLBACK;
    let high = TestPlugin::new("high", &log);

    let mut kernel = Kernel::builder(DEVICE)
        .test_plugin(high)
        .test_plugin(low)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    assert_eq!(kernel.active_screen().map(Id::as_str), Some("high.screen"));

    let mut kernel = Kernel::builder(DEVICE)
        .test_plugin(TestPlugin::new("high", &log))
        .test_plugin(TestPlugin::new("low", &log))
        .user_config("[slots]\ncore.home = low.screen\n")
        .build()
        .unwrap();
    kernel.start(Instant(0));
    assert_eq!(kernel.active_screen().map(Id::as_str), Some("low.screen"));
}

struct OpenerScreen;

impl crate::testing::Fake for OpenerScreen {
    fn draw(&mut self, canvas: &mut Canvas<'_>, _: &Place<'_>) {
        canvas.fill_rect(0, 0, 10, 10, Color::BLACK);
    }

    fn on_action(&mut self, action: &Id, cx: &mut Turn<'_, '_>) -> Handled {
        if *action == actions::CONFIRM {
            cx.command(Ask::OpenScreen(
                screens::PLACEHOLDER,
                paperpaw_sdk::Params::new(),
            ));
            return Handled::Yes;
        }
        Handled::No
    }
}

/// Opens the placeholder screen on *confirm*: a screen change to time.
pub(crate) fn opener() -> impl Fn(&mut Staging) -> Result<(), RegisterError> {
    |r| {
        r.add_fake(crate::testing::FakeSpec::new(
            Id::new_static("opener.screen"),
            "test.screen",
            Box::new(|_, _| Box::new(OpenerScreen)),
        ))?;
        r.bind_slot(
            slots::HOME,
            Id::new_static("opener.screen"),
            Priority::DEFAULT,
        )
    }
}

/// What [`opener`] registers under.
pub(crate) fn opener_manifest() -> Manifest {
    Manifest::new(Id::new_static("opener"), "Opener", Version::new(1, 0, 0))
        .uses(paperpaw_sdk::Uses::NOTHING)
}

/// A clock that stands still: all work takes no time.
fn still_clock() -> u64 {
    1_000_000
}

#[test]
fn slow_work_shows_the_busy_mark_first() {
    let mut kernel = Kernel::builder(DEVICE)
        .items(opener_manifest(), opener())
        .busy_mark(true)
        .clock(still_clock)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    render(&mut kernel);
    // A screen not opened before: the mark first, the work held back.
    let effects = press(&mut kernel, 100, "confirm");
    assert!(effects.proceed() && effects.refresh().is_some());
    assert_eq!(
        kernel.active_screen().map(Id::as_str),
        Some("opener.screen")
    );
    kernel.proceed(Instant(200));
    render(&mut kernel);
    assert_eq!(kernel.active_screen(), Some(&screens::PLACEHOLDER));
    // Back, not timed yet but going back is taken to be quick (#118);
    // then opening again, known to be quick: no mark.
    assert!(!press(&mut kernel, 300, "back").proceed());
    render(&mut kernel);
    let effects = press(&mut kernel, 500, "confirm");
    assert!(!effects.proceed());
    assert_eq!(kernel.active_screen(), Some(&screens::PLACEHOLDER));
}

#[test]
fn presses_while_a_screen_comes_up_are_dropped() {
    let log = log();
    let mut kernel = Kernel::builder(DEVICE)
        .test_plugin(TestPlugin::new("a", &log))
        .build()
        .unwrap();
    // The home screen came up; the panel showed it at 1000.
    kernel.start(Instant(0));
    render(&mut kernel);
    kernel.shown(Instant(1000));
    // Pressed while it came up: dropped, its release too.
    press(&mut kernel, 500, "page-forward");
    assert!(log.borrow().is_empty());
    // Pressed after it showed: taken.
    press(&mut kernel, 1200, "page-forward");
    kernel.shown(Instant(2000));
    // Turning pages on the same screen keeps a press made before the
    // last page showed: a frame behind.
    press(&mut kernel, 1500, "page-forward");
    assert_eq!(*log.borrow(), ["a:core.next-page", "a:core.next-page"]);
    // Two frames behind: dropped, so that quick presses on a slow screen
    // do not line up.
    kernel.shown(Instant(3000));
    press(&mut kernel, 1800, "page-forward");
    press(&mut kernel, 2500, "page-forward");
    assert_eq!(
        *log.borrow(),
        ["a:core.next-page", "a:core.next-page", "a:core.next-page"]
    );
    // Two frames behind but less than a second before the last: made
    // while the device was busy, it counts.
    kernel.shown(Instant(3500));
    press(&mut kernel, 2900, "page-forward");
    assert_eq!(log.borrow().len(), 4);
}

#[test]
fn buttons_route_through_key_map_to_screen_and_handlers() {
    let log = log();
    let mut kernel = Kernel::builder(DEVICE)
        .test_plugin(TestPlugin::new("a", &log))
        .build()
        .unwrap();
    kernel.start(Instant(0));

    let effects = press(&mut kernel, 100, "page-forward");
    assert_eq!(
        effects.refresh(),
        Some((Rect::new(20, 20, 5, 5), RefreshMode::Partial))
    );
    press(&mut kernel, 300, "confirm");
    // `back` is not handled by the screen; the global handler pops, which
    // is a no-op on the root screen.
    let effects = press(&mut kernel, 500, "back");
    assert_eq!(effects.refresh(), None);
    assert_eq!(
        *log.borrow(),
        ["a:core.next-page", "a:core.confirm", "a:core.back"]
    );
    assert!(kernel.take_warnings().is_empty());
}

#[test]
fn screen_layer_overrides_global_layer_and_user_overrides_plugin() {
    let log = log();
    let mut plugin = TestPlugin::new("a", &log);
    plugin.keymap = Some("[keymap.a.screen]\nbutton.confirm = a.special\n");

    let mut kernel = Kernel::builder(DEVICE).test_plugin(plugin).build().unwrap();
    kernel.start(Instant(0));
    press(&mut kernel, 100, "confirm");
    assert_eq!(*log.borrow(), ["a:a.special"]);

    log.borrow_mut().clear();
    let mut plugin = TestPlugin::new("a", &log);
    plugin.keymap = Some("[keymap.a.screen]\nbutton.confirm = a.special\n");
    let mut kernel = Kernel::builder(DEVICE)
        .test_plugin(plugin)
        .user_config("[keymap.a.screen]\nbutton.confirm = none\n")
        .build()
        .unwrap();
    kernel.start(Instant(0));
    press(&mut kernel, 100, "confirm");
    assert!(log.borrow().is_empty());
}

#[test]
fn failing_plugin_leaves_no_traces() {
    let log = log();
    let mut plugin = TestPlugin::new("bad", &log);
    plugin.misbehave = true;

    let mut kernel = Kernel::builder(DEVICE).test_plugin(plugin).build().unwrap();
    kernel.start(Instant(0));
    assert_eq!(kernel.active_screen(), Some(&screens::PLACEHOLDER));
    assert!(kernel.settings().get("bad.level").is_none());
    let bad = kernel.plugins().get("bad").unwrap();
    assert_eq!(bad.state, paperpaw_sdk::PluginState::Failed);

    let warnings = kernel.take_warnings();
    assert_eq!(warnings.len(), 1);
    assert_eq!(
        warnings[0].source,
        WarningSource::Plugin(Id::new_static("bad"))
    );
}

#[test]
fn disabled_plugins_are_skipped() {
    let log = log();
    let mut kernel = Kernel::builder(DEVICE)
        .test_plugin(TestPlugin::new("off", &log))
        .user_config("[plugins]\noff = no\n")
        .build()
        .unwrap();
    kernel.start(Instant(0));
    assert_eq!(kernel.active_screen(), Some(&screens::PLACEHOLDER));
    assert!(kernel.take_warnings().is_empty());
}

#[test]
fn plugins_can_ship_switched_off() {
    let log = log();
    let enabled = |config: &str| {
        let kernel = Kernel::builder(DEVICE)
            .test_plugin_off(TestPlugin::new("web", &log))
            .user_config(config)
            .build()
            .unwrap();
        kernel.shared.plugins.get("web").map(|info| info.enabled)
    };
    assert_eq!(enabled(""), Some(false), "listed, but off");
    assert_eq!(
        enabled(
            "[plugins]
web = on
"
        ),
        Some(true)
    );
    assert_eq!(
        enabled(
            "[plugins]
web = off
"
        ),
        Some(false)
    );
}

#[test]
fn user_settings_are_applied_and_validated() {
    let log = log();
    let mut kernel = Kernel::builder(DEVICE)
        .test_plugin(TestPlugin::new("a", &log))
        .user_config(
            "[core]\nlanguage = de\nfull-refresh-interval = 2\n\
             [a]\nlevel = 4\n[b]\nlevel = 1\n[a]\nlevel = 99\n",
        )
        .build()
        .unwrap();
    assert_eq!(kernel.translator().language(), "de");
    assert_eq!(kernel.translator().tr("core.action.home"), "Startseite");
    assert_eq!(kernel.settings().int("a.level"), 4);
    let warnings: Vec<String> = kernel
        .take_warnings()
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(warnings.len(), 2, "{warnings:?}");
    assert!(warnings[0].contains("`b.level`: unknown setting"));
    assert!(warnings[1].contains("`a.level`: invalid value"));

    // Every second partial refresh of a page would become full; this
    // small part of the screen never does.
    assert_eq!(kernel.settings().int("core.full-refresh-interval"), 2);
    kernel.start(Instant(0));
    let first = press(&mut kernel, 100, "page-forward");
    let second = press(&mut kernel, 300, "page-forward");
    assert_eq!(first.refresh().map(|r| r.1), Some(RefreshMode::Partial));
    assert_eq!(second.refresh().map(|r| r.1), Some(RefreshMode::Partial));
}

#[test]
fn long_tap_toggles_night_mode() {
    let mut kernel = Kernel::builder(DEVICE).build().unwrap();
    kernel.start(Instant(0));
    let down = InputEvent::Pointer(PointerEvent::new(
        0,
        PointerPhase::Down,
        Point::new(45, 45),
        PointerTool::Finger,
        1000,
    ));
    let effects = kernel.handle(Instant(1000), down);
    assert_eq!(effects.wake_at(), Some(Instant(1500)));
    let effects = kernel.tick(Instant(1500));
    assert!(kernel.settings().read(&settings::NIGHT_MODE));
    assert_eq!(effects.refresh().map(|r| r.1), Some(RefreshMode::Full));
    render(&mut kernel);
    assert_eq!(kernel.frame().pixel(0, 0), Color::BLACK);
}

#[test]
fn taps_resolve_through_zones() {
    let log = log();
    let mut kernel = Kernel::builder(DEVICE)
        .test_plugin(TestPlugin::new("a", &log))
        .build()
        .unwrap();
    kernel.start(Instant(0));
    tap(&mut kernel, 100, 85, 45);
    tap(&mut kernel, 300, 5, 45);
    assert_eq!(*log.borrow(), ["a:core.next-page", "a:core.prev-page"]);
}

fn swipe(kernel: &mut Kernel, at: u64, from: (i32, i32), to: (i32, i32)) {
    let event = |phase, (x, y)| {
        InputEvent::Pointer(PointerEvent::new(
            0,
            phase,
            Point::new(x, y),
            PointerTool::Finger,
            1000,
        ))
    };
    kernel.handle(Instant(at), event(PointerPhase::Down, from));
    kernel.handle(Instant(at + 30), event(PointerPhase::Move, to));
    kernel.handle(Instant(at + 60), event(PointerPhase::Up, to));
    render(kernel);
}

#[test]
fn swipes_in_from_the_edges_come_first() {
    let log = log();
    let mut kernel = Kernel::builder(DEVICE)
        .test_plugin(TestPlugin::new("a", &log))
        .build()
        .unwrap();
    kernel.start(Instant(0));
    // Up from the bottom edge: home; in from the left edge: back.
    swipe(&mut kernel, 100, (45, 88), (45, 30));
    swipe(&mut kernel, 300, (2, 45), (70, 45));
    // The same swipes in the middle of the screen turn pages.
    swipe(&mut kernel, 500, (45, 50), (45, 5));
    // At an edge, a tap is a tap.
    tap(&mut kernel, 700, 88, 45);
    assert_eq!(
        *log.borrow(),
        [
            "a:core.home",
            "a:core.back",
            "a:core.next-page",
            "a:core.next-page"
        ]
    );
}

/// A screen that holds the edges for itself, taking every finger.
struct EdgeHolder {
    log: Log,
    /// Holds the edges as a whole; else only takes the fingers it gets.
    holds: bool,
}

impl crate::testing::Fake for EdgeHolder {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Place<'_>) {}

    fn on_action(&mut self, action: &Id, _: &mut Turn<'_, '_>) -> Handled {
        self.log
            .borrow_mut()
            .push(alloc::format!("holder:{action}"));
        Handled::Yes
    }

    fn flags(&self) -> u32 {
        u32::from(self.holds) * crate::testing::HOLDS_EDGES
    }

    fn on_pointer(&mut self, event: &PointerEvent, _: &mut Turn<'_, '_>) -> Handled {
        match event.phase {
            PointerPhase::Down => self.log.borrow_mut().push(String::from("holder:down")),
            PointerPhase::Hover => self.log.borrow_mut().push(String::from("holder:hover")),
            _ => {}
        }
        Handled::Yes
    }
}

pub(crate) fn edge_holder_plugin(
    kept0: Log,
    kept1: bool,
) -> impl Fn(&mut Staging) -> Result<(), RegisterError> {
    move |r| {
        let (log, holds) = (kept0.clone(), kept1);
        r.add_fake(crate::testing::FakeSpec::new(
            Id::new_static("holder.screen"),
            "holder",
            Box::new(move |_, _| {
                Box::new(EdgeHolder {
                    log: log.clone(),
                    holds,
                })
            }),
        ))?;
        r.bind_slot(
            slots::HOME,
            Id::new_static("holder.screen"),
            Priority::DEFAULT,
        )
    }
}

/// What [`edge_holder_plugin`] registers under.
pub(crate) fn edge_holder_plugin_manifest() -> Manifest {
    Manifest::new(Id::new_static("holder"), "Holder", Version::new(1, 0, 0))
        .uses(paperpaw_sdk::Uses::NOTHING)
}

#[test]
fn the_screen_is_refreshed_whole_on_request() {
    let log = log();
    let mut kernel = Kernel::builder(DEVICE)
        .test_plugin(TestPlugin::new("a", &log))
        .user_config(
            "[keymap.global]
button.menu = core.refresh-screen
",
        )
        .build()
        .unwrap();
    kernel.start(Instant(0));
    render(&mut kernel);
    let effects = press(&mut kernel, 100, "menu");
    assert_eq!(
        effects.refresh,
        Some((Rect::new(0, 0, 90, 90), RefreshMode::Full))
    );
}

#[test]
fn screens_may_hold_the_edges() {
    for holds in [true, false] {
        let log = log();
        let mut kernel = Kernel::builder(DEVICE)
            .items(
                edge_holder_plugin_manifest(),
                edge_holder_plugin(log.clone(), holds),
            )
            .build()
            .unwrap();
        kernel.start(Instant(0));
        // The finger at the edge is the screen's: no going home. Also
        // when the screen only takes the fingers it wants (a grip that
        // moves a row) and leaves the edges otherwise.
        swipe(&mut kernel, 100, (45, 88), (45, 30));
        assert_eq!(*log.borrow(), ["holder:down"]);
    }
}

#[test]
fn tools_with_their_own_gestures_need_a_second_swipe() {
    let log = log();
    let mut plugin = TestPlugin::new("a", &log);
    plugin.keymap = Some(
        "[keymap.a.screen]
gestures.policy = core.gestures.own
",
    );
    let mut kernel = Kernel::builder(DEVICE).test_plugin(plugin).build().unwrap();
    kernel.start(Instant(0));
    // Swipes and taps of the key map do nothing: the tool takes them.
    swipe(&mut kernel, 100, (45, 50), (45, 5));
    tap(&mut kernel, 300, 85, 45);
    assert!(log.borrow().is_empty());
    // From the edge, the second swipe goes home; one alone does not.
    swipe(&mut kernel, 500, (45, 88), (45, 30));
    assert!(log.borrow().is_empty());
    swipe(&mut kernel, 1500, (45, 88), (45, 30));
    assert_eq!(*log.borrow(), ["a:core.home"]);
    log.borrow_mut().clear();
    // Too late for a second one.
    swipe(&mut kernel, 2000, (45, 88), (45, 30));
    swipe(&mut kernel, 9000, (45, 88), (45, 30));
    assert!(log.borrow().is_empty());

    // The user switches gestures off there: edges do nothing at all.
    let log = self::log();
    let mut plugin = TestPlugin::new("a", &log);
    plugin.keymap = Some(
        "[keymap.a.screen]
gestures.policy = core.gestures.own
",
    );
    let mut kernel = Kernel::builder(DEVICE)
        .test_plugin(plugin)
        .user_config(
            "[keymap.a.screen]
gestures.policy = core.gestures.off
",
        )
        .build()
        .unwrap();
    kernel.start(Instant(0));
    swipe(&mut kernel, 100, (45, 88), (45, 30));
    swipe(&mut kernel, 200, (45, 88), (45, 30));
    assert!(log.borrow().is_empty());
}

#[test]
fn reports_unknown_references_in_config() {
    let mut kernel = Kernel::builder(DEVICE)
        .user_config("[slots]\ncore.home = nope.screen\n[keymap.global]\nbutton.x = nope.action\n")
        .build()
        .unwrap();
    let warnings = kernel.take_warnings();
    assert_eq!(warnings.len(), 2, "{warnings:?}");
    kernel.start(Instant(0));
    assert_eq!(kernel.active_screen(), Some(&screens::PLACEHOLDER));
}

/// A plugin contributing two tiny bitmap fonts.
pub(crate) fn font_plugin() -> impl Fn(&mut Staging) -> Result<(), RegisterError> {
    |r| {
        for name in ["type.serif", "type.sans"] {
            let mut pack = paperpaw_text::PackBuilder::new();
            let metrics = paperpaw_text::LineMetrics::new(8.0, 2.0, 12.0);
            pack.add_strike(
                10.0,
                metrics,
                alloc::vec![('a', paperpaw_text::Glyph::empty(5.0))],
            );
            let font = paperpaw_sdk::Font::from_data(pack.build(), &[]).unwrap();
            r.add_font(paperpaw_sdk::FontSpec::new(Id::parse(name).unwrap(), font))?;
        }
        r.bind_font(
            paperpaw_sdk::builtin::fonts::UI,
            Id::new_static("type.sans"),
            Priority::DEFAULT,
        )?;
        Ok(())
    }
}

/// What [`font_plugin`] registers under.
pub(crate) fn font_plugin_manifest() -> Manifest {
    Manifest::new(Id::new_static("type"), "Fonts", Version::new(1, 0, 0))
        .uses(paperpaw_sdk::Uses::NOTHING)
}

#[test]
fn font_roles_follow_proposals_and_user_config() {
    let kernel = Kernel::builder(DEVICE)
        .items(font_plugin_manifest(), font_plugin())
        .build()
        .unwrap();
    let fonts = kernel.fonts();
    assert_eq!(
        fonts.role("core.font-ui").unwrap().id().as_str(),
        "type.sans"
    );
    // No proposal for reading: the first registered font is used.
    assert_eq!(
        fonts.role("core.font-reading").unwrap().id().as_str(),
        "type.serif"
    );

    let mut kernel = Kernel::builder(DEVICE)
        .items(font_plugin_manifest(), font_plugin())
        .user_config(
            "[fonts]
core.font-ui = type.serif
core.font-reading = nope.font
",
        )
        .build()
        .unwrap();
    assert_eq!(
        kernel.fonts().role("core.font-ui").unwrap().id().as_str(),
        "type.serif"
    );
    let warnings = kernel.take_warnings();
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].message.contains("unknown font `nope.font`"));
}

/// Subscribes to `core.started` and opens its screen with parameters.
pub(crate) struct LifecyclePlugin {
    log: Log,
}

struct LifecycleScreen {
    log: Log,
}

impl crate::testing::Fake for LifecycleScreen {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Place<'_>) {}

    fn on_action(&mut self, _: &Id, _: &mut Turn<'_, '_>) -> Handled {
        Handled::No
    }

    fn on_show(&mut self, _: &mut Turn<'_, '_>) {
        self.log.borrow_mut().push(String::from("show"));
    }
}

pub(crate) fn lifecycle_plugin(
    kept: LifecyclePlugin,
) -> impl Fn(&mut Staging) -> Result<(), RegisterError> {
    move |r| {
        let log = kept.log.clone();
        r.add_fake(crate::testing::FakeSpec::new(
            Id::new_static("life.screen"),
            "life",
            Box::new(move |_, params| {
                log.borrow_mut()
                    .push(alloc::format!("open {}", params.get("path").unwrap_or("-")));
                Box::new(LifecycleScreen { log: log.clone() })
            }),
        ))?;
        r.subscribe(paperpaw_sdk::builtin::events::STARTED, |k, _| {
            k.with_env(|_, out| {
                out.push(crate::Ask::OpenScreen(
                    Id::new_static("life.screen"),
                    paperpaw_sdk::Params::new().with("path", "books/a.txt"),
                ));
            });
        })?;
        // Back to the screen below, and at once another over it, as a
        // menu that closes to open a screen does.
        r.add_action(crate::kit::action::ActionSpec::new(SWAP, "swap").handler(
            |_: &crate::Place<'_>, _, out: &mut crate::Asks| {
                out.push(crate::Ask::Back);
                out.push(crate::Ask::OpenScreen(
                    Id::new_static("life.screen"),
                    paperpaw_sdk::Params::new().with("path", "books/c.txt"),
                ));
            },
        ))?;
        Ok(())
    }
}

/// What [`lifecycle_plugin`] registers under.
pub(crate) fn lifecycle_plugin_manifest() -> Manifest {
    Manifest::new(Id::new_static("life"), "Lifecycle", Version::new(1, 0, 0))
        .uses(paperpaw_sdk::Uses::NOTHING)
}

const SWAP: Id = Id::new_static("life.swap");

/// A screen below a full screen is shown again ([`Screen::on_show`]) when
/// that one closes; a screen on top only for a moment, while the commands
/// of a call run, is not shown.
#[test]
fn screens_below_a_full_screen_are_shown_once_on_top() {
    let log = log();
    let mut kernel = Kernel::builder(DEVICE)
        .items(
            lifecycle_plugin_manifest(),
            lifecycle_plugin(LifecyclePlugin { log: log.clone() }),
        )
        .build()
        .unwrap();
    kernel.start(Instant(0));
    let path = paperpaw_sdk::Params::new().with("path", "books/b.txt");
    kernel.open_screen(Instant(1), Id::new_static("life.screen"), path);
    assert_eq!(
        *log.borrow(),
        ["open books/a.txt", "show", "open books/b.txt", "show"]
    );
    log.borrow_mut().clear();
    kernel.run_action(Instant(2), SWAP);
    assert_eq!(*log.borrow(), ["open books/c.txt", "show"]);
    log.borrow_mut().clear();
    kernel.run_action(Instant(3), actions::BACK);
    assert_eq!(*log.borrow(), ["show"]);
}

#[test]
fn events_open_screens_with_params() {
    let log = log();
    let mut kernel = Kernel::builder(DEVICE)
        .items(
            lifecycle_plugin_manifest(),
            lifecycle_plugin(LifecyclePlugin { log: log.clone() }),
        )
        .build()
        .unwrap();
    kernel.start(Instant(100));
    assert_eq!(kernel.active_screen().map(Id::as_str), Some("life.screen"));
    assert_eq!(*log.borrow(), ["open books/a.txt", "show"]);
}

/// A full screen that logs events, plus an overlay covering the lower half.
pub(crate) struct OverlayPlugin {
    log: Log,
}

struct BaseScreen {
    log: Log,
}

impl crate::testing::Fake for BaseScreen {
    fn draw(&mut self, canvas: &mut Canvas<'_>, _: &Place<'_>) {
        let clip = canvas.clip();
        canvas.describe(paperpaw_sdk::semantics::Role::Text, "base", clip);
        self.log
            .borrow_mut()
            .push(alloc::format!("draw base {}x{}", clip.width, clip.height));
    }

    fn on_action(&mut self, _: &Id, _: &mut Turn<'_, '_>) -> Handled {
        Handled::No
    }

    fn on_event(&mut self, event: &Id, params: &paperpaw_sdk::Params, cx: &mut Turn<'_, '_>) {
        self.log.borrow_mut().push(alloc::format!(
            "event {event} {}",
            params.get("id").unwrap_or_default()
        ));
        if event.as_str() == "ov.changed" {
            cx.request_redraw();
        }
    }
}

struct HalfOverlay;

impl crate::testing::Fake for HalfOverlay {
    fn region(&self, _: &Place<'_>) -> Option<Rect> {
        Some(Rect::new(0, 45, 90, 45))
    }

    fn draw(&mut self, canvas: &mut Canvas<'_>, _: &Place<'_>) {
        canvas.fill_rect(0, 45, 90, 45, Color::BLACK);
        let area = Rect::new(0, 45, 90, 45);
        canvas.describe(paperpaw_sdk::semantics::Role::Text, "overlay", area);
    }

    fn on_action(&mut self, _: &Id, _: &mut Turn<'_, '_>) -> Handled {
        Handled::No
    }
}

pub(crate) fn overlay_plugin(
    kept: OverlayPlugin,
) -> impl Fn(&mut Staging) -> Result<(), RegisterError> {
    move |r| {
        let log = kept.log.clone();
        r.add_fake(crate::testing::FakeSpec::new(
            Id::new_static("ov.base"),
            "base",
            Box::new(move |_, _| Box::new(BaseScreen { log: log.clone() })),
        ))?;
        r.bind_slot(slots::HOME, Id::new_static("ov.base"), Priority::DEFAULT)?;
        r.add_fake(crate::testing::FakeSpec::new(
            Id::new_static("ov.half"),
            "half",
            Box::new(|_, _| Box::new(HalfOverlay)),
        ))?;
        r.bind_slot(slots::MENU, Id::new_static("ov.half"), Priority::DEFAULT)?;
        for (action, order) in [(actions::TOGGLE_NIGHT_MODE, 20), (actions::HOME, 10)] {
            r.add_menu_entry(
                crate::kit::menus::MenuEntry::new(paperpaw_sdk::builtin::menus::MAIN, action)
                    .order(order),
            )?;
        }
        Ok(())
    }
}

/// What [`overlay_plugin`] registers under.
pub(crate) fn overlay_plugin_manifest() -> Manifest {
    Manifest::new(Id::new_static("ov"), "Overlay", Version::new(1, 0, 0))
        .uses(paperpaw_sdk::Uses::NETWORK)
}

#[test]
fn overlays_redraw_only_their_region_and_close_on_outside_taps() {
    let log = log();
    let mut kernel = Kernel::builder(DEVICE)
        .items(
            overlay_plugin_manifest(),
            overlay_plugin(OverlayPlugin { log: log.clone() }),
        )
        .build()
        .unwrap();
    kernel.start(Instant(0));
    render(&mut kernel);
    log.borrow_mut().clear();

    // The menu action opens the overlay bound to core.menu.
    let effects = press(&mut kernel, 100, "menu");
    assert_eq!(kernel.active_screen().map(Id::as_str), Some("ov.half"));
    assert_eq!(
        effects.refresh(),
        Some((Rect::new(0, 45, 90, 45), RefreshMode::Partial))
    );
    // The overlay covers all that is drawn: the base below is not drawn.
    assert!(log.borrow().is_empty(), "{:?}", log.borrow());
    assert_eq!(kernel.frame().pixel(0, 60), Color::BLACK);
    assert_eq!(kernel.frame().pixel(0, 10), Color::WHITE);

    // A tap above the overlay closes it and restores the base screen.
    log.borrow_mut().clear();
    let effects = tap(&mut kernel, 300, 45, 10);
    assert_eq!(kernel.active_screen().map(Id::as_str), Some("ov.base"));
    assert_eq!(
        effects.refresh(),
        Some((Rect::new(0, 45, 90, 45), RefreshMode::Partial))
    );
    assert_eq!(*log.borrow(), ["draw base 90x45"]);
    assert_eq!(kernel.frame().pixel(0, 60), Color::WHITE);
}

#[test]
fn the_active_screen_is_described_alone() {
    let log = log();
    let mut kernel = Kernel::builder(DEVICE)
        .items(
            overlay_plugin_manifest(),
            overlay_plugin(OverlayPlugin { log: log.clone() }),
        )
        .build()
        .unwrap();
    kernel.start(Instant(0));
    render(&mut kernel);
    let labels = |described: Vec<paperpaw_sdk::semantics::Described>| -> Vec<String> {
        described.into_iter().map(|d| d.label).collect()
    };
    assert_eq!(labels(kernel.described_active()), ["base"]);
    press(&mut kernel, 100, "menu");
    assert_eq!(kernel.active_screen().map(Id::as_str), Some("ov.half"));
    // Everything shown, the base below the overlay too; the overlay
    // alone.
    assert_eq!(labels(kernel.described()), ["base", "overlay"]);
    let half = kernel.described_active();
    assert_eq!(half[0].clip, Rect::new(0, 45, 90, 45));
    assert_eq!(labels(half), ["overlay"]);
}

/// The base screen and the half overlay as the notice.
pub(crate) struct NoticePlugin {
    pub(crate) log: Log,
}

pub(crate) fn notice_plugin(
    kept: NoticePlugin,
) -> impl Fn(&mut Staging) -> Result<(), RegisterError> {
    move |r| {
        let log = kept.log.clone();
        r.add_fake(crate::testing::FakeSpec::new(
            Id::new_static("nt.base"),
            "base",
            Box::new(move |_, _| Box::new(BaseScreen { log: log.clone() })),
        ))?;
        r.bind_slot(slots::HOME, Id::new_static("nt.base"), Priority::DEFAULT)?;
        r.add_fake(crate::testing::FakeSpec::new(
            Id::new_static("nt.notice"),
            "notice",
            Box::new(|_, _| Box::new(HalfOverlay)),
        ))?;
        r.bind_slot(
            slots::NOTICE,
            Id::new_static("nt.notice"),
            Priority::DEFAULT,
        )?;
        Ok(())
    }
}

/// What [`notice_plugin`] registers under.
pub(crate) fn notice_plugin_manifest() -> Manifest {
    Manifest::new(Id::new_static("nt"), "Notice", Version::new(1, 0, 0))
        .uses(paperpaw_sdk::Uses::NOTHING)
}

/// The host's notice (no plugin's screen fills the notice slot here).
#[test]
fn notices_show_over_everything_and_go_by_themselves() {
    let log = log();
    let mut kernel = Kernel::builder(DEVICE)
        .items(
            crate::tests::font_plugin_manifest(),
            crate::tests::font_plugin(),
        )
        .items(
            overlay_plugin_manifest(),
            overlay_plugin(OverlayPlugin { log: log.clone() }),
        )
        .build()
        .unwrap();
    kernel.start(Instant(0));
    render(&mut kernel);

    kernel.apply(Ask::Notify(String::from("Done")));
    let effects = kernel.finish();
    let region = {
        let (notice, _) = kernel.notice.as_ref().unwrap();
        crate::screens::notice_area(notice, &kernel.shared.place(Instant(0)))
    };
    assert_eq!(effects.refresh().map(|(r, _)| r), Some(region));
    assert_eq!(
        effects.wake_at(),
        Some(Instant(4000)),
        "four seconds by default"
    );
    render(&mut kernel);
    // Inside the box, left of its words.
    let (x, y) = (
        i32::from(region.x) + 1,
        i32::from(region.y + region.height / 2),
    );
    assert_eq!(kernel.frame().pixel(x as u16, y as u16), Color::BLACK);
    // Input still goes to the screen below.
    assert_eq!(kernel.active_screen().map(Id::as_str), Some("ov.base"));
    // It is part of what the screen shows, read aloud too.
    let shown = |kernel: &mut Kernel| kernel.describe().iter().any(|p| p.label == "Done");
    assert!(shown(&mut kernel));

    // Time is up: gone.
    let effects = kernel.tick(Instant(4000));
    assert_eq!(effects.refresh().map(|(r, _)| r), Some(region));
    render(&mut kernel);
    assert_eq!(kernel.frame().pixel(x as u16, y as u16), Color::WHITE);
    assert_eq!(effects.wake_at(), None);
    assert!(!shown(&mut kernel));
}

#[test]
fn scheduled_events_come_at_their_time() {
    let log = log();
    let mut kernel = Kernel::builder(DEVICE)
        .items(
            overlay_plugin_manifest(),
            overlay_plugin(OverlayPlugin { log: log.clone() }),
        )
        .build()
        .unwrap();
    kernel.start(Instant(0));
    let event = Id::new_static("ov.later");
    let params = paperpaw_sdk::Params::new().with("id", "x");
    kernel.apply(Ask::work(
        paperpaw_sdk::work::Work::at(event.clone(), Instant(9000)).with(params.clone()),
    ));
    // Scheduled again: moved, not doubled.
    kernel.apply(Ask::work(
        paperpaw_sdk::work::Work::at(event, Instant(5000)).with(params),
    ));
    assert_eq!(kernel.finish().wake_at(), Some(Instant(5000)));
    log.borrow_mut().clear();
    kernel.tick(Instant(4999));
    assert!(log.borrow().is_empty());
    let effects = kernel.tick(Instant(5000));
    assert_eq!(*log.borrow(), ["event ov.later x"]);
    assert_eq!(effects.wake_at(), None);
}

#[test]
fn bindings_change_at_once_and_are_saved() {
    let log = log();
    let mut kernel = Kernel::builder(DEVICE)
        .items(
            overlay_plugin_manifest(),
            overlay_plugin(OverlayPlugin { log: log.clone() }),
        )
        .storage(paperpaw_sdk::MemoryStorage::new())
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel.apply(Ask::bind(
        String::from("global"),
        String::from("button.menu"),
        Some(actions::TOGGLE_NIGHT_MODE),
    ));
    kernel.finish();
    press(&mut kernel, 100, "menu");
    assert_eq!(
        kernel.active_screen().map(Id::as_str),
        Some("ov.base"),
        "no menu"
    );
    assert!(kernel.settings().bool("core.night-mode"));
    let saved = paperpaw_sdk::read_file(
        kernel.shared.storage.as_ref(),
        paperpaw_sdk::builtin::CONFIG_FILE,
        4096,
    )
    .unwrap();
    assert!(String::from_utf8_lossy(&saved).contains("menu = core.toggle-night-mode"));

    // Back to what the plugins bind: the menu again.
    kernel.apply(Ask::bind(
        String::from("global"),
        String::from("button.menu"),
        None,
    ));
    kernel.finish();
    press(&mut kernel, 300, "menu");
    assert_eq!(kernel.active_screen().map(Id::as_str), Some("ov.half"));
}

#[test]
fn screens_below_overlays_redraw_themselves() {
    let log = log();
    let mut kernel = Kernel::builder(DEVICE)
        .items(
            overlay_plugin_manifest(),
            overlay_plugin(OverlayPlugin { log: log.clone() }),
        )
        .build()
        .unwrap();
    kernel.start(Instant(0));
    render(&mut kernel);
    press(&mut kernel, 100, "menu");
    render(&mut kernel);
    log.borrow_mut().clear();

    // The base screen changes while the overlay is open: drawn again
    // where the overlay leaves it, the overlay, unchanged, not.
    kernel.apply(Ask::Emit(
        Id::new_static("ov.changed"),
        paperpaw_sdk::Params::new(),
    ));
    let effects = kernel.finish();
    assert_eq!(
        effects.refresh().map(|(r, _)| r),
        Some(Rect::new(0, 0, 90, 45))
    );
    render(&mut kernel);
    assert_eq!(*log.borrow(), ["event ov.changed ", "draw base 90x45"]);
    assert_eq!(kernel.frame().pixel(0, 60), Color::BLACK);
}

#[test]
fn effects_shown_together_join() {
    let first = crate::Effects {
        refresh: Some((Rect::new(0, 0, 10, 10), RefreshMode::Fast)),
        wake_at: Some(Instant(500)),
        grays: true,
        ..crate::Effects::default()
    };
    let later = crate::Effects {
        refresh: Some((Rect::new(20, 20, 10, 10), RefreshMode::Partial)),
        wake_at: Some(Instant(200)),
        ..crate::Effects::default()
    };
    let both = first.and(later);
    assert_eq!(
        both.refresh(),
        Some((Rect::new(0, 0, 30, 30), RefreshMode::Partial))
    );
    assert_eq!(both.wake_at(), Some(Instant(200)));
    // As the later frame says.
    assert!(!both.grays());
    assert_eq!(
        crate::Effects::default().and(first).refresh(),
        first.refresh()
    );
}

#[test]
fn open_slots_reopen_for_other_params() {
    let log = log();
    let mut kernel = Kernel::builder(DEVICE)
        .items(
            overlay_plugin_manifest(),
            overlay_plugin(OverlayPlugin { log: log.clone() }),
        )
        .build()
        .unwrap();
    kernel.start(Instant(0));
    let open = |kernel: &mut Kernel, menu: &str| {
        kernel.apply(Ask::OpenSlot(
            slots::MENU,
            paperpaw_sdk::Params::new().with("menu", menu.to_string()),
        ));
        kernel.finish();
    };
    let shown = |kernel: &Kernel| -> Vec<String> {
        kernel
            .stack
            .iter()
            .map(|s| alloc::format!("{} {}", s.id, s.params.get("menu").unwrap_or("-")))
            .collect()
    };
    open(&mut kernel, "a");
    assert_eq!(shown(&kernel), ["ov.base -", "ov.half a"]);
    // The same slot with the same params is just brought to the front.
    open(&mut kernel, "a");
    assert_eq!(shown(&kernel), ["ov.base -", "ov.half a"]);
    // Other params (e.g. another book) open it anew in its place.
    open(&mut kernel, "b");
    assert_eq!(shown(&kernel), ["ov.base -", "ov.half b"]);
}

use services_and_pictures::{
    ServicePlugin, get, picture_plugin, picture_plugin_manifest, service_plugin, wifi_kernel,
};

mod action_runs;
mod bluetooth_pairing;
mod busy_mark;
mod card_hints;
mod clean_up;
mod declared_work;
mod developer;
mod developer_link;
mod font_packs;
mod frames;
mod free_count;
mod guard;
mod host_notice;
mod hosting;
mod hyphenation_feed;
mod hyphenation_tables;
mod input;
mod input_kinds;
mod kept_groups;
mod kernel_calls;
mod key_guard;
mod keys_bands_power;
mod language_seed;
mod library_catalog;
mod library_catalog_native;
mod low_memory_notice;
mod menus_and_grays;
mod net_demand;
mod net_policy_wiring;
mod network_work;
mod off_screen;
mod open_with;
mod pictures_in_grays;
mod plugins_loaded;
pub(crate) mod questions;
mod reading_apps;
mod realtime;
mod refreshes;
mod screen_life;
mod screen_look;
mod services_and_pictures;
mod services_declared;
mod settings_files;
mod shell_io;
mod show_again;
mod stack;
mod start_screens;
mod stay_awake;
mod storage;
mod switching_and_data;
mod ui_ink;
mod work_sockets;

/// A greeting, provided as a service in a test.
struct Greeting(&'static str);
paperpaw_sdk::service_type!(Greeting, "provider.greeting/1");

/// Adds the fixtures that keep their fields (`TestPlugin`, `ServicePlugin`) to a builder.
pub(crate) trait AddFixture {
    fn test_plugin(self, plugin: TestPlugin) -> Self;
    fn test_plugin_off(self, plugin: TestPlugin) -> Self;
    fn service_plugin(self, plugin: ServicePlugin) -> Self;
}

impl AddFixture for crate::KernelBuilder {
    fn test_plugin(self, plugin: TestPlugin) -> Self {
        self.items(plugin.manifest(), test_plugin(plugin))
    }
    fn test_plugin_off(self, plugin: TestPlugin) -> Self {
        self.items_off(plugin.manifest(), test_plugin(plugin))
    }
    fn service_plugin(self, plugin: ServicePlugin) -> Self {
        self.items(plugin.manifest(), service_plugin(plugin))
    }
}
