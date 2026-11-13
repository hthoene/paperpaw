//! Every linked native plugin, admitted against what the X4's firmware offers
//! (`paperpaw_x4::native_offer`, the lists `shells/xteink` gives the kernel):
//! a plugin the device would refuse is found here, on a computer.
//!
//! The target of a linked plugin's metadata is this computer's, so the profile
//! is the one of this shell; only the services and the features are the X4's.

use paperpaw_core::native_host::shell::admission::admit;
use paperpaw_native::metadata::Host;
use paperpaw_x4::native_offer::{Wired, features, services};

fn host(wired: Wired) -> Host<'static> {
    Host {
        abi_major: 7,
        minor: paperpaw_native::host_glue::MINOR,
        profile: crate::native_runtime::host_profile(),
        services: services(wired),
        features: features(wired),
    }
}

/// The ids of the linked plugins `wired` does not admit, with why.
fn refused(wired: Wired) -> Vec<String> {
    let host = host(wired);
    paperpaw_app::NATIVE_PLUGINS
        .iter()
        .filter_map(|(id, linked)| {
            admit(linked.metadata(), &host)
                .err()
                .map(|why| format!("{id}: {why:?}"))
        })
        .collect()
}

#[test]
fn the_x4_offers_every_linked_plugin_what_it_needs() {
    let refused = refused(Wired::ALL);
    assert!(refused.is_empty(), "refused on the X4: {refused:#?}");
}

#[test]
fn without_core_secrets_the_plugins_that_declare_it_are_refused() {
    // Why decision 156's first test image wires `core.secrets` with a sealer
    // that does not seal: without the service these are not registered.
    let refused = refused(Wired {
        secrets: false,
        ..Wired::ALL
    });
    assert!(!refused.is_empty());
}
