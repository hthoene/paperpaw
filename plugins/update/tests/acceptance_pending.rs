//! Acceptance of the interface-7 port, pending on the host. The interface-6
//! update is pinned by `states.rs` and `page/` on a kernel and by the goldens;
//! these cases are the same behaviours through the production loader with the
//! native plugin. They never pass by themselves.
#![allow(clippy::unwrap_used, reason = "acceptance assertions")]
#[path = "../../test-support/pending.rs"]
mod pending;
use pending::Case;

const CASES: [Case; 4] = [
    // An update on the card is found, its version and who signed it said (the
    // device's file check), installed by the device; after the restart the
    // plugin says how it went. The page, the install, its end and the restart
    // message are native (`src/native_plugin.rs`, `src/native_card.rs`); what is
    // pending is the file check, the own key and the foreign image.
    Case {
        name: "update-from-the-card",
        golden: None,
        blocked_by: &["CHECK-FILE", "DUAL-BOOT-FLAG"],
    },
    // The update server is asked, an update downloaded and the code plugins
    // built for it fetched.
    Case {
        name: "update-online",
        golden: None,
        blocked_by: &["NET-flow", "CHECK-FILE"],
    },
    // The weekly look runs as declared work at a cheap moment.
    Case {
        name: "update-weekly-look",
        golden: None,
        blocked_by: &["WORK-declaration", "NET"],
    },
    // The page's states beyond the card's files (online, waiting, dual boot)
    // and the badge on its entry in the settings.
    Case {
        name: "update-page",
        golden: None,
        blocked_by: &["ITEM-STATE", "NET-flow", "DUAL-BOOT-FLAG"],
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
#[ignore = "BLOCKED CHECK-FILE/DUAL-BOOT-FLAG: no CommandSender::check_file (commands 1592) to ask what an update file is, no dual-boot flag"]
fn update_from_the_card() {
    CASES[0].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED NET-flow/CHECK-FILE: the online flow (check, download, plugin fetch) is not native yet"]
fn update_online() {
    CASES[1].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED WORK-declaration/NET: declared work and the network are not qualified for the native plugin"]
fn update_weekly_look() {
    CASES[2].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED ITEM-STATE/NET-flow/DUAL-BOOT-FLAG: the entry's badge and the online and dual-boot states are not native yet"]
fn update_page() {
    CASES[3].refuse().unwrap();
}
