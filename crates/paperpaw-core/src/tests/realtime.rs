//! Screens that run in real time (ADR 0030): raw keys and their way
//! out, the buttons held, full screen, refreshes the screen chooses, the
//! fastest refresh, the pace, staying awake, how frames went.

use core::time::Duration;

use super::*;
use crate::Items;
use paperpaw_sdk::Params;
use paperpaw_sdk::realtime::ScreenMode;

/// A plugin with a page on the home slot and a game screen of `mode`.
struct Game {
    log: Log,
    mode: ScreenMode,
}

fn game_items(kept: Game) -> impl Fn(&mut Items) -> Result<(), RegisterError> {
    move |r| {
        let log = kept.log.clone();
        r.add_fake(crate::testing::FakeSpec::new(
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
        let (log, mode) = (kept.log.clone(), kept.mode);
        r.add_fake(crate::testing::FakeSpec::new(
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

/// What [`game_items`] registers under.
fn game_manifest() -> Manifest {
    Manifest::new(Id::new_static("game"), "Game", Version::new(1, 0, 0))
        .uses(paperpaw_sdk::Uses::NOTHING)
}

/// The game: takes every key but *Menu*, says what it got and which
/// keys are held.
struct Play {
    log: Log,
    mode: ScreenMode,
}

impl crate::testing::Fake for Play {
    fn draw(&mut self, canvas: &mut Canvas<'_>, _: &Place<'_>) {
        canvas.fill_rect(0, 0, 10, 10, Color::BLACK);
    }

    fn mode(&self) -> Option<ScreenMode> {
        Some(self.mode)
    }

    fn on_action(&mut self, action: &Id, _: &mut Turn<'_, '_>) -> Handled {
        self.log
            .borrow_mut()
            .push(alloc::format!("action {action}"));
        Handled::Yes
    }

    fn on_key(&mut self, name: &str, pressed: bool, at: Instant, cx: &mut Turn<'_, '_>) -> Handled {
        if name == "menu" {
            return Handled::No;
        }
        let held: Vec<&str> = cx.env().keys_held().iter().copied().collect();
        let what = if pressed { "down" } else { "up" };
        self.log
            .borrow_mut()
            .push(alloc::format!("{what} {name} at {} held {held:?}", at.0));
        cx.request_redraw();
        Handled::Yes
    }
}

fn kernel(builder: crate::KernelBuilder, mode: ScreenMode, log: &Log) -> Kernel {
    let mut kernel = builder
        .items(
            game_manifest(),
            game_items(Game {
                log: log.clone(),
                mode,
            }),
        )
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
    let panel = kernel.shared.place(Instant(600)).panel();
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
        .items(
            game_manifest(),
            game_items(Game {
                log: log.clone(),
                mode: ScreenMode::new().raw_keys().memory(asked),
            }),
        )
        .test_plugin(TestPlugin::new("b", log))
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

#[test]
fn exhausted_keys_keep_slot_screen_for_new_screen_or_new_params() {
    let log = log();
    let mut kernel = kernel(
        Kernel::builder(DEVICE).test_plugin(TestPlugin::new("b", &log)),
        ScreenMode::new(),
        &log,
    );
    let before: Vec<u32> = kernel.stack.iter().map(|screen| screen.serial).collect();
    let other = if kernel.stack[0].id.as_str() == "b.screen" {
        "game.page"
    } else {
        "b.screen"
    };
    kernel.next_serial = u32::MAX;
    kernel.take_warnings();
    for (screen, params) in [
        (Some(Id::new_static(other)), Params::new()),
        (None, Params::new().with("book", "new")),
    ] {
        kernel.open_slot_command(slots::HOME, screen, params);
        assert_eq!(
            kernel
                .stack
                .iter()
                .map(|screen| screen.serial)
                .collect::<Vec<_>>(),
            before
        );
        assert_eq!(kernel.next_serial, u32::MAX);
        assert!(
            log.borrow().is_empty(),
            "no close callbacks on refused slot replacement"
        );
    }
    assert_eq!(kernel.take_warnings().len(), 2);
    // Returning to the existing slot needs no fresh key and remains allowed.
    kernel.open_slot_command(slots::HOME, None, Params::new());
    assert_eq!(kernel.stack.len(), 1);
    assert_eq!(kernel.stack[0].serial, before[0]);
    assert_eq!(kernel.next_serial, u32::MAX);
}

#[test]
fn key_exhaustion_preserves_replace_and_saved_task_state() {
    let log = log();
    let mut kernel = kernel(
        Kernel::builder(DEVICE).test_plugin(TestPlugin::new("b", &log)),
        ScreenMode::new(),
        &log,
    );
    let original: Vec<u32> = kernel.stack.iter().map(|screen| screen.serial).collect();
    kernel.next_serial = u32::MAX;
    kernel.replace(Id::new_static("b.screen"), &Params::new());
    assert_eq!(
        kernel
            .stack
            .iter()
            .map(|screen| screen.serial)
            .collect::<Vec<_>>(),
        original
    );
    assert_eq!(kernel.next_serial, u32::MAX);
    assert!(
        log.borrow().is_empty(),
        "no close or make callback on refusal"
    );

    // Save a two-screen task, then leave only one key for its restoration.
    kernel.next_serial = 20;
    kernel.switch_task(Id::new_static("b.screen"));
    let shown = kernel.stack[0].serial;
    assert_eq!(kernel.tasks[0].1.len(), 2);
    kernel.next_serial = u32::MAX - 1;
    log.borrow_mut().clear();
    kernel.switch_task(slots::HOME);
    assert_eq!(kernel.task, Some(Id::new_static("b.screen")));
    assert_eq!(kernel.stack.len(), 1);
    assert_eq!(kernel.stack[0].serial, shown);
    assert_eq!(
        kernel.tasks[0].1.len(),
        2,
        "saved task is not removed on refusal"
    );
    assert_eq!(kernel.next_serial, u32::MAX - 1);
    assert!(log.borrow().is_empty());
}

#[test]
fn room_closures_reserve_restore_keys_and_refuse_when_none_remain() {
    for available in [1, 2] {
        let log = log();
        let mut kernel = Kernel::builder(DEVICE)
            .memory(|| Some(paperpaw_sdk::memory::MemoryUse::new(1000, 256 * 1024)))
            .items(
                game_manifest(),
                game_items(Game {
                    log: log.clone(),
                    mode: ScreenMode::new().memory(64_000),
                }),
            )
            .test_plugin(TestPlugin::new("b", &log))
            .build()
            .unwrap();
        kernel.start(Instant(0));
        kernel.open_screen(
            Instant(5),
            Id::new_static("b.screen"),
            Params::new().with("n", "2"),
        );
        let old = kernel.stack.last().unwrap().serial;
        kernel.next_serial = u32::MAX - available;
        kernel.open_screen(Instant(10), Id::new_static("game.play"), Params::new());
        render(&mut kernel);
        assert_eq!(
            kernel.room_screen_keys_reserved(),
            usize::from(available == 2)
        );
        assert_eq!(kernel.screen_keys_remaining(), 0);
        let shown = kernel.stack.last().unwrap().serial;
        let count = kernel.stack.len();
        kernel.open(Id::new_static("b.screen"), None, &Params::new());
        assert_eq!(
            kernel.stack.len(),
            count,
            "a normal open cannot spend a reserved key"
        );
        assert_eq!(kernel.stack.last().unwrap().serial, shown);
        kernel.close_screen(paperpaw_sdk::ScreenKey::new(shown));
        kernel.sync_room();
        assert!(kernel.lease.is_none());
        assert_eq!(kernel.stack.len(), 2);
        assert_eq!(kernel.active_screen().map(Id::as_str), Some("b.screen"));
        let restored = kernel.stack.last().unwrap().serial;
        assert_eq!(restored, if available == 2 { u32::MAX - 1 } else { old });
        assert_eq!(kernel.next_serial, u32::MAX);
        assert!(!kernel.works.is_held());
    }
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
    // Closed: the screen between opens again, work goes on.
    kernel.apply(Command::Back);
    kernel.finish();
    render(&mut kernel);
    assert_eq!(kernel.active_screen().map(Id::as_str), Some("b.screen"));
    assert_eq!(kernel.stack.len(), 2);
    assert!(!kernel.works.is_held());
    assert_eq!(kernel.shared.room, paperpaw_sdk::realtime::Room::default());
    assert_eq!(kernel.framebuffer.rows(), 30, "bands as they were");
}

/// A screen that holds the device awake for five minutes when *up* is
/// pressed, and lets go when *down* is.
fn holder_items() -> impl Fn(&mut Items) -> Result<(), RegisterError> {
    |r| {
        r.add_fake(crate::testing::FakeSpec::new(
            Id::new_static("holder.screen"),
            "holder",
            Box::new(|_, _| Box::new(HolderScreen)),
        ))
    }
}

/// What [`holder_items`] registers under.
fn holder_manifest() -> Manifest {
    Manifest::new(Id::new_static("holder"), "Holder", Version::new(1, 0, 0))
        .uses(paperpaw_sdk::Uses::NOTHING)
}

struct HolderScreen;

impl crate::testing::Fake for HolderScreen {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Place<'_>) {}

    fn mode(&self) -> Option<ScreenMode> {
        Some(ScreenMode::new().raw_keys())
    }

    fn on_action(&mut self, _: &Id, _: &mut Turn<'_, '_>) -> Handled {
        Handled::No
    }

    fn on_key(&mut self, name: &str, pressed: bool, _: Instant, cx: &mut Turn<'_, '_>) -> Handled {
        match (name, pressed) {
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
        .items(holder_manifest(), holder_items())
        .test_plugin(TestPlugin::new("a", &log))
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
