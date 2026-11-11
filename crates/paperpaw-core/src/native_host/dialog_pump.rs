//! Questions end to end (`docs/abi/ui.md` 3.1 and 3.2): the pump opens the
//! dialog of every request whose asking callback returned (`starting` to
//! `open`) and hands each answer to its asker as `event.answered` (82).
//!
//! It runs between callbacks, from the service pump (never inside one), so
//! every request still `asked` has had its callback return: it starts, or was
//! cancelled with `not_started` by the callback's failure
//! ([`UiStore::callback_failed`](super::ui_store::UiStore)) or an initial text
//! left begun. The dialog opens in the slot of its kind as the queued `open`
//! the asker could not have made itself, with the params `core.request` and
//! `core.kind` (ASCII decimal, no sign, no leading zero). When the stack
//! cannot open it (full, guarded, no plugin bound to the slot) the asker gets
//! the cancel with `not_started`. An answer is encoded from the request when
//! it is delivered, one callback each and in order with the asker's other
//! events; the request is freed when that callback returned, whatever its
//! status (`ui.md` 3.2, cleanup).
use super::registrations::{self as wire};
use crate::native_host::codec;
use paperpaw_native::host_glue::ERR_BUSY;
use paperpaw_sdk::{
    Command, Id, Params,
    builtin::slots::{CHOICE, CHOOSE_FOLDER, CONFIRM, KEYBOARD, REMOTE_KEYBOARD},
};

/// Answers one pass hands to one plugin, so that none starves another.
const PER_OWNER: usize = 4;

/// The slot whose screen draws a question of `kind`; `None` for a kind that
/// has none (the store admits kinds 0 to 3 only).
pub(super) fn slot_of(kind: u32, remote: bool) -> Option<Id> {
    match (kind, remote) {
        (0, false) => Some(KEYBOARD),
        (0, true) => Some(REMOTE_KEYBOARD),
        (1, _) => Some(CHOICE),
        (2, _) => Some(CONFIRM),
        (3, _) => Some(CHOOSE_FOLDER),
        _ => None,
    }
}

/// `value` in ASCII decimal, the form `core.request` and `core.kind` have:
/// made without a failing allocation (`None` when the heap cannot).
fn decimal(value: u32) -> Option<alloc::string::String> {
    let mut digits = [0u8; 10];
    let mut at = digits.len();
    let mut rest = value;
    loop {
        at -= 1;
        digits[at] = b'0' + (rest % 10) as u8;
        rest /= 10;
        if rest == 0 {
            break;
        }
    }
    let mut text = alloc::string::String::new();
    text.try_reserve_exact(digits.len() - at).ok()?;
    for digit in &digits[at..] {
        text.push(char::from(*digit));
    }
    Some(text)
}

