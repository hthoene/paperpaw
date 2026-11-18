//! Menu and icon records through the real install path: admitted whole,
//! refused whole, read back as menu ids, gone with their owner, and
//! allocation-safe. The harness is the parent's linked factory.
#![allow(clippy::unwrap_used, reason = "fixture errors fail tests")]
use super::*;
use crate::native_host::native_menus::MENU_USER_DEFINED;
use crate::native_host::{
    declaration_store::Kind,
    registrations::{self as wire, Record},
};
use alloc::string::String;

/// Submits the menu batch `mode` names as a second batch of the factory.
///
/// # Safety
/// `host` and `ctx` are the live ones `screen_make` received.
#[expect(
    clippy::too_many_lines,
    reason = "one fixture batch per mode, side by side"
)]
pub(super) unsafe fn submit(host: *const PpHost, ctx: *mut c_void, mode: u8) -> i32 {
    let entry = |menu, item| {
        Record::MenuEntry(wire::MenuEntry {
            menu,
            item,
            order: 10,
            place: 0,
            anchor: "",
        })
    };
    let mut bytes = [0; 512];
    let len = match mode {
        // The whole family, once each.
        1 => wire::records_encode(
            &[
                entry("core.tools", "factory.screen"),
                Record::IconSpec(wire::IconSpec {
                    item: "factory.screen",
                    icon: "gear",
                }),
                Record::MenuNeed(wire::MenuNeed {
                    item: "factory.screen",
                    need: 4,
                    capability: 0,
                }),
                Record::MenuSection(wire::MenuSection {
                    name: "tools",
                    title: "factory.tools",
                    menu: "core.main",
                    order: 0,
                }),
                Record::MenuHide(wire::MenuHide {
                    menu: "core.main",
                    item: "other.item",
                }),
                Record::MenuDescribe(wire::MenuDescribe {
                    item: "screen",
                    key: "factory.line",
                }),
            ],
            &mut bytes,
        ),
        // An item of its own that does not exist.
        2 => wire::records_encode(&[entry("core.tools", "factory.missing")], &mut bytes),
        // The same menu and item twice.
        3 => wire::records_encode(
            &[
                entry("core.tools", "factory.screen"),
                entry("core.tools", "factory.screen"),
            ],
            &mut bytes,
        ),
        // Two items into one menu, and a section of that menu.
        4 => wire::records_encode(
            &[
                entry("core.tools", "factory.screen"),
                entry("core.tools", "other.item"),
                Record::MenuSection(wire::MenuSection {
                    name: "tools",
                    title: "factory.tools",
                    menu: "core.tools",
                    order: 0,
                }),
            ],
            &mut bytes,
        ),
        // Another plugin's item, a menu that does not exist yet, an unknown
        // place and an unknown need: all accepted.
        5 => wire::records_encode(
            &[
                Record::MenuEntry(wire::MenuEntry {
                    menu: "later.menu",
                    item: "other.item",
                    order: 0,
                    place: 99,
                    anchor: "",
                }),
                Record::MenuNeed(wire::MenuNeed {
                    item: "other.item",
                    need: 99,
                    capability: 0,
                }),
            ],
            &mut bytes,
        ),
        // One icon twice for the same item.
        6 => wire::records_encode(
            &[
                Record::IconSpec(wire::IconSpec {
                    item: "factory.screen",
                    icon: "gear",
                }),
                Record::IconSpec(wire::IconSpec {
                    item: "factory.screen",
                    icon: "moon",
                }),
            ],
            &mut bytes,
        ),
        // The same need twice for one item (different needs do not matter).
        7 => wire::records_encode(
            &[
                Record::MenuNeed(wire::MenuNeed {
                    item: "factory.screen",
                    need: 4,
                    capability: 0,
                }),
                Record::MenuNeed(wire::MenuNeed {
                    item: "factory.screen",
                    need: 1,
                    capability: 0,
                }),
            ],
            &mut bytes,
        ),
        // The same description twice for one item.
        8 => wire::records_encode(
            &[
                Record::MenuDescribe(wire::MenuDescribe {
                    item: "screen",
                    key: "factory.line",
                }),
                Record::MenuDescribe(wire::MenuDescribe {
                    item: "screen",
                    key: "factory.other",
                }),
            ],
            &mut bytes,
        ),
        // The same (menu, item) hide twice.
        9 => wire::records_encode(
            &[
                Record::MenuHide(wire::MenuHide {
                    menu: "core.main",
                    item: "other.item",
                }),
                Record::MenuHide(wire::MenuHide {
                    menu: "core.main",
                    item: "other.item",
                }),
            ],
            &mut bytes,
        ),
        // The same section name twice, in different menus.
        10 => wire::records_encode(
            &[
                Record::MenuSection(wire::MenuSection {
                    name: "tools",
                    title: "factory.tools",
                    menu: "core.main",
                    order: 0,
                }),
                Record::MenuSection(wire::MenuSection {
                    name: "tools",
                    title: "factory.tools",
                    menu: "core.tools",
                    order: 1,
                }),
            ],
            &mut bytes,
        ),
        // A description of an item of its own that does not exist.
        11 => wire::records_encode(
            &[Record::MenuDescribe(wire::MenuDescribe {
                item: "missing",
                key: "factory.line",
            })],
            &mut bytes,
        ),
        // The input and help declarations: two inputs, a hint for the plugin's
        // own screen and one for another plugin's, with and without a zone.
        20 => wire::records_encode(
            &[
                Record::InputSpec(wire::InputSpec {
                    name: "chord",
                    title: "factory.input.chord",
                    kind: 0,
                    need: 0,
                    capability: 0,
                    flags: 0,
                }),
                Record::InputSpec(wire::InputSpec {
                    name: "shake",
                    title: "factory.input.shake",
                    kind: 1,
                    need: 0,
                    capability: 0,
                    flags: 0,
                }),
                hint("factory.screen", 0, (0, 0, 0, 0)),
                hint("core.reader", 1, (100, 200, 300, 400)),
            ],
            &mut bytes,
        ),
        // A hint whose zone runs off the screen, and one for no full id.
        21 => wire::records_encode(&[hint("factory.screen", 1, (900, 0, 200, 100))], &mut bytes),
        22 => wire::records_encode(&[hint("screen", 0, (0, 0, 0, 0))], &mut bytes),
        _ => return ERR_INVALID,
    }
    .unwrap();
    // SAFETY: the encoded batch stays readable for the synchronous submit.
    unsafe { ((*host).registrations_submit)(ctx, borrowed(&bytes[..len])) }
}

