//! The X4's `core.device` host (`docs/abi/device.md` section 2): what the
//! firmware is, read by plugins through [`DeviceHost`].
//!
//! The firmware says what it knows once ([`Identity`], from its constants)
//! and keeps what changes in cells ([`X4Device::set_start`],
//! [`X4Device::set_value`]) where it already learns of the change: nothing is
//! polled, and a call allocates nothing (strings are borrowed from the
//! identity and the list the image carries, rows are visited one at a time).
//!
//! This part answers ops 1 (the device), 2 (named values), 4 (the plugins
//! built into the image) and 15 (the physical keys); every other part is
//! `ERR_NOT_FOUND` (the default of [`DeviceHost`]) until its slice lands, and
//! [`Identity::caps`] carries only the bits of the parts that are answered
//! (`check_caps_agree` is the test of it).

use core::cell::Cell;
use core::ops::ControlFlow;

use paperpaw_core::native_host::device_service::{
    BuiltinRow, DeviceFacts, DeviceHost, MAX_VALUES, Reading,
};
use paperpaw_sdk::firmware::Plugins;

/// What the firmware is, from its constants.
#[derive(Clone, Copy, Debug)]
pub struct Identity {
    /// The stable device id (`xteink-x4`).
    pub id: &'static str,
    /// Its name.
    pub model: &'static str,
    /// The store's device name (`x4`), empty for none.
    pub catalog: &'static str,
    /// The memory class: 0 small, 1 medium, 2 large.
    pub class: u32,
    /// `dev_device.caps`: only the bits of the parts this host answers.
    pub caps: u64,
    /// The power controls, as the kernel is told them.
    pub controls: u32,
    /// How many TLS sessions may be open at once.
    pub tls_sessions: u32,
    /// The names of the physical keys, in the device's order.
    pub keys: &'static [&'static str],
    /// The names of the readings op 2 lists (at most [`MAX_VALUES`] are used).
    pub values: &'static [&'static str],
}

/// One named reading, empty until the firmware has one.
#[derive(Debug)]
struct Slot {
    name: &'static str,
    value: Cell<Option<i64>>,
}

/// The X4's device, behind `core.device`.
#[derive(Debug)]
pub struct X4Device {
    identity: Identity,
    /// How this start came about (`dev_device.start`).
    start: Cell<u32>,
    /// How long the device slept before it woke, where known.
    slept_ms: Cell<Option<u64>>,
    /// The plugin crates the image carries; `None` for an image without a list.
    built_in: Option<Plugins>,
    slots: [Option<Slot>; MAX_VALUES as usize],
}

impl X4Device {
    /// The device for `identity`; `list` is the image's own record of the
    /// plugin crates built in (`paperpaw_update::Plugins`, the firmware's
    /// `PLUGINS`).
    #[must_use]
    pub fn new(identity: Identity, list: &'static [u8]) -> Self {
        let mut slots = [const { None }; MAX_VALUES as usize];
        for (slot, name) in slots.iter_mut().zip(identity.values) {
            *slot = Some(Slot {
                name,
                value: Cell::new(None),
            });
        }
        Self {
            identity,
            start: Cell::new(0),
            slept_ms: Cell::new(None),
            built_in: Plugins::read_static(list),
            slots,
        }
    }

    /// How this start came about: 0 unknown, 1 power on, 2 woke, 3 restart,
    /// 4 after a fault.
    pub fn set_start(&self, start: u32) {
        self.start.set(start);
    }

    /// How long the device slept before it woke.
    pub fn set_slept_ms(&self, slept_ms: Option<u64>) {
        self.slept_ms.set(slept_ms);
    }

    /// The reading `name` (one of [`Identity::values`]), `None` where it
    /// cannot be read now. False where the identity names no such reading.
    pub fn set_value(&self, name: &str, value: Option<i64>) -> bool {
        let found = self.slots.iter().flatten().find(|slot| slot.name == name);
        found.is_some_and(|slot| {
            slot.value.set(value);
            true
        })
    }
}

