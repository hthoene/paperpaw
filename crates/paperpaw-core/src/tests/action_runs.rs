//! Actions: run with params, handlers put in place of
//! others', what runs by itself, observers of actions and events, the
//! value a setting had.

use super::*;
use crate::Items;
use paperpaw_sdk::builtin::events;
use paperpaw_sdk::{ObserverSpec, Params};

const APPLY: Id = Id::new_static("obs.apply");
const PLAIN: Id = Id::new_static("obs.plain");
const SCREEN_ONLY: Id = Id::new_static("obs.screen-only");
const WRAPPED: Id = Id::new_static("obs.wrapped");
const LOCK: Id = Id::new_static("g.lock");

/// Actions of its own, two handlers in place of one of them, and
/// observers of actions and of settings changed.
fn observing(kept: Log) -> impl Fn(&mut Items) -> Result<(), RegisterError> {
    move |r| observing_items(&kept, r)
}

/// What [`observing`] adds (a function of its own for its length).
fn observing_items(kept: &Log, r: &mut Items) -> Result<(), RegisterError> {
    let log = kept.clone();
    r.add_action(
        ActionSpec::new(APPLY, "obs.apply").handler(move |_, params, _| {
            let profile = params.get("profile").unwrap_or("none");
            log.borrow_mut().push(alloc::format!("apply {profile}"));
        }),
    )?;
    let log = kept.clone();
    r.add_action(
        ActionSpec::new(PLAIN, "obs.plain")
            .describe("obs.plain.help")
            .keywords("obs.plain.words")
            .handler(move |_, _, _| log.borrow_mut().push(String::from("plain"))),
    )?;
    r.add_action(ActionSpec::new(SCREEN_ONLY, "obs.screen-only"))?;
    for (priority, name) in [(1, "low"), (5, "high")] {
        let log = kept.clone();
        r.override_action(
            Id::new_static("obs.overridden"),
            move |_, _, _| log.borrow_mut().push(String::from(name)),
            Priority(priority),
        )?;
    }
    r.add_action(ActionSpec::new(
        Id::new_static("obs.overridden"),
        "obs.overridden",
    ))?;
    // An action of its own, wrapped by a handler in its place that
    // runs the original, tries to run itself and runs another.
    let log = kept.clone();
    r.add_action(
        ActionSpec::new(WRAPPED, "obs.wrapped")
            .handler(move |_, _, _| log.borrow_mut().push(String::from("inner"))),
    )?;
    let log = kept.clone();
    r.override_action(
        WRAPPED,
        move |_, params, out| {
            log.borrow_mut().push(String::from("outer"));
            out.push(Command::RunOriginal(WRAPPED, params.clone()));
            out.push(Command::run(WRAPPED));
            out.push(Command::run(PLAIN));
        },
        Priority::DEFAULT,
    )?;
    // Opens a screen of its own; a step through profiles, run again
    // with the next.
    r.add_layer(crate::LayerSpec::new(
        Id::new_static("obs.page"),
        "page",
        Box::new(|_, _| Box::new(Lock)),
    ))?;
    r.add_action(
        ActionSpec::new(Id::new_static("obs.open"), "obs.open").handler(|_, _, out| {
            out.push(Command::OpenScreen(
                Id::new_static("obs.page"),
                Params::new(),
            ));
        }),
    )?;
    let log = kept.clone();
    r.add_action(
        ActionSpec::new(Id::new_static("obs.step"), "obs.step").handler(move |_, params, out| {
            let n: u8 = params.get("n").and_then(|n| n.parse().ok()).unwrap_or(0);
            log.borrow_mut().push(alloc::format!("step {n}"));
            if n < 2 {
                let next = Params::new().with("n", alloc::format!("{}", n + 1));
                out.push(Command::RunAction(Id::new_static("obs.step"), next));
            }
            // The same again: dropped.
            out.push(Command::RunAction(
                Id::new_static("obs.step"),
                params.clone(),
            ));
        }),
    )?;
    let log = kept.clone();
    r.add_action(
        ActionSpec::new(Id::new_static("obs.sealed"), "obs.sealed")
            .handler(move |_, _, _| log.borrow_mut().push(String::from("sealed")))
            .sealed(),
    )?;
    // In place of another plugin's action, which its screen takes.
    let log = kept.clone();
    r.override_action(
        Id::new_static("a.special"),
        move |_, _, _| log.borrow_mut().push(String::from("obs special")),
        Priority::DEFAULT,
    )?;
    let log = kept.clone();
    r.observe_actions(ObserverSpec::new(
        Id::new_static("obs.actions"),
        move |_, id, _, _| log.borrow_mut().push(alloc::format!("saw {id}")),
    ))?;
    let log = kept.clone();
    r.observe_events(ObserverSpec::new(
        Id::new_static("obs.events"),
        move |_, id, params, _| {
            if *id == events::SETTING_CHANGED {
                let old = params.get("old").unwrap_or("?");
                log.borrow_mut().push(alloc::format!("changed from {old}"));
            }
        },
    ))
}

