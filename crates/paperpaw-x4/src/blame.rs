//! Whose crash it is: the decision `crash.rs` takes in the firmware's panic
//! handler, here as plain functions for host tests.
//!
//! A panic in a native plugin reaches the one panic handler three ways:
//! the plugin called `Api::panic` itself; a CPU fault in the plugin's own
//! code (esp-hal reports every fault of the RISC-V chips with a `panic!`
//! from its exception handler, which knows nothing of the plugin's flag);
//! or the firmware's code that a plugin called panicked or faulted: a
//! shared function (its parsers of JSON, INI and XML read what the plugin
//! hands them), an allocation that failed there, or another call into the
//! firmware. Each is blamed on the innermost plugin whose code is on the
//! stack ([`blame`]), so that safe mode switches off that plugin alone
//! (ADR 0026, "A crash in shared code is the calling plugin's"); the
//! message says which of the two it was.

/// Whose crash it is, of the innermost plugin whose code is on the stack.
/// Both are the plugin's: safe mode switches it off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Blame {
    /// It panicked or faulted in its own code.
    Own,
    /// The firmware's code it called panicked or faulted: a shared
    /// function, an allocation there, a call through its table. The
    /// message says `[shared code, called from <plugin>]`.
    Caller,
}

/// Whose crash it is ([`is_plugins_own`] takes the same arguments).
pub fn blame(from_plugin: bool, from_exception: bool, fault_in_plugin: bool) -> Blame {
    if is_plugins_own(from_plugin, from_exception, fault_in_plugin) {
        Blame::Own
    } else {
        Blame::Caller
    }
}

/// What the crash slot says where a panic came while the panic handler
/// was formatting another (a message whose `Display` panics): the first
/// message is lost, the place and the plugin to blame are kept.
pub const PANIC_IN_PANIC: &str = "panic in panic (the first message was not formatted again)";

/// Whether the panic is the plugin's own. `from_plugin`: it called
/// `Api::panic`. `from_exception`: the panic comes from the CPU's
/// exception handler (see [`is_exception`]). `fault_in_plugin`: the program
/// counter of the fault (`mepc`, still the faulting instruction's inside
/// the handler) lies in a plugin's code. A fault in the firmware's code,
/// a shared function included, is not.
pub fn is_plugins_own(from_plugin: bool, from_exception: bool, fault_in_plugin: bool) -> bool {
    from_plugin || (from_exception && fault_in_plugin)
}

/// Whether a panic at `file` (its `Location`) comes from esp-hal's
/// exception handler (`src/exception_handler/mod.rs`): a fault of the CPU,
/// not a `panic!` of the code that ran.
pub fn is_exception(file: &str) -> bool {
    file.replace('\\', "/").contains("exception_handler/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plugin_that_panics_itself_is_blamed() {
        assert!(is_plugins_own(true, false, false));
        assert!(is_plugins_own(true, true, true));
    }

    #[test]
    fn a_fault_in_a_plugins_code_is_blamed_without_the_flag() {
        assert!(is_plugins_own(false, true, true));
    }

    #[test]
    fn a_fault_in_the_firmwares_code_is_not_the_plugins_own() {
        // A shared function a plugin called, or the firmware's own code.
        assert!(!is_plugins_own(false, true, false));
    }

    #[test]
    fn a_panic_in_shared_code_is_not_the_plugins_own_even_with_a_stale_pc() {
        // No exception: `mepc` is whatever an old trap left and says nothing.
        assert!(!is_plugins_own(false, false, true));
        assert!(!is_plugins_own(false, false, false));
    }

    #[test]
    fn a_crash_in_the_code_a_plugin_called_is_blamed_on_the_caller() {
        // A panic in a shared function (a parser, a failed allocation):
        // no flag, no exception.
        assert_eq!(blame(false, false, false), Blame::Caller);
        // A fault in the firmware's code it called.
        assert_eq!(blame(false, true, false), Blame::Caller);
        // A stale `mepc` in a plugin without an exception changes nothing.
        assert_eq!(blame(false, false, true), Blame::Caller);
    }

    #[test]
    fn its_own_panic_and_fault_stay_its_own() {
        assert_eq!(blame(true, false, false), Blame::Own);
        assert_eq!(blame(false, true, true), Blame::Own);
        assert_eq!(blame(true, true, false), Blame::Own);
    }

    #[test]
    fn the_panic_in_panic_note_fits_the_crash_slot_with_room_for_the_place() {
        // The slot's body holds the note, ` (file:line)` and ` @` with some
        // calls; the note must leave most of it.
        let body = crate::rtc_guard::BODY.len();
        assert!(
            PANIC_IN_PANIC.len() < body / 4,
            "{} of {body}",
            PANIC_IN_PANIC.len()
        );
        assert!(PANIC_IN_PANIC.is_ascii());
    }

    #[test]
    fn the_exception_handlers_panics_are_known_by_their_file() {
        assert!(is_exception(
            "/home/u/.cargo/registry/src/x/esp-hal-1.2.2/src/exception_handler/mod.rs"
        ));
        assert!(is_exception(
            "C:\\cargo\\esp-hal-1.2.2\\src\\exception_handler\\mod.rs"
        ));
        assert!(!is_exception("crates/paperpaw-text/src/layout.rs"));
        assert!(!is_exception("src/exceptions.rs"));
    }
}
