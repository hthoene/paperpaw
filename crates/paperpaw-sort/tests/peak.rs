//! The most heap the sorts hold at once beyond the list itself, and what the
//! in-place path costs in time. Prints a table; run with
//! `cargo test --release -p paperpaw-sort --test peak -- --ignored --nocapture`.
#![allow(
    clippy::unwrap_used,
    reason = "a test: the numbers fit the types they are made for"
)]
use paperpaw_sort::{sort_by, sort_by_cached_key};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::time::Instant;

struct Counting;

thread_local! {
    static LIVE: Cell<usize> = const { Cell::new(0) };
    static PEAK: Cell<usize> = const { Cell::new(0) };
    static REFUSE: Cell<bool> = const { Cell::new(false) };
}

// SAFETY: forwards to `System`, counting the bytes held and refusing (null)
// blocks of 64 bytes or more while `REFUSE` is set.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if layout.size() >= 64 && REFUSE.try_with(Cell::get).unwrap_or(false) {
            return core::ptr::null_mut();
        }
        let _ = LIVE.try_with(|live| {
            live.set(live.get() + layout.size());
            let _ = PEAK.try_with(|peak| peak.set(peak.get().max(live.get())));
        });
        // SAFETY: the caller's layout.
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let _ = LIVE.try_with(|live| live.set(live.get().saturating_sub(layout.size())));
        // SAFETY: allocated by `System` above.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// Peak bytes beyond what was held when `run` began (the list is made first),
/// and the time it took.
fn measured(
    refuse: bool,
    mut list: Vec<String>,
    run: impl FnOnce(&mut Vec<String>),
) -> (usize, std::time::Duration) {
    let base = LIVE.with(Cell::get);
    PEAK.with(|peak| peak.set(base));
    REFUSE.with(|cell| cell.set(refuse));
    let start = Instant::now();
    run(&mut list);
    let took = start.elapsed();
    REFUSE.with(|cell| cell.set(false));
    (PEAK.with(Cell::get) - base, took)
}

fn titles(len: u32) -> Vec<String> {
    let mut seed = 12_345_u64;
    let mut next = || {
        seed = seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        u32::try_from(seed >> 33).unwrap()
    };
    (0..len)
        .map(|_| format!("The Book {} of {}", next() % 5000, next() % 997))
        .collect()
}

#[test]
#[ignore = "prints a table"]
fn peaks_and_in_place_times() {
    println!("   n  positions peak   time   | in place peak   time   | cached-key (u64 keys) peak");
    for len in [13_u32, 100, 500, 1_000, 2_000, 4_000, 6_000, 10_000] {
        let list = titles(len);
        let (peak, time) = measured(false, list.clone(), |l| sort_by(l, |a, b| a.cmp(b)));
        let (held, held_time) = measured(true, list.clone(), |l| sort_by(l, |a, b| a.cmp(b)));
        let (cached, _) = measured(false, list.clone(), |l| {
            sort_by_cached_key(l, |s| u64::try_from(s.len()).unwrap());
        });
        println!(
            "{len:>6}  {:>8} B {time:>9.2?} | {:>8} B {held_time:>9.2?} | {:>8} B",
            peak, held, cached
        );
    }
}

fn numbers(len: usize) -> Vec<(u8, u32)> {
    let mut seed = 99_u32;
    (0..len)
        .map(|at| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            // Few distinct keys, so that stability decides the order.
            (
                u8::try_from(seed >> 29).unwrap(),
                u32::try_from(at).unwrap(),
            )
        })
        .collect()
}

#[test]
fn the_scratch_is_four_bytes_an_item_up_to_the_limit_and_none_beyond() {
    for (len, most) in [
        (13, 4 * 13),
        (1_000, 4_000),
        (2_048, 8_192),
        (2_049, 0),
        (10_000, 0),
    ] {
        let mut list = numbers(len);
        let base = LIVE.with(Cell::get);
        PEAK.with(|peak| peak.set(base));
        sort_by(&mut list, |a, b| a.0.cmp(&b.0));
        assert!(PEAK.with(Cell::get) - base <= most, "{len} items");
        let mut expected = numbers(len);
        expected.sort_by_key(|item| item.0);
        assert_eq!(list, expected, "{len} items");
        // Cached keys: the keys and the positions up to the limit, none beyond.
        let mut cached = numbers(len);
        PEAK.with(|peak| peak.set(LIVE.with(Cell::get)));
        let base = LIVE.with(Cell::get);
        sort_by_cached_key(&mut cached, |item| u64::from(item.0));
        let used = PEAK.with(Cell::get) - base;
        assert!(
            used <= if most == 0 { 0 } else { most + 8 * len },
            "{len} items: {used}"
        );
        assert_eq!(cached, expected, "cached, {len} items");
    }
}
