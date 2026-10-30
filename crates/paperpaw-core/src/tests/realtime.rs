//! Screens that run in real time (ADR 0030): raw keys and their way
//! out, the buttons held, full screen, refreshes the screen chooses, the
//! fastest refresh, the pace, staying awake, how frames went.

use core::time::Duration;

use super::*;
use paperpaw_sdk::Params;
use paperpaw_sdk::realtime::{KeyEvent, ScreenMode};

/// A plugin with a page on the home slot and a game screen of `mode`.
struct Game {
    log: Log,
    mode: ScreenMode,
}

impl Plugin for Game {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("game"), "Game", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let log = self.log.clone();
        r.add_screen(ScreenSpec::new(
            Id::new_static("game.page"),
            "page",
            Box::new(move |_, _| {
                Box::new(TestScreen {
                    name: "page",
                    log: log.clone(),
                })
            }),
        ))?;
        r.bind_slot(slots::HOME, Id::new_static("game.page"), Priority::DEFAULT)?;
        let (log, mode) = (self.log.clone(), self.mode);
        r.add_screen(ScreenSpec::new(
            Id::new_static("game.play"),
            "play",
            Box::new(move |env, _| {
                log.borrow_mut()
                    .push(alloc::format!("made {}", env.content().height));
                Box::new(Play {
                    log: log.clone(),
                    mode,
                })
            }),
        ))
    }
}

/// The game: takes every key but *Menu*, says what it got and which
/// keys are held.
struct Play {
    log: Log,
    mode: ScreenMode,
}

impl Screen for Play {
    fn draw(&mut self, canvas: &mut Canvas<'_>, _: &Env<'_>) {
        canvas.fill_rect(0, 0, 10, 10, Color::BLACK);
    }

    fn mode(&self) -> ScreenMode {
        self.mode
    }

    fn on_action(&mut self, action: &Id, _: &mut Cx<'_, '_>) -> Handled {
        self.log
            .borrow_mut()
            .push(alloc::format!("action {action}"));
        Handled::Yes
    }

    fn on_key(&mut self, key: &KeyEvent, cx: &mut Cx<'_, '_>) -> Handled {
        if key.name() == "menu" {
            return Handled::No;
        }
        let held: Vec<&str> = cx.env().keys_held().iter().collect();
        let what = if key.is_press() { "down" } else { "up" };
        self.log.borrow_mut().push(alloc::format!(
            "{what} {} at {} held {held:?}",
            key.name(),
            key.at().0
        ));
        cx.request_redraw();
        Handled::Yes
    }
}

fn kernel(builder: crate::KernelBuilder, mode: ScreenMode, log: &Log) -> Kernel {
    let mut kernel = builder
        .plugin(Game {
            log: log.clone(),
            mode,
        })
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel.open_screen(Instant(10), Id::new_static("game.play"), Params::new());
    render(&mut kernel);
    log.borrow_mut().clear();
    kernel
}

fn button(kernel: &mut Kernel, at: u64, name: &'static str, state: ButtonState) -> crate::Effects {
    let effects = kernel.handle(Instant(at), InputEvent::Button { name, state });
    render(kernel);
    effects
}

/// Presses and releases come as they happen, with their times and the
/// buttons held; one the screen does not take goes to the key map; the
/// power button never comes.
#[test]
fn raw_keys_come_pressed_and_released() {
    let log = log();
    let mode = ScreenMode::new().raw_keys();
    let mut kernel = kernel(Kernel::builder(DEVICE), mode, &log);
    button(&mut kernel, 100, "left", ButtonState::Pressed);
    button(&mut kernel, 120, "up", ButtonState::Pressed);
    button(&mut kernel, 130, "left", ButtonState::Released);
    button(&mut kernel, 180, "up", ButtonState::Released);
    assert_eq!(
        *log.borrow(),
        [
            "down left at 100 held [\"left\"]",
            "down up at 120 held [\"left\", \"up\"]",
            "up left at 130 held [\"up\"]",
            "up up at 180 held []",
        ]
    );
    log.borrow_mut().clear();
    press(&mut kernel, 300, "menu");
    press(&mut kernel, 500, "power");
    assert!(
        log.borrow()
            .iter()
            .all(|l| !l.starts_with("down") && !l.starts_with("up")),
        "{:?}",
        log.borrow()
    );
}

