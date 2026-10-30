//! Screens hosted in a screen of another plugin, and tasks with their own
//! way back.

use super::*;
use paperpaw_sdk::builtin::events;
use paperpaw_sdk::event::EventData;
use paperpaw_sdk::host::ScreenHost;
use paperpaw_sdk::{Gesture, Params};

const HOST: Id = Id::new_static("host.tabs");
const PAGE: Id = Id::new_static("host.page");
const LIST: Id = Id::new_static("host.list");

/// A page black in its top left corner (10 by 10): it logs taps, asks to
/// be drawn again on one, and says a word.
struct Page(Log);

impl Screen for Page {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        canvas.fill_rect(0, 0, 10, 10, Color::BLACK);
        let display = env.device().display;
        self.0.borrow_mut().push(alloc::format!(
            "page drawn {}x{}",
            display.width,
            display.height
        ));
    }

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        let (x, y) = (gesture.start.x, gesture.start.y);
        self.0
            .borrow_mut()
            .push(alloc::format!("page tapped at {x},{y}"));
        cx.request_redraw_area(Rect::new(0, 0, 4, 4));
        cx.command(Command::Notify(String::from("tapped")));
        Handled::Yes
    }
}

/// Hosts the page in its lower half.
struct Tabs {
    host: ScreenHost,
}

impl Screen for Tabs {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        self.host.draw(canvas, env);
    }

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        let pane = Rect::new(0, 45, 90, 45);
        assert!(self.host.show(cx, PAGE, Params::new(), pane));
        self.host.on_show(cx);
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        self.host.on_gesture(gesture, cx)
    }
}

struct HostPlugin(Log);

impl Plugin for HostPlugin {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("host"), "Host", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let log = self.0.clone();
        r.add_screen(ScreenSpec::new(
            PAGE,
            "page",
            Box::new(move |_, _| Box::new(Page(log.clone()))),
        ))?;
        r.add_screen(ScreenSpec::new(
            HOST,
            "tabs",
            Box::new(|_, _| {
                Box::new(Tabs {
                    host: ScreenHost::new(),
                })
            }),
        ))?;
        let log = self.0.clone();
        r.add_screen(ScreenSpec::new(
            LIST,
            "list",
            Box::new(move |_, _| Box::new(Page(log.clone()))),
        ))?;
        r.bind_slot(slots::HOME, HOST, Priority::DEFAULT)?;
        r.bind_slot(slots::LIBRARY, LIST, Priority::DEFAULT)
    }
}

/// A screen made by another plugin's, in a part of it: drawn there from
/// its own corner, the size of its part; taps there reach it in its
/// coordinates; what it asks for reaches the kernel, moved to the screen.
#[test]
fn a_screen_hosts_another_in_a_part_of_it() {
    let log = log();
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(NoticePlugin { log: super::log() })
        .plugin(HostPlugin(log.clone()))
        .build()
        .unwrap();
    kernel.start(Instant(0));
    render(&mut kernel);
    assert!(log.borrow().contains(&String::from("page drawn 90x45")));
    assert_eq!(
        kernel.frame().pixel(5, 50),
        Color::BLACK,
        "the page's corner"
    );
    assert_eq!(kernel.frame().pixel(5, 5), Color::WHITE);
    log.borrow_mut().clear();
    kernel.handle(
        Instant(100),
        InputEvent::Pointer(PointerEvent::new(
            0,
            PointerPhase::Down,
            Point::new(30, 60),
            PointerTool::Finger,
            1000,
        )),
    );
    let effects = kernel.handle(
        Instant(150),
        InputEvent::Pointer(PointerEvent::new(
            0,
            PointerPhase::Up,
            Point::new(30, 60),
            PointerTool::Finger,
            1000,
        )),
    );
    assert_eq!(*log.borrow(), ["page tapped at 30,15"]);
    assert!(kernel.notice.is_some(), "its command reached the kernel");
    let (region, _) = effects.refresh().unwrap();
    assert!(region.contains(Point::new(1, 46)), "{region:?}");
}

