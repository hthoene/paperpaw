//! Failed native event callbacks, made visible.
//!
//! A plugin's own event callback that fails (a negative status, `Started`
//! included) used to be seen by no one: the callers of
//! [`Kernel::native_event_call`](crate::Kernel) discard the status. The call
//! notes it here, in a table of a fixed size (nothing is allocated where the
//! callback fails), and the kernel turns each note into one warning when the
//! shell asks for its warnings ([`Kernel::take_warnings`]): the plugin, the
//! event and the status. One note for each plugin and event until the kernel
//! starts again, and room for six: a seventh is only counted. What happens to
//! the plugin is not changed.
//!
//! Limits, on purpose: the table is not cleared, so a pair already noted (or a
//! seventh) that fails again is not said again, nor counted; and the host's own
//! refusals `ERR_BUSY` and `ERR_STATE` are left out, so a plugin that itself ends
//! with one of them (a refused service call it passes up) is not said either.
//! The events that go through the screen queue (a screen's callbacks) do not
//! pass `native_event_call` and are not noted here. The table takes about 150 B
//! of the native instances' state.
use core::cell::RefCell;
use paperpaw_native::host_glue::{ERR_BUSY, ERR_STATE};

/// Plugin and event pairs kept; more are counted.
const SLOTS: usize = 6;

#[derive(Clone, Copy)]
struct Failure {
    key: u64,
    tag: u16,
    status: i32,
    reported: bool,
}

#[derive(Default)]
struct Table {
    list: [Option<Failure>; SLOTS],
    /// Pairs that found no room, not yet said.
    more: u32,
}

/// The notes of failed callbacks, kept by the native instances.
#[derive(Default)]
pub(crate) struct EventFailures(RefCell<Table>);

/// One failure to say: the owner's key, the event tag and the status.
pub(crate) struct Said {
    pub(crate) key: u64,
    pub(crate) tag: u16,
    pub(crate) status: i32,
}

impl EventFailures {
    /// Notes the failure of `owner`'s callback for the event `tag` (zero when
    /// the record could not be read). `ERR_BUSY` (nothing was called) and
    /// `ERR_STATE` (an owner that is gone) are the host's, not the plugin's.
    pub(crate) fn note(&self, owner: u64, tag: u32, status: i32) {
        if status >= 0 || status == ERR_BUSY || status == ERR_STATE {
            return;
        }
        let Ok(mut table) = self.0.try_borrow_mut() else {
            return;
        };
        let key = owner;
        let tag = u16::try_from(tag).unwrap_or(0);
        if table
            .list
            .iter()
            .flatten()
            .any(|failure| failure.key == key && failure.tag == tag)
        {
            return;
        }
        match table.list.iter_mut().find(|slot| slot.is_none()) {
            Some(slot) => {
                *slot = Some(Failure {
                    key,
                    tag,
                    status,
                    reported: false,
                });
            }
            None => table.more = table.more.saturating_add(1),
        }
    }

    /// The next failure not said yet, now said.
    pub(crate) fn take(&self) -> Option<Said> {
        let mut table = self.0.try_borrow_mut().ok()?;
        let failure = table
            .list
            .iter_mut()
            .flatten()
            .find(|failure| !failure.reported)?;
        failure.reported = true;
        Some(Said {
            key: failure.key,
            tag: failure.tag,
            status: failure.status,
        })
    }

    /// How many pairs found no room since the last call.
    pub(crate) fn take_more(&self) -> u32 {
        self.0
            .try_borrow_mut()
            .map_or(0, |mut table| core::mem::take(&mut table.more))
    }
}

/// The event's name for the lifecycle events a failure is most often of; the
/// number says the rest.
pub(crate) fn label(tag: u16) -> &'static str {
    use crate::native_host::registrations::{
        EVENT_RESUME_TAG, EVENT_STARTED_TAG, EVENT_STOPPING_TAG, EVENT_SUSPEND_TAG, EVENT_TICK_TAG,
    };
    match u32::from(tag) {
        EVENT_STARTED_TAG => "started",
        EVENT_STOPPING_TAG => "stopping",
        EVENT_RESUME_TAG => "resume",
        EVENT_SUSPEND_TAG => "suspend",
        EVENT_TICK_TAG => "tick",
        _ => "tag",
    }
}

/// `value` in decimal in `buf`, without allocating.
pub(crate) fn decimal(value: i64, buf: &mut [u8; 20]) -> &str {
    let mut at = buf.len();
    let mut rest = value.unsigned_abs();
    loop {
        at -= 1;
        buf[at] = b'0' + (rest % 10) as u8;
        rest /= 10;
        if rest == 0 {
            break;
        }
    }
    if value < 0 {
        at -= 1;
        buf[at] = b'-';
    }
    core::str::from_utf8(&buf[at..]).unwrap_or("?")
}

impl crate::Kernel {
    /// Says each failed callback noted since the last call, as one warning
    /// (none where the heap has no room for it; it is said at the next call).
    pub(crate) fn report_event_failures(&mut self) {
        loop {
            // A warning that cannot be made leaves the note unsaid.
            if self.warnings.try_reserve(1).is_err() {
                return;
            }
            let Some(said) = self.shared.native_instances.event_failures.take() else {
                break;
            };
            let id = self
                .shared
                .native_instances
                .id(said.key)
                .map(|id| alloc::string::String::from(id.as_str()))
                .unwrap_or_default();
            let (mut tag, mut status) = ([0; 20], [0; 20]);
            let tag = decimal(i64::from(said.tag), &mut tag);
            let status = decimal(i64::from(said.status), &mut status);
            let who = if id.is_empty() {
                "a native plugin"
            } else {
                id.as_str()
            };
            self.try_warn_runtime(&[
                who,
                ": its callback for event ",
                label(said.tag),
                " ",
                tag,
                " ended with status ",
                status,
            ]);
        }
        let more = self.shared.native_instances.event_failures.take_more();
        if more > 0 {
            let mut count = [0; 20];
            let count = decimal(i64::from(more), &mut count);
            self.try_warn_runtime(&[
                "more native event callbacks failed (",
                count,
                " not named here)",
            ]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn six_pairs_are_kept_and_the_rest_counted_once() {
        let failures = EventFailures::default();
        for tag in 1..=9 {
            failures.note(1, tag, -3);
        }
        // The same pair again changes nothing.
        failures.note(1, 1, -3);
        let mut said = 0;
        while failures.take().is_some() {
            said += 1;
        }
        assert_eq!(said, SLOTS);
        assert_eq!(failures.take_more(), 3);
        assert_eq!(failures.take_more(), 0);
    }

    #[test]
    fn successes_and_the_hosts_statuses_are_not_noted() {
        let failures = EventFailures::default();
        failures.note(1, 1, 0);
        failures.note(1, 2, 1);
        failures.note(1, 3, ERR_BUSY);
        failures.note(1, 4, ERR_STATE);
        assert!(failures.take().is_none());
        assert_eq!(failures.take_more(), 0);
    }

    #[test]
    fn numbers_are_written_without_allocating() {
        let mut buf = [0; 20];
        assert_eq!(decimal(0, &mut buf), "0");
        assert_eq!(decimal(-6, &mut buf), "-6");
        assert_eq!(decimal(65_535, &mut buf), "65535");
        assert_eq!(decimal(i64::MIN, &mut buf), "-9223372036854775808");
    }
}
