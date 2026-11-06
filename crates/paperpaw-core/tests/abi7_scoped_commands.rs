//! Slot-15 SetSetting: whole-batch validation, reserved-setting protection,
//! and readback of the applied value in the next real callback.
#![allow(clippy::unwrap_used, reason = "fixture failures must fail the test")]
use paperpaw_core::native_host::commands::{
    COMMAND_MAX_BYTES, CommandAuthority, CommandScope, CommandStage, WireCommand, commands_encode,
};
use paperpaw_core::native_host::environment::{EnvironmentScope, SettingValueRef, settings_decode};
use paperpaw_native::host_glue::{
    ERR_INVALID, ERR_NO_SPACE, ERR_STATE, ERR_UNSUPPORTED, ERR_UTF8, OK,
};
use paperpaw_platform::{DeviceInfo, DisplayInfo, InputCaps, MemoryClass, PixelFormat, Rotation};
use paperpaw_sdk::{
    Canvas, Command, Commands, Env, Id, Manifest, Plugin, Priority, RegisterError, Registrar,
    Screen, ScreenSpec, SettingExtra, SettingSpec, SettingValue, builtin,
};
use std::{
    cell::{Cell, RefCell},
    pin::pin,
    rc::Rc,
};

const DEVICE: DeviceInfo = DeviceInfo::new(
    "commands-test",
    "Commands fixture",
    DisplayInfo::new(480, 800, 220, PixelFormat::Gray4, Rotation::Deg0, None),
    InputCaps::new(true, false, false, &[]),
    MemoryClass::Small,
);

static SECRET: SettingExtra = SettingExtra::new().secret();
static SETTINGS: [SettingSpec; 5] = [
    SettingSpec::choice(
        "probe.mode",
        "mode",
        &["a", "b"],
        "a",
        builtin::groups::OTHER,
    ),
    SettingSpec::switch("probe.flag", "flag", false, builtin::groups::OTHER),
    SettingSpec::number("probe.number", "number", -1, 1, 0, builtin::groups::OTHER),
    SettingSpec::text("probe.text", "text", "", builtin::groups::OTHER),
    SettingSpec::text("probe.secret", "secret", "", builtin::groups::OTHER).with_extra(&SECRET),
];
static FOREIGN: [SettingSpec; 3] = [
    SettingSpec::switch("other.flag", "flag", false, builtin::groups::OTHER),
    SettingSpec::switch("other.guarded", "guarded", false, builtin::groups::OTHER).guarded(),
    SettingSpec::text("other.secret", "secret", "", builtin::groups::OTHER).with_extra(&SECRET),
];

fn set<'a>(key: &'a str, value: SettingValueRef<'a>) -> WireCommand<'a> {
    WireCommand::SetSetting { key, value }
}

fn batch(commands: &[WireCommand<'_>]) -> Vec<u8> {
    let mut out = vec![0; COMMAND_MAX_BYTES];
    let len = commands_encode(commands, &mut out).unwrap();
    out.truncate(len);
    out
}

fn submit(scope: &CommandScope, bytes: &[u8]) -> i32 {
    // SAFETY: the borrowed slice is live, unmodified and disjoint from host state.
    unsafe { scope.submit(bytes.as_ptr(), bytes.len()) }
}

fn read(env: &Env<'_>, key: &str) -> Vec<u8> {
    let scope = pin!(EnvironmentScope::new());
    let mut out = [0; 64];
    let len = scope
        .as_ref()
        .scope(env, || {
            // SAFETY: both slices are live, exclusive and disjoint from host state.
            unsafe { scope.settings_read(key.as_ptr(), key.len(), out.as_mut_ptr(), out.len()) }
        })
        .unwrap();
    out[..usize::try_from(len).unwrap()].to_vec()
}

/// Every refusal stages nothing; the final staged list proves it.
fn refusals(scope: &CommandScope) {
    let flag = set("probe.flag", SettingValueRef::Bool(true));
    let long = "t".repeat(BYTES);
    let cases: [(Vec<u8>, i32); 13] = [
        // Commands 56-59 decode but this host does not run them yet: the
        // whole batch is refused, the flag before it included.
        (
            batch(&[flag, WireCommand::Redraw { mode: 3 }]),
            ERR_UNSUPPORTED,
        ),
        (
            batch(&[flag, set("other.guarded", SettingValueRef::Bool(true))]),
            ERR_INVALID,
        ),
        // Second-pass refusal: the flag is converted first, then rolled back.
        (
            batch(&[flag, set("probe.mode", SettingValueRef::Text("z"))]),
            ERR_INVALID,
        ),
        (
            batch(&[set("probe.text", SettingValueRef::Text(&long))]),
            ERR_NO_SPACE,
        ),
        (
            batch(&[flag, set("other.secret", SettingValueRef::Text("x"))]),
            ERR_UNSUPPORTED,
        ),
        (
            batch(&[flag, set("probe.none", SettingValueRef::Bool(true))]),
            ERR_INVALID,
        ),
        (
            batch(&[flag, set("probe.flag", SettingValueRef::Int(1))]),
            ERR_INVALID,
        ),
        (
            batch(&[flag, set("probe.number", SettingValueRef::Int(5))]),
            ERR_INVALID,
        ),
        (
            batch(&[flag, set("probe.number", SettingValueRef::Text("1"))]),
            ERR_INVALID,
        ),
        (
            batch(&[flag, set("probe.secret", SettingValueRef::Text("x"))]),
            ERR_UNSUPPORTED,
        ),
        (batch(&[flag, flag, flag]), ERR_NO_SPACE),
        (vec![1, 4, 2, 0, 1], ERR_INVALID),
        (vec![1, 4, 3, 0, 1, 0xff], ERR_UTF8),
    ];
    for (bytes, error) in cases {
        assert_eq!(submit(scope, &bytes), error, "{bytes:?}");
    }
    // SAFETY: a null pointer with nonzero length is refused before any read.
    assert_eq!(unsafe { scope.submit(core::ptr::null(), 1) }, ERR_INVALID);
    assert_eq!(submit(scope, &vec![0; COMMAND_MAX_BYTES + 1]), ERR_NO_SPACE);
}

/// Stage byte budget used by the fixture: keys plus text of staged commands.
/// Tight enough that a byte count wrongly kept after a refused batch would
/// make the later text batch fail.
const BYTES: usize = 40;

/// A panic in the callback clears the slot; afterwards a new scope works.
fn unwinding(owner: &Id, env: &Env<'_>) {
    let scope = pin!(CommandScope::new());
    let mut stage = CommandStage::new(owner, env.settings(), None, 1, BYTES);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        scope
            .as_ref()
            .scope(&mut stage, || panic!("fixture unwind"))
            .unwrap();
    }));
    assert!(result.is_err());
    let flag = batch(&[set("probe.flag", SettingValueRef::Bool(true))]);
    assert_eq!(submit(&scope, &flag), ERR_STATE);
    let status = scope
        .as_ref()
        .scope(&mut stage, || submit(&scope, &flag))
        .unwrap();
    assert_eq!(status, OK);
    assert_eq!(stage.staged().len(), 1);
}

