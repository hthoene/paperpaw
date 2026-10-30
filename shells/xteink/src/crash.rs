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
//! `4280a1c4 (hello+0x7f30)`, and safe mode learns the plugin
//! (`safe_mode::crashed_in`), where the plugin panicked itself. A panic in
//! the firmware's code that the plugin called (a shared function) is the
//! firmware's: the message says `[shared code, called from <plugin>]`
//! and the plugin is not blamed.

use core::fmt::Write;

/// The last panic's message (zero-terminated), kept through the reset.
#[esp_hal::ram(unstable(rtc_fast, persistent))]
static mut LAST: [u8; 400] = [0; 400];
/// The calls kept with a panic, at most.
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

#[panic_handler]
fn panic(info: &core::panic::PanicInfo<'_>) -> ! {
    esp_println::println!("PANIC: {info}");
    let mut buf = [0u8; 400];
    let mut cut = Cut {
        buf: &mut buf,
        len: 0,
    };
    // The message first: the location's path is long.
    let _ = write!(cut, "{}", info.message());
    if let Some(at) = info.location() {
        let file = at.file().rsplit(['/', '\\']).next().unwrap_or(at.file());
        let _ = write!(cut, " ({file}:{})", at.line());
    }
    // Where it came from, without the panic's own machinery (the
    // addresses inside `core` and `alloc` are named the same way).
    let _ = write!(cut, " @");
    let mut last = 0;
    let mut in_plugin = false;
    // A plugin that panicked itself (through its table's `panic`) is
    // blamed; so is a fault in its own code. A panic in the firmware's
    // code that a plugin called, a shared function, is the firmware's bug
    // (no panics on bad input): the plugin is named as the caller, and not
    // blamed, so a boot loop there ends in safe mode for all plugins.
    let own = crate::native::api::panic_is_a_plugins();
    for frame in esp_backtrace::Backtrace::capture()
        .frames()
        .iter()
        .take(CALLS)
    {
        // The start's frame repeats to the end.
        let pc = frame.program_counter();
        if pc == last {
            break;
        }
        last = pc;
        let _ = write!(cut, " {pc:x}");
        crate::native::attribute(pc as u32, &mut |id, offset| {
            let _ = write!(cut, " ({id}+{offset:#x})");
            // The innermost plugin is the one it happened in.
            if !in_plugin {
                in_plugin = true;
                if own {
                    crate::safe_mode::crashed_in(id);
                } else {
                    let _ = write!(cut, " [shared code, called from {id}]");
                }
            }
        });
    }
    // SAFETY: one core, the firmware stopped: nothing else touches it.
    unsafe { core::ptr::addr_of_mut!(LAST).write_volatile(buf) };
    esp_hal::system::software_reset()
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
    // SAFETY: as in `panic`; read once at start or over USB.
    let buf = unsafe { core::ptr::addr_of!(LAST).read_volatile() };
    unsafe { core::ptr::addr_of_mut!(LAST).write_volatile([0; 400]) };
    let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    // Unset memory after a power-up is noise: printable text only.
    let text = core::str::from_utf8(&buf[..len]).ok()?;
    (!text.is_empty() && text.chars().all(|c| !c.is_control() || c == '\n'))
        .then(|| alloc::string::String::from(text))
}
