//! The screen stack as plugins read and steer it:
//! `Place::stack`, closing a screen of one's own, replacing, going back to
//! a screen, slots chosen for the user, asking before closing, a screen's
//! own row of hints, a screen opened again.

use super::*;
use crate::Asks;
use crate::Staging;
use paperpaw_sdk::builtin::events;
use paperpaw_sdk::{CloseCause, Gesture, HintVisibility, Instances, Params};

const SCREEN: Id = Id::new_static("s.screen");
const HOME: Id = Id::new_static("s.home");
const CLOSE: Id = Id::new_static("s.close");
const WHO: Id = Id::new_static("s.who");
const ONE: Id = Id::new_static("s.one");
const MANY: Id = Id::new_static("s.many");
const OPEN_T: Id = Id::new_static("s.open-t");

/// A screen that does what its params say: `name` (logged), `overlay`
/// (the lower half, `top`: a strip at the top), `refuse` (to close), `hints` (`hidden`, `on-press`),
/// `grays` (best shown in grays).
struct Configured {
    log: Log,
    params: Params,
}

impl Configured {
    fn name(&self) -> &str {
        self.params.get("name").unwrap_or("?")
    }

    fn says(&self, key: &str) -> Option<&str> {
        self.params.get(key)
    }

    fn note(&self, what: &str) {
        let line = alloc::format!("{} {what}", self.name());
        self.log.borrow_mut().push(line);
    }
}

impl crate::testing::Fake for Configured {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Place<'_>) {}

    fn region(&self, _: &Place<'_>) -> Option<Rect> {
        self.says("overlay").map(|at| match at {
            "top" => Rect::new(0, 0, 90, 20),
            _ => Rect::new(0, 45, 90, 45),
        })
    }

    fn on_action(&mut self, action: &Id, cx: &mut Turn<'_, '_>) -> Handled {
        if *action == CLOSE {
            cx.close();
        } else if *action == WHO {
            let slot = cx.slot().map_or("-", Id::as_str);
            let line = alloc::format!("is {} for {slot}", cx.screen_id().unwrap());
            self.note(&line);
        } else if *action == actions::NEXT_PAGE {
            self.note(&alloc::format!("action {action}"));
        } else {
            return Handled::No;
        }
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, _: &mut Turn<'_, '_>) -> Handled {
        self.note(&alloc::format!("gesture {:?}", gesture.kind));
        Handled::Yes
    }

    fn flags(&self) -> u32 {
        match self.says("grays") {
            Some(_) => crate::testing::GRAYS,
            None => 0,
        }
    }

    fn hints(&self) -> Option<HintVisibility> {
        match self.says("hints") {
            Some("hidden") => Some(HintVisibility::Hidden),
            Some("on-press") => Some(HintVisibility::OnPress),
            _ => None,
        }
    }

    fn on_close_request(&mut self, why: CloseCause, _: &mut Turn<'_, '_>) -> Handled {
        if self.says("refuse").is_some() {
            self.note(&alloc::format!("keeps open on {why:?}"));
            return Handled::Yes;
        }
        Handled::No
    }

    fn on_show(&mut self, _: &mut Turn<'_, '_>) {
        self.note("show");
    }

    fn on_reopen(&mut self, params: &Params, _: &mut Turn<'_, '_>) {
        let new = params.get("name").unwrap_or("?");
        self.note(&alloc::format!("reopen {new}"));
        self.params = params.clone();
    }

    fn on_close(&mut self, _: &mut Turn<'_, '_>) {
        self.note("close");
    }
}

fn stack_plugin(kept: Log) -> impl Fn(&mut Staging) -> Result<(), RegisterError> {
    move |r| {
        for (id, home, instances) in [
            (SCREEN, false, Instances::PER_PARAMS),
            (HOME, true, Instances::PER_PARAMS),
            (ONE, false, Instances::SINGLE),
            (MANY, false, Instances::MANY),
        ] {
            let log = kept.clone();
            r.add_fake(
                crate::testing::FakeSpec::new(
                    id,
                    "screen",
                    Box::new(move |_, params| {
                        let params = if home {
                            Params::new().with("name", "home")
                        } else {
                            params.clone()
                        };
                        Box::new(Configured {
                            log: log.clone(),
                            params,
                        })
                    }),
                )
                .instances(instances),
            )?;
        }
        r.add_action(ActionSpec::new(OPEN_T, "s.action").handler(
            |_: &crate::Place<'_>, _, out: &mut Asks| {
                out.push(Ask::OpenScreen(SCREEN, named("t")));
            },
        ))?;
        r.bind_slot(slots::HOME, HOME, Priority::DEFAULT)?;
        r.bind_slot(slots::MENU, SCREEN, Priority::DEFAULT)?;
        for action in [CLOSE, WHO] {
            r.add_action(ActionSpec::new(action, "s.action"))?;
        }
        crate::tests::hear_into(&kept);
        r.on(events::SCREEN_SHOWN, |_, p| {
            if let Some(shown) = crate::hear::data::<events::ScreenShown>(p) {
                let slot = shown.slot.as_ref().map_or("-", Id::as_str);
                crate::tests::heard(alloc::format!("shown {} for {slot}", shown.screen));
            }
        })
    }
}

