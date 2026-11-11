//! Acceptance of the interface-7 port, pending on the host. The interface-6
//! fonts browser is pinned by `install.rs` and the unit tests; these cases
//! are the same behaviours through the production loader with the native
//! plugin. They never pass by themselves.
#![allow(clippy::unwrap_used, reason = "acceptance assertions")]
#[path = "../../test-support/pending.rs"]
mod pending;
use pending::Case;

const CASES: [Case; 7] = [
    // The list of fonts (fetched with `core.net`, kept in the data folder), the
    // font's page with Download / Update / Remove, and choosing a font draw as before.
    Case {
        name: "fonts-browser-screens",
        golden: None,
        blocked_by: &["H1", "H4"],
    },
    // Downloading a font into the data folder and having it installed or
    // removed (the plugin does this through `core.net`, `command.install_plugin`
    // and `command.remove_plugin`); the host asks the user first (decisions 132,
    // 124) and its question is not wired to native plugins yet.
    Case {
        name: "fonts-browser-install",
        golden: None,
        blocked_by: &["H2"],
    },
    // The sample sentence on a font's page drawn in the font's own face, from a
    // sample pack downloaded to the data folder (`Font::acquire_data`). The
    // path, characters and address are tested without a host (`native_sample`);
    // the drawing needs a hosted run with a catalog and the network.
    Case {
        name: "fonts-browser-samples",
        golden: None,
        blocked_by: &["H1", "H4"],
    },
    // Coming back from a font's page to the list fetches nothing again, and the
    // page has its font from the list (the catalog kept is not read again).
    Case {
        name: "fonts-browser-list-back",
        golden: None,
        blocked_by: &["H1", "H4"],
    },
    // Updating the font one reads with keeps it chosen.
    Case {
        name: "fonts-browser-update-keeps-font",
        golden: None,
        blocked_by: &["H2"],
    },
    // A download cut off by a restart is dropped (`make` clears the staging
    // folder; the host's `install_done` ends one that was asked).
    Case {
        name: "fonts-browser-restart-drops-download",
        golden: None,
        blocked_by: &["H2"],
    },
    // *Your own fonts* shows the plugin's help part in the reader.
    Case {
        name: "fonts-browser-own-fonts",
        golden: None,
        blocked_by: &["draft-22-doc_open-root-5"],
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
#[ignore = "BLOCKED H1/H4: native screen commands and input are not qualified"]
fn fonts_browser_screens() {
    CASES[0].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H2: the host's install and removal questions are not wired to native plugins"]
fn fonts_browser_install() {
    CASES[1].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H4: native screens and input are not qualified, no hosted run with a catalog"]
fn fonts_browser_samples() {
    CASES[2].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H4: native screen commands and input are not qualified"]
fn fonts_browser_list_back() {
    CASES[3].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H2: the host's install question is not wired to native plugins"]
fn fonts_browser_update_keeps_font() {
    CASES[4].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H2: the host's install question is not wired to native plugins"]
fn fonts_browser_restart_drops_download() {
    CASES[5].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED draft-22-doc_open-root-5: command.open into core.reader (root 5, handed) is not there"]
fn fonts_browser_own_fonts() {
    CASES[6].refuse().unwrap();
}
