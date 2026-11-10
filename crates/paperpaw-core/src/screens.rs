//! The stack of screens: opening and closing them, overlays and the parts
//! of the screen they take, the notice, and calls into the screens.

#[allow(
    unused_imports,
    reason = "a part of the kernel, which sees all of it; not every part uses every name"
)]
use super::*;
use paperpaw_sdk::Pane;

/// A screen on the navigation stack.
pub(crate) struct ActiveScreen {
    /// Set only by the host when authorizing a guard question dialog.
    pub(crate) guard_dialog: bool,
    pub(crate) id: Id,
    /// Tells this opening of the screen from every other, also once it was
    /// made anew (see `Kernel::remake_screens`): e.g. the owner of a
    /// settings scope.
    pub(crate) serial: u32,
    /// The slot it was opened for, if any.
    pub(crate) slot: Option<Id>,
    /// Made for the part above the row of hints (when the row shows), as
    /// it asked ([`HintVisibility::Shown`]).
    pub(crate) hinted: bool,
    /// The part of the screen it is made for and drawn into: all of it
    /// but the bars shown over it and the row of hints (see
    /// [`Env::content`]).
    pub(crate) area: Rect,
    /// What it was opened with, e.g. the book.
    pub(crate) params: Params,
    pub(crate) screen: Box<dyn Screen>,
    /// When it wants [`Screen::on_timer`] ([`Cx::wake_at`]); below another
    /// screen, once it is on top again.
    pub(crate) timer: Option<Instant>,
    /// The key map layer put in front of its slot's while it is on top
    /// ([`Command::SetKeyLayer`]).
    pub(crate) key_layer: Option<Id>,
    /// Opened above a screen guarding the device by that screen (or one
    /// it opened): its keyboard. Others above it (a sleep screen) get no
    /// input and are closed as the device wakes.
    pub(crate) by_guard: bool,
    /// The policy bits a native plugin set at run time
    /// (`command.screen_policy`, `docs/abi/ui.md` 1.2): bit 1 guards, bit 4
    /// secret, bit 5 holds edges. It replaces the screen's own answers for
    /// those three and stays when the screen is made anew.
    pub(crate) policy: Option<u32>,
    /// How it shows where the screen is wide ([`Screen::pane`]), as it
    /// said when it was made.
    pub(crate) pane: paperpaw_sdk::Pane,
    /// `event.screen_ready` (87) was raised for this instance, or it will not
    /// be (`screen_ready`): at most once an instance.
    pub(crate) ready: bool,
    /// A draw of this instance happened since it was opened and 87 is yet to
    /// be raised for it (the pass after the draw raises it).
    pub(crate) drawn: bool,
}

/// The policy bits `command.screen_policy` sets: `guards` (1), `secret` (4),
/// `holds_edges` (5).
pub(crate) const POLICY_GUARDS: u32 = 1 << 1;
pub(crate) const POLICY_SECRET: u32 = 1 << 4;
pub(crate) const POLICY_EDGES: u32 = 1 << 5;
pub(crate) const POLICY_BITS: u32 = POLICY_GUARDS | POLICY_SECRET | POLICY_EDGES;

impl ActiveScreen {
    /// Whether it guards the device: what a native plugin set at run time,
    /// else the screen's own answer.
    pub(crate) fn guards(&self) -> bool {
        self.policy
            .map_or_else(|| self.screen.guards(), |bits| bits & POLICY_GUARDS != 0)
    }

    /// Whether what it shows is secret (a guard always is).
    pub(crate) fn secret(&self) -> bool {
        self.policy.map_or_else(
            || self.screen.secret(),
            |bits| bits & (POLICY_GUARDS | POLICY_SECRET) != 0,
        )
    }

    /// Whether it holds the edge swipes.
    pub(crate) fn holds_edges(&self) -> bool {
        self.policy.map_or_else(
            || self.screen.holds_edges(),
            |bits| bits & POLICY_EDGES != 0,
        )
    }
}

/// The revision of `Screen` that brought `Screen::times_under_overlays`
/// (see `paperpaw_sdk::SCREEN_REVISION`): asked only of screens built
/// with it or later.
const TIMES_UNDER_OVERLAYS_SINCE: u16 = 3;

/// How many tasks not shown are kept ([`Command::SwitchTask`]).
const TASKS_KEPT: usize = 8;
const SCREEN_KEYS_EXHAUSTED: &str = "screen key space exhausted";

fn next_screen_serial(next: u32, native: bool) -> Option<(u32, u32)> {
    let serial = if native { next.max(1) } else { next };
    serial.checked_add(1).map(|following| (serial, following))
}

impl Kernel {
    /// Closed room screens keep enough of the unused key space to reopen.
    pub(crate) fn screen_keys_remaining(&self) -> u32 {
        (u32::MAX - self.next_serial)
            .saturating_sub(u32::try_from(self.room_screen_keys_reserved()).unwrap_or(u32::MAX))
    }

    pub(crate) fn require_screen_keys(&mut self, count: usize) -> bool {
        if u32::try_from(count).is_ok_and(|count| count <= self.screen_keys_remaining()) {
            return true;
        }
        self.warn_runtime(SCREEN_KEYS_EXHAUSTED);
        false
    }

    /// Factory failure may be caused by persistent OOM. Reporting must not
    /// allocate infallibly or expose the screen's private params.
    fn warn_native_open_failed(&mut self) {
        const MESSAGE: &str = "Could not open native screen";
        let mut message = String::new();
        if message.try_reserve_exact(MESSAGE.len()).is_err()
            || self.warnings.try_reserve(1).is_err()
        {
            return;
        }
        message.push_str(MESSAGE);
        self.warnings.push(Warning {
            source: WarningSource::Runtime,
            line: None,
            message,
        });
    }

    fn screen_stack_has_room(&mut self) -> bool {
        if self.shared.stack_rows.cleanup_pending() {
            self.warn_runtime("UI instance cleanup is still pending");
            return false;
        }
        if self.stack.len() >= native_host::stack_rows::STACK_MAX {
            self.warn_runtime("screen stack is full");
            return false;
        }
        true
    }

