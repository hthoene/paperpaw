//! Edge detection: reports say which keys are down, the decoder says what
//! changed.

use alloc::vec::Vec;

use crate::key::Key;
use crate::map::ReportMap;

/// A key that went down or up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyChange {
    /// The key.
    pub key: Key,
    /// `true` for a press, `false` for a release.
    pub pressed: bool,
}

/// Decodes the input reports of one device.
#[derive(Debug, Clone)]
pub struct Decoder {
    map: ReportMap,
    /// The keys down, each with the report id that said so (0 for none).
    down: Vec<(u8, Key)>,
}

impl Decoder {
    /// A decoder for the device whose report descriptor is `map`.
    pub fn new(map: ReportMap) -> Self {
        Decoder {
            map,
            down: Vec::new(),
        }
    }

    /// A decoder for a boot protocol keyboard: reports of 8 bytes
    /// (modifiers, reserved, six keycodes) without a report id.
    pub fn boot_keyboard() -> Self {
        Self::new(ReportMap::boot_keyboard())
    }

    /// A decoder for the device whose report descriptor is `descriptor`:
    /// the boot keyboard's ([`Decoder::boot_keyboard`]) where it tells of
    /// no input report (empty, as for a device without a report map) or
    /// cannot be read.
    pub fn for_descriptor(descriptor: &[u8]) -> Self {
        match ReportMap::parse(descriptor) {
            Ok(map) if !map.reports.is_empty() => Self::new(map),
            _ => Self::boot_keyboard(),
        }
    }

    /// The changes a report makes: releases of keys that are no longer
    /// down, then presses of keys that are newly down. `data` is the
    /// report without its id, which is `report_id` (`None` if the device
    /// has none).
    ///
    /// Keycodes 0 and 1 are ignored, and a report with error roll-over
    /// (1) changes nothing. A report of an unknown id, or one too short
    /// for its fields, changes nothing.
    pub fn report(&mut self, report_id: Option<u8>, data: &[u8]) -> Vec<KeyChange> {
        let id = report_id.unwrap_or(0);
        let Some(keys) = self.map.report(id).and_then(|r| r.keys(data)) else {
            return Vec::new();
        };
        let mut now: Vec<Key> = Vec::new();
        for key in keys {
            if !now.contains(&key) {
                now.push(key);
            }
        }
        let mut changes = Vec::new();
        let gone: Vec<Key> = self
            .down
            .iter()
            .filter(|(i, k)| *i == id && !now.contains(k))
            .map(|(_, k)| *k)
            .collect();
        for key in gone {
            self.down.retain(|e| *e != (id, key));
            if !self.is_down(key) {
                changes.push(KeyChange {
                    key,
                    pressed: false,
                });
            }
        }
        for key in now {
            if !self.down.contains(&(id, key)) {
                let was_down = self.is_down(key);
                self.down.push((id, key));
                if !was_down {
                    changes.push(KeyChange { key, pressed: true });
                }
            }
        }
        changes
    }

    /// Releases every key that is down, for when the link ends.
    pub fn reset(&mut self) -> Vec<KeyChange> {
        let mut changes: Vec<KeyChange> = Vec::new();
        for (_, key) in self.down.drain(..) {
            if !changes.iter().any(|c| c.key == key) {
                changes.push(KeyChange {
                    key,
                    pressed: false,
                });
            }
        }
        changes
    }

    fn is_down(&self, key: Key) -> bool {
        self.down.iter().any(|(_, k)| *k == key)
    }
}
