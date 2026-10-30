//! What a plugin tells of the PaperPaw it runs in comes from the kernel
//! that runs, not from the SDK the plugin was built with: the
//! `User-Agent` header and whether a data plugin fits.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use paperpaw_sdk::builtin::CORE_PLUGIN;
use paperpaw_sdk::web::{user_agent, user_agent_of};
use paperpaw_sdk::{DATA_FORMATS, Manifest, SDK_VERSION, Version, install};
use paperpaw_sdk::{PluginInfo, PluginKind, PluginState, RequiredFor};

/// A kernel of PaperPaw `version`, which is also its SDK's.
fn kernel(version: Version) -> PluginInfo {
    let manifest = Manifest::new(CORE_PLUGIN, "PaperPaw kernel", version);
    PluginInfo::new(
        manifest,
        PluginState::Running,
        true,
        RequiredFor::default(),
        PluginKind::BuiltIn,
        None,
        None,
    )
}

#[test]
fn the_user_agent_names_the_paperpaw_that_runs() {
    let mut h = common::Harness::new();
    // No kernel listed: as this SDK says.
    assert_eq!(user_agent_of(&h.env()), user_agent());
    h.plugins.push(kernel(Version::new(0, 9, 2)));
    let (name, value) = user_agent_of(&h.env());
    assert_eq!(name, "User-Agent");
    assert_eq!(value, "PaperPaw/0.9.2 (e-reader)");
}

#[test]
fn a_data_plugin_fits_the_kernel_that_runs() {
    let mut h = common::Harness::new();
    let later = format!("{}.{}", SDK_VERSION.major, SDK_VERSION.minor + 1);
    assert!(!install::fits_in(&h.env(), &later, 1));
    let newer = Version::new(SDK_VERSION.major, SDK_VERSION.minor + 1, 0);
    h.plugins.push(kernel(newer));
    assert!(install::fits_in(&h.env(), &later, 1));
    assert!(!install::fits_in(&h.env(), &later, DATA_FORMATS + 1));
}
