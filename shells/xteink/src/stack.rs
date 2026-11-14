//! The stack's depth. It lies above everything else in RAM and grows down
//! towards it: run over, it writes into the memory below (the radio's
//! firmware state lies last) with nothing to stop it, and something fails
//! later, somewhere else. So the stack is filled with a pattern at start,
//! which shows how deep it went ([`left`]), and the lowest part is checked
//! now and then ([`check`]): overwritten, the X4 starts again saying why.
//! That is after the fact: a band above the end is watched by the CPU's
//! own data trigger as well ([`arm_band`]), which fires in the store that
//! reaches it, in the code that overflowed, and ends in the crash slot
//! blaming it (`crash.rs`).
//!
//! The band costs stack: it lies `RESERVE` = 4 KiB above the end (the larger of
//! the panic handler's and an interrupt's room, for two frames of just under
//! the band's 2 KiB meeting there; `paperpaw_x4::stack_band`), is 2 KiB high, and is aligned: 6 to
//! 8 KiB of the main stack's 41,096 bytes, about 33 KiB left for the deepest
//! legitimate path (a device gate).
//!
//! How the handler's depth is read off the device (the reserve's `HANDLER` is
//! assumed 2 KiB until then; checklist G7): the stack is painted at start, and
//! the panic handler reads the lowest word that is not the paint at its very
//! end and ends its message with it, `[stack handler=NNNB left=NNNB]`
//! (`paperpaw_x4::stack_note`, built in the G7 change): how deep it went below
//! where it was entered, and what was left to the stack's end. The message is
//! kept through the reset in the crash slot, so nothing reads RAM after the
//! reset (the bootloader runs in it first). The overflow test (G8) trips the
//! band with a plugin whose frame is just under the limit, and with an
//! interrupt at the band; the same note answers how much was left.

/// What the unused stack holds.
const PAINT: u32 = 0xA5A5_5A5A;
/// Bytes at the stack's end checked by [`check`].
const BAND: usize = 256;
/// Bytes left unpainted below the stack pointer at start: in use then.
const MARGIN: usize = 1024;

unsafe extern "C" {
    /// The stack's lowest address (esp-hal's linker script).
    static _stack_end: u32;
    /// The stack's highest address, where it starts.
    static _stack_start: u32;
    /// esp-hal's guard word near the stack's end, watched for writes.
    static __stack_chk_guard: u32;
}

/// The lowest address painted: the word after esp-hal's guard.
fn bottom() -> usize {
    core::ptr::addr_of!(__stack_chk_guard) as usize + 4
}

fn pointer() -> usize {
    let sp: usize;
    // SAFETY: reads a register.
    #[cfg(target_arch = "riscv32")]
    unsafe {
        core::arch::asm!("mv {}, sp", out(reg) sp)
    };
    // On Xtensa (no stable inline assembly): where a local lies, a few
    // bytes from it.
    #[cfg(target_arch = "xtensa")]
    {
        let here = 0u8;
        sp = core::ptr::addr_of!(here) as usize;
    }
    sp
}

/// Fills the stack below what is in use with the pattern: once, first
/// thing at start.
pub fn paint() {
    let top = pointer() - MARGIN;
    let mut at = bottom();
    while at < top {
        // SAFETY: unused stack, below the stack pointer, above the guard.
        unsafe { (at as *mut u32).write_volatile(PAINT) };
        at += 4;
    }
}

/// Bytes of the stack never used since start (the depth left at its
/// deepest), and the stack's size.
pub fn left() -> (usize, usize) {
    let mut at = bottom();
    let sp = pointer();
    // SAFETY: within the stack, below the stack pointer: not in use.
    while at < sp && unsafe { (at as *const u32).read_volatile() } == PAINT {
        at += 4;
    }
    let end = core::ptr::addr_of!(_stack_end) as usize;
    let start = core::ptr::addr_of!(_stack_start) as usize;
    (at - end, start - end)
}

