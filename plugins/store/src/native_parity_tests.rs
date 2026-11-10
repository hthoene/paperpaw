//! The native store rules give what the interface-6 code did: the same index
//! lines, the same fit of a build, the same addresses, the same hex.
#![allow(clippy::unwrap_used, reason = "fixtures")]
extern crate std;

use crate::catalog::Entry as Old;
use crate::native_model as native;
use alloc::{string::String, vec::Vec};
use proptest::prelude::*;

/// A field past what the store writes into a line (`MAX_NAME`, `MAX_SUMMARY`):
/// the interface-6 reading takes it, the native one refuses the line on
/// purpose (the store's own lines never hold one; see `PORTING-7.md`).
fn over_cap(e: &Old) -> bool {
    [&e.id, &e.version, &e.sdk, &e.author, &e.licence, &e.name]
        .iter()
        .any(|t| t.len() > native::MAX_NAME)
        || e.summary.len() > native::MAX_SUMMARY
}

/// What the native reading makes of `line` against the old one: the same,
/// except that a line with a field over its cap is refused.
fn same_reading(line: &str) -> Result<(), TestCaseError> {
    let new = native::Entry::parse(line).unwrap();
    let old = Old::parse(line);
    if old.as_ref().is_some_and(over_cap) {
        prop_assert!(new.is_none(), "over the cap, old reads it: {line:?}");
    } else {
        prop_assert_eq!(new.as_ref().map(old_entry), old);
    }
    Ok(())
}

pub(crate) fn old_entry(n: &native::Entry) -> Old {
    Old {
        id: n.id.clone(),
        name: n.name.clone(),
        summary: n.summary.clone(),
        group: n.group,
        author: n.author.clone(),
        licence: n.licence.clone(),
        version: n.version.clone(),
        sdk: n.sdk.clone(),
        code: n.code,
        official: n.official,
        unreviewed: n.unreviewed,
        needs_newer: n.needs_newer,
        waits_author: n.waits_author,
        size: n.size,
        added: n.added,
        updated: n.updated,
    }
}

fn text() -> impl Strategy<Value = String> {
    prop_oneof!["[a-z0-9 .-]{0,20}", "\\PC{0,30}", "[a-z\\t\\n\\r ]{0,20}"]
}

