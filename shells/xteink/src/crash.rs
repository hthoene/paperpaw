//! What happens when the firmware panics: it says why in its log, keeps
//! the message through the reset (for `boot` over USB: the log may not
//! reach the computer, e.g. when the radio took the USB link along) and
//! starts the X4 again, where halting would leave it on with nothing to
//! be done until a watchdog came. Started again, it writes the message to
//! the card ([`keep`]), for bug reports.
//!
//! The message ends with where the code was, `@` and the addresses of the
//! calls that led there, innermost first (the firmware is built with frame
//! pointers): `tools/x4-remote.py --crashes` names their functions from
//! the firmware's symbols. An allocation that failed says so this way
//! too, with the code that asked for it. A call in a native plugin's code
//! is followed by the plugin and the offset into its file, e.g.
//! `4280a1c4 (hello+0x7f30)`, and safe mode learns the innermost plugin
//! on the stack (`safe_mode::crashed_in`): where it panicked itself, and
//! where the firmware's code it called panicked (a shared function, an
//! allocation there), which the message says with `[shared code, called
//! from <plugin>]` (`paperpaw_x4::blame`).

use core::fmt::Write;

/// The last panic's message (zero-terminated), kept through the reset,
/// sealed with a magic word and a checksum (`paperpaw_x4::rtc_guard`): the
/// RTC memory also holds what a power-up or the firmware before left.
#[esp_hal::ram(unstable(rtc_fast, persistent))]
static mut LAST: [u8; paperpaw_x4::rtc_guard::MESSAGE_LEN] =
    [0; paperpaw_x4::rtc_guard::MESSAGE_LEN];
/// The calls kept with a panic, at most. The plugin to blame is looked
/// for in all calls captured (`ESP_BACKTRACE_CONFIG_BACKTRACE_FRAMES` in
/// `.cargo/config.toml`, 64): a panic deep in a shared body, a recursive
/// parser's or an allocation's, has the plugin's call further down.
const CALLS: usize = 16;

/// Writes into a fixed buffer, cutting what does not fit between two
/// characters (a character cut in two would make the whole message
/// unreadable) and leaving its last byte zero.
struct Cut<'a> {
    buf: &'a mut [u8],
    len: usize,
}

impl Write for Cut<'_> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let room = self.buf.len().saturating_sub(self.len + 1);
        let mut take = s.len().min(room);
        while !s.is_char_boundary(take) {
            take -= 1;
        }
        self.buf[self.len..self.len + take].copy_from_slice(&s.as_bytes()[..take]);
        self.len += take;
        Ok(())
    }
}

/// Whether this handler runs already: a panic inside it (a message whose
/// formatting panics, the plugin's `Display` that does) comes back here,
/// and formatting it again would panic again, one frame deeper each time,
/// until the stack is gone. A second entry names no message: it keeps the
/// place and the plugin to blame, and starts the X4 again. Plain loads
/// and stores (the X4's RISC-V has no atomic swap): one core, and a
/// panic's own interrupts do not panic.
static ENTERED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

#[panic_handler]
fn panic(info: &core::panic::PanicInfo<'_>) -> ! {
    // Where the stack pointer stands as the handler is entered (this frame is
    // a few bytes): the handler's own depth is counted from it.
    handle(info, crate::stack::sp())
}