/// A `help.hint` with the zone `(x, y, width, height)`.
fn hint(screen: &str, input: u32, zone: (u32, u32, u32, u32)) -> Record<'_> {
    Record::HelpHint(wire::HelpHint {
        screen,
        input,
        x: zone.0,
        y: zone.1,
        width: zone.2,
        height: zone.3,
        text: "factory.help.hint",
    })
}

/// The menu ids the declarations hold now, with their flags.
fn menu_ids(kernel: &crate::Kernel, custom: &[&str], user_defined: &[&str]) -> Vec<(String, u32)> {
    let Some(declarations) = &kernel.shared.registry.native_declarations else {
        return Vec::new();
    };
    let mut ids = Vec::new();
    declarations
        .menu_ids(
            &mut custom.iter().copied(),
            &|menu| user_defined.contains(&menu),
            &mut |menu, flags| ids.push((String::from(menu), flags)),
        )
        .unwrap();
    ids
}

fn rows(kernel: &crate::Kernel, kind: Kind) -> usize {
    let Some(declarations) = &kernel.shared.registry.native_declarations else {
        return 0;
    };
    let mut count = 0;
    declarations.store.borrow().keys(kind, &mut |_| count += 1);
    count
}

fn fresh() -> crate::Kernel {
    reset(NORMAL);
    DESTROYED.with(|value| value.set(0));
    REPORTS.with(|value| value.set(0));
    crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap()
}

fn install_with(kernel: &mut crate::Kernel, menus: u8) -> Result<(), Error> {
    MENUS.with(|value| value.set(menus));
    let result = install(kernel, 0);
    MENUS.with(|value| value.set(0));
    result
}

#[test]
fn the_whole_menu_family_is_admitted_and_stored_by_kind() {
    let mut kernel = fresh();
    install_with(&mut kernel, 1).unwrap();
    for (kind, count) in [
        (Kind::MenuEntry, 1),
        (Kind::Icon, 1),
        (Kind::MenuNeed, 1),
        (Kind::MenuSection, 1),
        (Kind::MenuHide, 1),
        (Kind::MenuDescribe, 1),
    ] {
        assert_eq!(rows(&kernel, kind), count, "{kind:?}");
    }
    // The screen of the same owner is published as before.
    assert!(
        kernel
            .shared
            .registry
            .screens
            .contains_key("factory.screen")
    );
}