/// What [`stack_plugin`] registers under.
fn stack_plugin_manifest() -> Manifest {
    Manifest::new(Id::new_static("s"), "Stack", Version::new(1, 0, 0))
        .uses(paperpaw_sdk::Uses::NOTHING)
}

fn named(name: &str) -> Params {
    Params::new().with("name", String::from(name))
}

fn stack_kernel(log: &Log) -> Kernel {
    let mut kernel = Kernel::builder(DEVICE)
        .items(stack_plugin_manifest(), stack_plugin(log.clone()))
        .storage(paperpaw_sdk::MemoryStorage::new())
        .build()
        .unwrap();
    kernel.start(Instant(0));
    log.borrow_mut().clear();
    kernel
}

fn names(kernel: &Kernel) -> Vec<String> {
    let env = kernel.shared.place(Instant(0));
    env.stack()
        .iter()
        .map(|entry| {
            let overlay = if entry.overlay { " (overlay)" } else { "" };
            alloc::format!("{}{overlay}", entry.screen)
        })
        .collect()
}

/// Plugins read the stack, a screen knows itself, and plugins hear which
/// screen came on top.
#[test]
fn the_stack_is_read_and_a_screen_knows_itself() {
    let log = log();
    let mut kernel = stack_kernel(&log);
    kernel.open_slot(Instant(1), slots::MENU, named("menu").with("overlay", "y"));
    assert_eq!(names(&kernel), ["s.home", "s.screen (overlay)"]);
    let env = kernel.shared.place(Instant(0));
    assert_eq!(
        env.top().and_then(|top| top.slot.clone()),
        Some(slots::MENU)
    );
    kernel.run_action(Instant(2), WHO);
    assert_eq!(
        *log.borrow(),
        [
            "menu show",
            "shown s.screen for core.menu",
            "menu is s.screen for core.menu"
        ]
    );
    log.borrow_mut().clear();
    kernel.run_action(Instant(3), actions::BACK);
    assert_eq!(
        *log.borrow(),
        ["menu close", "home show", "shown s.home for core.home"]
    );
}

/// A screen closes itself wherever it is: the screens above it stay.
#[test]
fn a_screen_closes_itself_below_others() {
    let log = log();
    let mut kernel = stack_kernel(&log);
    kernel.open_screen(Instant(1), SCREEN, named("a"));
    kernel.open_screen(Instant(2), SCREEN, named("b").with("overlay", "y"));
    let a = kernel.shared.stack[1].key;
    log.borrow_mut().clear();
    kernel.apply(Ask::Close(a));
    kernel.finish();
    assert_eq!(names(&kernel), ["s.home", "s.screen (overlay)"]);
    assert_eq!(*log.borrow(), ["a close"]);
    // The only one is never closed.
    kernel.run_action(Instant(3), CLOSE);
    kernel.run_action(Instant(4), CLOSE);
    assert_eq!(names(&kernel), ["s.home"]);
}

/// One screen in place of another in one step, and back to a screen (or
/// a slot's) in one step.
#[test]
fn screens_are_replaced_and_gone_back_to() {
    let log = log();
    let mut kernel = stack_kernel(&log);
    kernel.open_screen(Instant(1), SCREEN, named("step 1"));
    log.borrow_mut().clear();
    kernel.apply(Ask::Replace(SCREEN, named("step 2")));
    kernel.finish();
    assert_eq!(names(&kernel), ["s.home", "s.screen"]);
    assert_eq!(
        *log.borrow(),
        ["step 1 close", "step 2 show", "shown s.screen for -"],
        "the home screen is not shown between"
    );
    kernel.open_screen(Instant(2), SCREEN, named("step 3"));
    kernel.apply(Ask::BackTo(slots::HOME));
    kernel.finish();
    assert_eq!(names(&kernel), ["s.home"]);
    kernel.apply(Ask::BackTo(Id::new_static("s.none")));
    kernel.finish();
    assert_eq!(names(&kernel), ["s.home"]);
}

