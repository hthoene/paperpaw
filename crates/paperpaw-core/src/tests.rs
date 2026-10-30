//! Kernel behaviour tests: slots, priorities, key map layers, commands,
//! settings and transactional plugin registration.

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::rc::Rc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cell::RefCell;

use paperpaw_platform::{
    ButtonState, Color, DeviceInfo, DisplayInfo, InputCaps, InputEvent, Instant, PixelFormat,
    Point, PointerEvent, PointerPhase, PointerTool, Rect, RefreshMode, Rotation,
};
use paperpaw_sdk::builtin::{actions, screens, settings, slots};
use paperpaw_sdk::{
    ActionSpec, Canvas, Command, Cx, Env, Handled, Id, Manifest, Plugin, Priority, RegisterError,
    Registrar, Screen, ScreenSpec, SettingKind, SettingSpec, SettingValue, Version,
};

use crate::{HttpCall, Io, Kernel, ServerCall, SpeechCall, WarningSource};

/// What the shell is asked, one kind at a time, as the parts keep it: the
/// tests of each part look at its own.
impl Kernel {
    /// A plugin's request, as [`Command::Http`] makes it.
    fn request(&mut self, request: paperpaw_sdk::HttpRequest) {
        self.output.commands.push(Command::http(request));
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

type Log = Rc<RefCell<Vec<String>>>;

/// A configurable plugin providing one screen bound to the home slot.
struct TestPlugin {
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

impl Plugin for TestPlugin {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::parse(self.id).unwrap(), "Test", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let log = self.log.clone();
        let name = self.id;
        r.add_screen(ScreenSpec::new(
            self.screen_id(),
            "test.screen",
            Box::new(move |_, _| {
                Box::new(TestScreen {
                    name,
                    log: log.clone(),
                })
            }),
        ))?;
        r.bind_slot(slots::HOME, self.screen_id(), self.priority)?;
        r.add_action(ActionSpec::new(
            Id::parse(&alloc::format!("{}.special", self.id)).unwrap(),
            "test.special",
        ))?;
        r.add_setting(SettingSpec::new(
            Id::parse(&alloc::format!("{}.level", self.id)).unwrap(),
            "test.level",
            SettingKind::Int { min: 1, max: 5 },
            SettingValue::Int(2),
            paperpaw_sdk::builtin::groups::OTHER,
        ))?;
        if let Some(keymap) = self.keymap {
            r.add_keymap(Cow::Borrowed(keymap))?;
        }
        if self.misbehave {
            r.add_action(ActionSpec::new(Id::new_static("core.stolen"), ""))?;
        }
        Ok(())
    }
}

struct TestScreen {
    name: &'static str,
    log: Log,
}

impl Screen for TestScreen {
    fn draw(&mut self, canvas: &mut Canvas<'_>, _: &Env<'_>) {
        canvas.fill_rect(0, 0, 10, 10, Color::BLACK);
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
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

fn log() -> Log {
    Rc::new(RefCell::new(Vec::new()))
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
        .plugin(high)
        .plugin(low)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    assert_eq!(kernel.active_screen().map(Id::as_str), Some("high.screen"));

    let mut kernel = Kernel::builder(DEVICE)
        .plugin(TestPlugin::new("high", &log))
        .plugin(TestPlugin::new("low", &log))
        .user_config("[slots]\ncore.home = low.screen\n")
        .build()
        .unwrap();
    kernel.start(Instant(0));
    assert_eq!(kernel.active_screen().map(Id::as_str), Some("low.screen"));
}

/// Opens the placeholder screen on *confirm*: a screen change to time.
struct Opener;

struct OpenerScreen;

impl Screen for OpenerScreen {
    fn draw(&mut self, canvas: &mut Canvas<'_>, _: &Env<'_>) {
        canvas.fill_rect(0, 0, 10, 10, Color::BLACK);
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        if *action == actions::CONFIRM {
            cx.command(Command::OpenScreen(
                screens::PLACEHOLDER,
                paperpaw_sdk::Params::new(),
            ));
            return Handled::Yes;
        }
        Handled::No
    }
}

impl Plugin for Opener {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("opener"), "Opener", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_screen(ScreenSpec::new(
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

/// A clock that stands still: all work takes no time.
fn still_clock() -> u64 {
    1_000_000
}

#[test]
fn slow_work_shows_the_busy_mark_first() {
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(Opener)
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
    // Back, also not timed yet; then opening again, known to be quick:
    // no mark.
    assert!(press(&mut kernel, 300, "back").proceed());
    kernel.proceed(Instant(400));
    render(&mut kernel);
    let effects = press(&mut kernel, 500, "confirm");
    assert!(!effects.proceed());
    assert_eq!(kernel.active_screen(), Some(&screens::PLACEHOLDER));
}

#[test]
fn presses_while_a_screen_comes_up_are_dropped() {
    let log = log();
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(TestPlugin::new("a", &log))
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
        .plugin(TestPlugin::new("a", &log))
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

    let mut kernel = Kernel::builder(DEVICE).plugin(plugin).build().unwrap();
    kernel.start(Instant(0));
    press(&mut kernel, 100, "confirm");
    assert_eq!(*log.borrow(), ["a:a.special"]);

    log.borrow_mut().clear();
    let mut plugin = TestPlugin::new("a", &log);
    plugin.keymap = Some("[keymap.a.screen]\nbutton.confirm = a.special\n");
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(plugin)
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

    let mut kernel = Kernel::builder(DEVICE).plugin(plugin).build().unwrap();
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
        .plugin(TestPlugin::new("off", &log))
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
            .plugin_off(TestPlugin::new("web", &log))
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
        .plugin(TestPlugin::new("a", &log))
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
        .plugin(TestPlugin::new("a", &log))
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
        .plugin(TestPlugin::new("a", &log))
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

impl Screen for EdgeHolder {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}

    fn on_action(&mut self, action: &Id, _: &mut Cx<'_, '_>) -> Handled {
        self.log
            .borrow_mut()
            .push(alloc::format!("holder:{action}"));
        Handled::Yes
    }

    fn holds_edges(&self) -> bool {
        self.holds
    }

    fn on_pointer(&mut self, event: &PointerEvent, _: &mut Cx<'_, '_>) -> Handled {
        if event.phase == PointerPhase::Down {
            self.log.borrow_mut().push(String::from("holder:down"));
        }
        Handled::Yes
    }
}

struct EdgeHolderPlugin(Log, bool);

impl Plugin for EdgeHolderPlugin {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("holder"), "Holder", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let (log, holds) = (self.0.clone(), self.1);
        r.add_screen(ScreenSpec::new(
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

#[test]
fn the_screen_is_refreshed_whole_on_request() {
    let log = log();
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(TestPlugin::new("a", &log))
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
            .plugin(EdgeHolderPlugin(log.clone(), holds))
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
    let mut kernel = Kernel::builder(DEVICE).plugin(plugin).build().unwrap();
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
        .plugin(plugin)
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
struct FontPlugin;

impl Plugin for FontPlugin {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("type"), "Fonts", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
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

#[test]
fn font_roles_follow_proposals_and_user_config() {
    let kernel = Kernel::builder(DEVICE).plugin(FontPlugin).build().unwrap();
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
        .plugin(FontPlugin)
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

/// Subscribes to `core.started`, opens its screen with parameters and runs
/// a timer started in `on_show`.
struct LifecyclePlugin {
    log: Log,
}

struct LifecycleScreen {
    log: Log,
    ticks: u32,
}

impl Screen for LifecycleScreen {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        self.log.borrow_mut().push(String::from("show"));
        let at = cx.env().now().add_millis(10);
        cx.wake_at(at);
    }

    fn on_hide(&mut self) {
        self.log.borrow_mut().push(String::from("hide"));
    }

    fn on_timer(&mut self, cx: &mut Cx<'_, '_>) {
        self.ticks += 1;
        self.log.borrow_mut().push(alloc::format!(
            "timer {} at {}",
            self.ticks,
            cx.env().now().millis()
        ));
        if self.ticks < 2 {
            let at = cx.env().now().add_millis(10);
            cx.wake_at(at);
        }
    }
}

impl Plugin for LifecyclePlugin {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("life"), "Lifecycle", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let log = self.log.clone();
        r.add_screen(ScreenSpec::new(
            Id::new_static("life.screen"),
            "life",
            Box::new(move |_, params| {
                log.borrow_mut()
                    .push(alloc::format!("open {}", params.get("path").unwrap_or("-")));
                Box::new(LifecycleScreen {
                    log: log.clone(),
                    ticks: 0,
                })
            }),
        ))?;
        r.subscribe(
            paperpaw_sdk::builtin::events::STARTED,
            Box::new(|_, _, out| {
                out.push(paperpaw_sdk::Command::OpenScreen(
                    Id::new_static("life.screen"),
                    paperpaw_sdk::Params::new().with("path", "books/a.txt"),
                ));
            }),
        )?;
        // Back to the screen below, and at once another over it, as a
        // menu that closes to open a screen does.
        r.add_action(paperpaw_sdk::ActionSpec::new(SWAP, "swap").handler(
            |_: &Env<'_>, _, out: &mut paperpaw_sdk::Commands| {
                out.push(paperpaw_sdk::Command::Back);
                out.push(paperpaw_sdk::Command::OpenScreen(
                    Id::new_static("life.screen"),
                    paperpaw_sdk::Params::new().with("path", "books/c.txt"),
                ));
            },
        ))?;
        Ok(())
    }
}

const SWAP: Id = Id::new_static("life.swap");

/// A full screen opened over another hides it ([`Screen::on_hide`]),
/// shown again ([`Screen::on_show`]) when that one closes; a screen on
/// top only for a moment, while the commands of a call run, is not shown.
#[test]
fn screens_below_a_full_screen_are_hidden_and_shown_once_on_top() {
    let log = log();
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(LifecyclePlugin { log: log.clone() })
        .build()
        .unwrap();
    kernel.start(Instant(0));
    let path = paperpaw_sdk::Params::new().with("path", "books/b.txt");
    kernel.open_screen(Instant(1), Id::new_static("life.screen"), path);
    assert_eq!(
        *log.borrow(),
        [
            "open books/a.txt",
            "show",
            "open books/b.txt",
            "hide",
            "show"
        ]
    );
    log.borrow_mut().clear();
    kernel.run_action(Instant(2), SWAP);
    assert_eq!(*log.borrow(), ["open books/c.txt", "hide", "show"]);
    log.borrow_mut().clear();
    kernel.run_action(Instant(3), actions::BACK);
    assert_eq!(*log.borrow(), ["show"]);
}

#[test]
fn events_open_screens_with_params_and_timers_run() {
    let log = log();
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(LifecyclePlugin { log: log.clone() })
        .build()
        .unwrap();
    let effects = kernel.start(Instant(100));
    assert_eq!(kernel.active_screen().map(Id::as_str), Some("life.screen"));
    assert_eq!(effects.wake_at(), Some(Instant(110)));

    // Too early: nothing happens.
    kernel.tick(Instant(105));
    let effects = kernel.tick(Instant(110));
    assert_eq!(effects.wake_at(), Some(Instant(120)));
    let effects = kernel.tick(Instant(120));
    assert_eq!(effects.wake_at(), None);
    assert_eq!(
        *log.borrow(),
        [
            "open books/a.txt",
            "show",
            "timer 1 at 110",
            "timer 2 at 120"
        ]
    );
}

/// A full screen that logs events, plus an overlay covering the lower half.
struct OverlayPlugin {
    log: Log,
}

struct BaseScreen {
    log: Log,
}

impl Screen for BaseScreen {
    fn draw(&mut self, canvas: &mut Canvas<'_>, _: &Env<'_>) {
        let clip = canvas.clip();
        canvas.describe(paperpaw_sdk::semantics::Role::Text, "base", clip);
        self.log
            .borrow_mut()
            .push(alloc::format!("draw base {}x{}", clip.width, clip.height));
    }

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }

    fn on_event(&mut self, event: &Id, params: &paperpaw_sdk::Params, cx: &mut Cx<'_, '_>) {
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

impl Screen for HalfOverlay {
    fn region(&self, _: &Env<'_>) -> Option<Rect> {
        Some(Rect::new(0, 45, 90, 45))
    }

    fn draw(&mut self, canvas: &mut Canvas<'_>, _: &Env<'_>) {
        canvas.fill_rect(0, 45, 90, 45, Color::BLACK);
        let area = Rect::new(0, 45, 90, 45);
        canvas.describe(paperpaw_sdk::semantics::Role::Text, "overlay", area);
    }

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }
}

impl Plugin for OverlayPlugin {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("ov"), "Overlay", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NETWORK)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let log = self.log.clone();
        r.add_screen(ScreenSpec::new(
            Id::new_static("ov.base"),
            "base",
            Box::new(move |_, _| Box::new(BaseScreen { log: log.clone() })),
        ))?;
        r.bind_slot(slots::HOME, Id::new_static("ov.base"), Priority::DEFAULT)?;
        r.add_screen(ScreenSpec::new(
            Id::new_static("ov.half"),
            "half",
            Box::new(|_, _| Box::new(HalfOverlay)),
        ))?;
        r.bind_slot(slots::MENU, Id::new_static("ov.half"), Priority::DEFAULT)?;
        for (action, order) in [(actions::TOGGLE_NIGHT_MODE, 20), (actions::HOME, 10)] {
            r.add_menu_entry(
                paperpaw_sdk::MenuEntry::new(paperpaw_sdk::builtin::menus::MAIN, action)
                    .order(order),
            )?;
        }
        Ok(())
    }
}

#[test]
fn overlays_redraw_only_their_region_and_close_on_outside_taps() {
    let log = log();
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(OverlayPlugin { log: log.clone() })
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
        .plugin(OverlayPlugin { log: log.clone() })
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
struct NoticePlugin {
    log: Log,
}

impl Plugin for NoticePlugin {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("nt"), "Notice", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let log = self.log.clone();
        r.add_screen(ScreenSpec::new(
            Id::new_static("nt.base"),
            "base",
            Box::new(move |_, _| Box::new(BaseScreen { log: log.clone() })),
        ))?;
        r.bind_slot(slots::HOME, Id::new_static("nt.base"), Priority::DEFAULT)?;
        r.add_screen(ScreenSpec::new(
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

#[test]
fn notices_show_over_everything_and_go_by_themselves() {
    let log = log();
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(NoticePlugin { log: log.clone() })
        .build()
        .unwrap();
    kernel.start(Instant(0));
    render(&mut kernel);

    kernel.apply(Command::Notify(String::from("Done")));
    let effects = kernel.finish();
    assert_eq!(
        effects.refresh().map(|(r, _)| r),
        Some(Rect::new(0, 45, 90, 45))
    );
    assert_eq!(
        effects.wake_at(),
        Some(Instant(4000)),
        "four seconds by default"
    );
    render(&mut kernel);
    assert_eq!(kernel.frame().pixel(0, 60), Color::BLACK);
    // Input still goes to the screen below.
    assert_eq!(kernel.active_screen().map(Id::as_str), Some("nt.base"));
    // It is part of what the screen shows, read aloud too.
    let shown = |kernel: &mut Kernel| kernel.describe().iter().any(|p| p.label == "overlay");
    assert!(shown(&mut kernel));

    // Time is up: gone.
    let effects = kernel.tick(Instant(4000));
    assert_eq!(
        effects.refresh().map(|(r, _)| r),
        Some(Rect::new(0, 45, 90, 45))
    );
    render(&mut kernel);
    assert_eq!(kernel.frame().pixel(0, 60), Color::WHITE);
    assert_eq!(effects.wake_at(), None);
    assert!(!shown(&mut kernel));
}

#[test]
fn scheduled_events_come_at_their_time() {
    let log = log();
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(OverlayPlugin { log: log.clone() })
        .build()
        .unwrap();
    kernel.start(Instant(0));
    let event = Id::new_static("ov.later");
    let params = paperpaw_sdk::Params::new().with("id", "x");
    kernel.apply(Command::work(
        paperpaw_sdk::work::Work::at(event.clone(), Instant(9000)).with(params.clone()),
    ));
    // Scheduled again: moved, not doubled.
    kernel.apply(Command::work(
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
        .plugin(OverlayPlugin { log: log.clone() })
        .storage(paperpaw_sdk::MemoryStorage::new())
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel.apply(Command::bind(
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
    kernel.apply(Command::bind(
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
        .plugin(OverlayPlugin { log: log.clone() })
        .build()
        .unwrap();
    kernel.start(Instant(0));
    render(&mut kernel);
    press(&mut kernel, 100, "menu");
    render(&mut kernel);
    log.borrow_mut().clear();

    // The base screen changes while the overlay is open: drawn again
    // where the overlay leaves it, the overlay, unchanged, not.
    kernel.apply(Command::Emit(
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
        .plugin(OverlayPlugin { log: log.clone() })
        .build()
        .unwrap();
    kernel.start(Instant(0));
    let open = |kernel: &mut Kernel, menu: &str| {
        kernel.apply(Command::OpenSlot(
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

#[test]
fn menus_follow_order_and_user_definitions() {
    let log = log();
    let kernel = Kernel::builder(DEVICE)
        .plugin(OverlayPlugin { log: log.clone() })
        .build()
        .unwrap();
    let env_items = |kernel: &Kernel| -> Vec<String> {
        kernel
            .menus()
            .items("core.main")
            .iter()
            .map(|i| i.id.to_string())
            .collect()
    };
    let items = env_items(&kernel);
    // Entries of the core plugin and this plugin, sorted by order: a
    // short list, the quick panel first, the tools in a menu of their own.
    assert_eq!(
        items.iter().take(4).map(String::as_str).collect::<Vec<_>>(),
        ["core.quick-panel", "core.home", "core.home", "core.library"]
    );
    assert!(items.iter().any(|i| i == "core.tools"));
    assert!(!kernel.menus().items("core.main")[0].section);
    assert_eq!(kernel.shared.menus.icon("core.library"), Some("books"));
    assert_eq!(kernel.shared.menus.icon("core.tools"), Some("apps"));
    assert!(items.iter().filter(|a| *a == "core.home").count() == 2);

    let kernel = Kernel::builder(DEVICE)
        .plugin(OverlayPlugin { log })
        .user_config("[menus]\ncore.main = core.settings, core.toggle-night-mode\n")
        .build()
        .unwrap();
    assert_eq!(
        env_items(&kernel),
        ["core.settings", "core.toggle-night-mode"]
    );
}

/// A home screen best in grays (a picture), and the half overlay as its
/// menu.
struct GrayPlugin;

struct GrayScreen;

impl Screen for GrayScreen {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}

    fn grays(&self) -> bool {
        true
    }

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }
}

impl Plugin for GrayPlugin {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("gray"), "Gray", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_screen(ScreenSpec::new(
            Id::new_static("gray.page"),
            "page",
            Box::new(|_, _| Box::new(GrayScreen)),
        ))?;
        r.bind_slot(slots::HOME, Id::new_static("gray.page"), Priority::DEFAULT)?;
        r.add_screen(ScreenSpec::new(
            Id::new_static("gray.menu"),
            "menu",
            Box::new(|_, _| Box::new(HalfOverlay)),
        ))?;
        r.bind_slot(slots::MENU, Id::new_static("gray.menu"), Priority::DEFAULT)
    }
}

#[test]
fn screens_on_top_say_when_they_are_best_in_grays() {
    let mut kernel = Kernel::builder(DEVICE).plugin(GrayPlugin).build().unwrap();
    let effects = kernel.start(Instant(0));
    assert!(effects.refresh().is_some() && effects.grays());
    render(&mut kernel);
    // A menu over it: not while it shows; back again once it closed.
    let effects = press(&mut kernel, 100, "menu");
    assert!(effects.refresh().is_some() && !effects.grays());
    render(&mut kernel);
    let effects = press(&mut kernel, 300, "back");
    assert!(effects.refresh().is_some() && effects.grays());
}

#[test]
fn frames_drawn_again_in_grays_in_bands_of_as_many_bytes() {
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(OverlayPlugin { log: log() })
        .frame_rows(32)
        .row_align(8)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    render(&mut kernel);
    let format = kernel.frame().format;
    let bits = u32::from(format.bits_per_pixel());
    let mut bands = Vec::new();
    assert!(kernel.render_gray(&mut |top, rows| {
        bands.push((top, rows.height, rows.format));
    }));
    // The whole screen, in two bits a pixel, bands no larger in bytes.
    assert_eq!(bands.iter().map(|b| u32::from(b.1)).sum::<u32>(), 90);
    assert!(bands.iter().all(|b| b.2 == PixelFormat::Gray2));
    assert!(bands.iter().all(|b| u32::from(b.1) * 2 <= 32 * bits));
    // Its own bands again after.
    assert_eq!(kernel.frame().format, format);
    assert_eq!(kernel.frame().height, 32);
}

#[test]
fn drawn_again_whole_and_fully() {
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(OverlayPlugin { log: log() })
        .build()
        .unwrap();
    kernel.start(Instant(0));
    render(&mut kernel);
    let effects = kernel.draw_again(Instant(100));
    assert_eq!(
        effects.refresh(),
        Some((Rect::new(0, 0, 90, 90), RefreshMode::Full))
    );
}

#[test]
fn settings_reset_to_their_defaults() {
    let storage = paperpaw_sdk::MemoryStorage::new().with(
        "paperpaw.ini",
        "[core]
ui-size = 12
full-refresh-interval = 3
",
    );
    let mut kernel = Kernel::builder(DEVICE)
        .storage(storage)
        .plugin(OverlayPlugin { log: log() })
        .build()
        .unwrap();
    kernel.start(Instant(0));
    let ui_size = "core.ui-size";
    let default = kernel
        .shared
        .settings
        .specs()
        .find(|s| s.id.as_str() == ui_size)
        .unwrap()
        .default
        .clone();
    assert_ne!(kernel.shared.settings.get(ui_size), Some(&default));
    kernel.apply(Command::ResetSetting(
        paperpaw_sdk::builtin::settings::UI_SIZE.id().clone(),
    ));
    kernel.finish();
    assert_eq!(kernel.shared.settings.get(ui_size), Some(&default));
    // Gone from the file, the other setting kept.
    let mut buf = [0u8; 256];
    let n = kernel
        .shared
        .storage
        .read_at("paperpaw.ini", 0, &mut buf)
        .unwrap();
    let file = core::str::from_utf8(&buf[..n]).unwrap();
    assert!(!file.contains("ui-size"), "{file}");
    assert!(file.contains("full-refresh-interval = 3"), "{file}");
}

#[test]
fn menus_are_redefined_saved_and_reset() {
    let log = log();
    let storage = paperpaw_sdk::MemoryStorage::new().with(
        "paperpaw.ini",
        "[core]
ui-size = 12
",
    );
    let mut kernel = Kernel::builder(DEVICE)
        .storage(storage)
        .plugin(OverlayPlugin { log })
        .build()
        .unwrap();
    let items = |kernel: &Kernel| -> Vec<String> {
        kernel
            .menus()
            .items("core.main")
            .iter()
            .map(|i| i.id.to_string())
            .collect()
    };
    let file = |kernel: &Kernel| -> String {
        let mut buf = [0u8; 4096];
        let n = kernel
            .shared
            .storage
            .read_at("paperpaw.ini", 0, &mut buf)
            .unwrap();
        String::from(core::str::from_utf8(&buf[..n]).unwrap())
    };
    let defaults = items(&kernel);
    kernel.set_menu(
        Id::new_static("core.main"),
        Some(alloc::vec![
            Id::new_static("core.settings"),
            Id::new_static("core.home")
        ]),
    );
    assert_eq!(items(&kernel), ["core.settings", "core.home"]);
    assert!(kernel.shared.menus.is_customized("core.main"));
    // With what plugins put there when it was redefined: entries added
    // later show too.
    let saved = file(&kernel);
    let start = "[core]
ui-size = 12

[menus]
core.main = core.settings, core.home

[menus-known]
core.main = ";
    assert!(saved.starts_with(start), "{saved}");
    // Only what the user left out.
    let known = &saved[start.len()..];
    assert!(
        !known.contains("core.home") && !known.contains("core.settings"),
        "{saved}"
    );
    kernel.set_menu(Id::new_static("core.main"), None);
    assert_eq!(items(&kernel), defaults);
    assert_eq!(
        file(&kernel),
        "[core]
ui-size = 12

[menus]

[menus-known]
"
    );
}

#[test]
fn http_requests_go_to_the_shell_and_answers_come_back() {
    let request = |id: &str| {
        paperpaw_sdk::HttpRequest::get("https://example.org/x", Id::new_static("ov.reply"))
            .header("accept", "text/plain")
            .with(paperpaw_sdk::Params::new().with("id", String::from(id)))
    };
    // With network: the shell gets it and answers.
    let log = log();
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(OverlayPlugin { log: log.clone() })
        .network(true)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel.request(request("a"));
    let calls = kernel.take_requests();
    assert_eq!(calls.len(), 1);
    assert_eq!(
        (calls[0].method.as_str(), calls[0].url.as_str()),
        ("GET", "https://example.org/x")
    );
    assert!(kernel.take_requests().is_empty());
    kernel.http_response(
        Instant(5),
        crate::HttpResponse {
            id: calls[0].id,
            status: 200,
            body: b"ok".to_vec(),
            etag: None,
            ..Default::default()
        },
    );
    assert!(log.borrow().contains(&String::from("event ov.reply a")));
    // Without: it fails at once.
    let log = self::log();
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(OverlayPlugin { log: log.clone() })
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel.request(request("b"));
    kernel.run_commands();
    assert!(kernel.take_requests().is_empty());
    assert!(log.borrow().contains(&String::from("event ov.reply b")));
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "longer than the limit from before it was set; split it when it is next changed"
)]
fn plugins_serve_through_the_shell() {
    use paperpaw_sdk::server_params as p;
    let log = log();
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(OverlayPlugin { log: log.clone() })
        .storage(paperpaw_sdk::MemoryStorage::new())
        .network(true)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    let events = Id::new_static("ov.reply");
    kernel.output.commands.push(Command::serve(
        Id::new_static("ov.server"),
        8080,
        events.clone(),
    ));
    kernel.run_commands();
    assert!(kernel.serving());
    let calls = kernel.take_server_calls();
    let crate::ServerCall::Listen { server, port } = calls[0] else {
        panic!("{calls:?}");
    };
    assert_eq!(port, 8080);
    kernel.server_event(
        Instant(1),
        crate::ServerEvent::Request {
            server,
            request: 7,
            method: String::from("PUT"),
            target: String::from("/up?name=a"),
            body: true,
        },
    );
    // The plugin hears of it once the body is complete.
    let told = |log: &Log| log.borrow().iter().any(|l| l.starts_with("event ov.reply"));
    assert!(!told(&log));
    for (data, last) in [(&b"da"[..], false), (&b"ta"[..], true)] {
        kernel.server_event(
            Instant(1),
            crate::ServerEvent::Body {
                request: 7,
                data: data.to_vec(),
                last,
            },
        );
    }
    assert!(told(&log));
    let path = String::from(".paperpaw/tmp/request-7");
    let storage = kernel.shared.storage.as_ref();
    assert_eq!(
        paperpaw_sdk::read_file(storage, &path, 64).unwrap(),
        b"data"
    );
    kernel
        .output
        .commands
        .push(Command::respond(paperpaw_sdk::ServerResponse::new(
            7,
            200,
            "text/plain",
            b"ok".to_vec(),
        )));
    // A file goes in parts, read by the shell.
    let mut response = paperpaw_sdk::ServerResponse::new(8, 200, "text/plain", Vec::new());
    response = response.body_from(String::from("served.txt"));
    kernel
        .shared
        .storage
        .write("served.txt", b"0123456789")
        .unwrap();
    kernel.output.commands.push(Command::respond(response));
    kernel.run_commands();
    let calls = kernel.take_server_calls();
    assert!(
        matches!(&calls[0], crate::ServerCall::Respond { request: 7, status: 200, body, .. } if body == b"ok")
    );
    let crate::ServerCall::Respond {
        file: Some(file), ..
    } = &calls[1]
    else {
        panic!("{calls:?}");
    };
    assert_eq!(file.size, 10);
    let mut part = [0u8; 4];
    assert_eq!(kernel.read_served(&file.path, 8, &mut part).unwrap(), 2);
    assert_eq!(&part[..2], b"89");
    assert!(
        !kernel.shared.storage.exists(&path),
        "the body is cleaned up"
    );
    kernel
        .output
        .commands
        .push(Command::StopServing(Id::new_static("ov.server")));
    kernel.run_commands();
    assert!(!kernel.serving());
    assert!(matches!(
        kernel.take_server_calls()[0],
        crate::ServerCall::Stop { .. }
    ));
    // A server stops with the screen it was started on.
    kernel.output.commands.push(Command::OpenScreen(
        Id::new_static("ov.base"),
        paperpaw_sdk::Params::new().with("n", "2"),
    ));
    kernel.run_commands();
    kernel.output.commands.push(Command::serve(
        Id::new_static("ov.server"),
        8080,
        events.clone(),
    ));
    kernel.run_commands();
    assert!(kernel.serving());
    kernel.take_server_calls();
    kernel.output.commands.push(Command::Back);
    kernel.run_commands();
    assert!(!kernel.serving());
    assert!(matches!(
        kernel.take_server_calls()[0],
        crate::ServerCall::Stop { .. }
    ));
    let _ = p::KIND;
}

#[test]
fn files_go_up_and_come_down() {
    let storage = paperpaw_sdk::MemoryStorage::new().with("up.bin", b"\0\x01data".to_vec());
    let mut kernel = Kernel::builder(DEVICE)
        .storage(storage)
        .network(true)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    let reply = Id::new_static("core.test-reply");
    kernel.request(
        paperpaw_sdk::HttpRequest::new("PUT", "https://example.org/f", reply.clone())
            .body_from("up.bin")
            .save_to("down.bin"),
    );
    let calls = kernel.take_requests();
    // The shell reads the file to send, so it never needs to fit in memory.
    assert_eq!(calls[0].body, None);
    assert_eq!(calls[0].body_from.as_deref(), Some("up.bin"));
    assert_eq!(calls[0].max_response, crate::MAX_FILE);
    assert_eq!(calls[0].save_to.as_deref(), Some("down.bin"));
    kernel.http_response(
        Instant(1),
        crate::HttpResponse {
            id: calls[0].id,
            status: 200,
            body: b"\xffbinary".to_vec(),
            etag: Some(String::from("\"abc\"")),
            content_type: Some(String::from("application/epub+zip")),
            url: Some(String::from("https://example.org/g")),
            saved: false,
            ..crate::HttpResponse::default()
        },
    );
    let storage = kernel.shared.storage.as_ref();
    assert_eq!(
        paperpaw_sdk::read_file(storage, "down.bin", 64).unwrap(),
        b"\xffbinary"
    );
    // Written by the shell itself as it came: left as it is.
    kernel.request(
        paperpaw_sdk::HttpRequest::get("http://example.org/k", reply.clone()).save_to("kept.bin"),
    );
    let calls = kernel.take_requests();
    kernel.storage().write("kept.bin", b"streamed").unwrap();
    kernel.http_response(
        Instant(2),
        crate::HttpResponse {
            id: calls[0].id,
            status: 200,
            saved: true,
            ..Default::default()
        },
    );
    assert_eq!(
        paperpaw_sdk::read_file(kernel.storage(), "kept.bin", 64).unwrap(),
        b"streamed"
    );
    // A file that is not there fails at once.
    kernel.request(
        paperpaw_sdk::HttpRequest::new("PUT", "https://example.org/f", reply)
            .body_from("missing.bin"),
    );
    assert!(kernel.take_requests().is_empty());
}

#[test]
fn copied_text_reaches_plugins_and_the_shell() {
    let log = log();
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(OverlayPlugin { log: log.clone() })
        .build()
        .unwrap();
    kernel.start(Instant(0));
    assert!(!kernel.shared.env(Instant(0)).clipboard());
    let read = |id: &str| {
        Command::ReadClipboard(
            Id::new_static("ov.reply"),
            paperpaw_sdk::Params::new().with("id", String::from(id)),
        )
    };
    kernel
        .output
        .commands
        .extend([Command::SetClipboard(String::from("Alice")), read("a")]);
    kernel.run_commands();
    assert!(kernel.shared.env(Instant(0)).clipboard());
    assert_eq!(kernel.take_io(), [Io::Copy(String::from("Alice"))]);
    assert_eq!(kernel.take_io(), []);
    // Without the system's clipboard, the answer comes at once.
    assert!(log.borrow().contains(&String::from("event ov.reply a")));
    // Long text is cut at a character boundary.
    let long = "ä".repeat(paperpaw_sdk::MAX_CLIPBOARD);
    assert_eq!(crate::clipped(&long).len(), paperpaw_sdk::MAX_CLIPBOARD);

    // With it: the shell reads it when asked.
    let log = self::log();
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(OverlayPlugin { log: log.clone() })
        .system_clipboard(true)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel.set_clipboard_available(true);
    assert!(kernel.shared.env(Instant(0)).clipboard());
    kernel.output.commands.push(read("b"));
    kernel.run_commands();
    // Asked once, however many wait.
    assert_eq!(kernel.take_io(), [Io::ReadClipboard]);
    kernel.output.commands.push(read("c"));
    kernel.run_commands();
    assert_eq!(kernel.take_io(), []);
    assert!(!log.borrow().contains(&String::from("event ov.reply b")));
    kernel.clipboard_text(Instant(1), Some(String::from("from another app")));
    assert_eq!(kernel.take_io(), []);
    assert!(log.borrow().contains(&String::from("event ov.reply c")));
    assert!(log.borrow().contains(&String::from("event ov.reply b")));
}

#[test]
fn secrets_are_kept_apart_and_read_at_start() {
    use paperpaw_sdk::secrets::{FILE, SecretChange};
    let mut kernel = Kernel::builder(DEVICE)
        .storage(paperpaw_sdk::MemoryStorage::new())
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel
        .output
        .commands
        .push(Command::set_secret(SecretChange::keep(
            "a.key", "a.label", "hunter2",
        )));
    kernel.run_commands();
    // Its plugin sees it; another, or the kernel's own code, not.
    assert_eq!(kernel.shared.env(Instant(0)).secrets().get("a.key"), None);
    {
        let _other = kernel.shared.meter.enter("b");
        assert_eq!(kernel.shared.env(Instant(0)).secrets().get("a.key"), None);
        assert_eq!(kernel.shared.env(Instant(0)).secrets().listed().count(), 1);
    }
    {
        let _own = kernel.shared.meter.enter("a");
        assert_eq!(
            kernel.shared.env(Instant(0)).secrets().get("a.key"),
            Some("hunter2")
        );
    }
    let data = paperpaw_sdk::read_file(kernel.shared.storage.as_ref(), FILE, 4096).unwrap();
    assert!(!String::from_utf8_lossy(&data).contains("hunter2"));
    // Read again at the next start.
    let mut kernel = Kernel::builder(DEVICE)
        .storage(paperpaw_sdk::MemoryStorage::new().with(FILE, data))
        .build()
        .unwrap();
    kernel.start(Instant(0));
    assert_eq!(kernel.shared.secrets.get("a.key"), Some("hunter2"));
    // The last one removed, the file goes.
    kernel
        .output
        .commands
        .push(Command::set_secret(SecretChange::remove("a.key")));
    kernel.run_commands();
    assert!(kernel.shared.secrets.is_empty());
    assert!(!kernel.shared.storage.exists(FILE));
}

#[test]
fn changed_settings_are_saved_and_announced() {
    let log = log();
    let storage = paperpaw_sdk::MemoryStorage::new()
        .with("paperpaw.ini", "# my settings\n[core]\nui-size = 12\n");
    let mut kernel = Kernel::builder(DEVICE)
        .storage(storage)
        .plugin(OverlayPlugin { log: log.clone() })
        .build()
        .unwrap();
    assert_eq!(kernel.settings().int("core.ui-size"), 12);
    kernel.start(Instant(0));
    log.borrow_mut().clear();

    // Long tap in the centre runs core.toggle-night-mode.
    let down = InputEvent::Pointer(PointerEvent::new(
        0,
        PointerPhase::Down,
        Point::new(45, 45),
        PointerTool::Finger,
        1000,
    ));
    kernel.handle(Instant(100), down);
    kernel.tick(Instant(600));
    assert!(kernel.settings().bool("core.night-mode"));
    assert!(
        log.borrow()
            .contains(&String::from("event core.setting-changed core.night-mode"))
    );

    let mut buf = [0u8; 256];
    let n = kernel
        .shared
        .storage
        .read_at("paperpaw.ini", 0, &mut buf)
        .unwrap();
    assert_eq!(
        core::str::from_utf8(&buf[..n]).unwrap(),
        "# my settings\n[core]\nui-size = 12\nnight-mode = on\n"
    );
}

#[test]
fn plugins_switch_on_and_off_for_the_next_start() {
    use paperpaw_sdk::PluginState::{Off, Running};

    let log = log();
    let storage = paperpaw_sdk::MemoryStorage::new().with(
        "paperpaw.ini",
        "[plugins]
b = off
",
    );
    let mut kernel = Kernel::builder(DEVICE)
        .storage(storage)
        .plugin(OverlayPlugin { log: log.clone() })
        .plugin(TestPlugin::new("a", &log))
        .plugin(TestPlugin::new("b", &log))
        .build()
        .unwrap();
    let states: Vec<(&str, paperpaw_sdk::PluginState)> = kernel
        .plugins()
        .iter()
        .map(|p| (p.manifest.id.as_str(), p.state))
        .collect();
    assert_eq!(
        states,
        [
            ("core", Running),
            ("ov", Running),
            ("a", Running),
            ("b", Off)
        ]
    );
    assert!(kernel.settings().get("b.level").is_none());
    // "ov" alone provides the menu; the home screen has two providers.
    let ov = kernel.plugins().get("ov").unwrap();
    assert_eq!(ov.required_for.iter().collect::<Vec<_>>(), [&slots::MENU]);
    assert!(!kernel.plugins().restart_needed());

    kernel.apply(Command::SetPluginEnabled(Id::new_static("a"), false));
    kernel.apply(Command::SetPluginEnabled(Id::new_static("b"), true));
    // The kernel and plugins that alone provide a slot stay on.
    kernel.apply(Command::SetPluginEnabled(Id::new_static("core"), false));
    kernel.apply(Command::SetPluginEnabled(Id::new_static("ov"), false));
    assert_eq!(kernel.take_warnings().len(), 2);
    assert!(kernel.plugins().get("ov").unwrap().enabled);
    assert!(kernel.plugins().get("a").unwrap().restart_needed());
    assert!(kernel.plugins().restart_needed());

    let mut buf = [0u8; 64];
    let n = kernel
        .shared
        .storage
        .read_at("paperpaw.ini", 0, &mut buf)
        .unwrap();
    assert_eq!(
        core::str::from_utf8(&buf[..n]).unwrap(),
        "[plugins]
b = on
a = off
"
    );

    kernel.apply(Command::Restart);
    assert!(kernel.finish().restart());
}

#[test]
fn data_plugins_load_from_storage() {
    use paperpaw_sdk::PluginState::{Off, Running};

    let files = |config: &str| {
        paperpaw_sdk::MemoryStorage::new()
            .with(
                "plugins/large/plugin.ini",
                "[plugin]
name = Large print
description = Bigger text.
",
            )
            .with(
                "plugins/large/settings.ini",
                "[core]
ui-size = 14
night-mode = on
",
            )
            .with(
                "plugins/large/i18n/de.ini",
                "[meta]
language = de
name = Deutsch
plural = one-other

[messages]
large.plugin.name = Großdruck
",
            )
            .with("paperpaw.ini", config)
    };

    // Its defaults apply; the user's own choice wins.
    let kernel = Kernel::builder(DEVICE)
        .storage(files(
            "[core]
night-mode = off
",
        ))
        .build()
        .unwrap();
    let large = kernel.plugins().get("large").unwrap();
    assert_eq!(large.state, Running);
    assert_eq!(large.manifest.name, "Large print");
    assert_eq!(kernel.settings().int("core.ui-size"), 14);
    assert!(!kernel.settings().bool("core.night-mode"));

    // Switched off: listed with its translated name, no defaults.
    let mut kernel = Kernel::builder(DEVICE)
        .storage(files(
            "[plugins]
large = off
",
        ))
        .build()
        .unwrap();
    kernel.shared.translator.set_language("de");
    let large = kernel.plugins().get("large").unwrap();
    assert_eq!(large.state, Off);
    assert_eq!(large.name(&kernel.shared.translator), "Großdruck");
    assert_ne!(kernel.settings().int("core.ui-size"), 14);
}

#[test]
fn data_plugins_load_and_go_while_running() {
    use paperpaw_sdk::PluginState::Running;
    use paperpaw_text::{Glyph, LineMetrics, PackBuilder};

    let mut builder = PackBuilder::new();
    let metrics = LineMetrics::new(8.0, 2.0, 12.0);
    builder.add_strike(10.0, metrics, alloc::vec![('a', Glyph::empty(5.0))]);
    let pack = builder.build();

    let mut kernel = Kernel::builder(DEVICE)
        .storage(paperpaw_sdk::MemoryStorage::new())
        .build()
        .unwrap();
    kernel.take_warnings();
    // Downloaded while running: a family of one face.
    let storage = kernel.shared.storage.clone();
    storage
        .write(
            "plugins/font-round/plugin.ini",
            b"[plugin]
name = Round

[family]
name = Round
regular = round.ppfp
",
        )
        .unwrap();
    storage
        .write("plugins/font-round/fonts/round.ppfp", &pack)
        .unwrap();
    kernel.apply(Command::LoadPlugin("plugins/font-round".into()));
    assert!(kernel.take_warnings().is_empty());
    assert_eq!(kernel.plugins().get("font-round").unwrap().state, Running);
    assert!(kernel.shared.fonts.families().contains(&"Round"));
    assert!(kernel.output.commands.iter().any(
        |c| matches!(c, Command::Emit(e, _) if *e == paperpaw_sdk::builtin::events::PLUGINS_CHANGED)
    ));

    // Chosen for reading at once.
    kernel.apply(Command::SetSetting(
        settings::READING_FONT.id().clone(),
        SettingValue::Text("Round".into()),
    ));
    assert!(kernel.take_warnings().is_empty());
    let reading = kernel
        .shared
        .fonts
        .role(paperpaw_sdk::builtin::fonts::READING.as_str())
        .unwrap();
    assert_eq!(reading.id().as_str(), "font-round.regular");

    // Removed: its folder, its fonts, and the choice gives way.
    kernel.apply(Command::RemovePlugin(Id::new_static("font-round")));
    assert!(kernel.take_warnings().is_empty());
    assert!(!storage.exists("plugins/font-round/plugin.ini"));
    assert!(kernel.plugins().get("font-round").is_none());
    assert!(!kernel.shared.fonts.families().contains(&"Round"));
    assert_eq!(
        kernel.settings().text(settings::READING_FONT.as_str()),
        settings::DEFAULT_FONT
    );

    // Plugins with code stay.
    kernel.apply(Command::RemovePlugin(Id::new_static("core")));
    assert_eq!(kernel.take_warnings().len(), 1);
    assert!(kernel.plugins().get("core").is_some());
}

/// A font plugin `font-round` in `version`, its family Round.
fn round_plugin(version: &str, extra: &str) -> String {
    alloc::format!(
        "[plugin]
id = font-round
name = Round
version = {version}
{extra}
[family]
name = Round
regular = round.ppfp
"
    )
}

fn round_pack() -> Vec<u8> {
    use paperpaw_text::{Glyph, LineMetrics, PackBuilder};
    let mut builder = PackBuilder::new();
    let metrics = LineMetrics::new(8.0, 2.0, 12.0);
    builder.add_strike(10.0, metrics, alloc::vec![('a', Glyph::empty(5.0))]);
    builder.build()
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "longer than the limit from before it was set; split it when it is next changed"
)]
fn data_plugins_are_installed_in_place_of_every_version() {
    use paperpaw_sdk::PluginState::{Off, Running};

    let pack = round_pack();
    let storage = paperpaw_sdk::MemoryStorage::new()
        // Loaded at start, in a folder of another name.
        .with("plugins/round-old/plugin.ini", round_plugin("1.0.0", ""))
        .with("plugins/round-old/fonts/round.ppfp", pack.clone())
        // Refused at start: made for later formats.
        .with(
            "plugins/round-later/plugin.ini",
            round_plugin("3.0.0", "formats = 99"),
        )
        .with("plugins/other/plugin.ini", "[plugin]\nname = Other\n");
    let mut kernel = Kernel::builder(DEVICE).storage(storage).build().unwrap();
    kernel.take_warnings();
    kernel.apply(Command::SetSetting(
        settings::READING_FONT.id().clone(),
        SettingValue::Text("Round".into()),
    ));
    let storage = kernel.shared.storage.clone();
    let stage = |version: &str| {
        storage
            .write(
                "plugins/.font-round.part/plugin.ini",
                round_plugin(version, "").as_bytes(),
            )
            .unwrap();
        storage
            .write("plugins/.font-round.part/fonts/round.ppfp", &pack)
            .unwrap();
    };

    // A newer version: in its own folder, the others gone, the font one
    // reads with still chosen.
    stage("1.1.0");
    kernel.apply(Command::InstallPlugin(
        Id::new_static("font-round"),
        "plugins/.font-round.part".into(),
    ));
    assert!(kernel.take_warnings().is_empty());
    let round = kernel.plugins().get("font-round").unwrap();
    assert_eq!((round.state, round.manifest.version.minor), (Running, 1));
    assert!(storage.exists("plugins/font-round/fonts/round.ppfp"));
    for gone in [
        "plugins/round-old/plugin.ini",
        "plugins/round-later/plugin.ini",
        "plugins/.font-round.part/plugin.ini",
        "plugins/.font-round.old/plugin.ini",
    ] {
        assert!(!storage.exists(gone), "{gone}");
    }
    assert!(storage.exists("plugins/other/plugin.ini"));
    assert_eq!(
        kernel.settings().text(settings::READING_FONT.as_str()),
        "Round"
    );
    let reading = kernel
        .shared
        .fonts
        .role(paperpaw_sdk::builtin::fonts::READING.as_str())
        .unwrap();
    assert_eq!(reading.id().as_str(), "font-round.regular");

    // Updated while switched off: it stays off, in its folder.
    let storage = paperpaw_sdk::MemoryStorage::new()
        .with(
            "paperpaw.ini",
            "[plugins]
font-round = off
",
        )
        .with("plugins/font-round/plugin.ini", round_plugin("1.1.0", ""))
        .with("plugins/font-round/fonts/round.ppfp", pack.clone())
        .with(
            "plugins/other/plugin.ini",
            "[plugin]
name = Other
",
        );
    let mut kernel = Kernel::builder(DEVICE).storage(storage).build().unwrap();
    assert_eq!(kernel.plugins().get("font-round").unwrap().state, Off);
    let storage = kernel.shared.storage.clone();
    let stage = |version: &str| {
        storage
            .write(
                "plugins/.font-round.part/plugin.ini",
                round_plugin(version, "").as_bytes(),
            )
            .unwrap();
        storage
            .write("plugins/.font-round.part/fonts/round.ppfp", &pack)
            .unwrap();
    };
    stage("1.2.0");
    kernel.apply(Command::InstallPlugin(
        Id::new_static("font-round"),
        "plugins/.font-round.part".into(),
    ));
    let round = kernel.plugins().get("font-round").unwrap();
    assert_eq!((round.state, round.manifest.version.minor), (Off, 2));
    assert!(storage.exists("plugins/font-round/plugin.ini"));
    assert!(!storage.exists("plugins/.font-round.part/plugin.ini"));

    // A folder holding another plugin than it says: refused, the version
    // before stays.
    kernel.take_warnings();
    stage("2.0.0");
    kernel.apply(Command::InstallPlugin(
        Id::new_static("other"),
        "plugins/.font-round.part".into(),
    ));
    assert_eq!(kernel.take_warnings().len(), 1);
    assert!(!storage.exists("plugins/.font-round.part/plugin.ini"));
    let round = kernel.plugins().get("font-round").unwrap();
    assert_eq!(round.manifest.version.minor, 2);
    assert!(storage.exists("plugins/other/plugin.ini"));
}

/// Swapped page buttons apply while reading only (the reader's tests
/// show them swapped there): elsewhere they stay as they are.
#[test]
fn page_buttons_stay_outside_the_reader() {
    let log = log();
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(TestPlugin::new("a", &log))
        .setting_default(&settings::SWAP_PAGE_BUTTONS, true)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    press(&mut kernel, 100, "page-forward");
    press(&mut kernel, 300, "page-back");
    press(&mut kernel, 500, "confirm");
    assert_eq!(
        *log.borrow(),
        ["a:core.next-page", "a:core.prev-page", "a:core.confirm"]
    );
}

#[test]
fn keymap_presets_follow_device_defaults_and_switch_at_runtime() {
    let log = log();
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(TestPlugin::new("a", &log))
        .setting_default(&settings::KEYMAP_PRESET, "core.two-buttons".into())
        .build()
        .unwrap();
    assert!(kernel.take_warnings().is_empty());
    assert_eq!(
        kernel.settings().text("core.keymap-preset"),
        "core.two-buttons"
    );
    kernel.start(Instant(0));

    // Two-button preset: forward is "next", which falls back to next page;
    // a long press on forward confirms.
    press(&mut kernel, 100, "page-forward");
    kernel.handle(
        Instant(300),
        InputEvent::Button {
            name: "page-forward",
            state: ButtonState::Pressed,
        },
    );
    kernel.tick(Instant(900));
    assert_eq!(
        *log.borrow(),
        ["a:core.next", "a:core.next-page", "a:core.confirm"]
    );

    // Back to the standard layout: forward turns pages directly again.
    log.borrow_mut().clear();
    kernel.handle(
        Instant(1000),
        InputEvent::Button {
            name: "page-forward",
            state: ButtonState::Released,
        },
    );
    kernel
        .shared
        .settings
        .set(
            "core.keymap-preset",
            paperpaw_sdk::SettingValue::Text(Cow::Borrowed("core.standard")),
        )
        .unwrap();
    kernel.rebuild_keymap();
    press(&mut kernel, 1100, "page-forward");
    assert_eq!(*log.borrow(), ["a:core.next-page"]);

    // Unknown device defaults are reported.
    let mut kernel = Kernel::builder(DEVICE)
        .setting_default(&settings::KEYMAP_PRESET, "nope.preset".into())
        .build()
        .unwrap();
    assert_eq!(kernel.take_warnings().len(), 1);
}

#[test]
fn banded_frames_match_full_frames() {
    // Renders into a copy of the whole screen, as a panel controller keeps it.
    fn screen(kernel: &mut Kernel, copy: &mut paperpaw_graphics::Framebuffer) -> Vec<u8> {
        let mut bands = 0;
        kernel.render(&mut |top, rows| {
            bands += 1;
            copy.write_rows(top, rows);
        });
        assert!(bands > 0);
        copy.view().data.to_vec()
    }
    let build = |rows: Option<u16>| {
        let builder = Kernel::builder(DEVICE).plugin(OverlayPlugin { log: log() });
        match rows {
            Some(rows) => builder.frame_rows(rows),
            None => builder,
        }
        .build()
        .unwrap()
    };
    let new_copy = || {
        let display = DEVICE.display;
        paperpaw_graphics::Framebuffer::new(display.width, display.height, display.format).unwrap()
    };
    let (mut full, mut banded) = (build(None), build(Some(16)));
    let (mut full_copy, mut banded_copy) = (new_copy(), new_copy());
    full.start(Instant(0));
    banded.start(Instant(0));
    assert_eq!(
        screen(&mut full, &mut full_copy),
        screen(&mut banded, &mut banded_copy)
    );
    // An overlay redraws only part of the screen, across band borders.
    for kernel in [&mut full, &mut banded] {
        for (at, state) in [(100, ButtonState::Pressed), (150, ButtonState::Released)] {
            kernel.handle(
                Instant(at),
                InputEvent::Button {
                    name: "menu",
                    state,
                },
            );
        }
    }
    assert_eq!(
        screen(&mut full, &mut full_copy),
        screen(&mut banded, &mut banded_copy)
    );
    assert!(banded.frame().height <= 16);
}

#[test]
fn turned_screens_are_made_anew_for_the_new_size() {
    let log = log();
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(OverlayPlugin { log: log.clone() })
        .build()
        .unwrap();
    kernel.start(Instant(0));
    for (at, state) in [(100, ButtonState::Pressed), (150, ButtonState::Released)] {
        kernel.handle(
            Instant(at),
            InputEvent::Button {
                name: "menu",
                state,
            },
        );
    }
    let open: Vec<Id> = kernel.stack.iter().map(|s| s.id.clone()).collect();
    let effects = kernel.resize(Instant(200), 120, 60);
    assert_eq!(
        effects.refresh().map(|(_, mode)| mode),
        Some(RefreshMode::Full)
    );
    assert_eq!(kernel.device().display.width, 120);
    assert_eq!(kernel.shared.env(Instant(200)).device().display.height, 60);
    let mut size = (0, 0);
    kernel.render(&mut |_, rows| size = (rows.width, size.1 + rows.height));
    assert_eq!(size, (120, 60));
    // The same screens, in the same order.
    let reopened: Vec<Id> = kernel.stack.iter().map(|s| s.id.clone()).collect();
    assert_eq!(reopened, open);
    // The same size again (turned half way round): drawn whole.
    let effects = kernel.resize(Instant(300), 120, 60);
    assert_eq!(
        effects.refresh().map(|(_, mode)| mode),
        Some(RefreshMode::Full)
    );
}

#[test]
fn turned_bands_keep_their_memory() {
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(OverlayPlugin { log: log() })
        .frame_rows(32)
        .row_align(8)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    // Wider: fewer rows a band, still whole multiples of 8.
    kernel.resize(Instant(100), 150, 54);
    assert_eq!((kernel.frame().width, kernel.frame().height), (150, 16));
    let mut tops = Vec::new();
    kernel.render(&mut |top, rows| tops.push((top, rows.height)));
    assert_eq!(tops, [(0, 16), (16, 16), (32, 16), (48, 6)]);
    // Back: the rows it had.
    kernel.resize(Instant(200), 90, 90);
    assert_eq!(kernel.frame().height, 32);
}

#[test]
fn bands_pass_whole_aligned_rows_and_again() {
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(OverlayPlugin { log: log() })
        .frame_rows(16)
        .row_align(8)
        .build()
        .unwrap();
    let passed = |kernel: &mut Kernel, again: bool| {
        let mut parts = Vec::new();
        let mut out = |top: u16, rows: paperpaw_platform::FrameView<'_>| {
            parts.push((top, rows.height, rows.width))
        };
        if again {
            kernel.rerender(&mut out);
        } else {
            kernel.render(&mut out);
        }
        parts
    };
    kernel.start(Instant(0));
    passed(&mut kernel, false);
    // The overlay covers rows 45 to 90: passed from row 40, in bands of 16.
    for (at, state) in [(100, ButtonState::Pressed), (150, ButtonState::Released)] {
        kernel.handle(
            Instant(at),
            InputEvent::Button {
                name: "menu",
                state,
            },
        );
    }
    let parts = passed(&mut kernel, false);
    assert_eq!(
        parts,
        [(40, 8, 90), (48, 16, 90), (64, 16, 90), (80, 10, 90)]
    );
    // Drawn again the same, e.g. for the controller's memory of the frame shown.
    assert_eq!(passed(&mut kernel, true), parts);

    // Captured: the whole screen in bands, and what was rendered last
    // stays what is drawn again.
    let mut captured = Vec::new();
    assert!(kernel.capture(&mut |top, rows| captured.push((top, rows.height))));
    assert_eq!(captured.first(), Some(&(0, 16)));
    let height = captured.iter().map(|(_, h)| *h).sum::<u16>();
    assert_eq!(height, kernel.framebuffer.bounds().height);
    assert_eq!(passed(&mut kernel, true), parts);
    // With changes waiting, nothing is captured.
    kernel.frame.unrendered = Some(Rect::new(0, 0, 10, 10));
    assert!(!kernel.capture(&mut |_, _| panic!("captured with changes waiting")));
}

#[test]
fn the_power_button_puts_the_device_to_sleep_where_the_shell_can() {
    // Where the system sleeps on its own (Android), nothing happens.
    let mut kernel = Kernel::builder(DEVICE).build().unwrap();
    kernel.start(Instant(0));
    assert!(!kernel.shared.env(Instant(0)).caps().sleep);
    assert!(!press(&mut kernel, 100, "power").sleep());
    // Where the shell sleeps, it is told to.
    let mut kernel = Kernel::builder(DEVICE).sleep(true).build().unwrap();
    kernel.start(Instant(0));
    assert!(kernel.shared.env(Instant(0)).caps().sleep);
    assert!(press(&mut kernel, 100, "power").sleep());
    assert!(!press(&mut kernel, 200, "confirm").sleep());
}

#[test]
fn the_device_sleeps_by_itself_after_a_while_unused() {
    let mut kernel = Kernel::builder(DEVICE)
        .sleep(true)
        .setting_default(&settings::SLEEP_AFTER, 1)
        .build()
        .unwrap();
    let effects = kernel.start(Instant(0));
    // A wake-up is asked for when the minute is over.
    assert_eq!(effects.wake_at(), Some(Instant(60_000)));
    // Use puts it off.
    press(&mut kernel, 30_000, "confirm");
    assert!(!kernel.tick(Instant(60_000)).sleep());
    assert!(kernel.tick(Instant(90_100)).sleep());
    // Never, where the setting says so.
    let mut kernel = Kernel::builder(DEVICE)
        .sleep(true)
        .setting_default(&settings::SLEEP_AFTER, 0)
        .build()
        .unwrap();
    assert_eq!(kernel.start(Instant(0)).wake_at(), None);
    assert!(!kernel.tick(Instant(10_000_000)).sleep());
}

#[test]
fn speech_goes_to_the_shell_and_stops_with_its_screen() {
    let speech = |id: &str, queued: bool| {
        let params = paperpaw_sdk::Params::new().with("id", String::from(id));
        let speech = paperpaw_sdk::Speech::new("Hello", Id::new_static("ov.reply"));
        Command::speak(speech.with(params).queued(queued))
    };
    // Without speech, the reply comes at once.
    let log = log();
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(OverlayPlugin { log: log.clone() })
        .build()
        .unwrap();
    kernel.start(Instant(0));
    assert!(!kernel.shared.env(Instant(0)).caps().speech);
    kernel.output.commands.push(speech("a", false));
    kernel.run_commands();
    assert!(kernel.take_speech().is_empty());
    assert!(log.borrow().contains(&String::from("event ov.reply a")));

    // With it: the shell speaks and says when it is done.
    let log = self::log();
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(OverlayPlugin { log: log.clone() })
        .speech(true)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel.output.commands.push(speech("b", false));
    kernel.run_commands();
    let calls = kernel.take_speech();
    let [SpeechCall::Say { id, text, rate, .. }] = calls.as_slice() else {
        panic!("{calls:?}");
    };
    assert_eq!((text.as_str(), *rate), ("Hello", 100));
    assert!(kernel.speaking());
    assert!(!log.borrow().contains(&String::from("event ov.reply b")));
    kernel.speech_done(Instant(5), *id, true);
    assert!(!kernel.speaking());
    assert!(log.borrow().contains(&String::from("event ov.reply b")));

    // Queued texts follow; each is answered when done.
    kernel
        .output
        .commands
        .extend([speech("q1", false), speech("q2", true)]);
    kernel.run_commands();
    let calls = kernel.take_speech();
    let ids: Vec<(u32, bool)> = calls
        .iter()
        .filter_map(|c| match c {
            SpeechCall::Say { id, queue, .. } => Some((*id, *queue)),
            SpeechCall::Stop => None,
        })
        .collect();
    assert_eq!(ids.iter().map(|i| i.1).collect::<Vec<_>>(), [false, true]);
    kernel.speech_done(Instant(6), ids[0].0, true);
    assert!(kernel.speaking());
    assert!(log.borrow().contains(&String::from("event ov.reply q1")));
    kernel.speech_done(Instant(7), ids[1].0, true);
    assert!(!kernel.speaking());
    assert!(log.borrow().contains(&String::from("event ov.reply q2")));

    // Started over the menu, it stops when the menu closes.
    kernel.output.commands.push(Command::open_slot(slots::MENU));
    kernel.run_commands();
    kernel.output.commands.push(speech("c", false));
    kernel.run_commands();
    let Some(SpeechCall::Say { id, .. }) = kernel.take_speech().pop() else {
        panic!("no speech");
    };
    kernel.output.commands.push(Command::Back);
    kernel.run_commands();
    assert_eq!(kernel.take_speech(), [SpeechCall::Stop]);
    assert!(!kernel.speaking());
    // An end reported late is not passed on.
    kernel.speech_done(Instant(9), id, true);
    assert!(!log.borrow().contains(&String::from("event ov.reply c")));
}

/// Publishes a service and a point (`provider`), or uses them (`user`).
struct ServicePlugin {
    id: &'static str,
    log: Log,
}

impl Plugin for ServicePlugin {
    fn manifest(&self) -> Manifest {
        Manifest::new(
            Id::parse(self.id).unwrap(),
            "Services",
            Version::new(1, 0, 0),
        )
        .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        if self.id == "provider" {
            r.provide(
                Id::new_static("provider.greeting"),
                Greeting("hello"),
                Version::new(1, 0, 0),
            )?;
            // Not in its namespace: refused.
            let stolen = r.provide(
                Id::new_static("core.greeting"),
                Greeting("x"),
                Version::new(1, 0, 0),
            );
            assert!(matches!(stolen, Err(RegisterError::Namespace { .. })));
            return r.contribute(Id::new_static("provider.shapes"), Cow::Borrowed("circle"));
        }
        r.contribute(Id::new_static("provider.shapes"), Cow::Borrowed("square"))?;
        let log = self.log.clone();
        r.subscribe(
            paperpaw_sdk::builtin::events::STARTED,
            Box::new(
                move |env: &Env<'_>, _: &paperpaw_sdk::Params, _: &mut paperpaw_sdk::Commands| {
                    let greeting = env
                        .services()
                        .get::<Greeting>("provider.greeting", Version::new(1, 0, 0))
                        .map(|g| &g.0);
                    log.borrow_mut().push(alloc::format!("{greeting:?}"));
                    for (plugin, shape) in env.services().contributions("provider.shapes") {
                        log.borrow_mut().push(alloc::format!("{plugin}:{shape}"));
                    }
                },
            ),
        )
    }
}

#[test]
fn plugins_offer_each_other_services() {
    let log = log();
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(TestPlugin::new("a", &log))
        .plugin(ServicePlugin {
            id: "user",
            log: log.clone(),
        })
        .plugin(ServicePlugin {
            id: "provider",
            log: log.clone(),
        })
        .build()
        .unwrap();
    kernel.start(Instant(0));
    assert_eq!(
        *log.borrow(),
        ["Some(\"hello\")", "user:square", "provider:circle"]
    );
}

/// Turns the `menu` button into the next page, swallows `confirm`, and
/// refreshes everything in full.
struct FilterPlugin;

impl Plugin for FilterPlugin {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("filter"), "Filters", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        use paperpaw_sdk::filters::{
            InputFilterSpec, InputVerdict, RefreshFilterSpec, RefreshVerdict,
        };
        r.add_input_filter(InputFilterSpec::new(
            Id::new_static("filter.input"),
            "",
            Box::new(|_, event| match event {
                InputEvent::Button {
                    name: "menu",
                    state: ButtonState::Pressed,
                } => InputVerdict::Run(actions::NEXT_PAGE),
                InputEvent::Button {
                    name: "menu" | "confirm",
                    ..
                } => InputVerdict::Take,
                _ => InputVerdict::Pass,
            }),
        ))?;
        r.add_refresh_filter(RefreshFilterSpec::new(
            Id::new_static("filter.refresh"),
            "",
            Box::new(|_, refresh| {
                if refresh.screen.as_ref().map(Id::as_str) == Some("a.screen") {
                    RefreshVerdict::Mode(RefreshMode::Full)
                } else {
                    RefreshVerdict::Keep
                }
            }),
        ))
    }
}

#[test]
fn filters_see_inputs_and_refreshes_first() {
    let log = log();
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(TestPlugin::new("a", &log))
        .plugin(FilterPlugin)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    render(&mut kernel);
    let button = |name, state| InputEvent::Button { name, state };
    let effects = kernel.handle(Instant(100), button("menu", ButtonState::Pressed));
    kernel.handle(Instant(150), button("menu", ButtonState::Released));
    assert_eq!(*log.borrow(), ["a:core.next-page"]);
    // The screen asked for a partial refresh of a corner: in full, whole.
    assert_eq!(
        effects.refresh,
        Some((Rect::new(0, 0, 90, 90), RefreshMode::Full))
    );
    press(&mut kernel, 300, "confirm");
    assert_eq!(log.borrow().len(), 1, "confirm was taken");
}

/// Screens for the home and the reader slot that note the height they
/// were made for; the home one names its confirm button.
struct SizedPlugin(Rc<RefCell<Vec<(&'static str, u16)>>>);

struct SizedScreen;

impl Screen for SizedScreen {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }

    fn hint(&self, action: &Id, _: &Env<'_>) -> paperpaw_sdk::Hint {
        if *action == actions::CONFIRM {
            paperpaw_sdk::Hint::Text("Go".into())
        } else {
            paperpaw_sdk::Hint::Default
        }
    }
}

impl Plugin for SizedPlugin {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("sized"), "Sized", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        for (name, slot) in [("home", slots::HOME), ("reader", slots::READER)] {
            let made = self.0.clone();
            let id = Id::parse(&alloc::format!("sized.{name}")).unwrap();
            r.add_screen(ScreenSpec::new(
                id.clone(),
                "sized",
                Box::new(move |env, _| {
                    made.borrow_mut().push((name, env.device().display.height));
                    Box::new(SizedScreen)
                }),
            ))?;
            r.bind_slot(slot, id, Priority::DEFAULT)?;
        }
        r.set_screen_hints(
            Id::new_static("sized.reader"),
            paperpaw_sdk::HintVisibility::OnPress,
        )?;
        Ok(())
    }
}

#[test]
fn a_row_of_hints_names_the_buttons_below_the_screen() {
    use paperpaw_sdk::Hint;
    let made = Rc::new(RefCell::new(Vec::new()));
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(SizedPlugin(made.clone()))
        .button_row(&["back", "confirm", "left", "right"])
        .build()
        .unwrap();
    kernel.start(Instant(0));
    render(&mut kernel);
    // The home screen is made for the part above the row.
    let (_, above) = made.borrow()[0];
    assert!(above < 90 && above > 45, "{above}");
    assert_eq!(kernel.hint_row(), Some(Rect::new(0, above, 90, 90 - above)));
    // Nothing to go back to; the screen's own word; arrows.
    assert_eq!(
        kernel.hint.shown,
        [
            Hint::Hidden,
            Hint::Text("Go".into()),
            Hint::Icon("chevron-left"),
            Hint::Icon("chevron-right"),
        ]
    );

    // The reader keeps all of the screen, made once for it.
    kernel.apply(Command::OpenSlot(
        slots::READER,
        paperpaw_sdk::Params::new(),
    ));
    kernel.finish();
    assert_eq!(made.borrow().last(), Some(&("reader", 90)));
    assert_eq!(made.borrow().len(), 2);
    assert_eq!(kernel.hint_row(), None);
    // A button of the row shows the row over the page for a while; a side
    // button does not.
    press(&mut kernel, 50, "page-forward");
    assert_eq!(kernel.hint_row(), None);
    let effects = press(&mut kernel, 100, "confirm");
    let over = Rect::new(0, above, 90, 90 - above);
    assert_eq!(kernel.hint_row(), Some(over));
    assert!(effects.wake_at().is_some());
    render(&mut kernel);
    // Recognized as the button is let go, 50 ms after it went down.
    kernel.tick(Instant(150 + 4000));
    assert_eq!(kernel.hint_row(), None);
    // Gone, the page is drawn there again.
    assert_eq!(kernel.frame.unrendered.map(|r| r.bottom()), Some(90));
    kernel.apply(Command::ShowHints);
    kernel.finish();
    assert_eq!(kernel.hint_row(), Some(over));
    // Back from it, the home screen has the part above the row again.
    press(&mut kernel, 5000, "back");
    assert_eq!(kernel.content().height, above);
    assert_eq!(kernel.hint.shown[0], Hint::Hidden);

    // Switched off: the screens are made anew for all of it.
    kernel.apply(Command::set(
        &paperpaw_sdk::builtin::settings::BUTTON_HINTS,
        false,
    ));
    kernel.finish();
    assert_eq!(made.borrow().last(), Some(&("home", 90)));
    assert_eq!(kernel.hint_row(), None);
}

/// A home screen that shows a picture over all of it while asked to.
struct PicturePlugin(Rc<RefCell<bool>>);

struct PictureScreen(Rc<RefCell<bool>>);

impl Screen for PictureScreen {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        if *self.0.borrow() {
            let row = alloc::vec![128u8; usize::from(env.device().display.width)];
            for y in 0..i32::from(env.device().display.height) {
                canvas.draw_gray_row(0, y, &row);
            }
        } else {
            canvas.fill_rect(0, 0, 10, 10, Color::BLACK);
        }
    }

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }
}

impl Plugin for PicturePlugin {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("picture"), "Picture", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let shown = self.0.clone();
        r.add_screen(ScreenSpec::new(
            Id::new_static("picture.home"),
            "picture",
            Box::new(move |_, _| Box::new(PictureScreen(shown.clone()))),
        ))?;
        r.bind_slot(
            slots::HOME,
            Id::new_static("picture.home"),
            Priority::DEFAULT,
        )
    }
}

#[test]
fn leaving_a_large_picture_refreshes_in_full() {
    let shown = Rc::new(RefCell::new(false));
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(PicturePlugin(shown.clone()))
        .build()
        .unwrap();
    kernel.start(Instant(0));
    render(&mut kernel);
    let mode = |effects: crate::Effects| effects.refresh().map(|(_, mode)| mode);
    // A picture shown (drawn when rendered): asked as it is; leaving it,
    // in full.
    *shown.borrow_mut() = true;
    kernel.apply(Command::Redraw(RefreshMode::Partial));
    let effects = kernel.finish();
    assert_eq!(mode(effects), Some(RefreshMode::Partial));
    render(&mut kernel);
    assert!(kernel.frame.pictures_shown);
    *shown.borrow_mut() = false;
    kernel.apply(Command::Redraw(RefreshMode::Partial));
    assert_eq!(mode(kernel.finish()), Some(RefreshMode::Full));
    render(&mut kernel);
    assert!(!kernel.frame.pictures_shown);
}

/// Writes down the network's events, and asks for Wi-Fi on when the
/// network is wanted (as the `wifi` plugin does).
struct NetPlugin(Log);

impl Plugin for NetPlugin {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("net"), "Net", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NETWORK)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        use paperpaw_sdk::builtin::events;
        for event in [
            Id::from(events::NETWORK_WANTED),
            events::NETWORK_IDLE.into(),
            events::WIFI_CHANGED.into(),
            events::WIFI_SCANNED.into(),
            events::WIFI_FAILED.into(),
            Id::new_static("net.reply"),
        ] {
            let log = self.0.clone();
            let name = event.as_str().to_string();
            r.subscribe(
                event.clone(),
                Box::new(move |_, params, out| {
                    let extra = params
                        .get(paperpaw_sdk::http_params::STATUS)
                        .or(params.get(paperpaw_sdk::builtin::params::REASON))
                        .unwrap_or_default();
                    log.borrow_mut().push(alloc::format!("{name} {extra}"));
                    if event == events::NETWORK_WANTED {
                        out.push(Command::Wifi(paperpaw_sdk::wifi::WifiCommand::On));
                    }
                }),
            )?;
        }
        Ok(())
    }
}

fn wifi_kernel(log: &Log) -> Kernel {
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(NetPlugin(log.clone()))
        .wifi(true)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel
}

fn get(kernel: &mut Kernel) {
    kernel.request(paperpaw_sdk::HttpRequest::new(
        "GET",
        "http://example.org/",
        Id::new_static("net.reply"),
    ));
    kernel.finish();
}

#[test]
fn network_work_waits_for_wifi_and_goes_on_once_online() {
    use crate::{WifiCommand, WifiEvent, WifiState};
    let log = log();
    let mut kernel = wifi_kernel(&log);
    assert!(kernel.shared.env(Instant(0)).caps().network);
    assert_eq!(
        kernel.shared.env(Instant(0)).wifi().unwrap().state,
        WifiState::Off
    );
    get(&mut kernel);
    // Held while offline; plugins hear it once, and ask for the radio.
    assert!(kernel.take_requests().is_empty());
    assert_eq!(log.borrow().as_slice(), ["core.network-wanted "]);
    assert_eq!(kernel.take_wifi_calls(), [WifiCommand::On]);
    assert!(kernel.shared.wifi.as_ref().unwrap().wanted);
    get(&mut kernel);
    assert_eq!(log.borrow().len(), 1);
    // Online: both go to the shell.
    let online = WifiState::Online {
        ssid: String::from("Home"),
        address: String::from("10.0.0.2"),
    };
    kernel.wifi_event(Instant(10), WifiEvent::State(online));
    let calls = kernel.take_requests();
    assert_eq!(calls.len(), 2);
    assert!(!kernel.shared.wifi.as_ref().unwrap().wanted);
    // Answered: nothing uses the network any more.
    for call in calls {
        kernel.http_response(
            Instant(20),
            crate::HttpResponse {
                id: call.id,
                status: 200,
                ..Default::default()
            },
        );
    }
    assert_eq!(
        log.borrow().as_slice(),
        [
            "core.network-wanted ",
            "core.wifi-changed ",
            "net.reply 200",
            "net.reply 200",
            "core.network-idle "
        ]
    );
}

#[test]
fn network_work_fails_when_no_network_comes() {
    let log = log();
    let mut kernel = wifi_kernel(&log);
    get(&mut kernel);
    let deadline = Instant(paperpaw_sdk::wifi::NETWORK_WAIT_MS);
    assert_eq!(kernel.finish().wake_at(), Some(deadline));
    kernel.tick(Instant(1000));
    assert_eq!(log.borrow().len(), 1);
    kernel.tick(deadline);
    // Failed as without a network; then nothing uses it (the radio asked
    // for may go off again).
    assert_eq!(
        log.borrow().as_slice(),
        ["core.network-wanted ", "net.reply 0", "core.network-idle "]
    );
    assert!(
        !kernel.shared.wifi.as_ref().unwrap().wanted,
        "the radio may go off"
    );
    assert!(kernel.take_requests().is_empty());
    assert_eq!(kernel.finish().wake_at(), None);
}

#[test]
fn scans_and_failures_reach_plugins() {
    use crate::{JoinFailure, WifiEvent, WifiNetwork};
    let log = log();
    let mut kernel = wifi_kernel(&log);
    let net = |ssid: &str, signal| WifiNetwork::new(String::from(ssid), signal, true);
    kernel.wifi_event(
        Instant(1),
        WifiEvent::Scanned(alloc::vec![
            net("Weak", -80),
            net("", -40),
            net("Home", -50),
            net("Weak", -60)
        ]),
    );
    // Strongest first, each name once, hidden ones left out.
    let wifi = kernel.shared.wifi.as_ref().unwrap();
    let names: Vec<_> = wifi
        .networks
        .iter()
        .map(|n| (n.ssid.as_str(), n.signal))
        .collect();
    assert_eq!(names, [("Home", -50), ("Weak", -60)]);
    kernel.wifi_event(
        Instant(2),
        WifiEvent::Failed {
            ssid: String::from("Home"),
            reason: JoinFailure::Password,
        },
    );
    assert_eq!(
        log.borrow().as_slice(),
        ["core.wifi-scanned ", "core.wifi-failed password"]
    );
}

#[test]
fn wifi_commands_are_ignored_where_the_system_manages_it() {
    let mut kernel = Kernel::builder(DEVICE).network(true).build().unwrap();
    kernel.start(Instant(0));
    assert!(kernel.shared.env(Instant(0)).wifi().is_none());
    kernel
        .output
        .commands
        .push(Command::Wifi(paperpaw_sdk::wifi::WifiCommand::On));
    kernel.finish();
    assert!(kernel.take_wifi_calls().is_empty());
    // Requests go to the shell at once.
    kernel.request(paperpaw_sdk::HttpRequest::new(
        "GET",
        "http://example.org/",
        Id::new_static("net.reply"),
    ));
    assert_eq!(kernel.take_requests().len(), 1);
}

/// `paperpaw.ini` as version 1 wrote it stays readable (see
/// `docs/data-files.md`).
#[test]
fn settings_files_of_version_1_are_read() {
    let storage = paperpaw_sdk::MemoryStorage::new()
        .with("paperpaw.ini", include_str!("../testdata/paperpaw.ini.v1"));
    let mut kernel = Kernel::builder(DEVICE).storage(storage).build().unwrap();
    assert_eq!(kernel.settings().int("core.ui-size"), 12);
    assert!(kernel.settings().bool("core.night-mode"));
    assert!(kernel.shared.menus.is_customized("core.main"));
    let main: Vec<&str> = kernel
        .shared
        .menus
        .ids("core.main")
        .into_iter()
        .map(Id::as_str)
        .collect();
    assert_eq!(main, ["core.toggle-night-mode", "core.back"]);
    assert!(kernel.take_warnings().is_empty());
}

/// A book's own settings (`settings.ini` in its folder) as version 1
/// wrote them stay readable (see `docs/data-files.md`).
#[test]
fn book_settings_of_version_1_are_read() {
    let path = ".paperpaw/plugins/reader/books/0000000000000001/settings.ini";
    let storage = paperpaw_sdk::MemoryStorage::new()
        .with(path, include_str!("../testdata/book-settings.ini.v1"));
    let mut kernel = Kernel::builder(DEVICE).storage(storage).build().unwrap();
    kernel.set_scope(Some(String::from(path)));
    let settings = &kernel.shared.settings;
    assert_eq!(
        settings.scoped_value(settings::SCOPE_ONLY.as_str()),
        Some(&SettingValue::Bool(true))
    );
    assert_eq!(
        settings.scoped_value(settings::READING_FONT.as_str()),
        Some(&SettingValue::Text(Cow::Borrowed(settings::DEFAULT_FONT)))
    );
}

/// The shell takes everything at once: the radio first, network work only
/// once there is a network.
#[test]
fn the_shell_takes_all_it_is_asked_at_once() {
    let log = log();
    let mut kernel = wifi_kernel(&log);
    kernel.request(paperpaw_sdk::HttpRequest::new(
        "GET",
        "http://example.org/",
        Id::new_static("net.reply"),
    ));
    kernel.finish();
    assert_eq!(
        kernel.take_io(),
        [Io::Wifi(paperpaw_sdk::wifi::WifiCommand::On)]
    );
    let online = paperpaw_sdk::wifi::WifiState::Online {
        ssid: String::from("Home"),
        address: String::from("10.0.0.2"),
    };
    kernel.wifi_event(Instant(10), crate::WifiEvent::State(online));
    let io = kernel.take_io();
    assert!(matches!(io.as_slice(), [Io::Http(call)] if call.url == "http://example.org/"));
    assert_eq!(kernel.take_io(), []);
}

#[test]
fn overlays_leave_the_rows_they_do_not_cover() {
    let uncovered = crate::uncovered;
    let part = Rect::new(0, 260, 480, 100);
    // A menu from 270 down: the ten rows above it.
    let menu = Rect::new(0, 270, 480, 530);
    assert_eq!(uncovered(part, menu), Some(Rect::new(0, 260, 480, 10)));
    // A bar at the top down to 300: the rows below it.
    let bar = Rect::new(0, 0, 480, 300);
    assert_eq!(uncovered(part, bar), Some(Rect::new(0, 300, 480, 60)));
    // All of it covered; none of it; a narrower one: all.
    assert_eq!(uncovered(part, Rect::new(0, 200, 480, 600)), None);
    assert_eq!(uncovered(part, Rect::new(0, 500, 480, 100)), Some(part));
    assert_eq!(uncovered(part, Rect::new(40, 200, 400, 600)), Some(part));
}

mod action_runs;
mod chrome;
mod clean_up;
mod declared_work;
mod developer;
mod frames;
mod guard;
mod hosting;
mod input;
mod input_kinds;
mod kernel_calls;
mod library_catalog;
mod pictures_in_grays;
mod plugins_loaded;
mod realtime;
mod refreshes;
mod screen_life;
mod services_declared;
mod settings_files;
mod stack;
mod storage;

/// A greeting, provided as a service in a test.
struct Greeting(&'static str);
paperpaw_sdk::service_type!(Greeting, "provider.greeting/1");
