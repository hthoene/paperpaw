//! `core.ui` ops 4 `widget_make`, 5 `widget_drop`, 14 `status_values` and 15
//! `status_line` (`docs/abi/ui.md` 5.2, 5.3 and 6), over the handle table
//! ([`UiHandles`]) and the registry's directory (`Frame::ui_directory`).
//!
//! - **Who.** Op 4 is for the caller's top screen instance; op 5 for the
//!   instance's maker (the table checks it). Ops 14 and 15 run another
//!   plugin's callback and are depth 1 only (`ERR_BUSY` deeper); on the X4,
//!   until the nested-callback stack gate passes (section 5.1), op 15
//!   answers `ERR_UNSUPPORTED` and op 14 leaves the plugins' items without a
//!   value. The kernel's own items (`host_status`, todo 598) need no call:
//!   op 14 answers them everywhere, a drawn one with a status handle of the
//!   host ([`host_status::HOST`]).
//! - **Own.** A widget, item or line of the caller's own plugin is never
//!   called back: op 4 and 15 answer `ui_instance` with `flags` bit 0 and
//!   instance 0, op 14 a row with `flags` bit 2 and nothing else.
//! - **Below a guard.** A screen at or below another plugin's guard makes no
//!   widget (op 4 `ERR_STATE`: it is not the top) and gets `ERR_DENIED` from ops
//!   14 and 15; op 5 has no guard or depth rule (ui.md 4.3, PR #419).
//! - **Guard.** Under a guard, a private widget, item or line of another
//!   plugin is `ERR_DENIED` (ops 4 and 15) or a hidden row (op 14, `flags` bit
//!   1, no text, no handle); the cache drops private values when a guard
//!   starts ([`StatusCache::drop_private`]).
//! - **Op 14.** Own items, hidden ones and cached values need no call; the
//!   rest are grouped by (owner, token), groups in the order of their first
//!   item, one `Frame::status_ask` each (at most 8). A group whose call fails,
//!   an item without a row or an undeclared item comes back with `flags` 0, no
//!   handle, no text and is not cached; the op still answers `OK`. A row
//!   whose item is not in the group is ignored, the first row of an item wins.
//!   A drawn value (`flags` bit 0) gets a status handle valid for this
//!   callback only; text values are cached until [`UiServices::status_changed`].
//!
//! Memory: the rows are one fallible allocation of at most 8 x 144 bytes and
//! the group's reply one of 3,840 bytes, both freed at the return (no large
//! stack buffer); the cache is 16 values of at most 128 bytes, allocated as
//! they come and dropped by `status_changed`; the handle tables are the
//! `ui_handles` ones (1,088 bytes at most on the X4). A refused allocation is
//! `ERR_NO_MEMORY` with nothing handed out.
use super::{
    registrations::{self as wire, Record},
    ui_guard,
    ui_handles::{Dropped, UiHandles},
    ui_service::UiServices,
};
use crate::abi7::context::Frame;
use crate::native_host::codec;
use alloc::vec::Vec;
use core::cell::RefCell;
use paperpaw_native::host_glue::{
    ERR_BUSY, ERR_DENIED, ERR_FULL, ERR_INVALID, ERR_NO_MEMORY, ERR_NOT_FOUND, ERR_STATE,
    ERR_UNSUPPORTED,
};

use super::{host_services::Caller, stages_wire::core_status_v1 as status};
use crate::abi7::context::HostStatus;

/// The kernel's own status items (op 14 without a plugin call, todo 598).
#[path = "host_status.rs"]
pub(crate) mod host_status;

/// Items one op 14 asks for.
const ITEMS_MAX: usize = 8;
const ITEM_BYTES: usize = 128;
const TEXT_BYTES: usize = 128;
/// Cached text values.
const CACHE_MAX: usize = 16;

/// `ui_status_value.flags`: bit 0 drawn (compose it), 1 hidden, 2 own.
const DRAWN: u32 = 1;
const HIDDEN: u32 = 2;
const OWN: u32 = 4;
/// `ui_instance.flags` bit 0: the caller's own.
const INSTANCE_OWN: u32 = 1;

/// The longest key (item, separator, context) a value is cached under; a
/// longer ask is answered but not cached.
const KEY_MAX: usize = 128;

