//! Every allocation of KOReader's files model may be refused: reading,
//! writing, building and naming end in `NoMemory` or succeed, never abort.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the tests")]
#[path = "../../test-support/fail_alloc.rs"]
mod fail_alloc;
use paperpaw_plugin_koreader::native_model as model;
use paperpaw_sdk::annotations::Kind;
use paperpaw_sdk::document::Locator;

#[global_allocator]
static ALLOC: fail_alloc::Failing = fail_alloc::Failing;

const SIDECAR: &str = r#"-- we can read Lua syntax here!
return {
    ["annotations"] = {
        [1] = {
            ["chapter"] = "Chapter I",
            ["note"] = "Two\
lines \xC3\x28 bad",
            ["text"] = "Down the \"Rabbit\" Hole\065",
        },
        [2] = { ["datetime"] = "2026-10-02 08:00:00" },
    },
    ["percent_finished"] = 0.25,
    ["summary"] = { status = 'reading', modified = [[2026-10-02]], long = [=[a]]b]=] },
    ["stats"] = { 1, 2, -3.5e-2, true, false, nil, 0x1F },
}
"#;

#[test]
fn reading_and_writing_survive_refused_allocations() {
    fail_alloc::sweep(|| {
        let value = model::parse(SIDECAR)?.unwrap();
        let text = model::write(&value)?;
        assert!(text.contains("percent_finished"));
        Ok(())
    });
}

#[test]
fn tables_survive_refused_allocations() {
    fail_alloc::sweep(|| {
        let mut table = model::Table::from_list([model::string("a")?, model::string("b")?])?;
        table.set("k", model::string("v")?)?;
        table.set("k", model::Value::Nil)?;
        assert_eq!(table.list()?.len(), 2);
        model::write(&model::table([("t", model::string("x")?)])?)?;
        Ok(())
    });
}

#[test]
fn names_survive_refused_allocations() {
    fail_alloc::sweep(|| {
        model::sidecar("Books/a.epub")?;
        model::metadata_file("Books/a.epub")?;
        model::exchange_file("Books/a.epub")?;
        model::known_file("plugins/koreader", "Books/a.epub")?;
        Ok(())
    });
}

#[test]
fn what_is_known_survives_refused_allocations() {
    fail_alloc::sweep(|| {
        let known = [
            model::Known {
                key: (Kind::Bookmark, Locator::new(1, 2), Locator::new(1, 2)),
                note: None,
            },
            model::Known {
                key: (Kind::Highlight, Locator::new(3, 4), Locator::new(5, 6)),
                note: Some(0xabc),
            },
        ];
        let text = model::write_known(&known, 7)?;
        let last = model::parse_known(&text)?;
        assert_eq!(last.known.len(), 2);
        Ok(())
    });
}