/// The stack pointer now: the panic handler's entry, for [`handler_use`].
pub fn sp() -> usize {
    pointer()
}

/// What the panic handler reports of the stack, read at its end
/// (`paperpaw_x4::stack_note`): `None` if it was entered on a stack that is
/// not the main one (a radio task's); otherwise the bytes never touched since
/// boot, and, after a band trap only (`breakpoint`: the exception was a
/// breakpoint; the pointer is at the band), the depth below `entry` (the
/// pointer where the handler was entered, [`sp`]) the handler went. The
/// painted words show it (`paint` ran at start), so it needs nothing kept
/// across the reset: the figures go into the panic's message in the crash
/// slot, and the device checklist (G7) reads the handler's real depth from
/// the message after a band trap (G8's overflow test).
pub fn handler_use(entry: usize, breakpoint: bool) -> Option<(Option<u32>, u32)> {
    let to_u32 = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
    let end = to_u32(core::ptr::addr_of!(_stack_end) as usize);
    let start = to_u32(core::ptr::addr_of!(_stack_start) as usize);
    let entry = to_u32(entry);
    if !paperpaw_x4::stack_note::in_main_stack(entry, end, start) {
        return None;
    }
    let (left, _) = left();
    let left = to_u32(left);
    #[cfg(not(target_arch = "xtensa"))]
    let trap = paperpaw_x4::stack_note::is_band_trap(breakpoint, entry, end, start);
    #[cfg(target_arch = "xtensa")]
    let trap = paperpaw_x4::stack_note::is_band_trap_s3(breakpoint, entry, end, start);
    Some((
        trap.then(|| paperpaw_x4::stack_note::handler_depth(entry, end, left)),
        left,
    ))
}

/// Starts the X4 again (by a panic, which keeps the message) if the stack
/// reached its end since start, or if the band at its end was lost after a
/// plugin with native code was loaded (a re-arm after a sleep found the
/// trigger taken, a debugger's perhaps): that plugin runs on with no net,
/// and the start that follows refuses it ([`band_armed`] is false then).
pub fn check() {
    use core::sync::atomic::Ordering;
    if LOST.load(Ordering::Relaxed) {
        lost_with_native_plugins();
    }
    let reached = (bottom()..bottom() + BAND)
        .step_by(4)
        // SAFETY: as in `left`; the stack is never this deep here.
        .any(|at| unsafe { (at as *const u32).read_volatile() } != PAINT);
    if reached {
        panic!("the stack ran over");
    }
}

/// The CPU's trigger the band uses. esp-hal's (and esp-rtos', on every
/// task switch) is 0, the guard word's; its vector table check, off in
/// this firmware, takes 1. A later esp-hal that takes 2 as well is found
/// at start ([`arm_band`] finds it busy) and by the lock-file test in
/// `paperpaw-x4`'s `stack_band`.
#[cfg(target_arch = "riscv32")]
const TRIGGER: u32 = 2;

/// Whether the band is armed now: the plugins of interface 7 run only
/// where it is (`native::load_file` refuses them otherwise).
static ARMED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// Set when a re-arm found the trigger taken after the band had been armed.
static LOST: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
/// Set when a plugin with native code was let through [`band_armed`]'s gate.
static NATIVE: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// Starts the X4 again if a plugin with native code is loaded and the band
/// is lost: it would run on with no net, and the start that follows refuses
/// it ([`band_armed`] is false then, so there is no loop).
fn lost_with_native_plugins() {
    if NATIVE.load(core::sync::atomic::Ordering::Relaxed) {
        // The start that follows goes without native plugins, not as a clean
        // one that hosts them again (a nap could end in a reset each time).
        crate::safe_mode::band_lost();
        panic!("the stack's band was taken while a native plugin runs: starting again without it");
    }
}

/// Notes that a plugin with native code is loaded, which needs the band for
/// as long as it runs (`check` ends the run if the band is lost).
#[cfg(feature = "ppn5")]
pub fn native_loaded() {
    NATIVE.store(true, core::sync::atomic::Ordering::Relaxed);
}