impl crate::Kernel {
    /// One pass of the dialog pump; `true` when it wants another at once
    /// (answers remain, or a batch waits).
    // Its own frame: the pump keeps the services' passes apart, so that no
    // frame adds their locals (the firmware's frame limit for host code).
    #[inline(never)]
    pub(crate) fn native_dialog_pump(&mut self) -> bool {
        if self.shared.native_instances.depth() != 0 {
            return true;
        }
        let Some(store) = self
            .shared
            .native_instances
            .ui_services()
            .and_then(|ui| ui.store.try_clone())
        else {
            return false;
        };
        // Answers a host dialog could not give are told of in the log.
        for _ in 0..store.take_refused() {
            self.warn_runtime("a host dialog's answer was not taken; the question was cancelled");
        }
        // The kernel's clipboard is the store's (once); a text op 18 committed
        // goes out to the system (`Io::Copy`); a text left begun by a callback
        // that returned is dropped.
        store.bind_clipboard(&self.shared.clipboard);
        self.clipboard_copy_out();
        store.clipboard_callbacks_over();
        if store.callbacks_over().is_err() {
            return true;
        }
        let mut more = false;
        while let Some(start) = store.next_starting() {
            self.dialog_open(&store, &start);
        }
        // Answers: one callback each; what a callback staged runs before the
        // next is called.
        let mut index = 0;
        while let Some(owner) = self
            .shared
            .native_instances
            .running_nth(index)
            .map(|(owner, _)| owner)
        {
            index += 1;
            let Ok(super::native_instances::InstanceState::Running(lease)) =
                self.shared.native_instances.state(owner)
            else {
                continue;
            };
            for _ in 0..PER_OWNER {
                if self.shared.native_instances.outbox.full() {
                    self.native_outbox_run();
                }
                let Some(delivery) = store.take_delivery(owner) else {
                    break;
                };
                let record = wire::EventAnswered {
                    now_ms: self.now.millis(),
                    request: delivery.request,
                    kind: delivery.kind,
                    outcome: delivery.outcome,
                    choice: delivery.choice,
                    length: delivery.length,
                    flags: delivery.flags,
                };
                let mut bytes = [0u8; 64];
                let Ok(len) =
                    codec::encode_batch(&[wire::Record::EventAnswered(record)], &mut bytes)
                else {
                    // Never: the record is fixed size. Freed all the same.
                    let _ = store.delivered(delivery.request);
                    continue;
                };
                let called = self.native_event_call(owner, &lease, &bytes[..len]);
                if called == Err(ERR_BUSY) {
                    // A batch waits: no callback ran, so the answer is kept for
                    // the next pass, not freed.
                    let _ = store.undo_delivery(&delivery);
                    more = true;
                    break;
                }
                // Freed when the callback returned, whatever it said.
                let _ = store.delivered(delivery.request);
                self.native_outbox_run();
            }
            more |= store.has_delivery(owner);
        }
        more
    }

