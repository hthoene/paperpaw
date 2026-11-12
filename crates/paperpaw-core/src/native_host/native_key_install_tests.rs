//! `key.bind` through the real install path: admitted and bound in the key
//! map, conditioned on a setting, refused whole when malformed, gone with the
//! owner. The harness is the parent's linked factory.
#![allow(clippy::unwrap_used, reason = "fixture errors fail tests")]
use super::*;
use crate::keymap::InputKey;
use crate::native_host::{
    declaration_store::Kind,
    registrations::{self as wire, Record},
};
use alloc::string::String;

/// Where the fixture's modes start (below them are the menu records').
pub(super) const FIRST: u8 = 100;

fn bind<'a>(
    layer: &'a str,
    input: &'a str,
    action: &'a str,
    when: (&'a str, &'a str),
) -> Record<'a> {
    Record::KeyBind(wire::KeyBind {
        layer,
        input,
        action,
        when_key: when.0,
        when_value: when.1,
    })
}

/// Submits the binding batch `mode` names as a second batch of the factory.
///
/// # Safety
/// `host` and `ctx` are the live ones `screen_make` received.
pub(super) unsafe fn submit(host: *const PpHost, ctx: *mut c_void, mode: u8) -> i32 {
    let none = ("", "");
    let mut bytes = [0; 512];
    let len = match mode {
        // The keyboard's: held Back on its own screen ends it, and a global one.
        100 => wire::records_encode(
            &[
                bind("factory.screen", "long.back", "factory.done", none),
                bind("global", "swipe.up", "factory.done", none),
            ],
            &mut bytes,
        ),
        // Bound only while a switch is on (it is off).
        101 => wire::records_encode(
            &[bind(
                "factory.screen",
                "long.menu",
                "factory.done",
                ("core.night-pictures", "true"),
            )],
            &mut bytes,
        ),
        // Explicitly unbound (an empty action).
        102 => wire::records_encode(&[bind("factory.screen", "button.up", "", none)], &mut bytes),
        // A name that is no input, and a zone of the layer's own.
        103 => wire::records_encode(
            &[bind(
                "factory.screen",
                "factory.nonsense",
                "factory.done",
                none,
            )],
            &mut bytes,
        ),
        104 => wire::records_encode(
            &[bind("factory.screen", "tap.mine", "factory.done", none)],
            &mut bytes,
        ),
        // An action and a layer that are no ids, and a condition without a value.
        105 => wire::records_encode(
            &[bind("factory.screen", "long.back", "Not An Action", none)],
            &mut bytes,
        ),
        106 => wire::records_encode(
            &[bind("Not A Layer", "long.back", "factory.done", none)],
            &mut bytes,
        ),
        107 => wire::records_encode(
            &[bind(
                "factory.screen",
                "long.back",
                "factory.done",
                ("core.night-pictures", ""),
            )],
            &mut bytes,
        ),
        // The same binding twice.
        108 => wire::records_encode(
            &[
                bind("factory.screen", "long.back", "factory.done", none),
                bind("factory.screen", "long.back", "factory.other", none),
            ],
            &mut bytes,
        ),
        // Conditioned on a setting nobody declares: never bound.
        109 => wire::records_encode(
            &[bind(
                "factory.screen",
                "long.menu",
                "factory.done",
                ("nobody.declares-this", "true"),
            )],
            &mut bytes,
        ),
        // The global menu input replaced, and another input bound.
        112 => wire::records_encode(
            &[
                bind("global", "button.menu", "factory.done", none),
                bind("global", "swipe.up", "factory.done", none),
            ],
            &mut bytes,
        ),
        // The screen's own layer: a button bound to the plugin's own action.
        113 => wire::records_encode(
            &[bind(
                "factory.screen",
                "button.confirm",
                "factory.action",
                none,
            )],
            &mut bytes,
        ),
        _ => return ERR_INVALID,
    }
    .unwrap();
    // SAFETY: the encoded batch stays readable for the synchronous submit.
    unsafe { ((*host).registrations_submit)(ctx, borrowed(&bytes[..len])) }
}

fn fresh() -> crate::Kernel {
    reset(NORMAL);
    DESTROYED.with(|value| value.set(0));
    REPORTS.with(|value| value.set(0));
    crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap()
}

fn install_with(kernel: &mut crate::Kernel, mode: u8) -> Result<(), Error> {
    MENUS.with(|value| value.set(mode));
    let result = install(kernel, 0);
    MENUS.with(|value| value.set(0));
    result
}