/// A screen opened for a slot of the plugin's choosing, and a slot bound
/// for the user: at once and kept.
#[test]
fn slots_take_a_screen_chosen() {
    let log = log();
    let mut kernel = stack_kernel(&log);
    kernel.apply(Ask::OpenInSlot(slots::LIBRARY, SCREEN, named("shelf")));
    kernel.finish();
    let env = kernel.shared.place(Instant(0));
    assert_eq!(
        env.top().and_then(|top| top.slot.clone()),
        Some(slots::LIBRARY)
    );
    kernel.apply(Ask::BindSlot(slots::HOME, Some(SCREEN)));
    kernel.finish();
    assert_eq!(
        kernel.shared.registry.resolve_slot(slots::HOME.as_str()),
        Some(&SCREEN)
    );
    let saved = paperpaw_sdk::read_file(kernel.storage(), "paperpaw.ini", 4096).unwrap();
    assert!(String::from_utf8_lossy(&saved).contains("core.home = s.screen"));
    kernel.apply(Ask::BindSlot(slots::HOME, None));
    kernel.finish();
    assert_eq!(
        kernel.shared.registry.resolve_slot(slots::HOME.as_str()),
        Some(&HOME)
    );
    let saved = paperpaw_sdk::read_file(kernel.storage(), "paperpaw.ini", 4096).unwrap();
    assert!(!String::from_utf8_lossy(&saved).contains("core.home"));
}

/// A screen asked before it closes may stay open: on *Back*, a tap
/// outside it, its slot shown again.
#[test]
fn a_screen_may_keep_itself_open() {
    let log = log();
    let mut kernel = stack_kernel(&log);
    let form = named("form").with("refuse", "y").with("overlay", "y");
    kernel.open_screen(Instant(1), SCREEN, form);
    log.borrow_mut().clear();
    kernel.run_action(Instant(2), actions::BACK);
    tap(&mut kernel, 100, 45, 10);
    kernel.open_slot(Instant(200), slots::HOME, Params::new());
    assert_eq!(names(&kernel), ["s.home", "s.screen (overlay)"]);
    assert_eq!(
        *log.borrow(),
        [
            "form keeps open on Back",
            "form keeps open on TapOutside",
            "form keeps open on Slot"
        ]
    );
}

/// A screen says itself that it has no row of hints: it is made for all
/// of the screen, where its plugin said nothing.
#[test]
fn a_screen_says_its_own_row_of_hints() {
    let log = log();
    let mut kernel = Kernel::builder(DEVICE)
        .items(stack_plugin_manifest(), stack_plugin(log.clone()))
        .button_row(&["back", "confirm", "left", "right"])
        .build()
        .unwrap();
    kernel.start(Instant(0));
    assert!(kernel.stack[0].hinted);
    kernel.open_screen(Instant(1), SCREEN, named("page").with("hints", "hidden"));
    assert!(!kernel.stack[1].hinted);
    assert_eq!(kernel.shared.device.display.height, 90);
}

/// A menu over a page without the row shows the row, also where its
/// labels are those the row had before the page (#448): the row is drawn
/// again whole, not taken as still shown.
#[test]
fn a_menu_over_a_page_without_hints_draws_the_row_again() {
    let mut kernel = Kernel::builder(DEVICE)
        .items(stack_plugin_manifest(), stack_plugin(log()))
        .button_row(&["back", "confirm", "left", "right"])
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel.open_screen(Instant(1), SCREEN, named("list"));
    render(&mut kernel);
    kernel.open_screen(Instant(2), SCREEN, named("page").with("hints", "hidden"));
    render(&mut kernel);
    assert!(kernel.hint_row().is_none());
    assert!(kernel.frame().data.iter().all(|byte| *byte == 0xff));
    kernel.open_screen(Instant(3), SCREEN, named("menu").with("overlay", "top"));
    render(&mut kernel);
    assert!(kernel.hint_row().is_some());
    assert!(kernel.frame().data.iter().any(|byte| *byte != 0xff));
}