/// One answered text value, cached per (item, context).
#[derive(Debug)]
struct Cached {
    /// The item, `0xff` (no UTF-8 text holds it), the context: compared whole,
    /// never by a hash, so no crafted context can reach another item's value.
    key: Vec<u8>,
    private: bool,
    text: Vec<u8>,
}

impl Cached {
    fn is(&self, owner: u64, item: &str, context: &[u8]) -> bool {
        self.key.iter().copied().eq(key_of(owner, item, context))
    }
}

/// The values op 14 keeps until a status change or the minute tick. Bounded
/// ([`CACHE_MAX`]), every entry allocated fallibly; a refusal just does not
/// cache.
#[derive(Debug, Default)]
pub(crate) struct StatusCache {
    entries: Vec<Cached>,
    /// Where the next full-cache insert goes (the oldest).
    next: usize,
}

/// The owner (8 bytes), the item, `0xff`, the context: an item re-declared by
/// another plugin never reads the earlier owner's value.
fn key_of<'a>(owner: u64, item: &'a str, context: &'a [u8]) -> impl Iterator<Item = u8> + 'a {
    owner
        .to_le_bytes()
        .into_iter()
        .chain(item.bytes())
        .chain(core::iter::once(0xff))
        .chain(context.iter().copied())
}

impl StatusCache {
    pub(crate) fn get(&self, owner: u64, item: &str, context: &[u8]) -> Option<&[u8]> {
        self.entries
            .iter()
            .find(|e| e.is(owner, item, context))
            .map(|e| e.text.as_slice())
    }

    pub(crate) fn put(
        &mut self,
        owner: u64,
        item: &str,
        context: &[u8],
        private: bool,
        text: &str,
    ) {
        let length = 8 + item.len() + 1 + context.len();
        if length > KEY_MAX {
            return;
        }
        let mut key = Vec::new();
        let mut bytes = Vec::new();
        if key.try_reserve_exact(length).is_err() || bytes.try_reserve_exact(text.len()).is_err() {
            return;
        }
        key.extend(key_of(owner, item, context));
        bytes.extend_from_slice(text.as_bytes());
        let entry = Cached {
            key,
            private,
            text: bytes,
        };
        if let Some(at) = self.entries.iter().position(|e| e.is(owner, item, context)) {
            self.entries[at] = entry;
        } else if self.entries.len() < CACHE_MAX {
            if self.entries.try_reserve(1).is_ok() {
                self.entries.push(entry);
            }
        } else {
            self.entries[self.next % CACHE_MAX] = entry;
            self.next = (self.next + 1) % CACHE_MAX;
        }
    }

    /// Everything goes (event 73 `status_changed`, the minute tick).
    pub(crate) fn clear(&mut self) {
        self.entries.clear();
        self.next = 0;
    }

    /// A guard started: private values go (section 4.3).
    pub(crate) fn drop_private(&mut self) {
        self.entries.retain(|e| !e.private);
        self.next = 0;
    }
}

/// What the table and the cache are for a device.
pub(crate) struct WidgetState {
    pub(crate) handles: RefCell<UiHandles>,
    pub(crate) cache: RefCell<StatusCache>,
    /// Taps and long presses op 6 queued for their owners (`ui_widget_layout`).
    pub(crate) inputs: RefCell<super::ui_widget_layout::WidgetInputs>,
    /// The minute (of the monotonic clock) the cache last saw; `u64::MAX` before.
    minute: core::cell::Cell<u64>,
}

impl WidgetState {
    pub(crate) fn new(x4: bool) -> Self {
        Self {
            handles: RefCell::new(UiHandles::new(x4)),
            cache: RefCell::new(StatusCache::default()),
            inputs: RefCell::new(super::ui_widget_layout::WidgetInputs::new(x4)),
            minute: core::cell::Cell::new(u64::MAX),
        }
    }
}

impl UiServices {
    /// A screen instance closed: the widget instances it made go.
    pub(crate) fn screen_closed(&self, screen: u32) -> Result<usize, i32> {
        Ok(self
            .widgets
            .handles
            .try_borrow_mut()
            .map_err(|_| ERR_BUSY)?
            .drop_screen(screen))
    }