#[test]
fn menu_ids_come_once_each_with_the_users_flag_and_the_users_own_menus() {
    let mut kernel = fresh();
    install_with(&mut kernel, 4).unwrap();
    // `core.tools` is named by two entries and a section, listed once.
    assert_eq!(menu_ids(&kernel, &[], &[]), [("core.tools".into(), 0)]);
    let mut other = fresh();
    install_with(&mut other, 1).unwrap();
    // Entry-row menus, then section and hide menus, each once; then the
    // user's menus no plugin named; bit 5 where the user defined the menu.
    assert_eq!(
        menu_ids(
            &other,
            &["user.empty", "core.main"],
            &["core.main", "user.empty"]
        ),
        [
            ("core.tools".into(), 0),
            ("core.main".into(), 1 << 5),
            ("user.empty".into(), 1 << 5),
        ]
    );
}

#[test]
fn a_refused_seen_list_allocation_fails_the_whole_menu_answer() {
    let mut kernel = fresh();
    install_with(&mut kernel, 4).unwrap();
    let declarations = kernel.shared.registry.native_declarations.as_ref().unwrap();
    let mut reached_success = false;
    // The 1st allocation is the seen-list, the 2nd the copy of the first id.
    for allocation in 1..8 {
        let mut visited = 0_usize;
        arm_failure(allocation);
        let result =
            declarations.menu_ids(&mut ["user.menu"].into_iter(), &|_| false, &mut |_, _| {
                visited += 1
            });
        arm_failure(0);
        match result {
            Ok(()) => {
                // Every allocation the answer needs went through: both menus.
                assert_eq!(visited, 2, "allocation {allocation}");
                reached_success = true;
                break;
            }
            // The caller (op 11) must treat this as failure of the whole answer.
            Err(status) => assert_eq!(status, host_glue::ERR_NO_MEMORY, "allocation {allocation}"),
        }
    }
    assert!(reached_success, "the walk reached a complete answer");
}

#[test]
fn another_plugins_items_unknown_places_and_unknown_needs_are_accepted() {
    let mut kernel = fresh();
    install_with(&mut kernel, 5).unwrap();
    assert_eq!(menu_ids(&kernel, &[], &[]), [("later.menu".into(), 0)]);
}

#[test]
fn the_input_and_help_declarations_are_admitted_stored_and_removed_with_the_owner() {
    let mut kernel = fresh();
    install_with(&mut kernel, 20).unwrap();
    assert_eq!(rows(&kernel, Kind::InputSpec), 2);
    assert_eq!(rows(&kernel, Kind::HelpHint), 2);
    assert!(
        kernel
            .shared
            .registry
            .screens
            .contains_key("factory.screen")
    );
    kernel.unload_plugin(&Id::new_static("factory"));
    assert_eq!(rows(&kernel, Kind::InputSpec), 0);
    assert_eq!(rows(&kernel, Kind::HelpHint), 0);
    // A reinstall starts clean.
    install_with(&mut kernel, 20).unwrap();
    assert_eq!(rows(&kernel, Kind::HelpHint), 2);
}

#[test]
fn a_bad_hint_refuses_the_whole_owner_without_a_trace() {
    for mode in [21, 22] {
        let mut kernel = fresh();
        let result = install_with(&mut kernel, mode);
        // Refused while the factory declares (its submit fails) or when the
        // install maps the declarations: `ERR_INVALID` either way.
        assert!(
            matches!(
                result,
                Err(Error::Status(host_glue::ERR_INVALID) | Error::Factory(host_glue::ERR_INVALID))
            ),
            "mode {mode}: {result:?}"
        );
        assert_eq!(rows(&kernel, Kind::HelpHint), 0, "mode {mode}");
        install_with(&mut kernel, 20).unwrap();
    }
}

#[test]
fn a_bad_menu_batch_refuses_the_whole_owner_and_publishes_nothing() {
    for mode in [2, 3, 6, 7, 8, 9, 10, 11] {
        let mut kernel = fresh();
        let screens = kernel.shared.registry.screens.list().iter().count();
        let result = install_with(&mut kernel, mode);
        assert!(
            matches!(result, Err(Error::Status(host_glue::ERR_INVALID))),
            "mode {mode}: {result:?}"
        );
        assert_eq!(
            kernel.shared.registry.screens.list().iter().count(),
            screens,
            "mode {mode}"
        );
        assert_eq!(rows(&kernel, Kind::MenuEntry), 0, "mode {mode}");
        assert_eq!(rows(&kernel, Kind::Icon), 0, "mode {mode}");
        assert!(menu_ids(&kernel, &[], &[]).is_empty(), "mode {mode}");
        assert_eq!(DESTROYED.with(Cell::get), 1, "mode {mode}");
        // The same kernel installs the good batch afterwards.
        install_with(&mut kernel, 1).unwrap();
    }
}

