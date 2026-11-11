//! Image admission uses separate limits and never publishes unsupported views.
#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]
extern crate std;
use super::*;
use alloc::{format, string::String, vec, vec::Vec};
use core::{mem::size_of, pin::pin};
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NO_SPACE, OK};
use registration_scope::RegistrationScope;
use registrations::{
    ActionSpec, Record, ScreenSpec, SettingExtra, SettingGroup, SettingSpec, records_decode,
    records_encode,
};
use staged::{IMAGE_DECLARATIONS_MAX, IMAGE_INDEX_MAX_BYTES, SPECS_MAX};

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
fn successful_native_mapping_releases_decoded_raw_and_image_construction_stores() {
    for mode in 0..3 {
        let owner = pin!(HostOwner::new((), ()));
        let scope = pin!(RegistrationScope::<Registration>::new());
        let bytes = encode(&[group("g", "")]);
        let mut construction = owner.as_ref().begin::<Registration>("fixture").unwrap();
        if mode == 0 {
            construction
                .stage_batch(&records_decode(&bytes).unwrap(), |r| r)
                .unwrap();
            assert!(!construction.declarations.is_empty());
        } else {
            if mode == 2 {
                construction.set_image(&bytes).unwrap();
            }
            scope
                .as_ref()
                .scope(&mut construction, || assert_eq!(submit(&scope, &bytes), OK))
                .unwrap();
            assert!(!construction.raw.batches.is_empty());
        }
        let (batch, count) = construction
            .commit_native(
                Teardown {
                    report: |_, _, _| {},
                    cancel: |_| Ok(()),
                    release: |_| Ok(()),
                    destroy: |_| {},
                },
                |view| {
                    let mut count = 0;
                    view.visit(&mut |_| count += 1)?;
                    Ok(count)
                },
            )
            .unwrap();
        assert_eq!(count, 1);
        assert_eq!(batch.declarations.groups().len(), 0);
        assert_eq!(batch.declarations.bytes(), 0);
        assert_eq!(batch.raw.batches.capacity(), 0);
        assert_eq!(batch.raw.index.capacity(), 0);
        assert_eq!(batch.raw.index_bytes, 0);
        drop(batch);
        owner.as_ref().prepare_teardown().unwrap();
        owner.as_ref().finish().unwrap();
    }
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
        construction.finish_declarations().unwrap();
        assert!(matches!(
            construction.commit(crate::native_host::Teardown {
                report: |_, _, _| {},
                cancel: |_| Ok(()),
                release: |_| Ok(()),
                destroy: |_| panic!("never activated")
            }),
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
            assert_eq!(submit(&scope, &duplicate), OK);
        })
        .unwrap();
    assert!(matches!(
        construction.commit(crate::native_host::Teardown {
            report: |_, _, _| {},
            cancel: |_| Ok(()),
            release: |_| Ok(()),
            destroy: |_| {}
        }),
        Err(Error::Invalid)
    ));
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
    assert!(matches!(
        construction.commit(crate::native_host::Teardown {
            report: |_, _, _| {},
            cancel: |_| Ok(()),
            release: |_| Ok(()),
            destroy: |_| {}
        }),
        Err(Error::Invalid)
    ));
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
    let cost = size_of::<raw_declarations::RawBatch>() + size_of::<declaration_index::NameEntry>();
    let fitting = IMAGE_INDEX_MAX_BYTES / cost;
    let (accepted, bytes) = image_limit(fitting + 1, 1);
    assert_eq!(accepted, fitting);
    assert_eq!(bytes, fitting * cost);
    assert!(bytes <= IMAGE_INDEX_MAX_BYTES && bytes + cost > IMAGE_INDEX_MAX_BYTES);
}

#[test]
fn forced_hash_collisions_compare_actual_names_and_still_reject_duplicates() {
    for duplicate in [false, true] {
        let image = if duplicate {
            encode(&[group("a", ""), group("b", ""), group("a", "")])
        } else {
            encode(&[group("a", ""), group("b", ""), group("c", "")])
        };
        let owner = pin!(HostOwner::new((), ()));
        let scope = pin!(RegistrationScope::<Registration>::new());
        let mut construction = owner.as_ref().begin::<Registration>("fixture").unwrap();
        construction.set_image(&image).unwrap();
        construction.raw.force_hash = Some(7);
        scope
            .as_ref()
            .scope(&mut construction, || assert_eq!(submit(&scope, &image), OK))
            .unwrap();
        let result = construction.finish_declarations();
        if duplicate {
            assert_eq!(result, Err(ERR_INVALID));
        } else {
            result.unwrap();
            assert!(
                matches!(construction.raw.find(Some(&image), 0, "b").unwrap(), Some(Record::SettingGroup(group)) if group.name == "b")
            );
            assert!(
                construction
                    .raw
                    .find(Some(&image), 0, "missing")
                    .unwrap()
                    .is_none()
            );
        }
    }
}

