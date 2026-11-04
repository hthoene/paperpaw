//! A list and the item chosen in it beside each other where the screen is
//! wide (`Pane::DETAIL`): the parts they have, where input goes, a detail
//! in place of another, what ends it, the X4 never, and the display's new
//! size told (`events::DISPLAY_CHANGED`). Built without the `panes`
//! feature (the Xteink devices), never beside each other.
#![cfg_attr(
    not(feature = "panes"),
    allow(dead_code, unused_imports, reason = "the tests of panes are left out")
)]

use super::*;
use core::cell::Cell;
use paperpaw_sdk::builtin::events;
use paperpaw_sdk::ui::ScreenClass;
use paperpaw_sdk::{Gesture, Pane, Params};

const SCREEN: Id = Id::new_static("p.screen");
const OPEN: Id = Id::new_static("p.open");
const FULL: Id = Id::new_static("p.full");
const PING: Id = Id::new_static("p.ping");
const WAKE: Id = Id::new_static("p.wake");
const MENU: Id = Id::new_static("p.menu");

/// 600 by 300 at 100 dots per inch: 152 mm, expanded. The list's part is
/// 60 mm (236 dots, more than 38 %), the line one dot, the detail the rest.
const WIDE: (u16, u16) = (600, 300);
const LIST: Rect = Rect::new(0, 0, 236, 300);
const DETAIL: Rect = Rect::new(237, 0, 363, 300);

/// A screen named by its params (`list` without), a detail where they
/// say `detail`, an overlay where `overlay`; it logs what it is told.
struct Paned {
    log: Log,
    params: Params,
    opened: Rc<Cell<u32>>,
}

impl Paned {
    fn name(&self) -> &str {
        self.params.get("name").unwrap_or("list")
    }

    fn note(&self, what: &str) {
        let line = alloc::format!("{} {what}", self.name());
        self.log.borrow_mut().push(line);
    }
}

impl Screen for Paned {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}

    fn region(&self, _: &Env<'_>) -> Option<Rect> {
        self.params
            .get("overlay")
            .map(|_| Rect::new(10, 10, 100, 100))
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let open = |name: String, kind: &'static str| {
            Command::OpenScreen(SCREEN, Params::new().with("name", name).with(kind, "yes"))
        };
        if *action == PING {
            self.note("ping");
        } else if *action == OPEN {
            let n = self.opened.get() + 1;
            self.opened.set(n);
            cx.command(open(alloc::format!("d{n}"), "detail"));
        } else if *action == FULL {
            cx.command(open(String::from("full"), "full"));
        } else if *action == MENU {
            cx.command(open(String::from("menu"), "overlay"));
        } else if *action == WAKE {
            cx.wake_at(cx.env().now().add_millis(100));
        } else {
            return Handled::No;
        }
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, _: &mut Cx<'_, '_>) -> Handled {
        self.note(&alloc::format!(
            "tap {},{}",
            gesture.start.x,
            gesture.start.y
        ));
        Handled::Yes
    }

    fn on_show(&mut self, _: &mut Cx<'_, '_>) {
        self.note("show");
    }

    fn on_hide(&mut self) {
        self.note("hide");
    }

    fn on_close(&mut self, _: &mut Cx<'_, '_>) {
        self.note("close");
    }

    fn on_timer(&mut self, _: &mut Cx<'_, '_>) {
        self.note("timer");
    }

    fn on_resize(&mut self, cx: &mut Cx<'_, '_>) -> Handled {
        let content = cx.env().content();
        self.note(&alloc::format!(
            "resize {}x{}",
            content.width,
            content.height
        ));
        Handled::Yes
    }

    fn pane(&self) -> Pane {
        match self.params.get("detail") {
            Some(_) => Pane::DETAIL,
            None => Pane::FULL,
        }
    }
}

/// The screen, in the home slot as the list; hears the display's size.
struct PanePlugin {
    log: Log,
}

impl Plugin for PanePlugin {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("p"), "Panes", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let log = self.log.clone();
        let opened = Rc::new(Cell::new(0));
        r.add_screen(ScreenSpec::new(
            SCREEN,
            "screen",
            Box::new(move |_, params| {
                Box::new(Paned {
                    log: log.clone(),
                    params: params.clone(),
                    opened: opened.clone(),
                })
            }),
        ))?;
        for action in [OPEN, FULL, PING, WAKE, MENU] {
            r.add_action(ActionSpec::new(action, "action"))?;
        }
        let log = self.log.clone();
        r.on(events::DISPLAY_CHANGED, move |_, changed, _| {
            log.borrow_mut().push(alloc::format!(
                "display {}x{} {} {}",
                changed.width,
                changed.height,
                changed.class.name(),
                changed.landscape
            ));
        })?;
        r.bind_slot(slots::HOME, SCREEN, Priority::DEFAULT)
    }
}

