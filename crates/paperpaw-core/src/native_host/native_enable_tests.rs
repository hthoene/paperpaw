#![allow(clippy::unwrap_used, reason = "fixture errors fail tests")]
extern crate std;
use super::*;
use crate::{Kernel, config::read_config, tests::DEVICE};
use paperpaw_sdk::{
    Id, Manifest, MemoryStorage, PluginKind, PluginState, RequiredFor, Storage, Version,
    native::SafeMode,
};
use std::string::ToString;

fn kernel_with(file: &str, overrides: &str, safe: Option<SafeMode>) -> Kernel {
    let storage = MemoryStorage::new();
    if !file.is_empty() {
        storage.write("paperpaw.ini", file.as_bytes()).unwrap();
    }
    let builder = Kernel::builder(DEVICE)
        .storage(storage)
        .user_config(overrides);
    match safe {
        Some(safe) => builder.safe_mode(safe),
        None => builder,
    }
    .build()
    .unwrap()
}

fn kernel(file: &str, overrides: &str) -> Kernel {
    kernel_with(file, overrides, None)
}

/// Whether the plugin runs, asked as the shell asks: config read once.
fn enabled(k: &mut Kernel, id: &str, default_on: bool) -> bool {
    let config = k.native_config();
    k.native_decide(&config, id, default_on, false).hosts()
}

fn info(id: &'static str, state: PluginState, enabled: bool) -> PluginInfo {
    PluginInfo::new(
        paperpaw_sdk::ManifestRef::try_new(
            Manifest::new(Id::new_static(id), id, Version::new(1, 0, 0)),
            None,
        )
        .unwrap(),
        state,
        enabled,
        RequiredFor::default(),
        PluginKind::BuiltIn,
        None,
        None,
    )
}

#[test]
fn a_plugin_shipped_off_is_off_until_the_configuration_says_on() {
    let mut off = kernel("", "");
    assert!(!enabled(&mut off, "mail", false));
    assert!(enabled(&mut off, "mail", true));
    let mut on = kernel("[plugins]\nmail = on\n", "");
    assert!(enabled(&mut on, "mail", false));
    assert!(!enabled(&mut on, "notes-vault", false));
    // The shell's overrides count as the file does.
    assert!(enabled(
        &mut kernel("", "[plugins]\nmail = on\n"),
        "mail",
        false
    ));
}

#[test]
fn off_in_the_configuration_wins_over_everything() {
    let mut k = kernel("[plugins]\nmail = off\n", "");
    assert!(!enabled(&mut k, "mail", true));
    let mut k = kernel("[plugins]\nmail = on\nmail = off\n", "");
    assert!(!enabled(&mut k, "mail", true));
    // Off in one, on in the other: off.
    let mut k = kernel("[plugins]\nmail = on\n", "[plugins]\nmail = off\n");
    assert!(!enabled(&mut k, "mail", true));
    let mut k = kernel("[plugins]\nmail = off\n", "[plugins]\nmail = on\n");
    assert!(!enabled(&mut k, "mail", false));
    // Another section, another plugin, a value that is neither: nothing said.
    let mut k = kernel(
        "[other]\nmail = off\n[plugins]\nmailx = off\nmail = maybe\n",
        "",
    );
    assert!(enabled(&mut k, "mail", true) && !enabled(&mut k, "mail", false));
}

#[test]
fn a_native_plugin_is_listed_off_or_running_and_not_twice() {
    let mut k = kernel("", "");
    k.list_native(info("mail", PluginState::Off, false))
        .unwrap();
    k.list_native(info("clock", PluginState::Running, true))
        .unwrap();
    let mail = k.plugins().get("mail").unwrap();
    assert_eq!((mail.state, mail.enabled), (PluginState::Off, false));
    assert!(!mail.restart_needed());
    assert_eq!(
        k.list_native(info("mail", PluginState::Off, false)),
        Err(ERR_INVALID)
    );
    assert_eq!(
        k.list_native(info("core", PluginState::Running, true)),
        Err(ERR_DENIED)
    );
}

/// The plugin manager's switch: saved in `[plugins]` for the next start, the
/// row asks for a restart, the next start's decision follows.
#[test]
fn a_switch_is_saved_and_decides_the_next_start() {
    let mut k = kernel("", "");
    k.list_native(info("mail", PluginState::Off, false))
        .unwrap();
    k.list_native(info("clock", PluginState::Running, true))
        .unwrap();
    let id = Id::new_static("mail");
    k.set_plugin_enabled(&id, true);
    assert!(k.plugins().get("mail").unwrap().restart_needed());
    let saved = read_config(k.shared.storage.as_ref()).unwrap().unwrap();
    assert!(saved.contains("mail = on"), "{saved}");
    assert!(enabled(&mut k, "mail", false));
    // And back off, and a running one off: both stay as they run until the
    // restart; the decision for it follows the file.
    k.set_plugin_enabled(&id, false);
    assert!(!k.plugins().get("mail").unwrap().restart_needed());
    assert!(!enabled(&mut k, "mail", true));
    k.set_plugin_enabled(&Id::new_static("clock"), false);
    assert!(k.plugins().get("clock").unwrap().restart_needed());
    assert!(!enabled(&mut k, "clock", true));
}

