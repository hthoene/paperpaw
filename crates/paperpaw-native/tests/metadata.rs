//! The shared D1 manifest supplies the feature catalog and verdicts.
#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]
use paperpaw_native::metadata::{self, Feature, Host, Profile, Refusal, Service};
use serde_json::Value;
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../abi/fixtures/metadata")
}
fn json(name: &str) -> Value {
    serde_json::from_slice(&std::fs::read(root().join(name)).unwrap()).unwrap()
}
fn bytes(name: &str) -> Vec<u8> {
    let hex = std::fs::read_to_string(root().join(name)).unwrap();
    let hex = hex.trim();
    (0..hex.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&hex[at..at + 2], 16).unwrap())
        .collect()
}
fn with_host(run: impl FnOnce(&Host<'_>)) {
    let schema = json("schema.json");
    let metadata = &schema["metadata"];
    let profile = &metadata["profiles"][0];
    let features: Vec<_> = metadata["features"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| Feature {
            key: f["key"].as_str().unwrap(),
            revision: f["revision"].as_u64().unwrap() as u16,
        })
        .collect();
    let services: Vec<_> = metadata["services"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| Service {
            key: s["key"].as_str().unwrap(),
            major: s["major"].as_u64().unwrap() as u16,
            semantic_revision: s["semantic_revision"].as_u64().unwrap() as u16,
            min_table_bytes: s["min_table_bytes"].as_u64().unwrap() as u32,
        })
        .collect();
    run(&Host {
        abi_major: schema["major"].as_u64().unwrap() as u16,
        minor: schema["minor"].as_u64().unwrap() as u16,
        profile: Profile {
            target: profile["name"].as_str().unwrap(),
            entry_layout: profile["entry_layout"].as_u64().unwrap() as u16,
            entry_bytes: profile["entry_bytes"].as_u64().unwrap() as u16,
        },
        services: &services,
        features: &features,
    });
}

#[test]
fn feature_manifest_matches_the_bounded_reader() {
    with_host(|host| {
        for case in json("manifest.json")["cases"].as_array().unwrap() {
            let name = case["hex"].as_str().unwrap();
            let bytes = bytes(name);
            let result = metadata::check(&bytes, host);
            assert_eq!(
                result.is_ok(),
                case["expected"]["metadata"] == "accept",
                "{name}: {result:?}"
            );
            if let Some(expected) = case["expected"]["features"].as_array() {
                let expected: Vec<_> = expected.iter().map(|s| s.as_str().unwrap()).collect();
                assert_eq!(
                    result
                        .as_ref()
                        .unwrap()
                        .available_features()
                        .collect::<Vec<_>>(),
                    expected,
                    "{name}"
                );
            }
            if name == "feature-required-newer-revision.hex" {
                assert_eq!(
                    result.as_ref().unwrap_err(),
                    &Refusal::RequiredFeature {
                        key: "test.feature",
                        revision: 3
                    }
                );
            }
            if name == "feature-required-unknown.hex" {
                let Err(Refusal::RequiredFeature { key, .. }) = result else {
                    panic!("missing feature diagnostic")
                };
                assert!(!key.is_empty());
                assert!(host.features.iter().all(|f| f.key != key));
            }
        }
    });
}

#[test]
fn truncations_and_single_byte_flips_never_panic() {
    with_host(|host| {
        for case in json("manifest.json")["cases"].as_array().unwrap() {
            let original = bytes(case["hex"].as_str().unwrap());
            let valid = metadata::check(&original, host).is_ok();
            for end in 0..original.len() {
                let result = metadata::check(&original[..end], host);
                if valid {
                    assert!(result.is_err(), "truncated valid record at {end}");
                }
            }
            for at in 0..original.len() {
                for bit in 0..8 {
                    let mut changed = original.clone();
                    changed[at] ^= 1 << bit;
                    if let Ok(checked) = metadata::check(&changed, host) {
                        assert!(
                            checked
                                .available_features()
                                .all(|key| host.features.iter().any(|f| f.key == key))
                        );
                    }
                }
            }
        }
    });
}

fn finish(bytes: &mut [u8], services: u16, features: u16) {
    let total = bytes.len() as u32;
    bytes[8..12].copy_from_slice(&total.to_le_bytes());
    bytes[26..28].copy_from_slice(&services.to_le_bytes());
    bytes[28..30].copy_from_slice(&features.to_le_bytes());
}
fn service(bytes: &mut Vec<u8>, key: &str, major: u16, revision: u16, table: u32) {
    for value in [(12 + key.len()) as u16, key.len() as u16, major, revision] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&table.to_le_bytes());
    bytes.extend_from_slice(key.as_bytes());
}