fn rows(kernel: &crate::Kernel) -> usize {
    let Some(declarations) = &kernel.shared.registry.native_declarations else {
        return 0;
    };
    let mut count = 0;
    declarations
        .store
        .borrow()
        .keys(Kind::KeyBind, &mut |_| count += 1);
    count
}

/// What the key map gives `input` on `layer` now.
fn bound(kernel: &crate::Kernel, layer: &str, input: &str) -> Option<String> {
    let input = InputKey::parse(input).unwrap();
    kernel
        .shared
        .keymap
        .resolve(&[layer], &input)
        .map(|id| String::from(id.as_str()))
}

#[test]
fn the_keyboards_held_back_binding_is_admitted_and_reaches_its_screen() {
    let mut kernel = fresh();
    assert_eq!(bound(&kernel, "factory.screen", "long.back"), None);
    install_with(&mut kernel, 100).unwrap();
    assert_eq!(rows(&kernel), 2);
    // The binding is in the kernel's key map, on the screen's own layer and
    // on the global one; the screen of the same owner is published as before.
    assert_eq!(
        bound(&kernel, "factory.screen", "long.back").as_deref(),
        Some("factory.done")
    );
    assert_eq!(
        bound(&kernel, "global", "swipe.up").as_deref(),
        Some("factory.done")
    );
    assert!(
        kernel
            .shared
            .registry
            .screens
            .contains_key("factory.screen")
    );
}

#[test]
fn a_binding_with_a_condition_binds_while_the_setting_holds_it() {
    let mut kernel = fresh();
    install_with(&mut kernel, 101).unwrap();
    assert_eq!(rows(&kernel), 1);
    assert_eq!(bound(&kernel, "factory.screen", "long.menu"), None);
    // A change of that setting builds the key map anew.
    assert!(kernel.native_key_when("core.night-pictures"));
    assert!(!kernel.native_key_when("core.other"));
    assert!(!kernel.native_key_when(""));
    // The kernel's own hook for a changed setting builds it (`config.rs`).
    let id = crate::Id::parse("core.night-pictures").unwrap();
    kernel
        .shared
        .settings
        .set(id.as_str(), paperpaw_sdk::SettingValue::Bool(true))
        .unwrap();
    kernel.setting_changed(&id);
    assert_eq!(
        bound(&kernel, "factory.screen", "long.menu").as_deref(),
        Some("factory.done")
    );
    // And off again: the binding leaves the key map.
    kernel
        .shared
        .settings
        .set(id.as_str(), paperpaw_sdk::SettingValue::Bool(false))
        .unwrap();
    kernel.setting_changed(&id);
    assert_eq!(bound(&kernel, "factory.screen", "long.menu"), None);
    // A change of another setting leaves the key map as it is.
    let other = crate::Id::parse("core.night-mode").unwrap();
    kernel.setting_changed(&other);
    assert_eq!(bound(&kernel, "factory.screen", "long.menu"), None);
}

#[test]
fn a_binding_on_a_setting_nobody_declares_stays_off() {
    let mut kernel = fresh();
    install_with(&mut kernel, 109).unwrap();
    assert_eq!(rows(&kernel), 1);
    kernel.rebuild_keymap();
    assert_eq!(bound(&kernel, "factory.screen", "long.menu"), None);
    // It is a condition on that setting, so a change of it is watched.
    assert!(kernel.native_key_when("nobody.declares-this"));
}

#[test]
fn an_empty_action_is_an_explicit_unbinding() {
    let mut kernel = fresh();
    install_with(&mut kernel, 102).unwrap();
    // The device's own binding is not touched on another layer; here the
    // screen's layer says `none`, which stops the search.
    let input = InputKey::parse("button.up").unwrap();
    assert_eq!(
        kernel
            .shared
            .keymap
            .resolve(&["factory.screen", "global"], &input),
        None
    );
}

#[test]
fn a_malformed_binding_refuses_the_whole_owner_without_a_panic() {
    for mode in [103, 104, 105, 106, 107, 108] {
        let mut kernel = fresh();
        let screens = kernel.shared.registry.screens.list().iter().count();
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
        assert_eq!(
            kernel.shared.registry.screens.list().iter().count(),
            screens,
            "mode {mode}"
        );
        assert_eq!(rows(&kernel), 0, "mode {mode}");
        assert_eq!(bound(&kernel, "factory.screen", "long.back"), None);
        // The same kernel installs the good batch afterwards.
        install_with(&mut kernel, 100).unwrap();
    }
}