/// What [`observing`] registers under.
fn observing_manifest() -> Manifest {
    Manifest::new(Id::new_static("obs"), "Observing", Version::new(1, 0, 0))
        .uses(paperpaw_sdk::Uses::NOTHING)
}

fn observed() -> (Kernel, Log) {
    let (kernel, log, _) = observed_with_screen();
    (kernel, log)
}

/// A kernel with plugin `a`'s screen and its log, and the observing
/// plugin's log.
fn observed_with_screen() -> (Kernel, Log, Log) {
    let (log, screen) = (log(), log());
    let mut kernel = Kernel::builder(DEVICE)
        .test_plugin(TestPlugin::new("a", &screen))
        .items(observing_manifest(), observing(log.clone()))
        .items(guarding_manifest(), guarding(log.clone()))
        .build()
        .unwrap();
    kernel.start(Instant(0));
    log.borrow_mut().clear();
    screen.borrow_mut().clear();
    (kernel, log, screen)
}

struct Lock;

/// What passes the test lock, as sleep passes the real one: it takes
/// every other action itself.
const PASSING: [&str; 4] = ["obs.overridden", "obs.wrapped", "obs.guarded", "obs.open"];

impl crate::Layer for Lock {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}

    fn guards(&self) -> bool {
        true
    }

    fn on_action(&mut self, action: &Id, _: &mut Cx<'_, '_>) -> Handled {
        if PASSING.contains(&action.as_str()) {
            Handled::No
        } else {
            Handled::Yes
        }
    }
}

/// A lock that takes no action itself, a handler of its own plugin in
/// place of `obs.plain`'s, and an observer of actions.
fn guarding(kept: Log) -> impl Fn(&mut Items) -> Result<(), RegisterError> {
    move |r| {
        r.add_layer(crate::LayerSpec::new(
            LOCK,
            "lock",
            Box::new(|_, _| Box::new(Lock)),
        ))?;
        let log = kept.clone();
        r.override_action(
            Id::new_static("obs.guarded"),
            move |_, _, _| log.borrow_mut().push(String::from("g guarded")),
            Priority::DEFAULT,
        )?;
        // In place of a sealed action: refused when the kernel takes it.
        r.override_action(Id::new_static("obs.sealed"), |_, _, _| {}, Priority(50))?;
        let log = kept.clone();
        r.observe_actions(ObserverSpec::new(
            Id::new_static("g.actions"),
            move |_, id, _, _| log.borrow_mut().push(alloc::format!("g saw {id}")),
        ))
    }
}

/// What [`guarding`] registers under.
fn guarding_manifest() -> Manifest {
    Manifest::new(Id::new_static("g"), "Guarding", Version::new(1, 0, 0))
        .uses(paperpaw_sdk::Uses::NOTHING)
}

