//! Acceptance of the interface-7 port, pending on the host. The interface-6
//! fonts browser is pinned by `install.rs` and the unit tests; these cases
//! are the same behaviours through the production loader with the native
//! plugin. They never pass by themselves.
#![allow(clippy::unwrap_used, reason = "acceptance assertions")]
#[path = "../../test-support/pending.rs"]
mod pending;
use pending::Case;

const CASES: [Case; 3] = [
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
    // Every font's name and the sample sentence drawn in the font's own face,
    // from a sample pack downloaded at run time: fonts are declared in the
    // plugin's metadata (`resource.font_face`), none can be loaded later.
    Case {
        name: "fonts-browser-samples",
        golden: None,
        blocked_by: &["FONT-runtime"],
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
#[ignore = "BLOCKED FONT-runtime: no host path to draw with a font loaded at run time"]
fn fonts_browser_samples() {
    CASES[2].refuse().unwrap();
}