/// Whether the band at the stack's end is armed. Where it is not (the
/// chip has none, or trigger 2 was taken by something else), the plugins
/// that run native code do not run: they would have no net.
#[cfg_attr(
    not(any(target_arch = "riscv32", feature = "ppn5")),
    allow(
        dead_code,
        reason = "the Xtensa chips arm a band only with ppn5, which is its only caller"
    )
)]
pub fn band_armed() -> bool {
    ARMED.load(core::sync::atomic::Ordering::Relaxed)
}

/// Sets the CPU's data trigger to fire before any store into the band
/// at the stack's end (`paperpaw_x4::stack_band`), from now on: the
/// stack must be painted already ([`paint`] stores into it) and nothing
/// stores there after, but a call that runs over the stack's end. The
/// trigger raises a breakpoint exception, which esp-hal turns into a
/// panic at the faulting store (`crash.rs` blames the plugin whose code
/// it is, or the one that called the firmware's code it was in).
///
/// Called at start and again after every light sleep (the trigger is
/// the CPU's: a sleep may not keep it). Where the trigger is busy with
/// something else, nothing is written and [`band_armed`] says no. Never
/// a panic: a firmware that panicked at start for this would restart
/// for ever.
///
/// The ESP32-C3's only: the Xtensa chips have no native plugins.
#[cfg(target_arch = "riscv32")]
pub fn arm_band() {
    let end = core::ptr::addr_of!(_stack_end) as u32;
    let top = core::ptr::addr_of!(_stack_start) as u32;
    let Some(start) = paperpaw_x4::stack_band::band(end, top) else {
        ARMED.store(false, core::sync::atomic::Ordering::Relaxed);
        return;
    };
    let address = paperpaw_x4::stack_band::napot(start);
    let control = paperpaw_x4::stack_band::CONTROL;
    // Machine mode triggers on (bit 3).
    let enable = 1u32 << 3;
    // Interrupts off: a task switch (esp-rtos) selects its trigger in
    // between and the writes would land in that one.
    let armed = critical_section::with(|_| {
        let (have_control, have_address): (u32, u32);
        // SAFETY: the trigger CSRs of the chip; they touch no memory.
        unsafe {
            core::arch::asm!(
                "csrw 0x7a0, {id}",
                "csrr {c}, 0x7a1",
                "csrr {a}, 0x7a2",
                id = in(reg) TRIGGER,
                c = out(reg) have_control,
                a = out(reg) have_address,
                options(nomem, nostack),
            );
        }
        // Another's (a debugger's): nothing is written.
        if paperpaw_x4::stack_band::trigger(have_control, have_address, address)
            == paperpaw_x4::stack_band::Trigger::Taken
        {
            return false;
        }
        // SAFETY: as above. The trigger is written disabled, then its
        // address, then enabled; then read back.
        let (set_control, set_address): (u32, u32);
        unsafe {
            core::arch::asm!(
                "csrw 0x7a0, {id}",
                "csrw 0x7a5, {enable}",
                "csrw 0x7a1, zero",
                "csrw 0x7a2, {address}",
                "csrw 0x7a1, {control}",
                "csrr {c}, 0x7a1",
                "csrr {a}, 0x7a2",
                id = in(reg) TRIGGER,
                enable = in(reg) enable,
                address = in(reg) address,
                control = in(reg) control,
                c = out(reg) set_control,
                a = out(reg) set_address,
                options(nomem, nostack),
            );
        }
        // Armed is what the chip holds, not what was meant.
        paperpaw_x4::stack_band::holds(set_control, set_address, address)
    });
    let was = band_armed();
    ARMED.store(armed, core::sync::atomic::Ordering::Relaxed);
    if !armed && was {
        log::warn!("stack: trigger {TRIGGER} was taken: the band is no longer armed");
        LOST.store(true, core::sync::atomic::Ordering::Relaxed);
        // Here, where the loss is found: the resume after a sleep runs plugin
        // code before the main loop's next check does.
        lost_with_native_plugins();
    }
}

