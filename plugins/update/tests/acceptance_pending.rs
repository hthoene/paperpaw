//! Acceptance of the interface-7 port, pending on the host. The interface-6
//! update is pinned by `states.rs` and `page/` on a kernel and by the goldens;
//! these cases are the same behaviours through the production loader with the
//! native plugin. They never pass by themselves.
#![allow(clippy::unwrap_used, reason = "acceptance assertions")]
#[path = "../../test-support/pending.rs"]
mod pending;
use pending::Case;

const CASES: [Case; 4] = [
    // An update on the card is found, said who signed it, and installed by the
    // shell; after the restart the plugin says how it went.
    Case {
        name: "update-from-the-card",
        golden: None,
        blocked_by: &["FIRMWARE-service", "FS", "H1"],
    },
    // The update server is asked, an update downloaded and the code plugins
    // built for it fetched.
    Case {
        name: "update-online",
        golden: None,
        blocked_by: &["NET", "FS", "FIRMWARE-service"],
    },
    // The weekly look runs as declared work at a cheap moment.
    Case {
        name: "update-weekly-look",
        golden: None,
        blocked_by: &["WORK-declaration", "NET"],
    },
    // The page and the badge on its entry in the settings.
    Case {
        name: "update-page",
        golden: None,
        blocked_by: &["H1", "H2", "H3", "ITEM-STATE"],
    },
];

#[test]
fn the_cases_name_what_they_wait_for() {
    for case in &CASES {
        assert!(case.golden_exists());
        let error = case.refuse().unwrap_err();
        assert!(error.starts_with("BLOCKED "), "{error}");
        assert!(error.contains(case.blocked_by[0]), "{error}");
    }
}

#[test]
#[ignore = "BLOCKED FIRMWARE-service/FS/H1: no native firmware service, files or screen commands yet"]
fn update_from_the_card() {
    CASES[0].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED NET/FS/FIRMWARE-service: network, files and the firmware service are not on integration"]
fn update_online() {
    CASES[1].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED WORK-declaration/NET: declared work and the network are not qualified for the native plugin"]
fn update_weekly_look() {
    CASES[2].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H2/H3/ITEM-STATE: screen commands, dialogs, composition and a menu item's badge are not qualified"]
fn update_page() {
    CASES[3].refuse().unwrap();
}
