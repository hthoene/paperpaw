//! Long text on the keyboard, with the real kernel: a web address or a
//! key longer than a line comes back whole.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::cell::RefCell;
use std::rc::Rc;

use paperpaw_core::Kernel;
use paperpaw_platform::Rotation;
use paperpaw_plugin_keyboard::Keyboard;
use paperpaw_sdk::ask;
use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::event::{Event, EventData};
use paperpaw_sdk::replies::Typed;
use paperpaw_sdk::{
    ActionSpec, Commands, DeviceInfo, DisplayInfo, Env, Id, InputCaps, Instant, Manifest,
    MemoryClass, MemoryStorage, Params, PixelFormat, Plugin, RegisterError, Registrar, Version,
};

const DEVICE: DeviceInfo = DeviceInfo::new(
    "test",
    "Test device",
    DisplayInfo::new(480, 800, 220, PixelFormat::Gray4, Rotation::Deg0, None),
    InputCaps::new(true, true, false, &[]),
    MemoryClass::Large,
);

const ASK: Id = Id::new_static("asker.ask");
const TYPED: Event<Typed> = Event::new("asker.typed");

/// Asks for text and keeps what comes back.
struct Asker(Rc<RefCell<Vec<String>>>);

impl Plugin for Asker {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("asker"), "Asker", Version::new(0, 1, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_action(ActionSpec::new(ASK, "asker.ask").handler(
            |_: &Env<'_>, _, out: &mut Commands| {
                out.push(ask::text("Address", &TYPED).command());
            },
        ))?;
        let got = self.0.clone();
        r.on(TYPED, move |_, typed, _| {
            got.borrow_mut().push(typed.text.clone());
        })
    }
}

fn typed(length: usize) -> String {
    let got = Rc::new(RefCell::new(Vec::new()));
    let mut kernel = Kernel::builder(DEVICE)
        .storage(MemoryStorage::new())
        .plugin(Keyboard)
        .plugin(Asker(got.clone()))
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel.run_action(Instant(100), ASK);
    // Typed on a phone (or pasted): it takes the field's place.
    let text = Typed::new("x".repeat(length), Params::new());
    kernel.send_event(
        Instant(200),
        Id::new_static("keyboard.remote-text"),
        text.to_params(),
    );
    // Done (a long press on confirm).
    kernel.run_action(Instant(300), actions::MENU);
    let got = got.borrow();
    assert_eq!(got.len(), 1, "one answer");
    got[0].clone()
}

#[test]
fn a_long_address_comes_back_whole() {
    assert_eq!(typed(600).len(), 600);
}

#[test]
fn text_past_the_limit_is_cut() {
    assert_eq!(typed(5000).len(), 2000);
}
