//! Guards, the lock and questions: the rules of `docs/abi/ui.md` sections
//! 1.2, 1.3, 3.1 and 4.3 as pure functions over a borrowed view of the screen
//! stack.
//!
//! The stack itself (instances, hand-overs, commands) is H1's. This module
//! only decides, so that every consumer (the command applier, the input
//! router, `core.ui` ops 1 and 8, the compose hook) asks one place and the
//! rules cannot drift apart. Nothing here allocates, keeps a reference or
//! calls plugin code; a stack view is read-only and ordered bottom to top.
//!
//! "The guard" is the topmost instance whose `guards` flag is set; "the
//! guard's plugin" is its owner.
use paperpaw_native::host_glue::{ERR_DENIED, ERR_FULL, ERR_INVALID, ERR_NOT_FOUND, ERR_STATE};

/// The slot of the keyboard's "type on the phone" question.
pub(crate) const REMOTE_KEYBOARD: &str = "core.remote-keyboard";
/// Most open requests (not counting a nested one) on an ordinary device.
pub(crate) const MAX_REQUESTS: u8 = 4;
/// Most open requests (not counting a nested one) on the X4.
pub(crate) const MAX_REQUESTS_X4: u8 = 1;

/// One screen instance, as the rules need it.
#[allow(dead_code, reason = "consumers are H1's applier and the core.ui ops")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Row {
    pub(crate) instance: u32,
    /// The full host owner id; 0 is reserved for host screens.
    pub(crate) owner: u64,
    pub(crate) overlay: bool,
    /// Guards the device now (declared or set at run time).
    pub(crate) guards: bool,
    /// Declared or set secret (not the derived secrecy, see [`is_secret`]).
    pub(crate) secret: bool,
    /// A screen the host itself shows for a device state (`core.sleep`).
    pub(crate) host: bool,
    /// A dialog of a question the guard's plugin asked, authorized by the host.
    pub(crate) guard_dialog: bool,
}

/// The stack, bottom (index 0) to top.
#[allow(dead_code, reason = "consumers are H1's applier and the core.ui ops")]
pub(crate) trait UiStackView {
    fn len(&self) -> usize;
    fn row(&self, index: usize) -> Option<Row>;
}

/// A stack kept as a slice of rows (bottom to top).
#[allow(dead_code, reason = "consumers are H1's applier and the core.ui ops")]
pub(crate) struct Rows<'a>(pub(crate) &'a [Row]);

impl UiStackView for Rows<'_> {
    fn len(&self) -> usize {
        self.0.len()
    }
    fn row(&self, index: usize) -> Option<Row> {
        self.0.get(index).copied()
    }
}

fn invalid(row: Row) -> bool {
    (row.owner == 0) != row.host
}

fn invalid_view(view: &dyn UiStackView) -> bool {
    (0..view.len()).any(|index| view.row(index).is_none_or(invalid))
}

/// Index of the topmost guarding instance.
pub(crate) fn topmost_guard(view: &dyn UiStackView) -> Option<usize> {
    (0..view.len())
        .rev()
        .find(|&i| view.row(i).is_none_or(|r| r.guards || invalid(r)))
}

/// The topmost guarding instance, if any.
#[allow(
    dead_code,
    reason = "the kernel's stack sync; header-only test builds lack it"
)]
pub(crate) fn guard_instance(view: &dyn UiStackView) -> Option<u32> {
    topmost_guard(view)
        .and_then(|i| view.row(i))
        .map(|r| r.instance)
}

/// The guard's plugin, if any instance guards.
pub(crate) fn guard_owner(view: &dyn UiStackView) -> Option<u64> {
    topmost_guard(view)
        .and_then(|i| view.row(i))
        .map(|r| if invalid(r) { 0 } else { r.owner })
}

/// Index of the instance that gets keys and pointers. Without a guard that is
/// the top; with one it is the topmost instance that is not the host's own
/// screen for a device state: the sleep screen shows above the guard but "gets
/// no input while the guard is up" (section 4.3).
pub(crate) fn input_index(view: &dyn UiStackView) -> Option<usize> {
    if invalid_view(view) {
        return None;
    }
    let last = view.len().checked_sub(1)?;
    if topmost_guard(view).is_none() {
        return Some(last);
    }
    (0..=last)
        .rev()
        .find(|&i| view.row(i).is_some_and(|r| !r.host))
}

fn top(view: &dyn UiStackView) -> Option<Row> {
    input_index(view).and_then(|i| view.row(i))
}

