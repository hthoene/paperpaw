//! Image admission uses separate limits and never publishes unsupported views.
#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]
extern crate std;
use super::*;
use alloc::{format, string::String, vec, vec::Vec};
use core::{mem::size_of, pin::pin};
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NO_SPACE, OK};
use registration_scope::RegistrationScope;
use registrations::{
    Record, SettingExtra, SettingGroup, SettingSpec, records_decode, records_encode,
};
use staged::{IMAGE_DECLARATIONS_MAX, IMAGE_INDEX_MAX_BYTES};

fn encode(records: &[Record<'_>]) -> Vec<u8> {
    let mut bytes = vec![0; registrations::REGISTRATION_MAX_BYTES];
    let len = records_encode(records, &mut bytes).unwrap();
    bytes.truncate(len);
    bytes
}
fn group<'a>(name: &'a str, parent: &'a str) -> Record<'a> {
    Record::SettingGroup(SettingGroup {
        name,
        title: "Group",
        order: 0,
        parent,
        page: "",
    })
}
fn spec() -> Record<'static> {
    Record::SettingSpec(SettingSpec {
        name: "flag",
        kind: 0,
        flags: 0,
        group: "fixture.g",
        title: "Flag",
        help: "",
        default: &[1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1],
        min: 0,
        max: 0,
        choices: "",
    })
}
fn extra() -> Record<'static> {
    Record::SettingExtra(SettingExtra {
        name: "flag",
        unit: "",
        section: "",
        follows: "",
        shown_id: "",
        shown_value: "",
        editor_kind: 0,
        editor: "",
        step: 0,
        flags: 0,
        check: 0,
    })
}
fn submit(scope: &RegistrationScope<Registration>, bytes: &[u8]) -> i32 {
    // SAFETY: initialized bytes stay stable until this synchronous call returns.
    unsafe { registrations::submit(scope, bytes.as_ptr(), bytes.len()) }
}

#[test]
fn image_setup_is_once_only_before_any_submission_including_empty_images() {
    let owner = pin!(HostOwner::new((), ()));
    let mut construction = owner.as_ref().begin::<Registration>("fixture").unwrap();
    construction.set_image(&[]).unwrap();
    assert_eq!(construction.set_image(&[]), Err(Error::State));
    construction.abort();
    let mut construction = owner.as_ref().begin::<Registration>("fixture").unwrap();
    let empty = records_decode(&[0]).unwrap();
    construction.stage_batch(&empty, |r| r).unwrap();
    assert_eq!(construction.set_image(&[]), Err(Error::State));
    construction.abort();
    let scope = pin!(RegistrationScope::<Registration>::new());
    let mut construction = owner.as_ref().begin::<Registration>("fixture").unwrap();
    scope
        .as_ref()
        .scope(&mut construction, || {
            assert_eq!(submit(&scope, &[0xff]), ERR_INVALID)
        })
        .unwrap();
    assert_eq!(construction.set_image(&[]), Err(Error::State));
}

#[test]
fn mixed_image_and_owned_declarations_share_reference_and_duplicate_checks() {
    for image_spec in [false, true] {
        let owner = pin!(HostOwner::new((), ()));
        let scope = pin!(RegistrationScope::<Registration>::new());
        let image = if image_spec {
            encode(&[group("g", ""), spec()])
        } else {
            encode(&[group("g", "")])
        };
        let owned = if image_spec {
            encode(&[extra()])
        } else {
            encode(&[spec(), extra()])
        };
        let mut construction = owner.as_ref().begin::<Registration>("fixture").unwrap();
        construction.set_image(&image).unwrap();
        scope
            .as_ref()
            .scope(&mut construction, || {
                assert_eq!(submit(&scope, &image), OK);
                assert_eq!(submit(&scope, &owned), OK);
            })
            .unwrap();
        assert!(construction.raw.batches[0].bytes.is_empty());
        assert!(!construction.raw.batches[1].bytes.is_empty());
        construction.declaration_view().finish().unwrap();
        assert!(matches!(
            construction.commit(|_| panic!("never activated")),
            Err(Error::DeclarationViewsUnavailable)
        ));
    }
    let owner = pin!(HostOwner::new((), ()));
    let scope = pin!(RegistrationScope::<Registration>::new());
    let image = encode(&[group("g", "")]);
    let duplicate = image.clone();
    let mut construction = owner.as_ref().begin::<Registration>("fixture").unwrap();
    construction.set_image(&image).unwrap();
    scope
        .as_ref()
        .scope(&mut construction, || {
            assert_eq!(submit(&scope, &image), OK);
            assert_eq!(submit(&scope, &duplicate), ERR_INVALID);
        })
        .unwrap();
    assert!(matches!(construction.commit(|_| {}), Err(Error::Invalid)));
}

