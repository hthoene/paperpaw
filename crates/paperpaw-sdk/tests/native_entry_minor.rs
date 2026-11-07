//! Regression (Battery, #228; Code Reviewer on #256): the exported
//! `native_plugin!` arm, here with `heap = none`, takes a caller constant
//! named like one of the macro's own items (`MINOR`) and exports exactly
//! that metadata. A test crate of its own: the exported entry is one per crate.
use paperpaw_sdk::native::{Error, Host, Plugin};

struct Probe;

impl Plugin for Probe {
    fn make(_: &Host<'_>) -> Result<Self, Error> {
        Ok(Self)
    }
}

const MINOR: &[u8; 32] =
    b"PPAM\x02\0\x20\0\x20\0\0\0\x07\0\x01\0\x01\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0";
paperpaw_sdk::native_plugin!(Probe, metadata = MINOR, heap = none);

#[test]
fn the_exported_entry_takes_a_caller_constant_named_minor() {
    // The macro's own constant used to shadow this one: the entry did not
    // compile (E0391). Now both statics exist and carry the caller's bytes.
    assert_eq!(&PAPERPAW_METADATA, MINOR);
    let _entry = &PAPERPAW_ENTRY;
}