    /// The next drop `owner` has not heard of, as the `event.widget_dropped`
    /// record for its token (ui.md section 5.2), in `out`; the length, with
    /// the notice to give back to [`Self::widget_dropped_delivered`]. `None`
    /// when there is none. Core's dispatcher calls these two: the notice
    /// stays listed until the first delivery attempt is over, whatever its
    /// outcome (`ui.md` 8.2); then it gives it back, even when the callback
    /// failed or trapped. An owner that stops, is unloaded, switched off or in
    /// safe mode loses its notices without a delivery (`plugin_stopped`).
    pub(crate) fn next_widget_dropped(
        &self,
        owner: u64,
        now_ms: u64,
        out: &mut [u8],
    ) -> Result<Option<(Dropped, usize)>, i32> {
        let Some(notice) = self
            .widgets
            .handles
            .try_borrow()
            .map_err(|_| ERR_BUSY)?
            .next_dropped(owner)
        else {
            return Ok(None);
        };
        let record = Record::EventWidgetDropped(wire::EventWidgetDropped {
            now_ms,
            instance: notice.instance,
        });
        let len = codec::encode_batch(&[record], out)?;
        Ok(Some((notice, len)))
    }

    /// The owner of the oldest drop not yet told (`None`: nothing waits).
    pub(crate) fn next_dropped_owner(&self) -> Result<Option<u64>, i32> {
        Ok(self
            .widgets
            .handles
            .try_borrow()
            .map_err(|_| ERR_BUSY)?
            .next_dropped_owner())
    }

    /// The first delivery attempt of the notice is over, whatever its
    /// outcome: it is not listed any more.
    pub(crate) fn widget_dropped_delivered(&self, notice: &Dropped) -> Result<(), i32> {
        self.widgets
            .handles
            .try_borrow_mut()
            .map_err(|_| ERR_BUSY)?
            .dropped_delivered(notice);
        Ok(())
    }

    /// Whether `owner` may say instance `id` changed (`command.widget_changed`):
    /// `ERR_NOT_FOUND`, `ERR_DENIED`, `ERR_BUSY` while the table is in use.
    pub(crate) fn widget_check_changed(&self, owner: u64, id: u32) -> Result<(), i32> {
        self.widgets
            .handles
            .try_borrow()
            .map_err(|_| ERR_BUSY)?
            .check_changed(owner, id)
    }

    /// The checked `command.widget_changed` of `owner` take effect: each
    /// instance is marked for its maker, once. `ERR_BUSY` when the table is in
    /// use (nothing marked).
    pub(crate) fn widgets_changed(&self, owner: u64, ids: &[u32]) -> Result<(), i32> {
        let mut handles = self
            .widgets
            .handles
            .try_borrow_mut()
            .map_err(|_| ERR_BUSY)?;
        for id in ids {
            handles.mark_changed(owner, *id);
        }
        Ok(())
    }

    /// Whether a change waits for its maker.
    pub(crate) fn widget_change_waits(&self) -> bool {
        self.widgets
            .handles
            .try_borrow()
            .is_ok_and(|handles| handles.has_changed())
    }

    /// The oldest change not yet told, as (the maker, the instance), off the
    /// list. `None`: nothing waits (or the table is in use).
    pub(crate) fn take_widget_changed(&self) -> Option<(u64, u32)> {
        self.widgets.handles.try_borrow_mut().ok()?.take_changed()
    }

    /// The lowest screen above `after` that still has widget instances.
    pub(crate) fn next_widget_screen(&self, after: u32) -> Result<Option<u32>, i32> {
        Ok(self
            .widgets
            .handles
            .try_borrow()
            .map_err(|_| ERR_BUSY)?
            .next_screen(after))
    }

    /// A callback of `plugin` returned: its status handles end with it, and the
    /// hand-overs it made that no `open` claimed are freed (`ui.md` 1.1: one
    /// valid in the callback that made it only). A store in use leaves them to
    /// the plugin's stop.
    pub(crate) fn callback_ended(&self, plugin: u64, callback: u64) -> Result<(), i32> {
        let made = super::ui_store::Stamp::new(plugin, callback)
            .and_then(|stamp| self.store.end_callback(stamp));
        self.widgets
            .handles
            .try_borrow_mut()
            .map_err(|_| ERR_BUSY)?
            .end_callback(plugin, callback);
        made
    }

    /// A callback of `plugin` returned a failure: the questions it asked in it
    /// never start (`ui.md` 3.1, a failed start cancels).
    pub(crate) fn callback_failed(&self, plugin: u64, callback: u64) -> Result<(), i32> {
        self.store
            .callback_failed(super::ui_store::Stamp::new(plugin, callback)?)
    }