#[test]
fn mixed_decoded_and_owned_wire_duplicates_are_rejected_in_both_orders() {
    use registrations::{ResourceFontExtension, ResourceFontFace, ResourceHyphenation};
    let face = Record::ResourceFontFace(ResourceFontFace {
        name: "font",
        path: "fonts/a.ttf",
        family: "Fixture",
        style: 0,
        weight: 400,
        role: "",
        priority: 0,
    });
    let extension = Record::ResourceFontExtension(ResourceFontExtension {
        name: "font",
        path: "fonts/a.ppfp",
        extends: "fixture.other",
    });
    let hyphenation = Record::ResourceHyphenation(ResourceHyphenation {
        language: "en",
        path: "hyph/en.pphp",
    });
    let pairs = [
        (group("other", ""), group("other", "")),
        (spec(), spec()),
        (extra(), extra()),
        (hyphenation, hyphenation),
        (face, face),
        (face, extension),
    ];
    for (first, second) in pairs {
        for wire_first in [false, true] {
            let owner = pin!(HostOwner::new((), ()));
            let scope = pin!(RegistrationScope::<Registration>::new());
            let unrelated_image = [0];
            let mut construction = owner.as_ref().begin::<Registration>("fixture").unwrap();
            construction.set_image(&unrelated_image).unwrap();
            let setup = if matches!(first, Record::SettingExtra(_)) {
                encode(&[group("g", ""), spec()])
            } else {
                encode(&[group("g", "")])
            };
            construction
                .stage_batch(&records_decode(&setup).unwrap(), |r| r)
                .unwrap();
            let first_bytes = encode(&[first]);
            let second_bytes = encode(&[second]);
            if wire_first {
                scope
                    .as_ref()
                    .scope(&mut construction, || {
                        assert_eq!(submit(&scope, &first_bytes), OK)
                    })
                    .unwrap();
                construction
                    .stage_batch(&records_decode(&second_bytes).unwrap(), |r| r)
                    .unwrap();
            } else {
                construction
                    .stage_batch(&records_decode(&first_bytes).unwrap(), |r| r)
                    .unwrap();
                scope
                    .as_ref()
                    .scope(&mut construction, || {
                        assert_eq!(submit(&scope, &second_bytes), OK)
                    })
                    .unwrap();
            }
            assert_eq!(construction.raw.image_entries, 0);
            assert!(matches!(
                construction.commit(crate::native_host::Teardown {
                    report: |_, _, _| {},
                    cancel: |_| Ok(()),
                    release: |_| Ok(()),
                    destroy: |_| panic!("duplicate must never activate")
                }),
                Err(Error::Invalid)
            ));
        }
    }
}