/// Without raw keys, the same screen gets actions as before.
#[test]
fn without_raw_keys_buttons_are_actions() {
    let log = log();
    let mut kernel = kernel(Kernel::builder(DEVICE), ScreenMode::new(), &log);
    press(&mut kernel, 100, "confirm");
    assert_eq!(*log.borrow(), ["action core.confirm"]);
}

/// *Back* held for two seconds leaves the screen, whatever it does with
/// *Back* otherwise; its release is told first.
#[test]
fn back_held_leaves_a_screen_of_raw_keys() {
    let log = log();
    let mode = ScreenMode::new().raw_keys();
    let mut kernel = kernel(Kernel::builder(DEVICE), mode, &log);
    let effects = button(&mut kernel, 1000, "back", ButtonState::Pressed);
    let due = effects.wake_at().unwrap();
    assert_eq!(due, Instant(3000));
    kernel.tick(Instant(2500));
    assert_eq!(kernel.active_screen().map(Id::as_str), Some("game.play"));
    kernel.tick(due);
    render(&mut kernel);
    assert_eq!(kernel.active_screen().map(Id::as_str), Some("game.page"));
    assert_eq!(
        log.borrow().as_slice(),
        [
            "down back at 1000 held [\"back\"]",
            "up back at 3000 held [\"back\"]"
        ]
    );
    // Let go later, on the page: nothing more for the game.
    button(&mut kernel, 3200, "back", ButtonState::Released);
    assert_eq!(log.borrow().len(), 2);
}

/// A full screen has all of the panel: no row of hints below it.
#[test]
fn a_full_screen_has_no_row_of_hints() {
    let builder = || Kernel::builder(DEVICE).button_row(&["back", "confirm", "left", "right"]);
    let log = log();
    let mut normal = kernel(builder(), ScreenMode::new(), &log);
    assert!(normal.hint.height > 0);
    assert!(normal.shared.content.height < 90);
    drop(normal.stack.pop());
    let full = kernel(builder(), ScreenMode::new().full_screen(), &log);
    assert_eq!(full.shared.content.height, 90);
}

/// A screen that cleans itself gets no full refresh every few frames; the
/// fastest refresh is black and white where that is enough; the pace is
/// told to the shell.
#[test]
fn a_screen_chooses_its_refreshes_and_pace() {
    let config = "[core]\nfull-refresh-interval = 2\n";
    let modes = |mode: ScreenMode| {
        let log = log();
        let builder = Kernel::builder(DEVICE).user_config(config);
        let mut kernel = kernel(builder, mode, &log);
        (1..=3)
            .flat_map(|n| {
                let down = button(&mut kernel, n * 100, "left", ButtonState::Pressed);
                let up = button(&mut kernel, n * 100 + 50, "left", ButtonState::Released);
                [down, up]
            })
            .map(|effects| (effects.refresh().map(|(_, m)| m), effects.pace()))
            .collect::<Vec<_>>()
    };
    let paced = ScreenMode::new()
        .raw_keys()
        .cleans_itself()
        .fastest()
        .grays(2)
        .frame_every(Duration::from_millis(50));
    let frames = modes(paced);
    assert!(
        frames
            .iter()
            .all(|f| *f == (Some(RefreshMode::Mono), Some(Duration::from_millis(50)))),
        "{frames:?}"
    );
    let frames = modes(ScreenMode::new().raw_keys());
    assert!(
        frames.iter().any(|(m, _)| *m == Some(RefreshMode::Full)),
        "a full refresh every two: {frames:?}"
    );
}

/// The device stays awake longer while a screen asks for it.
#[test]
fn a_screen_keeps_the_device_awake_longer() {
    let log = log();
    let config = "[core]\nsleep-after = 1\n";
    let builder = || Kernel::builder(DEVICE).user_config(config).sleep(true);
    let mut normal = kernel(builder(), ScreenMode::new(), &log);
    let due = normal.sleep_due().unwrap();
    let mode = ScreenMode::new().awake_for(Duration::from_secs(600));
    let mut longer = kernel(builder(), mode, &log);
    let later = longer.sleep_due().unwrap();
    assert_eq!(later.since(due), 540_000);
    drop((normal.stack.pop(), longer.stack.pop()));
}

