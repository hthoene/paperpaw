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

/// `value` in ASCII decimal, the form `core.request` and `core.kind` have.
fn decimal(value: u32) -> alloc::string::String {
    alloc::format!("{value}")
}

impl crate::Kernel {
    /// One pass of the dialog pump; `true` when it wants another at once
    /// (answers remain, or a batch waits).
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
                let Ok(len) = wire::records_encode(&[wire::Record::EventAnswered(record)], &mut bytes)
                else {
                    // Never: the record is fixed size. Freed all the same.
                    let _ = store.delivered(delivery.request);
                    continue;
                };
                let called = self.native_event_call(owner, &lease, &bytes[..len]);
                // Freed when the callback returned, whatever it said.
                let _ = store.delivered(delivery.request);
                if called == Err(ERR_BUSY) {
                    more = true;
                }
                self.native_outbox_run();
            }
            more |= store.has_delivery(owner);
        }
        more
    }

    /// Opens the dialog of one starting request and tells the store how it
    /// went: up (`open`) or `not_started`.
    fn dialog_open(&mut self, store: &super::ui_store::UiStore, start: &super::ui_requests::Start) {
        let Some(slot) = slot_of(start.kind, start.remote) else {
            let _ = store.dialog_failed(start.request);
            return;
        };
        let before = self.stack.last().map(|top| top.serial);
        let params = Params::new()
            .with("core.request", decimal(start.request))
            .with("core.kind", decimal(start.kind));
        self.output.commands.push(Command::OpenSlot(slot.clone(), params));
        self.run_commands();
        let opened = self
            .stack
            .last()
            .filter(|top| Some(top.serial) != before && top.slot.as_ref() == Some(&slot))
            .map(|top| top.serial);
        match opened {
            Some(serial) => {
                let _ = store.dialog_opened(start.request, serial);
            }
            None => {
                let _ = store.dialog_failed(start.request);
            }
        }
    }
}

#[cfg(test)]
#[path = "dialog_pump_tests.rs"]
mod tests;
