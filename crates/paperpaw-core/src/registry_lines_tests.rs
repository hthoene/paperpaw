//! [`Kernel::registry_lines`]: a kernel with a plugin of the old API (two
//! screens offered for one slot, an action, a subscription) and a widget an
//! interface-7 plugin declared.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
extern crate std;
use crate::native_host::{
    declaration_store::{Allocations, ImageOwner, PendingDeclarations},
    native_declarations::NativeDeclarations,
    registrations::{self as wire, REGISTRATION_MAX_BYTES, Record, records_encode},
    shared::Shared,
};
use alloc::{rc::Rc, string::String, vec, vec::Vec};
use paperpaw_sdk::builtin::{events, slots};
use paperpaw_sdk::keys::KeyBindings as _;
use paperpaw_sdk::{Id, Manifest, Priority, Version};

struct Blank;
impl crate::testing::Fake for Blank {
    fn draw(&mut self, _: &mut paperpaw_sdk::Canvas<'_>, _: &crate::Place<'_>) {}
    fn on_action(&mut self, _: &Id, _: &mut crate::Turn<'_, '_>) -> paperpaw_sdk::Handled {
        paperpaw_sdk::Handled::No
    }
}

const LOW: Id = Id::new_static("shelf.low");
const HIGH: Id = Id::new_static("shelf.high");
const TIDY: Id = Id::new_static("shelf.tidy");
const SORT: Id = Id::new_static("shelf.sort");
const MENU: Id = Id::new_static("shelf.menu");

fn shelf(r: &mut crate::Items) -> Result<(), paperpaw_sdk::RegisterError> {
    for id in [LOW, HIGH] {
        r.add_fake(crate::testing::FakeSpec::new(
            id,
            "shelf.screen",
            alloc::boxed::Box::new(|_, _| alloc::boxed::Box::new(Blank)),
        ))?;
    }
    // The lower one is offered first: the order tried is by priority.
    r.bind_slot(slots::HOME, LOW, Priority::DEFAULT)?;
    r.bind_slot(slots::HOME, HIGH, Priority::PREFERRED)?;
    r.subscribe(events::BOOK_CLOSED, |_, _| {})?;
    r.add_action(crate::kit::action::ActionSpec::new(TIDY, "shelf.tidy"))?;
    r.add_action(crate::kit::action::ActionSpec::new(SORT, "shelf.sort"))?;
    // Entered against their order: a menu's rows stand by order.
    r.add_menu_entry(crate::kit::menus::MenuEntry::new(MENU, TIDY).order(30))?;
    r.add_menu_entry(crate::kit::menus::MenuEntry::new(MENU, SORT).order(10))?;
    // Two layers of its own, the later name first.
    r.add_keymap(
        "[keymap.shelf.zeta]\ntap.center = shelf.tidy\n[keymap.shelf.alpha]\nswipe.up = shelf.sort\n",
    )
}

/// The old-API side of `clock`: the action its natively declared menu row
/// names.
fn clock(r: &mut crate::Items) -> Result<(), paperpaw_sdk::RegisterError> {
    r.add_action(crate::kit::action::ActionSpec::new(
        Id::new_static("clock.go"),
        "clock.go",
    ))
}

struct Declarer;
impl ImageOwner for Declarer {
    fn plugin_id(&self) -> &str {
        "clock"
    }
    fn generation(&self) -> u32 {
        1
    }
    fn owner_id(&self) -> Result<u64, i32> {
        Ok(7)
    }
    fn with_image(&self, visit: &mut dyn FnMut(Option<&[u8]>)) {
        visit(Some(&[]));
    }
    fn invalid_declaration(&self, _: i32) {}
}

fn kernel() -> crate::Kernel {
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .items(
            Manifest::new(Id::new_static("shelf"), "Shelf", Version::new(1, 0, 0)),
            shelf,
        )
        .items(
            Manifest::new(Id::new_static("clock"), "Clock", Version::new(1, 0, 0)),
            clock,
        )
        .build()
        .unwrap();
    let directory = Shared::try_new(NativeDeclarations::default()).unwrap();
    let record = Record::WidgetSpec(wire::WidgetSpec {
        name: "face",
        title: "clock.face",
        category: "time",
        previews: "",
        cells: 0,
        token: 3,
        flags: 0,
    });
    // A row of `shelf`'s menu, between its own two by its order.
    let row = Record::MenuEntry(wire::MenuEntry {
        menu: "shelf.menu",
        item: "clock.go",
        order: 20,
        place: 0,
        anchor: "",
    });
    let mut bytes = vec![0; REGISTRATION_MAX_BYTES];
    let len = records_encode(&[record, row], &mut bytes).unwrap();
    let mut allocations = Allocations::default();
    let source: Rc<dyn ImageOwner> = Rc::new(Declarer);
    let mut pending = PendingDeclarations::new(Id::new_static("clock"), source);
    pending
        .stage_batch(&bytes[..len], &mut allocations)
        .unwrap();
    directory
        .store
        .borrow_mut()
        .prepare(pending, &mut allocations)
        .unwrap()
        .commit();
    kernel.shared.registry.native_declarations = Some(directory);
    kernel
}