/// The handler. Its frames are separate from `panic`'s, so that `entry` is
/// the pointer before them: the note it ends the message with (`stack_note`)
/// is how deep they went.
#[inline(never)]
fn handle(info: &core::panic::PanicInfo<'_>, entry: usize) -> ! {
    use core::sync::atomic::Ordering;
    let again = ENTERED.load(Ordering::Relaxed);
    ENTERED.store(true, Ordering::Relaxed);
    if !again {
        esp_println::println!("PANIC: {info}");
    }
    let mut buf = [0u8; paperpaw_x4::rtc_guard::MESSAGE_LEN];
    // The last of the text's room is the stack note's: the calls are cut, not it.
    let mut cut = Cut {
        buf: &mut buf[paperpaw_x4::rtc_guard::BODY]
            [..paperpaw_x4::rtc_guard::BODY.len() - paperpaw_x4::stack_note::NOTE_LEN],
        len: 0,
    };
    // The message first: the location's path is long.
    if again {
        let _ = write!(cut, "{}", paperpaw_x4::blame::PANIC_IN_PANIC);
    } else {
        let _ = write!(cut, "{}", info.message());
    }
    if let Some(at) = info.location() {
        let file = at.file().rsplit(['/', '\\']).next().unwrap_or(at.file());
        let _ = write!(cut, " ({file}:{})", at.line());
    }
    // Where it came from, without the panic's own machinery (the
    // addresses inside `core` and `alloc` are named the same way).
    let _ = write!(cut, " @");
    let mut last = 0;
    // The innermost plugin on the stack is blamed: one that panicked
    // itself (through its table's `panic`), one that faulted in its own
    // code (esp-hal reports a CPU fault through this same handler, without
    // the plugin's flag, so the faulting address, `mepc`, says whose code
    // it was), and one whose call into the firmware's code panicked or
    // faulted (a shared function, an allocation there): safe mode then
    // switches that plugin off, not all of them (ADR 0026).
    // The ESP32-S3's band interrupt panics itself (`stack::band_hit`): the
    // store it saw is the faulting instruction, as `mepc` is on the X4.
    let band_hit = crate::stack::band_hit();
    let from_exception = band_hit.is_some()
        || info
            .location()
            .is_some_and(|at| paperpaw_x4::blame::is_exception(at.file()));
    let mut fault_in_plugin = false;
    if let Some(pc) = band_hit.or_else(fault_address).filter(|_| from_exception) {
        crate::native::attribute(pc, &mut |id, offset| {
            fault_in_plugin = true;
            let _ = write!(cut, " fault at {pc:x} ({id}+{offset:#x})");
            // Its own code faulted (`is_plugins_own`): it is the one.
            crate::safe_mode::crashed_in(id);
        });
    }
    let blame = paperpaw_x4::blame::blame(
        crate::native::api::panic_is_a_plugins(),
        from_exception,
        fault_in_plugin,
    );
    // The faulting plugin is named already.
    let mut in_plugin = fault_in_plugin;
    for (n, frame) in esp_backtrace::Backtrace::capture()
        .frames()
        .iter()
        .enumerate()
    {
        // The start's frame repeats to the end.
        let pc = frame.program_counter();
        if pc == last {
            break;
        }
        last = pc;
        let printed = n < CALLS;
        if !printed && in_plugin {
            break;
        }
        if printed {
            let _ = write!(cut, " {pc:x}");
        }
        crate::native::attribute(pc as u32, &mut |id, offset| {
            if printed {
                let _ = write!(cut, " ({id}+{offset:#x})");
            }
            // The innermost plugin is the one it happened in.
            if !in_plugin {
                in_plugin = true;
                if blame == paperpaw_x4::blame::Blame::Caller {
                    let _ = write!(cut, " [shared code, called from {id}]");
                }
                crate::safe_mode::crashed_in(id);
            }
        });
    }
    // How deep the handler went, now that the deepest of it is done: the note
    // is the last of the text, in the room kept for it.
    let used = cut.len;
    // Only for a panic that ran on the main stack; the handler's depth only
    // after a band trap (a breakpoint, `mcause` 3: the store trigger).
    let breakpoint = band_hit.is_some() || (from_exception && trap_cause() == Some(BREAKPOINT));
    if let Some((depth, left)) = crate::stack::handler_use(entry, breakpoint) {
        let mut note = Cut {
            buf: &mut buf[paperpaw_x4::rtc_guard::BODY][used..],
            len: 0,
        };
        let _ = paperpaw_x4::stack_note::write(&mut note, depth, left);
    }
    paperpaw_x4::rtc_guard::seal_message(&mut buf);
    // SAFETY: one core, the firmware stopped: nothing else touches it.
    unsafe { core::ptr::addr_of_mut!(LAST).write_volatile(buf) };
    esp_hal::system::software_reset()
}

/// `mcause` of a breakpoint exception, the band's store trigger (and an
/// `ebreak`).
const BREAKPOINT: u32 = 3;

/// The cause of the exception being handled: `mcause`, still what the trap
/// left (nothing has trapped since, as for [`fault_address`]); an interrupt
/// has its top bit set. Only the RISC-V chips have it.
#[cfg(target_arch = "riscv32")]
fn trap_cause() -> Option<u32> {
    let cause: u32;
    // SAFETY: reads a machine-mode CSR; no memory is touched.
    unsafe { core::arch::asm!("csrr {}, mcause", out(reg) cause, options(nomem, nostack)) };
    Some(cause)
}

#[cfg(not(target_arch = "riscv32"))]
fn trap_cause() -> Option<u32> {
    None
}

/// The instruction that faulted: `mepc` is still its address inside the
/// exception handler's panic (nothing has trapped since). Only the RISC-V
/// chips have it; the X4 Pro's plugins carry no native code.
#[cfg(target_arch = "riscv32")]
fn fault_address() -> Option<u32> {
    let pc: u32;
    // SAFETY: reads a machine-mode CSR; no memory is touched.
    unsafe { core::arch::asm!("csrr {}, mepc", out(reg) pc, options(nomem, nostack)) };
    Some(pc)
}

#[cfg(not(target_arch = "riscv32"))]
fn fault_address() -> Option<u32> {
    None
}

/// Where panics are kept on the card, for bug reports: a line each, the
/// version and the message.
const FILE: &str = paperpaw_sdk::data_path!("crashes.txt");
/// The file starts anew once it is this large.
const MAX_FILE: u64 = 16 * 1024;

/// Writes the last run's panic `message` to the card.
pub fn keep(storage: &dyn paperpaw_platform::Storage, message: &str) {
    let line = alloc::format!(
        "{}\t{}\n",
        env!("PAPERPAW_VERSION"),
        message.replace(['\n', '\t'], " ")
    );
    let _ = storage.create_dir(paperpaw_sdk::data::DIR);
    let full = storage.size(FILE).is_ok_and(|size| size > MAX_FILE);
    let _ = if full {
        storage.write(FILE, line.as_bytes())
    } else {
        storage.append(FILE, line.as_bytes())
    };
}

/// The last panic's message, if the last run ended in one; cleared.
pub fn take_last() -> Option<alloc::string::String> {
    let message = last();
    // SAFETY: as in `panic`; read once at start or over USB.
    unsafe {
        core::ptr::addr_of_mut!(LAST).write_volatile([0; paperpaw_x4::rtc_guard::MESSAGE_LEN])
    };
    message
}

/// The same, left where it is: for a start that goes back to the firmware
/// before it (an update that failed its trial), which reads it as its own.
/// Noise, or what another firmware left, is no message.
pub fn last() -> Option<alloc::string::String> {
    // SAFETY: as in `panic`; read at start or over USB.
    let buf = unsafe { core::ptr::addr_of!(LAST).read_volatile() };
    paperpaw_x4::rtc_guard::message(&buf).map(alloc::string::String::from)
}