/// Whether the instance at `index` is secret: it says so, it guards, or it
/// lies above a guarding instance (section 1.2).
pub(crate) fn is_secret(view: &dyn UiStackView, index: usize) -> bool {
    let Some(row) = view.row(index) else {
        return true;
    };
    invalid_view(view)
        || row.secret
        || row.guards
        || (0..index).any(|i| view.row(i).is_none_or(|r| r.guards || invalid(r)))
}

/// Whether the instance on top is secret (its input goes to few filters and
/// no event tells where its focus is).
pub(crate) fn top_is_secret(view: &dyn UiStackView) -> bool {
    view.len()
        .checked_sub(1)
        .is_some_and(|i| is_secret(view, i))
}

/// The one instance that gets keys and pointers ([`input_index`]). With a guard
/// up it is always the guard or something above it (opens of others wait
/// below, [`open_index`]) and never the host's sleep screen, so the lock owns
/// the input path.
pub(crate) fn input_target(view: &dyn UiStackView) -> Option<u32> {
    top(view).map(|r| r.instance)
}

/// Whether `instance` may get `draw`/`input` at all: only the top one does.
pub(crate) fn may_receive_input(view: &dyn UiStackView, instance: u32) -> bool {
    input_target(view) == Some(instance)
}

/// Where a new instance goes: the index it is inserted at. With a guard up,
/// an open by another plugin goes directly below the topmost guard and waits
/// there (no `draw`, no `input`); the guard's plugin, a dialog the host
/// authorized for the guard's question and the host's own screen for a device
/// state go on top.
pub(crate) fn open_index(
    view: &dyn UiStackView,
    opener: u64,
    authorized_dialog: bool,
    host_screen: bool,
) -> usize {
    match topmost_guard(view) {
        Some(guard)
            if !(authorized_dialog
                || host_screen
                || (opener != 0 && guard_owner(view) == Some(opener))) =>
        {
            guard
        }
        _ => view.len(),
    }
}

/// `command.open` (any mode) and command 56 from the callback of the screen
/// instance `from` (`None`: an action or event): a guard dialog opens nothing
/// but its one nested question, so any ordinary open from it is `ERR_DENIED`
/// at the caller (section 4.3). Where the new instance goes is [`open_index`].
pub(crate) fn open_allowed(view: &dyn UiStackView, from: Option<u32>) -> Result<(), i32> {
    if invalid_view(view) {
        return Err(ERR_STATE);
    }
    let dialog = from
        .and_then(|instance| find(view, instance))
        .and_then(|i| view.row(i))
        .is_some_and(|r| r.guard_dialog);
    if dialog { Err(ERR_DENIED) } else { Ok(()) }
}

/// Whether the `core.input-filter` subscriber `filter_plugin` is called for
/// the next input. While the top instance is secret, only the filters of its
/// own plugin and of the topmost guard's plugin run (sections 1.2 and 4.3: with
/// a guard up that is the guard's plugin, beside the top guard dialog's plugin).
/// Others are skipped, not called.
pub(crate) fn filter_runs(view: &dyn UiStackView, filter_plugin: u64) -> bool {
    if filter_plugin == 0 || invalid_view(view) {
        return false;
    }
    // A guard makes the instance on top secret (it guards, or lies above the
    // guard), so one rule covers both.
    let Some(top_row) = top(view).filter(|_| top_is_secret(view)) else {
        return true;
    };
    top_row.owner == filter_plugin || guard_owner(view) == Some(filter_plugin)
}

/// A binding in the slot or global key layer that names another plugin's
/// action or input does nothing while a guard is up (logged); host actions
/// and the guard's plugin's own run (section 4.3).
pub(crate) fn binding_runs(view: &dyn UiStackView, target_plugin: Option<u64>) -> bool {
    if target_plugin == Some(0) || invalid_view(view) {
        return false;
    }
    match (guard_owner(view), target_plugin) {
        (None, _) | (Some(_), None) => true,
        (Some(guard), Some(plugin)) => guard == plugin,
    }
}

/// `command.raise_input`: while an instance guards only the guard's plugin
/// may raise.
pub(crate) fn raise_allowed(view: &dyn UiStackView, submitter: u64) -> Result<(), i32> {
    if submitter == 0 || invalid_view(view) {
        return Err(ERR_STATE);
    }
    match guard_owner(view) {
        Some(guard) if guard != submitter => Err(ERR_DENIED),
        _ => Ok(()),
    }
}

