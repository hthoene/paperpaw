//! Compact name index shared by construction checks and guarded lookups.
use super::registrations::Record;

/// Shared declaration categories. Slots 8..=15 belong to REG, 16..=22 to UIX.
/// Services adds its entries here; every category fits the seven-bit field of `NameEntry` (bits 3-9 of
/// `record_kind`, host-internal, so widening it changes no ABI).
pub(crate) const KIND_TABLE: &[(&str, u8)] = &[
    ("setting.group", 0),
    ("setting.spec", 1),
    ("setting.extra", 2),
    ("resource.hyphenation", 3),
    ("resource.font", 4),
    ("screen.spec", 5),
    ("action.spec", 6),
    ("stage.spec", 7),
    ("setup.step", 8),
    ("slot.spec", 9),
    ("slot.offer", 10),
    ("key.bind", 11),
    ("setting.default", 12),
    ("setting.choice", 13),
    ("preset.spec", 14),
    ("preset.value", 15),
    ("input.spec", 16),
    ("help.hint", 17),
    ("widget.spec", 18),
    ("widget.place", 19),
    ("status.item", 20),
    ("status.line", 21),
    ("status.place", 22),
    ("menu.entry", 23),
    ("icon.spec", 24),
    ("menu.need", 25),
    ("menu.section", 26),
    ("menu.hide", 27),
    ("menu.describe", 28),
    ("registration.stage", 29),
    ("book_data.spec", 30),
    ("format.spec", 31),
    ("reader.spec", 32),
    ("lookup.spec", 33),
];
pub(crate) const STAGE_KIND: u8 = KIND_TABLE[7].1;
/// Category of `registration.stage`: keyed by the stage's full id and the
/// token, so one plugin may subscribe two tokens to a stage, never one twice.
pub(crate) const STAGE_SUBSCRIPTION_KIND: u8 = KIND_TABLE[29].1;
pub(crate) const COMPOSITE_SEPARATOR: u8 = 0x1f;
const KIND_MASK: u16 = 0x7f;
const EXTENSION: u16 = 1 << 15;

#[derive(Clone, Copy, Default)]
pub(crate) struct NameEntry {
    pub(crate) hash: u32,
    pub(crate) batch: u16,
    pub(crate) record_kind: u16,
}
impl NameEntry {
    pub(crate) fn new(
        hash: u32,
        batch: u16,
        kind: u8,
        record: usize,
        extension: bool,
    ) -> Result<Self, i32> {
        if u16::from(kind) > KIND_MASK || record >= super::registrations::REGISTRATION_MAX_RECORDS {
            return Err(paperpaw_native::host_glue::ERR_INVALID);
        }
        Ok(Self {
            hash,
            batch,
            record_kind: (u16::from(kind) << 3)
                | (u16::from(extension) << 15)
                | u16::try_from(record).map_err(|_| paperpaw_native::host_glue::ERR_INVALID)?,
        })
    }

