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

#[cfg(feature = "ppn5")]
use paperpaw_core::native_host::shell::inflight::Inflight;
use paperpaw_sdk::native::{SafeMode, SafeModeReason};

/// Failed starts with code plugins before one goes without them
/// (`paperpaw_x4::start_count`, where the rules are tested).
#[allow(dead_code, reason = "named for the docs; the rules read it")]
pub const FAILED_STARTS: u8 = paperpaw_x4::start_count::FAILED_STARTS;
/// The key held at power-on for a start without code plugins: *Back* on
/// the devices with the X4's buttons, *Home* on the X4 Pro.
#[cfg(not(feature = "x4-pro"))]
pub const KEY: &str = paperpaw_platform::buttons::BACK;
#[cfg(feature = "x4-pro")]
pub const KEY: &str = paperpaw_platform::buttons::HOME;

/// The longest plugin id kept (the catalogue's ids have at most 48 bytes).
const ID_LEN: usize = 48;
/// Says the record was written by this firmware, not left by chance.
#[cfg(not(feature = "ppn5"))]
const MAGIC: u32 = 0x5050_5346;
/// The record has the plugin calls in it, with a count per depth (another
/// layout, another magic).
#[cfg(feature = "ppn5")]
const MAGIC: u32 = 0x5050_534a;

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
    /// The interface-7 plugins whose code is running now, one per nested
    /// call (set before each call into one, given back after): left set by a
    /// reset, the innermost is the plugin the reset happened in, a panic, a
    /// watchdog or a fault, whenever in the session it was.
    #[cfg(feature = "ppn5")]
    inflight: Inflight,
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
    #[cfg(feature = "ppn5")]
    inflight: Inflight::EMPTY,
};

#[esp_hal::ram(unstable(rtc_fast, persistent))]
static mut RECORD: Record = EMPTY;

/// The starts that did not reach their first frame, counted from the first
/// instruction (`paperpaw_x4::start_count::early`): the record above counts
/// from the loading of native plugins, too late for a start that dies before.
#[esp_hal::ram(unstable(rtc_fast, persistent))]
static mut EARLY: u32 = 0;

/// What [`begin`] read: the starts before this one that failed early.
static EARLIER: core::sync::atomic::AtomicU8 = core::sync::atomic::AtomicU8::new(0);

/// The first thing of a start, before the stack's band, the board or the
/// card: counts this start as one that may fail, and says how many before it
/// did. Where that is [`FAILED_STARTS`] or more, this start goes without the
/// band and without native plugins (`main`, [`check`]).
pub fn begin() -> u8 {
    // SAFETY: one core, before anything else runs; written here, at the
    // first frame (`started`) and by `check`, never at once.
    let word = unsafe { core::ptr::addr_of!(EARLY).read_volatile() };
    let earlier = paperpaw_x4::start_count::early::previous(word);
    // SAFETY: as above.
    unsafe {
        core::ptr::addr_of_mut!(EARLY).write_volatile(paperpaw_x4::start_count::early::next(word));
    }
    EARLIER.store(earlier, core::sync::atomic::Ordering::Relaxed);
    earlier
}

// The record is copied whole; one copy of that code, not one per caller.
#[cfg_attr(feature = "ppn5", inline(never))]
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

#[cfg_attr(feature = "ppn5", inline(never))]
fn write(record: Record) {
    // SAFETY: as in `read`.
    unsafe { core::ptr::addr_of_mut!(RECORD).write_volatile(record) };
}

fn text(bytes: &[u8; ID_LEN]) -> Option<String> {
    let len = bytes.iter().position(|&b| b == 0).unwrap_or(ID_LEN);
    let id = core::str::from_utf8(&bytes[..len]).ok()?;
    (!id.is_empty()).then(|| String::from(id))
}

/// The plugin a reset happened in, when it did in one.
#[cfg(feature = "ppn5")]
fn running(record: &Record) -> Option<String> {
    record.inflight.current().map(String::from)
}

#[cfg(not(feature = "ppn5"))]
fn running(_: &Record) -> Option<String> {
    None
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
    let earlier = EARLIER.load(core::sync::atomic::Ordering::Relaxed);
    let early = paperpaw_x4::start_count::early::refuses(earlier);
    let mut count = paperpaw_x4::start_count::Count {
        // Starts that died before the loading of native plugins count too.
        failed: record.failed.max(if early { FAILED_STARTS } else { 0 }),
        counted: false,
    };
    record.counted = 0;
    // A reset in the middle of a call into a plugin (cutting the power clears
    // this record, so it was not that): that plugin is switched off now, not
    // only after the starts that did not reach the first frame, because a
    // plugin that crashes on its first event after the first frame is shown
    // would otherwise crash every start.
    let inflight = running(&record);
    let reason = match count.decide(key_held, inflight.is_some()) {
        Some(paperpaw_x4::start_count::Refusal::KeyHeld) => SafeModeReason::KeyHeld,
        Some(paperpaw_x4::start_count::Refusal::FailedStarts) => SafeModeReason::FailedStarts,
        None => {
            write(record);
            return None;
        }
    };
    if early {
        // This start counts as the first anew (it goes without the band).
        // SAFETY: as in `begin`.
        unsafe {
            core::ptr::addr_of_mut!(EARLY).write_volatile(paperpaw_x4::start_count::early::anew());
        }
    }
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
    let mut count = paperpaw_x4::start_count::Count {
        failed: record.failed,
        counted: record.counted != 0,
    };
    if count.loading() {
        record.counted = 1;
        record.failed = count.failed;
        record.crashed = [0; ID_LEN];
    }
    set(&mut record.loaded, id);
    write(record);
}

