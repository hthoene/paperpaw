//! *Look for new books* (#277): a kernel action any plugin may run or name
//! in a menu. The library folder is walked anew; a walk already under way
//! goes on rather than starting over, so pressing again cannot keep the
//! library from ever finishing.

use crate::Ask;
use crate::Items;
use crate::kit::action::FixedAction;
use paperpaw_sdk::event::Event;
use paperpaw_sdk::{Id, RegisterError};

use super::catalog_job::CatalogJob;

/// The action: looks for new books.
pub(crate) const LOOK_AGAIN: Id = Id::new_static("core.library.look-again");

/// What the action's handler (a plain function) passes on to the catalog.
const LOOK_AGAIN_WANTED: Event<()> = Event::new("core.library-look-again-wanted");

pub(super) fn register(r: &mut Items) -> Result<(), RegisterError> {
    static ACTION: [FixedAction; 1] = [FixedAction::new(LOOK_AGAIN, "core.library.look-again")
        .handler(|_, _, out| out.push(crate::Ask::emit(&LOOK_AGAIN_WANTED, ())))];
    static ICON: [(Id, &str); 1] = [(LOOK_AGAIN, "refresh")];
    r.add_actions(&ACTION)?;
    r.add_icons(&ICON)?;
    r.on(LOOK_AGAIN_WANTED, |k, p| {
        k.with_core_event(&LOOK_AGAIN_WANTED, p, |env, core, _, out| {
            // Without a format nothing is a book.
            if env.formats().is_empty() {
                return;
            }
            let walking = core
                .catalog
                .job
                .borrow()
                .as_ref()
                .is_some_and(CatalogJob::is_walk);
            core.catalog.want_walk(true, false, out);
            let said = if walking {
                "core.library.look-again.busy"
            } else {
                "core.library.look-again.started"
            };
            out.push(Ask::Notify(env.i18n().tr(said).into()));
        });
    })
}