    pub(crate) fn kind(self) -> u8 {
        ((self.record_kind >> 3) & KIND_MASK) as u8
    }
    pub(crate) fn is_extension(self) -> bool {
        self.record_kind & EXTENSION != 0
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
        Record::StageSpec(r) => (STAGE_KIND, r.name),
        Record::SetupStep(r) => (KIND_TABLE[8].1, r.name),
        Record::SlotSpec(r) => (KIND_TABLE[9].1, r.name),
        Record::SlotOffer(r) => (KIND_TABLE[10].1, r.slot),
        Record::KeyBind(r) => (KIND_TABLE[11].1, r.layer),
        Record::SettingDefault(r) => (KIND_TABLE[12].1, r.key),
        Record::SettingChoice(r) => (KIND_TABLE[13].1, r.key),
        Record::PresetSpec(r) => (KIND_TABLE[14].1, r.name),
        Record::PresetValue(r) => (KIND_TABLE[15].1, r.preset),
        Record::InputSpec(r) => (KIND_TABLE[16].1, r.name),
        // A hint is keyed by its screen, the inputs it is for and its text.
        Record::HelpHint(r) => (KIND_TABLE[17].1, r.screen),
        Record::WidgetSpec(r) => (KIND_TABLE[18].1, r.name),
        Record::WidgetPlace(r) => (KIND_TABLE[19].1, r.name),
        Record::StatusItem(r) => (KIND_TABLE[20].1, r.name),
        Record::StatusLine(r) => (KIND_TABLE[21].1, r.name),
        Record::StatusPlace(r) => (KIND_TABLE[22].1, r.name),
        // Menus and icons (`screens-input.md` section 1): an entry and a hide
        // are keyed by (menu, item), the others by their item or name.
        Record::MenuEntry(r) => (KIND_TABLE[23].1, r.menu),
        Record::IconSpec(r) => (KIND_TABLE[24].1, r.item),
        Record::MenuNeed(r) => (KIND_TABLE[25].1, r.item),
        Record::MenuSection(r) => (KIND_TABLE[26].1, r.name),
        Record::MenuHide(r) => (KIND_TABLE[27].1, r.menu),
        Record::MenuDescribe(r) => (KIND_TABLE[28].1, r.item),
        Record::RegistrationStage(r) => (STAGE_SUBSCRIPTION_KIND, r.stage),
        // A plugin keeps a folder of a name once, whatever its kind
        // (`library.md` section 1).
        Record::BookDataSpec(r) => (KIND_TABLE[30].1, r.name),
        // A format and a reading app: `<plugin>.<name>`, each name once per
        // plugin (`document.md` section 1).
        Record::FormatSpec(r) => (KIND_TABLE[31].1, r.name),
        Record::ReaderSpec(r) => (KIND_TABLE[32].1, r.name),
        // A lookup provider: its name once per plugin (`document.md` section 7).
        Record::LookupSpec(r) => (KIND_TABLE[33].1, r.name),
        _ => return None,
    })
}
/// A token as decimal text in `scratch` (a key field is text).
fn token_text(token: u32, scratch: &mut [u8; 10]) -> &str {
    let mut at = scratch.len();
    let mut rest = token;
    loop {
        at -= 1;
        scratch[at] = b'0' + (rest % 10) as u8;
        rest /= 10;
        if rest == 0 {
            break;
        }
    }
    core::str::from_utf8(&scratch[at..]).unwrap_or("0")
}