    /// Opens screen `id` with `params` on top, for `slot` if it was bound
    /// to one.
    pub(crate) fn open(&mut self, id: Id, slot: Option<Id>, params: &Params) {
        if !self.screen_stack_has_room() || !self.require_screen_keys(1) {
            return;
        }
        let Some(spec) = self.shared.registry.screens.get(&id) else {
            return self.warn_runtime(format!("unknown screen `{id}`"));
        };
        // Native allocation finishes before changing the serial, layout or screen.
        let Ok(native_screen) = spec.prepare_native(params) else {
            self.warn_native_open_failed();
            return;
        };
        let on_open = spec.refresh_on_open();
        let Some((serial, next_serial)) = next_screen_serial(self.next_serial, spec.is_native())
        else {
            return self.warn_runtime(SCREEN_KEYS_EXHAUSTED);
        };
        self.next_serial = next_serial;
        let key = paperpaw_sdk::ScreenKey::new(serial);
        let entry = paperpaw_sdk::StackEntry::new(id.clone(), slot.clone(), false, key);
        // Made once, at the size it asked for: above the row of hints, or
        // all of the screen, less the bars shown over it.
        let mut hinted = self.shared.registry.hints_of(&id) == HintVisibility::Shown;
        let mut area = self.layout(hinted, Some(&entry), None);
        set_area(&mut self.shared.device, &mut self.shared.content, area);
        let running = self.shared.meter.enter(id.namespace());
        let mut screen = match native_screen {
            Some(screen) => screen,
            None => {
                // Legacy factories are infallible; this also keeps their sizing order.
                let Ok(screen) = spec.make(&self.shared.env(self.now), params) else {
                    return;
                };
                screen
            }
        };
        // It says otherwise of the row of hints: made again for the part
        // of the screen it then has.
        let own = match screen.mode().is_full_screen() {
            // All of the panel its own: no row of hints.
            true => Some(HintVisibility::Hidden),
            false => screen.hints(),
        };
        if let Some(own) = own
            && (own == HintVisibility::Shown) != hinted
        {
            hinted = !hinted;
            area = self.layout(hinted, Some(&entry), None);
            set_area(&mut self.shared.device, &mut self.shared.content, area);
            spec.remake(&mut screen, &self.shared.env(self.now), params);
        }
        // An overlay shares its full screen's part, a guard has no bars:
        // made again for that part.
        let place = self.area_on_top(screen.as_ref(), hinted, &entry, area);
        if place != area {
            area = place;
            set_area(&mut self.shared.device, &mut self.shared.content, area);
            spec.remake(&mut screen, &self.shared.env(self.now), params);
        }
        let pane = paperpaw_sdk::call_since(screen.as_mut(), 2, Pane::FULL, |s| s.pane()).known();
        drop(running);
        // A detail beside the list on top, where the screen is wide: in
        // place of the detail shown there where the list opened it, made
        // again for its part.
        let full = screen.region(&self.shared.env(self.now)).is_none();
        self.close_detail_for(pane, full);
        let beside = self.beside_top(screen.as_ref(), pane, area);
        if let Some(part) = beside
            && let Some(spec) = self.shared.registry.screens.get(&id)
        {
            area = part;
            set_area(&mut self.shared.device, &mut self.shared.content, area);
            let _running = self.shared.meter.enter(id.namespace());
            // The one made for all of it goes first (see `Gone`).
            if !spec.is_native() {
                drop(mem::replace(&mut screen, Box::new(Gone)));
                spec.remake(&mut screen, &self.shared.env(self.now), params);
            }
        }
        let active = ActiveScreen {
            guard_dialog: false,
            id,
            serial,
            slot,
            hinted,
            area,
            params: params.clone(),
            screen,
            timer: None,
            key_layer: None,
            by_guard: self.guard().is_some() && self.from_guard_own,
            policy: None,
            pane,
            ready: false,
            drawn: false,
        };
        self.push_opened(active, params, beside.is_some(), on_open);
    }

    /// Puts the screen just made on the stack (below a guard, or on top) and
    /// tells it and the display; `beside` when it is shown beside the list.
    fn push_opened(
        &mut self,
        active: ActiveScreen,
        params: &Params,
        beside: bool,
        on_open: Option<paperpaw_sdk::RefreshMode>,
    ) {
        // Opened by another while a screen guards the device: it waits
        // below the guard, hidden.
        if let Some(guard) = self.guard().filter(|_| !self.from_guard) {
            self.stack.insert(guard, active);
            self.sync_stack();
            self.hidden_screen_opened(guard, params);
            return;
        }
        // The list a detail shows beside stays shown.
        if active.screen.region(&self.shared.env(self.now)).is_none() && !beside {
            self.hide_from(self.shown_base());
        }
        self.screens_changed = true;
        self.stack.push(active);
        self.sync_stack();
        let top = self.stack.len() - 1;
        self.native_screen_opened(top, params);
        let overlay = self.overlay(top);
        self.screen_changed(self.changed_region(top), overlay.is_some());
        // As the screen asks, where it does.
        if let Some(mode) = on_open {
            self.output.refresh_set = Some(mode);
        }
    }

    fn hidden_screen_opened(&mut self, index: usize, params: &Params) {
        {
            let _running = self.shared.meter.enter(self.stack[index].id.namespace());
            self.stack[index].screen.on_hide();
        }
        self.native_screen_opened(index, params);
    }

    fn native_screen_opened(&mut self, index: usize, params: &Params) {
        if self.is_native_at(index) {
            // The private wrapper uses this hook to announce an instance before
            // it is shown, including instances hidden below a guarding screen.
            self.on_reopen_with_handover(index, params);
        }
    }

    /// Opens screen `id` as [`Command::OpenScreen`] asks: where one of it
    /// is open that may not be opened twice ([`paperpaw_sdk::Instances`]),
    /// goes back to that one instead, closing the screens above it, and
    /// gives it the params ([`Screen::on_reopen`]). *Back* then leaves it
    /// in one press.
    pub(crate) fn open_once(&mut self, id: Id, slot: Option<Id>, params: &Params) {
        let Some(instances) = self
            .shared
            .registry
            .screens
            .get(&id)
            .map(|spec| spec.instances())
        else {
            return self.open(id, slot, params);
        };
        let open = match instances {
            paperpaw_sdk::Instances::SINGLE => {
                self.stack.iter().rposition(|active| active.id == id)
            }
            paperpaw_sdk::Instances::MANY => None,
            // One for each set of params, also for kinds to come.
            _ => self
                .stack
                .iter()
                .rposition(|active| active.id == id && self.same_params(active, params)),
        };
        let Some(index) = open else {
            return self.open(id, slot, params);
        };
        // Below a screen that guards the device, for another: it stays
        // there, as it is.
        if self.held_by_guard() && self.guard() > Some(index) {
            return;
        }
        if index + 1 < self.stack.len() {
            if !self.may_close_above(index, paperpaw_sdk::CloseCause::Slot) {
                return;
            }
            self.close_above(index);
        }
        self.reopen(index, params);
    }

