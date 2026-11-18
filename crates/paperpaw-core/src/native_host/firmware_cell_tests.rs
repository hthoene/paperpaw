//! The firmware cell: nothing is made at rest, the first command makes the
//! table and machine, and they go again once nothing is left (#2654).
#![allow(clippy::unwrap_used, reason = "fixture failures must fail tests")]
use super::*;
use crate::native_host::firmware_driver::plugin_tests::kernel_host;
use crate::native_host::plugin_job::PluginHost;
use paperpaw_native::header5::Host as Loader;
use paperpaw_platform::firmware_fake::{FakeFirmware, paperpaw_facts};

fn fake(dual: bool) -> Box<dyn FirmwareHost> {
    let mut fake = FakeFirmware::new(
        "0.5.0",
        "paperpaw-x4",
        5,
        paperpaw_facts("0.6.0", "paperpaw-x4", 4096),
    );
    fake.dual_capable = dual;
    Box::new(fake)
}

fn plugin_host() -> PluginHost {
    let (loader, offered): (Loader, _) = kernel_host();
    PluginHost {
        loader: Some(loader),
        offered,
        signers: &[],
    }
}

#[test]
fn nothing_is_made_until_a_command_needs_it_and_it_goes_when_nothing_is_left() {
    let cell = FirmwareCell::new(fake(false));
    assert!(cell.driver().is_none() && cell.machine().is_none());
    assert!(!cell.plugins_accepted());
    assert!(cell.set_plugin_host(plugin_host()));
    assert!(cell.plugins_accepted(), "told while at rest");
    let driver = cell.make().unwrap();
    assert!(Shared::ptr_eq(&driver, &cell.make().unwrap()), "one driver");
    assert!(
        driver.borrow().plugins_accepted(),
        "it keeps what it was told"
    );
    assert!(cell.machine().is_some());
    // Held by a command: it stays.
    assert!(!cell.rest());
    drop(driver);
    assert!(cell.rest());
    assert!(cell.driver().is_none() && cell.machine().is_none());
    assert!(
        cell.plugins_accepted(),
        "what it was told rests with the cell"
    );
    assert!(cell.rest(), "at rest twice");
}

#[test]
fn a_driver_with_work_left_stays_made() {
    let cell = FirmwareCell::new(fake(false));
    let driver = cell.make().unwrap();
    driver
        .borrow_mut()
        .submit_check(7, 1, "update.ppu")
        .unwrap();
    drop(driver);
    assert!(!cell.rest(), "a queued check keeps it");
    let driver = cell.driver().unwrap();
    driver.borrow_mut().cancel(7, 1);
    drop(driver);
    assert!(cell.rest(), "nothing left: it goes");
}

#[test]
fn what_an_ended_job_left_for_its_plugin_rests_with_the_parts() {
    let cell = FirmwareCell::new(fake(false));
    let driver = cell.make().unwrap();
    driver
        .borrow_mut()
        .submit_check(7, 1, "update.ppu")
        .unwrap();
    // The check ends (refused by the pump's first step or aborted): its
    // outcome is told and kept for op 13, and its result slot stays.
    driver.borrow_mut().abort_all();
    loop {
        let ready = driver.borrow_mut().take_ready();
        let Some(outcome) = ready else { break };
        driver.borrow_mut().delivered(outcome);
    }
    let told = driver.borrow().lookup(7, 1).unwrap();
    drop(driver);
    assert!(cell.rest(), "only what plugins read is left: it rests");
    assert!(cell.driver().is_none());
    assert_eq!(
        cell.lookup(7, 1).unwrap(),
        Some(told),
        "op 13 still answers"
    );
    assert_eq!(cell.lookup(7, 2).unwrap(), None);
    // Made again, the driver has it too; resting again keeps it.
    assert_eq!(cell.make().unwrap().borrow().lookup(7, 1), Some(told));
    assert!(cell.rest());
    // The plugin stops: nothing is left for it.
    cell.forget_resting(7);
    assert_eq!(cell.lookup(7, 1).unwrap(), None);
    assert!(
        cell.machine().is_none(),
        "no result kept: no machine either"
    );
}

#[test]
fn a_skip_is_counted_across_the_drivers_lives() {
    let cell = FirmwareCell::new(fake(false));
    let counter = cell.skip_counter();
    counter.set(2);
    let driver = cell.make().unwrap();
    assert_eq!(driver.borrow().skipped(), 2);
    drop(driver);
    assert!(cell.rest());
    assert_eq!(cell.make().unwrap().borrow().skipped(), 2);
}

#[test]
fn a_device_that_boots_two_firmwares_keeps_its_driver() {
    let cell = FirmwareCell::new(fake(true));
    assert!(cell.driver().is_some(), "made from the start");
    assert!(!cell.rest(), "its latch is the machine's");
    assert!(cell.driver().is_some());
}