/// All unique-key fields, borrowed from the record without joining strings
/// (a number is written into `scratch`).
fn name_parts<'a>(
    record: Record<'a>,
    scratch: &'a mut [u8; 10],
) -> Option<(u8, [&'a str; 4], usize)> {
    let (kind, name) = key(record)?;
    let mut parts = [name, "", "", ""];
    let len = match record {
        Record::SlotOffer(r) => {
            parts[1] = r.screen;
            2
        }
        Record::KeyBind(r) => {
            parts[1] = r.input;
            parts[2] = r.when_key;
            parts[3] = r.when_value;
            4
        }
        Record::HelpHint(r) => {
            // `input` is 0..=2 at validation.
            parts[1] = ["0", "1", "2"].get(r.input as usize).copied().unwrap_or("");
            parts[2] = r.text;
            3
        }
        Record::SettingChoice(r) => {
            parts[1] = r.value;
            2
        }
        Record::PresetValue(r) => {
            parts[1] = r.key;
            2
        }
        Record::MenuEntry(r) => {
            parts[1] = r.item;
            2
        }
        Record::MenuHide(r) => {
            parts[1] = r.item;
            2
        }
        Record::RegistrationStage(r) => {
            parts[1] = token_text(r.token, scratch);
            2
        }
        _ => 1,
    };
    Some((kind, parts, len))
}

pub(crate) fn record_hash(record: Record<'_>) -> Option<u32> {
    let mut scratch = [0; 10];
    let (kind, parts, len) = name_parts(record, &mut scratch)?;
    Some(if len == 1 {
        name_hash(kind, parts[0])
    } else {
        composite_hash(kind, &parts[..len])
    })
}

/// Hash equality never replaces comparison of every original key field.
pub(crate) fn same_record_name(a: Record<'_>, b: Record<'_>) -> bool {
    let (mut scratch_a, mut scratch_b) = ([0; 10], [0; 10]);
    let (Some((ak, a, al)), Some((bk, b, bl))) =
        (name_parts(a, &mut scratch_a), name_parts(b, &mut scratch_b))
    else {
        return false;
    };
    ak == bk
        && al == bl
        && a[..al]
            .iter()
            .zip(&b[..bl])
            .all(|(a, b)| same_name(ak, a, b))
}

pub(crate) fn matches_parts(record: Record<'_>, kind: u8, wanted: &[&str]) -> bool {
    let mut scratch = [0; 10];
    let Some((actual_kind, parts, len)) = name_parts(record, &mut scratch) else {
        return false;
    };
    actual_kind == kind
        && len == wanted.len()
        && parts[..len]
            .iter()
            .zip(wanted)
            .all(|(a, b)| same_name(kind, a, b))
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
    composite_hash(kind, &[name])
}

/// Separate composite fields with a byte that cannot occur in an admitted id.
/// Callers compare the original fields after a hash match, as for plain names.
pub(crate) fn composite_hash(kind: u8, parts: &[&str]) -> u32 {
    let mut hash = 2_166_136_261_u32;
    for (index, part) in parts.iter().enumerate() {
        if index != 0 {
            hash = (hash ^ u32::from(COMPOSITE_SEPARATOR)).wrapping_mul(16_777_619);
        }
        for byte in part.bytes() {
            let byte = if kind == 3 {
                byte.to_ascii_lowercase()
            } else {
                byte
            };
            hash = (hash ^ u32::from(byte)).wrapping_mul(16_777_619);
        }
    }
    hash
}

const _: () = {
    let mut index = 0;
    while index < KIND_TABLE.len() {
        assert!(KIND_TABLE[index].1 as u16 <= KIND_MASK);
        index += 1;
    }
};
const _: () = assert!(core::mem::size_of::<NameEntry>() == 8);
// The kind (bits 3-9), the record index (bits 0-2) and the extension flag (bit 15) do not overlap.
const _: () = assert!((KIND_MASK << 3) & EXTENSION == 0 && (KIND_MASK << 3) & 7 == 0);

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

#[cfg(test)]
mod packed_tests {
    #![allow(clippy::unwrap_used, reason = "fixture refusal must fail the test")]
    use super::*;
    #[test]
    fn every_seven_bit_kind_preserves_record_and_extension_in_eight_bytes() {
        for kind in 0..=127 {
            for record in 0..8 {
                for extension in [false, true] {
                    let entry = NameEntry::new(17, 3, kind, record, extension).unwrap();
                    assert_eq!(
                        (entry.kind(), entry.record(), entry.is_extension()),
                        (kind, record, extension)
                    );
                    assert_eq!((entry.hash, entry.batch), (17, 3));
                }
            }
        }
        assert!(NameEntry::new(0, 0, 128, 0, false).is_err());
        assert!(NameEntry::new(0, 0, 7, 8, false).is_err());
        assert_eq!(core::mem::size_of::<NameEntry>(), 8);
    }
    #[test]
    fn composite_keys_have_unambiguous_field_boundaries() {
        assert_ne!(
            composite_hash(STAGE_KIND, &["a", "bc"]),
            composite_hash(STAGE_KIND, &["ab", "c"])
        );
        assert_eq!(
            composite_hash(STAGE_KIND, &["a", "bc"]),
            name_hash(STAGE_KIND, "a\u{1f}bc")
        );
        assert_eq!(name_hash(3, "en-US"), name_hash(3, "EN-us"));
        for (index, (_, kind)) in KIND_TABLE.iter().enumerate() {
            assert_eq!(usize::from(*kind), index);
        }
    }
}

#[cfg(test)]
mod pack_tests {
    #![allow(clippy::unwrap_used, reason = "fixture refusal must fail the test")]
    use super::NameEntry;

    #[test]
    fn kind_record_and_extension_round_trip_for_every_field_edge() {
        for kind in [0_u8, 1, 28, 31, 32, 64, 127] {
            for record in 0..super::super::registrations::REGISTRATION_MAX_RECORDS {
                for extension in [false, true] {
                    let entry = NameEntry::new(7, 3, kind, record, extension).unwrap();
                    assert_eq!(entry.kind(), kind);
                    assert_eq!(entry.record(), record);
                    assert_eq!(entry.is_extension(), extension);
                    assert_eq!((entry.hash, entry.batch), (7, 3));
                }
            }
        }
        // 128 does not fit the seven bits; a ninth record does not fit three.
        assert!(NameEntry::new(0, 0, 128, 0, false).is_err());
        assert!(NameEntry::new(0, 0, 0, 8, false).is_err());
    }
}
