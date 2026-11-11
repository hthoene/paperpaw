//! A screen that guards the device (a lock): nothing shows above it but
//! what it opens itself.

use super::*;
use core::cell::Cell;
use paperpaw_sdk::Params;

const LOCK: Id = Id::new_static("g.lock");
const CHILD: Id = Id::new_static("g.child");
const OTHER: Id = Id::new_static("g.other");

/// A lock, locked until *Select*; *Menu* opens its own keyboard.
struct GuardPlugin(Rc<Cell<bool>>);

struct Lock(Rc<Cell<bool>>);

impl Screen for Lock {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}

    fn guards(&self) -> bool {
        self.0.get()
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        if *action == actions::CONFIRM {
            self.0.set(false);
            cx.command(Command::Back);
        } else if *action == actions::MENU {
            cx.command(Command::OpenScreen(CHILD, Params::new()));
        } else {
            return Handled::No;
        }
        Handled::Yes
    }
}

struct Plain;

impl Screen for Plain {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }
}

impl Plugin for GuardPlugin {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("g"), "Guard", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let locked = self.0.clone();
        r.add_screen(ScreenSpec::new(
            LOCK,
            "lock",
            Box::new(move |_, _| Box::new(Lock(locked.clone()))),
        ))?;
        for id in [CHILD, OTHER, Id::new_static("g.home")] {
            r.add_screen(ScreenSpec::new(
                id,
                "plain",
                Box::new(|_, _| Box::new(Plain)),
            ))?;
        }
        r.bind_slot(slots::HOME, Id::new_static("g.home"), Priority::DEFAULT)
    }
}

/// Locked, a screen another opens (a rule, a timer, the shell) waits
/// below the lock, others' *Back* closes nothing and notices wait; what
/// the lock opens itself shows above it. Unlocked, the screen that waited
/// is on top and the notice shows.
#[test]
fn nothing_shows_above_a_guarding_screen_but_its_own() {
    let locked = Rc::new(Cell::new(true));
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(GuardPlugin(locked.clone()))
        .plugin(NoticePlugin { log: log() })
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel.open_screen(Instant(1), LOCK, Params::new());
    assert_eq!(kernel.active_screen(), Some(&LOCK));

    kernel.open_screen(Instant(2), OTHER, Params::new());
    assert_eq!(kernel.active_screen(), Some(&LOCK), "waits below");
    kernel.apply(Command::Notify(String::from("A message")));
    kernel.apply(Command::Back);
    kernel.finish();
    assert_eq!(kernel.active_screen(), Some(&LOCK), "not closed by another");
    assert!(kernel.notice.is_none(), "held");

    // Its own keyboard shows above it, and goes back to it.
    kernel.run_action(Instant(3), actions::MENU);
    assert_eq!(kernel.active_screen(), Some(&CHILD));
    kernel.run_action(Instant(4), actions::BACK);
    assert_eq!(kernel.active_screen(), Some(&LOCK));

    // Unlocked: what waited is on top, the message shows.
    kernel.run_action(Instant(5), actions::CONFIRM);
    assert_eq!(kernel.active_screen(), Some(&OTHER));
    assert!(kernel.notice.is_some());
}

/// Locked, no plugin asks for plugins: that leads to an install, which
/// changes how the device is set up. Unlocked, the store hears it.
#[test]
fn no_plugins_are_asked_for_while_locked() {
    let locked = Rc::new(Cell::new(true));
    let log = log();
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(GuardPlugin(locked.clone()))
        .plugin(super::plugins_loaded::Store {
            log: log.clone(),
            installs: true,
        })
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel.open_screen(Instant(1), LOCK, Params::new());
    let asks = || Command::request_plugins(OTHER, [Id::new_static("vocabulary")]);
    kernel.apply(asks());
    kernel.run_commands();
    assert!(log.borrow().is_empty(), "{:?}", log.borrow());

    kernel.run_action(Instant(2), actions::CONFIRM);
    kernel.apply(asks());
    kernel.run_commands();
    assert_eq!(*log.borrow(), ["g.other asks vocabulary"]);
}

/// A guard that starts drops the private status values cached before it,
/// and public ones stay (`core.ui` op 14, ui.md 4.3).
#[test]
fn a_guard_starting_drops_the_private_cached_status_values() {
    use crate::native_host::{
        HostResources,
        ui_service::UiServices,
        ui_store::{Limits, UiStore},
    };
    let pool = HostResources::try_new().unwrap();
    pool.hosts()
        .wire_ui(UiServices::new(
            crate::native_host::shared::Shared::try_new(UiStore::new(Limits::ELSEWHERE)).unwrap(),
            false,
        ))
        .unwrap();
    let owner = pool.try_owner().unwrap();
    let locked = Rc::new(Cell::new(true));
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(GuardPlugin(locked.clone()))
        .build()
        .unwrap();
    kernel
        .shared
        .native_instances
        .adopt_pool(owner.try_view().unwrap())
        .unwrap();
    kernel.start(Instant(0));
    let ui = pool.hosts().ui().unwrap();
    {
        let mut cache = ui.widgets.cache.borrow_mut();
        cache.put(1, "p.private", b"", true, "secret");
        cache.put(1, "p.public", b"", false, "open");
    }
    kernel.open_screen(Instant(1), LOCK, Params::new());
    let cache = ui.widgets.cache.borrow();
    assert_eq!(cache.get(1, "p.private", b""), None);
    assert_eq!(cache.get(1, "p.public", b""), Some(&b"open"[..]));
}

/// Notices given while a screen guards the device wait, at most four (two on
/// the X4), the oldest dropped, and show one after the other once it lets go.
#[test]
fn notices_held_by_a_guard_wait_in_a_queue_and_show_in_turn() {
    for (memory, kept) in [
        (
            paperpaw_platform::MemoryClass::Large,
            ["n4", "n5", "n6", "n7"].as_slice(),
        ),
        (
            paperpaw_platform::MemoryClass::Small,
            ["n6", "n7"].as_slice(),
        ),
    ] {
        let locked = Rc::new(Cell::new(true));
        let mut kernel = Kernel::builder(DEVICE.with_memory(memory))
            .plugin(GuardPlugin(locked))
            .plugin(NoticePlugin { log: log() })
            .build()
            .unwrap();
        kernel.start(Instant(0));
        kernel.open_screen(Instant(1), LOCK, Params::new());
        for n in 1..=7 {
            kernel.apply(Command::Notify(alloc::format!("n{n}")));
        }
        kernel.finish();
        assert!(kernel.notice.is_none(), "held");
        assert_eq!(kernel.waiting_notices.texts(), kept, "{memory:?}");

        // Unlocked: the oldest that is kept shows, the others follow when it
        // is over, each for its full time.
        kernel.run_action(Instant(5), actions::CONFIRM);
        assert!(kernel.notice.is_some());
        let mut left = kept.len() - 1;
        assert_eq!(kernel.waiting_notices.texts(), &kept[1..]);
        let mut now = 5;
        while left > 0 {
            now += 4000;
            kernel.tick(Instant(now));
            left -= 1;
            assert!(kernel.notice.is_some());
            assert_eq!(kernel.waiting_notices.texts(), &kept[kept.len() - left..]);
        }
        now += 4000;
        kernel.tick(Instant(now));
        assert!(kernel.notice.is_none());
        assert!(kernel.waiting_notices.is_empty());
    }
}
