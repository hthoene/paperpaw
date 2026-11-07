//! `service.dev_check` carries the checked code's or image's digest
//! (`docs/abi/device.md` section 5.1): `sha256` is appended and optional,
//! empty when unsigned, so the record stays within its size row.
#![allow(clippy::unwrap_used, reason = "fixture failures must fail the test")]
use paperpaw_core::native_host::registrations::{
    Record, ServiceDevCheck, record_decode_one, record_encode_one, record_framed_len,
};

fn round_trip(record: Record<'_>) -> usize {
    let mut out = [0u8; 512];
    let len = record_encode_one(&record, &mut out).unwrap();
    assert_eq!(len, record_framed_len(&record).unwrap());
    assert_eq!(record_decode_one(&out[..len]), Ok(record));
    len
}

#[test]
fn the_digest_is_optional_and_fits_the_size_row() {
    let text = "t".repeat(64);
    let check = |sha256: &'static [u8]| {
        Record::ServiceDevCheck(ServiceDevCheck {
            reason: 0,
            needs_draft: 0,
            bytes: u64::MAX,
            signer: &text,
            key: &[7; 32],
            id: &text,
            version: &text,
            flags: 0,
            sha256,
        })
    };
    let unsigned = round_trip(check(&[]));
    let signed = round_trip(check(&[9; 32]));
    // device.md section 7: at most 287 bytes framed, the digest 34 of them.
    assert_eq!(signed, 287);
    assert_eq!(signed - unsigned, 34);
}
