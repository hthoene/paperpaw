//! `command.set_menu` and `command.bind_key` through the command stage
//! (`registry.md` 4.2, #455): staged for the kernel (which saves them in
//! `paperpaw.ini` as in interface 6) from any plugin, also a plugin file on
//! the card without `uses.registry` (owner decision 291); an unknown menu or
//! item, a submenu cycle, and a batch that leaves the global layer no input to
//! the main menu refused whole.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
use super::*;
use crate::kit::menus::MenuEntry;
use paperpaw_sdk::builtin::actions;

fn submit(stage: &mut CommandStage<'_>, commands: &[WireCommand<'_>]) -> i32 {
    let mut bytes = [0; 512];
    let len = commands_encode(commands, &mut bytes).unwrap();
    stage.stage(&commands_decode(&bytes[..len]).unwrap())
}

fn menu<'a>(menu: &'a str, items: &'a str) -> WireCommand<'a> {
    WireCommand::SetMenu {
        menu,
        items,
        flags: 0,
    }
}

fn bind<'a>(input: &'a str, action: &'a str, flags: u32) -> WireCommand<'a> {
    WireCommand::BindKey {
        layer: "global",
        input,
        action,
        flags,
    }
}

/// A kernel with the menu `probe.a`, which holds Back and the submenu
/// `probe.b`, which holds Back.
fn kernel() -> crate::Kernel {
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let a = Id::new_static("probe.a");
    let b = Id::new_static("probe.b");
    kernel
        .shared
        .menus
        .add(MenuEntry::new(a.clone(), actions::BACK));
    kernel
        .shared
        .menus
        .add(MenuEntry::new(a, b.clone()).order(1));
    kernel.shared.menus.add(MenuEntry::new(b, actions::BACK));
    kernel
}

/// The stage of a plugin file on the card: no `uses.registry`, which such a
/// plugin is never given.
fn stage<'a>(owner: &'a Id, env: &'a crate::Place<'a>) -> CommandStage<'a> {
    CommandStage::new(owner, env.settings(), None, 16, 4096).with_ui(
        env,
        Some(paperpaw_sdk::ScreenKey::new(7)),
        0,
        100,
    )
}

#[test]
fn a_card_plugin_without_uses_registry_saves_menus_and_keys() {
    let kernel = kernel();
    let env = kernel.shared.place(paperpaw_platform::Instant(0));
    let owner = Id::new_static("menu-editor");
    let mut stage = stage(&owner, &env);
    let commands = [
        menu("probe.a", "probe.b\ncore.back"),
        WireCommand::SetMenu {
            menu: "probe.b",
            items: "",
            flags: 1,
        },
        bind("button.menu", "core.back", 0),
        bind("button.back", "", 0),
        bind("button.menu", "", 1),
    ];
    assert_eq!(submit(&mut stage, &commands), OK);
    let staged = stage.staged();
    assert!(
        matches!(&staged[0], Ask::SetMenu(menu, Some(items))
            if menu.as_str() == "probe.a"
                && items.iter().map(Id::as_str).eq(["probe.b", "core.back"])),
        "{staged:?}"
    );
    assert!(
        matches!(&staged[1], Ask::SetMenu(menu, None) if menu.as_str() == "probe.b"),
        "{staged:?}"
    );
    let binds: alloc::vec::Vec<_> = staged[2..]
        .iter()
        .map(|ask| match ask {
            Ask::Bind {
                layer,
                input,
                action,
            } => (
                layer.as_str(),
                input.as_str(),
                action.as_ref().map(Id::as_str),
            ),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(
        binds,
        [
            ("global", "button.menu", Some("core.back")),
            // Empty binds nothing (interface 6 `NOTHING`), a reset none.
            ("global", "button.back", Some(actions::NOTHING.as_str())),
            ("global", "button.menu", None),
        ]
    );
}

#[test]
fn unknown_ids_cycles_and_bad_flags_refuse_the_batch() {
    let kernel = kernel();
    let env = kernel.shared.place(paperpaw_platform::Instant(0));
    let owner = Id::new_static("menu-editor");
    let mut stage = stage(&owner, &env);
    for batch in [
        // A menu, an item the device does not know.
        &[menu("probe.none", "core.back")][..],
        &[menu("probe.a", "probe.unknown")],
        // `probe.b` in `probe.a`, `probe.a` in `probe.b`: a cycle, also when
        // a batch makes it in two steps; a menu in itself.
        &[menu("probe.b", "probe.a")],
        &[
            menu("probe.a", "core.back"),
            menu("probe.b", "probe.a"),
            menu("probe.a", "probe.b"),
        ],
        &[menu("probe.a", "probe.a")],
        // A reset with items, an unknown flag.
        &[WireCommand::SetMenu {
            menu: "probe.a",
            items: "core.back",
            flags: 1,
        }],
        &[WireCommand::SetMenu {
            menu: "probe.a",
            items: "",
            flags: 2,
        }],
        &[bind("button.back", "core.back", 1)],
        &[bind("button.back", "core.back", 2)],
        &[bind("no-input", "core.back", 0)],
    ] {
        assert_eq!(submit(&mut stage, batch), ERR_INVALID, "{batch:?}");
    }
    assert!(stage.staged().is_empty());
    // Taking `probe.b` out of `probe.a` first breaks the cycle.
    assert_eq!(
        submit(
            &mut stage,
            &[menu("probe.a", "core.back"), menu("probe.b", "probe.a")]
        ),
        OK
    );
}

/// Global `bind_key`s that unbind `inputs`.
fn away(inputs: &[String]) -> alloc::vec::Vec<WireCommand<'_>> {
    inputs.iter().map(|input| bind(input, "", 0)).collect()
}

#[test]
fn a_batch_that_binds_away_the_last_input_to_the_main_menu_is_denied() {
    let kernel = kernel();
    let env = kernel.shared.place(paperpaw_platform::Instant(0));
    let owner = Id::new_static("keys");
    let mut stage = stage(&owner, &env);
    // Every global input to the main menu this device has (buttons, touch).
    let menu_inputs: alloc::vec::Vec<String> = env
        .keys()
        .bindings("global")
        .into_iter()
        .filter(|(_, action, _)| action.as_ref().is_some_and(|id| id.as_str() == "core.menu"))
        .map(|(input, ..)| input)
        .collect();
    assert!(menu_inputs.len() > 1, "{menu_inputs:?}");
    assert_eq!(submit(&mut stage, &away(&menu_inputs)), ERR_DENIED);
    // One left, or another input bound to the menu in the same batch: kept.
    assert_eq!(
        submit(&mut stage, &away(&menu_inputs[1..])),
        OK,
        "{menu_inputs:?}"
    );
    let mut moved = away(&menu_inputs);
    moved.push(bind("long.back", "core.menu", 0));
    assert_eq!(submit(&mut stage, &moved), OK);
    // Screen layers are not the global one.
    let screen: alloc::vec::Vec<_> = menu_inputs
        .iter()
        .map(|input| WireCommand::BindKey {
            layer: "reader.page",
            input,
            action: "",
            flags: 0,
        })
        .collect();
    assert_eq!(submit(&mut stage, &screen), OK);
}
