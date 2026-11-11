//! The trusted-installer grant follows `core.grant.install.<plugin>`: off by
//! default, read for the plugin that asks and no other, current at every step.
#![allow(clippy::unwrap_used, reason = "fixture errors fail tests")]
use super::*;
use crate::native_host::{
    plugin_install::PluginInstall, plugin_job::PluginHost, plugin_jobs::PluginJobs,
    plugin_rules::Evidence,
};
use alloc::format;
use paperpaw_native::{
    header5::{Host as Loader, X4_PROFILE},
    metadata::Host as Offered,
};
use paperpaw_platform::{
    firmware::WRITE_BLOCK,
    firmware_fake::{FakeFirmware, paperpaw_facts},
};
use paperpaw_sdk::{Id, SettingSpec, SettingValue, builtin};

fn kernel() -> Kernel {
    let fake = FakeFirmware::new(
        "0.5.0",
        "paperpaw-x4",
        5,
        paperpaw_facts("0.6.0", "paperpaw-x4", 2 * WRITE_BLOCK),
    );
    let mut kernel = Kernel::builder(crate::tests::DEVICE)
        .firmware_host(alloc::boxed::Box::new(fake))
        .build()
        .unwrap();
    for id in ["core.grant.install.dials", "core.grant.install.clock"] {
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
    }
    let driver = kernel.firmware.clone().unwrap();
    driver.borrow_mut().set_plugin_host(PluginHost {
        loader: Loader::x4(1),
        offered: Offered {
            abi_major: 7,
            minor: 1,
            profile: X4_PROFILE,
            services: &[],
            features: &[],
        },
        signers: &[],
    });
    kernel
}

fn asks(kernel: &Kernel, owner: u64) {
    let driver = kernel.firmware.clone().unwrap();
    let job = PluginInstall::new(owner, 1, "Download/dials", 1, Evidence::NONE).unwrap();
    driver
        .borrow_mut()
        .submit_plugin(PluginJobs::Install(job))
        .unwrap();
}

fn granted(kernel: &Kernel, owner: u64) -> bool {
    let driver = kernel.firmware.clone().unwrap();
    kernel.feed_install_grants(&driver);
    driver.borrow().install_granted(owner)
}

fn grant(kernel: &mut Kernel, id: &str, on: bool) {
    kernel
        .shared
        .settings
        .set(&format!("core.grant.install.{id}"), SettingValue::Bool(on))
        .unwrap();
}

fn running(kernel: &mut Kernel, id: &'static str) -> u64 {
    kernel
        .shared
        .native_instances
        .test_insert(Id::new_static(id), None, true)
        .unwrap()
}

#[test]
fn off_by_default_on_when_granted_and_off_again_when_revoked() {
    let mut kernel = kernel();
    let dials = running(&mut kernel, "dials");
    asks(&kernel, dials);
    assert!(!granted(&kernel, dials), "no grant given");
    grant(&mut kernel, "dials", true);
    assert!(granted(&kernel, dials));
    // Revoked while its job waits: the next step reads it off.
    grant(&mut kernel, "dials", false);
    assert!(!granted(&kernel, dials));
}

#[test]
fn another_plugins_grant_does_not_count_and_a_stopped_plugin_has_none() {
    let mut kernel = kernel();
    let dials = running(&mut kernel, "dials");
    let clock = running(&mut kernel, "clock");
    asks(&kernel, dials);
    grant(&mut kernel, "clock", true);
    assert!(!granted(&kernel, dials), "the clock's grant is the clock's");
    grant(&mut kernel, "dials", true);
    assert!(granted(&kernel, dials));
    assert!(!granted(&kernel, clock), "it asked for nothing");
    kernel.shared.native_instances.stop(dials).unwrap();
    assert!(!granted(&kernel, dials), "a plugin that stopped holds none");
}

#[test]
fn no_job_means_nothing_to_read() {
    let mut kernel = kernel();
    let dials = running(&mut kernel, "dials");
    grant(&mut kernel, "dials", true);
    assert!(!granted(&kernel, dials));
}

#[test]
fn the_key_is_built_on_the_stack_and_an_id_that_does_not_fit_has_none() {
    let mut buffer = [0; KEY_MAX];
    assert_eq!(
        grant_key("dials", &mut buffer),
        Some("core.grant.install.dials")
    );
    let long = "a".repeat(KEY_MAX);
    assert_eq!(grant_key(&long, &mut buffer), None);
}

#[test]
fn more_owners_than_the_device_holds_are_not_all_read() {
    let mut granted = Granted::NONE;
    for owner in 1..=(REQUESTS_MAX as u64 + 2) {
        granted.add(owner);
    }
    assert!(granted.holds(1) && granted.holds(REQUESTS_MAX as u64));
    assert!(!granted.holds(REQUESTS_MAX as u64 + 1), "no room: no grant");
    assert!(!Granted::NONE.holds(0));
}