impl DeviceHost for X4Device {
    fn device(&self, visit: &mut dyn FnMut(&DeviceFacts<'_>)) -> Result<(), i32> {
        let this = &self.identity;
        visit(&DeviceFacts {
            id: this.id,
            model: this.model,
            catalog: this.catalog,
            class: this.class,
            caps: this.caps,
            controls: this.controls,
            tls_sessions: this.tls_sessions,
            start: self.start.get(),
            slept_ms: self.slept_ms.get(),
        });
        Ok(())
    }

    fn values(&self, visit: &mut dyn FnMut(&Reading<'_>) -> ControlFlow<()>) -> Result<(), i32> {
        for slot in self.slots.iter().flatten() {
            let reading = Reading {
                name: slot.name,
                value: slot.value.get(),
            };
            if visit(&reading).is_break() {
                break;
            }
        }
        Ok(())
    }

    /// The crates the image lists whose name is a plugin id
    /// (`paperpaw-plugin-<id>`); any other crate is not a plugin and is left
    /// out, as an update's check leaves it out ([`PluginCrate::plugin_id`]).
    ///
    /// [`PluginCrate::plugin_id`]: paperpaw_sdk::firmware::PluginCrate::plugin_id
    fn builtin(
        &self,
        visit: &mut dyn FnMut(&BuiltinRow<'_>) -> ControlFlow<()>,
    ) -> Result<(), i32> {
        let Some(list) = &self.built_in else {
            return Ok(());
        };
        for krate in list.iter() {
            let Some(id) = krate.plugin_id() else {
                continue;
            };
            let row = BuiltinRow {
                id,
                version: krate.version,
            };
            if visit(&row).is_break() {
                break;
            }
        }
        Ok(())
    }

    fn keys(&self, visit: &mut dyn FnMut(&str) -> ControlFlow<()>) -> Result<(), i32> {
        for name in self.identity.keys {
            if visit(name).is_break() {
                break;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use std::string::{String, ToString};
    use std::vec::Vec;

    use paperpaw_core::native_host::device_service::{caps, check_caps_agree};
    use paperpaw_native::host_glue::ERR_NOT_FOUND;
    use paperpaw_sdk::firmware::PluginCrate;

    use super::*;

    const KEYS: &[&str] = &["back", "confirm", "left", "right"];
    const VALUES: &[&str] = &["usb-pin-high", "usb-frames"];

    fn list(crates: &[(&'static str, &'static str)]) -> &'static [u8] {
        let record = Plugins::record(
            crates
                .iter()
                .map(|&(name, version)| PluginCrate { name, version }),
        )
        .unwrap();
        std::boxed::Box::leak(record.into_boxed_slice())
    }

    fn identity() -> Identity {
        Identity {
            id: "xteink-x4",
            model: "Xteink X4",
            catalog: "x4",
            class: 0,
            caps: caps::NETWORK | caps::SLEEP | caps::ROTATION | caps::BUTTON_ROW,
            controls: 0b111,
            tls_sessions: 1,
            keys: KEYS,
            values: VALUES,
        }
    }

    fn device(crates: &[(&'static str, &'static str)]) -> X4Device {
        X4Device::new(identity(), list(crates))
    }

    fn built_in(device: &X4Device) -> Vec<(String, String)> {
        let mut rows = Vec::new();
        device
            .builtin(&mut |row| {
                rows.push((row.id.to_string(), row.version.to_string()));
                ControlFlow::Continue(())
            })
            .unwrap();
        rows
    }

    #[test]
    fn the_device_says_what_the_firmware_is() {
        let device = device(&[]);
        device.set_start(2);
        device.set_slept_ms(Some(9_000));
        let mut seen = None;
        device
            .device(&mut |facts| {
                seen = Some((
                    facts.id.to_string(),
                    facts.catalog.to_string(),
                    facts.class,
                    facts.caps,
                    facts.controls,
                    facts.tls_sessions,
                    facts.start,
                    facts.slept_ms,
                ));
            })
            .unwrap();
        let (id, catalog, class, caps_seen, controls, tls, start, slept) = seen.unwrap();
        assert_eq!(
            (id.as_str(), catalog.as_str(), class),
            ("xteink-x4", "x4", 0)
        );
        assert_eq!(caps_seen, identity().caps);
        assert_eq!((controls, tls, start, slept), (0b111, 1, 2, Some(9_000)));
    }

    /// The caps name exactly the parts that are answered: Wi-Fi, Bluetooth
    /// and firmware are not here yet.
    #[test]
    fn caps_and_the_answered_parts_agree() {
        assert_eq!(check_caps_agree(&device(&[])), Ok(()));
        let device = device(&[]);
        assert_eq!(device.wifi(&mut |_| {}), Err(ERR_NOT_FOUND));
        assert_eq!(device.bluetooth(), Err(ERR_NOT_FOUND));
        assert_eq!(device.firmware(&mut |_| {}), Err(ERR_NOT_FOUND));
    }

    /// The ids are the crates' plugin ids, in the image's order, so the list
    /// compares equal with an update's check; crates that are no plugin are
    /// not rows.
    #[test]
    fn the_built_in_list_is_the_images_plugin_ids() {
        let device = device(&[
            ("paperpaw-plugin-stats", "1.0.8"),
            ("paperpaw-update", "0.6.0"),
            ("paperpaw-plugin-hyphenation-de", "0.2.0"),
            ("paperpaw-plugin-Bad", "9"),
            ("paperpaw-plugin-", "9"),
        ]);
        assert_eq!(
            built_in(&device),
            [
                ("stats".to_string(), "1.0.8".to_string()),
                ("hyphenation-de".to_string(), "0.2.0".to_string()),
            ]
        );
    }

    #[test]
    fn a_visitor_that_stops_is_not_called_again() {
        let device = device(&[
            ("paperpaw-plugin-a", "1"),
            ("paperpaw-plugin-b", "1"),
            ("paperpaw-plugin-c", "1"),
        ]);
        let mut calls = 0;
        device
            .builtin(&mut |_| {
                calls += 1;
                ControlFlow::Break(())
            })
            .unwrap();
        assert_eq!(calls, 1);
        let mut keys = 0;
        device
            .keys(&mut |_| {
                keys += 1;
                ControlFlow::Break(())
            })
            .unwrap();
        assert_eq!(keys, 1);
    }

    /// An image without a list (or one that cannot be read) has no rows, and
    /// still is a host with the part: an empty list, not "not here".
    #[test]
    fn an_image_without_a_list_has_an_empty_one() {
        let device = X4Device::new(identity(), b"not a list");
        assert_eq!(built_in(&device), []);
    }

    #[test]
    fn the_keys_are_in_the_devices_order() {
        let device = device(&[]);
        let mut names = Vec::new();
        device
            .keys(&mut |name| {
                names.push(name.to_string());
                ControlFlow::Continue(())
            })
            .unwrap();
        assert_eq!(names, KEYS);
    }

    /// A reading the firmware has not got is `None`, never a zero; a name
    /// the identity does not list cannot be set.
    #[test]
    fn values_are_what_the_firmware_last_read() {
        let device = device(&[]);
        let read = |device: &X4Device| {
            let mut rows = Vec::new();
            device
                .values(&mut |reading| {
                    rows.push((reading.name.to_string(), reading.value));
                    ControlFlow::Continue(())
                })
                .unwrap();
            rows
        };
        assert_eq!(
            read(&device),
            [
                ("usb-pin-high".to_string(), None),
                ("usb-frames".to_string(), None)
            ]
        );
        assert!(device.set_value("usb-frames", Some(7)));
        assert!(!device.set_value("temperature-c", Some(20)));
        assert_eq!(read(&device)[1], ("usb-frames".to_string(), Some(7)));
        assert!(device.set_value("usb-frames", None));
        assert_eq!(read(&device)[1].1, None);
    }

    /// More names than op 2 lists are not kept.
    #[test]
    fn no_more_values_than_op_2_lists() {
        const MANY: &[&str] = &["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"];
        let device = X4Device::new(
            Identity {
                values: MANY,
                ..identity()
            },
            list(&[]),
        );
        let mut count = 0;
        device
            .values(&mut |_| {
                count += 1;
                ControlFlow::Continue(())
            })
            .unwrap();
        assert_eq!(count, MAX_VALUES);
    }
}