/// Each task keeps its own way back: switched away and back, its
/// screens open again as they were.
#[test]
fn tasks_keep_their_own_way_back() {
    let log = log();
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(HostPlugin(log.clone()))
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel.open_screen(Instant(1), PAGE, Params::new().with("n", "1"));
    let ids = |kernel: &Kernel| -> Vec<String> {
        kernel.screens().map(|id| id.as_str().into()).collect()
    };
    assert_eq!(ids(&kernel), ["host.tabs", "host.page"]);
    kernel.apply(Command::SwitchTask(slots::LIBRARY));
    kernel.finish();
    assert_eq!(ids(&kernel), ["host.list"]);
    kernel.open_screen(Instant(2), PAGE, Params::new());
    kernel.apply(Command::SwitchTask(slots::HOME));
    kernel.finish();
    assert_eq!(ids(&kernel), ["host.tabs", "host.page"]);
    assert_eq!(kernel.stack[1].params.get("n"), Some("1"));
    kernel.apply(Command::SwitchTask(slots::LIBRARY));
    kernel.finish();
    assert_eq!(ids(&kernel), ["host.list", "host.page"]);
}

/// A task kept follows its book: moved, its screen opens the book where
/// it is now; deleted, the screens from the one that shows it are let go
/// and those below it stay, so switching back opens no file that is gone.
#[test]
fn tasks_kept_follow_their_books() {
    let log = log();
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(HostPlugin(log.clone()))
        .build()
        .unwrap();
    kernel.start(Instant(0));
    let path = paperpaw_sdk::builtin::params::PATH;
    // Two books one over the other, and a screen of neither over them.
    kernel.open_screen(
        Instant(1),
        PAGE,
        Params::new().with(path, "books/kept.epub"),
    );
    kernel.open_screen(Instant(1), PAGE, Params::new().with(path, "books/a.epub"));
    kernel.open_screen(Instant(1), PAGE, Params::new().with("n", "over it"));
    let ids = |kernel: &Kernel| -> Vec<String> {
        kernel.screens().map(|id| id.as_str().into()).collect()
    };
    let shown = |kernel: &Kernel| -> Vec<Option<String>> {
        kernel
            .stack
            .iter()
            .map(|s| s.params.get(path).map(String::from))
            .collect()
    };
    kernel.apply(Command::SwitchTask(slots::LIBRARY));
    kernel.finish();
    let moved = events::BookMoved::new("books/a.epub".into(), "books/b.epub".into());
    kernel.send_event(
        Instant(2),
        events::BOOK_MOVED.id().clone(),
        moved.to_params(),
    );
    kernel.apply(Command::SwitchTask(slots::HOME));
    kernel.finish();
    assert_eq!(
        ids(&kernel),
        ["host.tabs", "host.page", "host.page", "host.page"]
    );
    assert_eq!(
        shown(&kernel),
        [
            None,
            Some("books/kept.epub".into()),
            Some("books/b.epub".into()),
            None
        ]
    );
    kernel.apply(Command::SwitchTask(slots::LIBRARY));
    kernel.finish();
    let removed = events::BookRemoved::new("books/b.epub".into());
    kernel.send_event(
        Instant(3),
        events::BOOK_REMOVED.id().clone(),
        removed.to_params(),
    );
    kernel.apply(Command::SwitchTask(slots::HOME));
    kernel.finish();
    // The other book's screen stays; the deleted one's and what was over
    // it go.
    assert_eq!(ids(&kernel), ["host.tabs", "host.page"]);
    assert_eq!(shown(&kernel), [None, Some("books/kept.epub".into())]);
    // Its book deleted too: only the task's first screen is left.
    kernel.apply(Command::SwitchTask(slots::LIBRARY));
    kernel.finish();
    let removed = events::BookRemoved::new("books/kept.epub".into());
    kernel.send_event(
        Instant(4),
        events::BOOK_REMOVED.id().clone(),
        removed.to_params(),
    );
    kernel.apply(Command::SwitchTask(slots::HOME));
    kernel.finish();
    assert_eq!(ids(&kernel), ["host.tabs"]);
}
