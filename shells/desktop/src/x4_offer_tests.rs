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

// A plugin file on the card (a card plugin) is offered the same services as
// a linked one, as the families are wired, and of the consent features only
// the families it may reach (`card_features`). The linked plugins' metadata
// stands for card plugins' here: the check is the same.

use paperpaw_x4::native_offer::card_features;

fn card_host(wired: Wired) -> Host<'static> {
    Host {
        features: card_features(wired),
        ..host(wired)
    }
}

fn metadata_of(id: &str) -> &'static [u8] {
    paperpaw_app::NATIVE_PLUGINS
        .iter()
        .find(|(name, _)| *name == id)
        .unwrap_or_else(|| panic!("{id} is not linked in this build"))
        .1
        .metadata()
}

/// The features the plugin `id` keeps on `host`, or why it is refused.
fn kept(id: &str, host: &Host<'static>) -> Result<Vec<String>, String> {
    paperpaw_native::metadata::check(metadata_of(id), host)
        .map(|checked| checked.available_features().map(String::from).collect())
        .map_err(|why| format!("{why:?}"))
}

const NO_SECRETS: Wired = Wired {
    secrets: false,
    ..Wired::ALL
};

#[test]
fn a_card_plugin_that_only_declares_screens_is_admitted_with_nothing_wired() {
    // The keys-like case: `core.ui`, `core.registry` and the device.
    assert_eq!(kept("about", &card_host(Wired::default())), Ok(vec![]));
}

#[test]
fn a_card_plugin_that_reaches_the_network_and_the_card_is_admitted_where_they_are_wired() {
    // The stats-like case: `core.net`, `core.library`, `core.secrets`, `core.document`
    // and the features `uses.network` and `uses.library`.
    let kept = kept("opds", &card_host(Wired::ALL)).unwrap();
    for feature in ["uses.network", "uses.library"] {
        assert!(kept.iter().any(|f| f == feature), "{feature}: {kept:?}");
    }
    assert!(!kept.iter().any(|f| f == "uses.secrets"), "{kept:?}");
}

#[test]
fn a_card_plugin_is_refused_the_service_of_a_family_that_is_not_wired() {
    let work = Wired {
        work: false,
        ..Wired::ALL
    };
    let card = Wired {
        card: false,
        ..Wired::ALL
    };
    for (what, wired) in [("net", work), ("library", card), ("secrets", NO_SECRETS)] {
        let refusal = kept("opds", &card_host(wired)).unwrap_err();
        assert!(refusal.contains("Service"), "{what}: {refusal}");
    }
    // `core.secrets` and the sockets of a plugin that keeps a password.
    assert!(kept("mail", &card_host(Wired::ALL)).is_ok());
    assert!(kept("mail", &card_host(NO_SECRETS)).is_err());
    assert!(kept("lock", &card_host(NO_SECRETS)).is_err());
}

#[test]
fn a_card_plugin_that_requires_a_first_party_feature_is_refused_whatever_is_wired() {
    // `uses.power`, and `uses.firmware` with `uses.install`: required, so no
    // admission; a linked plugin has them.
    for id in ["power", "update"] {
        assert!(kept(id, &card_host(Wired::ALL)).is_err(), "{id}");
    }
    for id in ["power", "update"] {
        assert!(kept(id, &host(Wired::ALL)).is_ok(), "{id} linked");
    }
}

#[test]
fn a_card_plugin_never_gets_the_feature_that_reaches_other_plugins_secrets() {
    // `settings` asks for `uses.secrets` and `uses.power` but does not require
    // them: on the card list it is admitted without them, on the linked list it
    // keeps both. Without the bit the host denies the calls it gates
    // (`secrets_service`: only the owner reads, and only `uses.secrets` lists
    // another plugin's names).
    let card = kept("settings", &card_host(Wired::ALL)).unwrap();
    let linked = kept("settings", &host(Wired::ALL)).unwrap();
    assert!(linked.iter().any(|f| f == "uses.secrets"), "{linked:?}");
    for feature in ["uses.secrets", "uses.power", "uses.registry", "uses.wifi"] {
        assert!(!card.iter().any(|f| f == feature), "{feature}: {card:?}");
    }
}
