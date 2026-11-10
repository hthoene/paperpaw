//! Every allocation of the model's text and list building may be refused:
//! each ends in `NoMemory` or succeeds, never aborts.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the tests")]
#[path = "../../test-support/fail_alloc.rs"]
mod fail_alloc;
use paperpaw_plugin_plugin_manager::native_model::{
    self, Kind, NotInstalled, PluginRow, Problem, Run, SafeMode, Say,
};
use paperpaw_sdk::native::Error;

#[global_allocator]
static ALLOC: fail_alloc::Failing = fail_alloc::Failing;

struct Words;
impl Say for Words {
    fn say(&mut self, key: &str) -> Result<String, Error> {
        paperpaw_ui_kit::text::copy(key)
    }
    fn say_with(&mut self, key: &str, name: &str, value: &str) -> Result<String, Error> {
        paperpaw_ui_kit::text::concat(&[key, "[", name, "=", value, "]"])
    }
}

fn row(id: &str, flags: u32) -> PluginRow {
    PluginRow {
        id: id.into(),
        name: id.into(),
        version: "1.0.0".into(),
        kind: Kind::Code,
        run: Run::Running,
        flags,
        problem: None,
        problem_arg: 0,
        update: String::new(),
    }
}

fn name(id: &str) -> Result<String, Error> {
    paperpaw_ui_kit::text::copy(id)
}

#[test]
fn notes_and_notices_survive_refused_allocations() {
    // The fixtures are built here, outside the refused runs.
    let rows = [
        row("a", 1),
        row("a", 1 | 2),
        row("a", 1 | 4),
        PluginRow {
            problem: Some(Problem::SafeMode),
            ..row("a", 0)
        },
        PluginRow {
            update: "2.0.0".into(),
            ..row("a", 1)
        },
    ];
    let one = [("a".to_string(), NotInstalled::Damaged)];
    let two = [one[0].clone(), ("b".to_string(), NotInstalled::OtherAuthor)];
    fail_alloc::sweep(|| {
        for row in &rows {
            native_model::note(&mut Words, row, "a description")?;
        }
        native_model::safe_mode_notice(&mut Words, SafeMode::Failures, "a", "b", name)?;
        native_model::safe_mode_notice(&mut Words, SafeMode::User, "", "", name)?;
        native_model::not_installed_notice(&mut Words, &one, name)?;
        native_model::not_installed_notice(&mut Words, &two, name)?;
        native_model::line("core.home", "home.screen")?;
        Ok(())
    });
}