/// What the shell says of a frame shown reaches plugins: its time counts
/// for the mode, merged frames are counted.
#[test]
fn frames_shown_are_told_to_plugins() {
    let log = log();
    let mode = ScreenMode::new().raw_keys();
    let mut kernel = kernel(Kernel::builder(DEVICE), mode, &log);
    assert!(kernel.shared.panel.last_frame().is_none());
    let area = Rect::new(0, 0, 10, 10);
    // The frame of its opening, shown.
    kernel.frame_shown(
        area,
        RefreshMode::Full,
        Duration::from_millis(5),
        Duration::from_millis(1600),
        Instant(50),
    );
    button(&mut kernel, 100, "left", ButtonState::Pressed);
    button(&mut kernel, 110, "left", ButtonState::Released);
    kernel.frame_shown(
        area,
        RefreshMode::Fast,
        Duration::from_millis(3),
        Duration::from_millis(400),
        Instant(520),
    );
    let panel = kernel.shared.env(Instant(600)).panel();
    let frame = panel.last_frame().unwrap();
    assert_eq!(frame.area(), area);
    assert_eq!(frame.mode(), Some(RefreshMode::Fast));
    assert_eq!(frame.merged(), 1, "two frames asked, one shown");
    assert_eq!(
        panel.refresh_time(RefreshMode::Fast),
        Some(Duration::from_millis(400))
    );
}

/// The memory the test's probe says is free.
static FREE: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

fn probe() -> Option<paperpaw_sdk::memory::MemoryUse> {
    let free = FREE.load(core::sync::atomic::Ordering::Relaxed);
    Some(paperpaw_sdk::memory::MemoryUse::new(free, 256 * 1024))
}

/// A kernel with the page at the bottom, a screen above it, and the game
/// asking for `asked` bytes on top, `free` free.
fn leased(asked: u32, free: u32, log: &Log) -> Kernel {
    FREE.store(free, core::sync::atomic::Ordering::Relaxed);
    let mut kernel = Kernel::builder(DEVICE)
        .memory(probe)
        .frame_rows(30)
        .plugin(Game {
            log: log.clone(),
            mode: ScreenMode::new().raw_keys().memory(asked),
        })
        .plugin(TestPlugin::new("b", log))
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel.open_screen(
        Instant(5),
        Id::new_static("b.screen"),
        Params::new().with("n", "2"),
    );
    kernel.open_screen(Instant(10), Id::new_static("game.play"), Params::new());
    render(&mut kernel);
    kernel
}

/// Room made, and given back, in one test: the probe's number is shared.
#[test]
fn room_is_made_only_where_needed_and_given_back() {
    let log = log();
    // Room enough: nothing is done for it.
    let kernel = leased(4000, 10_000, &log);
    let room = kernel.shared.room;
    assert!(room.is_granted() && room.steps().is_empty(), "{room:?}");
    assert_eq!(room.free(), Some(10_000));
    assert_eq!(kernel.stack.len(), 3, "the screens below stay");
    // Not enough: caches, work, the screens below.
    let mut kernel = leased(64_000, 1000, &log);
    let room = kernel.shared.room;
    assert!(room.is_partial(), "{room:?}");
    assert!(
        room.steps()
            .contains(paperpaw_sdk::realtime::RoomSteps::CACHES)
    );
    assert!(
        room.steps()
            .contains(paperpaw_sdk::realtime::RoomSteps::WORK)
    );
    assert!(
        room.steps()
            .contains(paperpaw_sdk::realtime::RoomSteps::SCREENS)
    );
    assert!(
        room.steps()
            .contains(paperpaw_sdk::realtime::RoomSteps::BANDS)
    );
    assert_eq!(kernel.framebuffer.rows(), 15, "bands of half the rows");
    assert_eq!(kernel.stack.len(), 2, "the screen between closed");
    assert!(kernel.works.is_held());
    // Closed by Back held: the screen between opens again, work goes on.
    button(&mut kernel, 1000, "back", ButtonState::Pressed);
    kernel.tick(Instant(3000));
    render(&mut kernel);
    assert_eq!(kernel.active_screen().map(Id::as_str), Some("b.screen"));
    assert_eq!(kernel.stack.len(), 2);
    assert!(!kernel.works.is_held());
    assert_eq!(kernel.shared.room, paperpaw_sdk::realtime::Room::default());
    assert_eq!(kernel.framebuffer.rows(), 30, "bands as they were");
}

