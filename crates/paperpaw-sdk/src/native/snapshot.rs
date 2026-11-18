//! Current device and battery values copied through the host's snapshot slot.
use super::{Error, Host, binding::status, wire::snapshot_wire as wire};
use crate::Instant;
use paperpaw_native::host_glue::PpBuffer;

/// An owned snapshot. It retains no host or callback borrows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Snapshot {
    now: Instant,
    unix_seconds: Option<i64>,
    percent: Option<u8>,
    millivolts: Option<u16>,
    charging: bool,
    power_saving: bool,
    width: u16,
    height: u16,
    dpi: u16,
    touch: bool,
    gray_levels: Option<u16>,
    colour: bool,
    utc_offset_minutes: Option<i16>,
}

impl Snapshot {
    /// The snapshot's monotonic millisecond clock.
    #[must_use]
    pub const fn instant(&self) -> Instant {
        self.now
    }

    /// Unix seconds, if known, preserving the wire's full signed range.
    /// Call [`crate::time::UnixTime::from_secs`] to convert to the SDK time type.
    #[must_use]
    pub const fn unix_seconds(&self) -> Option<i64> {
        self.unix_seconds
    }

    /// Battery percentage, including a measured zero; None means unavailable.
    #[must_use]
    pub const fn battery_percent(&self) -> Option<u8> {
        self.percent
    }

    /// Battery voltage in millivolts; a measured zero remains Some(0).
    #[must_use]
    pub const fn battery_millivolts(&self) -> Option<u16> {
        self.millivolts
    }

    /// Whether the battery is charging; false when no battery is reported.
    #[must_use]
    pub const fn is_charging(&self) -> bool {
        self.charging
    }

    /// Whether battery saving mode is active; false when no battery is reported.
    #[must_use]
    pub const fn is_power_saving(&self) -> bool {
        self.power_saving
    }

    /// Display width in pixels.
    #[must_use]
    pub const fn display_width(&self) -> u16 {
        self.width
    }

    /// Display height in pixels.
    #[must_use]
    pub const fn display_height(&self) -> u16 {
        self.height
    }

    /// Display pixels per inch.
    #[must_use]
    pub const fn display_dpi(&self) -> u16 {
        self.dpi
    }

    /// Whether touch input is available.
    #[must_use]
    pub const fn has_touch(&self) -> bool {
        self.touch
    }

    /// Distinct gray levels the panel's pixel format holds (2, 4, 16 or 256);
    /// None for colour and shell-defined formats.
    #[must_use]
    pub const fn display_gray_levels(&self) -> Option<u16> {
        self.gray_levels
    }

    /// Whether the panel shows colour.
    #[must_use]
    pub const fn display_is_colour(&self) -> bool {
        self.colour
    }

    /// Minutes the user's time zone is ahead of UTC now (east of it,
    /// summer time included: `core.time-zone`, else the system's), -720
    /// to 840; None while the device knows no time. Local time is
    /// `unix_seconds + 60 * offset`, e.g. for a day's boundary.
    #[must_use]
    pub const fn utc_offset_minutes(&self) -> Option<i16> {
        self.utc_offset_minutes
    }
}

/// Capture the current snapshot with one fixed-size, allocation-free host call.
///
/// # Errors
/// Returns the host refusal or a generated decoder error for malformed data.
pub fn capture_snapshot(host: &Host<'_>) -> Result<Snapshot, Error> {
    let mut bytes = [0; wire::SNAPSHOT_MAX_BYTES];
    // SAFETY: entry admission retains table/context; output is disjoint local storage.
    status(unsafe {
        (host.table.env_snapshot)(
            host.ctx,
            PpBuffer {
                ptr: bytes.as_mut_ptr(),
                cap: bytes.len(),
            },
        )
    })?;
    // The snapshot contract puts total_bytes in the four bytes at offset 4.
    // Only this header value is read before the generated decoder validates it.
    let total = usize::try_from(u32::from_le_bytes(
        bytes[4..8].try_into().map_err(|_| Error::Invalid)?,
    ))
    .map_err(|_| Error::Invalid)?;
    if !(wire::SNAPSHOT_BYTES..=wire::SNAPSHOT_MAX_BYTES).contains(&total) {
        return Err(Error::Invalid);
    }
    let value = wire::snapshot_decode(&bytes[..total]).map_err(Error::from_status)?;
    Ok(Snapshot {
        now: Instant(value.monotonic_ms),
        unix_seconds: value.unix_seconds,
        percent: value.battery.map(|battery| battery.percent),
        millivolts: value.battery.and_then(|battery| battery.millivolts),
        charging: value.battery.is_some_and(|battery| battery.charging),
        power_saving: value.battery.is_some_and(|battery| battery.saver),
        width: value.width,
        height: value.height,
        dpi: value.dpi,
        touch: value.touch,
        gray_levels: value.pixel.gray_levels(),
        colour: matches!(
            value.pixel,
            wire::PixelFormat::Rgb565 | wire::PixelFormat::Rgb888
        ),
        utc_offset_minutes: value.utc_offset_minutes,
    })
}

#[cfg(test)]
#[path = "snapshot_tests.rs"]
mod tests;