#[test]
fn a_global_binding_that_leaves_another_input_to_the_menu_is_bound() {
    let mut kernel = fresh();
    assert_eq!(
        bound(&kernel, "global", "button.menu").as_deref(),
        Some("core.menu")
    );
    install_with(&mut kernel, 112).unwrap();
    // The default key map reaches the menu by touch too, so the button may go.
    assert_eq!(rows(&kernel), 2);
    assert_eq!(
        bound(&kernel, "global", "button.menu").as_deref(),
        Some("factory.done")
    );
    assert_eq!(
        bound(&kernel, "global", "tap.center").as_deref(),
        Some("core.menu")
    );
    assert_eq!(
        bound(&kernel, "global", "swipe.up").as_deref(),
        Some("factory.done")
    );
}

#[test]
fn unloading_the_owner_takes_its_bindings_out_of_the_key_map() {
    let mut kernel = fresh();
    install_with(&mut kernel, 100).unwrap();
    kernel.unload_plugin(&Id::new_static("factory"));
    assert_eq!(rows(&kernel), 0);
    assert_eq!(bound(&kernel, "factory.screen", "long.back"), None);
    // The default key map's binding is back.
    assert_ne!(
        bound(&kernel, "global", "swipe.up").as_deref(),
        Some("factory.done")
    );
    install_with(&mut kernel, 100).unwrap();
    assert_eq!(
        bound(&kernel, "factory.screen", "long.back").as_deref(),
        Some("factory.done")
    );
}

/// A device with buttons and no touch screen (the X4's kind).
const BUTTONS_ONLY: paperpaw_platform::DeviceInfo = paperpaw_platform::DeviceInfo::new(
    "buttons",
    "Buttons only",
    paperpaw_platform::DisplayInfo::new(
        90,
        90,
        100,
        paperpaw_platform::PixelFormat::Gray4,
        paperpaw_platform::Rotation::Deg0,
        None,
    ),
    paperpaw_platform::InputCaps::new(true, false, false, &["back", "confirm", "menu", "up"]),
    paperpaw_platform::MemoryClass::Large,
);

#[test]
fn a_global_binding_that_takes_the_last_menu_input_is_dropped_and_said_once() {
    reset(NORMAL);
    let mut kernel = crate::Kernel::builder(BUTTONS_ONLY).build().unwrap();
    assert_eq!(
        bound(&kernel, "global", "button.menu").as_deref(),
        Some("core.menu")
    );
    install_with(&mut kernel, 112).unwrap();
    // The touch zones that open the menu are no way to it here, so the button
    // stays; the plugin's other binding is made, and both rows are its own.
    assert_eq!(rows(&kernel), 2);
    assert_eq!(
        bound(&kernel, "global", "button.menu").as_deref(),
        Some("core.menu")
    );
    assert_eq!(
        bound(&kernel, "global", "swipe.up").as_deref(),
        Some("factory.done")
    );
    // The drop is the plugin's warning, once however often the map is built.
    kernel.rebuild_keymap();
    kernel.rebuild_keymap();
    let said: Vec<_> = kernel
        .take_warnings()
        .into_iter()
        .filter(|warning| warning.message.contains("no input to the main menu"))
        .collect();
    assert_eq!(said.len(), 1, "{said:?}");
    assert_eq!(
        said[0].source,
        crate::WarningSource::Plugin(Id::new_static("factory"))
    );
    assert!(said[0].message.contains("button.menu"));
    // Unloaded, the plugin's rows are gone and the default's binding is back.
    kernel.unload_plugin(&Id::new_static("factory"));
    assert_eq!(
        bound(&kernel, "global", "swipe.up").as_deref(),
        Some("core.next-page")
    );
}

#[test]
fn every_host_allocation_failure_refuses_the_binding_owner_whole() {
    let mut reached_success = false;
    for allocation in 1..64 {
        let mut kernel = fresh();
        INSTALL_FAIL_NTH.with(|value| value.set(allocation));
        MENUS.with(|value| value.set(100));
        // The admission: the key map is built after it, as the kernel's own
        // refresh is (`register_native_in`).
        let result = kernel.admit_native(
            host(0),
            &FlatDepth::try_new().unwrap(),
            None,
            |_, _, _, _| {},
        );
        MENUS.with(|value| value.set(0));
        arm_failure(0);
        INSTALL_FAIL_NTH.with(|value| value.set(0));
        if result.is_ok() {
            reached_success = true;
            assert_eq!(rows(&kernel), 2);
            break;
        }
        assert_eq!(rows(&kernel), 0, "allocation {allocation}");
        assert_eq!(
            bound(&kernel, "factory.screen", "long.back"),
            None,
            "allocation {allocation}"
        );
    }
    assert!(reached_success, "the walk reached a fully admitted owner");
}
