//! Every allocation of the page's rows may be refused: each ends in `NoMemory`
//! or succeeds, never aborts.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "fixture errors fail the tests"
)]
#[path = "../../test-support/fail_alloc.rs"]
mod fail_alloc;
use paperpaw_plugin_store::native_model::Entry;
use paperpaw_plugin_store::native_page::{Installed, Kind, Origin, PageInput, rows};

#[global_allocator]
static ALLOC: fail_alloc::Failing = fail_alloc::Failing;

#[test]
fn the_rows_survive_refused_allocations() {
    let entry = Entry {
        id: "clock".into(),
        version: "1.1.0".into(),
        ..Entry::default()
    };
    let installed = Installed {
        kind: Kind::Data,
        enabled: true,
        restart_needed: true,
        version: "1.0.0",
        origin: Origin::Store,
        release: None,
        trouble: None,
    };
    fail_alloc::sweep(|| {
        for (installed, own_job, notice) in [
            (None, false, false),
            (Some(&installed), false, false),
            (Some(&installed), true, false),
            (None, false, true),
        ] {
            let page = PageInput {
                entry: &entry,
                installed,
                own_job,
                notice,
                no_room: false,
                takes_betas: true,
                has_description: true,
                has_shots: true,
                has_source: true,
            };
            rows(&page)?;
        }
        Ok(())
    });
}
