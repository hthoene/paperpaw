//! Bars along the edges of the screen: what they leave to
//! the screens, where things are drawn, where taps go, when they show.

use super::*;
use core::mem;
use paperpaw_sdk::chrome::{ChromeSpec, Edge};
use paperpaw_sdk::{Direction, Gesture, Params, StackEntry};

const PAGE: Id = Id::new_static("bar.page");
const BOOK: Id = Id::new_static("bar.book");
const MENU: Id = Id::new_static("bar.menu");
const LOCK: Id = Id::new_static("bar.lock");
const SIZE: u16 = 20;

/// A screen black in its top left corner (10 by 10) that logs the size
/// it was made for and the taps it gets.
struct Corner {
    name: &'static str,
    log: Log,
}

impl Screen for Corner {
    fn draw(&mut self, canvas: &mut Canvas<'_>, _: &Env<'_>) {
        canvas.fill_rect(0, 0, 10, 10, Color::BLACK);
    }

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }

    fn on_gesture(&mut self, gesture: Gesture, _: &mut Cx<'_, '_>) -> Handled {
        let (x, y) = (gesture.start.x, gesture.start.y);
        let line = alloc::format!("{} {:?} at {x},{y}", self.name, gesture.kind);
        self.log.borrow_mut().push(line);
        Handled::Yes
    }
}

/// A bar: black in its corner, it logs the taps it gets and its life,
/// and asks for its timer once shown.
struct Bar {
    log: Log,
    timer_asked: bool,
}

impl Screen for Bar {
    fn draw(&mut self, canvas: &mut Canvas<'_>, _: &Env<'_>) {
        canvas.fill_rect(0, 0, 10, 10, Color::BLACK);
    }

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }

    fn on_gesture(&mut self, gesture: Gesture, _: &mut Cx<'_, '_>) -> Handled {
        let (x, y) = (gesture.start.x, gesture.start.y);
        let line = alloc::format!("bar {:?} at {x},{y}", gesture.kind);
        self.log.borrow_mut().push(line);
        Handled::Yes
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        self.log.borrow_mut().push(String::from("bar shown"));
        if !mem::replace(&mut self.timer_asked, true) {
            let at = cx.env().now().add_millis(100);
            cx.wake_at(at);
        }
    }

    fn on_timer(&mut self, _: &mut Cx<'_, '_>) {
        self.log.borrow_mut().push(String::from("bar timer"));
    }

    fn on_close(&mut self, _: &mut Cx<'_, '_>) {
        self.log.borrow_mut().push(String::from("bar closed"));
    }
}

fn bar(log: &Log) -> paperpaw_sdk::ScreenFactory {
    let log = log.clone();
    Box::new(move |env, _| {
        let display = env.device().display;
        let line = alloc::format!("bar made {}x{}", display.width, display.height);
        log.borrow_mut().push(line);
        Box::new(Bar {
            log: log.clone(),
            timer_asked: false,
        })
    })
}

/// A panel over the lower part of the screen it is opened on (20 high,
/// 20 above its bottom): an overlay.
struct Panel;

impl Screen for Panel {
    fn draw(&mut self, canvas: &mut Canvas<'_>, _: &Env<'_>) {
        canvas.fill_rect(0, 0, 400, 400, Color::BLACK);
    }

    fn region(&self, env: &Env<'_>) -> Option<Rect> {
        let height = env.device().display.height;
        Some(Rect::new(0, height - 40, env.device().display.width, 20))
    }

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }

    fn on_gesture(&mut self, _: Gesture, _: &mut Cx<'_, '_>) -> Handled {
        Handled::Yes
    }
}

/// A lock: it guards the device; black in its corner, it logs its taps.
struct Lock(Corner);

impl Screen for Lock {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        self.0.draw(canvas, env);
    }

    fn guards(&self) -> bool {
        true
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        // It asks to switch tasks: not past itself.
        if *action == actions::NEXT_PAGE {
            cx.command(Command::SwitchTask(slots::READER));
            return Handled::Yes;
        }
        Handled::No
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        self.0.on_gesture(gesture, cx)
    }
}