/// The kernel on `device` with the list on top, turned to `size`.
fn on(device: DeviceInfo, size: (u16, u16), log: &Log) -> Kernel {
    let mut kernel = Kernel::builder(device)
        .plugin(PanePlugin { log: log.clone() })
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel.resize(Instant(10), size.0, size.1);
    render(&mut kernel);
    log.borrow_mut().clear();
    kernel
}

/// The list with detail `d1` beside it on a wide screen.
fn with_detail(log: &Log) -> Kernel {
    let mut kernel = on(DEVICE, WIDE, log);
    kernel.run_action(Instant(100), OPEN);
    render(&mut kernel);
    log.borrow_mut().clear();
    kernel
}

fn areas(kernel: &Kernel) -> Vec<Rect> {
    kernel.stack.iter().map(|active| active.area).collect()
}

fn took(log: &Log) -> Vec<String> {
    log.take()
}

#[cfg(feature = "panes")]
#[test]
fn a_detail_shows_beside_its_list_where_the_screen_is_wide() {
    let log = log();
    let mut kernel = on(DEVICE, WIDE, &log);
    kernel.run_action(Instant(100), OPEN);
    render(&mut kernel);
    assert_eq!(areas(&kernel), [LIST, DETAIL]);
    assert_eq!(kernel.panes(), Some((0, 1)));
    // The list laid out anew for its part, never hidden; the detail made
    // once, for its own.
    assert_eq!(took(&log), ["list resize 236x300", "list show", "d1 show"]);
    // The line between them, nothing on either side of it.
    let canvas = kernel.framebuffer.canvas();
    assert_eq!(canvas.pixel(Point::new(236, 150)), Some(Color::BLACK));
    assert_eq!(canvas.pixel(Point::new(235, 150)), Some(Color::WHITE));
    assert_eq!(canvas.pixel(Point::new(237, 150)), Some(Color::WHITE));
}

/// On the X4, in either orientation, a detail is a full screen as ever,
/// and the list below it is hidden.
#[test]
fn on_the_x4_a_detail_is_a_full_screen() {
    let x4 = DeviceInfo::new(
        "x4",
        "X4",
        DisplayInfo::new(480, 800, 220, PixelFormat::Gray4, Rotation::Deg0, None),
        InputCaps::new(true, false, false, &[]),
        paperpaw_platform::MemoryClass::Small,
    );
    for size in [(480, 800), (800, 480)] {
        let log = log();
        let mut kernel = on(x4, size, &log);
        kernel.run_action(Instant(100), OPEN);
        let whole = Rect::new(0, 0, size.0, size.1);
        assert_eq!(areas(&kernel), [whole, whole]);
        assert_eq!(kernel.panes(), None);
        assert_eq!(took(&log), ["list hide", "d1 show"]);
    }
}

/// Narrower than expanded (medium, 120 mm here): no panes either.
#[test]
fn a_medium_screen_shows_a_detail_alone() {
    let log = log();
    let mut kernel = on(DEVICE, (480, 300), &log);
    kernel.run_action(Instant(100), OPEN);
    assert_eq!(kernel.panes(), None);
    assert_eq!(kernel.stack[1].area, Rect::new(0, 0, 480, 300));
}

/// Keys go to the detail at first; a tap goes to the part it lands in,
/// in its coordinates, and keys follow it there.
#[cfg(feature = "panes")]
#[test]
fn a_tap_moves_input_to_the_part_it_lands_in() {
    let log = log();
    let mut kernel = with_detail(&log);
    kernel.run_action(Instant(200), PING);
    tap(&mut kernel, 300, 20, 30);
    kernel.run_action(Instant(500), PING);
    tap(&mut kernel, 600, 300, 30);
    kernel.run_action(Instant(800), PING);
    assert_eq!(
        took(&log),
        [
            "d1 ping",
            "list tap 20,30",
            "list ping",
            "d1 tap 63,30",
            "d1 ping"
        ]
    );
}

/// A finger on the line between the parts goes to the nearer one (the
/// list where it is as near to both).
#[cfg(feature = "panes")]
#[test]
fn a_tap_on_the_line_goes_to_the_nearer_part() {
    let log = log();
    let mut kernel = with_detail(&log);
    tap(&mut kernel, 200, 300, 30);
    tap(&mut kernel, 500, 236, 30);
    kernel.run_action(Instant(800), PING);
    assert_eq!(took(&log), ["d1 tap 63,30", "list tap 236,30", "list ping"]);
}

