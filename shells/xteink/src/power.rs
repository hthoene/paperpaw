//! The processor's sleep while there is nothing to do: on battery it
//! sleeps lightly (some 0.1 mA, where spinning at full speed takes some
//! 20) until something is due, and reads the buttons on waking, since the
//! timer that reads them stops meanwhile. On USB it stays awake: a light
//! sleep cuts the link to the computer (its log, remote control).
//!
//! Two watchdogs start the X4 again if the firmware stops coming by: the
//! RTC's, which counts on in light sleep too, and a timer group's. Long
//! work on the card keeps them fed for a while (see [`working`]).

use core::cell::{Cell, RefCell};
use core::sync::atomic::{AtomicBool, Ordering};

use critical_section::Mutex;
use esp_hal::rtc_cntl::sleep::{LowPower, RtcSleepConfig};
use esp_hal::time::{Duration, Instant};

static LOW_POWER: Mutex<RefCell<Option<LowPower<'static>>>> = Mutex::new(RefCell::new(None));

/// Where the firmware was last (see [`mark`]), kept through a reset: after
/// a watchdog's reset it tells where it hung.
#[esp_hal::ram(unstable(rtc_fast, persistent))]
static mut WHERE: u32 = 0;

/// Light sleeps, kept through a reset (for trying them over USB, which
/// they cut off).
#[esp_hal::ram(unstable(rtc_fast, persistent))]
static mut NAPS: u32 = 0;

/// What [`WHERE`] and [`NAPS`] said at start-up, with the reason for the
/// reset.
static AT_START: Mutex<RefCell<(u32, u32, Option<esp_hal::rtc_cntl::SocResetReason>)>> =
    Mutex::new(RefCell::new((0, 0, None)));

/// Places the firmware notes being at (see [`mark`]).
pub mod at {
    pub const LOOP: u32 = 1;
    pub const NAP_IDLE: u32 = 2;
    pub const WOKEN: u32 = 4;
    pub const BUTTONS_READ: u32 = 5;
    pub const INPUT: u32 = 6;
    pub const TICK: u32 = 7;
    pub const SHOWING: u32 = 8;
    pub const STATUS: u32 = 9;
    /// A try of light sleeps over USB ended (by starting again).
    #[cfg(feature = "remote")]
    pub const TRIED: u32 = 10;
    /// In a long light sleep tried over USB.
    #[cfg(feature = "remote")]
    pub const TRIED_NAP: u32 = 11;
    /// Starting the radio: its scheduler, its driver, as a station.
    pub const RADIO_SCHEDULER: u32 = 20;
    pub const RADIO_DRIVER: u32 = 21;
    pub const RADIO_STATION: u32 = 22;
    pub const RADIO_STARTED: u32 = 23;
    pub const RADIO_SCAN: u32 = 24;
    pub const RADIO_CONNECT: u32 = 25;
}

/// Notes being at `place` (one of [`at`]).
pub fn mark(place: u32) {
    // SAFETY: one core, written only here and read once at start-up.
    unsafe { core::ptr::addr_of_mut!(WHERE).write_volatile(place) };
}

/// Notes how the last run ended; call once at start-up.
pub fn note_start() {
    // SAFETY: as in `mark`.
    let place = unsafe { core::ptr::addr_of!(WHERE).read_volatile() };
    // SAFETY: as in `mark`.
    let naps = unsafe { core::ptr::addr_of!(NAPS).read_volatile() };
    unsafe { core::ptr::addr_of_mut!(NAPS).write_volatile(0) };
    let reason = esp_hal::system::reset_reason();
    critical_section::with(|cs| AT_START.replace(cs, (place, naps, reason)));
}

/// Where the last run was when it ended, its light sleeps and the reason
/// for the reset.
pub fn last_run() -> (u32, u32, Option<esp_hal::rtc_cntl::SocResetReason>) {
    critical_section::with(|cs| *AT_START.borrow_ref(cs))
}

fn count_nap() {
    // SAFETY: as in `mark`.
    unsafe {
        let naps = core::ptr::addr_of_mut!(NAPS);
        naps.write_volatile(naps.read_volatile().wrapping_add(1));
    }
}

/// Completed light sleeps in this run, for hardware diagnostics.
#[cfg(feature = "remote")]
pub fn naps() -> u32 {
    // SAFETY: only the main loop writes NAPS, and calls this function.
    unsafe { core::ptr::addr_of!(NAPS).read_volatile() }
}

/// Light sleeps are allowed (on battery).
static ALLOWED: AtomicBool = AtomicBool::new(false);

/// Waits shorter than this spin: sleeping takes its own time to start and
/// end.
const MIN_NAP_US: u64 = 3000;

pub fn init(low_power: LowPower<'static>) {
    critical_section::with(|cs| LOW_POWER.replace(cs, Some(low_power)));
}

/// Allows light sleeps (on battery) or not (on USB).
pub fn allow(allowed: bool) {
    ALLOWED.store(allowed, Ordering::Relaxed);
}