/// The memory free for the squeeze test: what the screen below lets go
/// of comes free.
static SQUEEZE_FREE: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

fn squeeze_probe() -> Option<paperpaw_sdk::memory::MemoryUse> {
    let free = SQUEEZE_FREE.load(core::sync::atomic::Ordering::Relaxed);
    Some(paperpaw_sdk::memory::MemoryUse::new(free, 256 * 1024))
}

/// A screen that lets go of 60 KB when squeezed, and makes it again as it
/// shows.
struct Squeezer(Log);

impl Plugin for Squeezer {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("sq"), "Squeezer", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let log = self.0.clone();
        r.add_screen(ScreenSpec::new(
            Id::new_static("sq.screen"),
            "squeezer",
            Box::new(move |_, _| Box::new(SqueezerScreen(log.clone()))),
        ))
    }
}

struct SqueezerScreen(Log);

impl Screen for SqueezerScreen {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }

    fn on_squeeze(
        &mut self,
        squeeze: &paperpaw_sdk::realtime::Squeeze,
        _: &mut Cx<'_, '_>,
    ) -> Handled {
        SQUEEZE_FREE.fetch_add(60_000, core::sync::atomic::Ordering::Relaxed);
        let line = alloc::format!("squeezed for {}", squeeze.asked());
        self.0.borrow_mut().push(line);
        Handled::Yes
    }

    fn on_show(&mut self, _: &mut Cx<'_, '_>) {
        self.0.borrow_mut().push(String::from("shown"));
    }
}

/// The screens below a screen that needs memory let go of what they make
/// again: where that is enough, none is closed, and each makes it again
/// as it shows.
#[test]
fn screens_below_are_squeezed_before_they_close() {
    let log = log();
    SQUEEZE_FREE.store(1000, core::sync::atomic::Ordering::Relaxed);
    let mut kernel = Kernel::builder(DEVICE)
        .memory(squeeze_probe)
        .plugin(Game {
            log: log.clone(),
            mode: ScreenMode::new().raw_keys().memory(48_000),
        })
        .plugin(Squeezer(log.clone()))
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel.open_screen(Instant(5), Id::new_static("sq.screen"), Params::new());
    log.borrow_mut().clear();
    kernel.open_screen(Instant(10), Id::new_static("game.play"), Params::new());
    render(&mut kernel);
    let room = kernel.shared.room;
    assert!(room.is_granted(), "{room:?}");
    let steps = room.steps();
    assert!(steps.contains(paperpaw_sdk::realtime::RoomSteps::SQUEEZED));
    assert!(!steps.contains(paperpaw_sdk::realtime::RoomSteps::SCREENS));
    assert_eq!(kernel.stack.len(), 3, "the screens below stay");
    assert!(log.borrow().iter().any(|l| l == "squeezed for 48000"));
    log.borrow_mut().clear();
    button(&mut kernel, 1000, "back", ButtonState::Pressed);
    kernel.tick(Instant(3000));
    render(&mut kernel);
    assert_eq!(kernel.active_screen().map(Id::as_str), Some("sq.screen"));
    assert!(log.borrow().iter().any(|l| l == "shown"));
}

/// A screen that holds the device awake for five minutes when *up* is
/// pressed, and lets go when *down* is.
struct Holder;

impl Plugin for Holder {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("holder"), "Holder", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_screen(ScreenSpec::new(
            Id::new_static("holder.screen"),
            "holder",
            Box::new(|_, _| Box::new(HolderScreen)),
        ))
    }
}

struct HolderScreen;

impl Screen for HolderScreen {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}

    fn mode(&self) -> ScreenMode {
        ScreenMode::new().raw_keys()
    }

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }

    fn on_key(&mut self, key: &KeyEvent, cx: &mut Cx<'_, '_>) -> Handled {
        match (key.name(), key.is_press()) {
            ("up", true) => cx.keep_awake(
                paperpaw_sdk::power::WakeHold::new(Id::new_static("holder.clock"), "clock")
                    .lasting(Duration::from_secs(300)),
            ),
            ("down", true) => cx.let_sleep(Id::new_static("holder.clock")),
            _ => {}
        }
        Handled::Yes
    }
}