    /// Gives the open instance `index` the params of a new open
    /// ([`Screen::on_reopen`]), and the hand-over that open names.
    pub(crate) fn reopen(&mut self, index: usize, params: &Params) {
        self.stack[index].params = params.clone();
        self.on_reopen_with_handover(index, params);
    }

    /// Whether the screen at `index` is a native one.
    pub(crate) fn is_native_at(&self, index: usize) -> bool {
        self.stack
            .get(index)
            .and_then(|active| self.shared.registry.screens.get(&active.id))
            .is_some_and(|maker| maker.is_native())
    }

    /// `Screen::on_reopen` of instance `index`, after the hand-over the
    /// applied `open` names went to it (docs/abi/ui.md 1.1 step 5; an open of
    /// none clears its params and text). A screen that is not native keeps
    /// what it has and leaves the hand-over to the drain.
    fn on_reopen_with_handover(&mut self, index: usize, params: &Params) {
        let info = self.attach_pending_handover(index);
        self.shared.native_instances.set_open_info(info);
        self.with_screen(index, |screen, cx| {
            screen.on_reopen(params, cx);
            Handled::Yes
        });
        // Whatever `on_reopen` left: never for another screen's.
        self.shared.native_instances.set_open_info(None);
    }

    /// The hand-over the applied `open` names goes to native instance
    /// `index`: what its `screen_opened` says of it comes back (none for no
    /// staged open).
    fn attach_pending_handover(
        &mut self,
        index: usize,
    ) -> Option<crate::native_host::ui_store::Info> {
        if !self.is_native_at(index) {
            return None;
        }
        let serial = self.stack.get(index)?.serial;
        let pending = self.shared.native_instances.take_pending_handover()?;
        let store = self.shared.native_instances.ui_store()?;
        let attached = store.attach(pending, serial);
        let info = attached.is_ok().then(|| store.info(serial)).flatten();
        if attached.is_err() {
            if let Some(id) = pending {
                let _ = store.release(id);
            }
            self.warn_runtime("a screen's hand-over could not be attached; it opens without");
        }
        Some(info.unwrap_or(crate::native_host::ui_store::Info::NONE))
    }

    /// Whether `active` has the params an open brings: where a staged open
    /// names a hand-over (or none), its encoded params are compared, as its
    /// text is not.
    fn same_params(&self, active: &ActiveScreen, params: &Params) -> bool {
        let same = active.params == *params;
        match (
            self.shared.native_instances.pending_handover(),
            self.shared.native_instances.ui_store(),
        ) {
            (Some(pending), Some(store)) => same && store.same_params(pending, active.serial),
            _ => same,
        }
    }

    /// Whether the screen open for a slot at `index` already has the params an
    /// open brings (docs/abi/ui.md 1.1: modes 1 and 3 compare them): where a
    /// staged open names a hand-over (or none), its params are compared, as
    /// its text is not.
    pub(crate) fn slot_open_same(&self, index: usize, params: &Params) -> bool {
        match (
            self.shared.native_instances.pending_handover(),
            self.shared.native_instances.ui_store(),
        ) {
            // A screen that is not native owns no hand-over: it keeps the
            // rule of params it has.
            (Some(pending), Some(store)) if self.is_native_at(index) => {
                store.same_params(pending, self.stack[index].serial)
            }
            _ => params.is_empty() || self.stack[index].params == *params,
        }
    }

    /// The topmost screen that guards the device ([`Screen::guards`]).
    pub(crate) fn guard(&self) -> Option<usize> {
        self.stack.iter().rposition(ActiveScreen::guards)
    }

    /// Whether a screen guards the device and the command being carried
    /// out is not its own (nor of a screen above it).
    pub(crate) fn held_by_guard(&self) -> bool {
        !self.from_guard && self.guard().is_some()
    }

    /// Shows the notice held while a screen guarded the device, once none
    /// does.
    pub(crate) fn release_held_notice(&mut self) {
        if self.guard().is_none()
            && let Some(text) = self.held_notice.take()
        {
            self.notify(text);
        }
    }

    /// The topmost full screen, or the list beside it where it is its
    /// detail ([`Kernel::panes`]): it and the screens above it show.
    pub(crate) fn shown_base(&self) -> usize {
        self.panes()
            .map_or_else(|| self.full_base(), |(list, _)| list)
    }

    /// Tells the screens from `index` on that a full screen covers them
    /// now ([`Screen::on_hide`]).
    fn hide_from(&mut self, index: usize) {
        for active in self.stack.iter_mut().skip(index) {
            let _running = self.shared.meter.enter(active.id.namespace());
            active.screen.on_hide();
        }
    }

    /// Shows `text` with the screen bound to the notice slot until the
    /// notice time is over.
    pub(crate) fn notify(&mut self, text: String) {
        if self.held_by_guard() {
            self.held_notice = Some(text);
            return;
        }
        let Some(id) = self
            .shared
            .registry
            .resolve_slot(slots::NOTICE.as_str())
            .cloned()
        else {
            return;
        };
        let Some(spec) = self.shared.registry.screens.get(&id) else {
            return;
        };
        let env = self.shared.env(self.now);
        let params = Params::new().with(builtin_params::TEXT, text);
        let running = self.shared.meter.enter(id.namespace());
        let Ok(screen) = spec.make(&env, &params) else {
            return;
        };
        drop(running);
        let seconds = self
            .shared
            .settings
            .int(core_settings::NOTICE_SECONDS.as_str())
            .clamp(1, 60) as u32;
        let region = notice_area(screen.as_ref(), &env);
        self.end_notice();
        self.notice = Some((screen, self.now.add_millis(u64::from(seconds) * 1000)));
        self.invalidate(region);
        self.output.request_refresh(RefreshMode::Fast);
        self.notice_cause();
    }