fn entry() -> impl Strategy<Value = native::Entry> {
    (
        (
            text(),
            text(),
            text(),
            0usize..20,
            text(),
            text(),
            text(),
            text(),
        ),
        (any::<[bool; 5]>(), any::<u64>(), any::<u32>(), any::<u32>()),
    )
        .prop_map(
            |(
                (id, name, summary, group, author, licence, version, sdk),
                (flags, size, added, updated),
            )| {
                native::Entry {
                    id,
                    name,
                    summary,
                    group,
                    author,
                    licence,
                    version,
                    sdk,
                    code: flags[0],
                    official: flags[1],
                    unreviewed: flags[2],
                    needs_newer: flags[3],
                    waits_author: flags[4],
                    size,
                    added,
                    updated,
                }
            },
        )
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(400)))]

    #[test]
    fn index_lines_are_written_as_before(entry in entry()) {
        let line = entry.line().unwrap();
        prop_assert_eq!(&line, &old_entry(&entry).line());
        // And read back the same by both.
        same_reading(line.trim_end_matches('\n'))?;
    }

    #[test]
    fn any_line_is_read_as_before(line in "\\PC{0,80}|[0-9a-z\\t]{0,60}") {
        same_reading(&line)?;
    }

    #[test]
    fn queries_are_encoded_as_before(text in "\\PC{0,30}") {
        prop_assert_eq!(native::encode(&text).unwrap(), paperpaw_sdk::web::encode(&text));
    }

    #[test]
    fn addresses_are_made_as_before(
        list in "https://[a-z]{1,8}\\.org/(api/)?plugins\\.json(\\?k=[a-z]{1,4})?",
        device in "[a-z0-9 -]{1,10}",
        language in "[a-z]{2}(-[A-Z]{2})?",
        community in any::<bool>(),
        abi in proptest::option::of("[0-9a-f]{16}"),
    ) {
        let new = native::list_url(&list, &device, &language, community, abi.as_deref()).unwrap();
        // The old address takes the fingerprint itself; text of the same
        // digits is what it prints.
        let old = crate::catalog::list_url(&list, &device, &language, community, None);
        let old = match &abi {
            Some(abi) => alloc::format!("{old}&abi={abi}"),
            None => old,
        };
        prop_assert_eq!(new, old);
    }

    #[test]
    fn hex_is_read_as_before(text in "[0-9a-fA-F]{0,70}|\\PC{0,70}") {
        let old32: Option<[u8; 32]> = paperpaw_sdk::digest::hex::decode(&text).and_then(|v| v.try_into().ok());
        let old64: Option<[u8; 64]> = paperpaw_sdk::digest::hex::decode(&text).and_then(|v| v.try_into().ok());
        prop_assert_eq!(paperpaw_sdk::digest::hex::decode_array::<32>(&text), old32);
        prop_assert_eq!(paperpaw_sdk::digest::hex::decode_array::<64>(&text), old64);
    }

    #[test]
    fn a_build_fits_as_before(
        interface in proptest::option::of(0u16..12),
        minor in proptest::option::of(0u16..6),
        have_minor in proptest::option::of(0u16..6),
        api in proptest::option::of(0u32..800),
        families in proptest::option::of(any::<[u16; 8]>()),
        provides_api in 0u32..800,
        have in any::<[u16; 8]>(),
        oldest in any::<[u16; 8]>(),
        checks in any::<bool>(),
    ) {
        let fam: [u16; 8] = families.unwrap_or([0; 8]).map(|r| r % 6);
        let have = have.map(|r| r % 6);
        let oldest = oldest.map(|r| r % 6);
        let families = families.map(|_| fam);
        let n = native::Needs { interface, minor, api, families };
        let o = crate::needs::Needs { interface, minor, api, families };
        let np = native::Provides { minor: have_minor, api: provides_api, families: have, oldest, checks_families: checks };
        let op = crate::needs::Provides { minor: have_minor, api: provides_api, families: have, oldest, checks_families: checks };
        let new = match np.fit(&n) {
            native::Fit::Yes => "yes",
            native::Fit::Newer => "newer",
            native::Fit::Older => "older",
        };
        let old = match op.fit(&o) {
            crate::needs::Fit::Yes => "yes",
            crate::needs::Fit::Newer => "newer",
            crate::needs::Fit::Older => "older",
        };
        prop_assert_eq!(new, old);
    }
}

#[test]
fn groups_are_those_of_the_old_code() {
    assert_eq!(native::GROUPS, crate::catalog::GROUPS);
    assert_eq!(native::SECTIONS, crate::catalog::SECTIONS);
    for name in native::GROUPS.iter().chain(&["", "unknown", "READING"]) {
        assert_eq!(native::group(name), crate::catalog::group(name));
    }
}

#[test]
fn the_official_list_is_the_old_one() {
    assert_eq!(native::OFFICIAL_LIST, crate::catalog::OFFICIAL_LIST);
    assert_eq!(
        crate::declarations::OFFICIAL_LIST,
        crate::catalog::OFFICIAL_LIST
    );
}

const KEY: &str = "0101010101010101010101010101010101010101010101010101010101010101";
const SIG: &str = "0202020202020202020202020202020202020202020202020202020202020202\
0303030303030303030303030303030303030303030303030303030303030303";

fn revoke(list: &mut native::Revocations, plugin: &str, key: &str, sig: &str) {
    list.field("plugin", plugin).unwrap();
    list.field("key", key).unwrap();
    list.field("signature", sig).unwrap();
    list.field("other", "ignored").unwrap();
}

