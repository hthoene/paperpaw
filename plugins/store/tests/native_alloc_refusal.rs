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
            list.entry_done(&|_| true, &|_| Some([1; 32]), &|| Vec::new())?;
        }
        assert_eq!(list.pending().len(), 2);
        Ok(())
    });
}
