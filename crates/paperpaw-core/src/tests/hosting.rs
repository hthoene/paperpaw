//! Tasks with their own way back (the hosting of a screen in another went
//! with the old SDK host, #244 D6b).

use super::*;
use crate::Items;
use paperpaw_sdk::builtin::events;
use paperpaw_sdk::event::EventData;
use paperpaw_sdk::{Gesture, Params};

const HOST: Id = Id::new_static("host.tabs");
const PAGE: Id = Id::new_static("host.page");
const LIST: Id = Id::new_static("host.list");

/// A page black in its top left corner (10 by 10): it logs taps, asks to
/// be drawn again on one, and says a word.
struct Page(Log);

impl crate::Layer for Page {
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
fn host_plugin(kept: Log) -> impl Fn(&mut Items) -> Result<(), RegisterError> {
    move |r| {
        let log = kept.clone();
        r.add_layer(crate::LayerSpec::new(
            PAGE,
            "page",
            Box::new(move |_, _| Box::new(Page(log.clone()))),
        ))?;
        let log = kept.clone();
        r.add_layer(crate::LayerSpec::new(
            HOST,
            "tabs",
            Box::new(move |_, _| Box::new(Page(log.clone()))),
        ))?;
        let log = kept.clone();
        r.add_layer(crate::LayerSpec::new(
            LIST,
            "list",
            Box::new(move |_, _| Box::new(Page(log.clone()))),
        ))?;
        r.bind_slot(slots::HOME, HOST, Priority::DEFAULT)?;
        r.bind_slot(slots::LIBRARY, LIST, Priority::DEFAULT)
    }
}

/// What [`host_plugin`] registers under.
fn host_plugin_manifest() -> Manifest {
    Manifest::new(Id::new_static("host"), "Host", Version::new(1, 0, 0))
        .uses(paperpaw_sdk::Uses::NOTHING)
}

/// Each task keeps its own way back: switched away and back, its
/// screens open again as they were.
#[test]
fn tasks_keep_their_own_way_back() {
    let log = log();
    let mut kernel = Kernel::builder(DEVICE)
        .items(host_plugin_manifest(), host_plugin(log.clone()))
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
        .items(host_plugin_manifest(), host_plugin(log.clone()))
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
