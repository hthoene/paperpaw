//! Bounded, owned environment values captured inside a scoped dispatch frame.

use super::{
    context::{Frame, FrameSlot},
    snapshot_wire::{self, Battery, PixelFormat, Snapshot},
};
use paperpaw_native::host_glue::{ERR_NO_SPACE, ERR_STATE, ERR_UNSUPPORTED, OK, PpBuffer, buffers};
use paperpaw_platform::{DeviceInfo, Instant, SystemStatus};
use paperpaw_sdk::Env;

impl Frame for Env<'_> {
    fn settings(&self) -> &paperpaw_sdk::Settings {
        Env::settings(self)
    }

    fn translator(&self) -> &paperpaw_i18n::Translator {
        Env::i18n(self)
    }

    fn snapshot(&self) -> Result<Snapshot, i32> {
        capture(self.device(), self.status(), self.now())
    }
}

/// Copy source values without retaining any device, status or environment borrow.
/// Wall time matches SDK UnixTime::now().secs(), including negative rounding.
pub(crate) fn capture(
    device: &DeviceInfo,
    status: &SystemStatus,
    now: Instant,
) -> Result<Snapshot, i32> {
    use paperpaw_platform::PixelFormat as Source;
    let pixel = match device.display.format {
        Source::Mono1 => PixelFormat::Mono1,
        Source::Gray2 => PixelFormat::Gray2,
        Source::Gray4 => PixelFormat::Gray4,
        Source::Gray8 => PixelFormat::Gray8,
        Source::Rgb565 => PixelFormat::Rgb565,
        Source::Rgb888 => PixelFormat::Rgb888,
        Source::Other { code, bits } => PixelFormat::Other { code, bits },
        _ => return Err(ERR_UNSUPPORTED),
    };
    Ok(Snapshot {
        monotonic_ms: now.millis(),
        unix_seconds: status
            .clock
            .map(|clock| clock.unix_millis_at(now).div_euclid(1000)),
        battery: status.battery.map(|battery| Battery {
            percent: battery.percent,
            charging: battery.charging,
            saver: battery.saver,
            millivolts: battery.millivolts,
        }),
        width: device.display.width,
        height: device.display.height,
        dpi: device.display.dpi,
        touch: device.input.touch,
        pixel,
    })
}

/// Slot-4 copy-out backend. Returns OK after complete encoding.
/// Short buffers fail before any write; bytes beyond that count are untouched.
/// No frame, or a query-only frame, returns ERR_STATE.
///
/// # Safety
/// Non-null output must be writable for its capacity and disjoint from all host
/// state and live input borrows. It may be uninitialized. Exclusive write access
/// must last throughout this synchronous call. Null with zero capacity is valid.
pub(crate) unsafe fn snapshot(slot: &FrameSlot, out: PpBuffer) -> i32 {
    if out.cap > snapshot_wire::SNAPSHOT_MAX_BYTES {
        return ERR_NO_SPACE;
    }
    // SAFETY: the caller guarantees exclusive, live storage. The helper checks
    // null/length bounds and creates MaybeUninit output without reading bytes.
    let out = match unsafe { buffers::buffer_out(out, snapshot_wire::SNAPSHOT_MAX_BYTES) } {
        Ok(out) => out,
        Err(error) => return error,
    };
    slot.with(|frame| {
        frame
            .snapshot()
            .and_then(|value| snapshot_wire::snapshot_encode(&value, out))
            .map(|_| OK)
            .unwrap_or_else(|error| error)
    })
    .unwrap_or(ERR_STATE)
}
