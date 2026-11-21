//! `event.bluetooth` (98, `docs/abi/device.md` 4) to the plugin bound to
//! `core.bluetooth`: what the kernel's radio tells of the native plugins'
//! Bluetooth commands (the events they are given, [`EVENTS`]) and of its
//! radio and bonds (`BLUETOOTH_CHANGED`, kind 1).
//!
//! * **Addressed, no subscription:** 98 goes to the manager whether or not
//!   it subscribed, as `event.answered` goes to its asker.
//! * **Queued in one order:** held in the manager's queue of the setting
//!   events (`setting_events`), whose pump tells it between callbacks in the
//!   order of everything else the plugin hears. Only a running manager hears
//!   it; one that does not fit is dropped and logged as any queued edge.
//! * **What is told:** found devices, the scan's end, links made and lost,
//!   pairings, done and failed commands. What carries a payload (values,
//!   reports, services) is not an event of interface 7 and is not told.
use super::{
    device_service::kind_code,
    native_instances::InstanceState,
    registrations::{self as wire, EVENT_BLUETOOTH_NAME_MAX_BYTES, EVENT_BLUETOOTH_TAG},
};
use crate::Kernel;
use paperpaw_sdk::{
    Id, Params,
    bluetooth::{BluetoothView, BtError, BtEvent},
    builtin::{events, slots},
    event::Event,
    secrets::Anyone,
};

/// The events the kernel's radio answers native plugins' commands with.
pub(crate) const EVENTS: Event<BtEvent> = Event::new("core.bluetooth-native");

/// `kind` of 98 (`device.md` 4).
const CHANGED: u32 = 1;
const FOUND: u32 = 2;
const SCAN_ENDED: u32 = 3;
const CONNECTED: u32 = 4;
const DISCONNECTED: u32 = 5;
const PAIRED: u32 = 6;
const DONE: u32 = 7;
const FAILED: u32 = 8;

/// `reason` of 98.
const fn reason(error: BtError) -> u32 {
    match error {
        BtError::Off => 1,
        BtError::NotAllowed => 2,
        BtError::NotFound => 3,
        BtError::Busy => 4,
        BtError::Timeout => 5,
        BtError::Declined => 6,
        BtError::Lost => 7,
        BtError::Att(_) => 8,
        BtError::Held(_) => 9,
        BtError::Unsupported => 10,
        _ => 0,
    }
}

/// `name` cut to the record's cap at a character boundary.
fn capped(name: &str) -> &str {
    let mut end = name.len().min(EVENT_BLUETOOTH_NAME_MAX_BYTES);
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    name.get(..end).unwrap_or_default()
}

/// A record of `kind` about nothing else.
const fn bare(now_ms: u64, kind: u32, device: u32, reason: u32) -> wire::EventBluetooth<'static> {
    wire::EventBluetooth {
        now_ms,
        kind,
        device,
        name: "",
        device_kind: 0,
        signal: 0,
        reason,
    }
}

impl Kernel {
    /// Where `event` is one the Bluetooth manager hears as 98, it is queued
    /// for it. `true` where nobody else hears it (the native plugins' own
    /// [`EVENTS`]).
    pub(crate) fn native_bluetooth_event(&mut self, event: &Id, params: &Params) -> bool {
        let own = *event == *EVENTS.id();
        if !own && *event != events::BLUETOOTH_CHANGED {
            return false;
        }
        let now_ms = self.shared.now.millis();
        let told = if own {
            EVENTS
                .read(event, params)
                .and_then(|change| self.queue_bluetooth(&change, now_ms))
        } else {
            self.queue_bluetooth_record(&bare(now_ms, CHANGED, 0, 0))
        };
        if told == Some(false) {
            self.try_warn_runtime(&["a native plugin lost event.bluetooth; not retried"]);
        }
        own
    }

    /// Queues `change` for the manager: `None` where it is not told (no
    /// manager runs, or the event is none of 98's), `Some(false)` where it
    /// is dropped and that is to be logged (the first drop since its queue
    /// was last empty, or no queue could be made).
    fn queue_bluetooth(&self, change: &BtEvent, now_ms: u64) -> Option<bool> {
        let number = change.device().map_or(0, |device| device.number());
        let record = match change {
            BtEvent::Found(device) => wire::EventBluetooth {
                name: capped(device.name.as_deref().unwrap_or_default()),
                device_kind: kind_code(device.kind),
                signal: i32::from(device.signal.unwrap_or(0)),
                ..bare(now_ms, FOUND, number, 0)
            },
            BtEvent::ScanEnded => bare(now_ms, SCAN_ENDED, 0, 0),
            BtEvent::Connected { .. } => bare(now_ms, CONNECTED, number, 0),
            BtEvent::Disconnected { reason: why, .. } => {
                bare(now_ms, DISCONNECTED, number, why.map_or(0, reason))
            }
            BtEvent::Paired { device, .. } => {
                let all = self.shared.bluetooth.as_ref();
                let view = all.map(|all| BluetoothView::new(all, &Anyone));
                let bond = view.as_ref().and_then(|view| view.bond(*device));
                wire::EventBluetooth {
                    name: capped(bond.map_or("", |bond| bond.name.as_str())),
                    device_kind: bond.map_or(0, |bond| kind_code(bond.kind)),
                    ..bare(now_ms, PAIRED, number, 0)
                }
            }
            BtEvent::Done { .. } => bare(now_ms, DONE, number, 0),
            BtEvent::Failed(failure) => bare(now_ms, FAILED, number, reason(failure.error)),
            _ => return None,
        };
        self.queue_bluetooth_record(&record)
    }

    /// Holds `record` in the queue of the running plugin bound to
    /// `core.bluetooth`; as [`Self::queue_bluetooth`].
    fn queue_bluetooth_record(&self, record: &wire::EventBluetooth<'_>) -> Option<bool> {
        let manager = self
            .shared
            .registry
            .resolve_slot(slots::BLUETOOTH.as_str())?
            .namespace();
        let instances = &self.shared.native_instances;
        let owner = instances.find(manager)?;
        if !matches!(instances.state(owner), Ok(InstanceState::Running(_))) {
            return None;
        }
        let record = wire::Record::EventBluetooth(*record);
        let held = instances.setting_events.with_queue(owner, |queue| {
            queue
                .push_record(EVENT_BLUETOOTH_TAG, &record)
                .map_err(|_| queue.note_drop())
        });
        // A queue that cannot even be made is reported every time.
        Some(!matches!(held, Err(_) | Ok(Err(true))))
    }
}

/// Whether event `tag` goes to its plugin without a subscription: 98, which
/// the host addresses to the Bluetooth manager.
pub(super) const fn addressed(tag: u32) -> bool {
    tag == EVENT_BLUETOOTH_TAG
}
