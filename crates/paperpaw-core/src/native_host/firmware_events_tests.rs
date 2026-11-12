//! The firmware pump: the job ends through the kernel's pump and its outcome is
//! kept once told; the statuses of 5.5; 96 and 97 can be registered.
#![allow(clippy::unwrap_used, reason = "fixture errors fail tests")]
use super::*;
use crate::native_host::firmware_install::KIND_FIRMWARE;
use paperpaw_platform::{
    firmware::WRITE_BLOCK,
    firmware_fake::{FakeFirmware, paperpaw_facts},
};

fn kernel() -> Kernel {
    let fake = FakeFirmware::new(
        "0.5.0",
        "paperpaw-x4",
        5,
        paperpaw_facts("0.6.0", "paperpaw-x4", 2 * WRITE_BLOCK),
    );
    // A random source: the confirmation's nonce needs one.
    fn fill(buf: &mut [u8]) -> bool {
        buf.fill(7);
        true
    }
    Kernel::builder(crate::tests::DEVICE)
        .random(fill)
        .firmware_host(alloc::boxed::Box::new(fake))
        .build()
        .unwrap()
}

fn outcome(kind: u32, reason: u8) -> Outcome {
    Outcome {
        owner: 1,
        request: 1,
        kind,
        reason,
        flags: 0,
        id: None,
    }
}

#[test]
fn a_kernel_without_a_firmware_host_has_nothing_to_pump() {
    let mut kernel = Kernel::builder(crate::tests::DEVICE).build().unwrap();
    assert!(kernel.firmware.is_none());
    assert!(!kernel.native_firmware_pump());
}

#[test]
fn a_check_ends_through_the_pump_and_its_outcome_is_kept_for_its_plugin() {
    let mut kernel = kernel();
    let driver = kernel.firmware.as_ref().unwrap().make().unwrap();
    driver
        .borrow_mut()
        .submit_check(1, 7, "update.ppu")
        .unwrap();
    let mut passes = 0;
    while kernel.native_firmware_pump() {
        passes += 1;
        assert!(passes < 100, "the pump ends");
    }
    // Nobody is running to be told: the outcome is delivered at once, kept.
    let kept = driver.borrow().kept(1, KIND_CHECK).unwrap();
    assert_eq!((kept.request, kept.reason), (7, 0));
    assert_eq!(driver.borrow().waiting(), 0);
}

#[test]
fn an_install_waits_for_its_confirmation_and_the_pump_goes_quiet() {
    let mut kernel = kernel();
    let driver = kernel.firmware.as_ref().unwrap().make().unwrap();
    driver
        .borrow_mut()
        .submit_install(1, 5, "update.ppu", None, 0)
        .unwrap();
    for _ in 0..20 {
        kernel.native_firmware_pump();
    }
    // The question waits for the user (shown, or waiting to be); nothing was
    // written and the pump has gone quiet.
    let machine = driver.borrow().machine().unwrap();
    assert_eq!(
        machine.borrow().state(),
        crate::native_host::firmware_install::State::WaitingForUser
    );
    assert_eq!(driver.borrow().kept(1, KIND_FIRMWARE), None);
    assert!(!driver.borrow().has_work(0));
}

#[test]
fn the_statuses_follow_the_outcome_reasons() {
    let cases = [
        (0, OK),
        (1, ERR_IO),
        (2, ERR_DENIED),
        (3, ERR_CANCELLED),
        (4, ERR_INVALID),
        (9, ERR_DENIED),
        (12, ERR_UNSUPPORTED),
        (13, ERR_NO_SPACE),
        (14, ERR_UNSUPPORTED),
        (18, ERR_STATE),
        (20, ERR_CANCELLED),
        (21, ERR_STATE),
        (200, ERR_IO),
    ];
    for (reason, status) in cases {
        assert_eq!(
            status_of(outcome(KIND_FIRMWARE, reason)),
            status,
            "reason {reason}"
        );
    }
    // A check that ran is `OK` with its verdict; one that could not run is not.
    assert_eq!(status_of(outcome(KIND_CHECK, 8)), OK);
    assert_eq!(status_of(outcome(KIND_CHECK, 12)), OK);
    assert_eq!(status_of(outcome(KIND_CHECK, 20)), ERR_CANCELLED);
    assert_eq!(status_of(outcome(KIND_CHECK, 1)), ERR_IO);
}

#[test]
fn the_firmware_events_can_be_registered_for() {
    assert!(super::super::events::is_event_tag(96));
    assert!(super::super::events::is_event_tag(97));
    assert!(!super::super::events::is_event_tag(98));
}

