//! Acceptance of the interface-7 port, pending on the host: the behaviours of
//! the update (the interface-6 plugin is removed; its tests drove a kernel)
//! through the production loader with the native plugin. They never pass by
//! themselves.
#![allow(clippy::unwrap_used, reason = "acceptance assertions")]
#[path = "../../test-support/pending.rs"]
mod pending;
use pending::Case;

const CASES: [Case; 3] = [
    // An update on the card is found, its version and who signed it said (the
    // device's file check), installed by the device; after the restart the
    // plugin says how it went. All of it is native (`src/native_plugin.rs`,
    // `src/native_check.rs`, `src/native_card.rs`), with the plugins the update
    // leaves out (`native_plugin/leaves.rs`); what is pending is the dual-boot
    // notice (the flag the firmware has to give).
    Case {
        name: "update-from-the-card",
        golden: None,
        blocked_by: &["DUAL-BOOT-FLAG"],
    },
    // The update server is asked, an update downloaded and the code plugins
    // built for it fetched. Native: the search, the offer, the download with its
    // progress and size check, the fetch of the builds of installed code plugins
    // (`native_plugin/online.rs`, `fetch.rs`, `native_online.rs`); the weekly
    // look and the check a rule asks for are declared work
    // (`tests/native_online_flow.rs`), and the plugins the update leaves out are
    // named and may be left behind. Pending: a built-in plugin that comes back as a
    // file.
    Case {
        name: "update-online",
        golden: None,
        blocked_by: &["FILE-FOR-NEXT-FIRMWARE"],
    },
    // The page's states beyond the card's and the server's: dual boot (the update
    // that waits for plugins and the *New* mark on its entry in the settings are
    // native, `native_plugin/waiting.rs`, `native_plugin/leaves.rs`).
    Case {
        name: "update-page",
        golden: None,
        blocked_by: &["DUAL-BOOT-FLAG"],
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
#[ignore = "BLOCKED DUAL-BOOT-FLAG: no dual-boot flag in dev_firmware or dev_device (the notice waits for the firmware)"]
fn update_from_the_card() {
    CASES[0].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED FILE-FOR-NEXT-FIRMWARE: a built-in plugin that comes back as a file has no host way to be put in place for the next firmware"]
fn update_online() {
    CASES[1].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED DUAL-BOOT-FLAG: the dual-boot flag is missing"]
fn update_page() {
    CASES[2].refuse().unwrap();
}