fn holding(config: &'static str) -> Kernel {
    let log = log();
    let mut kernel = Kernel::builder(DEVICE)
        .user_config(config)
        .sleep(true)
        .plugin(Holder)
        .plugin(TestPlugin::new("a", &log))
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel.open_screen(Instant(0), Id::new_static("holder.screen"), Params::new());
    button(&mut kernel, 0, "up", ButtonState::Pressed);
    button(&mut kernel, 0, "up", ButtonState::Released);
    kernel
}

/// A hold puts the sleep off until it ends; let go, closed with its
/// screen, ignored with the setting off or a battery nearly empty.
#[test]
fn holds_put_sleep_off_within_bounds() {
    let config = "[core]\nsleep-after = 1\n";
    let mut kernel = holding(config);
    assert_eq!(
        kernel.sleep_due(),
        Some(Instant(300_000)),
        "held five minutes"
    );
    // Its time is counted for its plugin.
    kernel.tick(Instant(30_000));
    let table = kernel.shared.meter.table.borrow();
    let holder = table
        .iter()
        .find(|u| u.plugin.as_str() == "holder")
        .unwrap();
    assert_eq!(holder.awake_ms, 30_000);
    assert_eq!(holder.awake_reason, "clock");
    drop(table);
    // Let go.
    button(&mut kernel, 31_000, "down", ButtonState::Pressed);
    assert_eq!(kernel.sleep_due(), Some(Instant(91_000)));
    // Ended with its screen.
    let mut kernel = holding(config);
    let key = paperpaw_sdk::ScreenKey::new(kernel.stack.last().unwrap().serial);
    kernel.close_screen(key);
    kernel.count_awake();
    assert_eq!(kernel.sleep_due(), Some(Instant(60_000)));
    // Not with the setting off.
    let kernel = holding("[core]\nsleep-after = 1\nawake-holds = off\n");
    assert_eq!(kernel.sleep_due(), Some(Instant(60_000)));
    // Nor with the battery nearly empty.
    let mut kernel = holding(config);
    let status = paperpaw_platform::SystemStatus::default()
        .with_battery(Some(paperpaw_platform::Battery::new(8, false, None)));
    kernel.set_status(Instant(1000), status);
    assert_eq!(kernel.sleep_due(), Some(Instant(60_000)));
}

/// On a touch screen, a finger held two seconds in the top left corner
/// leaves a full screen, as Back held does; one moved out of it does not.
#[test]
fn a_finger_held_in_the_corner_leaves_a_full_screen() {
    let log = log();
    let mode = ScreenMode::new().full_screen();
    let mut kernel = kernel(Kernel::builder(DEVICE), mode, &log);
    let finger = |kernel: &mut Kernel, at: u64, phase: PointerPhase, x: i32, y: i32| {
        let event = paperpaw_platform::PointerEvent::new(
            0,
            phase,
            paperpaw_platform::Point::new(x, y),
            paperpaw_platform::PointerTool::Finger,
            1000,
        );
        kernel.handle(Instant(at), InputEvent::Pointer(event))
    };
    // Moved out of the corner: nothing.
    finger(&mut kernel, 100, PointerPhase::Down, 3, 3);
    finger(&mut kernel, 200, PointerPhase::Move, 60, 60);
    kernel.tick(Instant(2500));
    assert_eq!(kernel.active_screen().map(Id::as_str), Some("game.play"));
    finger(&mut kernel, 2600, PointerPhase::Up, 60, 60);
    // Held there: it leaves.
    let effects = finger(&mut kernel, 3000, PointerPhase::Down, 4, 5);
    assert!(effects.wake_at().is_some_and(|at| at <= Instant(5000)));
    assert_eq!(kernel.escape_due(), Some(Instant(5000)));
    kernel.tick(Instant(5000));
    render(&mut kernel);
    assert_eq!(kernel.active_screen().map(Id::as_str), Some("game.page"));
}
