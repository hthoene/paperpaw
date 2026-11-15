//! What a staged `plugin.ini` requires, read before the host takes the folder.
#![allow(clippy::unwrap_used, reason = "fixtures")]
use paperpaw_plugin_store::native_requires::{MAX_REQUIRED, required};
use paperpaw_sdk::MemoryStorage;

fn read(ini: &str) -> Vec<String> {
    let card = MemoryStorage::new().with("stage/alpha/plugin.ini", ini);
    required(&card, "stage/alpha", "alpha").unwrap()
}

#[test]
fn the_ids_come_in_the_order_named_and_the_versions_are_the_hosts_to_judge() {
    assert_eq!(
        read("[plugin]\nid = alpha\nrequires = beta, gamma 1.2.0 ,delta\n"),
        ["beta", "gamma", "delta"]
    );
}

#[test]
fn no_file_no_key_or_a_key_of_another_section_requires_nothing() {
    let card = MemoryStorage::new();
    assert!(required(&card, "stage/alpha", "alpha").unwrap().is_empty());
    assert!(read("[plugin]\nid = alpha\n").is_empty());
    assert!(read("[plugin]\nid = alpha\n[other]\nrequires = beta\n").is_empty());
    assert!(read("[plugin]\nid = alpha\nrequires =\n").is_empty());
}

#[test]
fn a_wrong_list_is_read_as_none() {
    for wrong in [
        "alpha",
        "beta, beta",
        "Beta",
        "beta.widget",
        "beta 1.0.0 extra",
        "../beta",
    ] {
        let ini = format!("[plugin]\nrequires = {wrong}\n");
        assert!(read(&ini).is_empty(), "{wrong}");
    }
    let many: Vec<String> = (0..=MAX_REQUIRED).map(|n| format!("p{n}x")).collect();
    assert!(read(&format!("[plugin]\nrequires = {}\n", many.join(","))).is_empty());
    let fits: Vec<String> = (0..MAX_REQUIRED).map(|n| format!("p{n}x")).collect();
    assert_eq!(
        read(&format!("[plugin]\nrequires = {}\n", fits.join(","))),
        fits
    );
}

#[test]
fn a_manifest_over_the_bound_requires_nothing() {
    let big = format!("[plugin]\nrequires = beta\n{}", "#".repeat(17 * 1024));
    assert!(read(&big).is_empty());
}