/// A screen opened again with the same params while it is open is gone
/// back to, not stacked: by a remote control, an action (as a key or a
/// widget's tap runs it) and its slot. *Back* then leaves it in one press.
/// With other params, another opens above it.
#[test]
fn a_screen_opened_again_is_gone_back_to() {
    let log = log();
    let mut kernel = stack_kernel(&log);
    kernel.open_screen(Instant(1), SCREEN, named("t"));
    kernel.open_screen(Instant(2), SCREEN, named("u"));
    assert_eq!(names(&kernel), ["s.home", "s.screen", "s.screen"]);
    log.borrow_mut().clear();
    kernel.open_screen(Instant(3), SCREEN, named("t"));
    assert_eq!(names(&kernel), ["s.home", "s.screen"]);
    assert_eq!(
        *log.borrow(),
        // As a plugin's screen does: shown as it is opened again on top, and
        // as the top screen.
        [
            "u close",
            "t reopen t",
            "t show",
            "t show",
            "shown s.screen for -"
        ]
    );
    kernel.run_action(Instant(4), OPEN_T);
    kernel.run_action(Instant(5), OPEN_T);
    kernel.open_screen(Instant(6), SCREEN, named("t"));
    assert_eq!(names(&kernel), ["s.home", "s.screen"]);
    // Params alike in another order.
    let v = Params::new().with("name", "v").with("x", "1");
    kernel.open_screen(Instant(7), SCREEN, v);
    let v = Params::new().with("x", "1").with("name", "v");
    kernel.open_screen(Instant(8), SCREEN, v);
    assert_eq!(names(&kernel), ["s.home", "s.screen", "s.screen"]);
    kernel.open_slot(Instant(9), slots::MENU, named("menu"));
    kernel.open_slot(Instant(10), slots::MENU, named("menu"));
    assert_eq!(
        names(&kernel),
        ["s.home", "s.screen", "s.screen", "s.screen"]
    );
    kernel.run_action(Instant(11), actions::BACK);
    kernel.run_action(Instant(12), actions::BACK);
    kernel.run_action(Instant(13), actions::BACK);
    assert_eq!(names(&kernel), ["s.home"]);
}

/// A screen of one instance at most is gone back to whatever the params,
/// and hears the new ones; a screen of many opens each time.
#[test]
fn screens_say_how_many_of_them_open() {
    let log = log();
    let mut kernel = stack_kernel(&log);
    kernel.open_screen(Instant(1), ONE, named("a"));
    kernel.open_screen(Instant(2), SCREEN, named("x"));
    log.borrow_mut().clear();
    kernel.open_screen(Instant(3), ONE, named("b"));
    assert_eq!(names(&kernel), ["s.home", "s.one"]);
    assert_eq!(
        *log.borrow(),
        [
            "x close",
            "a reopen b",
            "b show",
            "b show",
            "shown s.one for -"
        ]
    );
    kernel.run_action(Instant(4), actions::BACK);
    assert_eq!(names(&kernel), ["s.home"]);
    kernel.open_screen(Instant(5), MANY, named("m"));
    kernel.open_screen(Instant(6), MANY, named("m"));
    assert_eq!(names(&kernel), ["s.home", "s.many", "s.many"]);
}

/// A screen opened again that a screen above it keeps from closing: the
/// stack stays as it is, nothing more opens.
#[test]
fn a_screen_opened_again_waits_for_the_ones_above() {
    let log = log();
    let mut kernel = stack_kernel(&log);
    kernel.open_screen(Instant(1), ONE, named("a"));
    kernel.open_screen(Instant(2), SCREEN, named("form").with("refuse", "y"));
    kernel.open_screen(Instant(3), ONE, named("b"));
    assert_eq!(names(&kernel), ["s.home", "s.one", "s.screen"]);
}

/// Uses the same slot and hint policy as a sleep plugin.
fn hidden_sleep_plugin() -> impl Fn(&mut Staging) -> Result<(), RegisterError> {
    |r| {
        const SLEEP: Id = Id::new_static("quiet.sleep");
        r.add_fake(crate::testing::FakeSpec::new(
            SLEEP,
            "Sleep",
            Box::new(|_, _| {
                Box::new(Configured {
                    log: log(),
                    params: named("sleep").with("hints", "hidden"),
                })
            }),
        ))?;
        r.bind_slot(slots::SLEEP, SLEEP, Priority::DEFAULT)
    }
}