fn corner(name: &'static str, log: &Log) -> paperpaw_sdk::ScreenFactory {
    let log = log.clone();
    Box::new(move |env, _| {
        let display = env.device().display;
        let line = alloc::format!("{name} made {}x{}", display.width, display.height);
        log.borrow_mut().push(line);
        Box::new(Corner {
            name,
            log: log.clone(),
        })
    })
}

/// A bar along `edge`, not over the book, for touch screens.
struct BarPlugin(Log, Edge);

impl Plugin for BarPlugin {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("bar"), "Bar", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_screen(ScreenSpec::new(PAGE, "page", corner("page", &self.0)))?;
        r.add_screen(ScreenSpec::new(BOOK, "book", corner("book", &self.0)))?;
        r.add_screen(ScreenSpec::new(
            MENU,
            "menu",
            Box::new(|_, _| Box::new(Panel)),
        ))?;
        let log = self.0.clone();
        r.add_screen(ScreenSpec::new(
            LOCK,
            "lock",
            Box::new(move |env, _| {
                let display = env.device().display;
                let line = alloc::format!("lock made {}x{}", display.width, display.height);
                log.borrow_mut().push(line);
                Box::new(Lock(Corner {
                    name: "lock",
                    log: log.clone(),
                }))
            }),
        ))?;
        r.bind_slot(slots::HOME, PAGE, Priority::DEFAULT)?;
        r.bind_slot(slots::READER, BOOK, Priority::DEFAULT)?;
        r.add_setting(SettingSpec::new(
            Id::new_static("bar.size"),
            "",
            SettingKind::Int { min: 0, max: 40 },
            SettingValue::Int(i64::from(SIZE)),
            paperpaw_sdk::builtin::groups::OTHER,
        ))?;
        // Not over books, nor over the menu (which shares the bars of the
        // screen it is opened on all the same).
        let shown = |_: &Env<'_>, entry: &StackEntry| entry.screen != BOOK && entry.screen != MENU;
        r.add_chrome(
            ChromeSpec::new(Id::new_static("bar.tabs"), self.1, bar(&self.0))
                .size(|env| env.settings().int("bar.size") as u16)
                .shown(shown)
                .need(paperpaw_sdk::MenuNeed::Touch),
        )
    }
}

fn bar_kernel(log: &Log, edge: Edge, device: DeviceInfo) -> Kernel {
    let mut kernel = Kernel::builder(device)
        .plugin(BarPlugin(log.clone(), edge))
        .build()
        .unwrap();
    kernel.start(Instant(0));
    render(&mut kernel);
    kernel
}

/// Along the bottom: the screen gets the rest, the bar its strip; each
/// draws from its own corner and gets the taps in its part, in its own
/// coordinates.
#[test]
fn a_bar_at_the_bottom_leaves_the_rest_to_the_screen() {
    let log = log();
    let mut kernel = bar_kernel(&log, Edge::Bottom, DEVICE);
    assert_eq!(
        *log.borrow(),
        ["page made 90x70", "bar made 90x20", "bar shown"]
    );
    assert_eq!(
        kernel.shared.env(Instant(0)).content(),
        Rect::new(0, 0, 90, 70)
    );
    assert_eq!(
        kernel.frame().pixel(5, 5),
        Color::BLACK,
        "the page's corner"
    );
    assert_eq!(
        kernel.frame().pixel(5, 75),
        Color::BLACK,
        "the bar's corner"
    );
    assert_eq!(kernel.frame().pixel(5, 65), Color::WHITE);
    log.borrow_mut().clear();
    tap(&mut kernel, 100, 30, 80);
    tap(&mut kernel, 200, 30, 40);
    assert_eq!(*log.borrow(), ["bar Tap at 30,10", "page Tap at 30,40"]);
}

