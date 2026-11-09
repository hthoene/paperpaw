//! The trust matrix of `device.md` 5.2: pinned, rotated, new, sideloaded,
//! revoked, and a digest that is not the named one.
#![allow(clippy::unwrap_used, reason = "fixture errors fail tests")]
use super::*;
use paperpaw_native::signature::{public_key, sign_message};

const A: [u8; 32] = [1; 32];
const B: [u8; 32] = [2; 32];
const C: [u8; 32] = [3; 32];
const R_SEED: [u8; 32] = [7; 32];
const DIGEST: [u8; 32] = [9; 32];

fn recovery() -> [u8; 32] {
    public_key(&R_SEED)
}

/// The rotation from `old` to `new`, signed by the recovery key.
fn step(old: &[u8; 32], new: &[u8; 32]) -> Rotation {
    Rotation {
        to: *new,
        signature: sign_message(rotation_message("dials", old, new).as_bytes(), &R_SEED),
    }
}

fn standing(pinned: Option<[u8; 32]>, recovery: Option<[u8; 32]>) -> Standing {
    Standing {
        pinned,
        recovery,
        revoked: false,
    }
}

fn staged(key: [u8; 32], standing: Standing) -> Staged<'static> {
    Staged {
        id: "dials",
        code: Some((key, DIGEST)),
        standing,
    }
}

fn author(author: Option<[u8; 32]>, recovery: Option<[u8; 32]>, sha: Option<[u8; 32]>) -> Evidence {
    Evidence::new(
        Some(Author {
            author,
            recovery,
            sha256: sha,
        }),
        &[],
    )
    .unwrap()
}

fn rotations(list: &[Rotation]) -> Evidence {
    Evidence::new(None, list).unwrap()
}

fn pins_of(decision: Decision) -> (bool, Pins) {
    match decision {
        Decision::Install { sideload, pins } => (sideload, pins),
        Decision::Refused(why) => panic!("refused with {why}"),
    }
}

#[test]
fn a_key_pinned_here_installs_and_takes_nothing_but_the_code() {
    let s = staged(A, standing(Some(A), Some(recovery())));
    let (sideload, pins) = pins_of(decide(&s, &Evidence::NONE));
    assert!(!sideload);
    assert_eq!(
        (pins.first, pins.rotations().len(), pins.code),
        (None, 0, Some(DIGEST))
    );
    // The catalog's word about the author does not move a pin.
    let (_, pins) = pins_of(decide(&s, &author(Some(B), None, None)));
    assert_eq!(pins.first, None);
}

#[test]
fn a_revoked_pinned_key_never_installs() {
    let mut standing = standing(Some(A), Some(recovery()));
    standing.revoked = true;
    let s = staged(A, standing);
    assert_eq!(
        decide(&s, &Evidence::NONE),
        Decision::Refused(reason::REVOKED)
    );
    assert_eq!(
        decide(&s, &author(Some(A), None, Some(DIGEST))),
        Decision::Refused(7)
    );
}

