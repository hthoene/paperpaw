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

/// Starts the X4 again (by a panic, which keeps the message) if the stack
/// reached its end since start.
pub fn check() {
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

/// Whether the band at the stack's end is armed. Where it is not (the
/// chip has none, or trigger 2 was taken by something else), the plugins
/// that run native code do not run: they would have no net.
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
    // Trigger type 2 (match): machine mode (bit 6), store (bit 1), NAPOT
    // (match 1, bit 7), the breakpoint exception (action 0). As esp-hal
    // sets its own.
    let control = (1u32 << 6) | (1 << 1) | (1 << 7);
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
        // Load, store or execute set (bits 0 to 2): in use. Ours, set
        // before, is no other's.
        let in_use = have_control & 0b111 != 0;
        let ours = (have_control & 0xffff) == control && have_address == address;
        if in_use && !ours {
            return false;
        }
        // SAFETY: as above. The trigger is written disabled, then its
        // address, then enabled.
        unsafe {
            core::arch::asm!(
                "csrw 0x7a0, {id}",
                "csrw 0x7a5, {enable}",
                "csrw 0x7a1, zero",
                "csrw 0x7a2, {address}",
                "csrw 0x7a1, {control}",
                id = in(reg) TRIGGER,
                enable = in(reg) enable,
                address = in(reg) address,
                control = in(reg) control,
                options(nomem, nostack),
            );
        }
        true
    });
    if !armed && band_armed() {
        log::warn!("stack: trigger {TRIGGER} was taken: the band is no longer armed");
    }
    ARMED.store(armed, core::sync::atomic::Ordering::Relaxed);
}

/// No band on the Xtensa chips.
#[cfg(not(target_arch = "riscv32"))]
pub fn arm_band() {}

/// Says in the log, once the log runs, whether the band is armed: the
/// start arms it before the log exists. The Xtensa chips have no band
/// and no native plugins: nothing to say.
pub fn report_band() {
    #[cfg(target_arch = "riscv32")]
    if band_armed() {
        log::info!("stack: the band at the stack's end is armed");
    } else {
        log::warn!(
            "stack: the band at the stack's end is NOT armed (trigger {TRIGGER} busy or no \
             room): plugins with native code are not run"
        );
    }
}
