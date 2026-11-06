//! Existing environment semantics through the bounded scoped slot-4 adapter.
#![allow(
    clippy::unwrap_used,
    reason = "test fixture errors must fail assertions"
)]
#[path = "../src/abi7/context.rs"]
mod context;
#[path = "../src/abi7/snapshot.rs"]
mod snapshot;
#[path = "../src/abi7/snapshot_wire.rs"]
#[allow(
    clippy::all,
    clippy::pedantic,
    reason = "schema-generated snapshot codec"
)]
pub mod snapshot_wire;

use context::{Frame, FrameSlot};
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NO_SPACE, ERR_STATE, PpBuffer};
use paperpaw_platform::{
    Battery, DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, PixelFormat, Rotation,
    SystemStatus, WallClock,
};
use paperpaw_sdk::{Id, Manifest, Plugin, RegisterError, Registrar, Settings, Version};
use snapshot_wire::{SNAPSHOT_BYTES, SNAPSHOT_MAX_BYTES, Snapshot, snapshot_decode};
use std::{cell::RefCell, mem::MaybeUninit, pin::pin, ptr, rc::Rc};

const DEVICE: DeviceInfo = DeviceInfo::new(
    "snapshot-test",
    "Snapshot fixture",
    DisplayInfo::new(480, 800, 220, PixelFormat::Gray4, Rotation::Deg0, None),
    InputCaps::new(true, false, false, &[]),
    MemoryClass::Small,
);

struct View {
    value: Snapshot,
    settings: Settings,
    translator: paperpaw_i18n::Translator,
}
impl Frame for View {
    fn settings(&self) -> &Settings {
        &self.settings
    }
    fn translator(&self) -> &paperpaw_i18n::Translator {
        &self.translator
    }
    fn snapshot(&self) -> Result<Snapshot, i32> {
        Ok(self.value)
    }
}

fn view(status: SystemStatus) -> Result<View, i32> {
    Ok(View {
        value: snapshot::capture(&DEVICE, &status, Instant(0))?,
        settings: Settings::new(),
        translator: paperpaw_i18n::Translator::new(),
    })
}

fn output(slot: &FrameSlot, out: &mut [u8]) -> i32 {
    // SAFETY: exclusively borrowed live output does not overlap the host slot.
    unsafe {
        snapshot::snapshot(
            slot,
            PpBuffer {
                ptr: out.as_mut_ptr(),
                cap: out.len(),
            },
        )
    }
}

#[test]
fn absence_zero_and_negative_wall_seconds_preserve_existing_status() {
    for battery in [
        None,
        Some(Battery::new(0, false, None)),
        Some(Battery::new(0, true, Some(0)).with_saver(true)),
    ] {
        for unix_ms in [None, Some(0), Some(-1), Some(-1001)] {
            let status = SystemStatus::new(
                unix_ms.map(|ms| WallClock::new(Instant(10), ms, None)),
                battery,
            );
            let value = snapshot::capture(&DEVICE, &status, Instant(10)).unwrap();
            assert_eq!(value.monotonic_ms, 10);
            assert_eq!(value.unix_seconds, unix_ms.map(|ms| ms.div_euclid(1000)));
            assert_eq!(
                value
                    .battery
                    .map(|b| (b.percent, b.charging, b.saver, b.millivolts)),
                battery.map(|b| (b.percent, b.charging, b.saver, b.millivolts))
            );
        }
    }
}

#[test]
fn every_pixel_format_retains_display_and_touch_semantics() {
    for format in [
        PixelFormat::Mono1,
        PixelFormat::Gray2,
        PixelFormat::Gray4,
        PixelFormat::Gray8,
        PixelFormat::Rgb565,
        PixelFormat::Rgb888,
        PixelFormat::Other { code: 0, bits: 0 },
        PixelFormat::Other { code: 91, bits: 12 },
    ] {
        for touch in [false, true] {
            let mut device = DEVICE;
            device.display.format = format;
            device.input.touch = touch;
            let value =
                snapshot::capture(&device, &SystemStatus::default(), Instant(u64::MAX)).unwrap();
            assert_eq!(
                (value.width, value.height, value.dpi, value.touch),
                (480, 800, 220, touch)
            );
            assert_eq!(value.pixel.gray_levels(), format.gray_levels());
            assert_eq!(value.pixel.bits_per_pixel(), format.bits_per_pixel());
            assert_eq!(value.monotonic_ms, u64::MAX);
        }
    }
}

#[test]
fn fresh_frames_preserve_unknown_zero_and_charging_transitions() {
    let slot = pin!(FrameSlot::new());
    let mut bytes = [0; SNAPSHOT_BYTES];
    for battery in [
        None,
        Some(Battery::new(0, false, None)),
        Some(Battery::new(0, true, Some(0))),
        None,
    ] {
        let frame = view(SystemStatus::new(None, battery)).unwrap();
        slot.as_ref()
            .scope(&frame, || {
                assert_eq!(output(&slot, &mut bytes), SNAPSHOT_BYTES as i32);
                assert_eq!(snapshot_decode(&bytes).unwrap(), frame.value);
            })
            .unwrap();
        assert_eq!(output(&slot, &mut bytes), ERR_STATE);
    }
    let invalid = view(SystemStatus::new(
        None,
        Some(Battery::new(101, false, None)),
    ))
    .unwrap();
    bytes.fill(0xa5);
    slot.as_ref()
        .scope(&invalid, || {
            assert_eq!(output(&slot, &mut bytes), ERR_INVALID)
        })
        .unwrap();
    assert!(bytes.iter().all(|byte| *byte == 0xa5));
}

