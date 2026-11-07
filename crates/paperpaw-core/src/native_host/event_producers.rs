//! Typed EVT producer adapters. Admission stays disabled until Core wires delivery.
use super::registrations as wire;
use crate::abi7::settings_wire::{SettingValueRef, settings_encode};
use core::mem::MaybeUninit;
use paperpaw_native::host_glue::{ERR_NO_SPACE, ERR_UNSUPPORTED};
use paperpaw_platform::SystemStatus;
use paperpaw_sdk::{
    SettingValue,
    power::{Power, PowerLevel},
    wifi::WifiState,
};

/// Captured before a settings mutation, without cloning an owned setting string.
/// An unrepresentable value must drop the edge and set the reread bit, not become empty.
pub(crate) struct OldValue {
    bytes: [u8; wire::EVENT_SETTING_CHANGED_OLD_MAX_BYTES],
    len: u16,
    status: Result<(), i32>,
}
impl OldValue {
    pub(crate) fn capture(value: Option<&SettingValue>) -> Self {
        let mut captured = Self {
            bytes: [0; wire::EVENT_SETTING_CHANGED_OLD_MAX_BYTES],
            len: 0,
            status: Ok(()),
        };
        let Some(value) = value else {
            return captured;
        };
        let value = match value {
            SettingValue::Bool(value) => SettingValueRef::Bool(*value),
            SettingValue::Int(value) => SettingValueRef::Int(*value),
            SettingValue::Text(value) => SettingValueRef::Text(value),
            _ => {
                captured.status = Err(ERR_UNSUPPORTED);
                return captured;
            }
        };
        let mut encoded = [MaybeUninit::uninit(); wire::EVENT_SETTING_CHANGED_OLD_MAX_BYTES];
        let len = match settings_encode(value, &mut encoded) {
            Ok(len) if len <= encoded.len() => len,
            other => {
                captured.status = Err(other.err().unwrap_or(ERR_NO_SPACE));
                return captured;
            }
        };
        for (to, from) in captured.bytes.iter_mut().zip(&encoded[..len]) {
            // SAFETY: the generated encoder initialized the complete prefix above.
            *to = unsafe { from.assume_init() };
        }
        captured.len = len as u16;
        captured
    }
    pub(crate) fn bytes(&self) -> Result<&[u8], i32> {
        self.status?;
        Ok(&self.bytes[..usize::from(self.len)])
    }
}

/// Core's single runtime implements this producer-facing seam. Calls copy bounded
/// data synchronously and never invoke plugins or drain pending callbacks.
pub(crate) trait EventSink {
    fn setting(&self, now: u64, key: &str, old: Result<&[u8], i32>);
    fn hint(&self, tag: u32);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ChargeState {
    pub(crate) charging: bool,
    pub(crate) external_power: bool,
    pub(crate) low: bool,
}
impl ChargeState {
    pub(crate) fn from_status(status: &SystemStatus, external_power: Option<bool>) -> Self {
        let charging = status.battery.is_some_and(|battery| battery.charging);
        Self {
            charging,
            external_power: external_power.unwrap_or(false) || charging,
            low: status.battery.is_some_and(|battery| {
                !battery.charging && battery.percent <= crate::awake::LOW_BATTERY_PERCENT
            }),
        }
    }
    fn flags(self) -> u32 {
        u32::from(self.charging)
            | (u32::from(self.external_power) << 1)
            | (u32::from(self.low) << 2)
    }
}

#[derive(Clone, Copy)]
pub(crate) enum Hint<'a> {
    Plugins,
    Wifi(&'a WifiState),
    Power(&'a Power),
    Status,
    Charge(ChargeState),
}
impl Hint<'_> {
    pub(crate) fn tag(self) -> u32 {
        match self {
            Self::Plugins => wire::EVENT_PLUGINS_CHANGED_TAG,
            Self::Wifi(_) => wire::EVENT_WIFI_CHANGED_TAG,
            Self::Power(_) => wire::EVENT_POWER_CHANGED_TAG,
            Self::Status => wire::EVENT_STATUS_CHANGED_TAG,
            Self::Charge(_) => wire::EVENT_CHARGE_CHANGED_TAG,
        }
    }
    /// Encode current state at delivery, not a stale snapshot of a coalesced hint.
    pub(crate) fn encode(self, now_ms: u64, out: &mut [u8]) -> Result<usize, i32> {
        let record = match self {
            Self::Plugins => {
                wire::Record::EventPluginsChanged(wire::EventPluginsChanged { now_ms })
            }
            Self::Wifi(state) => wire::Record::EventWifiChanged(wire::EventWifiChanged {
                now_ms,
                state: match state {
                    WifiState::Off => 0,
                    WifiState::Joining { .. } => 2,
                    WifiState::Online { .. } => 3,
                    WifiState::Hosting { .. } => 4,
                    _ => 1,
                },
            }),
            Self::Power(power) => wire::Record::EventPowerChanged(wire::EventPowerChanged {
                now_ms,
                level: match power.level {
                    PowerLevel::Saving => 1,
                    PowerLevel::Offline => 2,
                    _ => 0,
                },
                flags: u32::from(power.automatic),
                stretch: power.stretch,
                mode: power.mode.as_str(),
            }),
            Self::Status => wire::Record::EventStatusChanged(wire::EventStatusChanged { now_ms }),
            Self::Charge(charge) => wire::Record::EventChargeChanged(wire::EventChargeChanged {
                now_ms,
                flags: charge.flags(),
            }),
        };
        wire::records_encode(&[record], out)
    }
}

/// Return every dropped edge to the shell reporter, including full-store and OOM.
/// Core supplies the slot's visibility result and a host-only logging callback.
pub(crate) fn setting(
    pending: &mut super::event_pending::Pending,
    now_ms: u64,
    key: &str,
    old: &OldValue,
    visibility: super::event_pending::Visibility,
    mut report: impl FnMut(u32, super::flat::DropReason),
) -> Result<(), i32> {
    let result = pending.setting(now_ms, key, old.bytes(), visibility);
    if result.is_err() {
        report(
            wire::EVENT_SETTING_CHANGED_TAG,
            super::flat::DropReason::Producer,
        );
    }
    result
}

#[cfg(test)]
#[path = "event_producers_tests.rs"]
mod tests;