/// The class goes by the interface's size: larger text that makes the
/// screen medium shows the detail alone, the list hidden; back to the
/// default, beside it again.
#[cfg(feature = "panes")]
#[test]
fn larger_interface_text_joins_the_parts() {
    let log = log();
    let mut kernel = with_detail(&log);
    let size = paperpaw_sdk::builtin::settings::UI_SIZE.id().clone();
    kernel.apply(Command::SetSetting(size.clone(), SettingValue::Int(12)));
    kernel.finish();
    assert_eq!(kernel.panes(), None);
    let whole = Rect::new(0, 0, WIDE.0, WIDE.1);
    assert_eq!(areas(&kernel), [whole, whole]);
    assert!(took(&log).contains(&String::from("list hide")));
    kernel.apply(Command::SetSetting(size, SettingValue::Int(8)));
    kernel.finish();
    assert_eq!(kernel.panes(), Some((0, 1)));
    assert_eq!(areas(&kernel), [LIST, DETAIL]);
    assert!(took(&log).contains(&String::from("list show")));
}

/// `core.pane.other` moves keys to the other part and back; alone, the
/// list keeps them.
#[cfg(feature = "panes")]
#[test]
fn the_other_pane_action_moves_keys_between_the_two() {
    let log = log();
    let mut kernel = with_detail(&log);
    kernel.run_action(Instant(200), actions::PANE_OTHER);
    kernel.run_action(Instant(300), PING);
    kernel.run_action(Instant(400), actions::PANE_OTHER);
    kernel.run_action(Instant(500), PING);
    assert_eq!(took(&log), ["list ping", "d1 ping"]);
    let mut alone = on(DEVICE, WIDE, &log);
    alone.run_action(Instant(200), actions::PANE_OTHER);
    alone.run_action(Instant(300), PING);
    assert_eq!(took(&log), ["list ping"]);
}

/// A detail the list opens while keys are its own takes the place of the
/// one shown (closed, dropped before): going through a list piles nothing
/// up, the list is not laid out again, and keys stay with it.
#[cfg(feature = "panes")]
#[test]
fn a_detail_the_list_opens_takes_the_place_of_the_one_shown() {
    let log = log();
    let mut kernel = with_detail(&log);
    tap(&mut kernel, 200, 20, 30);
    log.borrow_mut().clear();
    kernel.run_action(Instant(300), OPEN);
    assert_eq!(kernel.stack.len(), 2);
    assert_eq!(areas(&kernel), [LIST, DETAIL]);
    assert_eq!(took(&log), ["d1 close", "list show", "d2 show"]);
    kernel.run_action(Instant(400), PING);
    assert_eq!(took(&log), ["list ping"]);
}

/// A detail the detail opens shows alone over both (they are hidden);
/// *Back* brings the two back, each shown again.
#[cfg(feature = "panes")]
#[test]
fn a_detail_the_detail_opens_shows_alone_over_both() {
    let log = log();
    let mut kernel = with_detail(&log);
    kernel.run_action(Instant(200), OPEN);
    assert_eq!(kernel.stack.len(), 3);
    assert_eq!(kernel.panes(), None);
    assert_eq!(kernel.stack[2].area, Rect::new(0, 0, 600, 300));
    assert_eq!(took(&log), ["list hide", "d1 hide", "d2 show"]);
    kernel.run_action(Instant(300), actions::BACK);
    assert_eq!(kernel.panes(), Some((0, 1)));
    assert_eq!(areas(&kernel), [LIST, DETAIL]);
    assert_eq!(took(&log), ["d2 close", "list show", "d1 show"]);
}

/// *Back* closes the detail: the list has all of the width again, and
/// keys are the screen on top's.
#[cfg(feature = "panes")]
#[test]
fn back_closes_the_detail_and_the_list_takes_the_whole_width() {
    let log = log();
    let mut kernel = with_detail(&log);
    kernel.run_action(Instant(200), actions::PANE_OTHER);
    kernel.run_action(Instant(300), FULL);
    // Keys went to the list: what it opens is above both, and keys are
    // the new screen's.
    assert_eq!(kernel.panes(), None);
    assert_eq!(kernel.input.pane_keys, None);
    log.borrow_mut().clear();
    kernel.run_action(Instant(400), actions::BACK);
    kernel.run_action(Instant(500), PING);
    assert_eq!(
        took(&log),
        ["full close", "list show", "d1 show", "d1 ping"]
    );
    kernel.run_action(Instant(600), actions::BACK);
    assert_eq!(areas(&kernel), [Rect::new(0, 0, 600, 300)]);
    assert_eq!(took(&log), ["d1 close", "list resize 600x300", "list show"]);
}

