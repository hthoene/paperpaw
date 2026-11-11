//! The native reading of a list and of a plugin's page (`native_list`) gives
//! what the interface-6 reading did, for any list a library could send, in
//! pieces of any size.
#![allow(clippy::unwrap_used, reason = "fixtures")]
extern crate std;
use super::{Device, Item, Picks, read_list, read_plugin};
use crate::native_item as native;
use crate::native_list::{Hooks, ListReader, PageReader};
use crate::native_model::{Provides, SECTIONS};
use crate::native_parity_tests::old_entry;
use alloc::{
    format,
    string::{String, ToString},
    vec::Vec,
};
use paperpaw_sdk::MemoryStorage;
use paperpaw_sdk::install::Digest as OldDigest;
use paperpaw_sdk::native::{Fingerprint, INTERFACE};
use proptest::prelude::*;

const FINGERPRINT: Fingerprint = Fingerprint::of(INTERFACE, 0x4567_89ab_cdef);
const KEY: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const OURS: &str = "148ccf3ebc30506b355c6b5cb2baf87d9265606cc6a81f085bb3b9740a95aebd";
const SHA256: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const SHA1: &str = "0123456789abcdef0123456789abcdef01234567";

fn abi() -> String {
    format!("{:016x}", FINGERPRINT.0)
}

fn pick<T: Clone + core::fmt::Debug + 'static>(of: &[T]) -> impl Strategy<Value = T> + use<T> {
    proptest::sample::select(of.to_vec())
}