/// `command.screen_policy` turning `guards` on for `instance` (section 1.2):
/// only the submitter's own instance that is the top of the whole stack, not
/// counting the host's own screen for a device state. Anywhere else
/// `ERR_STATE` (the whole batch is refused); another plugin's instance is
/// `ERR_DENIED`; an instance not on the stack `ERR_NOT_FOUND`.
pub(crate) fn guard_on_allowed(
    view: &dyn UiStackView,
    submitter: u64,
    instance: u32,
) -> Result<(), i32> {
    if submitter == 0 || invalid_view(view) {
        return Err(ERR_STATE);
    }
    let index = find(view, instance).ok_or(ERR_NOT_FOUND)?;
    if view.row(index).is_none_or(|r| r.owner != submitter) {
        return Err(ERR_DENIED);
    }
    let top_index = (0..view.len())
        .rev()
        .find(|&i| view.row(i).is_some_and(|r| !r.host))
        .ok_or(ERR_STATE)?;
    if index == top_index {
        Ok(())
    } else {
        Err(ERR_STATE)
    }
}

fn find(view: &dyn UiStackView, instance: u32) -> Option<usize> {
    (0..view.len()).find(|&i| view.row(i).is_some_and(|r| r.instance == instance))
}

/// What a permitted `command.close` does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Close {
    /// Close the instance at this index (the screens above it stay).
    Instance(usize),
    /// The root screen is never closed: `OK`, no effect.
    NoEffect,
}

/// `command.close` (section 1.3). `instance` 0 is the submitter's own top
/// instance (`ERR_STATE` outside its own top screen); a non-zero one is the
/// submitter's own, or another plugin's `overlay`. A guarding instance, and
/// any instance above the topmost guard, is closed only by the guard's plugin,
/// except a guard dialog closing itself.
pub(crate) fn close_allowed(
    view: &dyn UiStackView,
    submitter: u64,
    instance: u32,
) -> Result<Close, i32> {
    if submitter == 0 || invalid_view(view) {
        return Err(ERR_STATE);
    }
    let index = if instance == 0 {
        let Some(i) = view.len().checked_sub(1) else {
            return Err(ERR_STATE);
        };
        if view.row(i).is_none_or(|r| r.owner != submitter) {
            return Err(ERR_STATE);
        }
        i
    } else {
        find(view, instance).ok_or(ERR_NOT_FOUND)?
    };
    let row = view.row(index).ok_or(ERR_NOT_FOUND)?;
    let own = row.owner == submitter;
    if instance != 0 && !own && !row.overlay {
        return Err(ERR_DENIED);
    }
    // A guarding instance (any, also a lower one under a newer guard) and
    // everything at or above the topmost guard is the guard's plugin's to
    // close; a guard dialog may close itself.
    let protected = row.guards || topmost_guard(view).is_some_and(|guard| index >= guard);
    let allowed = guard_owner(view) == Some(submitter) || (own && (row.guard_dialog || row.guards));
    if protected && !allowed {
        return Err(ERR_DENIED);
    }
    Ok(if index == 0 {
        Close::NoEffect
    } else {
        Close::Instance(index)
    })
}

/// `command.back_to` closing the screens above the instance at index `target`
/// (section 1.3): a `target` not on the stack is `ERR_NOT_FOUND` (checked
/// before any arithmetic); with a guard up, only the guard's plugin closes
/// anything (everything above the target is the guard or above it, or the
/// target is below it); nothing to close is `OK` for anyone.
pub(crate) fn back_to_allowed(
    view: &dyn UiStackView,
    submitter: u64,
    target: usize,
) -> Result<(), i32> {
    if submitter == 0 || invalid_view(view) {
        return Err(ERR_STATE);
    }
    if target >= view.len() {
        return Err(ERR_NOT_FOUND);
    }
    let closes_something = target + 1 < view.len();
    match guard_owner(view) {
        Some(guard) if closes_something && guard != submitter => Err(ERR_DENIED),
        _ => Ok(()),
    }
}

/// The rows `core.ui` op 1 lists for `viewer` (top first by the caller): the
/// range `start..end` of the bottom-to-top view, and whether rows were left
/// out (`service.end.flags` bit 2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Visible {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) left_out: bool,
}

/// Op 1 under a guard: every other plugin sees the topmost guard alone; the
/// guard's plugin sees from the guard up (section 4.3).
pub(crate) fn visible_rows(view: &dyn UiStackView, viewer: u64) -> Visible {
    let len = view.len();
    if viewer == 0 || invalid_view(view) {
        return Visible {
            start: 0,
            end: 0,
            left_out: len != 0,
        };
    }
    match topmost_guard(view) {
        None => Visible {
            start: 0,
            end: len,
            left_out: false,
        },
        Some(guard) if guard_owner(view) == Some(viewer) => Visible {
            start: guard,
            end: len,
            left_out: guard > 0,
        },
        Some(guard) => Visible {
            start: guard,
            end: guard + 1,
            left_out: len > 1,
        },
    }
}

