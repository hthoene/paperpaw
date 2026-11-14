//! What the device has installed and what goes on with the installs, as the
//! screens read it (host-free): the rows as the registry gives them, who put
//! each version in place included, sorted; and the activity put into
//! `Installs`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "fixture errors fail the tests"
)]
use paperpaw_plugin_store::native_installs::{Activity, HostInstalls, Row, Snapshot};
use paperpaw_plugin_store::native_page::{Kind, Origin, Trouble};
use paperpaw_plugin_store::native_queue::{Mark, Queue};
use paperpaw_plugin_store::native_screens::{Installs, Step};

fn row(id: &str, kind: Kind, version: &str) -> Row {
    Row::new(id, kind, true, false, version, None).unwrap()
}

fn rows() -> Vec<Row> {
    vec![
        row("zeta", Kind::Data, "1.0.0"),
        row("alpha", Kind::Code, "1.2.0"),
        row("builtin", Kind::BuiltIn, "1.0.0"),
        Row::new(
            "beta",
            Kind::Code,
            true,
            true,
            "2.0.0",
            Some(Trouble::OtherBuild),
        )
        .unwrap(),
    ]
}

#[test]
fn the_rows_are_sorted_and_found_by_id() {
    let snapshot = Snapshot::settle(rows());
    let ids: Vec<_> = snapshot.rows().iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, ["alpha", "beta", "builtin", "zeta"]);
    let beta = snapshot.have("beta").unwrap();
    assert_eq!(beta.kind, Kind::Code);
    assert!(beta.restart_needed && beta.enabled);
    assert_eq!(beta.version, "2.0.0");
    assert_eq!(beta.trouble, Some(Trouble::OtherBuild));
    assert!(snapshot.have("missing").is_none());
}

#[test]
fn a_version_a_store_installed_is_the_stores_as_it_named_it() {
    // Registry flags: bit 0 enabled, bit 1 restart needed, bit 5 installed by
    // a store.
    let store = 1 | (1 << 5);
    let alpha = Row::from_registry("alpha", Kind::Code, store, ("1.2.0-beta.2", ""), 0).unwrap();
    let zeta = Row::from_registry("zeta", Kind::Data, 1, ("1.0.0", ""), 0).unwrap();
    // Built-in and linked plugins are never a store's, whatever the bit says.
    let linked = Row::from_registry("linked", Kind::BuiltIn, store, ("1.0.0", ""), 0).unwrap();
    // The update waiting for the restart is the version that counts.
    let beta = Row::from_registry("beta", Kind::Code, store, ("1.0.0", "1.1.0"), 10).unwrap();
    let snapshot = Snapshot::settle(vec![zeta, linked, beta, alpha]);
    let alpha = snapshot.have("alpha").unwrap();
    assert_eq!(alpha.origin, Origin::Store);
    // The release as the store named it, its pre-release with it.
    assert_eq!(alpha.release, Some("1.2.0-beta.2"));
    assert!(alpha.enabled && !alpha.restart_needed);
    let zeta = snapshot.have("zeta").unwrap();
    assert_eq!((zeta.origin, zeta.release), (Origin::Local, None));
    let linked = snapshot.have("linked").unwrap();
    assert_eq!((linked.origin, linked.release), (Origin::Local, None));
    let beta = snapshot.have("beta").unwrap();
    assert_eq!((beta.origin, beta.release), (Origin::Store, Some("1.1.0")));
    assert!(beta.restart_needed && beta.version == "1.1.0");
    assert_eq!(beta.trouble, Some(Trouble::OtherBuild));
}

#[test]
fn the_activity_is_what_the_screens_read() {
    let snapshot = Snapshot::settle(rows());
    let mut activity = Activity::default();
    let idle = HostInstalls::new(&snapshot, &activity);
    assert_eq!(idle.step("alpha"), Step::Idle);
    assert!(!idle.queued() && !idle.waiting("alpha") && idle.other_job("alpha").is_none());
    activity
        .step
        .push(("alpha".to_owned(), Step::Downloading { done: 1, total: 3 }));
    activity.queue.add(["alpha", "zeta"], Mark::Said).unwrap();
    activity.revoked.push("zeta".to_owned());
    activity.no_room.push("beta".to_owned());
    activity.restart_needed.push("beta".to_owned());
    activity.actions.push("fonts-browser.open".to_owned());
    activity.other_job = Some(("alpha".to_owned(), "Alpha".to_owned()));
    activity.flash_hint = true;
    activity.can_install = true;
    activity.takes_betas = true;
    let busy = HostInstalls::new(&snapshot, &activity);
    assert_eq!(busy.step("alpha"), Step::Downloading { done: 1, total: 3 });
    assert!(busy.queued() && busy.waiting("zeta") && !busy.waiting("beta"));
    assert!(busy.revoked("zeta") && !busy.revoked("alpha"));
    assert!(busy.no_room("beta") && busy.restart_needed("beta"));
    assert!(busy.has_action("fonts-browser.open") && !busy.has_action("other"));
    // The job that goes on is another plugin's, not this one's own.
    assert_eq!(busy.other_job("alpha"), None);
    assert_eq!(busy.other_job("beta"), Some("Alpha"));
    assert!(busy.flash_hint() && busy.can_install() && busy.takes_betas());
    assert_eq!(busy.have("alpha").unwrap().version, "1.2.0");
    let _ = Queue::default();
}

#[test]
fn the_notes_are_kept_once_and_the_job_follows_its_plugin() {
    let mut activity = Activity::default();
    activity.note_no_room("alpha").unwrap();
    activity.note_no_room("alpha").unwrap();
    activity.note_no_room("beta").unwrap();
    activity.note_revoked("alpha").unwrap();
    activity.note_revoked("alpha").unwrap();
    assert_eq!(activity.no_room, ["alpha", "beta"]);
    assert_eq!(activity.revoked, ["alpha"]);
    activity.clear_no_room("alpha");
    assert_eq!(activity.no_room, ["beta"]);
    activity.set_other_job("alpha", "Alpha").unwrap();
    activity.set_other_job("beta", "Beta").unwrap();
    let snapshot = Snapshot::default();
    let installs = HostInstalls::new(&snapshot, &activity);
    assert_eq!(installs.other_job("alpha"), Some("Beta"));
    assert_eq!(installs.other_job("beta"), None);
    assert!(installs.no_room("beta") && !installs.no_room("alpha"));
    assert!(installs.revoked("alpha") && !installs.revoked("beta"));
    activity.clear_other_job();
    assert_eq!(
        HostInstalls::new(&snapshot, &activity).other_job("alpha"),
        None
    );
}

#[test]
fn the_restart_list_is_the_snapshots_waiting_plugins() {
    let snapshot = Snapshot::settle(rows());
    let mut activity = Activity::default();
    activity.set_restart_needed(Some(&snapshot)).unwrap();
    assert_eq!(activity.restart_needed, ["beta"]);
    assert!(HostInstalls::new(&snapshot, &activity).restart_needed("beta"));
    assert!(!HostInstalls::new(&snapshot, &activity).restart_needed("alpha"));
    // Without a snapshot nothing waits.
    activity.set_restart_needed(None).unwrap();
    assert!(activity.restart_needed.is_empty());
}

#[test]
fn a_pre_release_firmware_version_is_x_dash_y() {
    use paperpaw_plugin_store::native_page::is_pre_release;
    assert!(is_pre_release("0.5.0-beta.1"));
    assert!(is_pre_release("0.5.0-beta.1+abc"));
    assert!(!is_pre_release("0.5.0"));
    assert!(!is_pre_release("0.5.0+build-7"));
}
