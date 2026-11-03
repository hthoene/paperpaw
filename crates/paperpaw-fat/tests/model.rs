//! `FatStorage` and `ExFatStorage` against a model in memory: random
//! writes, appends,
//! reads at any offset, removals and moves of files in a few folders,
//! with data crossing cluster and block boundaries. After every step the
//! storage reads as the model says; after a sync, so does the card
//! mounted anew from its bytes.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use std::collections::BTreeMap;

use common::{Card, exfat, exfat_card, read};
use paperpaw_platform::{Storage, StorageError};
use proptest::prelude::*;

const PATHS: [&str; 6] = [
    "a.txt",
    "b.bin",
    "books/c.epub",
    "books/deep/d.txt",
    "notes/e.txt",
    "notes/f with a long name.txt",
];

#[derive(Debug, Clone)]
enum Op {
    Write(usize, Vec<u8>),
    Append(usize, Vec<u8>),
    Read(usize, u64, usize),
    Remove(usize),
    Rename(usize, usize),
    Sync,
}

fn data() -> impl Strategy<Value = Vec<u8>> {
    prop_oneof![
        prop::collection::vec(any::<u8>(), 0..100),
        // Around blocks and clusters, up to several of them.
        (0usize..20_000, any::<u8>()).prop_map(|(n, seed)| {
            (0..n)
                .map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed))
                .collect()
        }),
    ]
}

fn op() -> impl Strategy<Value = Op> {
    let path = 0..PATHS.len();
    prop_oneof![
        3 => (path.clone(), data()).prop_map(|(p, d)| Op::Write(p, d)),
        3 => (path.clone(), data()).prop_map(|(p, d)| Op::Append(p, d)),
        3 => (path.clone(), 0u64..25_000, 0usize..5000).prop_map(|(p, o, n)| Op::Read(p, o, n)),
        1 => path.clone().prop_map(Op::Remove),
        1 => (path.clone(), path).prop_map(|(a, b)| Op::Rename(a, b)),
        1 => Just(Op::Sync),
    ]
}

/// Every file of the model, read whole from `storage`; missing ones are
/// missing.
fn check(storage: &dyn Storage, model: &BTreeMap<&str, Vec<u8>>) -> Result<(), TestCaseError> {
    for path in PATHS {
        match model.get(path) {
            Some(data) => {
                prop_assert_eq!(storage.size(path), Ok(data.len() as u64), "{}", path);
                prop_assert!(read(storage, path) == *data, "{} differs", path);
            }
            None => prop_assert!(!storage.exists(path), "{} is there", path),
        }
    }
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(48)))]

    #[test]
    fn storage_follows_the_model(ops in prop::collection::vec(op(), 1..40)) {
        let card = Card::new(2 << 20);
        let model = follows(card.storage, ops)?;
        check(card.remounted().storage, &model)?;
    }

    #[test]
    fn exfat_storage_follows_the_model(ops in prop::collection::vec(op(), 1..40)) {
        let bytes = exfat_card();
        let model = follows(&exfat(&bytes), ops)?;
        check(&exfat(&bytes), &model)?;
    }
}

/// Runs `ops` on `s` and the model, checking after each, then syncs; the
/// model, for the card mounted anew to read as it says too.
fn follows(
    s: &dyn Storage,
    ops: Vec<Op>,
) -> Result<BTreeMap<&'static str, Vec<u8>>, TestCaseError> {
    let mut model: BTreeMap<&'static str, Vec<u8>> = BTreeMap::new();
    for op in ops {
        match op {
            Op::Write(p, d) => {
                s.write(PATHS[p], &d).unwrap();
                model.insert(PATHS[p], d);
            }
            Op::Append(p, d) => {
                s.append(PATHS[p], &d).unwrap();
                model.entry(PATHS[p]).or_default().extend_from_slice(&d);
            }
            Op::Read(p, offset, len) => {
                let mut buf = vec![0u8; len];
                let result = s.read_at(PATHS[p], offset, &mut buf);
                match model.get(PATHS[p]) {
                    Some(data) => {
                        let start = (offset as usize).min(data.len());
                        let end = (start + len).min(data.len());
                        prop_assert_eq!(result, Ok(end - start));
                        prop_assert!(buf[..end - start] == data[start..end]);
                    }
                    None => prop_assert_eq!(result, Err(StorageError::NotFound)),
                }
            }
            Op::Remove(p) => {
                s.remove(PATHS[p]).unwrap();
                model.remove(PATHS[p]);
            }
            Op::Rename(a, b) => {
                let result = s.rename(PATHS[a], PATHS[b]);
                if a == b {
                    prop_assert_eq!(result, Ok(()));
                } else if let Some(data) = model.remove(PATHS[a]) {
                    prop_assert_eq!(result, Ok(()));
                    model.insert(PATHS[b], data);
                } else {
                    prop_assert_eq!(result, Err(StorageError::NotFound));
                }
            }
            Op::Sync => s.sync().unwrap(),
        }
        check(s, &model)?;
    }
    s.sync().unwrap();
    Ok(model)
}
