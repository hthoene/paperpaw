//! *Look for new books* (#277): a kernel action any plugin may run or name
//! in a menu. The library folder is walked anew; a walk already under way
//! goes on rather than starting over, so pressing again cannot keep the
//! library from ever finishing.

use alloc::rc::Rc;
use paperpaw_sdk::event::Event;
use paperpaw_sdk::{Command, FixedAction, Id, RegisterError, Registrar};

use super::{Catalog, catalog_job::CatalogJob};

/// The action: looks for new books.
pub(crate) const LOOK_AGAIN: Id = Id::new_static("core.library.look-again");

/// What the action's handler (a plain function) passes on to the catalog.
const LOOK_AGAIN_WANTED: Event<()> = Event::new("core.library-look-again-wanted");

pub(super) fn register(r: &mut Registrar, catalog: &Rc<Catalog>) -> Result<(), RegisterError> {
    static ACTION: [FixedAction; 1] = [FixedAction::new(LOOK_AGAIN, "core.library.look-again")
        .handler(|_, _, out| out.push(LOOK_AGAIN_WANTED.emit(())))];
    static ICON: [(Id, &str); 1] = [(LOOK_AGAIN, "refresh")];
    r.add_actions(&ACTION)?;
    r.add_icons(&ICON)?;
    let catalog = Rc::clone(catalog);
    r.on(LOOK_AGAIN_WANTED, move |env, _, out| {
        // Without a format nothing is a book.
        if env.formats().is_empty() {
            return;
        }
        let walking = catalog.job.borrow().as_ref().is_some_and(CatalogJob::is_walk);
        catalog.want_walk(true, false, out);
        let said = if walking {
            "core.library.look-again.busy"
        } else {
            "core.library.look-again.started"
        };
        out.push(Command::Notify(env.i18n().tr(said).into()));
    })
}