#[test]
fn mixed_decoded_and_owned_wire_distinct_names_remain_valid() {
    for wire_first in [false, true] {
        let owner = pin!(HostOwner::new((), ()));
        let scope = pin!(RegistrationScope::<Registration>::new());
        let unrelated_image = [0];
        let mut construction = owner.as_ref().begin::<Registration>("fixture").unwrap();
        construction.set_image(&unrelated_image).unwrap();
        let decoded = encode(&[group("a", "")]);
        let wire = encode(&[group("b", "")]);
        if wire_first {
            scope
                .as_ref()
                .scope(&mut construction, || assert_eq!(submit(&scope, &wire), OK))
                .unwrap();
        }
        construction
            .stage_batch(&records_decode(&decoded).unwrap(), |r| r)
            .unwrap();
        if !wire_first {
            scope
                .as_ref()
                .scope(&mut construction, || assert_eq!(submit(&scope, &wire), OK))
                .unwrap();
        }
        assert_eq!(construction.declarations().groups().len(), 1);
        construction.finish_declarations().unwrap();
        assert!(matches!(
            construction.commit(crate::native_host::Teardown {
                report: |_, _, _| {},
                cancel: |_| Ok(()),
                release: |_| Ok(()),
                destroy: |_| {}
            }),
            Err(Error::DeclarationViewsUnavailable)
        ));
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Submission {
    Image,
    Owned,
    Decoded,
}
const SUBMISSIONS: [Submission; 3] = [Submission::Image, Submission::Owned, Submission::Decoded];
fn stage_submission(
    construction: &mut Construction<'_, (), (), Registration>,
    scope: core::pin::Pin<&RegistrationScope<Registration>>,
    mode: Submission,
    bytes: &[u8],
) {
    if mode == Submission::Decoded {
        construction
            .stage_batch(&records_decode(bytes).unwrap(), |r| r)
            .unwrap();
    } else {
        scope
            .scope(construction, || assert_eq!(submit(&scope, bytes), OK))
            .unwrap();
    }
}

#[test]
fn forward_extras_resolve_across_every_submission_path_and_mixed_pair() {
    for known_image in [false, true] {
        for extra_mode in SUBMISSIONS {
            for spec_mode in SUBMISSIONS {
                if !known_image
                    && (extra_mode == Submission::Image || spec_mode == Submission::Image)
                {
                    continue;
                }
                let owner = pin!(HostOwner::new((), ()));
                let scope = pin!(RegistrationScope::<Registration>::new());
                let extra_wire = encode(&[extra()]);
                let spec_wire = encode(&[spec()]);
                let mut image = extra_wire.clone();
                image.extend_from_slice(&spec_wire);
                let mut construction = owner.as_ref().begin::<Registration>("fixture").unwrap();
                if known_image {
                    construction.set_image(&image).unwrap();
                }
                stage_submission(
                    &mut construction,
                    scope.as_ref(),
                    Submission::Decoded,
                    &encode(&[group("g", "")]),
                );
                let bytes = if extra_mode == Submission::Image {
                    &image[..extra_wire.len()]
                } else {
                    &extra_wire
                };
                stage_submission(&mut construction, scope.as_ref(), extra_mode, bytes);
                let bytes = if spec_mode == Submission::Image {
                    &image[extra_wire.len()..]
                } else {
                    &spec_wire
                };
                stage_submission(&mut construction, scope.as_ref(), spec_mode, bytes);
                construction.finish_declarations().unwrap();
                let result = construction.commit(crate::native_host::Teardown {
                    report: |_, _, _| {},
                    cancel: |_| Ok(()),
                    release: |_| Ok(()),
                    destroy: |_| {},
                });
                if extra_mode != Submission::Decoded || spec_mode != Submission::Decoded {
                    assert!(matches!(result, Err(Error::DeclarationViewsUnavailable)));
                } else {
                    assert!(result.is_ok());
                }
            }
        }
    }
}

#[test]
fn forward_extra_in_the_same_batch_is_valid_on_every_path() {
    for mode in SUBMISSIONS {
        let owner = pin!(HostOwner::new((), ()));
        let scope = pin!(RegistrationScope::<Registration>::new());
        let wire = encode(&[group("g", ""), extra(), spec()]);
        let image = wire.clone();
        let mut construction = owner.as_ref().begin::<Registration>("fixture").unwrap();
        if mode == Submission::Image {
            construction.set_image(&image).unwrap();
        }
        let bytes = if mode == Submission::Image {
            &image
        } else {
            &wire
        };
        stage_submission(&mut construction, scope.as_ref(), mode, bytes);
        construction.finish_declarations().unwrap();
        let result = construction.commit(crate::native_host::Teardown {
            report: |_, _, _| {},
            cancel: |_| Ok(()),
            release: |_| Ok(()),
            destroy: |_| {},
        });
        if mode != Submission::Decoded {
            assert!(matches!(result, Err(Error::DeclarationViewsUnavailable)));
        } else {
            assert!(result.is_ok());
        }
    }
}

#[test]
fn missing_specs_fail_at_commit_on_every_path_even_if_present_in_image_bytes() {
    for known_image in [false, true] {
        for mode in SUBMISSIONS {
            if !known_image && mode == Submission::Image {
                continue;
            }
            let owner = pin!(HostOwner::new((), ()));
            let scope = pin!(RegistrationScope::<Registration>::new());
            let wire = encode(&[extra()]);
            let mut image = wire.clone();
            image.extend_from_slice(&encode(&[spec()]));
            let mut construction = owner.as_ref().begin::<Registration>("fixture").unwrap();
            if known_image {
                construction.set_image(&image).unwrap();
            }
            stage_submission(
                &mut construction,
                scope.as_ref(),
                Submission::Decoded,
                &encode(&[group("g", "")]),
            );
            let bytes = if mode == Submission::Image {
                &image[..wire.len()]
            } else {
                &wire
            };
            stage_submission(&mut construction, scope.as_ref(), mode, bytes);
            assert!(matches!(
                construction.commit(crate::native_host::Teardown {
                    report: |_, _, _| {},
                    cancel: |_| Ok(()),
                    release: |_| Ok(()),
                    destroy: |_| panic!("missing spec must not activate")
                }),
                Err(Error::Invalid)
            ));
        }
    }
}

fn screen(name: &str) -> Record<'_> {
    Record::ScreenSpec(ScreenSpec {
        name,
        title: "Screen",
        token: 1,
        flags: 0,
    })
}
fn action(name: &str) -> Record<'_> {
    Record::ActionSpec(ActionSpec {
        name,
        title: "Action",
        token: 2,
        flags: 0,
    })
}

