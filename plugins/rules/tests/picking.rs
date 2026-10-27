//! Picking an action for a rule, with the real kernel: more actions than a
//! page hold are listed by the plugin that brings them, a plugin's own are
//! one step in, and back goes out of the plugin again before it closes the
//! list.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::cell::RefCell;
use std::rc::Rc;

use paperpaw_core::Kernel;
use paperpaw_plugin_rules::Rules;
use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::semantics::Role;
use paperpaw_sdk::{
    ActionSpec, DeviceInfo, DisplayInfo, Id, InputCaps, Instant, Manifest, MemoryClass,
    MemoryStorage, Params, PixelFormat, Plugin, RegisterError, Registrar, Version,
};

const DEVICE: DeviceInfo = DeviceInfo {
    id: "test",
    name: "Test device",
    display: DisplayInfo {
        width: 480,
        height: 800,
        dpi: 220,
        format: PixelFormat::Gray4,
        rotation: paperpaw_platform::Rotation::Deg0,
        color: None,
    },
    input: InputCaps {
        buttons: true,
        touch: true,
        pen: false,
        keys: &[],
    },
    memory: MemoryClass::Large,
};

/// The list that picks, and what it tells.
const PICK: Id = Id::new_static("rules.pick");
const PICKED: Id = Id::new_static("test.picked");

/// A plugin with many actions, "<Name> action 01" and on, ids `<id>.a01`.
struct Crowd {
    id: &'static str,
    name: &'static str,
    count: usize,
}

impl Plugin for Crowd {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static(self.id), self.name, Version::new(0, 1, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        for n in 1..=self.count {
            r.add_action(ActionSpec::new(
                Id::parse(&format!("{}.a{n:02}", self.id)).unwrap(),
                format!("{} action {n:02}", self.name),
            ))?;
        }
        Ok(())
    }
}

/// Keeps what the list replied.
struct Reader(Rc<RefCell<Vec<String>>>);

impl Plugin for Reader {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("test"), "Test", Version::new(0, 1, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let picked = self.0.clone();
        r.subscribe(
            PICKED,
            Box::new(move |_, params, _| {
                let id = params.get("id").unwrap_or_default().to_string();
                picked.borrow_mut().push(id);
            }),
        )
    }
}

struct Test {
    kernel: Kernel,
    picked: Rc<RefCell<Vec<String>>>,
    now: u64,
}

impl Test {
    /// Two plugins of twenty actions each, besides the kernel's own.
    fn new() -> Self {
        let picked = Rc::new(RefCell::new(Vec::new()));
        let mut kernel = Kernel::builder(DEVICE)
            .storage(MemoryStorage::new())
            .plugin(Rules)
            .plugin(Crowd {
                id: "alpha",
                name: "Alpha tools",
                count: 20,
            })
            .plugin(Crowd {
                id: "beta",
                name: "Beta tools",
                count: 20,
            })
            .plugin(Reader(picked.clone()))
            .build()
            .unwrap();
        kernel.start(Instant(0));
        Self {
            kernel,
            picked,
            now: 0,
        }
    }

    fn at(&mut self) -> Instant {
        self.now += 1000;
        Instant(self.now)
    }

    /// Asks for an action, as adding a rule does.
    fn open(&mut self) {
        let at = self.at();
        let params = Params::new()
            .with("mode", "action".to_string())
            .with("reply", PICKED.as_str().to_string());
        self.kernel.open_screen(at, PICK, params);
    }

    fn press(&mut self, action: Id) {
        let at = self.at();
        self.kernel.run_action(at, action);
    }

    /// The heading of the list shown.
    fn heading(&mut self) -> String {
        self.kernel
            .describe()
            .into_iter()
            .find(|part| part.role == Role::Heading)
            .map(|part| part.label)
            .unwrap_or_default()
    }

    /// The rows shown, and which of them has the focus.
    fn rows(&mut self) -> (usize, Option<usize>) {
        let rows: Vec<_> = self
            .kernel
            .describe()
            .into_iter()
            .filter(|part| part.role == Role::Row)
            .collect();
        (rows.len(), rows.iter().position(|part| part.focused))
    }
}

/// The groups of the test: Alpha, Beta, the kernel's own and the rules'.
const GROUPS: usize = 4;

#[test]
fn a_long_list_comes_by_plugins_and_a_plugin_opens_into_its_actions() {
    let mut test = Test::new();
    test.open();
    assert_eq!(test.kernel.active_screen(), Some(&PICK));
    // The plugins, not their forty actions and more.
    assert_eq!(test.heading(), "Which action?");
    // The focus is not shown until a button moves it, on touch screens.
    assert_eq!(test.rows(), (GROUPS, None));

    // The first is Alpha, in alphabetical order, and its name heads the list.
    test.press(actions::CONFIRM);
    assert_eq!(test.heading(), "Alpha tools");
    let (rows, _) = test.rows();
    assert!(rows > GROUPS, "its actions: {rows}");

    // Back goes out to the plugins, the focus where it was; the list is
    // still there.
    test.press(actions::BACK);
    assert_eq!(test.kernel.active_screen(), Some(&PICK));
    assert_eq!(test.heading(), "Which action?");
    assert_eq!(test.rows().0, GROUPS);

    // Back again closes it, with nothing picked.
    test.press(actions::BACK);
    assert_ne!(test.kernel.active_screen(), Some(&PICK));
    assert!(test.picked.borrow().is_empty());
}

#[test]
fn back_from_a_plugin_returns_to_it_in_the_list() {
    let mut test = Test::new();
    test.open();
    // Beta is second: the first press shows the focus, the next moves it.
    test.press(actions::DOWN);
    test.press(actions::DOWN);
    test.press(actions::CONFIRM);
    assert_eq!(test.heading(), "Beta tools");
    test.press(actions::BACK);
    assert_eq!(test.rows(), (GROUPS, Some(1)), "the focus on Beta again");
}

#[test]
fn an_action_in_a_plugin_is_picked_and_the_list_closes() {
    let mut test = Test::new();
    test.open();
    // Beta is second: the first press shows the focus, the next moves it.
    test.press(actions::DOWN);
    test.press(actions::DOWN);
    test.press(actions::CONFIRM);
    assert_eq!(test.heading(), "Beta tools");
    // The third action in its order.
    test.press(actions::DOWN);
    test.press(actions::DOWN);
    test.press(actions::CONFIRM);
    assert_eq!(*test.picked.borrow(), ["beta.a03"]);
    assert_ne!(test.kernel.active_screen(), Some(&PICK));
}