fn build() -> impl Strategy<Value = String> {
    let abis = [
        "7.0", "7.2", "7.3", "6.1", "8.0", "7.x", "07.1", "7.", "7.1.2", "zz", "",
    ]
    .map(String::from)
    .into_iter()
    .chain([abi()])
    .collect::<Vec<_>>();
    (
        pick(&["x4", "x4-pro"]),
        pick(&abis),
        pick(&["/b/x.ppn", "https://cdn.example/x.ppn", ""]),
        pick(&[SHA256, "ab", "ABCDEF0123456789abcdef0123456789abcdef0123456789abcdef0123456789"]),
        proptest::option::of(pick(&[50u32, 400, 900])),
        proptest::option::of(pick(&[3usize, 8, 9])),
        pick(&["differs", "same"]),
        proptest::option::of(100u64..90_000),
    )
        .prop_map(|(device, abi, url, sha, api, families, rebuilt, len)| {
            let api = api.map_or(String::new(), |a| format!(r#""needs_api": {a},"#));
            let families = families.map_or(String::new(), |n| {
                let list = (0..n).map(|i| (i % 4).to_string()).collect::<Vec<_>>().join(",");
                format!(r#""families": [{list}],"#)
            });
            let len = len.map_or(String::new(), |n| format!(r#""image_len": {n},"#));
            format!(
                r#"{{"device": "{device}", "abi": "{abi}", "url": "{url}", "size": 70000, "sha256": "{sha}", {api}{families}{len}"rebuilt": "{rebuilt}"}}"#
            )
        })
}

fn file() -> impl Strategy<Value = String> {
    (
        pick(&["plugin.ini", "a.txt", "../x", "plugin.ini"]),
        pick(&["f/a", "https://x.example/a", ""]),
        pick(&[SHA256, SHA1, "zz"]),
        pick(&["sha256", "sha1", "md5"]),
        0u64..5000,
    )
        .prop_map(|(path, url, digest, kind, size)| {
            format!(r#"{{"path": "{path}", "url": "{url}", "size": {size}, "{kind}": "{digest}"}}"#)
        })
}

fn shot() -> impl Strategy<Value = String> {
    (
        pick(&[
            "s/1.png",
            "s/2.JPG",
            "s/3.gif",
            "s/4.jpeg?x=1",
            "https://x.example/5.png#a",
        ]),
        pick(&[100u32, 2560, 2561]),
        pick(&[100u32, 3000]),
    )
        .prop_map(|(url, w, h)| {
            format!(r#"{{"url": "{url}", "width": {w}, "height": {h}, "size": 5}}"#)
        })
}

fn rotation() -> impl Strategy<Value = String> {
    (pick(&[OURS, KEY, "zz"]), pick(&["a", "b", "zz"])).prop_map(|(to, sig)| {
        let sig = sig.repeat(128).chars().take(128).collect::<String>();
        let sig = if sig.starts_with('z') {
            String::from("zz")
        } else {
            sig.replace('b', "0").replace('a', "1")
        };
        format!(r#"{{"to": "{to}", "signature": "{sig}"}}"#)
    })
}

#[derive(Debug, Clone)]
struct Spec {
    head: String,
    builds: Vec<String>,
    files: Vec<String>,
    shots: Vec<String>,
    rotations: Vec<String>,
    tail: String,
}

fn spec() -> impl Strategy<Value = Spec> {
    (
        (
            pick(&["good", "a-b", "Bad_Id", "x", "", "other"]),
            pick(&["Name", "", "long-é-name"]),
            pick(&["1.0.0", "1.2", "x"]),
            pick(&["0.4", "0.9", "x"]),
            pick(&["data", "code", "other"]),
            pick(&["reading", "look", "zzz"]),
        ),
        (
            pick(&["", "author", "update"]),
            pick(&[KEY, OURS, "zz", ""]),
            any::<[bool; 4]>(),
            pick(&[1u32, 99]),
            pick(&["", r#""devices": ["x4"],"#, r#""devices": ["android"],"#]),
        ),
        proptest::collection::vec(build(), 0..4),
        proptest::collection::vec(file(), 0..4),
        proptest::collection::vec(shot(), 0..6),
        proptest::collection::vec(rotation(), 0..10),
    )
        .prop_map(
            |((id, name, version, sdk, kind, group), (waits, key, flags, formats, devices), builds, files, shots, rotations)| {
                let [official, built_in, unreviewed, verified] = flags;
                let head = format!(
                    r#""id": "{id}", "name": "{name}", "summary": "Does it.", "group": "{group}", "kind": "{kind}", "author": "A", "licence": "MIT", "waits": "{waits}", "author_key": "{key}", "official": {official}, "built_in": {built_in}, "unreviewed": {unreviewed}, {devices}"#
                );
                let tail = format!(
                    r#""version": "{version}", "sdk": "{sdk}", "formats": {formats}, "size": 30, "heap": 1000, "verified": {verified}, "source": {{"url": "https://s.example/x"}}"#
                );
                Spec { head, builds, files, shots, rotations, tail }
            },
        )
}

fn plugin_json(s: &Spec) -> String {
    format!(
        r#"{{{}, "repository": "https://r.example/x", "author_since": "2024", "author_plugins": 3, "description": "How it works.", "rotations": [{}], "screenshots": [{}], "latest": {{{}, "files": [{}], "builds": [{}]}}}}"#,
        s.head,
        s.rotations.join(","),
        s.shots.join(","),
        s.tail,
        s.files.join(","),
        s.builds.join(","),
    )
}

fn old_provides(p: Provides) -> crate::needs::Provides {
    crate::needs::Provides {
        minor: p.minor,
        api: p.api,
        families: p.families,
        oldest: p.oldest,
        checks_families: p.checks_families,
    }
}

fn provides(on: bool) -> Option<Provides> {
    on.then_some(Provides {
        minor: Some(2),
        api: 400,
        families: [3, 2, 0, 0, 0, 0, 0, 0],
        oldest: [2, 1, 0, 0, 0, 0, 0, 0],
        checks_families: true,
    })
}

fn digest_text(d: &native::Digest) -> String {
    match d {
        native::Digest::Sha1(h) => format!("sha1:{h}"),
        native::Digest::Sha256(h) => format!("sha256:{h}"),
    }
}

fn nothing(_: &str) -> Option<[u8; 32]> {
    None
}
fn none() -> Vec<String> {
    Vec::new()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(300)))]

    #[test]
    fn a_list_is_read_as_before(
        specs in proptest::collection::vec(spec(), 0..6),
        chunk in 1usize..400,
        on in any::<bool>(),
        official in any::<bool>(),
        sections in proptest::collection::vec((0usize..6, pick(&["good", "a-b", "Bad_Id", "x"])), 0..40),
    ) {
        let sections_json = (0..SECTIONS.len()).map(|n| {
            let ids = sections.iter().filter(|(s, _)| *s == n).map(|(_, id)| format!("\"{id}\"")).collect::<Vec<_>>().join(",");
            format!("\"{}\": [{ids}]", SECTIONS[n])
        }).collect::<Vec<_>>().join(",");
        let text = format!(
            r#"{{"format": 2, "sections": {{{sections_json}}}, "plugins": [{}]}}"#,
            specs.iter().map(plugin_json).collect::<Vec<_>>().join(",")
        );
        let storage = MemoryStorage::new().with("p.json", text.clone());
        let old_device = Device {
            name: "x4",
            fingerprint: Some(FINGERPRINT),
            code_file: "code.ppn",
            plugins: None,
            sdk: paperpaw_sdk::SDK_VERSION,
            official,
            provides: provides(on).map(old_provides),
        };
        let mut old = Vec::new();
        let old_picks: Picks = read_list(&storage, "p.json", &old_device, &mut |e| old.push(e)).unwrap();

        let device = native::Device {
            name: "x4",
            fingerprint: Some(FINGERPRINT),
            code_file: "code.ppn",
            have: None,
            sdk: paperpaw_sdk::SDK_VERSION,
            official,
            provides: provides(on),
        };
        let hooks = Hooks { author_of: &nothing, stopped: &none, host_checks: false };
        let mut reader = ListReader::new(device, hooks);
        let mut new = Vec::new();
        for piece in text.as_bytes().chunks(chunk) {
            reader.feed(piece, &mut |e| { new.push(e); Ok(()) }).unwrap();
        }
        let list = reader.finish(&mut |e| { new.push(e); Ok(()) }).unwrap().unwrap();
        prop_assert_eq!(new.iter().map(old_entry).collect::<Vec<_>>(), old);
        prop_assert_eq!(list.picks.fair, old_picks.fair);
        prop_assert_eq!(&list.picks.sections, &old_picks.sections);
        prop_assert!(list.revoked.is_empty());
    }

    #[test]
    fn a_page_is_read_as_before(s in spec(), chunk in 1usize..400, on in any::<bool>(), official in any::<bool>()) {
        let text = format!(r#"{{"format": 2, "plugin": {}}}"#, plugin_json(&s));
        let storage = MemoryStorage::new().with("p.json", text.clone());
        let old_device = Device {
            name: "x4",
            fingerprint: Some(FINGERPRINT),
            code_file: "code.ppn",
            plugins: None,
            sdk: paperpaw_sdk::SDK_VERSION,
            official,
            provides: provides(on).map(old_provides),
        };
        let base = "https://lib.example/api/plugins/x.json";
        let old = read_plugin(&storage, "p.json", &old_device, base);

        let device = native::Device {
            name: "x4",
            fingerprint: Some(FINGERPRINT),
            code_file: "code.ppn",
            have: None,
            sdk: paperpaw_sdk::SDK_VERSION,
            official,
            provides: provides(on),
        };
        let mut reader = PageReader::new(device);
        for piece in text.as_bytes().chunks(chunk) {
            reader.feed(piece).unwrap();
        }
        let new = reader.finish().unwrap();
        // The old reading also drops a page whose package the install would
        // refuse (a path, a digest, a missing key); the native one hands it
        // to the install. So what the old one reads, the native one reads
        // the same; the rest it reads more of.
        let Some(old) = old else { return Ok(()); };
        let new = new.expect("a page the old reading takes is read");
        prop_assert_eq!(old_entry(&new.entry), old.entry);
        prop_assert_eq!(new.author_key, old.author_key);
        prop_assert_eq!(new.recovery_key, old.recovery_key);
        prop_assert_eq!(
            new.rotations.iter().map(|r| (r.to, r.signature)).collect::<Vec<_>>(),
            old.rotations.iter().map(|r| (r.to, r.signature)).collect::<Vec<_>>()
        );
        prop_assert_eq!(new.files.len(), old.package.files.len());
        for (n, o) in new.files.iter().zip(&old.package.files) {
            prop_assert_eq!(&n.path, &o.path);
            prop_assert_eq!(paperpaw_sdk::web::join_url(base, &n.url), o.url.clone());
            prop_assert_eq!(n.size, o.size);
            prop_assert_eq!(digest_text(&n.digest), o.digest.text());
        }
        prop_assert_eq!(&new.more.source, &old.more.source);
        prop_assert_eq!(&new.more.repository, &old.more.repository);
        prop_assert_eq!(&new.more.author_key, &old.more.author_key);
        prop_assert_eq!(new.more.author_since, old.more.author_since);
        prop_assert_eq!(new.more.author_plugins, old.more.author_plugins);
        prop_assert_eq!(&new.more.verified, &old.more.verified);
        prop_assert_eq!(new.more.heap, old.more.heap);
        prop_assert_eq!(new.more.image_len, old.more.image_len);
        prop_assert_eq!(&new.more.description, &old.more.description);
        prop_assert_eq!(new.more.shots.len(), old.more.shots.len());
        for (n, o) in new.more.shots.iter().zip(&old.more.shots) {
            prop_assert_eq!(paperpaw_sdk::web::join_url(base, &n.url), o.url.clone());
            prop_assert_eq!((n.width, n.height, n.size), (o.width, o.height, o.size));
        }
        let _ = (Item::new, OldDigest::default);
    }
}

#[test]
fn the_first_party_key_is_the_same() {
    assert_eq!(native::FIRST_PARTY_KEY, super::FIRST_PARTY_KEY);
}