#[test]
fn a_removed_native_plugin_leaves_the_list() {
    let mut k = kernel("", "");
    k.list_native(info("mail", PluginState::Off, false))
        .unwrap();
    k.list_native(info("clock", PluginState::Running, true))
        .unwrap();
    assert!(k.unload_plugin(&Id::new_static("mail")));
    assert!(k.unload_plugin(&Id::new_static("clock")));
    assert!(k.plugins().get("mail").is_none() && k.plugins().get("clock").is_none());
    // A plugin of the same id may be listed again (an install after a removal).
    k.list_native(info("mail", PluginState::Off, false))
        .unwrap();
}

fn safe(reason: SafeModeReason, crashed: Option<&str>) -> SafeMode {
    let mut safe = SafeMode::new(reason);
    safe.crashed = crashed.map(ToString::to_string);
    safe
}

/// Safe mode, a crash in a known plugin: it is switched off in the file (a
/// built-in one too), the other card plugins are only held back this start,
/// built-in ones run; the next start keeps the culprit off.
#[test]
fn safe_mode_switches_the_crashing_plugin_off_for_good() {
    let mut k = kernel_with(
        "",
        "",
        Some(safe(SafeModeReason::FailedStarts, Some("mail"))),
    );
    let config = k.native_config();
    let d = |k: &mut Kernel, id: &str, card| k.native_decide(&config, id, true, card);
    assert_eq!(d(&mut k, "mail", true), NativeDecision::Crashed);
    assert_eq!(
        d(&mut k, "clock", true),
        NativeDecision::SafeMode { enabled: true }
    );
    assert_eq!(d(&mut k, "stats", false), NativeDecision::Host);
    let file = read_config(k.shared.storage.as_ref()).unwrap().unwrap();
    assert!(
        file.contains("mail = off") && !file.contains("clock"),
        "{file}"
    );
    assert_eq!(
        k.shared.native_safe_mode.as_ref().unwrap().switched_off,
        [Id::new_static("mail")]
    );
    // An essential built-in plugin that crashed is held back this start only:
    // never saved off, so the device cannot be left without the only way to
    // switch it on again but a PC.
    let mut b = kernel_with(
        "",
        "",
        Some(safe(SafeModeReason::FailedStarts, Some("settings"))),
    );
    let config = b.native_config();
    let once = b.native_decide(&config, "settings", true, false);
    assert_eq!(once, NativeDecision::CrashedOnce);
    assert!(!once.hosts() && once.enabled());
    assert_eq!(once.problem(), Some(PluginProblem::Crashed));
    assert_eq!(
        b.native_decide(&config, "clock", true, false),
        NativeDecision::Host
    );
    let kept = read_config(b.shared.storage.as_ref()).ok().flatten();
    assert!(
        kept.as_deref()
            .is_none_or(|file| !file.contains("settings")),
        "{kept:?}"
    );
    assert!(
        b.shared
            .native_safe_mode
            .as_ref()
            .unwrap()
            .switched_off
            .is_empty()
    );
    // The next start, without safe mode, hosts it again.
    let mut again = kernel(kept.as_deref().unwrap_or(""), "");
    assert!(enabled(&mut again, "settings", true));
    // Any other built-in plugin that crashed is off from the next start on
    // (a plugin that crashes at every start must not alternate crash starts
    // and safe mode).
    let mut c = kernel_with(
        "",
        "",
        Some(safe(SafeModeReason::FailedStarts, Some("widgets"))),
    );
    let config = c.native_config();
    assert_eq!(
        c.native_decide(&config, "widgets", true, false),
        NativeDecision::Crashed
    );
    let saved = read_config(c.shared.storage.as_ref()).unwrap().unwrap();
    assert!(saved.contains("widgets = off"), "{saved}");
    let mut after = kernel(&saved, "");
    assert!(!enabled(&mut after, "widgets", true));
    // The next start, without safe mode: the card plugin is still off.
    let mut next = kernel(&file, "");
    assert!(!enabled(&mut next, "mail", true));
    assert!(enabled(&mut next, "clock", true));
}