    /// Event 73 or the minute tick: cached status values go.
    pub(crate) fn status_changed(&self) -> Result<(), i32> {
        self.widgets
            .cache
            .try_borrow_mut()
            .map_err(|_| ERR_BUSY)?
            .clear();
        Ok(())
    }

    /// The clock reached `now_ms`: a new minute ends the cached values (the
    /// minute tick of ui.md op 14); within a minute nothing changes.
    pub(crate) fn minute_tick(&self, now_ms: u64) -> Result<(), i32> {
        let minute = now_ms / 60_000;
        if self.widgets.minute.get() == minute {
            return Ok(());
        }
        self.status_changed()?;
        // Only a cleared cache counts as seen: a refusal tries again.
        self.widgets.minute.set(minute);
        Ok(())
    }

    /// A guard started: private cached values go.
    pub(crate) fn guard_started(&self) -> Result<(), i32> {
        self.widgets
            .cache
            .try_borrow_mut()
            .map_err(|_| ERR_BUSY)?
            .drop_private();
        Ok(())
    }

    /// Whether `plugin` made or owns a widget or status handle, or a tap
    /// waits for it (#248, D3).
    ///
    /// # Errors
    /// `ERR_BUSY` while a table is borrowed.
    pub(crate) fn plugin_holds(&self, plugin: u64) -> Result<bool, i32> {
        let handles = self.widgets.handles.try_borrow().map_err(|_| ERR_BUSY)?;
        let inputs = self.widgets.inputs.try_borrow().map_err(|_| ERR_BUSY)?;
        Ok(handles.plugin_holds(plugin) || inputs.owner_holds(plugin))
    }

    /// A plugin stopped: what it made or owns goes (widgets, status handles);
    /// the owners of what it made are told.
    pub(crate) fn plugin_stopped(&self, plugin: u64) -> Result<(), i32> {
        self.widgets
            .handles
            .try_borrow_mut()
            .map_err(|_| ERR_BUSY)?
            .drop_plugin(plugin);
        // Taps still waiting for it would reach a plugin that is gone.
        self.widgets
            .inputs
            .try_borrow_mut()
            .map_err(|_| ERR_BUSY)?
            .drop_owner(plugin);
        // Values of the owner's items would answer for a plugin that is gone.
        self.widgets
            .cache
            .try_borrow_mut()
            .map_err(|_| ERR_BUSY)?
            .clear();
        Ok(())
    }
}

/// The guard's plugin, if any instance guards.
fn guard_of(frame: &dyn Frame) -> Result<Option<u64>, i32> {
    let mut guard = None;
    frame.with_stack(&mut |view| guard = ui_guard::guard_owner(view))?;
    Ok(guard)
}

/// The nested-callback gate of ops 14 and 15.
fn nested_gate(frame: &dyn Frame) -> Result<u64, i32> {
    let epoch = frame.callback_epoch()?;
    if frame.callback_depth()? != 1 {
        return Err(ERR_BUSY);
    }
    if frame.nested_closed()? {
        return Err(ERR_UNSUPPORTED);
    }
    Ok(epoch)
}

/// Ops 14 and 15 are `ERR_DENIED` for a screen at or below another plugin's
/// guard (ui.md 4.3); outside a screen callback there is nothing to check.
fn below_guard_gate(frame: &dyn Frame, who: &Caller<'_>) -> Result<(), i32> {
    let Ok(screen) = frame.calling_instance() else {
        return Ok(());
    };
    let mut denied = false;
    frame.with_stack(&mut |view| {
        let at = (0..view.len()).find(|&i| view.row(i).is_some_and(|r| r.instance == screen));
        denied = match (ui_guard::topmost_guard(view), at) {
            (Some(guard), Some(at)) => {
                at <= guard && ui_guard::guard_owner(view) != Some(who.owner)
            }
            // A caller's screen the stack does not hold, under a guard: fail closed.
            (Some(_), None) => ui_guard::guard_owner(view) != Some(who.owner),
            (None, _) => false,
        };
    })?;
    if denied {
        return Err(ERR_DENIED);
    }
    Ok(())
}

/// A widget id without its placement (`x@2` is `x`).
fn base_of(widget: &str) -> Result<&str, i32> {
    let base = widget.split('@').next().unwrap_or("");
    if base.is_empty() {
        return Err(ERR_INVALID);
    }
    Ok(base)
}

fn instance_reply(instance: u32, flags: u32, out: &mut [u8]) -> Result<usize, i32> {
    codec::encode_one(
        &Record::ServiceUiInstance(wire::ServiceUiInstance { instance, flags }),
        out,
    )
}