#[test]
fn services_match_exactly_and_requirement_keys_are_unique_and_sorted() {
    with_host(|host| {
        let base = bytes("valid-empty-requirements.hex");
        let wanted = host.services.first().unwrap();
        let mut valid = base.clone();
        service(
            &mut valid,
            wanted.key,
            wanted.major,
            wanted.semantic_revision,
            wanted.min_table_bytes,
        );
        finish(&mut valid, 1, 0);
        assert!(metadata::check(&valid, host).is_ok());
        for (major, revision, table) in [
            (
                wanted.major + 1,
                wanted.semantic_revision,
                wanted.min_table_bytes,
            ),
            (
                wanted.major,
                wanted.semantic_revision + 1,
                wanted.min_table_bytes,
            ),
            (
                wanted.major,
                wanted.semantic_revision,
                wanted.min_table_bytes + 1,
            ),
        ] {
            let mut bad = base.clone();
            service(&mut bad, wanted.key, major, revision, table);
            finish(&mut bad, 1, 0);
            assert_eq!(
                metadata::check(&bad, host).unwrap_err(),
                Refusal::Service { key: wanted.key }
            );
        }
        for second in [wanted, host.services.last().unwrap()] {
            let mut bad = base.clone();
            service(
                &mut bad,
                second.key,
                second.major,
                second.semantic_revision,
                second.min_table_bytes,
            );
            service(
                &mut bad,
                wanted.key,
                wanted.major,
                wanted.semantic_revision,
                wanted.min_table_bytes,
            );
            finish(&mut bad, 2, 0);
            assert_eq!(metadata::check(&bad, host).unwrap_err(), Refusal::Malformed);
        }
        let mut bad = base;
        service(&mut bad, "missing.service", 1, 0, 0);
        finish(&mut bad, 1, 0);
        assert_eq!(
            metadata::check(&bad, host).unwrap_err(),
            Refusal::Service {
                key: "missing.service"
            }
        );
    });
}

#[test]
fn declared_services_borrow_validated_records_in_canonical_order() {
    with_host(|host| {
        let mut bytes = bytes("valid-empty-requirements.hex");
        assert_eq!(metadata::check(&bytes, host).unwrap().services().count(), 0);
        let mut wanted: Vec<_> = host.services.iter().collect();
        wanted.sort_unstable_by_key(|service| service.key);
        for wanted in &wanted {
            service(
                &mut bytes,
                wanted.key,
                wanted.major,
                wanted.semantic_revision,
                wanted.min_table_bytes,
            );
        }
        finish(&mut bytes, wanted.len() as u16, 0);
        let checked = metadata::check(&bytes, host).unwrap();
        let expected: Vec<_> = wanted
            .iter()
            .map(|service| metadata::Declared {
                key: service.key,
                major: service.major,
                semantic_revision: service.semantic_revision,
            })
            .collect();
        assert_eq!(checked.services().collect::<Vec<_>>(), expected);
        assert_eq!(checked.services().collect::<Vec<_>>(), expected);
        for service in checked.services() {
            let start = bytes.as_ptr() as usize;
            let key = service.key.as_ptr() as usize;
            assert!((start..start + bytes.len()).contains(&key));
            assert!(key + service.key.len() <= start + bytes.len());
        }
    });
}
