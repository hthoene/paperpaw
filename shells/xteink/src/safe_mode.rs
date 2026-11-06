//! Safe mode (ADR 0024): starts that load code plugins and never reach
//! their first frame are counted in RTC memory, which a reset keeps (a
//! panic, a watchdog); after [`FAILED_STARTS`] of them PaperPaw starts
//! without code plugins, and so it does when the reader holds *Back* (the
//! X4, the X4 Classic) or *Home* (the X4 Pro) at power-on. The kernel then switches off
//! the plugin the last crash was in, or every code plugin when that is not
//! known, and the plugin manager says so ([`paperpaw_sdk::native::SafeMode`]).
//!
//! What is kept: how many starts failed, the code plugin loaded last, and
//! the one a panic's calls were in (`crash` finds it). Cutting the power
//! clears it all, as a start after that is a new one.

use alloc::string::String;

use paperpaw_sdk::native::{SafeMode, SafeModeReason};

/// Failed starts with code plugins before one goes without them.
pub const FAILED_STARTS: u8 = 2;
/// The key held at power-on for a start without code plugins: *Back* on
/// the devices with the X4's buttons, *Home* on the X4 Pro.
#[cfg(not(feature = "x4-pro"))]
pub const KEY: &str = paperpaw_platform::buttons::BACK;
#[cfg(feature = "x4-pro")]
pub const KEY: &str = paperpaw_platform::buttons::HOME;

/// The longest plugin id kept (the catalogue's ids have at most 48 bytes).
const ID_LEN: usize = 48;
/// Says the record was written by this firmware, not left by chance.
const MAGIC: u32 = 0x5050_5347;

/// What safe mode keeps through a reset.
#[derive(Clone, Copy)]
#[repr(C)]
struct Record {
    magic: u32,
    /// Starts with code plugins that did not reach their first frame.
    failed: u8,
    /// Not 0: this start loads code plugins (counted once).
    counted: u8,
    /// The code plugin loaded last, and the one a panic was in.
    loaded: [u8; ID_LEN],
    crashed: [u8; ID_LEN],
    /// The interface-7 plugin whose code is running now (set before each
    /// call into it, cleared after): left set by a reset, it is the plugin
    /// the reset happened in, a panic, a watchdog or a fault, whenever in
    /// the session it was.
    inflight: [u8; ID_LEN],
}

// SAFETY: numbers and bytes only, any pattern of bits one of them (a
// record left by chance is told by its magic).
unsafe impl esp_hal::Persistable for Record {}

const EMPTY: Record = Record {
    magic: MAGIC,
    failed: 0,
    counted: 0,
    loaded: [0; ID_LEN],
    crashed: [0; ID_LEN],
    inflight: [0; ID_LEN],
};

#[esp_hal::ram(unstable(rtc_fast, persistent))]
static mut RECORD: Record = EMPTY;

fn read() -> Record {
    // SAFETY: one core; written only by this module, at start, by the
    // loader and in the panic handler, never at once.
    let record = unsafe { core::ptr::addr_of!(RECORD).read_volatile() };
    if record.magic == MAGIC {
        record
    } else {
        // Power was cut: RTC memory holds noise.
        EMPTY
    }
}

fn write(record: Record) {
    // SAFETY: as in `read`.
    unsafe { core::ptr::addr_of_mut!(RECORD).write_volatile(record) };
}

fn text(bytes: &[u8; ID_LEN]) -> Option<String> {
    let len = bytes.iter().position(|&b| b == 0).unwrap_or(ID_LEN);
    let id = core::str::from_utf8(&bytes[..len]).ok()?;
    (!id.is_empty()).then(|| String::from(id))
}

fn set(field: &mut [u8; ID_LEN], id: &str) {
    *field = [0; ID_LEN];
    let len = id.len().min(ID_LEN);
    field[..len].copy_from_slice(&id.as_bytes()[..len]);
}

/// At the start: whether this one goes without code plugins, and why
/// (`key_held`: the reader holds [`KEY`]). A start in safe mode counts
/// anew from it.
pub fn check(key_held: bool) -> Option<SafeMode> {
    let mut record = read();
    record.counted = 0;
    // A reset in the middle of a call into a plugin (cutting the power clears
    // this record, so it was not that): that plugin is switched off now, not
    // only after the starts that did not reach the first frame, because a
    // plugin that crashes on its first event after the first frame is shown
    // would otherwise crash every start.
    let inflight = text(&record.inflight);
    let reason = if key_held {
        SafeModeReason::KeyHeld
    } else if record.failed >= FAILED_STARTS || inflight.is_some() {
        SafeModeReason::FailedStarts
    } else {
        write(record);
        return None;
    };
    let mut safe_mode = SafeMode::new(reason);
    safe_mode.last_loaded = text(&record.loaded);
    safe_mode.crashed = text(&record.crashed).or(inflight);
    write(EMPTY);
    Some(safe_mode)
}

/// The code of plugin `id` is being loaded: the first at a start counts
/// it as one that may fail.
pub fn loading(id: &str) {
    let mut record = read();
    if record.counted == 0 {
        record.counted = 1;
        record.failed = record.failed.saturating_add(1);
        record.crashed = [0; ID_LEN];
    }
    set(&mut record.loaded, id);
    write(record);
}

/// The first frame is shown: the start with its code plugins went well.
pub fn started() {
    write(EMPTY);
}

/// The panic going on was in the code of plugin `id` (from the panic
/// handler: nothing is allocated).
pub fn crashed_in(id: &str) {
    let mut record = read();
    set(&mut record.crashed, id);
    write(record);
}

/// Plugin `id`'s code is about to run (`make`, a callback, its destructor):
/// the id is kept where a reset leaves it until [`returned`].
pub fn calling(id: &str) {
    let mut record = read();
    set(&mut record.inflight, id);
    write(record);
}

/// The plugin's code returned.
pub fn returned() {
    let mut record = read();
    record.inflight = [0; ID_LEN];
    write(record);
}
