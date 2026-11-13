//! Every allocation of the snapshot of what is installed may be refused: it
//! ends in `NoMemory` or succeeds, never aborts.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "fixture errors fail the tests"
)]
#[path = "../../test-support/fail_alloc.rs"]
mod fail_alloc;
use paperpaw_plugin_store::native_installs::{Row, Snapshot};
use paperpaw_plugin_store::native_page::{Kind, Origin};
use paperpaw_sdk::native::Error;

#[global_allocator]
static ALLOC: fail_alloc::Failing = fail_alloc::Failing;

#[test]
fn the_snapshot_survives_refused_allocations() {
    fail_alloc::sweep(|| {
        let zeta = Row::new("zeta", Kind::Data, true, false, "1.0.0", None)?;
        // A store's install copies its release too.
        let alpha = Row::from_registry("alpha", Kind::Code, 1 | (1 << 5), ("1.2.0", ""), 0)?;
        let builtin = Row::new("builtin", Kind::BuiltIn, true, false, "1.0.0", None)?;
        // The test's own list is not what the sweep refuses.
        let rows = fail_alloc::exempt(|| vec![zeta, alpha, builtin]);
        let snapshot = Snapshot::settle(rows);
        assert_eq!(
            snapshot.have("alpha").map(|a| a.origin),
            Some(Origin::Store)
        );
        Ok::<(), Error>(())
    });
}