/// While a screen guards the device, handlers another plugin put in
/// place of an action's own do not run (the action's own does), the
/// guard's plugin's do; only its observers see the actions.
#[test]
fn a_guard_keeps_others_overrides_and_observers_out() {
    let (mut kernel, log, _) = observed_with_screen();
    kernel.open_screen(Instant(10), LOCK, Params::new());
    log.borrow_mut().clear();
    kernel.apply(Command::run(Id::new_static("obs.overridden")));
    kernel.apply(Command::run(WRAPPED));
    kernel.apply(Command::run(Id::new_static("obs.guarded")));
    kernel.finish();
    assert_eq!(
        *log.borrow(),
        [
            "g saw obs.overridden",
            "g saw obs.wrapped",
            "inner",
            "g saw obs.guarded",
            "g guarded"
        ]
    );
}

/// A screen that takes an action comes before a handler in its place;
/// one run with params reaches the screen too.
#[test]
fn a_screen_comes_before_an_override() {
    let (mut kernel, log, screen) = observed_with_screen();
    let special = Id::new_static("a.special");
    kernel.apply(Command::RunAction(
        special.clone(),
        Params::new().with("n", "1"),
    ));
    kernel.finish();
    assert_eq!(*screen.borrow(), ["a:a.special"]);
    assert!(
        !log.borrow().contains(&String::from("obs special")),
        "{:?}",
        log.borrow()
    );
}

/// A handler in place of an action's own runs the original; what runs
/// the action itself again is dropped, with a warning, and nothing else.
#[test]
fn an_override_runs_the_original_and_never_itself() {
    let (mut kernel, log) = observed();
    kernel.apply(Command::run(WRAPPED));
    kernel.finish();
    assert_eq!(
        *log.borrow(),
        [
            "saw obs.wrapped",
            "g saw obs.wrapped",
            "outer",
            "inner",
            "saw obs.plain",
            "g saw obs.plain",
            "plain"
        ]
    );
    let warnings = kernel.take_warnings();
    assert!(
        warnings.iter().any(|w| w.message.contains("runs itself")),
        "{warnings:?}"
    );
}

/// A handler made for params gets those it is run with, none when run
/// plainly; observers see each action first.
#[test]
fn actions_run_with_params() {
    let (mut kernel, log) = observed();
    let params = Params::new().with("profile", "night");
    kernel.apply(Command::RunAction(APPLY, params));
    kernel.apply(Command::run(APPLY));
    kernel.apply(Command::run(PLAIN));
    kernel.finish();
    assert_eq!(
        *log.borrow(),
        [
            "saw obs.apply",
            "g saw obs.apply",
            "apply night",
            "saw obs.apply",
            "g saw obs.apply",
            "apply none",
            "saw obs.plain",
            "g saw obs.plain",
            "plain"
        ]
    );
}

/// Of the handlers put in place of an action's own, the one of the
/// highest priority runs.
#[test]
fn the_override_of_the_highest_priority_runs() {
    let (mut kernel, log) = observed();
    kernel.apply(Command::run(Id::new_static("obs.overridden")));
    kernel.finish();
    assert_eq!(
        *log.borrow(),
        ["saw obs.overridden", "g saw obs.overridden", "high"]
    );
}

/// What runs by itself is told from screen actions; descriptions and
/// keywords are kept.
#[test]
fn runnable_actions_are_told_from_screen_actions() {
    let (kernel, _) = observed();
    let actions = &kernel.shared.actions;
    assert!(actions.runnable(PLAIN.as_str()));
    assert!(actions.runnable(APPLY.as_str()));
    assert!(actions.runnable("obs.overridden"), "by its override");
    assert!(!actions.runnable(SCREEN_ONLY.as_str()));
    assert!(!actions.runnable("obs.nothing"));
    let plain = actions.get(PLAIN.as_str()).unwrap();
    assert_eq!(plain.description(), Some("obs.plain.help"));
    assert_eq!(plain.search_keywords(), Some("obs.plain.words"));
}

/// An observer of events hears a setting change with the value it had.
#[test]
fn a_setting_changed_says_what_it_was() {
    let (mut kernel, log) = observed();
    let level = Id::new_static("a.level");
    kernel.apply(Command::SetSetting(level.clone(), SettingValue::Int(4)));
    kernel.finish();
    kernel.apply(Command::ResetSetting(level));
    kernel.finish();
    assert_eq!(*log.borrow(), ["changed from 2", "changed from 4"]);
}