#[test]
fn unloading_the_owner_removes_its_menu_rows_and_a_reinstall_starts_clean() {
    let mut kernel = fresh();
    install_with(&mut kernel, 1).unwrap();
    assert_eq!(menu_ids(&kernel, &[], &[]).len(), 2);
    kernel.unload_plugin(&Id::new_static("factory"));
    for kind in [
        Kind::MenuEntry,
        Kind::Icon,
        Kind::MenuNeed,
        Kind::MenuSection,
        Kind::MenuHide,
        Kind::MenuDescribe,
    ] {
        assert_eq!(rows(&kernel, kind), 0, "{kind:?}");
    }
    assert!(menu_ids(&kernel, &[], &[]).is_empty());
    install_with(&mut kernel, 1).unwrap();
    assert_eq!(menu_ids(&kernel, &[], &[]).len(), 2);
}

#[test]
fn every_host_allocation_failure_refuses_the_menu_owner_whole() {
    let mut reached_success = false;
    for allocation in 1..64 {
        let mut kernel = fresh();
        let screens = kernel.shared.registry.screens.list().iter().count();
        INSTALL_FAIL_NTH.with(|value| value.set(allocation));
        let result = install_with(&mut kernel, 1);
        arm_failure(0);
        INSTALL_FAIL_NTH.with(|value| value.set(0));
        if result.is_ok() {
            reached_success = true;
            assert_eq!(rows(&kernel, Kind::MenuEntry), 1);
            assert_eq!(menu_ids(&kernel, &[], &[]).len(), 2);
            break;
        }
        assert_eq!(
            kernel.shared.registry.screens.list().iter().count(),
            screens,
            "allocation {allocation}"
        );
        for kind in [Kind::MenuEntry, Kind::Icon, Kind::MenuHide] {
            assert_eq!(rows(&kernel, kind), 0, "allocation {allocation}");
        }
        assert!(menu_ids(&kernel, &[], &[]).is_empty());
    }
    assert!(reached_success, "the walk reached a fully admitted owner");
}

/// What op 11 reports for `menu` now (`None` when it is not listed).
fn menu_flag(kernel: &crate::Kernel, menu: &str) -> Option<u32> {
    let mut found = None;
    kernel
        .native_menu_ids(&mut |id, flags| {
            if id == menu {
                found = Some(flags);
            }
        })
        .unwrap();
    found
}

#[test]
fn op_11_sets_bit_5_for_a_user_defined_menu_in_all_four_states() {
    use paperpaw_sdk::Id;
    let mut kernel = fresh();
    install_with(&mut kernel, 1).unwrap();
    // Declared by a plugin (`core.tools` here), the user has no definition.
    assert_eq!(menu_flag(&kernel, "core.tools"), Some(0));
    // Defined with rows other than the declared ones.
    kernel.set_menu(
        Id::new_static("core.tools"),
        Some(alloc::vec![Id::new_static("core.home")]),
    );
    assert_eq!(menu_flag(&kernel, "core.tools"), Some(MENU_USER_DEFINED));
    // An empty definition shows even where no plugin names the menu.
    kernel.set_menu(Id::new_static("user.empty"), Some(Vec::new()));
    assert_eq!(menu_flag(&kernel, "user.empty"), Some(MENU_USER_DEFINED));
    // A definition equal to the declared rows still counts.
    let declared: Vec<Id> = kernel
        .shared
        .menus
        .contributed_ids("core.main")
        .into_iter()
        .cloned()
        .collect();
    kernel.set_menu(Id::new_static("core.main"), Some(declared));
    assert_eq!(menu_flag(&kernel, "core.main"), Some(MENU_USER_DEFINED));
    // A reset clears bit 5; a menu only the user named is gone.
    kernel.set_menu(Id::new_static("core.tools"), None);
    kernel.set_menu(Id::new_static("user.empty"), None);
    kernel.set_menu(Id::new_static("core.main"), None);
    assert_eq!(menu_flag(&kernel, "core.tools"), Some(0));
    assert_eq!(menu_flag(&kernel, "user.empty"), None);
    assert_eq!(menu_flag(&kernel, "core.main"), Some(0));
}

#[test]
fn op_11_is_a_state_error_before_the_declarations_exist() {
    let kernel = fresh();
    // The store is made by the first native install.
    assert!(kernel.shared.registry.native_declarations.is_none());
    assert_eq!(
        kernel.native_menu_ids(&mut |_, _| panic!("no menu is listed")),
        Err(paperpaw_native::host_glue::ERR_STATE)
    );
}