    /// Binds an input for the user (see [`Command::Bind`]).
    pub(crate) fn bind(&mut self, layer: &str, input: &str, action: Option<Id>) {
        self.shared.registry_generation.bump();
        let valid_layer = layer == GLOBAL_KEYMAP_LAYER || Id::parse(layer).is_ok();
        if !valid_layer || !keymap::is_input(input) {
            return self.warn_runtime(format!("cannot bind `{input}` in `{layer}`"));
        }
        let section = format!("keymap.{layer}");
        let action = action.as_ref();
        // At once, also where it cannot be saved. The shell's overrides stay
        // above the user's bindings, but not for an input bound now: the
        // choice made here wins (until the next start, where the shell's
        // configuration is applied again).
        self.keys.user = bound(&self.keys.user, &section, input, action);
        self.keys.shell = paperpaw_config::remove(&self.keys.shell, &section, input);
        self.update_config(&format_args!("`{section}.{input}`"), |source| {
            bound(source, &section, input, action)
        });
        self.rebuild_keymap();
    }

    /// Takes the notice away, if one shows.
    pub(crate) fn end_notice(&mut self) {
        let Some((screen, _)) = self.notice.take() else {
            return;
        };
        let env = self.shared.env(self.now);
        let region = notice_area(screen.as_ref(), &env);
        self.invalidate(region);
        self.output.request_refresh(RefreshMode::Fast);
        self.notice_cause();
    }

    /// The refresh is for the notice, where nothing else said why.
    fn notice_cause(&mut self) {
        let cause = &mut self.output.refresh_cause;
        cause.get_or_insert(paperpaw_sdk::filters::RefreshCause::Notice);
    }

    /// Closes the screens from `index` on, the top one first, telling
    /// each ([`Screen::on_close`]).
    pub(crate) fn close_from(&mut self, index: usize) {
        for i in (index..self.stack.len()).rev() {
            self.with_screen(i, |screen, cx| {
                screen.on_close(cx);
                Handled::Yes
            });
            self.cleanup_ui_instance(self.stack[i].serial);
            self.stack.truncate(i);
            self.screens_changed = true;
        }
        self.sync_stack();
        self.sync_content();
    }

    /// The open screens as plugins see them ([`Env::stack`]), after the
    /// stack changed.
    pub(crate) fn sync_stack(&mut self) {
        let env = self.shared.env(self.now);
        let entries: Vec<paperpaw_sdk::StackEntry> = self
            .stack
            .iter()
            .map(|active| {
                let overlay = active.screen.region(&env).is_some();
                let key = paperpaw_sdk::ScreenKey::new(active.serial);
                paperpaw_sdk::StackEntry::new(active.id.clone(), active.slot.clone(), overlay, key)
                    .guarding(active.guards())
            })
            .collect();
        debug_assert_eq!(self.shared.native_instances.depth(), 0);
        let guard_before = native_host::ui_guard::guard_instance(&*self.shared.stack_rows);
        self.shared
            .stack_rows
            .update(self.stack.iter().zip(&entries).map(|(active, entry)| {
                use native_host::stack_rows::{
                    GUARD_DIALOG, GUARDS, HOST, OVERLAY, SECRET, StackRow,
                };
                let host = active.id.namespace() == "core";
                let owner = if host {
                    0
                } else {
                    self.shared
                        .native_instances
                        .find(active.id.namespace())
                        .unwrap_or(0)
                };
                StackRow {
                    instance: active.serial,
                    owner,
                    flags: (u8::from(entry.overlay) * OVERLAY)
                        | (u8::from(entry.guards) * GUARDS)
                        | (u8::from(active.secret()) * SECRET)
                        | (u8::from(host) * HOST)
                        | (u8::from(active.guard_dialog) * GUARD_DIALOG),
                }
            }));
        // A guard that is new (or another one) drops the private status values
        // cached before it; a refusal is harmless, private items are hidden
        // under a guard before the cache is read.
        let guard_after = native_host::ui_guard::guard_instance(&*self.shared.stack_rows);
        if guard_after.is_some()
            && guard_after != guard_before
            && let Some(ui) = self.shared.native_instances.ui_services()
            && ui.guard_started().is_err()
        {
            self.warn_runtime("status cache not cleared at guard start");
        }
        self.shared.stack.clear();
        self.shared.stack.extend(entries);
        self.sync_chromes();
        self.sync_pane_keys();
        self.guard_takes_fingers();
    }

    /// Closes the open screen `key` (see [`Command::Close`]), leaving the
    /// screens above it; not the only one, nor a screen guarding the device
    /// for another. What it started (servers, sockets, speech) runs on
    /// until the screens above it close too.
    pub(crate) fn close_screen(&mut self, key: paperpaw_sdk::ScreenKey) {
        let Some(index) = self
            .stack
            .iter()
            .position(|active| paperpaw_sdk::ScreenKey::new(active.serial) == key)
        else {
            return;
        };
        if self.stack.len() < 2 || (self.held_by_guard() && self.guard() == Some(index)) {
            return;
        }
        if index + 1 == self.stack.len() {
            return self.close_above(index - 1);
        }
        let base = self.shown_base();
        let region = self.changed_region(index);
        self.with_screen(index, |screen, cx| {
            screen.on_close(cx);
            Handled::Yes
        });
        self.cleanup_ui_instance(self.stack[index].serial);
        self.stack.remove(index);
        self.screens_changed = true;
        self.sync_stack();
        if index == base {
            // The full screen shown went: what was below it shows.
            let content = self.content();
            self.hint.shown.clear();
            self.invalidate(content);
            self.output.request_refresh(RefreshMode::Partial);
        } else if index > base {
            self.invalidate(region);
            self.output.request_refresh(RefreshMode::Partial);
        }
    }

    fn cleanup_ui_instance(&mut self, instance: u32) {
        if let Err(status) = self.native_instance_closed(instance) {
            if status == paperpaw_native::host_glue::ERR_BUSY {
                if let Err(error) = self.shared.stack_rows.retry_cleanup(instance) {
                    self.shared.stack_rows.note_close_error(error);
                }
            } else {
                self.warn_runtime("UI instance cleanup failed");
            }
        }
    }