#[test]
fn short_invalid_and_uninitialized_outputs_have_bounded_writes() {
    let slot = pin!(FrameSlot::new());
    let frame = view(SystemStatus::default()).unwrap();
    slot.as_ref()
        .scope(&frame, || {
            for length in [0, SNAPSHOT_BYTES - 1] {
                let mut out = vec![0xa5; length];
                assert_eq!(output(&slot, &mut out), ERR_NO_SPACE);
                assert!(out.iter().all(|b| *b == 0xa5));
            }
            let mut oversized = [0xa5; SNAPSHOT_MAX_BYTES + 1];
            assert_eq!(output(&slot, &mut oversized), ERR_NO_SPACE);
            assert!(oversized.iter().all(|b| *b == 0xa5));
            // SAFETY: null is rejected before any nonempty output slice is built.
            assert_eq!(
                unsafe {
                    snapshot::snapshot(
                        &slot,
                        PpBuffer {
                            ptr: ptr::null_mut(),
                            cap: SNAPSHOT_BYTES,
                        },
                    )
                },
                ERR_INVALID
            );
            let mut out = [MaybeUninit::<u8>::uninit(); SNAPSHOT_BYTES];
            // SAFETY: output is valid uninitialized exclusive storage.
            assert_eq!(
                unsafe {
                    snapshot::snapshot(
                        &slot,
                        PpBuffer {
                            ptr: out.as_mut_ptr().cast(),
                            cap: out.len(),
                        },
                    )
                },
                SNAPSHOT_BYTES as i32
            );
            // SAFETY: successful encoder return initialized exactly SNAPSHOT_BYTES.
            let bytes = unsafe { std::slice::from_raw_parts(out.as_ptr().cast::<u8>(), out.len()) };
            assert_eq!(snapshot_decode(bytes).unwrap(), frame.value);
            let mut extra = [0xa5; SNAPSHOT_MAX_BYTES];
            assert_eq!(output(&slot, &mut extra), SNAPSHOT_BYTES as i32);
            assert!(extra[SNAPSHOT_BYTES..].iter().all(|b| *b == 0xa5));
        })
        .unwrap();
}

#[test]
fn scope_cleanup_and_nested_refusal_preserve_the_current_snapshot() {
    let slot = pin!(FrameSlot::new());
    let frame = view(SystemStatus::default()).unwrap();
    let mut bytes = [0xa5; SNAPSHOT_BYTES];
    assert_eq!(output(&slot, &mut bytes), ERR_STATE);
    slot.as_ref()
        .scope(&frame, || {
            assert!(matches!(
                slot.as_ref().scope(&frame, || ()),
                Err(context::Error::Busy)
            ));
            assert_eq!(output(&slot, &mut bytes), SNAPSHOT_BYTES as i32);
            assert_eq!(
                slot.with(|f| ptr::eq(f.settings(), &frame.settings)),
                Ok(true)
            );
            assert_eq!(
                slot.with(|f| f.translator().tr("key").to_owned()),
                Ok("key".into())
            );
        })
        .unwrap();
    assert_eq!(output(&slot, &mut bytes), ERR_STATE);
    let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        slot.as_ref()
            .scope(&frame, || panic!("fixture unwind"))
            .unwrap();
    }));
    assert!(unwind.is_err());
    assert_eq!(output(&slot, &mut bytes), ERR_STATE);
    slot.as_ref()
        .scope(&frame, || {
            assert_eq!(output(&slot, &mut bytes), SNAPSHOT_BYTES as i32)
        })
        .unwrap();
}

struct Probe(Rc<RefCell<Vec<Snapshot>>>);
impl Plugin for Probe {
    fn manifest(&self) -> Manifest {
        Manifest::new(
            Id::new_static("snapshot-probe"),
            "Snapshot probe",
            Version::new(1, 0, 0),
        )
    }
    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let seen = self.0.clone();
        r.subscribe(
            paperpaw_sdk::builtin::events::STARTED,
            Box::new(move |env, _, _| {
                let slot = pin!(FrameSlot::new());
                let mut bytes = [0; SNAPSHOT_BYTES];
                slot.as_ref()
                    .scope(env, || {
                        assert_eq!(output(&slot, &mut bytes), SNAPSHOT_BYTES as i32);
                        let decoded = snapshot_decode(&bytes).unwrap();
                        assert_eq!(decoded.unix_seconds, paperpaw_sdk::time::unix_seconds(env));
                        seen.borrow_mut().push(decoded);
                    })
                    .unwrap();
                assert_eq!(output(&slot, &mut bytes), ERR_STATE);
            }),
        )
    }
}

#[test]
fn actual_kernel_event_env_can_supply_the_scoped_snapshot() {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let mut kernel = paperpaw_core::Kernel::builder(DEVICE)
        .plugin(Probe(seen.clone()))
        .build()
        .unwrap();
    kernel.set_status(
        Instant(0),
        SystemStatus::new(
            Some(WallClock::new(Instant(0), 0, None)),
            Some(Battery::new(0, false, Some(0))),
        ),
    );
    kernel.start(Instant(0));
    assert_eq!(seen.borrow().len(), 1);
    let actual = seen.borrow()[0];
    assert_eq!(actual.unix_seconds, Some(0));
    assert_eq!(actual.battery.unwrap().millivolts, Some(0));
}