/// Turned to narrow, the two part: the list laid out for all of the
/// screen and hidden below the detail; turned back, beside it again and
/// shown. Plugins hear the display's size once each time.
#[cfg(feature = "panes")]
#[test]
fn turning_narrow_and_back_parts_and_joins_them() {
    let log = log();
    let mut kernel = with_detail(&log);
    kernel.resize(Instant(200), 300, 600);
    assert_eq!(kernel.panes(), None);
    let whole = Rect::new(0, 0, 300, 600);
    assert_eq!(areas(&kernel), [whole, whole]);
    kernel.tick(Instant(250));
    assert_eq!(
        took(&log),
        [
            "list resize 300x600",
            "d1 resize 300x600",
            "list hide",
            "display 300x600 compact false",
            "d1 show",
        ]
    );
    kernel.resize(Instant(300), WIDE.0, WIDE.1);
    kernel.tick(Instant(350));
    assert_eq!(areas(&kernel), [LIST, DETAIL]);
    assert_eq!(
        took(&log),
        [
            "list resize 236x300",
            "d1 resize 363x300",
            "display 600x300 expanded true",
            "list show",
            "d1 show",
        ]
    );
}

/// The list beside the detail runs as the screen on top does: its time
/// comes.
#[cfg(feature = "panes")]
#[test]
fn the_list_beside_the_detail_gets_its_time() {
    let log = log();
    let mut kernel = with_detail(&log);
    kernel.run_action(Instant(200), actions::PANE_OTHER);
    let effects = kernel.run_action(Instant(300), WAKE);
    assert_eq!(effects.wake_at(), Some(Instant(400)));
    kernel.tick(Instant(400));
    assert_eq!(took(&log), ["list timer"]);
}

/// An overlay over the detail takes input; the two stay beside each
/// other under it.
#[cfg(feature = "panes")]
#[test]
fn an_overlay_over_the_detail_takes_input() {
    let log = log();
    let mut kernel = with_detail(&log);
    kernel.run_action(Instant(200), actions::PANE_OTHER);
    kernel.run_action(Instant(300), MENU);
    assert_eq!(kernel.stack.len(), 3);
    assert_eq!(kernel.panes(), Some((0, 1)));
    assert_eq!(areas(&kernel)[..2], [LIST, DETAIL]);
    log.borrow_mut().clear();
    kernel.run_action(Instant(400), PING);
    assert_eq!(took(&log), ["menu ping"]);
}

/// In a right-to-left interface the list's part is on the right.
#[cfg(feature = "panes")]
#[test]
fn right_to_left_the_list_is_on_the_right() {
    let log = log();
    let arabic = "[meta]\nlanguage = ar\nname = العربية\ndirection = rtl\n\n[messages]\np.x = س\n";
    let storage = paperpaw_sdk::MemoryStorage::new()
        .with("plugins/arabic/plugin.ini", "[plugin]\nname = Arabic\n")
        .with("plugins/arabic/i18n/ar.ini", arabic);
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(PanePlugin { log: log.clone() })
        .storage(storage)
        .user_config("[core]\nlanguage = ar\n")
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel.resize(Instant(10), WIDE.0, WIDE.1);
    kernel.run_action(Instant(100), OPEN);
    assert_eq!(
        areas(&kernel),
        [Rect::new(364, 0, 236, 300), Rect::new(0, 0, 363, 300)]
    );
}

/// The class the event carries is the display's, as the SDK reckons it.
#[test]
fn the_display_change_says_the_class() {
    let log = log();
    let mut kernel = on(DEVICE, (480, 300), &log);
    kernel.resize(Instant(100), 480, 300);
    // The same size: nothing to tell.
    kernel.resize(Instant(200), 400, 300);
    let env = kernel.shared.env(Instant(200));
    let class = ScreenClass::of(&env, Rect::new(0, 0, 400, 300));
    assert_eq!(class, ScreenClass::Medium);
    assert_eq!(
        took(&log),
        [
            "list resize 400x300",
            "display 400x300 medium true",
            "list show"
        ]
    );
}

/// Built without panes (the Xteink devices): a detail is a full screen
/// even where the screen is wide, and there is no action to move input
/// between parts.
#[cfg(not(feature = "panes"))]
#[test]
fn without_panes_a_detail_is_a_full_screen_even_where_wide() {
    let log = log();
    let mut kernel = on(DEVICE, WIDE, &log);
    kernel.run_action(Instant(100), OPEN);
    render(&mut kernel);
    let whole = Rect::new(0, 0, WIDE.0, WIDE.1);
    assert_eq!(areas(&kernel), [whole, whole]);
    assert_eq!(kernel.panes(), None);
    assert_eq!(took(&log), ["list hide", "d1 show"]);
    let other = actions::PANE_OTHER;
    assert!(kernel.shared.actions.get(other.as_str()).is_none());
    kernel.run_action(Instant(200), actions::PANE_OTHER);
    assert_eq!(kernel.input_index(), Some(1));
}