/// Along the top: the screen is drawn below it, from its own corner, and
/// a tap in it is in its coordinates.
#[test]
fn a_bar_at_the_top_moves_the_screen_below_it() {
    let log = log();
    let mut kernel = bar_kernel(&log, Edge::Top, DEVICE);
    assert_eq!(
        kernel.frame().pixel(5, 25),
        Color::BLACK,
        "the page's corner"
    );
    assert_eq!(kernel.frame().pixel(15, 25), Color::WHITE);
    log.borrow_mut().clear();
    tap(&mut kernel, 100, 30, 50);
    assert_eq!(*log.borrow(), ["page Tap at 30,30"]);
}

/// A bar shows over the screens it says: not over the book, which gets
/// all of the screen; back on the page, it shows again.
#[test]
fn a_bar_shows_over_the_screens_it_says() {
    let log = log();
    let mut kernel = bar_kernel(&log, Edge::Left, DEVICE);
    log.borrow_mut().clear();
    kernel.open_slot(
        Instant(10),
        slots::READER,
        Params::new().with("book", "a.txt"),
    );
    render(&mut kernel);
    assert_eq!(*log.borrow(), ["book made 90x90", "bar closed"]);
    assert!(kernel.chrome_strips.is_empty());
    assert!(kernel.chromes[0].screen.is_none(), "let go");
    kernel.run_action(Instant(20), actions::BACK);
    render(&mut kernel);
    assert_eq!(kernel.chrome_strips, [(0, Rect::new(0, 0, SIZE, 90))]);
    assert_eq!(
        kernel.frame().pixel(25, 5),
        Color::BLACK,
        "the page beside it"
    );
}

/// A bar for touch screens is not there on a device without one.
#[test]
fn a_bar_for_touch_needs_a_touch_screen() {
    let log = log();
    let buttons = DEVICE.with_input(DEVICE.input.with_touch(false));
    let kernel = bar_kernel(&log, Edge::Bottom, buttons);
    assert!(kernel.chromes.is_empty());
    assert_eq!(*log.borrow(), ["page made 90x90"]);
}

/// A screen that guards the device has no bars, and its part of the
/// screen is all of it: drawn from the top, it gets taps down to the
/// bottom. Unlocked, the bar shows again; the page keeps its part.
#[test]
fn a_guard_has_no_bars() {
    let log = log();
    let mut kernel = bar_kernel(&log, Edge::Top, DEVICE);
    log.borrow_mut().clear();
    kernel.open_screen(Instant(10), LOCK, Params::new());
    render(&mut kernel);
    // Made once more for all of the screen, as it says it guards once made.
    assert_eq!(
        *log.borrow(),
        ["lock made 90x70", "lock made 90x90", "bar closed"]
    );
    assert!(kernel.chrome_strips.is_empty());
    assert_eq!(
        kernel.frame().pixel(5, 5),
        Color::BLACK,
        "the lock's corner"
    );
    assert_eq!(kernel.frame().pixel(5, 25), Color::WHITE);
    log.borrow_mut().clear();
    tap(&mut kernel, 100, 30, 85);
    assert_eq!(*log.borrow(), ["lock Tap at 30,85"]);
    // The lock asks to switch tasks: refused while it guards.
    let ids = |k: &Kernel| -> Vec<String> { k.screens().map(|id| id.as_str().into()).collect() };
    kernel.run_action(Instant(200), actions::NEXT_PAGE);
    assert_eq!(ids(&kernel), ["bar.page", "bar.lock"]);
    log.borrow_mut().clear();
    let top = kernel.stack.len() - 1;
    kernel.close_from(top);
    render(&mut kernel);
    assert_eq!(*log.borrow(), ["bar made 90x20", "bar shown"]);
    assert_eq!(kernel.stack[0].area, Rect::new(0, SIZE, 90, 90 - SIZE));
}

