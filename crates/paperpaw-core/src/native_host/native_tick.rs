//! `event.tick` for native plugins (`docs/abi/records.md`): each plugin that
//! registered for it gets one tick a minute while the device is awake.
//!
//! The host does not filter ticks by widget or screen, never asks for an extra
//! wake to send one (a tick is made where [`Kernel::tick`](crate::Kernel::tick)
//! runs anyway) and a plugin cannot unsubscribe in interface 7. A device that
//! slept past several minutes gets one tick when it wakes, not one for each.

use paperpaw_sdk::Event;

/// The kernel's name for the tick: native plugins registered for it are
/// subscribed to it as to any core event.
pub(crate) const TICK: Event<()> = Event::new("core.tick");

/// The time between two ticks.
const MINUTE_MS: u64 = 60_000;

impl crate::Kernel {
    /// Sends `event.tick` where a minute has passed since the last one. The
    /// first call only starts the minute.
    pub(crate) fn native_tick(&mut self) {
        let now = self.now;
        let due = match self.native_tick_at {
            None => {
                self.native_tick_at = Some(now);
                false
            }
            Some(at) => now.since(at) >= MINUTE_MS,
        };
        if due {
            self.native_tick_at = Some(now);
            self.output.commands.push(TICK.emit(()));
        }
    }
}
