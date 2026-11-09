//! The key map while a screen guards the device (`docs/abi/ui.md` section 4.3):
//! another plugin's action bound in a layer other than the own key layer does
//! nothing and is logged; host actions, the guard's plugin's own actions and the
//! own key layer run.

use super::*;
use core::cell::Cell;
use paperpaw_sdk::Params;

const LOCK: Id = Id::new_static("kg.lock");
const LAYER: Id = Id::new_static("kg.layer");

/// A lock that guards while `locked`; *Menu* puts the key layer `kg.layer` in front.
struct Lock {
    locked: Rc<Cell<bool>>,
}

impl Screen for Lock {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}

    fn guards(&self) -> bool {
        self.locked.get()
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        match action.as_str() {
            "kg.layer-on" => cx.set_key_layer(Some(LAYER)),
            "kg.layer-off" => cx.set_key_layer(None),
            _ => return Handled::No,
        }
        Handled::Yes
    }
}

struct GuardPlugin {
    locked: Rc<Cell<bool>>,
    log: Log,
}

impl Plugin for GuardPlugin {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("kg"), "Guard", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let locked = self.locked.clone();
        r.add_screen(ScreenSpec::new(
            LOCK,
            "lock",
            Box::new(move |_, _| {
                Box::new(Lock {
                    locked: locked.clone(),
                })
            }),
        ))?;
        for name in ["layer-on", "layer-off"] {
            r.add_action(ActionSpec::new(
                Id::parse(&alloc::format!("kg.{name}")).unwrap(),
                "kg.action",
            ))?;
        }
        let log = self.log.clone();
        r.add_action(
            ActionSpec::new(Id::new_static("kg.own"), "kg.own")
                .handler(move |_, _, _| log.borrow_mut().push(String::from("kg.own"))),
        )?;
        // The guard's own action, another plugin's, and a host action, in
        // the global layer; the other plugin's again in the own key layer.
        r.add_keymap(
            "[keymap.global]\n\
             button.up = kg.own\n\
             button.down = other.run\n\
             button.left = core.home\n\
             button.confirm = kg.layer-on\n\
             button.back = kg.layer-off\n\
             [keymap.kg.layer]\n\
             button.right = other.run\n",
        )?;
        r.bind_slot(slots::HOME, LOCK, Priority::DEFAULT)
    }
}

struct OtherPlugin(Log);

impl Plugin for OtherPlugin {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("other"), "Other", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let log = self.0.clone();
        r.add_action(
            ActionSpec::new(Id::new_static("other.run"), "other.run")
                .handler(move |_, _, _| log.borrow_mut().push(String::from("other.run"))),
        )
    }
}

fn kernel(locked: bool) -> (Kernel, Log, Rc<Cell<bool>>) {
    let log = log();
    let flag = Rc::new(Cell::new(locked));
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(GuardPlugin {
            locked: flag.clone(),
            log: log.clone(),
        })
        .plugin(OtherPlugin(log.clone()))
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel.open_screen(Instant(1), LOCK, Params::new());
    (kernel, log, flag)
}

fn runs(log: &Log) -> Vec<String> {
    log.borrow().clone()
}

#[test]
fn without_a_guard_every_binding_runs() {
    let (mut kernel, log, _) = kernel(false);
    press(&mut kernel, 100, "up");
    press(&mut kernel, 300, "down");
    assert_eq!(runs(&log), ["kg.own", "other.run"]);
    assert!(kernel.warnings.is_empty());
}

#[test]
fn under_a_guard_another_plugins_binding_does_nothing_and_is_logged_once() {
    let (mut kernel, log, _) = kernel(true);
    press(&mut kernel, 100, "up");
    press(&mut kernel, 300, "down");
    press(&mut kernel, 500, "down");
    // The guard's own action ran; the other plugin's never did.
    assert_eq!(runs(&log), ["kg.own"]);
    let said: Vec<_> = kernel
        .warnings
        .iter()
        .filter(|warning| {
            warning
                .message
                .contains("does nothing while a screen guards")
        })
        .collect();
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(said[0].message.contains("button.down"));
    assert!(said[0].message.contains("other.run"));
}

#[test]
fn a_host_action_still_runs_under_a_guard() {
    let (mut kernel, _, _) = kernel(true);
    let before = kernel.warnings.len();
    press(&mut kernel, 100, "left");
    // `core.home` is the host's: no warning, and the guard still holds.
    assert_eq!(kernel.warnings.len(), before);
    assert_eq!(kernel.active_screen(), Some(&LOCK));
}

#[test]
fn the_own_key_layer_runs_another_plugins_action_under_a_guard() {
    let (mut kernel, log, _) = kernel(true);
    // `right` in the layer names the other plugin's action: nothing before the
    // layer is put in front (it is not bound at all), then it runs.
    press(&mut kernel, 100, "right");
    assert!(runs(&log).is_empty());
    press(&mut kernel, 300, "confirm");
    kernel.finish();
    press(&mut kernel, 500, "right");
    assert_eq!(runs(&log), ["other.run"]);
    // Taken away again, it is nothing.
    press(&mut kernel, 700, "back");
    press(&mut kernel, 900, "right");
    assert_eq!(runs(&log), ["other.run"]);
}

#[test]
fn when_the_guard_ends_the_other_binding_runs_again() {
    let (mut kernel, log, flag) = kernel(true);
    press(&mut kernel, 100, "down");
    assert!(runs(&log).is_empty());
    flag.set(false);
    press(&mut kernel, 300, "down");
    assert_eq!(runs(&log), ["other.run"]);
}
