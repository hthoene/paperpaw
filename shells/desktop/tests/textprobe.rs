//! The device harness uses these exact workloads; no device measurements here.
#![allow(clippy::unwrap_used, reason = "tests fail on malformed fixtures")]
#[path = "../../xteink/src/textprobe/workload.rs"]
mod workload;

#[test]
fn device_workloads_are_4096_bytes_and_match_through_the_backend() {
    for case in [
        "ascii",
        "hebrew",
        "rlm-ascii",
        "rlm-alternating",
        "bitmap-ascii",
        "bitmap-A",
    ] {
        let text = workload::fixture(case).unwrap();
        assert_eq!(text.len(), 4096);
        for mode in ["raster", "describing", "blind", "rtl-ink"] {
            for warm in [false, true] {
                let mut outputs = Vec::new();
                for adapter in [false, true] {
                    let fonts = workload::fonts(case).unwrap();
                    fonts.set_describing(mode == "describing");
                    let mut frame = workload::frame(mode).unwrap();
                    if warm {
                        for _ in 0..2 {
                            workload::draw(&fonts, &mut frame, &text, mode, false).unwrap();
                        }
                    }
                    let mut samples = Vec::new();
                    for _ in 0..if warm { 10 } else { 1 } {
                        let advance =
                            workload::draw(&fonts, &mut frame, &text, mode, adapter).unwrap();
                        samples.push((advance.to_bits(), frame.view().data.to_vec()));
                    }
                    outputs.push(samples);
                }
                assert_eq!(outputs[0], outputs[1], "{case}/{mode}/{warm}");
            }
        }
    }
}

#[path = "../../xteink/src/textprobe/config.rs"]
mod config;

#[test]
fn test_command_refuses_unknown_or_unbounded_arguments() {
    let value =
        config::Config::parse("textprobe rlm-alternating adapter warm fragmented blind").unwrap();
    assert_eq!(value.font(), "split-ascii-stored");
    assert_eq!(value.repeats, 1);
    assert_eq!(
        config::Config::parse("textprobe ascii direct warm clean raster repeat10")
            .unwrap()
            .repeats,
        10
    );
    assert!(config::Config::parse("textprobe ascii direct cold clean raster repeat10").is_none());
    for bad in [
        "textprobe",
        "textprobe other direct cold clean raster",
        "textprobe hebrew global cold clean raster",
        "textprobe hebrew direct hot clean raster",
        "textprobe hebrew direct cold random raster",
        "textprobe hebrew direct cold clean outline",
        "textprobe hebrew direct cold clean raster extra",
    ] {
        assert!(config::Config::parse(bad).is_none(), "{bad}");
    }
}

#[path = "../../xteink/src/peak/scoped.rs"]
mod scoped;

#[test]
fn fixtures_match_device_contract_sha256() {
    use paperpaw_core::sdk::digest::{hex, sha256};
    for (case, expected) in [
        (
            "ascii",
            "c93eee2d0db02f10acc7460d9576e122dcf8cd53c4bf8dfcae1b3e74ebcfff5a",
        ),
        (
            "hebrew",
            "6a53c5fc20760eb28b198288ed6668066e89dcf560a06b6ffd99c45b6e860765",
        ),
        (
            "rlm-ascii",
            "2133bde9be9f92a5ce44367b4a6fd8415e69c93830e9e863fb62cf892c0f495a",
        ),
        (
            "rlm-alternating",
            "b608850c0392486b189305f227765f606129f5e83c3a305bd263e6e15dc57028",
        ),
    ] {
        assert_eq!(
            hex::encode(&sha256::digest(workload::fixture(case).unwrap().as_bytes())),
            expected
        );
    }
    let real = include_bytes!("../../../crates/paperpaw-text/testdata/pack.ppfp.v1");
    assert_eq!(
        hex::encode(&sha256::digest(real)),
        "98ac905376d3246a88a605724e3a72f2e8bef359a240c7a0131890eaa938c7f5"
    );
}

#[test]
fn real_stored_control_has_covered_glyphs_and_visible_output() {
    let fonts = workload::fonts("bitmap-A").unwrap();
    let font = fonts.get("probe.font").unwrap();
    assert!(font.has_char('A'));
    assert!(
        !font.has_char('a'),
        "historical lowercase control does not cover stored glyphs"
    );
    let text = workload::fixture("bitmap-A").unwrap();
    let mut frame = workload::frame("raster").unwrap();
    let advance = workload::draw(&fonts, &mut frame, &text, "raster", true).unwrap();
    assert!(advance > 0.0);
    assert!(frame.view().data.iter().any(|&byte| byte != 0));
}

#[path = "../../xteink/src/textprobe/stack_scope.rs"]
mod stack_scope;