/// Sleeps lightly until `until` (milliseconds since start) where allowed,
/// then reads the buttons; `false` if it did not (the caller waits).
pub fn nap_until(until: u64) -> bool {
    let deadline = Instant::EPOCH + Duration::from_millis(until);
    mark(at::NAP_IDLE);
    nap(deadline)
}

/// Sleeps lightly until `deadline` where allowed.
fn nap(deadline: Instant) -> bool {
    if !ALLOWED.load(Ordering::Relaxed) {
        return false;
    }
    sleep_until(deadline)
}

/// Sleeps lightly until `deadline`, if it is far enough; whether it did.
fn sleep_until(deadline: Instant) -> bool {
    feed();
    if deadline <= Instant::now() + Duration::from_micros(MIN_NAP_US) {
        return false;
    }
    // Taken out for the sleep: the interrupt that reads the buttons must
    // find the lock free.
    let Some(mut low_power) = critical_section::with(|cs| LOW_POWER.borrow_ref_mut(cs).take())
    else {
        return false;
    };
    low_power.set_wakeup_deadline(deadline);
    // The X4's ADC interrupt and sleep entry both use the internal
    // analog bus. Keep the sampler out until the HAL has restored the
    // clocks and regulators. The RTC alarm wakes the hardware without
    // a CPU interrupt; sample_now reads the buttons after waking.
    #[cfg(feature = "x4")]
    critical_section::with(|_| low_power.sleep_light(RtcSleepConfig::default()));
    #[cfg(not(feature = "x4"))]
    low_power.sleep_light(RtcSleepConfig::default());
    critical_section::with(|cs| LOW_POWER.replace(cs, Some(low_power)));
    mark(at::WOKEN);
    count_nap();
    crate::input::sample_now();
    mark(at::BUTTONS_READ);
    true
}

/// Light sleep for a reader put to sleep: until `until_ms` (milliseconds
/// since start) or until the power button goes down, with the readings
/// paused ([`crate::input::pause`]); whether the button is down at waking.
/// The watchdogs are off meanwhile ([`watchdogs`]).
#[cfg(feature = "esp32c3")]
pub fn sleep_asleep(until_ms: u64) -> bool {
    let Some(mut low_power) = critical_section::with(|cs| LOW_POWER.borrow_ref_mut(cs).take())
    else {
        return false;
    };
    low_power.set_wakeup_deadline(Instant::EPOCH + Duration::from_millis(until_ms));
    mark(at::NAP_IDLE);
    let down = crate::input::power_nap(|| low_power.sleep_light(RtcSleepConfig::default()));
    low_power.clear_wakeup_deadline();
    critical_section::with(|cs| LOW_POWER.replace(cs, Some(low_power)));
    mark(at::WOKEN);
    count_nap();
    down
}

/// The watchdogs for a light sleep that lasts minutes: the timer group's
/// stops (its seconds are far beyond a sleeping reader), the RTC's is armed
/// for `ms`, longer than the longest nap, and fed before each. A sleep that
/// never ends is then a reset after those minutes, not a reader awake with
/// nobody looking until the battery is flat. [`watchdogs_awake`] puts them
/// back, on every way out of the sleep.
#[cfg(feature = "esp32c3")]
pub fn watchdogs_for_sleep(ms: u64) {
    critical_section::with(|cs| {
        if let Some((rtc, wdt)) = WATCHDOGS.borrow_ref_mut(cs).as_mut() {
            wdt.disable();
            arm_ms(&mut rtc.rwdt, ms);
            rtc.rwdt.feed();
        }
        CAME_BY.borrow(cs).set(Instant::now());
    });
}

/// Both watchdogs as they are awake, with their seconds, fed.
#[cfg(feature = "esp32c3")]
pub fn watchdogs_awake() {
    critical_section::with(|cs| {
        if let Some((rtc, wdt)) = WATCHDOGS.borrow_ref_mut(cs).as_mut() {
            arm_ms(&mut rtc.rwdt, AWAKE_RTC_S.borrow(cs).get() * 1000);
            wdt.enable();
            rtc.rwdt.feed();
            wdt.feed();
        }
        CAME_BY.borrow(cs).set(Instant::now());
    });
}

/// Turns the power off; only the power button wakes the X4 (it starts
/// again).
pub fn deep() -> ! {
    let low_power = critical_section::with(|cs| LOW_POWER.borrow_ref_mut(cs).take());
    let Some(mut low_power) = low_power else {
        panic!("no low power control");
    };
    // No alarm of a light sleep may end it, nor a watchdog.
    low_power.clear_wakeup_deadline();
    critical_section::with(|cs| {
        if let Some((rtc, wdt)) = WATCHDOGS.borrow_ref_mut(cs).as_mut() {
            rtc.rwdt.disable();
            wdt.disable();
        }
    });
    low_power.sleep_deep(RtcSleepConfig::deep())
}

type TimerWatchdog = esp_hal::timer::timg::Wdt<esp_hal::peripherals::TIMG1<'static>>;

