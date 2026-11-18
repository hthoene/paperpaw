//! Every allocation of the store's rules may be refused: each ends in
//! `NoMemory` or succeeds, never aborts.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the tests")]
#[path = "../../test-support/fail_alloc.rs"]
mod fail_alloc;
use paperpaw_plugin_store::native_model as model;

#[global_allocator]
static ALLOC: fail_alloc::Failing = fail_alloc::Failing;

#[test]
fn index_lines_survive_refused_allocations() {
    let entry = model::Entry {
        id: "airy-theme".into(),
        name: "Luftig\tUnd\nmehr".into(),
        summary: "A summary".into(),
        group: 5,
        author: "Someone".into(),
        licence: "Apache-2.0".into(),
        version: "1.0.0".into(),
        sdk: "0.4".into(),
        code: true,
        size: 2345,
        added: 20_260_101,
        updated: 20_260_202,
        ..model::Entry::default()
    };
    fail_alloc::sweep(|| {
        let line = entry.line()?;
        let back = model::Entry::parse(line.trim_end_matches('\n'))?.unwrap();
        assert_eq!(back.id, "airy-theme");
        Ok(())
    });
}

#[test]
fn addresses_survive_refused_allocations() {
    fail_alloc::sweep(|| {
        model::list_url(
            "https://paperpaw.org/api/v1/plugins.json?k=1",
            "x4 pro",
            "de-DE",
            true,
            Some("0123456789abcdef"),
        )?;
        model::list_url(model::OFFICIAL_LIST, "x4", "en", false, None)?;
        model::encode("ä b/c")?;
        Ok(())
    });
}

#[test]
fn revocations_survive_refused_allocations() {
    let key = "01".repeat(32);
    let signature = "02".repeat(64);
    fail_alloc::sweep(|| {
        let mut list = model::Revocations::default();
        for plugin in ["a", "b", "a"] {
            list.field("plugin", plugin)?;
            list.field("key", &key)?;
            list.field("signature", &signature)?;
            list.entry_done(&|_| true, (&|_| Some([1; 32]), &|| Vec::new(), false))?;
        }
        assert_eq!(list.pending().len(), 2);
        Ok(())
    });
}

#[test]
fn runs_and_sections_survive_refused_allocations() {
    fail_alloc::sweep(|| {
        let mut runs = Vec::new();
        for group in [1, 1, 2, 3] {
            model::add_to_run(&mut runs, group)?;
        }
        assert_eq!(runs.len(), 3);
        let mut ids = Vec::new();
        assert!(model::keep_in_section(&mut ids, "airy-theme")?);
        assert!(!model::keep_in_section(&mut ids, "Not A Name")?);
        Ok(())
    });
}

#[test]
fn reading_a_plugin_survives_refused_allocations() {
    use paperpaw_plugin_store::native_item::Item;
    let long = "é".repeat(200);
    fail_alloc::sweep(|| {
        let mut item = Item::new();
        item.read(&["id"], "airy-theme", "x4")?;
        item.read(&["name"], &long, "x4")?;
        item.read(&["summary"], &long, "x4")?;
        item.read(&["latest", "version"], "1.0.0", "x4")?;
        item.read(&["devices", "[]"], "x4", "x4")?;
        assert!(item.device);
        Ok(())
    });
}

#[test]
fn a_list_and_a_page_survive_refused_allocations() {
    use paperpaw_plugin_store::native_item::Device;
    use paperpaw_plugin_store::native_list::{Hooks, ListReader, PageReader};
    use paperpaw_sdk::native::{Fingerprint, INTERFACE};
    let fingerprint = Fingerprint::of(INTERFACE, 0x4567_89ab_cdef);
    let abi = format!("{:016x}", fingerprint.0);
    let sha = "0123456789abcdef".repeat(4);
    let plugin = format!(
        r#"{{"id": "airy", "name": "{}", "summary": "s", "group": "look", "kind": "code",
 "author": "A", "licence": "MIT", "author_key": "{}", "official": true,
 "rotations": [{{"to": "{}", "signature": "{}"}}],
 "screenshots": [{{"url": "s/1.png", "width": 9, "height": 9, "size": 5}}],
 "latest": {{"version": "1.0.0", "sdk": "0.4", "formats": 1, "size": 9,
  "files": [{{"path": "plugin.ini", "url": "f/p", "size": 9, "sha256": "{sha}"}}],
  "builds": [{{"device": "x4", "abi": "{abi}", "url": "/b", "size": 5, "sha256": "{sha}",
   "families": [1,1,1,1,1,1,1,1]}}]}}}}"#,
        "é".repeat(100),
        "ab".repeat(32),
        "cd".repeat(32),
        "ef".repeat(64),
    );
    let list = format!(
        r#"{{"format": 2, "sections": {{"new": ["airy"]}}, "plugins": [{plugin}],
 "revoked_keys": [{{"plugin": "airy", "key": "{}", "signature": "{}"}}]}}"#,
        "01".repeat(32),
        "02".repeat(64),
    );
    let page = format!(r#"{{"format": 2, "plugin": {plugin}}}"#);
    let device = || Device {
        name: "x4",
        fingerprint: Some(fingerprint),
        code_file: "code.ppn",
        have: None,
        sdk: paperpaw_sdk::SDK_VERSION,
        official: true,
        provides: None,
    };
    fn author(_: &str) -> Option<[u8; 32]> {
        Some([1; 32])
    }
    fn none() -> Vec<String> {
        Vec::new()
    }
    fail_alloc::sweep(|| {
        let hooks = Hooks {
            author_of: &author,
            stopped: &none,
            host_checks: false,
        };
        let mut reader = ListReader::new(device(), hooks);
        let mut found = 0;
        for piece in list.as_bytes().chunks(97) {
            reader.feed(piece, &mut |_| {
                found += 1;
                Ok(())
            })?;
        }
        let read = reader.finish(&mut |_| {
            found += 1;
            Ok(())
        })?;
        // The revocation is handed over in a copy that is made fallibly too.
        assert_eq!(read.as_ref().map(|l| l.revoked.len()), Some(1));
        assert_eq!(found, 1);
        let mut reader = PageReader::new(device());
        for piece in page.as_bytes().chunks(97) {
            reader.feed(piece)?;
        }
        assert!(reader.finish()?.is_some());
        Ok(())
    });
}
