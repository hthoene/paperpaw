//! The list and a plugin's page of the library are read in pieces of any
//! size with what fits this device; long texts are cut; broken or hostile
//! input ends in `None` or an error, never a panic.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "fixture errors fail the tests"
)]
use paperpaw_plugin_store::native_item::{Device, Digest, FIRST_PARTY_KEY, Have};
use paperpaw_plugin_store::native_list::{Hooks, ListReader, PageReader};
use paperpaw_plugin_store::native_model::{Entry, MAX_NAME};
use paperpaw_sdk::native::{Fingerprint, INTERFACE};

const FINGERPRINT: Fingerprint = Fingerprint::of(INTERFACE, 0x4567_89ab_cdef);
const SHA256: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const OURS: &str = "148ccf3ebc30506b355c6b5cb2baf87d9265606cc6a81f085bb3b9740a95aebd";

fn abi() -> String {
    format!("{:016x}", FINGERPRINT.0)
}

fn device<'a>(have: Option<&'a paperpaw_plugin_store::native_item::HaveFn>) -> Device<'a> {
    Device {
        name: "x4",
        fingerprint: Some(FINGERPRINT),
        code_file: "code.ppn",
        have,
        sdk: paperpaw_sdk::SDK_VERSION,
        official: true,
        provides: None,
    }
}

fn nobody(_: &str) -> Option<[u8; 32]> {
    None
}
fn none() -> Vec<String> {
    Vec::new()
}

fn plugin(id: &str, kind: &str, extra: &str) -> String {
    format!(
        r#"{{"id": "{id}", "name": "Name {id}", "summary": "Does {id}.", "group": "look", "kind": "{kind}",
 "author": "A", "licence": "MIT", {extra}
 "latest": {{"version": "1.0.0", "sdk": "0.4", "formats": 1, "size": 30}}}}"#
    )
}

fn read(text: &str, chunk: usize, device: Device<'_>) -> Vec<Entry> {
    let hooks = Hooks {
        author_of: &nobody,
        stopped: &none,
    };
    let mut reader = ListReader::new(device, hooks);
    let mut out = Vec::new();
    for piece in text.as_bytes().chunks(chunk) {
        reader
            .feed(piece, &mut |e| {
                out.push(e);
                Ok(())
            })
            .unwrap();
    }
    reader
        .finish(&mut |e| {
            out.push(e);
            Ok(())
        })
        .unwrap()
        .expect("a list");
    out
}

#[test]
fn what_fits_is_listed_in_pieces_of_any_size() {
    let code = plugin(
        "own",
        "code",
        &format!(
            r#""official": true, "author_key": "{OURS}", "latest": {{"x": 1}},"#
        ),
    )
    .replace(
        r#""size": 30}"#,
        &format!(
            r#""size": 30, "builds": [{{"device": "x4", "abi": "{}", "url": "/b/x.ppn", "size": 70000, "sha256": "{SHA256}"}}]}}"#,
            abi()
        ),
    );
    let text = format!(
        r#"{{"format": 2, "plugins": [{}, {}, {}, {code}]}}"#,
        plugin("plain", "data", ""),
        plugin("built", "data", r#""built_in": true,"#),
        plugin("phone", "data", r#""devices": ["android"],"#),
    );
    for chunk in [1, 7, 64, 100_000] {
        let entries = read(&text, chunk, device(None));
        let ids: Vec<_> = entries.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["plain", "own"], "chunk {chunk}");
        assert!(!entries[0].code && entries[1].code);
        assert!(entries[1].official, "PaperPaw's own key");
        assert_eq!(entries[1].size, 30 + 70_000);
    }
    assert_eq!(OURS.len(), 64);
    assert_eq!(FIRST_PARTY_KEY[0], 0x14);
}

#[test]
fn what_the_device_has_decides_what_may_be_installed() {
    let have = |id: &str| match id {
        "plain" => Some(Have::Code),
        "built" => Some(Have::BuiltIn { enabled: true }),
        _ => None,
    };
    let text = format!(
        r#"{{"format": 2, "plugins": [{}, {}, {}]}}"#,
        plugin("plain", "data", ""),
        plugin("built", "data", ""),
        plugin("fresh", "data", ""),
    );
    let entries = read(&text, 50, device(Some(&have)));
    let ids: Vec<_> = entries.iter().map(|e| e.id.as_str()).collect();
    // A code plugin is not replaced by data, nor a built-in one.
    assert_eq!(ids, ["fresh"]);
}

#[test]
fn a_page_is_read_with_its_files_keys_and_pictures() {
    let text = format!(
        r#"{{"format": 2, "plugin": {{"id": "airy", "name": "{}", "summary": "s", "group": "look",
 "kind": "data", "author": "A", "licence": "MIT", "repository": "https://r.example/airy",
 "description": "How.", "author_plugins": 3,
 "screenshots": [{{"url": "s/1.png", "width": 100, "height": 100, "size": 5}},
                 {{"url": "s/2.gif", "width": 100, "height": 100, "size": 5}}],
 "latest": {{"version": "1.2.0", "sdk": "0.4", "formats": 1, "size": 9, "heap": 1000,
   "files": [{{"path": "plugin.ini", "url": "f/plugin.ini", "size": 9, "sha256": "{}"}}]}}}}}}"#,
        "n".repeat(500),
        SHA256.to_ascii_uppercase()
    );
    for chunk in [1, 13, 1_000_000] {
        let mut reader = PageReader::new(device(None));
        for piece in text.as_bytes().chunks(chunk) {
            reader.feed(piece).unwrap();
        }
        let page = reader.finish().unwrap().expect("the page fits");
        assert_eq!(page.entry.name.len(), MAX_NAME, "cut, not dropped");
        assert_eq!(page.files.len(), 1);
        assert_eq!(page.files[0].digest, Digest::Sha256(SHA256.into()));
        assert_eq!(page.more.shots.len(), 1, "only a picture PaperPaw decodes");
        assert_eq!(page.more.heap, Some(1000));
        assert_eq!(page.more.author_plugins, 3);
        assert!(!page.code);
    }
}

