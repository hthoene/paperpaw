//! The library while the kernel makes its catalog anew, and looking for
//! new books: the books found so far counted as such (where there was no
//! catalog, the kernel shows them as it goes on), and an action that has
//! the kernel walk the library folder again, offered on an empty library
//! (books copied onto the card while it was in a computer).

use alloc::string::String;

use paperpaw_sdk::library::catalog::State;
use paperpaw_sdk::work::WorkState;
use paperpaw_sdk::{
    Command, Commands, Cx, Env, FixedAction, Handled, Id, Instant, Params, Rect, StorageError,
    library,
};

/// The kernel's work that makes the catalog and the sorted books.
const CATALOG_WORK: Id = Id::new_static("core.catalog");

/// How often an empty library, waiting for the kernel, looks whether the
/// kernel is at work (in milliseconds).
const WATCH_MS: u64 = 15_000;

/// How long the kernel may do nothing while the library waits for it,
/// before the library says it cannot read the card (in milliseconds): a
/// card swapped while PaperPaw ran, a catalog or sorted books damaged
/// that nothing makes anew (#145).
const STUCK_MS: u64 = 90_000;

/// Looks for new books: the kernel walks the library folder anew.
pub(crate) const LOOK_AGAIN: Id = Id::new_static("files.look-again");

/// The key of [`library::Library::extended`] the kernel's library answers
/// with whether it is making the catalog anew (`walking`).
const WALKING: Id = Id::new_static("core.library-walking");

/// How long after looking for new books the library makes sure the
/// kernel began (in milliseconds): it does at once, and tells the view
/// when it ends or fails.
const BEGUN_MS: u64 = 5_000;

/// The action of [`LOOK_AGAIN`].
pub(crate) const ACTION: FixedAction =
    FixedAction::new(LOOK_AGAIN, "files.look-again").handler(look_again);

fn look_again(env: &Env<'_>, _: &Params, out: &mut Commands) {
    // A second press while the books are still being collected lets the
    // walk go on: walked anew, it would start over.
    let (request, said) = if walking(env) {
        (library::request_catalog(), "files.look-again.busy")
    } else {
        (library::request_catalog_anew(), "files.look-again.started")
    };
    out.push(request);
    out.push(Command::Notify(env.i18n().tr(said).into()));
}

impl super::ListScreen {
    /// Looking for new books (the button or the menu, before the action's
    /// own handler, which it passes on to): an empty library says it
    /// collects the books from now on, rather than that there are none,
    /// until the kernel shows the first or says it is done.
    pub(super) fn looking(&mut self, cx: &mut Cx<'_, '_>) -> Handled {
        if self.look_again {
            self.collecting = true;
            self.refilter(cx.env());
            cx.request_redraw();
            cx.wake_at(Instant(cx.env().now().millis() + BEGUN_MS));
        }
        Handled::No
    }

    /// A while after looking for new books: where no catalog is being
    /// made (the kernel never began, as where another handler took the
    /// action), the library says again that it has none, not that it
    /// collects them.
    pub(super) fn looked(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        if self.collecting && !super::collecting(&env) && !walking(&env) {
            self.collecting = false;
            self.refilter(&env);
            cx.request_redraw();
        }
        self.watch(cx);
    }

    /// While the library waits for the kernel (collecting the books, or
    /// preparing them): looks now and then whether the kernel is at work.
    /// If it does nothing for [`STUCK_MS`], nothing will come: the library
    /// says the folder cannot be read (what it says when the kernel
    /// fails), and asks the kernel once more.
    pub(super) fn watch(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let now = env.now();
        if self.failed || !(self.collecting || self.preparing()) {
            self.idle_since = None;
            return;
        }
        let working = matches!(
            env.work().get(&CATALOG_WORK),
            Some(WorkState::Running { .. })
        );
        if working {
            self.idle_since = Some(now);
        }
        let since = *self.idle_since.get_or_insert(now);
        if now.since(since) < STUCK_MS {
            cx.wake_at(Instant(now.millis() + WATCH_MS));
            return;
        }
        self.idle_since = None;
        self.failed = true;
        self.card_gone = card_gone(&env);
        self.collecting = false;
        self.refilter(&env);
        cx.command(library::request_catalog());
        cx.request_redraw();
    }

    /// Where the sorted books could not be opened as the card cannot be
    /// read (taken out while the library was open, #145): the library says
    /// so at once, as where the kernel fails, not that it prepares them.
    /// The kernel makes nothing of a card it cannot read, so no word of it
    /// would come; it walks the library anew once the card is back.
    pub(super) fn unreadable(&mut self, env: &Env<'_>) {
        if self.sorted.is_none() && !readable(env) {
            self.failed = true;
            self.card_gone = card_gone(env);
            self.collecting = false;
        }
    }
}

/// Whether the catalog is still to be made (none yet). Not where the
/// storage cannot be read (a card the device cannot use): none would
/// ever come, and the empty library says where books go.
pub(crate) fn collecting(env: &Env<'_>) -> bool {
    matches!(
        library::service(env).catalog_state(env),
        State::Missing | State::Building
    ) && readable(env)
}

/// Whether the storage can be read at all: not while the memory card is
/// taken out (#145).
pub(crate) fn readable(env: &Env<'_>) -> bool {
    env.storage().list("", &mut |_| {}).is_ok()
}

/// Whether the memory card is lost until PaperPaw starts again (taken out
/// and put back more often than a run allows, say): then nothing is tried
/// again by itself, and the library asks for a restart.
pub(crate) fn card_gone(env: &Env<'_>) -> bool {
    env.storage().size("") == Err(StorageError::CARD_GONE)
}

/// The keys of the title and the words of a library whose folder could not
/// be read: a restart asked for where the card is lost until then.
pub(crate) const fn failed_words(card_gone: bool) -> (&'static str, &'static str) {
    if card_gone {
        ("files.list.card-gone.title", "files.list.card-gone")
    } else {
        ("files.list.failed.title", "files.list.failed")
    }
}

/// Whether the library's catalog is being made anew: its books are those
/// found so far.
pub(crate) fn walking(env: &Env<'_>) -> bool {
    library::service(env)
        .extended(env, &WALKING, &Params::new())
        .is_some_and(|answer| answer.get("walking") == Some("true"))
}

/// How many books the filter's row says there are: `count` so far while
/// the catalog is being made anew.
pub(crate) fn count(env: &Env<'_>, count: usize) -> String {
    let key = if walking(env) {
        "files.list.so-far"
    } else {
        "files.books"
    };
    env.i18n().plural(key, count as i64, &[])
}

/// Where the button that looks for new books goes in an empty list's
/// `area`, and what is left above it for what the empty list says.
pub(crate) fn button(area: Rect, row: u16, margin: u16) -> (Rect, Rect) {
    let height = row.min(area.height / 3);
    let width = (area.width.saturating_sub(2 * margin)).min(area.width * 3 / 4);
    let y = area.y + area.height.saturating_sub(height + margin);
    let button = Rect::new(area.x + (area.width - width) / 2, y, width, height);
    let above = Rect::new(area.x, area.y, area.width, y.saturating_sub(area.y));
    (button, above)
}