/// The ESP32-S3's band, the debug assistant's region monitor
/// (`paperpaw_x4::s3_band`, `abi/heap-word.md` section 6): the registers
/// behind the arming routine, the interrupt that ends in the crash slot, and
/// the store's address for the blame.
#[cfg(all(target_arch = "xtensa", feature = "ppn5"))]
mod s3 {
    use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

    use esp_hal::interrupt::bind_handler;
    use esp_hal::peripherals::{ASSIST_DEBUG, Interrupt};
    use paperpaw_x4::s3_band::Monitor;

    /// The core that runs plugin calls: the first. (The second core runs no
    /// PaperPaw code; if it ever does, its monitor is armed the same way.)
    const CORE: usize = 0;

    /// The address of the store that reached the band, kept for the crash
    /// slot's blame (0 before one did).
    static HIT: AtomicU32 = AtomicU32::new(0);
    /// Set once the interrupt's handler is bound.
    static BOUND: AtomicBool = AtomicBool::new(false);
    /// Set around the start's delivery test: the handler then records that it
    /// ran and returns, instead of panicking.
    static SELF_TEST: AtomicBool = AtomicBool::new(false);
    /// Set by the handler during the delivery test.
    static DELIVERED: AtomicBool = AtomicBool::new(false);

    /// The registers of the first core's DRAM0 area 0.
    pub struct Regs(ASSIST_DEBUG<'static>);

    impl Regs {
        pub fn new() -> Self {
            // SAFETY: the debug assistant is not given to anything else in
            // this firmware (esp-hal's driver of it is not used); the arming
            // routine and the band's handler are its only users, never at once
            // (the handler stops the monitor first).
            Self(unsafe { ASSIST_DEBUG::steal() })
        }

        /// The program counter of the store the monitor saw last.
        pub fn store_pc(&self) -> u32 {
            self.0
                .register_block()
                .cpu(CORE)
                .area_pc()
                .read()
                .area_pc()
                .bits()
        }
    }

    impl Monitor for Regs {
        fn set_bounds(&mut self, min: u32, max: u32) {
            let cpu = self.0.register_block().cpu(CORE);
            cpu.area_dram0_0_min()
                .write(|w| unsafe { w.area_dram0_0_min().bits(min) });
            cpu.area_dram0_0_max()
                .write(|w| unsafe { w.area_dram0_0_max().bits(max) });
        }
        fn bounds(&self) -> (u32, u32) {
            let cpu = self.0.register_block().cpu(CORE);
            (
                cpu.area_dram0_0_min().read().area_dram0_0_min().bits(),
                cpu.area_dram0_0_max().read().area_dram0_0_max().bits(),
            )
        }
        fn set_write(&mut self, on: bool) {
            self.0
                .register_block()
                .cpu(CORE)
                .montr_ena()
                .modify(|_, w| w.area_dram0_0_wr_ena().bit(on));
        }
        fn write_enabled(&self) -> bool {
            self.0
                .register_block()
                .cpu(CORE)
                .montr_ena()
                .read()
                .area_dram0_0_wr_ena()
                .bit_is_set()
        }
        fn set_interrupt(&mut self, on: bool) {
            self.0
                .register_block()
                .cpu(CORE)
                .intr_ena()
                .modify(|_, w| w.area_dram0_0_wr_intr_ena().bit(on));
        }
        fn interrupt_enabled(&self) -> bool {
            self.0
                .register_block()
                .cpu(CORE)
                .intr_ena()
                .read()
                .area_dram0_0_wr_intr_ena()
                .bit_is_set()
        }
        fn raw(&self) -> bool {
            self.0
                .register_block()
                .cpu(CORE)
                .intr_raw()
                .read()
                .area_dram0_0_wr_raw()
                .bit_is_set()
        }
        fn clear_raw(&mut self) {
            self.0
                .register_block()
                .cpu(CORE)
                .intr_clr()
                .write(|w| w.area_dram0_0_wr_clr().set_bit());
        }
        fn probe_store(&mut self, at: u32) {
            // SAFETY: `at` is the band's first word, spare stack the paint
            // filled at start (`paint`) and nothing uses: it already holds
            // what is written.
            unsafe { (at as *mut u32).write_volatile(super::PAINT) };
        }
    }

    /// The monitor's interrupt, at the highest level esp-hal lets Rust handle.
    /// The store that reached the band completed a few instructions before
    /// (the band is spare stack: it harmed nothing). The monitor is stopped
    /// first, so that nothing cuts off the panic that follows; the panic is
    /// blamed on the code that stored (`crash.rs`, [`hit`]).
    #[esp_hal::handler(priority = esp_hal::interrupt::Priority::Priority3)]
    fn band_interrupt() {
        let mut regs = Regs::new();
        if SELF_TEST.swap(false, Ordering::Relaxed) {
            // The start's delivery test (`deliver`): the interrupt arrived.
            regs.clear_raw();
            DELIVERED.store(true, Ordering::Relaxed);
            return;
        }
        paperpaw_x4::s3_band::stop(&mut regs);
        HIT.store(regs.store_pc(), Ordering::Relaxed);
        panic!("the stack's band was reached");
    }

    /// Binds the interrupt's handler once. The monitor's own interrupt enable
    /// stays off until the arming routine's self-test has passed.
    pub fn bind() {
        if !BOUND.swap(true, Ordering::Relaxed) {
            bind_handler(Interrupt::ASSIST_DEBUG, band_interrupt);
        }
    }

    /// The delivery test (Senior S2 on #917): with the monitor armed and its
    /// interrupt enabled, one more store into the band must run the handler,
    /// which the arming self-test does not show (it reads the raw bit only).
    /// The store is made outside any critical section, so that the interrupt
    /// can be taken. Where it is not delivered in the time given, the monitor
    /// is stopped and the band is not armed.
    pub fn deliver(regs: &mut Regs, start: u32) -> bool {
        DELIVERED.store(false, Ordering::Relaxed);
        SELF_TEST.store(true, Ordering::Relaxed);
        regs.probe_store(start);
        let delivered =
            (0..paperpaw_x4::s3_band::POLLS * 100).any(|_| DELIVERED.load(Ordering::Relaxed));
        if !delivered {
            paperpaw_x4::s3_band::stop(regs);
        }
        SELF_TEST.store(false, Ordering::Relaxed);
        delivered
    }

    /// The program counter of the store that reached the band, if one did.
    pub fn hit() -> Option<u32> {
        Some(HIT.load(Ordering::Relaxed)).filter(|pc| *pc != 0)
    }
}

/// The program counter of the store that reached the band (the ESP32-S3's
/// band interrupt), for the crash slot's blame: the code that stored is the
/// one that overflowed.
#[cfg(all(target_arch = "xtensa", feature = "ppn5"))]
pub fn band_hit() -> Option<u32> {
    s3::hit()
}

/// The ESP32-S3's band is armed with `ppn5` only (the interface-7 plugins);
/// elsewhere there is none.
#[cfg(not(all(target_arch = "xtensa", feature = "ppn5")))]
pub fn band_hit() -> Option<u32> {
    None
}

/// Arms the ESP32-S3's band (`paperpaw_x4::s3_band::arm`): the debug
/// assistant's region monitor over the band at the stack's end, with its
/// self-test. Called at start and after every light sleep (the monitor's
/// registers are the chip's: a sleep may not keep them). Where it does not
/// arm, nothing is left running and [`band_armed`] says no. Never a panic.
#[cfg(all(target_arch = "xtensa", feature = "ppn5"))]
pub fn arm_band() {
    let end = core::ptr::addr_of!(_stack_end) as u32;
    let top = core::ptr::addr_of!(_stack_start) as u32;
    let Some(start) = paperpaw_x4::s3_band::band(end, top) else {
        ARMED.store(false, core::sync::atomic::Ordering::Relaxed);
        return;
    };
    s3::bind();
    let mut regs = s3::Regs::new();
    // Interrupts off: the self-test's store and the raw bit it reads must not
    // meet a task switch that arms or reads the monitor in between (nothing
    // else does, but the loop is short).
    let mut armed = critical_section::with(|_| paperpaw_x4::s3_band::arm(&mut regs, start));
    // The interrupt must arrive, not only the raw bit latch (the self-test in
    // `arm` is inside a critical section: it cannot show that).
    if armed.is_ok() && !s3::deliver(&mut regs, start) {
        armed = Err(paperpaw_x4::s3_band::ArmError::NotDelivered);
    }
    let was = band_armed();
    ARMED.store(armed.is_ok(), core::sync::atomic::Ordering::Relaxed);
    if let Err(why) = armed
        && was
    {
        log::warn!("stack: the S3's band did not re-arm ({why:?})");
        LOST.store(true, core::sync::atomic::Ordering::Relaxed);
        lost_with_native_plugins();
    }
}

/// Whether a plugin call may be entered now (the `call_begin` hook of the
/// interface-7 plugins, todo 185): on the ESP32-S3 only at interrupt level 0
/// on the first core, the one the band's monitor watches. At a level of 3 or
/// more the band's interrupt cannot fire, and on the other core the monitor
/// is not armed: the call is refused (the plugin is not run) and the refusal
/// is logged once. Elsewhere every call is allowed.
#[cfg(all(target_arch = "xtensa", feature = "ppn5"))]
pub fn dispatch_allowed() -> bool {
    use core::sync::atomic::{AtomicBool, Ordering};
    static LOGGED: AtomicBool = AtomicBool::new(false);
    let level = esp_hal::xtensa_lx::interrupt::get_level();
    let on_core = esp_hal::system::Cpu::current() == esp_hal::system::Cpu::ProCpu;
    let allowed = on_core && paperpaw_x4::s3_band::dispatch_allowed(level);
    if !allowed && !LOGGED.swap(true, Ordering::Relaxed) {
        log::warn!("stack: a plugin call refused (interrupt level {level}, first core: {on_core})");
    }
    allowed
}

/// Whether a plugin call may be entered now on the ESP32-C3 (the `call_begin`
/// hook of the interface-7 plugins): only while the band is armed and has not
/// been lost (`paperpaw_x4::stack_band::native_call_allowed`). The re-arm after
/// every light sleep (`power.rs`) runs before any plugin code does, and where
/// it fails every call is refused from then on, the device starts again
/// without native plugins ([`lost_with_native_plugins`]).
#[cfg(all(target_arch = "riscv32", feature = "ppn5"))]
pub fn dispatch_allowed() -> bool {
    use core::sync::atomic::Ordering;
    paperpaw_x4::stack_band::native_call_allowed(
        ARMED.load(Ordering::Relaxed),
        LOST.load(Ordering::Relaxed),
    )
}

/// Every call may run where there is no band (no interface-7 plugin runs).
#[cfg(not(feature = "ppn5"))]
#[allow(
    dead_code,
    reason = "its only caller is the interface-7 plugins' hook (ppn5)"
)]
pub fn dispatch_allowed() -> bool {
    true
}

/// No band on the Xtensa chips without `ppn5`.
#[cfg(all(target_arch = "xtensa", not(feature = "ppn5")))]
pub fn arm_band() {}

/// Says in the log, once the log runs, whether the band is armed: the
/// start arms it before the log exists. The Xtensa chips without `ppn5` have
/// no band and no native plugins: nothing to say.
pub fn report_band() {
    #[cfg(any(target_arch = "riscv32", all(target_arch = "xtensa", feature = "ppn5")))]
    if band_armed() {
        log::info!("stack: the band at the stack's end is armed");
    } else {
        log::warn!(
            "stack: the band at the stack's end is NOT armed (no trigger or monitor free, or no \
             room): plugins with native code are not run"
        );
    }
}