/// A removal with its data forgets the plugin's grants, `core.grant.<kind>.<id>`
/// of exactly that id, and no other plugin's.
fn is_set(kernel: &Kernel, id: &str) -> bool {
    kernel.shared.settings.is_set(id)
}

#[test]
fn removed_with_its_data_a_plugin_loses_its_grants_and_only_its_own() {
    use paperpaw_sdk::{SettingSpec, SettingValue, builtin};
    let mut kernel = kernel();
    let ids = [
        "core.grant.install.dials",
        "core.grant.style.dials",
        "core.grant.install.dials-plus",
        "core.grant.install.clock",
    ];
    for id in ids {
        kernel
            .shared
            .settings
            .define(SettingSpec::switch(
                id,
                "grant",
                false,
                builtin::groups::OTHER,
            ))
            .unwrap();
        kernel
            .shared
            .settings
            .set(id, SettingValue::Bool(true))
            .unwrap();
    }
    assert!(
        kernel.forget_removed("dials"),
        "no secrets host wired: nothing to fail"
    );
    assert!(
        !kernel.forget_listed("dials"),
        "a listed id's secrets are not forgotten by a host that is not there"
    );
    assert!(
        !is_set(&kernel, ids[0]) && !is_set(&kernel, ids[1]),
        "the plugin's own grants went"
    );
    assert!(
        is_set(&kernel, ids[2]) && is_set(&kernel, ids[3]),
        "no other plugin's grant did"
    );
    // Nothing to forget: nothing changes, and nothing is said.
    kernel.forget_grants("dials");
    kernel.forget_grants("nobody");
    assert!(is_set(&kernel, ids[2]) && is_set(&kernel, ids[3]));
}

#[test]
fn a_plugin_with_many_grants_loses_every_one() {
    use paperpaw_sdk::{SettingSpec, SettingValue, builtin};
    let mut kernel = kernel();
    // Ids are `&'static` in a spec: a test may leak 20 short strings.
    let ids: alloc::vec::Vec<&'static str> = (0..20)
        .map(|n| &*alloc::format!("core.grant.kind{n}.dials").leak())
        .collect();
    for &id in &ids {
        kernel
            .shared
            .settings
            .define(SettingSpec::switch(
                id,
                "grant",
                false,
                builtin::groups::OTHER,
            ))
            .unwrap();
        kernel
            .shared
            .settings
            .set(id, SettingValue::Bool(true))
            .unwrap();
    }
    kernel.forget_grants("dials");
    assert!(ids.iter().all(|&id| !is_set(&kernel, id)));
}

#[test]
fn the_grants_go_even_when_the_secrets_could_not_be_forgotten() {
    use paperpaw_sdk::{SettingSpec, SettingValue, builtin};
    let mut kernel = kernel();
    let id = "core.grant.install.dials";
    kernel
        .shared
        .settings
        .define(SettingSpec::switch(
            id,
            "grant",
            false,
            builtin::groups::OTHER,
        ))
        .unwrap();
    kernel
        .shared
        .settings
        .set(id, SettingValue::Bool(true))
        .unwrap();
    assert!(!kernel.forget_removed_by("dials", || false));
    assert!(!is_set(&kernel, id), "no grant stays for a later plugin");
}

#[test]
fn a_removal_a_cut_left_is_finished_at_the_first_pass_without_making_the_driver() {
    use crate::native_host::plugin_swap::{Removal, begin_removal};
    use paperpaw_platform::{Storage as _, firmware::Text};
    let card = paperpaw_sdk::MemoryStorage::new();
    card.write("plugins/dials/plugin.ini", b"x").unwrap();
    card.write(".paperpaw/plugins/dials/notes.txt", b"x")
        .unwrap();
    let removal = Removal {
        id: Text::new("dials"),
        data: true,
    };
    begin_removal(&card, &removal).unwrap();
    let fake = FakeFirmware::new(
        "0.5.0",
        "paperpaw-x4",
        5,
        paperpaw_facts("0.6.0", "paperpaw-x4", 2 * WRITE_BLOCK),
    );
    let mut kernel = Kernel::builder(crate::tests::DEVICE)
        .storage(card)
        .firmware_host(alloc::boxed::Box::new(fake))
        .build()
        .unwrap();
    // The start left it at the step the host must do (forget its secrets and
    // grants): kept by the cell, no driver made for it.
    let cell = kernel.firmware.clone().unwrap();
    assert!(cell.resume_wanted());
    assert!(cell.driver().is_none());
    assert!(kernel.shared.storage.size("plugins/.journal").is_ok());
    kernel.native_firmware_pump();
    assert!(!cell.resume_wanted());
    assert!(kernel.shared.storage.size("plugins/.journal").is_err());
    assert!(cell.driver().is_none(), "nothing else asked for one");
}