/// What [`hidden_sleep_plugin`] registers under.
fn hidden_sleep_plugin_manifest() -> Manifest {
    Manifest::new(Id::new_static("quiet"), "Quiet", Version::new(1, 0, 0))
        .uses(paperpaw_sdk::Uses::NOTHING)
}

fn reader_with_hints() -> Kernel {
    let mut kernel = Kernel::builder(DEVICE)
        .items(stack_plugin_manifest(), stack_plugin(log()))
        .items(hidden_sleep_plugin_manifest(), hidden_sleep_plugin())
        .sleep(true)
        .button_row(&["back", "confirm", "left", "right"])
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel.open_screen(
        Instant(1),
        SCREEN,
        named("reader").with("hints", "on-press"),
    );
    render(&mut kernel);
    assert!(kernel.hint_row().is_none());
    press(&mut kernel, 10, "confirm");
    assert!(kernel.hint_row().is_some());
    assert!(kernel.frame().data.iter().any(|byte| *byte != 0xff));
    kernel
}

fn assert_blank_sleep_frame(kernel: &mut Kernel) {
    render(kernel);
    assert!(kernel.hint_row().is_none());
    // The fixture's sleep screen draws nothing. Any ink is stale chrome
    // or button hints that would stay on the sleeping e-paper panel.
    assert!(kernel.frame().data.iter().all(|byte| *byte == 0xff));
}

#[test]
fn hidden_sleep_screen_removes_recent_reader_hints_before_suspend() {
    let mut kernel = reader_with_hints();
    let effects = kernel.run_action(Instant(100), actions::SLEEP);
    assert!(effects.sleep());
    assert_eq!(
        kernel.stack.last().map(|top| &top.id),
        Some(&Id::new_static("quiet.sleep"))
    );
    assert_blank_sleep_frame(&mut kernel);

    // A shell may redraw for its display mode before entering sleep.
    kernel.draw_again(Instant(101));
    assert_blank_sleep_frame(&mut kernel);
    kernel.suspend(Instant(102));
    kernel.draw_again(Instant(103));
    assert_blank_sleep_frame(&mut kernel);
}

#[test]
fn shown_menu_keeps_its_hints_after_reader_hint_timeout() {
    let mut kernel = reader_with_hints();
    kernel.open_screen(Instant(100), SCREEN, named("menu"));
    render(&mut kernel);
    assert!(kernel.hint_row().is_some());
    assert!(kernel.stack.last().unwrap().hinted);
    kernel.tick(Instant(5000));
    kernel.draw_again(Instant(5001));
    render(&mut kernel);
    assert!(kernel.hint_row().is_some());
    assert!(kernel.frame().data.iter().any(|byte| *byte != 0xff));
}

#[test]
fn the_row_of_hints_over_a_page_in_grays_comes_and_goes_with_the_whole_frame() {
    let mut kernel = Kernel::builder(DEVICE)
        .items(stack_plugin_manifest(), stack_plugin(log()))
        .button_row(&["back", "confirm", "left", "right"])
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel.open_screen(
        Instant(1),
        SCREEN,
        named("reader").with("hints", "on-press").with("grays", "y"),
    );
    render(&mut kernel);
    let screen = kernel.framebuffer.bounds();
    // A press that changes nothing on the page brings the row: shown as a
    // frame in grays, which a shell does not leave for the next page.
    let effects = press(&mut kernel, 10, "confirm");
    assert!(kernel.hint_row().is_some());
    assert!(effects.grays());
    assert_eq!(effects.refresh().map(|(area, _)| area), Some(screen));
    render(&mut kernel);
    // Its time over, it goes the same way, or it stays until the next
    // page, which brings it again.
    let effects = kernel.tick(Instant(60 + crate::HINTS_MS));
    assert!(kernel.hint_row().is_none());
    assert!(effects.grays());
    assert_eq!(effects.refresh().map(|(area, _)| area), Some(screen));
}

#[test]
fn hidden_overlay_overrides_the_readers_active_hint_timer() {
    let mut kernel = reader_with_hints();
    kernel.open_screen(
        Instant(100),
        SCREEN,
        named("overlay")
            .with("overlay", "y")
            .with("hints", "hidden"),
    );
    render(&mut kernel);
    assert_eq!(kernel.base_hints(), HintVisibility::OnPress);
    assert!(kernel.hint_row().is_none());
    kernel.draw_again(Instant(101));
    render(&mut kernel);
    assert!(kernel.frame().data.iter().all(|byte| *byte == 0xff));
}