    pub(crate) fn drain_deferred_ui_closes(&mut self) {
        debug_assert_eq!(self.shared.native_instances.depth(), 0);
        if self.shared.stack_rows.take_close_error() != 0 {
            self.warn_runtime("deferred UI cleanup queue is full");
        }
        // Snapshot the work first. Busy entries are retried next main-loop pass.
        for action in self.shared.stack_rows.take_pending() {
            let instance = action.instance;
            if instance == 0 {
                continue;
            }
            if action.close {
                if let Some(index) = self
                    .stack
                    .iter()
                    .position(|screen| screen.serial == instance)
                {
                    self.with_screen(index, |screen, cx| {
                        screen.on_close(cx);
                        Handled::Yes
                    });
                    self.cleanup_ui_instance(instance);
                    self.stack.remove(index);
                    self.screens_changed = true;
                    self.sync_stack();
                    self.stop_for_closed_screens();
                    self.screen_changed(self.content(), false);
                } else {
                    self.cleanup_ui_instance(instance);
                }
            }
        }
        self.sweep_closed_ui_instances();
    }

    /// Recover closed instances not kept in the fixed cleanup queue. UiStore
    /// is authoritative for retained resources; the real stack protects live ones.
    fn sweep_closed_ui_instances(&mut self) {
        if !self.shared.stack_rows.take_cleanup_sweep() {
            return;
        }
        let result: Result<(), i32> = (|| {
            let Some(store) = self.shared.native_instances.ui_store() else {
                return Ok(());
            };
            let mut after = 0;
            while let Some(instance) = store.next_retained_instance(after)? {
                after = instance;
                if !self.stack.iter().any(|screen| screen.serial == instance) {
                    self.native_instance_closed(instance)?;
                }
            }
            // Widget instances of closed screens the store no longer holds.
            let mut after = 0;
            while let Some(ui) = self.shared.native_instances.ui_services()
                && let Some(instance) = ui.next_widget_screen(after)?
            {
                after = instance;
                if !self.stack.iter().any(|screen| screen.serial == instance) {
                    self.native_instance_closed(instance)?;
                }
            }
            Ok(())
        })();
        if let Err(status) = result {
            self.shared.stack_rows.request_cleanup_sweep();
            if status != paperpaw_native::host_glue::ERR_BUSY {
                self.warn_runtime("UI instance cleanup sweep failed; retry pending");
            }
        }
    }

    /// A retired owner cannot veto removal, including its root or guard.
    pub(crate) fn remove_retired_native_screens(&mut self, owner: &Id) {
        let mut removed = false;
        for index in (0..self.stack.len()).rev() {
            if self.stack[index].id.namespace() == owner.as_str() {
                // The owner is revoked already. Dropping its private wrapper
                // releases the lease without calling retired plugin code.
                self.cleanup_ui_instance(self.stack[index].serial);
                self.stack.remove(index);
                removed = true;
            }
        }
        if removed {
            self.screens_changed = true;
            self.sync_stack();
            self.stop_for_closed_screens();
            self.screen_changed(self.content(), false);
        }
    }

    /// Whether the screens above `index` may close for `why` (see
    /// [`Kernel::may_close_from`]).
    pub(crate) fn may_close_above(&mut self, index: usize, why: paperpaw_sdk::CloseCause) -> bool {
        self.may_close_from(index + 1, why)
    }

    /// Whether the screens from `index` on may close for `why`: each is
    /// asked, the top one first ([`Screen::on_close_request`]).
    pub(crate) fn may_close_from(&mut self, index: usize, why: paperpaw_sdk::CloseCause) -> bool {
        (index..self.stack.len()).rev().all(|i| {
            self.with_screen(i, |screen, cx| screen.on_close_request(why, cx)) == Handled::No
        })
    }

    /// Closes the screen on top for `why` (*Back*, a tap outside it), if
    /// it agrees ([`Screen::on_close_request`]); never the only one.
    pub(crate) fn close_top(&mut self, why: paperpaw_sdk::CloseCause) {
        let Some(below) = self.stack.len().checked_sub(2) else {
            return;
        };
        if self.may_close_above(below, why) {
            self.close_above(below);
        }
    }

    /// Opens screen `id` in place of the one on top ([`Command::Replace`]).
    pub(crate) fn replace(&mut self, id: Id, params: &Params) {
        if !self.require_screen_keys(1) {
            return;
        }
        // Not in place of a screen guarding the device: below it.
        if self.held_by_guard() {
            return self.open(id, None, params);
        }
        let Some(top) = self.stack.len().checked_sub(1) else {
            return self.open(id, None, params);
        };
        let closed = self.changed_region(top);
        self.close_from(top);
        self.stop_for_closed_screens();
        self.open(id, None, params);
        self.invalidate(closed);
    }

    /// Closes the screens above the topmost screen `id`, or the screen of
    /// slot `id` ([`Command::BackTo`]).
    pub(crate) fn back_to(&mut self, id: &Id) {
        let Some(index) = self
            .stack
            .iter()
            .rposition(|active| active.id == *id || active.slot.as_ref() == Some(id))
        else {
            return;
        };
        let guarded = self.held_by_guard() && self.guard() > Some(index);
        if index + 1 < self.stack.len()
            && !guarded
            && self.may_close_above(index, paperpaw_sdk::CloseCause::Slot)
        {
            self.close_above(index);
        }
    }

