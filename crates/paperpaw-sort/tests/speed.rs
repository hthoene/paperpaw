//! How the shared sorts compare with the standard library's on library-sized
//! lists (2,000 and 10,000 entries). Prints the times; run with
//! `cargo test --release -p paperpaw-sort --test speed -- --ignored --nocapture`.
#![allow(
    clippy::unnecessary_sort_by,
    reason = "the closures mirror the callers being compared"
)]
use paperpaw_sort::{sort, sort_by, sort_by_cached_key, sort_by_key, sort_unstable};
use std::time::{Duration, Instant};

fn time(mut run: impl FnMut()) -> Duration {
    run();
    let start = Instant::now();
    for _ in 0..20 {
        run();
    }
    start.elapsed() / 20
}

#[test]
#[ignore = "prints timings"]
fn library_sized_lists() {
    for len in [2_000_u32, 10_000] {
        let mut seed = 12_345_u64;
        let mut next = || {
            seed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            u32::try_from(seed >> 33).unwrap()
        };
        let titles: Vec<String> = (0..len)
            .map(|_| format!("The Book {} of {}", next() % 5000, next() % 997))
            .collect();
        let shelf: Vec<(u32, [u8; 8], u32)> = (0..len)
            .map(|_| (next() % 4, u64::from(next()).to_le_bytes(), next()))
            .collect();
        let keys: Vec<u64> = (0..len)
            .map(|_| u64::from(next()) << 32 | u64::from(next()))
            .collect();
        let pairs: Vec<(u32, u32)> = (0..len).map(|_| (next(), next())).collect();
        let rows = [
            (
                "book keys, u64 (sort_unstable)",
                time(|| keys.clone().sort_unstable()),
                time(|| sort_unstable(&mut keys.clone())),
            ),
            (
                "archive index, (u32, u32) (sort_unstable)",
                time(|| pairs.clone().sort_unstable()),
                time(|| sort_unstable(&mut pairs.clone())),
            ),
            (
                "title order (sort_by, lowercased)",
                time(|| {
                    titles
                        .clone()
                        .sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()))
                }),
                time(|| {
                    sort_by(&mut titles.clone(), |a, b| {
                        a.to_lowercase().cmp(&b.to_lowercase())
                    })
                }),
            ),
            (
                "title order (cached key)",
                time(|| titles.clone().sort_by_cached_key(|a| a.to_lowercase())),
                time(|| sort_by_cached_key(&mut titles.clone(), |a| a.to_lowercase())),
            ),
            (
                "shelf entries (sort_unstable_by_key)",
                time(|| shelf.clone().sort_unstable_by_key(|e| (e.0, e.1, e.2))),
                time(|| sort_by_key(&mut shelf.clone(), |e| (e.0, e.1, e.2))),
            ),
            (
                "plain Strings (sort)",
                time(|| titles.clone().sort()),
                time(|| sort(&mut titles.clone())),
            ),
        ];
        for (name, std, mine) in rows {
            println!(
                "{len:>6} {name:<38} std {std:>10.2?} shared {mine:>10.2?} ({:.2}x)",
                mine.as_secs_f64() / std.as_secs_f64()
            );
        }
    }
}