/// Op 4: an instance of the widget for the caller's top screen instance.
pub(super) fn widget_make(
    frame: &dyn Frame,
    ui: &UiServices,
    who: &Caller<'_>,
    input: &[u8],
    out: &mut [u8],
) -> Result<usize, i32> {
    let reference: wire::ServiceUiWidgetRef<'_> = codec::single(input)?;
    let base = base_of(reference.widget)?;
    // Depth 1 only (ui.md, PR #419): a child holds no widgets.
    frame.callback_epoch()?;
    if frame.callback_depth()? != 1 {
        return Err(ERR_BUSY);
    }
    let screen = frame.calling_instance()?;
    // Only for the caller's own top instance (the one that gets input); a
    // screen below a guard is never the top.
    let mut on_top = false;
    let mut guard = None;
    frame.with_stack(&mut |view| {
        on_top = ui_guard::input_target(view) == Some(screen);
        guard = ui_guard::guard_owner(view);
    })?;
    if !on_top {
        return Err(ERR_STATE);
    }
    let declared = frame.ui_directory()?.widget(base)?;
    if declared.owner == who.owner {
        return instance_reply(0, INSTANCE_OWN, out);
    }
    if declared.private && guard.is_some_and(|g| g != who.owner) {
        return Err(ERR_DENIED);
    }
    let id = ui
        .widgets
        .handles
        .try_borrow_mut()
        .map_err(|_| ERR_BUSY)?
        .make_widget(
            declared.owner,
            who.owner,
            screen,
            declared.token,
            declared.private,
        )?;
    instance_reply(id, 0, out)
}

/// Op 5: drops an instance, for its maker only.
pub(super) fn widget_drop(ui: &UiServices, who: &Caller<'_>, input: &[u8]) -> Result<(), i32> {
    let instance: wire::ServiceUiInstance = codec::single(input)?;
    if instance.flags != 0 {
        return Err(ERR_INVALID);
    }
    ui.widgets
        .handles
        .try_borrow_mut()
        .map_err(|_| ERR_BUSY)?
        .drop_widget(who.owner, instance.instance)
}

/// The ids of an op 14 ask: 0 to 8 lines of 1 to 128 bytes.
fn split_items(items: &str) -> Result<([&str; ITEMS_MAX], usize), i32> {
    let mut lines = [""; ITEMS_MAX];
    if items.is_empty() {
        return Ok((lines, 0));
    }
    let mut count = 0;
    for line in items.split('\n') {
        if line.is_empty() || line.len() > ITEM_BYTES || count == ITEMS_MAX {
            return Err(ERR_INVALID);
        }
        lines[count] = line;
        count += 1;
    }
    Ok((lines, count))
}

/// One row of the reply under construction.
#[derive(Clone, Copy)]
struct Value {
    flags: u32,
    handle: u32,
    text: [u8; TEXT_BYTES],
    len: usize,
}

const EMPTY: Value = Value {
    flags: 0,
    handle: 0,
    text: [0; TEXT_BYTES],
    len: 0,
};

impl Value {
    fn set_text(&mut self, text: &[u8]) {
        let len = text.len().min(TEXT_BYTES);
        self.text[..len].copy_from_slice(&text[..len]);
        self.len = len;
    }

    fn text(&self) -> &str {
        self.text
            .get(..self.len)
            .and_then(|bytes| core::str::from_utf8(bytes).ok())
            .unwrap_or("")
    }
}

/// An item that needs the owner's call.
#[derive(Clone, Copy)]
struct Pending {
    owner: u64,
    token: u32,
    private: bool,
}

fn scratch(length: usize) -> Result<Vec<u8>, i32> {
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(length).map_err(|_| ERR_NO_MEMORY)?;
    bytes.resize(length, 0);
    Ok(bytes)
}