    /// Shows task `id` (see [`Command::SwitchTask`]): the screens open
    /// now kept as the task shown before, by ids and params, and task
    /// `id`'s opened again (the first time its slot, or its screen). Not
    /// past a screen that guards the device.
    pub(crate) fn switch_task(&mut self, id: Id) {
        let current = self.task.clone().unwrap_or(slots::HOME);
        // Not while a screen guards the device, whoever asks (also the
        // guard, or a screen it opened).
        if id == current || self.guard().is_some() {
            return;
        }
        // Not to what cannot be shown: nothing is closed for it.
        let known = self
            .tasks
            .iter()
            .any(|(task, screens)| *task == id && !screens.is_empty())
            || self.shared.registry.resolve_slot(id.as_str()).is_some()
            || self.shared.registry.screens.contains_key(&id);
        if !known {
            return self.warn_runtime(format!("no task `{id}` to switch to"));
        }
        let needed = self
            .tasks
            .iter()
            .find(|(task, _)| *task == id)
            .map_or(1, |(_, screens)| screens.len().max(1));
        if !self.require_screen_keys(needed) {
            return;
        }
        let kept = self
            .stack
            .iter()
            .map(|active| {
                (
                    active.id.clone(),
                    active.slot.clone(),
                    active.params.clone(),
                )
            })
            .collect();
        let again = self
            .tasks
            .iter()
            .position(|(task, _)| *task == id)
            .map(|at| self.tasks.remove(at).1);
        self.tasks.retain(|(task, _)| *task != current);
        // A few tasks are kept, the one shown longest ago let go first.
        if self.tasks.len() >= TASKS_KEPT {
            self.tasks.remove(0);
        }
        self.tasks.push((current, kept));
        self.close_from(0);
        self.stop_for_closed_screens();
        self.task = (id != slots::HOME).then(|| id.clone());
        match again {
            Some(screens) if !screens.is_empty() => {
                for (screen, slot, params) in screens {
                    self.open(screen, slot, &params);
                }
            }
            _ if self.shared.registry.resolve_slot(id.as_str()).is_some() => {
                self.open_slot_command(id, None, Params::new());
            }
            _ => self.open(id, None, &Params::new()),
        }
        let bounds = self.framebuffer.bounds();
        self.screen_changed(bounds, false);
    }

    /// Tasks kept follow their books. A book deleted (`BOOK_REMOVED`):
    /// a task's screens from the first that shows it are let go (those
    /// above it were opened over it, e.g. its menu), those below stay,
    /// and a task with none left goes; so switching back never opens a
    /// file that is gone. A book moved (`BOOK_MOVED`): its screens open
    /// it where it is now.
    pub(crate) fn tasks_follow_books(&mut self, event: &Id, params: &Params) {
        let path = builtin_params::PATH;
        if let Some(removed) = events::BOOK_REMOVED.read(event, params) {
            let gone = Some(removed.path.as_str());
            for (_, screens) in &mut self.tasks {
                if let Some(at) = screens.iter().position(|(_, _, p)| p.get(path) == gone) {
                    screens.truncate(at);
                }
            }
            self.tasks.retain(|(_, screens)| !screens.is_empty());
        } else if let Some(moved) = events::BOOK_MOVED.read(event, params) {
            let screens = self.tasks.iter_mut().flat_map(|(_, screens)| screens);
            for (_, _, p) in screens.filter(|(_, _, p)| p.get(path) == Some(moved.from.as_str())) {
                p.set(path, moved.path.clone());
            }
        }
    }

    /// Closes all screens above `index`.
    pub(crate) fn close_above(&mut self, index: usize) {
        let mut region: Option<Rect> = None;
        let mut overlays_only = true;
        for i in index + 1..self.stack.len() {
            let r = self.changed_region(i);
            overlays_only &= self.overlay(i).is_some();
            region = Some(region.map_or(r, |u: Rect| u.union(&r)));
        }
        self.close_from(index + 1);
        self.stop_for_closed_screens();
        if let Some(region) = region {
            self.screen_changed(region, overlays_only);
        }
    }

    /// The region of an overlay on the screen, `None` for full screens.
    pub(crate) fn overlay(&self, index: usize) -> Option<Rect> {
        let active = self.stack.get(index)?;
        let base = self.shared.env(self.now);
        let device = base.device_within(active.area);
        let env = base.within(&device, active.area);
        let region = active.screen.region(&env)?;
        Some(chrome::on_screen(region, active.area))
    }

    /// What opening or closing screen `index` changes: its region, or
    /// all of the screen where it washes out what lies below it
    /// ([`Screen::dims_below`](paperpaw_sdk::Screen::dims_below)).
    pub(crate) fn changed_region(&self, index: usize) -> Rect {
        if self.overlay(index).is_some() && self.stack[index].screen.dims_below() {
            self.content()
        } else {
            self.region_of(index)
        }
    }

    /// The area screen `index` occupies.
    pub(crate) fn region_of(&self, index: usize) -> Rect {
        self.overlay(index)
            .unwrap_or_else(|| self.stack.get(index).map_or(self.content(), |s| s.area))
    }

    /// The part of the screen the screen on top has: all of it, or less
    /// the row of hints and the bars shown.
    pub(crate) fn content(&self) -> Rect {
        self.shared.content
    }

    /// Screens are made for, and handled in, `area` now: the display as
    /// they see it is as large.
    pub(crate) fn apply_area(&mut self, area: Rect) {
        set_area(&mut self.shared.device, &mut self.shared.content, area);
    }

    /// Sizes the content for the screen on top, after screens closed; all
    /// of the screen is drawn again if that changed it.
    pub(crate) fn sync_content(&mut self) {
        self.hint.until = None;
        let before = self.shared.content;
        if let Some(area) = self.stack.last().map(|s| s.area) {
            self.apply_area(area);
        }
        if self.shared.content != before {
            self.hint.shown.clear();
            let panel = self.framebuffer.bounds();
            self.invalidate(panel);
        }
    }

    /// Makes every open screen anew for the size it has now, e.g. after
    /// the screen was resized or the row of hints changed; `shown` is
    /// [`Kernel::shown_base`] before the change.
    pub(crate) fn remake_screens(&mut self, shown: usize) {
        // The bars are made anew for the new size too.
        self.close_chromes();
        let mut kept = Vec::with_capacity(self.stack.len());
        for i in 0..self.stack.len() {
            let area = self.area_of(i);
            kept.push(self.remake_screen(i, area));
        }
        self.sync_stack();
        // Made anew, those below the screen shown are hidden again; those
        // kept were hidden already, unless they showed (a list beside its
        // detail, which now covers it).
        let base = self.shown_base();
        for (i, (active, kept)) in self.stack.iter_mut().zip(kept).enumerate().take(base) {
            if !kept || i >= shown {
                let _running = self.shared.meter.enter(active.id.namespace());
                active.screen.on_hide();
            }
        }
        self.hint.shown.clear();
        self.end_notice();
        self.sync_content();
        self.frame.unrendered = None;
        self.frame.rendered = None;
        let bounds = self.framebuffer.bounds();
        self.screen_changed(bounds, false);
    }