static WATCHDOGS: Mutex<RefCell<Option<(esp_hal::rtc_cntl::Rtc<'static>, TimerWatchdog)>>> =
    Mutex::new(RefCell::new(None));

/// Starts the X4 again if its start hangs (a part that never answers)
/// for `rtc_s` seconds, rather than keep it on: the RTC's watchdog, armed
/// before the board is set up; [`watch`] takes it over.
pub fn watch_start(rtc_s: u64) {
    // SAFETY: only the RTC's watchdog is used, and only until the board
    // hands the RTC to `watch`, which sets it up again.
    let rtc_timer = unsafe { esp_hal::peripherals::RTC_TIMER::steal() };
    arm(&mut esp_hal::rtc_cntl::Rtc::new(rtc_timer).rwdt, rtc_s);
}

/// Sets the RTC's watchdog to reset the X4 after `seconds`.
fn arm(rwdt: &mut esp_hal::rtc_cntl::Rwdt, seconds: u64) {
    arm_ms(rwdt, seconds * 1000);
}

/// Sets the RTC's watchdog to reset the X4 after `ms`.
fn arm_ms(rwdt: &mut esp_hal::rtc_cntl::Rwdt, ms: u64) {
    use esp_hal::rtc_cntl::{RwdtStage, RwdtStageAction};
    rwdt.set_stage_action(RwdtStage::Stage0, RwdtStageAction::ResetSystem);
    rwdt.set_timeout(RwdtStage::Stage0, Duration::from_millis(ms));
    rwdt.enable();
}

/// Starts the X4 again if the firmware does not come by: after `rtc_s`
/// seconds by the RTC's watchdog, which counts on in light sleep (a sleep
/// that does not end), after `timer_s` seconds by the timer group's. Hung,
/// it would stay on (and on battery drain it) with nothing to be done.
pub fn watch(
    mut rtc: esp_hal::rtc_cntl::Rtc<'static>,
    mut wdt: TimerWatchdog,
    (rtc_s, timer_s): (u64, u64),
) {
    arm(&mut rtc.rwdt, rtc_s);
    // It counts on in light sleep too (esp-hal pauses it there): a sleep
    // that does not end is what it is for.
    let regs = esp_hal::peripherals::LPWR::regs();
    // SAFETY: the key that unlocks the watchdog's registers, then locks.
    regs.wdtwprotect().write(|w| unsafe { w.bits(0x50D8_3AA1) });
    regs.wdtconfig0()
        .modify(|_, w| w.wdt_pause_in_slp().clear_bit());
    regs.wdtwprotect().write(|w| unsafe { w.bits(0) });
    wdt.set_timeout(
        esp_hal::timer::timg::MwdtStage::Stage0,
        Duration::from_secs(timer_s),
    );
    wdt.enable();
    critical_section::with(|cs| {
        CAME_BY.borrow(cs).set(Instant::now());
        AWAKE_RTC_S.borrow(cs).set(rtc_s);
        WATCHDOGS.replace(cs, Some((rtc, wdt)));
    });
}

/// The RTC watchdog's seconds when awake (see [`watch`]), to put back after
/// a sleep.
static AWAKE_RTC_S: Mutex<Cell<u64>> = Mutex::new(Cell::new(0));

/// How long work in one step of the main loop may keep the watchdogs fed
/// (see [`working`]): removing a large folder takes minutes (the books'
/// caches over one; x4-remote waits 15 for it). A loop that never ends
/// (e.g. on a FAT chain that leads back into itself) is caught after it.
const WORK_S: u64 = 15 * 60;

/// When the main loop last came by (see [`feed`]).
static CAME_BY: Mutex<Cell<Instant>> = Mutex::new(Cell::new(Instant::EPOCH));

/// Tells the watchdogs the firmware is alive: the main loop came by.
pub fn feed() {
    critical_section::with(|cs| {
        CAME_BY.borrow(cs).set(Instant::now());
        feed_watchdogs(cs);
    });
}

/// Tells the watchdogs long work goes on (a block of the card read or
/// written), for at most [`WORK_S`] after the main loop last came by:
/// legitimate work (a folder removed or copied, a large file written)
/// does not look hung, a loop that never ends still does.
pub fn working() {
    critical_section::with(|cs| {
        if CAME_BY.borrow(cs).get().elapsed() < Duration::from_secs(WORK_S) {
            feed_watchdogs(cs);
        }
    });
}

fn feed_watchdogs(cs: critical_section::CriticalSection<'_>) {
    if let Some((rtc, wdt)) = WATCHDOGS.borrow_ref_mut(cs).as_mut() {
        rtc.rwdt.feed();
        wdt.feed();
    }
}

#[cfg(feature = "remote")]
/// One light sleep of `seconds` whatever the rest, then a new start: for
/// trying over USB that the RTC's watchdog ends a sleep that goes on too
/// long (it starts the X4 again before).
pub fn try_long_nap(seconds: u64) -> ! {
    mark(at::TRIED_NAP);
    sleep_until(Instant::now() + Duration::from_secs(seconds));
    mark(at::TRIED);
    esp_hal::system::software_reset()
}
