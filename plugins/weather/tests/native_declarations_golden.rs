//! What the plugin declares is a reviewed golden: the batches, framed with
//! their lengths, are `testdata/declarations.bin.v1` byte for byte. A change to
//! a declaration is a change to what the settings page, the widget catalogue
//! and the menus show, so it is reviewed: set `PAPERPAW_BLESS=1` to write the
//! file anew, and read the diff.
#![allow(clippy::unwrap_used, reason = "test assertions")]
#![allow(
    clippy::redundant_closure,
    reason = "the declaring functions are generic over the sink: a closure fixes it"
)]
#[path = "../declarations.rs"]
#[allow(
    dead_code,
    reason = "the build script and the plugin read the same file"
)]
mod declarations;
use paperpaw_sdk::native::declare::{Declarations, DeclareError};

fn framed(
    declare: impl FnOnce(
        &mut Declarations<&mut dyn FnMut(&[u8]) -> Result<(), i32>>,
    ) -> Result<(), DeclareError>,
    out: &mut Vec<u8>,
) {
    let mut sink = |batch: &[u8]| -> Result<(), i32> {
        out.extend_from_slice(&u32::try_from(batch.len()).unwrap().to_le_bytes());
        out.extend_from_slice(batch);
        Ok(())
    };
    let mut d = Declarations::new(&mut sink as &mut dyn FnMut(&[u8]) -> Result<(), i32>);
    declare(&mut d).unwrap();
}

#[test]
fn the_declarations_are_the_reviewed_golden() {
    let mut all = Vec::new();
    framed(|d| declarations::settings(d), &mut all);
    framed(|d| declarations::screens(d), &mut all);
    framed(|d| declarations::navigation(d), &mut all);
    framed(|d| declarations::widgets(d), &mut all);
    framed(|d| declarations::events(d), &mut all);
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/testdata/declarations.bin.v1");
    if std::env::var_os("PAPERPAW_BLESS").is_some() {
        std::fs::write(path, &all).unwrap();
    }
    assert!(
        all == std::fs::read(path).unwrap(),
        "the declarations changed: review, then PAPERPAW_BLESS=1 cargo test"
    );
}