fn screens_and_actions(records: &[Record<'_>]) -> Result<(), i32> {
    let image = encode(records);
    let owner = pin!(HostOwner::new((), ()));
    let scope = pin!(RegistrationScope::<Registration>::new());
    let mut construction = owner.as_ref().begin::<Registration>("fixture").unwrap();
    construction.set_image(&image).unwrap();
    scope
        .as_ref()
        .scope(&mut construction, || assert_eq!(submit(&scope, &image), OK))
        .unwrap();
    construction.finish_declarations()?;
    // Indexed: found by kind and name.
    for record in records {
        let (kind, name) = declaration_index::key(*record).unwrap();
        assert!(kind == 5 || kind == 6);
        assert!(
            construction
                .raw
                .find(Some(&image), kind, name)
                .unwrap()
                .is_some()
        );
    }
    Ok(())
}

#[test]
fn screens_and_actions_are_indexed_and_never_declared_twice() {
    // A screen and an action may share a name: they are different kinds.
    screens_and_actions(&[screen("main"), action("main"), screen("other")]).unwrap();
    assert_eq!(
        screens_and_actions(&[screen("main"), action("go"), screen("main")]),
        Err(ERR_INVALID)
    );
    assert_eq!(
        screens_and_actions(&[action("go"), action("go")]),
        Err(ERR_INVALID)
    );
}

fn padded_group(name: &str, total: usize) -> Vec<u8> {
    let wire = encode(&[group(name, "")]);
    assert!(wire[2] < 128);
    let body = &wire[3..];
    // A known record may carry appended fields, skipped by the decoder.
    let body_len = total - 4;
    assert!((128..16384).contains(&body_len));
    let mut padded = vec![
        1,
        wire[1],
        (body_len as u8 & 127) | 128,
        (body_len >> 7) as u8,
    ];
    padded.extend_from_slice(body);
    padded.resize(total, 0);
    padded
}

#[test]
fn owned_wire_uses_one_form_at_exact_32_kib_boundary() {
    let owner = pin!(HostOwner::new((), ()));
    let scope = pin!(RegistrationScope::<Registration>::new());
    let mut construction = owner.as_ref().begin::<Registration>("fixture").unwrap();
    let cost = size_of::<raw_declarations::RawBatch>() + size_of::<declaration_index::NameEntry>();
    for n in 0..8 {
        let remaining =
            staged::DECLARATIONS_MAX_BYTES - n * (registrations::REGISTRATION_MAX_BYTES + cost);
        let total = registrations::REGISTRATION_MAX_BYTES.min(remaining - cost);
        let wire = padded_group(&format!("g{n}"), total);
        scope
            .as_ref()
            .scope(&mut construction, || assert_eq!(submit(&scope, &wire), OK))
            .unwrap();
        assert!(construction.declarations().is_empty());
    }
    assert_eq!(
        construction.raw.retained(None),
        staged::DECLARATIONS_MAX_BYTES
    );
    construction.finish_declarations().unwrap();
    let wire = encode(&[group("overflow", "")]);
    scope
        .as_ref()
        .scope(&mut construction, || {
            assert_eq!(submit(&scope, &wire), ERR_NO_SPACE)
        })
        .unwrap();
    assert_eq!(
        construction.raw.retained(None),
        staged::DECLARATIONS_MAX_BYTES
    );
    assert!(
        construction
            .commit(crate::native_host::Teardown {
                report: |_, _, _| {},
                cancel: |_| Ok(()),
                release: |_| Ok(()),
                destroy: |_| {}
            })
            .is_err()
    );
}

#[test]
fn a_refused_image_batch_fails_the_whole_construction_without_an_owned_fallback() {
    let (image, ranges) = group_image(IMAGE_DECLARATIONS_MAX + 1, 8);
    let owner = pin!(HostOwner::new((), ()));
    let scope = pin!(RegistrationScope::<Registration>::new());
    let mut construction = owner.as_ref().begin::<Registration>("fixture").unwrap();
    construction.set_image(&image).unwrap();
    let mut statuses = Vec::new();
    for (start, len) in ranges {
        let status = scope
            .as_ref()
            .scope(&mut construction, || {
                submit(&scope, &image[start..start + len])
            })
            .unwrap();
        statuses.push(status);
    }
    assert_eq!(statuses.last(), Some(&ERR_NO_SPACE));
    assert!(statuses[..statuses.len() - 1].iter().all(|s| *s == OK));
    // Nothing was copied instead: no batch is staged as an owned copy.
    assert!(
        construction
            .raw
            .batches
            .iter()
            .all(|batch| batch.bytes.is_empty() && batch.image.is_some())
    );
    assert!(construction.declarations().is_empty());
    // The construction is poisoned: it cannot commit.
    assert!(
        construction
            .commit(crate::native_host::Teardown {
                report: |_, _, _| {},
                cancel: |_| Ok(()),
                release: |_| Ok(()),
                destroy: |_| panic!("a refused construction must not activate"),
            })
            .is_err()
    );
}

#[test]
fn a_batch_outside_the_image_range_is_copied_and_never_read_through_the_image() {
    // The X4 gives `[0, code_at)`. A batch that lies past the range (or that the
    // plugin builds elsewhere) is not image-backed: it is copied at submit and
    // counted as an owned copy, so the image is never read for it.
    let inside = encode(&[group("g", "")]);
    let past = encode(&[group("h", "")]);
    let mut all = inside.clone();
    all.extend_from_slice(&past);
    let owner = pin!(HostOwner::new((), ()));
    let scope = pin!(RegistrationScope::<Registration>::new());
    let mut construction = owner.as_ref().begin::<Registration>("fixture").unwrap();
    construction.set_image(&all[..inside.len()]).unwrap();
    for bytes in [&all[..inside.len()], &all[inside.len()..]] {
        let status = scope
            .as_ref()
            .scope(&mut construction, || submit(&scope, bytes))
            .unwrap();
        assert_eq!(status, OK);
    }
    let backed = construction
        .raw
        .batches
        .iter()
        .map(|batch| batch.image.is_some())
        .collect::<Vec<_>>();
    assert_eq!(
        backed,
        [true, false],
        "only the batch inside the range is borrowed"
    );
    assert_eq!(construction.raw.batches[1].bytes.len(), past.len());
    let inside_image = &all[..inside.len()];
    assert!(construction.raw.batches[0].view(Some(inside_image)).is_ok());
    assert_eq!(
        construction.raw.batches[0].view(Some(&inside_image[..inside.len() - 1])),
        Err(ERR_INVALID),
        "a range cut short refuses the batch"
    );
}

/// `count` specs named `s0`, `s1`, … in batches of `per_batch`, as wire bytes.
fn spec_batches(count: usize, per_batch: usize) -> Vec<Vec<u8>> {
    let names: Vec<String> = (0..count).map(|n| format!("s{n}")).collect();
    names
        .chunks(per_batch)
        .map(|chunk| {
            let records: Vec<_> = chunk
                .iter()
                .map(|name| match spec() {
                    Record::SettingSpec(s) => Record::SettingSpec(SettingSpec { name, ..s }),
                    other => other,
                })
                .collect();
            encode(&records)
        })
        .collect()
}

/// How many of `count` specs are accepted: borrowed from `image` or copied.
fn accepted_specs(count: usize, image: bool) -> usize {
    let batches = spec_batches(count, 8);
    let whole: Vec<u8> = batches.concat();
    let owner = pin!(HostOwner::new((), ()));
    let scope = pin!(RegistrationScope::<Registration>::new());
    let mut construction = owner.as_ref().begin::<Registration>("fixture").unwrap();
    if image {
        construction.set_image(&whole).unwrap();
    }
    let mut accepted = 0;
    let mut start = 0;
    for batch in &batches {
        let bytes = &whole[start..start + batch.len()];
        start += batch.len();
        let status = scope
            .as_ref()
            .scope(&mut construction, || submit(&scope, bytes))
            .unwrap();
        if status != OK {
            assert_eq!(status, ERR_NO_SPACE);
            break;
        }
        accepted += 8.min(count - accepted);
    }
    accepted
}

#[test]
fn a_linked_plugin_with_more_than_128_specs_is_borrowed_from_its_image_not_copied() {
    // The widget pack declares 131 settings: past the owned-copy limit of 128
    // (the `Factory(-3)` of a linked plugin hosted without an image), within
    // the image limit.
    let many = SPECS_MAX + 3;
    assert_eq!(accepted_specs(many, true), many);
    assert_eq!(accepted_specs(many, false), SPECS_MAX);
}