fn started(env: &Env<'_>, out: &mut Commands) {
    let owner = Id::new_static("probe");
    let mut stage = CommandStage::new(&owner, env.settings(), None, 3, BYTES);
    let scope = pin!(CommandScope::new());
    let flag = batch(&[set("probe.flag", SettingValueRef::Bool(true))]);
    assert_eq!(submit(&scope, &flag), ERR_STATE);
    scope
        .as_ref()
        .scope(&mut stage, || {
            assert_eq!(submit(&scope, &flag), OK);
            refusals(&scope);
            let text = batch(&[set("probe.text", SettingValueRef::Text("Grüße"))]);
            assert_eq!(submit(&scope, &text), OK);
            let two = batch(&[set("probe.flag", SettingValueRef::Bool(true)); 2]);
            assert_eq!(submit(&scope, &two), ERR_NO_SPACE);
            let mut nested = CommandStage::new(&owner, env.settings(), None, 1, BYTES);
            assert!(scope.as_ref().scope(&mut nested, || ()).is_err());
        })
        .unwrap();
    assert_eq!(submit(&scope, &flag), ERR_STATE);
    assert_eq!(
        stage.staged(),
        [
            Command::SetSetting(Id::new_static("probe.flag"), SettingValue::Bool(true)),
            Command::SetSetting(
                Id::new_static("probe.text"),
                SettingValue::Text("Grüße".into())
            ),
        ]
    );
    // Nothing is applied before the host drains the stage after the callback.
    assert_eq!(
        settings_decode(&read(env, "probe.flag")),
        Ok(SettingValueRef::Bool(false))
    );
    unwinding(&owner, env);
    out.extend(stage.into_commands());
    cross_plugin_settings(&owner, env, out);
}

fn cross_plugin_settings(owner: &Id, env: &Env<'_>, out: &mut Commands) {
    let mut stage = CommandStage::new(owner, env.settings(), None, 2, BYTES);
    let scope = pin!(CommandScope::new());
    let guarded = batch(&[
        set("other.flag", SettingValueRef::Bool(true)),
        set("other.guarded", SettingValueRef::Bool(true)),
    ]);
    scope
        .as_ref()
        .scope(&mut stage, || {
            assert_eq!(submit(&scope, &guarded), ERR_INVALID);
        })
        .unwrap();
    assert!(stage.staged().is_empty());
    let bytes = batch(&[set("other.flag", SettingValueRef::Bool(true))]);
    scope
        .as_ref()
        .scope(&mut stage, || assert_eq!(submit(&scope, &bytes), OK))
        .unwrap();
    assert_eq!(stage.staged().len(), 1);
    assert!(!env.settings().bool("other.flag"));
    out.extend(stage.into_commands());
}