    /// Asks a text question as a plugin's `ui.ask` does, for the tools and
    /// tests that open the keyboard without a plugin that asks: the request
    /// is the keyboard plugin's own (its answer goes back to it, which takes
    /// no notice), and the dialog opens in the keyboard slot with the params
    /// `core.request` and `core.kind`, from the pump below. `secret` asks for
    /// a secret text. Nothing of the stack's admission (the guard's rules)
    /// is applied.
    ///
    /// # Errors
    /// Why not: no native keyboard serves the slot, no `ui` service is
    /// wired, or the question was not taken (memory, the limits).
    pub fn stage_text_question(
        &mut self,
        now: paperpaw_platform::Instant,
        title: &str,
        secret: bool,
    ) -> Result<crate::Effects, &'static str> {
        let _running = self.shared.meter.enter(crate::meter::CORE);
        let owner = self
            .shared
            .registry
            .resolve_slot(KEYBOARD.as_str())
            .map(Id::namespace)
            .and_then(|plugin| self.shared.native_instances.key_of(plugin))
            .ok_or("no native keyboard serves the keyboard slot")?;
        let store = self
            .shared
            .native_instances
            .ui_services()
            .and_then(|ui| ui.store.try_clone())
            .ok_or("no ui service is wired")?;
        let who = super::ui_store::Stamp::new(owner, 1).map_err(|_| "no stamp")?;
        let question = super::ui_requests::Question {
            kind: 0,
            flags: if secret { 1 << 2 } else { 0 },
            remote: false,
            title,
            text: "",
            choices: "",
        };
        store
            .ask(who, None, &question, secret, None, true)
            .map_err(|_| "the question was not taken")?;
        self.shared.reader = true;
        self.now = now;
        self.last_activity = now;
        // The pump of the finish opens the dialog.
        Ok(self.finish())
    }

    /// A text op 18 committed goes out to the system (`Io::Copy`); with no
    /// memory for the copy it is skipped and the log says so.
    fn clipboard_copy_out(&mut self) {
        if !self.shared.clipboard.take_copy() {
            return;
        }
        match self.shared.clipboard.text_for_copy() {
            Some(text) => self.io.clipboard.copy(text),
            None => self.warn_runtime("the clipboard could not be copied out: no memory"),
        }
    }

    /// Opens the dialog of one starting request and tells the store how it
    /// went: up (`open`) or `not_started`. The guard's own question opens above
    /// the guard and is marked as its dialog; any other plugin's under a guard
    /// opens below it and waits there (`ui.md` 4.3), as every screen it opens.
    fn dialog_open(&mut self, store: &super::ui_store::UiStore, start: &super::ui_requests::Start) {
        let (Some(slot), Some(request), Some(kind)) = (
            slot_of(start.kind, start.remote),
            decimal(start.request),
            decimal(start.kind),
        ) else {
            let _ = store.dialog_failed(start.request);
            return;
        };
        // The host's own choice and confirm dialogs answer the store directly
        // (`dialogs`): the slot resolves to their screen, and the dialog is made
        // from the question as the store holds it. Any other screen reads the
        // question from the store with op 2. Kinds 0 and 3 have no host
        // dialog: with no dialog plugin they end as cancelled.
        let host = matches!(start.kind, 1 | 2)
            && self
                .shared
                .registry
                .resolve_slot(slot.as_str())
                .is_some_and(|id| {
                    *id == paperpaw_sdk::builtin::screens::CHOICE
                        || *id == paperpaw_sdk::builtin::screens::CONFIRM
                });
        // Made before the open, every copy fallible: with no memory for the
        // dialog nothing opens, and the request ends as not started (an SDK
        // screen left on the slot would answer nothing).
        let (asked, shared) = if host {
            let asked = store
                .starting_question(start.request, |shown| {
                    super::dialogs::Asked::copy(
                        shown.kind,
                        shown.flags,
                        shown.title,
                        shown.text,
                        shown.choices,
                    )
                })
                .ok()
                .flatten();
            let shared = self
                .shared
                .native_instances
                .ui_services()
                .and_then(|ui| ui.store.try_clone());
            let (Some(asked), Some(shared)) = (asked, shared) else {
                let _ = store.dialog_failed(start.request);
                return;
            };
            (Some(asked), Some(shared))
        } else {
            (None, None)
        };
        let params = Params::new()
            .with("core.request", request)
            .with("core.kind", kind);
        let own = self.guard().is_some_and(|guard| {
            self.shared
                .native_instances
                .id(start.asker)
                .is_some_and(|asker| asker.as_str() == self.stack[guard].id.namespace())
        });
        // Serials only grow: what is at or past this was opened just now.
        let first = self.next_serial;
        self.from_guard = own;
        self.from_guard_own = own;
        self.apply(Command::OpenSlot(slot.clone(), params));
        self.from_guard = false;
        self.from_guard_own = false;
        // Looked up at once, before the commands the open led to run: one of
        // them may open the same slot again (another plugin's, which waits
        // below a guard), and the question must not be bound to that instance.
        let opened = self
            .stack
            .iter_mut()
            .find(|top| top.serial >= first && top.slot.as_ref() == Some(&slot))
            .map(|dialog| {
                dialog.guard_dialog = own;
                if let (Some(asked), Some(shared)) = (asked, shared) {
                    // At once too: the dialog is the host's from its first draw.
                    let answer =
                        super::dialogs::Answerer::new(shared, dialog.serial, start.request);
                    let env = self.shared.env(self.now);
                    dialog.screen = asked.into_screen(&env, answer);
                }
                dialog.serial
            });
        match opened {
            Some(serial) => {
                // Told before the follow-ups run: one that closes the dialog
                // cancels the request as any close does.
                let _ = store.dialog_opened(start.request, serial);
            }
            None => {
                let _ = store.dialog_failed(start.request);
            }
        }
        self.run_commands();
    }
}

#[cfg(test)]
#[path = "dialog_pump_tests.rs"]
mod tests;