#[test]
fn a_pin_on_another_key_installs_only_through_rotations_the_recovery_key_signed() {
    let pinned = standing(Some(A), Some(recovery()));
    let s = staged(B, pinned);
    let (sideload, pins) = pins_of(decide(&s, &rotations(&[step(&A, &B)])));
    assert!(!sideload);
    assert_eq!(pins.rotations(), &[step(&A, &B)]);
    assert_eq!(pins.first, None);
    // No rotation, or none that names the signing key: key changed.
    assert_eq!(
        decide(&s, &Evidence::NONE),
        Decision::Refused(reason::KEY_CHANGED)
    );
    assert_eq!(
        decide(&s, &rotations(&[step(&A, &C)])),
        Decision::Refused(6)
    );
    // A step before the pinned key is skipped: X to A, then A to B.
    let chain = [step(&C, &A), step(&A, &B)];
    let (_, pins) = pins_of(decide(&s, &rotations(&chain)));
    assert_eq!(pins.rotations(), &[step(&A, &B)]);
    // Two steps: A to C to B.
    let (_, pins) = pins_of(decide(&s, &rotations(&[step(&A, &C), step(&C, &B)])));
    assert_eq!(pins.rotations().len(), 2);
    // A step after the first taken that does not hold refuses.
    let mut forged = step(&C, &B);
    forged.signature[0] ^= 1;
    assert_eq!(
        decide(&s, &rotations(&[step(&A, &C), forged])),
        Decision::Refused(6)
    );
    // A signature by another key than the recovery key.
    let by_other = Rotation {
        to: B,
        signature: sign_message(rotation_message("dials", &A, &B).as_bytes(), &[8; 32]),
    };
    assert_eq!(decide(&s, &rotations(&[by_other])), Decision::Refused(6));
    // A rotation of another plugin is none of this one's.
    let elsewhere = Rotation {
        to: B,
        signature: sign_message(rotation_message("other", &A, &B).as_bytes(), &R_SEED),
    };
    assert_eq!(decide(&s, &rotations(&[elsewhere])), Decision::Refused(6));
    // No recovery key pinned: only a successor (a new id).
    let s = staged(B, standing(Some(A), None));
    assert_eq!(
        decide(&s, &rotations(&[step(&A, &B)])),
        Decision::Refused(6)
    );
}

#[test]
fn a_new_id_installs_only_if_the_catalog_names_the_signing_key() {
    let s = staged(A, standing(None, None));
    let (sideload, pins) = pins_of(decide(&s, &author(Some(A), Some(recovery()), Some(DIGEST))));
    assert!(!sideload);
    assert_eq!(pins.first, Some((A, Some(recovery()))));
    let (_, pins) = pins_of(decide(&s, &author(Some(A), None, None)));
    assert_eq!(pins.first, Some((A, None)));
    assert_eq!(
        decide(&s, &author(Some(B), None, None)),
        Decision::Refused(reason::NOT_THE_AUTHOR)
    );
    assert_eq!(decide(&s, &author(None, None, None)), Decision::Refused(5));
}

#[test]
fn no_evidence_is_a_sideload_that_pins_the_key_once_confirmed() {
    let s = staged(A, standing(None, None));
    let (sideload, pins) = pins_of(decide(&s, &Evidence::NONE));
    assert!(sideload);
    assert_eq!(pins.first, Some((A, None)));
    // Rotations alone are no author record: still a sideload.
    let (sideload, _) = pins_of(decide(&s, &rotations(&[step(&A, &B)])));
    assert!(sideload);
    // A pinned id is no sideload, though it brings no evidence.
    let s = staged(A, standing(Some(A), None));
    assert!(!pins_of(decide(&s, &Evidence::NONE)).0);
}

#[test]
fn a_digest_other_than_the_named_one_is_reason_18() {
    let s = staged(A, standing(Some(A), None));
    assert_eq!(
        decide(&s, &author(None, None, Some([0; 32]))),
        Decision::Refused(reason::CHANGED)
    );
    assert!(matches!(
        decide(&s, &author(None, None, Some(DIGEST))),
        Decision::Install { .. }
    ));
    // Before any key rule: a revoked key with a wrong digest is still 18.
    let mut standing = standing(Some(A), None);
    standing.revoked = true;
    let s = staged(A, standing);
    assert_eq!(
        decide(&s, &author(None, None, Some([0; 32]))),
        Decision::Refused(18)
    );
}

#[test]
fn a_data_plugin_has_no_author_to_pin_and_no_digest_to_match() {
    let s = Staged {
        id: "dials",
        code: None,
        standing: standing(None, None),
    };
    let (sideload, pins) = pins_of(decide(&s, &Evidence::NONE));
    assert!(!sideload);
    assert_eq!(pins, Pins::NONE);
    // A digest named for code that is not there: the content changed.
    assert_eq!(
        decide(&s, &author(None, None, Some(DIGEST))),
        Decision::Refused(18)
    );
}

#[test]
fn evidence_holds_at_most_six_rotations() {
    let list = [step(&A, &B); ROTATIONS_MAX];
    assert!(Evidence::new(None, &list).is_some());
    let seven = [step(&A, &B); ROTATIONS_MAX + 1];
    assert!(Evidence::new(None, &seven).is_none());
}