struct Probe(Rc<RefCell<Vec<String>>>);
impl Plugin for Probe {
    fn manifest(&self) -> Manifest {
        Manifest::new(
            Id::new_static("probe"),
            "Probe",
            paperpaw_sdk::Version::new(1, 0, 0),
        )
    }
    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_settings(&SETTINGS)?;
        r.subscribe(
            builtin::events::STARTED,
            Box::new(|env, _, out| started(env, out)),
        )?;
        let log = self.0.clone();
        r.subscribe(
            builtin::events::SETTING_CHANGED,
            Box::new(move |env, event, _| {
                if event.get(builtin::params::ID) == Some("other.flag") {
                    assert!(env.settings().bool("other.flag"));
                    log.borrow_mut().push("other.flag changed".into());
                    return;
                }
                assert_ne!(event.get(builtin::params::ID), Some("other.guarded"));
                let flag = read(env, "probe.flag");
                let flag = settings_decode(&flag).unwrap();
                let text = read(env, "probe.text");
                log.borrow_mut()
                    .push(format!("{flag:?} {:?}", settings_decode(&text)));
            }),
        )
    }
}

struct Other;
impl Plugin for Other {
    fn manifest(&self) -> Manifest {
        Manifest::new(
            Id::new_static("other"),
            "Other",
            paperpaw_sdk::Version::new(1, 0, 0),
        )
    }
    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_settings(&FOREIGN)
    }
}

#[test]
fn staged_set_setting_is_read_back_in_the_next_callback() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut kernel = paperpaw_core::Kernel::builder(DEVICE)
        .plugin(Probe(log.clone()))
        .plugin(Other)
        .build()
        .unwrap();
    kernel.start(paperpaw_platform::Instant(0));
    assert!(kernel.settings().bool("other.flag"));
    assert!(!kernel.settings().bool("other.guarded"));
    assert_eq!(kernel.settings().text("other.secret"), "");
    assert_eq!(kernel.secrets().get("other.secret"), None);
    let log = log.borrow();
    assert_eq!(
        log.iter()
            .filter(|entry| *entry == "other.flag changed")
            .count(),
        1
    );
    assert!(!log.is_empty(), "no SETTING_CHANGED callback ran");
    assert_eq!(
        log.iter()
            .find(|entry| entry.starts_with("Bool(true)"))
            .unwrap(),
        "Bool(true) Ok(Text(\"Grüße\"))",
        "{log:?}"
    );
}

struct EmptyEditor;
impl Screen for EmptyEditor {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}
    fn on_action(&mut self, _: &Id, _: &mut paperpaw_sdk::Cx<'_, '_>) -> paperpaw_sdk::Handled {
        paperpaw_sdk::Handled::No
    }
}

struct GuardedWriter {
    editor: bool,
    authority: Rc<Cell<Option<CommandAuthority>>>,
    expected: i32,
}
impl Plugin for GuardedWriter {
    fn manifest(&self) -> Manifest {
        let id = if self.editor { "editor" } else { "other" };
        Manifest::new(
            Id::new_static(id),
            "Writer",
            paperpaw_sdk::Version::new(1, 0, 0),
        )
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let owner = self.manifest().id;
        if self.editor {
            let screen = Id::new_static("editor.settings");
            r.add_screen(ScreenSpec::new(
                screen.clone(),
                "Editor",
                Box::new(|_, _| Box::new(EmptyEditor)),
            ))?;
            r.bind_slot(builtin::slots::SETTINGS, screen, Priority::DEFAULT)?;
        } else {
            r.add_settings(&FOREIGN)?;
        }
        let authority = self.authority.clone();
        let expected = self.expected;
        r.subscribe(
            builtin::events::STARTED,
            Box::new(move |env, _, out| {
                let mut stage =
                    CommandStage::new(&owner, env.settings(), authority.get(), 1, BYTES);
                let scope = pin!(CommandScope::new());
                let bytes = batch(&[set("other.guarded", SettingValueRef::Bool(true))]);
                scope
                    .as_ref()
                    .scope(&mut stage, || assert_eq!(submit(&scope, &bytes), expected))
                    .unwrap();
                assert_eq!(stage.staged().len(), usize::from(expected == OK));
                out.extend(stage.into_commands());
            }),
        )
    }
}

fn run_guarded_writer(editor: bool, resolve_authority: bool) {
    let authority = Rc::new(Cell::new(None));
    let expected = if editor && !resolve_authority {
        ERR_INVALID
    } else {
        OK
    };
    let writer = GuardedWriter {
        editor,
        authority: authority.clone(),
        expected,
    };
    let mut builder = paperpaw_core::Kernel::builder(DEVICE);
    if editor {
        builder = builder.plugin(Other);
    }
    let owner = writer.manifest().id;
    let mut kernel = builder.plugin(writer).build().unwrap();
    if resolve_authority {
        authority.set(Some(kernel.command_settings_authority(&owner)));
    }
    kernel.start(paperpaw_platform::Instant(0));
    assert_eq!(kernel.settings().bool("other.guarded"), expected == OK);
}

#[test]
fn the_host_resolved_settings_editor_can_set_foreign_guarded_settings() {
    run_guarded_writer(true, true);
}

#[test]
fn a_plugins_own_guarded_setting_is_admitted() {
    run_guarded_writer(false, false);
}

#[test]
fn no_host_authority_means_no_foreign_guarded_exemption() {
    run_guarded_writer(true, false);
}