#[test]
fn group_cycle_across_image_and_owned_batches_is_rejected_before_activation() {
    let owner = pin!(HostOwner::new((), ()));
    let scope = pin!(RegistrationScope::<Registration>::new());
    let image = encode(&[group("g", "fixture.h")]);
    let owned = encode(&[group("h", "fixture.g")]);
    let mut construction = owner.as_ref().begin::<Registration>("fixture").unwrap();
    construction.set_image(&image).unwrap();
    scope
        .as_ref()
        .scope(&mut construction, || {
            assert_eq!(submit(&scope, &image), OK);
            assert_eq!(submit(&scope, &owned), OK);
        })
        .unwrap();
    assert!(matches!(construction.commit(|_| {}), Err(Error::Invalid)));
}

fn group_image(count: usize, per_batch: usize) -> (Vec<u8>, Vec<(usize, usize)>) {
    let names: Vec<String> = (0..count).map(|n| format!("g{n}")).collect();
    let mut image = Vec::new();
    let mut ranges = Vec::new();
    for chunk in names.chunks(per_batch) {
        let records: Vec<_> = chunk.iter().map(|name| group(name, "")).collect();
        let wire = encode(&records);
        ranges.push((image.len(), wire.len()));
        image.extend_from_slice(&wire);
    }
    (image, ranges)
}
fn image_limit(count: usize, per_batch: usize) -> (usize, usize) {
    let (image, ranges) = group_image(count, per_batch);
    let owner = pin!(HostOwner::new((), ()));
    let scope = pin!(RegistrationScope::<Registration>::new());
    let mut construction = owner.as_ref().begin::<Registration>("fixture").unwrap();
    construction.set_image(&image).unwrap();
    let mut accepted = 0;
    for (start, len) in ranges {
        let status = scope
            .as_ref()
            .scope(&mut construction, || {
                submit(&scope, &image[start..start + len])
            })
            .unwrap();
        if status != OK {
            assert_eq!(status, ERR_NO_SPACE);
            break;
        }
        accepted += per_batch.min(count - accepted);
    }
    assert!(construction.declarations().is_empty());
    assert!(
        construction
            .raw
            .batches
            .iter()
            .all(|batch| batch.bytes.is_empty())
    );
    (accepted, construction.raw.index_bytes)
}
#[test]
fn image_entry_limit_accepts_512_and_refuses_the_513th() {
    assert_eq!(IMAGE_DECLARATIONS_MAX, 512);
    assert_eq!(
        image_limit(IMAGE_DECLARATIONS_MAX, 8).0,
        IMAGE_DECLARATIONS_MAX
    );
    assert_eq!(
        image_limit(IMAGE_DECLARATIONS_MAX + 1, 8).0,
        IMAGE_DECLARATIONS_MAX
    );
}
#[test]
fn image_index_budget_accepts_the_last_fitting_slot_and_refuses_the_next() {
    assert_eq!(IMAGE_INDEX_MAX_BYTES, 8192);
    let cost = size_of::<raw_declarations::RawBatch>() + 4;
    let fitting = IMAGE_INDEX_MAX_BYTES / cost;
    let (accepted, bytes) = image_limit(fitting + 1, 1);
    assert_eq!(accepted, fitting);
    assert_eq!(bytes, fitting * cost);
    assert!(bytes <= IMAGE_INDEX_MAX_BYTES && bytes + cost > IMAGE_INDEX_MAX_BYTES);
}
