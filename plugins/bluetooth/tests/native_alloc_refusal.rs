//! Every allocation of the model's rows and searches may be refused: each
//! ends in `NoMemory` or succeeds, never aborts.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the tests")]
#[path = "../../test-support/fail_alloc.rs"]
mod fail_alloc;
use paperpaw_plugin_bluetooth::native_model::{self, Bond, Found, Kind, Search, User};

#[global_allocator]
static ALLOC: fail_alloc::Failing = fail_alloc::Failing;

fn bond(id: u32, kind: Kind) -> Bond {
    Bond {
        id,
        name: "Remote".into(),
        kind,
        connected: true,
        keys: false,
        given: String::new(),
    }
}

fn found(id: u32, signal: Option<i32>) -> Found {
    Found {
        id,
        name: "Remote".into(),
        kind: Kind::Remote,
        signal,
        bonded: false,
    }
}

#[test]
fn rows_and_searches_survive_refused_allocations() {
    // The fixtures are built here, outside the refused runs.
    let bonds = [bond(1, Kind::Remote), bond(2, Kind::Sensor)];
    let users = [User {
        id: "a".into(),
        name: "A".into(),
    }];
    let devices = [found(1, Some(-60)), found(2, None), found(1, Some(-30))];
    fail_alloc::sweep(|| {
        native_model::rows(&bonds, &users)?;
        let mut search = Search::default();
        for device in &devices {
            search.found(Found {
                name: paperpaw_ui_kit::text::copy(&device.name)?,
                ..*device
            })?;
        }
        let _ = search.choose(0);
        search.connected(1);
        search.paired(1, Kind::Other, "")?;
        Ok(())
    });
}
