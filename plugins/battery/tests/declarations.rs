//! The settings of the battery plugin as interface-7 declaration records: the golden
//! batches Kernel's staging (S2) is tested with, built from the plugin's real
//! interface-6 settings with the SDK's declaration builder.
//!
//! `testdata/declarations.bin.v1` holds the batches in order, each as a
//! 4-byte little-endian length and then that many bytes of one encoded batch
//! (what `registrations.submit` takes). The test rebuilds them and compares
//! byte for byte; `PAPERPAW_BLESS_DECLARATIONS=1` rewrites the file, and only
//! when someone sets it on purpose after reading the diff.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_core::native_host::{
    declarations::validate_batch,
    registrations::{Record, records_decode},
};
use paperpaw_plugin_battery::Battery;
use paperpaw_sdk::native::declare::Declarations;
use paperpaw_sdk::{Plugin, Registrar};

const PLUGIN: &str = "battery";
const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/testdata/declarations.bin.v1");

fn built() -> Vec<Vec<u8>> {
    let mut r = Registrar::new(paperpaw_sdk::Id::new_static(PLUGIN));
    Battery.register(&mut r).unwrap();
    Declarations::from_registrar(PLUGIN, r)
        .unwrap()
        .batches()
        .unwrap()
}

fn framed(batches: &[Vec<u8>]) -> Vec<u8> {
    let mut out = Vec::new();
    for batch in batches {
        out.extend_from_slice(&u32::try_from(batch.len()).unwrap().to_le_bytes());
        out.extend_from_slice(batch);
    }
    out
}

#[test]
fn every_batch_is_valid_for_the_host() {
    let batches = built();
    let mut specs = 0;
    for batch in &batches {
        let decoded = records_decode(batch).unwrap();
        validate_batch(PLUGIN, &decoded).unwrap();
        specs += decoded
            .iter()
            .filter(|r| matches!(r, Record::SettingSpec(_)))
            .count();
    }
    // One record for each setting the plugin registers.
    assert_eq!(specs, 2);
}

#[test]
fn the_golden_batches_are_what_the_builder_writes() {
    let now = framed(&built());
    if std::env::var_os("PAPERPAW_BLESS_DECLARATIONS").is_some() {
        std::fs::write(FIXTURE, &now).unwrap();
    }
    let golden = std::fs::read(FIXTURE).expect("testdata/declarations.bin.v1");
    assert!(
        golden == now,
        "the battery declaration batches changed: read the difference, then run with PAPERPAW_BLESS_DECLARATIONS=1"
    );
}