/// The original runs only from a handler in its place: asked for by any
/// other (a timer, a rule), it is refused, also while a screen guards
/// the device, where it would open what the guard keeps closed.
#[test]
fn the_original_runs_only_from_its_override() {
    let (mut kernel, log, _) = observed_with_screen();
    kernel.apply(Command::RunOriginal(WRAPPED, Params::new()));
    kernel.finish();
    assert!(
        !log.borrow().contains(&String::from("inner")),
        "{:?}",
        log.borrow()
    );
    kernel.open_screen(Instant(10), LOCK, Params::new());
    kernel.apply(Command::RunOriginal(
        Id::new_static("obs.open"),
        Params::new(),
    ));
    kernel.finish();
    assert_eq!(kernel.active_screen(), Some(&LOCK));
    let warnings = kernel.take_warnings();
    assert!(
        warnings
            .iter()
            .any(|w| w.message.contains("own handler runs only")),
        "{warnings:?}"
    );
}

/// An action the lock takes runs no handler; one it lets pass opens
/// above it.
#[test]
fn the_lock_takes_what_it_does_not_let_pass() {
    let (mut kernel, log, _) = observed_with_screen();
    kernel.open_screen(Instant(10), LOCK, Params::new());
    log.borrow_mut().clear();
    kernel.run_action(Instant(20), PLAIN);
    assert!(
        !log.borrow().contains(&String::from("plain")),
        "{:?}",
        log.borrow()
    );
    kernel.run_action(Instant(30), Id::new_static("obs.open"));
    assert_eq!(kernel.active_screen(), Some(&Id::new_static("obs.page")));
}

/// An action run again with other params runs (stepping through
/// profiles); with the same params it is dropped.
#[test]
fn an_action_runs_again_with_other_params() {
    let (mut kernel, log) = observed();
    kernel.apply(Command::run(Id::new_static("obs.step")));
    kernel.finish();
    let log = log.borrow();
    let steps: Vec<&str> = log
        .iter()
        .map(String::as_str)
        .filter(|line| line.starts_with("step"))
        .collect();
    assert_eq!(steps, ["step 0", "step 1", "step 2"]);
}

/// A handler in place of a sealed action is refused: of one's own
/// action as it is registered, of another's as the kernel takes it.
#[test]
fn a_sealed_action_keeps_its_handler() {
    let mut r = Items::new(Id::new_static("p"));
    r.add_action(ActionSpec::new(Id::new_static("p.x"), "x").sealed())
        .unwrap();
    assert!(
        r.override_action(Id::new_static("p.x"), |_, _, _| {}, Priority(1))
            .is_err()
    );
    let (mut kernel, log) = observed();
    let warnings = kernel.take_warnings();
    assert!(
        warnings.iter().any(|w| w.message.contains("is sealed")),
        "{warnings:?}"
    );
    log.borrow_mut().clear();
    kernel.apply(Command::run(Id::new_static("obs.sealed")));
    kernel.finish();
    assert!(
        log.borrow().contains(&String::from("sealed")),
        "{:?}",
        log.borrow()
    );
}

/// A screen above a guard that takes no action passes none on: what
/// another plugin then runs (a timer, a rule) is not trusted, and opens
/// below the guard; what the guard let pass opens above it.
mod above_the_guard {
    use super::*;

    struct Gate;

    impl crate::Layer for Gate {
        fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}

        fn guards(&self) -> bool {
            true
        }