/// Sorts each item: no value, own, hidden, cached, or needing the owner's call.
fn classify(
    frame: &dyn Frame,
    ui: &UiServices,
    who: &Caller<'_>,
    context: &[u8],
    items: &[&str],
    rows: &mut [Value],
    pending: &mut [Option<Pending>; ITEMS_MAX],
) -> Result<(), i32> {
    if items.is_empty() {
        return Ok(());
    }
    let guarded = guard_of(frame)?.is_some_and(|g| g != who.owner);
    let directory = frame.ui_directory()?;
    for (at, item) in items.iter().enumerate() {
        let declared = match directory.status_item(item) {
            Ok(declared) => declared,
            // Nobody declared it: no value, as for an item without a row.
            Err(ERR_NOT_FOUND) => continue,
            Err(error) => return Err(error),
        };
        if declared.owner == who.owner {
            rows[at].flags = OWN;
        } else if declared.private && guarded {
            rows[at].flags = HIDDEN;
        } else if let Some(text) =
            ui.widgets
                .cache
                .try_borrow()
                .map_err(|_| ERR_BUSY)?
                .get(declared.owner, item, context)
        {
            rows[at].set_text(text);
        } else {
            pending[at] = Some(Pending {
                owner: declared.owner,
                token: declared.token,
                private: declared.private,
            });
        }
    }
    Ok(())
}

/// The kernel's own items among those nobody declared (`host_status`): a
/// text, or a status handle of the host for a drawn value, valid in the
/// caller's `callback` only. No room for another handle leaves the item
/// without a value, as for a plugin's.
fn host_items(
    frame: &dyn Frame,
    ui: &UiServices,
    (caller, callback): (u64, u64),
    items: &[&str],
    pending: &[Option<Pending>; ITEMS_MAX],
    rows: &mut [Value],
) -> Result<(), i32> {
    for (at, item) in items.iter().enumerate() {
        let Some(row) = rows.get_mut(at) else {
            break;
        };
        if pending[at].is_some() || row.flags != 0 || row.len != 0 {
            continue;
        }
        let mut text = [0u8; TEXT_BYTES];
        match frame.host_status(item, &mut text) {
            Some(HostStatus::Text(len)) => row.set_text(text.get(..len).unwrap_or(&[])),
            Some(HostStatus::Drawn(token)) => {
                let granted = ui
                    .widgets
                    .handles
                    .try_borrow_mut()
                    .map_err(|_| ERR_BUSY)?
                    .grant_status(caller, callback, host_status::HOST, token, false);
                match granted {
                    Ok(handle) => {
                        row.flags = DRAWN;
                        row.handle = handle;
                    }
                    Err(ERR_FULL) => {}
                    Err(error) => return Err(error),
                }
            }
            None => {}
        }
    }
    Ok(())
}

/// One owner call for the items `group[..members]` and the rows it fills.
#[allow(
    clippy::too_many_arguments,
    reason = "one step of op 14 that needs the whole call state"
)]
fn fill_group(
    frame: &dyn Frame,
    ui: &UiServices,
    who: &Caller<'_>,
    epoch: u64,
    context: &[u8],
    items: &[&str],
    lead: Pending,
    group: &[usize],
    pending: &[Option<Pending>; ITEMS_MAX],
    rows: &mut [Value],
) -> Result<(), i32> {
    let mut ids = [""; ITEMS_MAX];
    for (id, at) in ids.iter_mut().zip(group) {
        *id = items[*at];
    }
    let ids = &ids[..group.len()];
    let mut reply = scratch(status::MAX_REPLY)?;
    let length = match frame.status_ask(lead.owner, lead.token, ids, context, &mut reply) {
        Ok(length) => length,
        // A failed owner call (its own refusal for memory too): its items have
        // no value, the rest go on.
        Err(_) => return Ok(()),
    };
    let Some(bytes) = reply.get(..length) else {
        return Ok(());
    };
    let Ok(records) = status::reply_decode(bytes) else {
        return Ok(());
    };
    let mut filled = [false; ITEMS_MAX];
    for record in records {
        let Ok(status::Record::StatusValue(value)) = record else {
            continue;
        };
        let Some(member) = ids.iter().position(|id| *id == value.item) else {
            continue;
        };
        if filled[member] {
            continue;
        }
        filled[member] = true;
        let at = group[member];
        if value.flags & DRAWN != 0 {
            let granted = ui
                .widgets
                .handles
                .try_borrow_mut()
                .map_err(|_| ERR_BUSY)?
                .grant_status(who.owner, epoch, lead.owner, lead.token, false);
            match granted {
                Ok(handle) => {
                    rows[at].flags = DRAWN;
                    rows[at].handle = handle;
                }
                // No room for another handle: the item has no value.
                Err(ERR_FULL) => {}
                Err(error) => return Err(error),
            }
        } else {
            rows[at].set_text(value.text.as_bytes());
            ui.widgets
                .cache
                .try_borrow_mut()
                .map_err(|_| ERR_BUSY)?
                // Each item's own flag, not the group's first (fail closed).
                .put(
                    lead.owner,
                    items[at],
                    context,
                    pending[at].is_none_or(|p| p.private),
                    value.text,
                );
        }
    }
    Ok(())
}

