//! What the interface-6 tests (`modes.rs`, `menu.rs`, `off_screen.rs`) proved
//! with a whole kernel and the plugin in it. The decisions that need no host
//! are tested in `src/model/tests.rs`; these need the host to run a native
//! plugin end to end and stay ignored until it does. Each names what is missing.

#[test]
#[ignore = "needs core.device op 12 (DEV reads, PR #311) and a host that runs native plugins (Core H1)"]
fn the_mode_is_worn_on_start_battery_charge_and_setting_changes() {
    unreachable!(
        "start, resume, battery_measured, charge_changed and setting_changed each end in command.wear only when the mode changes"
    );
}

#[test]
#[ignore = "needs DEV commands 1572-1575 on the host with uses.power (PR #312 merged; native dispatch pending)"]
fn the_power_menu_sleeps_switches_off_and_restarts() {
    unreachable!(
        "choice 0..n of the question maps to sleep, power_off, restart, touch lock toggle"
    );
}

#[test]
#[ignore = "needs the sleep plugin on interface 7 (reads handover param `off`) and screen-before-power ordering in one batch"]
fn switching_off_shows_the_sleep_screen_first() {
    unreachable!(
        "open slot core.sleep with off=1, then power_off, in one batch; the frame is drawn before power is cut"
    );
}

#[test]
#[ignore = "needs the status stage on the host (Core todo 76) and PR #335"]
fn the_status_item_shows_the_mode_unless_it_is_normal() {
    unreachable!("core.status answers nothing in Normal and the Saving/Offline marker otherwise");
}

#[test]
#[ignore = "needs the host's quick panel to draw a choice setting as segments"]
fn the_mode_is_in_the_quick_panel() {
    unreachable!("menu core.quick lists power.mode at order 30");
}