/// An overlay shares the part of the full screen below it, bars and
/// all, also where the bars are not shown over it by itself: it lies
/// right above the bar, a tap in it is its own, and only it is drawn
/// again.
#[test]
fn an_overlay_shares_the_bars_of_the_screen_below() {
    let log = log();
    let mut kernel = bar_kernel(&log, Edge::Bottom, DEVICE);
    log.borrow_mut().clear();
    let effects = kernel.open_screen(Instant(10), MENU, Params::new());
    assert!(log.borrow().is_empty(), "the bar stays: {:?}", log.borrow());
    let top = kernel.stack.len() - 1;
    assert_eq!(kernel.stack[top].area, Rect::new(0, 0, 90, 70));
    let (region, _) = effects.refresh().unwrap();
    assert!(!region.contains(Point::new(5, 75)), "the bar: {region:?}");
    render(&mut kernel);
    assert_eq!(kernel.frame().pixel(5, 45), Color::BLACK, "the panel");
    assert_eq!(
        kernel.frame().pixel(5, 75),
        Color::BLACK,
        "the bar's corner"
    );
    assert_eq!(kernel.frame().pixel(15, 75), Color::WHITE);
    tap(&mut kernel, 100, 30, 45);
    assert_eq!(kernel.stack.len(), 2, "a tap in the panel keeps it");
}

/// A bar's life is a screen's: shown, its timer, closed where it goes;
/// made anew for another size of the screen, or of itself.
#[test]
fn bars_live_as_screens_do() {
    let log = log();
    let mut kernel = bar_kernel(&log, Edge::Bottom, DEVICE);
    log.borrow_mut().clear();
    let wake = kernel.finish().wake_at;
    assert_eq!(wake, Some(Instant(100)));
    kernel.tick(Instant(100));
    assert_eq!(*log.borrow(), ["bar timer"]);
    log.borrow_mut().clear();
    kernel.resize(Instant(200), 80, 90);
    render(&mut kernel);
    assert_eq!(
        *log.borrow(),
        [
            "bar closed",
            "page made 80x70",
            "bar made 80x20",
            "bar shown"
        ]
    );
    log.borrow_mut().clear();
    kernel.set_setting(Instant(300), "bar.size", "30").unwrap();
    render(&mut kernel);
    assert_eq!(
        *log.borrow(),
        [
            "bar closed",
            "bar made 80x30",
            "bar shown",
            "page made 80x60"
        ]
    );
}

/// Swipes in from the edges begin inside the screen's part: not on a
/// bar, and the edge next to the bar is where the screen ends.
#[test]
fn edges_are_those_of_the_screens_part() {
    let log = log();
    let kernel = bar_kernel(&log, Edge::Bottom, DEVICE);
    assert_eq!(kernel.edge_at(Point::new(45, 80)), None, "on the bar");
    assert_eq!(kernel.edge_at(Point::new(45, 69)), Some(Direction::Down));
}

/// A task that cannot be shown closes nothing; a few tasks are kept.
#[test]
fn switching_to_no_task_closes_nothing() {
    let log = log();
    let mut kernel = bar_kernel(&log, Edge::Bottom, DEVICE);
    kernel.apply(Command::SwitchTask(Id::new_static("bar.nothing")));
    kernel.finish();
    assert_eq!(kernel.stack.len(), 1);
    assert!(
        kernel
            .take_warnings()
            .iter()
            .any(|w| w.message.contains("bar.nothing"))
    );
}

/// A few tasks are kept; with as many as are kept, the one switched to
/// shows again (it is looked up before the oldest is let go).
#[test]
fn a_task_switched_to_at_capacity_shows_again() {
    let log = log();
    let mut kernel = bar_kernel(&log, Edge::Bottom, DEVICE);
    for n in 0..8 {
        let task = Id::parse(&alloc::format!("bar.task{n}")).unwrap();
        let screens = alloc::vec![(BOOK, None, Params::new())];
        kernel.tasks.push((task, screens));
    }
    kernel.apply(Command::SwitchTask(Id::new_static("bar.task0")));
    kernel.finish();
    let ids: Vec<&str> = kernel.screens().map(Id::as_str).collect();
    assert_eq!(ids, ["bar.book"]);
    assert_eq!(kernel.tasks.len(), 8);
    assert!(kernel.tasks.iter().any(|(task, _)| *task == slots::HOME));
}