/// Op 14: the values of up to 8 status items.
pub(super) fn status_values(
    frame: &dyn Frame,
    ui: &UiServices,
    who: &Caller<'_>,
    input: &[u8],
    out: &mut [u8],
) -> Result<usize, i32> {
    let ask: wire::ServiceUiStatusAsk<'_> = codec::single(input)?;
    // The kernel's own items need no nested call: on the X4, until its stack
    // gate passes, only the plugins' items go without a value (todo 598).
    let epoch = frame.callback_epoch()?;
    if frame.callback_depth()? != 1 {
        return Err(ERR_BUSY);
    }
    let closed = frame.nested_closed()?;
    below_guard_gate(frame, who)?;
    let (items, count) = split_items(ask.items)?;
    let items = &items[..count];
    let mut rows: Vec<Value> = Vec::new();
    rows.try_reserve_exact(count).map_err(|_| ERR_NO_MEMORY)?;
    rows.resize(count, EMPTY);
    let mut pending: [Option<Pending>; ITEMS_MAX] = [None; ITEMS_MAX];
    classify(frame, ui, who, ask.context, items, &mut rows, &mut pending)?;
    host_items(frame, ui, (who.owner, epoch), items, &pending, &mut rows)?;
    if closed {
        pending = [None; ITEMS_MAX];
    }
    // One call per (owner, token), groups in the order of their first item.
    let mut done = [false; ITEMS_MAX];
    for first in 0..count {
        let Some(lead) = pending[first].filter(|_| !done[first]) else {
            continue;
        };
        let mut group = [0_usize; ITEMS_MAX];
        let mut members = 0;
        for at in first..count {
            if pending[at].is_some_and(|p| p.owner == lead.owner && p.token == lead.token)
                && !done[at]
            {
                done[at] = true;
                group[members] = at;
                members += 1;
            }
        }
        fill_group(
            frame,
            ui,
            who,
            epoch,
            ask.context,
            items,
            lead,
            &group[..members],
            &pending,
            &mut rows,
        )?;
    }
    let mut used = 0;
    for (item, row) in items.iter().zip(&rows) {
        used += codec::encode_one(
            &Record::ServiceUiStatusValue(wire::ServiceUiStatusValue {
                item,
                flags: row.flags,
                handle: row.handle,
                text: row.text(),
            }),
            out.get_mut(used..).ok_or(ERR_INVALID)?,
        )?;
    }
    let end = wire::ServiceEnd {
        flags: 0,
        total: u32::try_from(count).map_err(|_| ERR_INVALID)?,
        generation: 0,
        next: 0,
        cursor: None,
    };
    used += codec::encode_one(
        &Record::ServiceEnd(end),
        out.get_mut(used..).ok_or(ERR_INVALID)?,
    )?;
    Ok(used)
}

/// Op 15: a handle for a status line drawn by its owner.
pub(super) fn status_line(
    frame: &dyn Frame,
    ui: &UiServices,
    who: &Caller<'_>,
    input: &[u8],
    out: &mut [u8],
) -> Result<usize, i32> {
    let ask: wire::ServiceUiStatusAsk<'_> = codec::single(input)?;
    let epoch = nested_gate(frame)?;
    below_guard_gate(frame, who)?;
    if ask.items.is_empty() || ask.items.len() > ITEM_BYTES || ask.items.contains('\n') {
        return Err(ERR_INVALID);
    }
    let guard = guard_of(frame)?;
    let declared = frame.ui_directory()?.status_line(ask.items)?;
    if declared.owner == who.owner {
        return instance_reply(0, INSTANCE_OWN, out);
    }
    if declared.private && guard.is_some_and(|g| g != who.owner) {
        return Err(ERR_DENIED);
    }
    let handle = ui
        .widgets
        .handles
        .try_borrow_mut()
        .map_err(|_| ERR_BUSY)?
        .grant_status(who.owner, epoch, declared.owner, declared.token, true)?;
    instance_reply(handle, 0, out)
}

#[cfg(test)]
#[path = "ui_widget_ops_tests.rs"]
mod tests;