/// An install's batch holds the install, the author's keys and six steps: a
/// page's longer chain is cut at six, so the host ends it with "key changed".
#[test]
fn a_chain_of_rotations_is_cut_at_six_steps() {
    let step = |n: usize| {
        format!(
            r#"{{"to": "{n:02x}{}", "signature": "{n:02x}{}"}}"#,
            "01".repeat(31),
            "02".repeat(63)
        )
    };
    let steps: Vec<String> = (0..9).map(step).collect();
    let text = format!(
        r#"{{"format": 2, "plugin": {{"id": "clock", "name": "Clock", "summary": "s", "group": "look",
 "kind": "data", "author": "A", "licence": "MIT", "rotations": [{}],
 "latest": {{"version": "1.2.0", "sdk": "0.4", "formats": 1, "size": 9,
   "files": [{{"path": "plugin.ini", "url": "f/plugin.ini", "size": 9, "sha256": "{SHA256}"}}]}}}}}}"#,
        steps.join(", ")
    );
    let mut reader = PageReader::new(device(None));
    reader.feed(text.as_bytes()).unwrap();
    let page = reader.finish().unwrap().expect("the page fits");
    assert_eq!(
        page.rotations.len(),
        paperpaw_plugin_store::native_item::MAX_STEPS
    );
    assert_eq!(page.rotations.len(), 6);
    // The first six, in the author's order.
    assert_eq!(page.rotations[0].to[0], 0);
    assert_eq!(page.rotations[5].to[0], 5);
}

#[test]
fn text_that_is_no_list_or_no_page_is_none_without_a_panic() {
    for text in [
        "",
        "not json",
        "{",
        r#"{"format": 1, "plugins": []}"#,
        r#"{"format": 2}"#,
        r#"{"format": 2, "plugins": [{}, {"id": 5}, null, []]}"#,
        r#"{"plugin": {"id": "x"}}"#,
        "\u{0}\u{1}\u{ffff}",
    ] {
        let hooks = Hooks {
            author_of: &nobody,
            stopped: &none,
        };
        let mut reader = ListReader::new(device(None), hooks);
        let _ = reader.feed(text.as_bytes(), &mut |_| Ok(()));
        let _ = reader.finish(&mut |_| Ok(()));
        let mut reader = PageReader::new(device(None));
        let _ = reader.feed(text.as_bytes());
        let _ = reader.finish();
    }
    // Nested far deeper than the parser reads.
    let deep = format!("{}{}", "[".repeat(5_000), "]".repeat(5_000));
    let mut reader = PageReader::new(device(None));
    reader.feed(deep.as_bytes()).unwrap();
    assert!(reader.finish().unwrap().is_none());
}

#[test]
fn an_error_of_the_caller_ends_the_reading() {
    let text = format!(
        r#"{{"format": 2, "plugins": [{}]}}"#,
        plugin("plain", "data", "")
    );
    let hooks = Hooks {
        author_of: &nobody,
        stopped: &none,
    };
    let mut reader = ListReader::new(device(None), hooks);
    let result = reader
        .feed(text.as_bytes(), &mut |_| {
            Err(paperpaw_sdk::native::Error::NoMemory)
        })
        .and_then(|()| reader.finish(&mut |_| Ok(())).map(|_| ()));
    assert_eq!(result, Err(paperpaw_sdk::native::Error::NoMemory));
}