    /// Fits screen `index` to `area`: it lays itself out anew where it
    /// can ([`Screen::on_resize`], revision 2, asked through
    /// [`paperpaw_sdk::call_since`] as every newer item is), keeping
    /// all it has. Else it is made anew. The old one goes first: told as
    /// when it closes, so it keeps what it would lose (a reading
    /// position, text typed), and dropped before the new one is made, so
    /// the two never take memory at once. What it asks for then is not
    /// done: the screen stays open, made anew. Whether it was kept.
    pub(crate) fn remake_screen(&mut self, index: usize, area: Rect) -> bool {
        if !self
            .shared
            .registry
            .screens
            .contains_key(&self.stack[index].id)
        {
            return false;
        }
        let asked = self.output.commands.len();
        let before = mem::replace(&mut self.stack[index].area, area);
        let resized = self.with_screen(index, |screen, cx| {
            paperpaw_sdk::call_since(screen, 2, Handled::No, |screen| screen.on_resize(cx))
        });
        if resized == Handled::Yes {
            self.apply_area(area);
            return true;
        }
        self.output.commands.truncate(asked);
        self.stack[index].area = before;
        self.with_screen(index, |screen, cx| {
            screen.on_close(cx);
            Handled::Yes
        });
        self.output.commands.truncate(asked);
        self.apply_area(area);
        let env = self.shared.env(self.now);
        let active = &mut self.stack[index];
        active.area = area;
        let Some(spec) = self.shared.registry.screens.get(&active.id) else {
            return false;
        };
        let _running = self.shared.meter.enter(active.id.namespace());
        // `Gone` has no size: nothing is allocated for it.
        if !spec.is_native() {
            active.screen = Box::new(Gone);
            spec.remake(&mut active.screen, &env, &active.params);
        }
        let made = active.screen.as_mut();
        active.pane = paperpaw_sdk::call_since(made, 2, Pane::FULL, |s| s.pane()).known();
        false
    }

    /// Runs `f` with the screen at `index` and a context for it.
    pub(crate) fn with_screen(
        &mut self,
        index: usize,
        f: impl FnOnce(&mut dyn Screen, &mut Cx<'_, '_>) -> Handled,
    ) -> Handled {
        let earlier = mem::take(&mut self.output.redraw);
        let given = self.output.commands.len();
        // The guard, and the screens it opened above itself (the
        // keyboard to type its password, of the keyboard plugin), open
        // screens above it; another above it (a sleep screen) does not.
        let trusted = self.guard().is_some_and(|guard| {
            index == guard || (index > guard && self.stack.get(index).is_some_and(|s| s.by_guard))
        });
        let Some(active) = self.stack.get_mut(index) else {
            self.output.redraw = earlier;
            return Handled::No;
        };
        let area = active.area;
        let asked_area = self.output.redraw_area.take();
        let base = self.shared.env(self.now);
        let device = base.device_within(area);
        let env = base.within(&device, area);
        let running = self.shared.meter.enter(active.id.namespace());
        let key = paperpaw_sdk::ScreenKey::new(active.serial);
        let mut cx = Cx::new(&env, &mut self.output)
            .for_screen(key)
            .with_params(&self.action_params);
        let handled = f(active.screen.as_mut(), &mut cx);
        drop(running);
        // A part it asked to draw again is in its own coordinates.
        let own = mem::replace(&mut self.output.redraw_area, asked_area);
        if let Some(own) = own.map(|own| chrome::on_screen(own, area)) {
            let all = self.output.redraw_area.map_or(own, |a| a.union(&own));
            self.output.redraw_area = Some(all);
        }
        // A time it asked for is its own: it waits while the screen is
        // covered.
        if let Some(at) = self.output.wake_at.take() {
            active.timer = Some(active.timer.map_or(at, |t| t.min(at)));
        }
        // A key map layer it asks for is its own, laid at once: one asked
        // for any other screen is not (`Command::SetKeyLayer` from
        // elsewhere is refused).
        let reserved = crate::commands::Reserved::of(&self.shared.settings, &self.shared.registry);
        let owner = self.stack[index].id.namespace();
        let out = &mut self.output.commands;
        if crate::commands::vet_reserved(out, given, owner, &reserved) {
            self.warn_runtime(format!(
                "{owner} asked what only the settings editor may; not done"
            ));
        }
        let mut at = given;
        while at < self.output.commands.len() {
            let Command::SetKeyLayer(asked, layer) = &self.output.commands[at] else {
                at += 1;
                continue;
            };
            let (asked, layer) = (*asked, layer.clone());
            self.output.commands.remove(at);
            if asked == key {
                self.set_key_layer(key, layer);
            }
        }
        // The firmware confirmation's answer comes from that screen alone:
        // taken here, out of its own commands, so no event another sends
        // (with the nonce guessed or not) is ever taken for it.
        if self.stack[index].id == crate::native_host::firmware_confirm::SCREEN {
            let mut at = given;
            while at < self.output.commands.len() {
                let Command::Emit(event, params) = &self.output.commands[at] else {
                    at += 1;
                    continue;
                };
                if *event != crate::native_host::firmware_confirm::ANSWERED {
                    at += 1;
                    continue;
                }
                let params = params.clone();
                self.output.commands.remove(at);
                self.firmware_answer(&params);
            }
        }
        // The commands of a guarding screen, or of one above it, open
        // screens above it.
        if trusted {
            self.trusted.extend(given..self.output.commands.len());
            self.own_trusted.extend(given..self.output.commands.len());
        }
        // `request_redraw` means the caller's own region. A screen below an
        // overlay is still partly visible (e.g. the page behind a menu that
        // changes a reading setting); one below a full screen is redrawn
        // when it is shown again.
        let requested = mem::replace(&mut self.output.redraw, earlier);
        if requested {
            let top = self.stack.len() - 1;
            // The detail beside a list covers none of it.
            let detail = self.panes().map(|(_, detail)| detail);
            if index == top {
                self.output.redraw = true;
            } else if (index + 1..=top).all(|i| detail == Some(i) || self.overlay(i).is_some()) {
                // Less what the overlays above it cover: they paint all
                // of their region (as `redraw` draws them), so only the
                // rest of it changes on the screen. Not where one shows
                // the full screen through, which leaves those below it
                // undrawn.
                let through: Vec<ShowsThrough> = (index + 1..=top)
                    .map(|i| self.stack[i].screen.shows_through())
                    .collect();
                let region = self.region_of(index);
                let shown = if through.contains(&ShowsThrough::FullScreen) {
                    Some(region)
                } else {
                    (index + 1..=top)
                        .zip(through)
                        .filter(|&(_, t)| t == ShowsThrough::Nothing)
                        .filter_map(|(i, _)| self.overlay(i))
                        .try_fold(region, uncovered)
                };
                if let Some(shown) = shown {
                    self.invalidate(shown);
                }
            }
        }
        handled
    }

