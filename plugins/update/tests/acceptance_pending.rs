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
    // `src/native_check.rs`, `src/native_card.rs`); what is pending is the
    // dual-boot notice (the flag the firmware has to give) and the plugins an
    // update leaves out.
    Case {
        name: "update-from-the-card",
        golden: None,
        blocked_by: &["DUAL-BOOT-FLAG", "LEAVES-OUT"],
    },
    // The update server is asked, an update downloaded and the code plugins
    // built for it fetched. Native: the search, the offer, the download with its
    // progress and size check, the fetch of the builds of installed code plugins
    // (`native_plugin/online.rs`, `fetch.rs`, `native_online.rs`); the weekly
    // look and the check a rule asks for are declared work
    // (`tests/native_online_flow.rs`). Pending: the plugins an update leaves out,
    // and a built-in plugin that comes back as a file.
    Case {
        name: "update-online",
        golden: None,
        blocked_by: &["LEAVES-OUT", "FILE-FOR-NEXT-FIRMWARE"],
    },
    // The page's states beyond the card's and the server's: dual boot (the update
    // that waits for plugins is native, `native_plugin/waiting.rs`) and the badge
    // on its entry in the settings.
    Case {
        name: "update-page",
        golden: None,
        blocked_by: &["ITEM-STATE", "DUAL-BOOT-FLAG"],
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
#[ignore = "BLOCKED DUAL-BOOT-FLAG/LEAVES-OUT: no dual-boot flag; the plugins an update leaves out wait for core.device op 16 leaves_out (device.md 5.1, #732) and its SDK wrapper"]
fn update_from_the_card() {
    CASES[0].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED LEAVES-OUT/FILE-FOR-NEXT-FIRMWARE: the plugins an update leaves out wait for the SDK wrapper of core.device op 16; a built-in plugin that comes back as a file has no host way to be put in place for the next firmware"]
fn update_online() {
    CASES[1].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED ITEM-STATE/DUAL-BOOT-FLAG: the entry's badge waits for CommandSender::set_item_state, and the dual-boot flag is missing"]
fn update_page() {
    CASES[2].refuse().unwrap();
}
