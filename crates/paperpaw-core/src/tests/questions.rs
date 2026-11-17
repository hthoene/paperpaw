//! The kernel's own questions in its tests (#293): a `core.ui` store wired as
//! a shell wires it, and a stand-in dialog on `core.choice` and `core.confirm`
//! (the dialog plugin's slots) that draws nothing; a test reads the question
//! the kernel asked and answers it as the dialog would, through the store.
use alloc::boxed::Box;
use alloc::string::String;

use paperpaw_sdk::builtin::slots;
use paperpaw_sdk::{
    Canvas, Command, Cx, Env, Handled, Id, Manifest, Priority, RegisterError, Version,
};

use crate::native_host::shared::Shared;
use crate::native_host::ui_service::UiServices;
use crate::native_host::ui_store::{Limits, UiStore};
use crate::{Items, Kernel};

/// The stand-in dialog's screen.
pub(crate) const DIALOG: Id = Id::new_static("q.dialog");

struct Dialog;

impl crate::testing::Fake for Dialog {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}
    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }
}

/// What [`serves`] registers under.
pub(crate) fn serves_manifest() -> Manifest {
    Manifest::new(Id::new_static("q"), "Questions", Version::new(1, 0, 0))
        .uses(paperpaw_sdk::Uses::NOTHING)
}

/// The stand-in dialog on both slots.
pub(crate) fn serves() -> impl Fn(&mut Items) -> Result<(), RegisterError> {
    |r| {
        r.add_fake(crate::testing::FakeSpec::new(
            DIALOG,
            "q.dialog",
            Box::new(|_, _| Box::new(Dialog)),
        ))?;
        r.bind_slot(slots::CHOICE, DIALOG, Priority::DEFAULT)?;
        r.bind_slot(slots::CONFIRM, DIALOG, Priority::DEFAULT)
    }
}

/// Wires the `core.ui` store into a started kernel, as a shell does.
pub(crate) fn wire(kernel: &mut Kernel) -> Shared<UiStore> {
    let store = Shared::try_new(UiStore::new(Limits::ELSEWHERE)).unwrap();
    kernel
        .shared
        .native_instances
        .test_insert(Id::try_parse("q").unwrap(), None, true)
        .unwrap();
    kernel
        .shared
        .native_instances
        .test_resources()
        .unwrap()
        .hosts()
        .wire_ui(UiServices::new(store.try_clone().unwrap(), false))
        .unwrap();
    store
}

/// The question the dialog on top was opened for: its kind, title, text and
/// lines (`None` where the top screen is no open question's dialog).
pub(crate) fn asked(kernel: &Kernel, store: &UiStore) -> Option<(u32, String, String, String)> {
    let top = kernel.stack.last()?.serial;
    let (request, ..) = store.dialog_of(top)?;
    store
        .question(top, request, |shown| {
            (
                shown.kind,
                String::from(shown.title),
                String::from(shown.text),
                String::from(shown.choices),
            )
        })
        .ok()
}

/// Answers the question of the dialog on top as the dialog would (`choice`:
/// 1 yes or 0 no, or a line's index) and closes it; the answer is delivered.
pub(crate) fn answer(kernel: &mut Kernel, store: &UiStore, choice: u32) {
    let top = kernel.stack.last().unwrap().serial;
    let (request, ..) = store.dialog_of(top).expect("a question's dialog on top");
    store.answer(top, request, 0, choice).unwrap();
    kernel.apply(Command::Back);
    let _ = kernel.finish();
}