    /// The full screen whose time comes below overlays alone: the topmost
    /// full screen, where every screen above it is an overlay and it asks
    /// for it ([`Screen::times_under_overlays`], revision 3). `None` where
    /// a full screen is on top.
    fn timed_under_overlays(&mut self) -> Option<usize> {
        let top = self.stack.len().checked_sub(1)?;
        let full = (0..top).rev().find(|&i| self.overlay(i).is_none())?;
        if (full + 1..=top).any(|i| self.overlay(i).is_none()) {
            return None;
        }
        let screen = self.stack[full].screen.as_mut();
        paperpaw_sdk::call_since(screen, TIMES_UNDER_OVERLAYS_SINCE, false, |s| {
            s.times_under_overlays()
        })
        .then_some(full)
    }

    /// The earliest time a screen asked for that comes: the top one's,
    /// and that of the one below overlays alone that runs.
    pub(crate) fn screen_timer(&mut self) -> Option<Instant> {
        let top = self.stack.last().and_then(|top| top.timer);
        let below = self
            .timed_under_overlays()
            .and_then(|index| self.stack[index].timer);
        top.into_iter().chain(below).min()
    }

    /// Calls [`Screen::on_timer`] of the screens whose time has come: the
    /// one on top, then the one below overlays alone that asks for it
    /// (found anew, as the top one may have closed).
    pub(crate) fn screen_timers_due(&mut self) {
        let now = self.now;
        if let Some(top) = self.stack.last_mut()
            && top.timer.is_some_and(|at| at <= now)
        {
            top.timer = None;
            self.with_top(|screen, cx| {
                screen.on_timer(cx);
                Handled::Yes
            });
        }
        if let Some(index) = self.timed_under_overlays()
            && self.stack[index].timer.is_some_and(|at| at <= now)
        {
            self.stack[index].timer = None;
            self.with_screen(index, |screen, cx| {
                screen.on_timer(cx);
                Handled::Yes
            });
        }
    }

    /// Runs `f` with the active screen and a context for it.
    pub(crate) fn with_top(
        &mut self,
        f: impl FnOnce(&mut dyn Screen, &mut Cx<'_, '_>) -> Handled,
    ) -> Handled {
        match self.stack.len().checked_sub(1) {
            Some(top) => self.with_screen(top, f),
            None => Handled::No,
        }
    }

    /// After the screen stack changed: redraw `region`, flashing for full
    /// screens and refreshing quickly when only overlays changed.
    pub(crate) fn screen_changed(&mut self, region: Rect, overlays_only: bool) {
        self.input.captured.clear();
        self.invalidate(region);
        let flash = self
            .shared
            .settings
            .bool(core_settings::FLASH_SCREENS.as_str());
        self.output.request_refresh(if overlays_only || !flash {
            RefreshMode::Partial
        } else {
            RefreshMode::Full
        });
        self.show_pending = true;
    }

    /// Stops what closed screens started: servers, sockets, speech.
    pub(crate) fn stop_for_closed_screens(&mut self) {
        let screens = self.stack.len();
        let out = &mut self.output.commands;
        self.io.servers.stop_for_closed_screens(screens, out);
        self.io.sockets.close_for_closed_screens(screens, out);
        self.bluetooth_screens_closed();
        self.io.speech.stop_for_closed_screens(screens);
        if self
            .io
            .speech
            .voice
            .as_ref()
            .is_some_and(|(_, s)| *s > screens)
        {
            self.stop_speaking();
        }
    }
}

/// Where the notice `screen` shows on the screen: in the part of the
/// screen on top (`env`'s), from its corner.
pub(crate) fn notice_area(screen: &dyn Screen, env: &Env<'_>) -> Rect {
    let content = env.content();
    let all = Rect::new(0, 0, content.width, content.height);
    chrome::on_screen(screen.region(env).unwrap_or(all), content)
}

/// Screens are made for, and handled in, `area` now (see
/// `Kernel::apply_area`).
fn set_area(device: &mut DeviceInfo, content: &mut Rect, area: Rect) {
    device.display.width = area.width;
    device.display.height = area.height;
    *content = area;
}

/// `source` with `input` in `section` bound to `action`, or to what the
/// plugins bind it to (`None`).
fn bound(source: &str, section: &str, input: &str, action: Option<&Id>) -> String {
    match action {
        Some(action) => paperpaw_config::set(source, section, input, action.as_str()),
        None => paperpaw_config::remove(source, section, input),
    }
}

/// The part of `area` that `cover` leaves to be seen, as a rectangle:
/// `None` if it covers all of it; less the rows it covers at the top or
/// the bottom where it is as wide; else all of `area`.
pub(crate) fn uncovered(area: Rect, cover: Rect) -> Option<Rect> {
    let Some(common) = area.intersection(&cover) else {
        return Some(area);
    };
    if common == area {
        return None;
    }
    if common.x != area.x || common.width != area.width {
        return Some(area);
    }
    if common.y == area.y {
        let top = common.bottom() as u16;
        return Some(Rect::new(
            area.x,
            top,
            area.width,
            (area.bottom() - common.bottom()) as u16,
        ));
    }
    if common.bottom() == area.bottom() {
        return Some(Rect::new(area.x, area.y, area.width, common.y - area.y));
    }
    Some(area)
}

/// What a screen's place holds between the old screen dropped and the new
/// one made ([`Kernel::remake_screen`]): never drawn nor asked anything.
pub(crate) struct Gone;

impl Screen for Gone {
    fn draw(&mut self, _: &mut paperpaw_graphics::Canvas<'_>, _: &Env<'_>) {}

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }
}