#[test]
fn revocations_that_could_hold_are_kept_and_the_rest_left() {
    let here = |p: &str| matches!(p, "a" | "b" | "stopped");
    let author = |_: &str| Some([1u8; 32]);
    let stopped = || alloc::vec![String::from("stopped")];
    let mut list = native::Revocations::default();
    let cases: [(&str, &str, &str); 8] = [
        ("a", KEY, SIG),
        // Twice for one plugin.
        ("a", KEY, SIG),
        // Not here, a bad key, a bad signature, no plugin.
        ("c", KEY, SIG),
        ("b", "zz", SIG),
        ("b", KEY, "00"),
        ("", KEY, SIG),
        // Stopped already.
        ("stopped", KEY, SIG),
        ("b", KEY, SIG),
    ];
    for (plugin, key, sig) in cases {
        revoke(&mut list, plugin, key, sig);
        list.entry_done(&here, &author, &stopped).unwrap();
    }
    let kept: Vec<&str> = list.pending().iter().map(|r| r.plugin.as_str()).collect();
    assert_eq!(kept, ["a", "b"]);
    assert_eq!(list.pending()[0].key, [1; 32]);
    assert_eq!(list.pending()[0].signature[..32], [2; 32]);
    assert_eq!(list.pending()[0].signature[32..], [3; 32]);
    // A key the plugin's author did not have is passed over.
    let mut list = native::Revocations::default();
    revoke(&mut list, "a", KEY, SIG);
    list.entry_done(&here, &|_| Some([9u8; 32]), &stopped)
        .unwrap();
    revoke(&mut list, "a", KEY, SIG);
    list.entry_done(&here, &|_| None, &stopped).unwrap();
    assert!(list.pending().is_empty());
}

#[test]
fn a_long_list_of_revocations_is_bounded() {
    let here = |_: &str| true;
    let author = |_: &str| Some([1u8; 32]);
    let stopped = || Vec::new();
    // Entries for plugins that are not here take no place from those that are.
    let mut list = native::Revocations::default();
    for n in 0..native::MAX_READ + 100 {
        let plugin = alloc::format!("p{n}");
        revoke(&mut list, &plugin, KEY, SIG);
        list.entry_done(&here, &author, &stopped).unwrap();
    }
    assert!(list.pending().len() <= paperpaw_sdk::native::keys::MAX_REVOKED);
    // A huge field is cut at what a valid one holds, plus one.
    let mut list = native::Revocations::default();
    list.field("plugin", &"x".repeat(100_000)).unwrap();
    list.field("key", &"é".repeat(100_000)).unwrap();
    list.entry_done(&here, &author, &stopped).unwrap();
    assert!(list.pending().is_empty());
}

#[test]
fn an_abi_is_a_fingerprint_or_major_dot_minor() {
    use crate::native_model::Abi;
    let f = paperpaw_sdk::native::Fingerprint::of(7, 0x1234);
    assert_eq!(
        Abi::parse(&alloc::format!("{f}")),
        Some(Abi::Fingerprint(f))
    );
    assert_eq!(
        Abi::parse(" 7.2 "),
        Some(Abi::Dotted { major: 7, minor: 2 })
    );
    assert_eq!(Abi::parse("0.0"), Some(Abi::Dotted { major: 0, minor: 0 }));
    assert_eq!(Abi::parse("7.65535").and_then(Abi::minor), Some(65535));
    assert_eq!(Abi::parse("7.1").map(Abi::interface), Some(7));
    for bad in [
        "", ".", "7", "7.", ".1", "7.1.2", "07.1", "7.01", "7.-1", "+7.1", "7.x", "7.65536",
        "70000.1", "7 .1", "٧.١", "7,1",
    ] {
        assert_eq!(Abi::parse(bad), None, "{bad:?}");
    }
}

/// The one difference from the old reading: a field past 80 bytes (a summary
/// past 300) is no line of the store's, which cuts what it writes.
#[test]
fn a_field_over_its_cap_is_read_by_the_old_code_and_refused_by_the_native() {
    let line = |name: &str, summary: &str| {
        alloc::format!("0\t10\t0\t1\t2\tid\t1.0.0\t1.0\tme\tMIT\t{name}\t{summary}")
    };
    for (name, summary, kept) in [
        ("x".repeat(80), "s".repeat(300), true),
        ("x".repeat(81), String::new(), false),
        ("é".repeat(41), String::new(), false),
        (String::new(), "s".repeat(301), false),
    ] {
        let line = line(&name, &summary);
        let old = Old::parse(&line);
        assert!(old.is_some(), "the old reading takes it");
        assert_eq!(native::Entry::parse(&line).unwrap().is_some(), kept);
    }
}

