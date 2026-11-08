//! What the device check reads from the host, as plain values.
use crate::steps::{BatteryReading, Facts, MemoryReading};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::native::{
    Error, Host,
    device::{Device, DeviceCapability, DeviceCursor, WifiState},
    snapshot::{Snapshot, capture_snapshot},
};
use paperpaw_ui_kit::{mem::push, text::copy};

/// What the device is and has.
#[derive(Debug, Clone, Default)]
pub struct Machine {
    pub id: String,
    pub name: String,
    pub facts: Facts,
    /// The physical keys, in the device's order.
    pub keys: Vec<String>,
    /// The panel's pixel format as the report names it.
    pub format: &'static str,
    pub dpi: u16,
}

/// The radio now.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Radio {
    pub on: bool,
    pub online: bool,
    pub wanted: bool,
    pub busy: bool,
}

fn with_device<T>(
    host: &Host<'_>,
    f: impl FnOnce(&Device, &mut [u8; 64], &mut [u8; 1024]) -> Result<T, Error>,
) -> Result<T, Error> {
    // The buffers are on the heap, not in this frame: the device reads would
    // otherwise cost the X4's stack more than a kilobyte each.
    let mut buffer = Vec::new();
    buffer
        .try_reserve_exact(64 + 1024)
        .map_err(|_| Error::NoMemory)?;
    buffer.resize(64 + 1024, 0);
    let (input, out) = buffer.split_at_mut(64);
    let (Ok(input), Ok(out)) = (
        <&mut [u8; 64]>::try_from(input),
        <&mut [u8; 1024]>::try_from(out),
    ) else {
        return Err(Error::State);
    };
    let device = Device::open(host)?;
    let result = f(&device, input, out);
    let released = device.release(host);
    let value = result?;
    released?;
    Ok(value)
}

/// The report's name of a pixel format.
fn format_name(snapshot: &Snapshot) -> &'static str {
    match (snapshot.display_gray_levels(), snapshot.display_is_colour()) {
        (_, true) => "Colour",
        (Some(2), _) => "Mono1",
        (Some(4), _) => "Gray2",
        (Some(16), _) => "Gray4",
        (Some(256), _) => "Gray8",
        _ => "Other",
    }
}

/// The device, its keys and what it has.
///
/// # Errors
/// The host's refusals and allocation failures.
#[inline(never)]
pub fn machine(host: &Host<'_>) -> Result<Machine, Error> {
    let snapshot = capture_snapshot(host)?;
    with_device(host, |device, input, out| {
        let identity = device.device(host, input, out)?;
        let (id, name) = (copy(identity.id())?, copy(identity.model())?);
        let mut facts = Facts {
            buttons: false,
            touch: identity.supports(DeviceCapability::Touch),
            frontlight: identity.supports(DeviceCapability::Frontlight),
            wifi: identity.supports(DeviceCapability::Wifi),
            sleep: identity.supports(DeviceCapability::Sleep),
        };
        let mut keys = Vec::new();
        let page = device.keys(host, input, out)?;
        for key in page.rows() {
            push(&mut keys, copy(key.name())?)?;
        }
        facts.buttons = !keys.is_empty();
        Ok(Machine {
            id,
            name,
            facts,
            keys,
            format: format_name(&snapshot),
            dpi: snapshot.display_dpi(),
        })
    })
}

/// The battery, where there is one.
///
/// # Errors
/// The snapshot's refusal.
pub fn battery(snapshot: &Snapshot) -> Option<BatteryReading> {
    snapshot.battery_percent().map(|percent| BatteryReading {
        percent,
        millivolts: snapshot.battery_millivolts(),
    })
}

/// The heap and stack, where the host reports them.
///
/// # Errors
/// The host's refusals.
pub fn memory(host: &Host<'_>) -> Result<MemoryReading, Error> {
    with_device(host, |device, input, out| {
        let m = device.memory(host, input, out)?;
        Ok(MemoryReading {
            heap_free: m.heap_free(),
            heap_size: m.heap_size(),
            stack_free: m.stack_free(),
            stack_size: m.stack_size(),
        })
    })
}

/// A hardware value by name (`usb-pin-high`, `usb-frames`), where the device has it.
///
/// # Errors
/// The host's refusals.
pub fn value(host: &Host<'_>, wanted: &str) -> Result<Option<i32>, Error> {
    with_device(host, |device, input, out| {
        let page = device.values(host, input, out)?;
        for row in page.rows() {
            if row.name() == wanted {
                return Ok(Some(row.value()));
            }
        }
        Ok(None)
    })
}

/// The radio now.
///
/// # Errors
/// The host's refusals.
pub fn radio(host: &Host<'_>) -> Result<Radio, Error> {
    with_device(host, |device, input, out| {
        let wifi = device.wifi(host, input, out)?;
        let state = wifi.state();
        Ok(Radio {
            on: !matches!(state, WifiState::Off),
            online: matches!(state, WifiState::Online),
            wanted: wifi.wanted(),
            busy: wifi.busy(),
        })
    })
}

/// How many networks the last search found.
///
/// # Errors
/// The host's refusals.
pub fn networks(host: &Host<'_>) -> Result<usize, Error> {
    with_device(host, |device, input, out| {
        let page = device.networks(host, DeviceCursor::start(), input, out)?;
        Ok(usize::try_from(page.end().total()).unwrap_or(usize::MAX))
    })
}

/// Whether the host started this run after a wake, a power-on or a restart.
///
/// # Errors
/// The host's refusals.
pub fn start(host: &Host<'_>) -> Result<paperpaw_sdk::native::device::StartKind, Error> {
    with_device(host, |device, input, out| {
        Ok(device.device(host, input, out)?.start())
    })
}