/// Whether the instance at `index` is drawn (by the host, as itself) under an
/// overlay owned by `overlay_owner`: not when it is secret, or at or below a
/// guard, and the overlay is another plugin's (section 1.2).
pub(crate) fn drawn_under_overlay(
    view: &dyn UiStackView,
    index: usize,
    overlay_owner: u64,
) -> bool {
    if invalid_view(view) {
        return false;
    }
    let Some(row) = view.row(index) else {
        return false;
    };
    if row.owner == overlay_owner {
        return true;
    }
    let at_or_below_guard = topmost_guard(view).is_some_and(|g| index <= g);
    !(is_secret(view, index) || at_or_below_guard)
}

/// A question as `core.ui` op 8 gets it.
#[allow(dead_code, reason = "consumer is H2's op 8 handler")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Ask<'a> {
    pub(crate) asker: u64,
    pub(crate) kind: u32,
    pub(crate) slot: &'a str,
    /// The `secret` flag the caller set (kind 0).
    pub(crate) secret: bool,
    /// Asked from the dialog instance of an open request: that request.
    pub(crate) parent: Option<Parent>,
}

/// The open request whose dialog asks a nested question.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Parent {
    pub(crate) secret: bool,
    /// The parent is itself a nested question: it cannot nest again.
    pub(crate) nested: bool,
    /// The parent already has its one nested question open.
    pub(crate) has_nested: bool,
}

/// The requests open now, for the limits.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct OpenRequests {
    /// Open requests, nested ones not counted.
    pub(crate) total: u8,
    /// The asker has one open (asking again cancels it).
    pub(crate) asker_has_one: bool,
}

/// What an admitted question needs done.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Admission {
    /// Cancel the asker's open request first (outcome 1).
    pub(crate) cancels_asker_request: bool,
    /// The dialog instance is secret (the flag, or inherited from the parent).
    pub(crate) secret: bool,
    /// The dialog opens below the guard and waits (a question of a plugin
    /// other than the guard's).
    pub(crate) below_guard: bool,
}

/// Whether `core.ui` op 8 admits a question (sections 3.1 and 4.3).
///
/// - `kind` 0-3, `slot` empty or, for kind 0, `core.remote-keyboard`
///   (`ERR_INVALID`).
/// - A nested question (asked from the dialog of an open request): only kind 0
///   on `core.remote-keyboard` (else `ERR_DENIED`), one per parent and never
///   from a nested one (`ERR_FULL`), `ERR_DENIED` while any instance guards (the
///   phone keyboard would carry the PIN over the network; refused for 1.0),
///   and it inherits the parent's `secret`. It is also allowed on the X4.
/// - Otherwise at most four open requests device-wide, one on the X4
///   (`ERR_FULL`), not counting a nested one, and one per asker (asking again
///   cancels the open one, so it does not count against the limit).
pub(crate) fn ask_admission(
    view: &dyn UiStackView,
    ask: &Ask<'_>,
    open: OpenRequests,
    x4: bool,
) -> Result<Admission, i32> {
    if ask.asker == 0 || invalid_view(view) {
        return Err(ERR_STATE);
    }
    if ask.kind > 3 {
        return Err(ERR_INVALID);
    }
    let remote = match ask.slot {
        "" => false,
        REMOTE_KEYBOARD if ask.kind == 0 => true,
        _ => return Err(ERR_INVALID),
    };
    if let Some(parent) = ask.parent {
        if parent.nested || parent.has_nested {
            return Err(ERR_FULL);
        }
        if topmost_guard(view).is_some() || ask.kind != 0 || !remote {
            return Err(ERR_DENIED);
        }
        return Ok(Admission {
            cancels_asker_request: false,
            secret: parent.secret || ask.secret,
            below_guard: false,
        });
    }
    let cancels = open.asker_has_one;
    let limit = if x4 { MAX_REQUESTS_X4 } else { MAX_REQUESTS };
    if open.total.saturating_sub(u8::from(cancels)) >= limit {
        return Err(ERR_FULL);
    }
    Ok(Admission {
        cancels_asker_request: cancels,
        secret: ask.secret,
        below_guard: guard_owner(view).is_some_and(|g| g != ask.asker),
    })
}

#[cfg(test)]
#[path = "ui_guard_tests.rs"]
mod tests;
