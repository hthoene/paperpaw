//! Whose crash it is: the decision `crash.rs` takes in the firmware's panic
//! handler, here as plain functions for host tests.
//!
//! A panic in a native plugin reaches the one panic handler three ways:
//! the plugin called `Api::panic` itself; a CPU fault in the plugin's own
//! code (esp-hal reports every fault of the RISC-V chips with a `panic!`
//! from its exception handler, which knows nothing of the plugin's flag);
//! or the firmware's code that a plugin called (a shared function) panicked.
//! The first two are the plugin's, which safe mode then switches off
//! alone; the third is the firmware's bug, and the plugin is only named as
//! the caller.

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
    fn a_fault_in_the_firmwares_code_is_not_the_plugins() {
        // A shared function a plugin called, or the firmware's own code.
        assert!(!is_plugins_own(false, true, false));
    }

    #[test]
    fn a_panic_in_shared_code_is_not_the_plugins_even_with_a_stale_pc() {
        // No exception: `mepc` is whatever an old trap left and says nothing.
        assert!(!is_plugins_own(false, false, true));
        assert!(!is_plugins_own(false, false, false));
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