/// What the store writes is always within the caps, as the old code cut
/// every field of the library before it wrote it (`cut`): a Beta 3 index holds
/// no such line.
#[test]
fn what_the_old_code_cut_fits_the_caps() {
    let name = "é".repeat(60);
    let cut = crate::catalog::cut(&name, native::MAX_NAME);
    assert!(cut.len() <= native::MAX_NAME);
}

proptest! {
    /// The A to Z key of a name is the old one for any name.
    #[test]
    fn the_name_key_is_the_old_one(name in "\\PC{0,40}") {
        prop_assert_eq!(
            crate::native_index::name_key(&name),
            crate::catalog::name_key(&name)
        );
    }
}

/// An index the interface-6 store kept on the card (its line from the old
/// `Entry::line`, offsets, head and sections in the Beta 3 format) is read
/// by the native `Index` as it is.
#[test]
fn an_index_kept_by_interface_6_is_read_natively() {
    use crate::native_index::{HEAD, INDEX, Index, OFFSETS, SECTIONS_FILE};
    use paperpaw_sdk::{MemoryStorage, Storage};
    let old = |id: &str, name: &str, group: usize| Old {
        id: id.into(),
        name: name.into(),
        summary: "Does it.".into(),
        group,
        author: "A".into(),
        licence: "MIT".into(),
        version: "1.0.0".into(),
        sdk: "0.4".into(),
        size: 30,
        added: 20_260_101,
        updated: 20_260_202,
        ..Old::default()
    };
    let entries = [
        old("a-one", "A one", 0),
        old("b-two", "B\ttwo", 0),
        old("c-three", "C three", 2),
    ];
    let (mut text, mut offsets, mut at) = (String::new(), Vec::new(), 0u32);
    for entry in &entries {
        let line = entry.line();
        offsets.extend_from_slice(&at.to_le_bytes());
        at += line.len() as u32;
        text.push_str(&line);
    }
    let storage = MemoryStorage::new();
    storage.write(INDEX, text.as_bytes()).unwrap();
    storage.write(OFFSETS, &offsets).unwrap();
    storage
        .write(SECTIONS_FILE, b"recommended\tc-three,a-one\nnew\t\n")
        .unwrap();
    storage
        .write(HEAD, b"store-index 5\t0:2,2:1\tfair\t2,0,0,0,0\n")
        .unwrap();
    let index = Index::kept(&storage).unwrap().expect("the Beta 3 head");
    assert_eq!(index.runs(), &[(0, 2), (2, 1)]);
    assert!(index.fair());
    assert_eq!(index.section(&storage, 0).unwrap(), ["c-three", "a-one"]);
    for (n, entry) in entries.iter().enumerate() {
        let read = index.entry(&storage, n).unwrap().expect("a line");
        assert_eq!(read.id, entry.id);
        // The tab of a name was made a space when the line was written.
        assert_eq!(read.name, entry.name.replace('\t', " "));
        assert_eq!(
            (read.group, read.size, read.added),
            (entry.group, entry.size, entry.added)
        );
    }
    let mut seen = Vec::new();
    index
        .scan(&storage, &mut |n, e| {
            seen.push((n, e.id));
            Ok(())
        })
        .unwrap();
    assert_eq!(seen.len(), 3);
}

proptest! {
    /// Whether a version is an update, and what a pre-release is, are the old
    /// code's for any two texts.
    #[test]
    fn offers_is_the_old_one(
        offered in "[0-9]{1,2}\\.[0-9]{1,2}\\.[0-9]{1,2}(-(beta|rc)\\.[0-9]{1,2})?",
        here in "[0-9]{1,2}\\.[0-9]{1,2}\\.[0-9]{1,2}(-(beta|rc)\\.[0-9]{1,2})?",
        betas in any::<bool>(),
    ) {
        prop_assert_eq!(
            crate::native_page::offers(&offered, &here, betas),
            crate::origin::offers(&offered, &here, betas)
        );
        prop_assert_eq!(
            crate::native_page::is_pre_release(&offered),
            crate::origin::is_pre_release(&offered)
        );
    }
}