        fn on_action(&mut self, action: &Id, _: &mut Cx<'_, '_>) -> Handled {
            if action.as_str() == "up.raise" {
                Handled::No
            } else {
                Handled::Yes
            }
        }
    }

    struct Top;

    impl crate::Layer for Top {
        fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}

        fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
            Handled::No
        }
    }

    fn gate_plugin() -> impl Fn(&mut Items) -> Result<(), RegisterError> {
        |r| {
            r.add_layer(crate::LayerSpec::new(
                Id::new_static("gate.lock"),
                "lock",
                Box::new(|_, _| Box::new(Gate)),
            ))
        }
    }

    /// What [`gate_plugin`] registers under.
    fn gate_plugin_manifest() -> Manifest {
        Manifest::new(Id::new_static("gate"), "Gate", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn up() -> impl Fn(&mut Items) -> Result<(), RegisterError> {
        |r| {
            for id in ["up.top", "up.evil"] {
                r.add_layer(crate::LayerSpec::new(
                    Id::parse(id).unwrap(),
                    "top",
                    Box::new(|_, _| Box::new(Top)),
                ))?;
            }
            r.add_action(
                ActionSpec::new(Id::new_static("up.raise"), "raise").handler(|_, _, out| {
                    out.push(Command::OpenScreen(Id::new_static("up.top"), Params::new()));
                }),
            )?;
            r.add_action(
                ActionSpec::new(Id::new_static("up.go"), "go").handler(|_, _, out| {
                    out.push(Command::OpenScreen(
                        Id::new_static("up.evil"),
                        Params::new(),
                    ));
                }),
            )
        }
    }

    /// What [`up`] registers under.
    fn up_manifest() -> Manifest {
        Manifest::new(Id::new_static("up"), "Up", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    #[test]
    fn what_no_guard_let_pass_opens_below_it() {
        let log = log();
        let mut kernel = Kernel::builder(DEVICE)
            .test_plugin(TestPlugin::new("a", &log))
            .items(gate_plugin_manifest(), gate_plugin())
            .items(up_manifest(), up())
            .build()
            .unwrap();
        kernel.start(Instant(0));
        kernel.open_screen(Instant(10), Id::new_static("gate.lock"), Params::new());
        kernel.run_action(Instant(20), Id::new_static("up.raise"));
        assert_eq!(kernel.active_screen(), Some(&Id::new_static("up.top")));
        // That screen gets no input: an action run (as from a timer of
        // plugin `up`) reaches the guard, which takes it.
        assert!(kernel.top_shut_out());
        kernel.apply(Command::run(Id::new_static("up.go")));
        kernel.finish();
        let ids: Vec<&str> = kernel.stack.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, ["a.screen", "gate.lock", "up.top"]);
        // Waking, it closes; the guard is on top.
        kernel.resume(Instant(30));
        assert_eq!(kernel.active_screen(), Some(&Id::new_static("gate.lock")));
    }
}

/// A plugin changes its own secrets; another's only the settings editor
/// (the plugin of the settings slot) does, never another on the reader's
/// behalf.
mod secrets_of_others {
    use super::*;
    use paperpaw_sdk::secrets::SecretChange;

    /// The settings editor: the screen of the settings slot.
    fn editor() -> impl Fn(&mut Items) -> Result<(), RegisterError> {
        |r| {
            r.add_layer(crate::LayerSpec::new(
                Id::new_static("editor.screen"),
                "editor",
                Box::new(|_, _| Box::new(Lock)),
            ))?;
            r.bind_slot(
                slots::SETTINGS,
                Id::new_static("editor.screen"),
                Priority::DEFAULT,
            )?;
            r.add_action(
                ActionSpec::new(Id::new_static("editor.change"), "x").handler(|_, _, out| {
                    out.push(Command::set_secret(SecretChange::keep(
                        "keeper.pw",
                        "",
                        "new",
                    )));
                }),
            )
        }
    }

    /// What [`editor`] registers under.
    fn editor_manifest() -> Manifest {
        Manifest::new(Id::new_static("editor"), "Editor", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn snoop() -> impl Fn(&mut Items) -> Result<(), RegisterError> {
        |r| {
            r.add_action(
                ActionSpec::new(Id::new_static("snoop.rename"), "x").handler(|_, _, out| {
                    out.push(Command::set_secret(SecretChange::keep(
                        "keeper.pw",
                        "Mine",
                        "y",
                    )));
                    out.push(Command::set_secret(SecretChange::keep(
                        "snoop.own",
                        "Own",
                        "z",
                    )));
                }),
            )?;
            r.add_action(
                ActionSpec::new(Id::new_static("snoop.change"), "x").handler(|_, _, out| {
                    out.push(Command::set_secret(SecretChange::keep(
                        "keeper.pw",
                        "",
                        "new",
                    )));
                }),
            )?;
            r.subscribe(
                Id::new_static("snoop.when"),
                Box::new(|_, _, out| {
                    out.push(Command::set_secret(SecretChange::remove("keeper.pw")));
                }),
            )
        }
    }

    /// What [`snoop`] registers under.
    fn snoop_manifest() -> Manifest {
        Manifest::new(Id::new_static("snoop"), "Snoop", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    #[test]
    fn only_a_plugin_changes_its_secrets() {
        let log = log();
        let mut kernel = Kernel::builder(DEVICE)
            .test_plugin(TestPlugin::new("a", &log))
            .items(snoop_manifest(), snoop())
            .items(editor_manifest(), editor())
            .build()
            .unwrap();
        kernel.start(Instant(0));
        kernel.set_secret(SecretChange::keep("keeper.pw", "Password", "x"));
        kernel.run_action(Instant(10), Id::new_static("snoop.rename"));
        assert_eq!(kernel.shared.secrets.get("keeper.pw"), Some("x"), "renamed");
        assert_eq!(kernel.shared.secrets.get("snoop.own"), Some("z"));
        // Not on the reader's behalf: not taken away.
        kernel.apply(Command::Emit(Id::new_static("snoop.when"), Params::new()));
        kernel.finish();
        assert_eq!(kernel.shared.secrets.get("keeper.pw"), Some("x"));
        // Not on the reader's behalf either (any input starts one).
        kernel.run_action(Instant(20), Id::new_static("snoop.change"));
        assert_eq!(kernel.shared.secrets.get("keeper.pw"), Some("x"));
        // The settings editor does (Settings › Passwords).
        kernel.run_action(Instant(30), Id::new_static("editor.change"));
        assert_eq!(kernel.shared.secrets.get("keeper.pw"), Some("new"));
        let warnings = kernel.take_warnings();
        assert!(
            warnings
                .iter()
                .any(|w| w.message.contains("settings editor")),
            "{warnings:?}"
        );
    }
}

/// The work an original asks for is its own plugin's, also run from a
/// handler of another in its place.
mod originals_work {
    use super::*;
    use paperpaw_sdk::work::Work;

    fn worker() -> impl Fn(&mut Items) -> Result<(), RegisterError> {
        |r| {
            r.add_action(
                ActionSpec::new(Id::new_static("wk.plan"), "plan").handler(|_, _, out| {
                    out.push(Command::work(Work::at(
                        Id::new_static("wk.later"),
                        Instant(5000),
                    )));
                }),
            )
        }
    }

    /// What [`worker`] registers under.
    fn worker_manifest() -> Manifest {
        Manifest::new(Id::new_static("wk"), "Worker", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn wrapper() -> impl Fn(&mut Items) -> Result<(), RegisterError> {
        |r| {
            r.override_action(
                Id::new_static("wk.plan"),
                |_, params, out| {
                    out.push(Command::RunOriginal(
                        Id::new_static("wk.plan"),
                        params.clone(),
                    ))
                },
                Priority::DEFAULT,
            )
        }
    }

    /// What [`wrapper`] registers under.
    fn wrapper_manifest() -> Manifest {
        Manifest::new(Id::new_static("wrap"), "Wrapper", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    #[test]
    fn the_originals_work_is_kept() {
        let log = log();
        let mut kernel = Kernel::builder(DEVICE)
            .test_plugin(TestPlugin::new("a", &log))
            .items(worker_manifest(), worker())
            .items(wrapper_manifest(), wrapper())
            .build()
            .unwrap();
        kernel.start(Instant(0));
        let effects = kernel.run_action(Instant(10), Id::new_static("wk.plan"));
        assert!(
            effects.wake_at.is_some_and(|at| at <= Instant(5000)),
            "{:?}",
            effects.wake_at
        );
    }
}

/// A sealed action keeps its handler also where the handler in its place
/// came first.
#[test]
fn a_sealed_action_keeps_its_handler_in_either_order() {
    let mut r = Items::new(Id::new_static("p"));
    r.override_action(Id::new_static("p.x"), |_, _, _| {}, Priority(1))
        .unwrap();
    assert!(
        r.add_action(ActionSpec::new(Id::new_static("p.x"), "x").sealed())
            .is_err()
    );
    let (log, screen) = (log(), log());
    let mut kernel = Kernel::builder(DEVICE)
        .test_plugin(TestPlugin::new("a", &screen))
        .items(guarding_manifest(), guarding(log.clone()))
        .items(observing_manifest(), observing(log.clone()))
        .build()
        .unwrap();
    kernel.start(Instant(0));
    let warnings = kernel.take_warnings();
    assert!(
        warnings.iter().any(|w| w.message.contains("is sealed")),
        "{warnings:?}"
    );
    log.borrow_mut().clear();
    kernel.apply(Command::run(Id::new_static("obs.sealed")));
    kernel.finish();
    assert!(
        log.borrow().contains(&String::from("sealed")),
        "{:?}",
        log.borrow()
    );
}

/// A setting that keeps the device safe is changed by its own plugin and
/// the settings editor alone: another's command, an action of a data
/// plugin or a default of another are refused.
mod guarded_settings {
    use super::*;

    fn safe() -> impl Fn(&mut Items) -> Result<(), RegisterError> {
        |r| {
            r.add_setting(
                SettingSpec::new(
                    Id::new_static("safe.on"),
                    "on",
                    SettingKind::Bool,
                    SettingValue::Bool(true),
                    paperpaw_sdk::builtin::groups::OTHER,
                )
                .guarded(),
            )?;
            r.add_action(
                ActionSpec::new(Id::new_static("safe.off"), "off").handler(|_, _, out| {
                    out.push(Command::SetSetting(
                        Id::new_static("safe.on"),
                        SettingValue::Bool(false),
                    ));
                }),
            )
        }
    }

    /// What [`safe`] registers under.
    fn safe_manifest() -> Manifest {
        Manifest::new(Id::new_static("safe"), "Safe", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn other() -> impl Fn(&mut Items) -> Result<(), RegisterError> {
        |r| {
            r.set_setting_default(Id::new_static("safe.on"), Cow::Borrowed("false"))?;
            r.add_action(ActionSpec::new(Id::new_static("other.off"), "off").handler(
                |_, _, out| {
                    out.push(Command::SetSetting(
                        Id::new_static("safe.on"),
                        SettingValue::Bool(false),
                    ));
                    out.push(Command::BindSlot(
                        slots::SLEEP,
                        Some(Id::new_static("other.screen")),
                    ));
                },
            ))
        }
    }

    /// What [`other`] registers under.
    fn other_manifest() -> Manifest {
        Manifest::new(Id::new_static("other"), "Other", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    #[test]
    fn a_guarded_setting_is_its_plugins() {
        let log = log();
        let storage = paperpaw_sdk::MemoryStorage::new()
            .with("plugins/macro/plugin.ini", "[plugin]\nformats = 3\n")
            .with("plugins/macro/actions.ini", "[off]\nrun = safe.on=false\n");
        let mut kernel = Kernel::builder(DEVICE)
            .test_plugin(TestPlugin::new("a", &log))
            .items(safe_manifest(), safe())
            .items(other_manifest(), other())
            .storage(storage)
            .build()
            .unwrap();
        kernel.start(Instant(0));
        let on = |k: &Kernel| k.settings().get("safe.on") == Some(&SettingValue::Bool(true));
        assert!(on(&kernel), "no other plugin's default");
        kernel.run_action(Instant(10), Id::new_static("other.off"));
        assert!(on(&kernel), "no other plugin's command");
        kernel.run_action(Instant(20), Id::new_static("macro.off"));
        assert!(on(&kernel), "no data plugin's action");
        assert!(
            kernel
                .shared
                .registry
                .resolve_slot(slots::SLEEP.as_str())
                .is_none()
        );
        kernel.run_action(Instant(30), Id::new_static("safe.off"));
        assert!(!on(&kernel), "its own plugin's");
    }
}
