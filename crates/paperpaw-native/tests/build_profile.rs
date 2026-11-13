//! The build's profile and the retargeted metadata of linked plugins
//! (Decision 172): every profile `abi/abi.def` declares is chosen for its
//! target and nothing else, and a plugin's metadata set to a profile is the
//! generator's own bytes for that profile.
#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]
use paperpaw_native::metadata::{self, BUILD_PROFILE, Profile};
use serde_json::Value;
use std::path::PathBuf;

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Every declared profile: name, entry layout, entry bytes.
fn declared() -> Vec<(String, u16, u16)> {
    let schema: Value =
        serde_json::from_slice(&std::fs::read(repo().join("abi/abi.def")).unwrap()).unwrap();
    schema["metadata"]["profiles"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            (
                p["name"].as_str().unwrap().to_owned(),
                p["entry_layout"].as_u64().unwrap() as u16,
                p["entry_bytes"].as_u64().unwrap() as u16,
            )
        })
        .collect()
}

/// The generator's metadata for `profile` (`abi/fixtures/entry`).
fn fixture(profile: &str) -> Vec<u8> {
    let hex =
        std::fs::read_to_string(repo().join(format!("abi/fixtures/entry/{profile}.hex"))).unwrap();
    let hex = hex.trim();
    (0..hex.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&hex[at..at + 2], 16).unwrap())
        .collect()
}

#[test]
fn every_declared_profile_is_chosen_for_one_target_and_only_those() {
    let source =
        std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/profile.rs"))
            .unwrap();
    // `profile("name", entry_bytes)`: each declared profile once, as declared.
    let mut chosen: Vec<(String, u16, u16)> = source
        .split("profile(\"")
        .skip(1)
        .map(|rest| {
            let (name, rest) = rest.split_once("\", ").unwrap();
            let (bytes, _) = rest.split_once(')').unwrap();
            (name.to_owned(), 1, bytes.parse().unwrap())
        })
        .collect();
    chosen.sort();
    let mut declared = declared();
    declared.sort();
    assert_eq!(chosen, declared);
}

#[test]
fn this_build_has_the_linux_profile() {
    // The tests run on Linux x86-64 (CI, the team's machines).
    let profile = BUILD_PROFILE.expect("a profile for this build");
    assert_eq!(profile.target, "linux-x86_64-sysv64-le");
    assert_eq!(profile.entry_bytes, 32);
    assert_eq!(profile.entry_layout, 1);
}

#[test]
fn metadata_set_to_a_profile_is_the_generators_for_it() {
    let declared = declared();
    for (from, ..) in &declared {
        let source = fixture(from);
        for (to, layout, bytes) in &declared {
            let profile = Profile {
                target: to,
                entry_layout: *layout,
                entry_bytes: *bytes,
            };
            let len = metadata::retargeted_len(&source, &profile);
            let wanted = fixture(to);
            assert_eq!(len, wanted.len(), "{from} -> {to}");
            let made = retarget_padded(&source, &profile);
            assert_eq!(&made[..len], wanted.as_slice(), "{from} -> {to}");
        }
    }
}

/// [`metadata::retarget`] for a length known only at run time: retargets
/// into an array of exactly that length, picked from a few sizes.
fn retarget_padded(source: &[u8], profile: &Profile<'_>) -> [u8; 160] {
    fn into<const N: usize>(source: &[u8], profile: &Profile<'_>) -> [u8; 160] {
        let made: [u8; N] = metadata::retarget(source, profile);
        let mut out = [0; 160];
        out[..N].copy_from_slice(&made);
        out
    }
    macro_rules! lengths {
        ($($n:literal)*) => {
            match metadata::retargeted_len(source, profile) {
                $($n => into::<$n>(source, profile),)*
                other => panic!("no case for length {other}"),
            }
        };
    }
    lengths!(70 71 72 73 74 75 76 77 78 79 80 81 82 83 84 85 86 87 88 89 90 91 92 93 94 95 96
             97 98 99 100 101 102 103 104 105 106 107 108 109 110)
}

#[test]
fn a_record_that_is_not_format_2_is_refused() {
    let mut source = fixture("linux-x86_64-sysv64-le");
    source[4] = 3;
    let profile = BUILD_PROFILE.unwrap();
    let refused = std::panic::catch_unwind(|| metadata::retargeted_len(&source, &profile));
    assert!(refused.is_err());
}