/// The stack's band was lost while native plugins run (a re-arm after a sleep
/// failed): written before the restart that follows, so that the next start
/// goes without native plugins (`paperpaw_x4::start_count::Count::lost_the_band`)
/// instead of hosting them again at once.
pub fn band_lost() {
    let mut record = read();
    let mut count = paperpaw_x4::start_count::Count {
        failed: record.failed,
        counted: record.counted != 0,
    };
    count.lost_the_band();
    record.failed = count.failed;
    write(record);
}

/// The first frame is shown: the start with its code plugins went well.
pub fn started() {
    write(EMPTY);
    // SAFETY: as in `begin`.
    unsafe {
        core::ptr::addr_of_mut!(EARLY).write_volatile(paperpaw_x4::start_count::early::CLEARED);
    }
}

/// The panic going on was in the code of plugin `id` (from the panic
/// handler: nothing is allocated).
pub fn crashed_in(id: &str) {
    let mut record = read();
    set(&mut record.crashed, id);
    write(record);
}

/// Plugin `id`'s code is about to run (`make`, a callback, its destructor):
/// the id is kept where a reset leaves it until [`returned`]. A call inside
/// another plugin's call is kept apart from it.
#[cfg(feature = "ppn5")]
pub fn calling(id: &str) {
    with_inflight(|inflight| inflight.enter(id));
}

/// The innermost call returned: the plugin around it, if any, is the running
/// one again.
#[cfg(feature = "ppn5")]
pub fn returned() {
    with_inflight(Inflight::leave);
}

/// Changes the calls in place: this runs around every call into a plugin,
/// including the nested ones, so it must not copy the whole record (about
/// 300 B of stack each) the way `read`/`write` do.
#[cfg(feature = "ppn5")]
fn with_inflight(change: impl FnOnce(&mut Inflight)) {
    // SAFETY: as in `read`: one core, written only by this module, at start,
    // by the loader and in the panic handler, never at once. The reference
    // lives only through `change`, which touches nothing else.
    unsafe {
        let record = core::ptr::addr_of_mut!(RECORD);
        if core::ptr::addr_of!((*record).magic).read_volatile() != MAGIC {
            // Power was cut: RTC memory holds noise.
            reset();
        }
        change(&mut *core::ptr::addr_of_mut!((*record).inflight));
    }
}

/// Writes the empty record (a constant, no copy on the caller's stack).
#[cfg(feature = "ppn5")]
#[inline(never)]
fn reset() {
    // SAFETY: as in `read`.
    unsafe { core::ptr::addr_of_mut!(RECORD).write_volatile(EMPTY) };
}

/// What the escape's watch reads in the button interrupt: the record of calls
/// while a call is open (see [`Running`]). One volatile word each, no copy of
/// the record. The same numbers twice are the same call.
#[cfg(feature = "ppn5")]
pub fn running_call() -> Option<paperpaw_x4::escape::Running> {
    // The watch counts as many depths as the record keeps.
    const _: () = assert!(
        paperpaw_x4::escape::DEPTHS == paperpaw_core::native_host::shell::inflight::NESTING
    );
    // SAFETY: as in `read`; the interrupt only loads the words of a record that
    // the main loop may be changing, through a raw pointer, and the main loop
    // stores them with matching volatile stores.
    unsafe {
        let record = core::ptr::addr_of!(RECORD);
        if core::ptr::addr_of!((*record).magic).read_volatile() != MAGIC {
            return None;
        }
        Inflight::watch_at(core::ptr::addr_of!((*record).inflight)).map(|w| {
            paperpaw_x4::escape::Running {
                call: w.call,
                open: w.open,
                host: w.host,
                entries: w.entries,
            }
        })
    }
}

/// A plugin's call into the host's code begins (a host slot): the plugin
/// waits for the host, so the escape does not blame it meanwhile.
#[cfg(feature = "ppn5")]
pub fn host_begin() {
    with_inflight(Inflight::host_enter);
}

/// The host slot returned.
#[cfg(feature = "ppn5")]
pub fn host_end() {
    with_inflight(Inflight::host_leave);
}

/// The escape resets the processor now (from the button interrupt): with a
/// plugin to blame, its call stays recorded and the next start switches that
/// plugin off (`check`): the calls down to the blamed depth are kept, the
/// deeper ones (which came and went, or are the innermost) are not. Without a
/// plugin to blame the record of calls is cleared.
#[cfg(feature = "ppn5")]
pub fn escape_reset(blame: paperpaw_x4::escape::Blame) {
    use paperpaw_x4::escape::Blame;
    match blame {
        Blame::Depth(depth) => with_inflight(|inflight| inflight.keep_depth(depth)),
        Blame::Nobody => with_inflight(|inflight| *inflight = Inflight::EMPTY),
    }
}