/// No culprit known (a key held, or no name): every card plugin that was on is
/// switched off and saved; built-in ones run; ones off stay off.
#[test]
fn safe_mode_without_a_culprit_switches_every_card_plugin_off() {
    for reason in [SafeModeReason::KeyHeld, SafeModeReason::FailedStarts] {
        let mut k = kernel_with("[plugins]\nold = off\n", "", Some(safe(reason, None)));
        let config = k.native_config();
        let on = k.native_decide(&config, "clock", true, true);
        assert_eq!(
            on,
            NativeDecision::SafeMode { enabled: false },
            "{reason:?}"
        );
        assert!(!on.enabled() && on.problem() == Some(PluginProblem::SafeMode));
        assert_eq!(
            k.native_decide(&config, "old", true, true),
            NativeDecision::Off
        );
        assert_eq!(
            k.native_decide(&config, "stats", true, false),
            NativeDecision::Host
        );
        let file = read_config(k.shared.storage.as_ref()).unwrap().unwrap();
        assert!(file.contains("clock = off"), "{file}");
    }
}

#[test]
fn the_decision_says_how_to_list_the_plugin() {
    assert_eq!(
        (
            NativeDecision::Host.enabled(),
            NativeDecision::Host.problem()
        ),
        (true, None)
    );
    assert_eq!(
        (NativeDecision::Off.enabled(), NativeDecision::Off.problem()),
        (false, None)
    );
    assert_eq!(
        (
            NativeDecision::Crashed.enabled(),
            NativeDecision::Crashed.problem()
        ),
        (false, Some(PluginProblem::Crashed))
    );
    assert_eq!(
        (
            NativeDecision::CrashedOnce.enabled(),
            NativeDecision::CrashedOnce.problem()
        ),
        (true, Some(PluginProblem::Crashed))
    );
}

/// A linked plugin's manifest kept in the program (#248, B).
static KEPT: Manifest = Manifest::fixed(Id::new_static("kept"), "kept", "", Version::new(1, 0, 0));

/// The row of a linked plugin listed with its manifest kept in the program
/// points there: no copy on the heap (#248, B; Senior M1, #1372).
#[test]
fn a_native_plugin_listed_with_a_kept_manifest_points_to_it() {
    let mut k = kernel("", "");
    let made = Manifest::new(Id::new_static("kept"), "kept", Version::new(1, 0, 0));
    let manifest = paperpaw_sdk::ManifestRef::try_new(made, Some(&KEPT)).unwrap();
    let info = PluginInfo::new(
        manifest,
        PluginState::Running,
        true,
        RequiredFor::default(),
        PluginKind::BuiltIn,
        None,
        None,
    );
    k.list_native(info).unwrap();
    let listed = &k.plugins().get("kept").unwrap().manifest;
    assert!(listed.is_fixed());
    assert!(core::ptr::eq(&**listed, &KEPT));
}

/// A plugin whose manifest's copy the heap has no room for is left out of
/// the list, with a warning, not a stop (#248, B; Senior R1, #1372).
#[test]
fn a_plugin_whose_manifest_has_no_room_is_not_listed() {
    use crate::native_host::shell::tests::FAIL_NTH;
    let mut k = kernel("", "");
    let made = || Manifest::new(Id::new_static("tight"), "tight", Version::new(1, 0, 0));
    FAIL_NTH.with(|n| n.set(1));
    k.add_plugin_info(made(), PluginState::Running, true);
    let left = FAIL_NTH.with(|n| n.replace(0));
    assert_eq!(left, 0);
    assert!(k.plugins().get("tight").is_none());
    // With room, it is listed.
    k.add_plugin_info(made(), PluginState::Running, true);
    assert!(k.plugins().get("tight").is_some());
}

/// A plugin with one action, to see whether anything of it is registered.
struct Acting;

impl paperpaw_sdk::Plugin for Acting {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("acting"), "Acting", Version::new(1, 0, 0))
    }

    fn register(&self, r: &mut paperpaw_sdk::Registrar) -> Result<(), paperpaw_sdk::RegisterError> {
        r.add_action(paperpaw_sdk::ActionSpec::new(
            Id::new_static("acting.go"),
            "Go",
        ))
    }
}

/// A plugin whose row's copy the heap refuses is not loaded at all, rather
/// than registered unlisted, which would let a second copy load or an update
/// install afresh; with room, it then loads once, listed (#248, todo 568).
#[test]
fn a_plugin_without_room_for_its_row_is_not_loaded() {
    use crate::native_host::shell::tests::FAIL_NTH;
    let mut k = kernel("", "");
    let manifest = paperpaw_sdk::Plugin::manifest(&Acting);
    // The first allocation of the registration: the row's copy.
    FAIL_NTH.with(|n| n.set(1));
    k.register_plugin(&Acting, manifest);
    let left = FAIL_NTH.with(|n| n.replace(0));
    assert_eq!(left, 0, "the refusal was used");
    assert!(k.plugins().get("acting").is_none());
    assert!(
        k.actions().get("acting.go").is_none(),
        "loaded without a row"
    );
    k.register_plugin(&Acting, paperpaw_sdk::Plugin::manifest(&Acting));
    assert!(k.plugins().get("acting").is_some());
    assert!(k.actions().get("acting.go").is_some());
}
