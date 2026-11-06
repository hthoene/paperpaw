//! Compact name index shared by construction checks and guarded lookups.
use super::registrations::Record;

#[derive(Clone, Copy, Default)]
pub(crate) struct NameEntry {
    pub(crate) hash: u32,
    pub(crate) batch: u16,
    pub(crate) record_kind: u16,
}
impl NameEntry {
    pub(crate) fn kind(self) -> u8 {
        ((self.record_kind >> 3) & 7) as u8
    }
    pub(crate) fn is_extension(self) -> bool {
        self.record_kind & (1 << 6) != 0
    }
    pub(crate) fn record(self) -> usize {
        usize::from(self.record_kind & 7)
    }
    pub(crate) fn order(self) -> (u8, u32) {
        (self.kind(), self.hash)
    }
}
pub(crate) fn key(record: Record<'_>) -> Option<(u8, &str)> {
    Some(match record {
        Record::SettingGroup(r) => (0, r.name),
        Record::SettingSpec(r) => (1, r.name),
        Record::SettingExtra(r) => (2, r.name),
        Record::ResourceHyphenation(r) => (3, r.language),
        Record::ResourceFontFace(r) => (4, r.name),
        Record::ResourceFontExtension(r) => (4, r.name),
        // Indexed so that a plugin never declares a screen or an action twice and the
        // host finds them by name (`open_screen`, `run_action` use `<plugin>.<name>`).
        Record::ScreenSpec(r) => (5, r.name),
        Record::ActionSpec(r) => (6, r.name),
        _ => return None,
    })
}
pub(crate) fn same_name(kind: u8, a: &str, b: &str) -> bool {
    if kind == 3 {
        a.eq_ignore_ascii_case(b)
    } else {
        a == b
    }
}
/// FNV-1a over the full relative name; language tags are case-insensitive.
pub(crate) fn name_hash(kind: u8, name: &str) -> u32 {
    name.bytes().fold(2_166_136_261, |hash, byte| {
        let byte = if kind == 3 {
            byte.to_ascii_lowercase()
        } else {
            byte
        };
        (hash ^ u32::from(byte)).wrapping_mul(16_777_619)
    })
}
const _: () = assert!(core::mem::size_of::<NameEntry>() == 8);

// The low three bits of record_kind hold the record index.
const _: () = assert!(super::registrations::REGISTRATION_MAX_RECORDS <= 8);

/// Sorts `items` by `key`, unstable, in place, without recursion or extra
/// memory: a heap sort. The standard library's unstable sort is a large
/// generic that every item type instantiates again (about 6 KB of the X4's
/// flash for the name index alone). A heap sort is `O(n log n)` in time; the
/// lists here are bounded by the per-plugin declaration limits.
pub(crate) fn sort_by_key<T, K: Ord>(items: &mut [T], key: impl Fn(&T) -> K) {
    let len = items.len();
    for start in (0..len / 2).rev() {
        sift_down(items, start, len, &key);
    }
    for end in (1..len).rev() {
        items.swap(0, end);
        sift_down(items, 0, end, &key);
    }
}

fn sift_down<T, K: Ord>(items: &mut [T], mut root: usize, end: usize, key: &impl Fn(&T) -> K) {
    loop {
        let mut child = 2 * root + 1;
        if child >= end {
            return;
        }
        if child + 1 < end && key(&items[child]) < key(&items[child + 1]) {
            child += 1;
        }
        if key(&items[root]) >= key(&items[child]) {
            return;
        }
        items.swap(root, child);
        root = child;
    }
}

#[cfg(test)]
mod sort_tests {
    use super::sort_by_key;
    use alloc::vec;
    use alloc::vec::Vec;

    #[test]
    fn it_sorts_like_the_standard_library_on_every_small_shape() {
        // Every length up to 12 of pseudo-random keys with many ties.
        let mut seed = 0x1234_5678_u32;
        for len in 0..=12_usize {
            for _ in 0..40 {
                let mut items: Vec<(u32, u32)> = (0..len)
                    .map(|i| {
                        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                        ((seed >> 24) % 5, i as u32)
                    })
                    .collect();
                let mut expected: Vec<u32> = items.iter().map(|e| e.0).collect();
                expected.sort_unstable();
                sort_by_key(&mut items, |e| e.0);
                let keys: Vec<u32> = items.iter().map(|e| e.0).collect();
                assert_eq!(keys, expected, "len {len}");
                let mut ids: Vec<u32> = items.iter().map(|e| e.1).collect();
                ids.sort_unstable();
                assert_eq!(ids, (0..len as u32).collect::<Vec<_>>(), "nothing lost");
            }
        }
    }

    #[test]
    fn sorted_reversed_and_large_inputs_come_out_sorted() {
        let mut up: Vec<u32> = (0..1000).collect();
        sort_by_key(&mut up, |v| *v);
        assert!(up.windows(2).all(|w| w[0] <= w[1]));
        let mut down: Vec<u32> = (0..1000).rev().collect();
        sort_by_key(&mut down, |v| *v);
        assert_eq!(down, (0..1000).collect::<Vec<_>>());
        let mut same = vec![7_u32; 300];
        sort_by_key(&mut same, |v| *v);
        assert!(same.iter().all(|v| *v == 7));
    }
}
