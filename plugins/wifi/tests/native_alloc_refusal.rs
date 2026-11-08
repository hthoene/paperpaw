//! Every allocation of the page's and the networks' rows may be refused: each
//! ends in `NoMemory` or succeeds, never aborts.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the tests")]
#[path = "../../test-support/fail_alloc.rs"]
mod fail_alloc;
use paperpaw_plugin_wifi::native_ini;
use paperpaw_plugin_wifi::native_model::{self, Link, Network};

#[global_allocator]
static ALLOC: fail_alloc::Failing = fail_alloc::Failing;

#[test]
fn rows_and_notes_survive_refused_allocations() {
    // The fixtures are built here, outside the refused runs.
    let network = Network {
        ssid: "Home".into(),
        signal: -60,
        secured: false,
    };
    let found = [("Home", -60, false), ("Café", -80, true)];
    fail_alloc::sweep(|| {
        native_model::page_rows(true, true)?;
        native_model::page_rows(false, false)?;
        native_model::note_keys(Link::Connected, true, &network)?;
        native_model::note_keys(Link::Apart, false, &network)?;
        native_model::collect(found, 8)?;
        network.try_clone()?;
        Ok(())
    });
}

#[test]
fn the_networks_of_the_file_survive_refused_allocations() {
    let file = b"ssid = Home\npassword = secret\nssid = Cafe\nssid = Bar\npassword = x\n\
                 ssid = 0123456789012345678901234567890123\npassword = lost\n";
    fail_alloc::sweep(|| {
        native_ini::networks(file)?;
        Ok(())
    });
}
