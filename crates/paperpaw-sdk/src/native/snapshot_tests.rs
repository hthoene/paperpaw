#![allow(clippy::unwrap_used, reason = "fixture errors must fail tests")]
use super::*;
use core::{cell::Cell, ffi::c_void, mem::MaybeUninit, ptr};
use paperpaw_native::host_glue::{self as abi, PpHost};

struct Context {
    snapshot: wire::Snapshot,
    result: i32,
    corrupt_reserved: bool,
    extension: bool,
    total_bytes: Option<u32>,
    calls: Cell<usize>,
}

unsafe extern "C" fn snapshot_slot(ctx: *mut c_void, out: PpBuffer) -> i32 {
    // SAFETY: tests retain a live context throughout each host call.
    let ctx = unsafe { &*ctx.cast::<Context>() };
    ctx.calls.set(ctx.calls.get() + 1);
    assert_eq!(out.cap, wire::SNAPSHOT_MAX_BYTES);
    if ctx.result != abi::OK {
        return ctx.result;
    }
    // SAFETY: wrapper supplies call-local writable storage for the claimed capacity.
    let output =
        unsafe { core::slice::from_raw_parts_mut(out.ptr.cast::<MaybeUninit<u8>>(), out.cap) };
    match wire::snapshot_encode(&ctx.snapshot, output) {
        Ok(length) => {
            if ctx.corrupt_reserved {
                // The generated canonical prefix ends with reserved bytes.
                output[length - 1].write(1);
            }
            if ctx.extension {
                for byte in &mut output[length..length + 8] {
                    byte.write(0x91);
                }
            }
            if ctx.extension || ctx.total_bytes.is_some() {
                let total = ctx.total_bytes.unwrap_or((length + 8) as u32);
                // The approved snapshot header places total_bytes at offset 4.
                for (slot, byte) in output[4..8].iter_mut().zip(total.to_le_bytes()) {
                    slot.write(byte);
                }
            }
            abi::OK
        }
        Err(error) => error,
    }
}

fn table() -> PpHost {
    let mut table = abi::host_table();
    table.env_snapshot = snapshot_slot;
    table
}

fn context() -> Context {
    let mut snapshot = wire::Snapshot::new(480, 800, 220, wire::PixelFormat::Gray4);
    snapshot.monotonic_ms = 0x1234_5678_9abc_def0;
    snapshot.unix_seconds = Some(-123);
    snapshot.touch = true;
    let mut battery = wire::Battery::new(87);
    battery.millivolts = Some(4102);
    battery.charging = true;
    battery.saver = true;
    snapshot.battery = Some(battery);
    Context {
        snapshot,
        result: abi::OK,
        corrupt_reserved: false,
        extension: false,
        total_bytes: None,
        calls: Cell::new(0),
    }
}

fn capture(ctx: &mut Context) -> Result<Snapshot, Error> {
    let table = table();
    capture_snapshot(&Host {
        table: &table,
        ctx: ptr::from_mut(ctx).cast(),
    })
}

#[test]
fn generated_snapshot_fields_are_converted_to_owned_sdk_values() {
    let mut ctx = context();
    let got = capture(&mut ctx).unwrap();
    assert_eq!(got.instant(), Instant(0x1234_5678_9abc_def0));
    assert_eq!(got.unix_seconds(), Some(-123));
    assert_eq!(got.battery_percent(), Some(87));
    assert_eq!(got.battery_millivolts(), Some(4102));
    assert!(got.is_charging());
    assert!(got.is_power_saving());
    assert_eq!(
        (got.display_width(), got.display_height(), got.display_dpi()),
        (480, 800, 220)
    );
    assert!(got.has_touch());
    assert_eq!(ctx.calls.get(), 1);
    // Changes after capture cannot change the owned value.
    ctx.snapshot.battery = None;
    assert_eq!(capture(&mut ctx).unwrap().battery_percent(), None);
    assert_eq!(got.battery_percent(), Some(87));
}

#[test]
fn absent_values_stay_distinct_from_present_zeroes() {
    let mut ctx = context();
    ctx.snapshot.battery = None;
    ctx.snapshot.unix_seconds = None;
    ctx.snapshot.monotonic_ms = 0;
    ctx.snapshot.touch = false;
    let absent = capture(&mut ctx).unwrap();
    assert_eq!(
        (
            absent.battery_percent(),
            absent.battery_millivolts(),
            absent.unix_seconds()
        ),
        (None, None, None)
    );
    assert_eq!(absent.instant(), Instant(0));
    assert!(!absent.is_charging());
    assert!(!absent.is_power_saving());
    assert!(!absent.has_touch());
    ctx.snapshot.battery = Some(wire::Battery::new(0));
    ctx.snapshot.unix_seconds = Some(0);
    let zero_percent = capture(&mut ctx).unwrap();
    assert_eq!(zero_percent.battery_percent(), Some(0));
    assert_eq!(zero_percent.battery_millivolts(), None);
    assert_eq!(zero_percent.unix_seconds(), Some(0));
    assert!(!zero_percent.is_power_saving());
    let mut battery = wire::Battery::new(0);
    battery.millivolts = Some(0);
    ctx.snapshot.battery = Some(battery);
    assert_eq!(capture(&mut ctx).unwrap().battery_millivolts(), Some(0));
}

#[test]
fn host_refusals_and_malformed_snapshots_are_reported() {
    let mut ctx = context();
    for (status, error) in [
        (abi::ERR_UNSUPPORTED, Error::Unsupported),
        (abi::ERR_STATE, Error::State),
        (abi::ERR_NO_SPACE, Error::NoSpace),
        (abi::ERR_IO, Error::Io),
        (-901, Error::Other(-901)),
        (1, Error::Other(1)),
    ] {
        ctx.result = status;
        assert_eq!(capture(&mut ctx), Err(error));
    }
    ctx.result = abi::OK;
    ctx.corrupt_reserved = true;
    assert_eq!(capture(&mut ctx), Err(Error::Invalid));
}

#[test]
fn default_host_table_reports_unsupported() {
    let table = abi::host_table();
    let host = Host {
        table: &table,
        ctx: ptr::null_mut(),
    };
    assert_eq!(capture_snapshot(&host), Err(Error::Unsupported));
}

#[test]
fn extreme_wall_seconds_preserve_the_full_wire_range() {
    let mut ctx = context();
    for seconds in [i64::MIN, i64::MAX] {
        ctx.snapshot.unix_seconds = Some(seconds);
        assert_eq!(capture(&mut ctx).unwrap().unix_seconds(), Some(seconds));
    }
}

#[test]
fn same_version_extension_bytes_preserve_the_known_snapshot() {
    let mut ctx = context();
    let canonical = capture(&mut ctx).unwrap();
    ctx.extension = true;
    assert_eq!(capture(&mut ctx), Ok(canonical));
}

#[test]
fn declared_totals_outside_the_generated_bounds_are_invalid() {
    let mut ctx = context();
    for total in [0, 51, 129, u32::MAX] {
        ctx.total_bytes = Some(total);
        assert_eq!(capture(&mut ctx), Err(Error::Invalid));
    }
}