/// The lines of `part`, without it.
fn part<'a>(lines: &'a [String], part: &str) -> Vec<&'a str> {
    lines
        .iter()
        .filter_map(|line| line.strip_prefix(part)?.strip_prefix(' '))
        .collect()
}

#[test]
fn a_slots_screens_stand_in_the_order_they_are_tried_not_in_the_lines_order() {
    let lines = kernel().registry_lines();
    // As offered, with the priorities (the kernel's own placeholder first,
    // below every plugin's); then as tried: the higher first, though
    // `shelf.high` sorts before `shelf.low` only by chance of its name.
    assert_eq!(
        part(&lines, "slot-offer core.home"),
        [
            "| core.placeholder Priority(-32768)",
            "| shelf.low Priority(0)",
            "| shelf.high Priority(100)"
        ]
    );
    assert_eq!(
        part(&lines, "slot-order core.home"),
        ["| shelf.high", "| shelf.low", "| core.placeholder"]
    );
    // The list of the slot's screens is code of its own, in the same order.
    assert_eq!(
        part(&lines, "slot-list core.home"),
        part(&lines, "slot-order core.home")
    );
    assert!(
        part(&lines, "slot core.home")
            .iter()
            .any(|line| line.contains("bound=Some(\"shelf.high\")")),
        "{lines:#?}"
    );
    // The subscribers in the order they hear the event: the kernel's own
    // plugin first.
    assert_eq!(
        part(&lines, "subscription core.book-closed"),
        ["| core", "| shelf"]
    );
    assert!(part(&lines, "screen shelf.low")[0].contains("title=shelf.screen"));
    assert!(
        lines
            .iter()
            .any(|line| line.starts_with("plugin | shelf 1.0.0 "))
    );
}

#[test]
fn a_menus_rows_of_both_kinds_stand_in_the_one_order_a_view_gets() {
    let lines = kernel().registry_lines();
    let items: Vec<&str> = part(&lines, "menu-merged shelf.menu")
        .into_iter()
        .filter_map(|line| line.split(' ').nth(1))
        .collect();
    assert_eq!(
        items,
        ["shelf.sort", "clock.go", "shelf.tidy"],
        "{lines:#?}"
    );
}

#[test]
fn the_key_map_layers_stand_as_the_kernel_walks_them() {
    let kernel = kernel();
    let lines = kernel.registry_lines();
    let mut walked = Vec::new();
    let _ = kernel.shared.keymap.each_layer(&mut |layer| {
        walked.push(alloc::format!("| {layer}"));
        core::ops::ControlFlow::Continue(())
    });
    assert!(walked.len() >= 3, "{walked:?}");
    assert_eq!(part(&lines, "key-layer"), walked);
}

#[test]
fn what_an_interface_7_plugin_declared_is_listed_with_its_plugin() {
    let lines = kernel().registry_lines();
    let widgets = part(&lines, "native Widget clock");
    assert_eq!(widgets.len(), 1, "{lines:#?}");
    assert!(widgets[0].contains("name: \"face\""), "{}", widgets[0]);
    assert!(widgets[0].contains("category: \"time\""), "{}", widgets[0]);
}

#[test]
fn the_lines_of_a_part_are_sorted_by_their_key_alone() {
    let lines = kernel().registry_lines();
    let keys: Vec<&str> = lines
        .iter()
        .filter(|line| line.starts_with("action "))
        .map(|line| line.split(" | ").next().unwrap())
        .collect();
    assert!(keys.len() > 3, "the kernel's own actions");
    assert!(keys.windows(2).all(|pair| pair[0] <= pair[1]), "{keys:#?}");
    // A long value is cut to its start, its length and its hash.
    assert!(lines.iter().all(|line| line.len() < 700), "a line too long");
}
