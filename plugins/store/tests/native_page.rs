//! The rows of a plugin's page and the words for how an install ended
//! (slice 6, host-free).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "fixture errors fail the tests"
)]
use paperpaw_plugin_store::native_model::Entry;
use paperpaw_plugin_store::native_page::{
    Installed, Kind, Origin, PageInput, Row, Trouble, actions, local as local_row, may_install,
    newer, offers, rows, unbuilt,
};
use paperpaw_plugin_store::native_refusal::{Outcome, after_restart, is_data, outcome};

fn entry(version: &str, code: bool) -> Entry {
    Entry {
        id: "clock".into(),
        name: "Clock".into(),
        version: version.into(),
        code,
        ..Entry::default()
    }
}

fn have(kind: Kind, version: &'static str, origin: Origin) -> Installed<'static> {
    Installed {
        kind,
        enabled: true,
        restart_needed: false,
        version,
        origin,
        release: None,
        trouble: None,
    }
}

fn page<'a>(entry: &'a Entry, installed: Option<&'a Installed<'a>>) -> PageInput<'a> {
    PageInput {
        entry,
        installed,
        own_job: false,
        notice: false,
        no_room: false,
        takes_betas: false,
        has_description: false,
        has_shots: false,
        has_source: false,
    }
}

#[test]
fn a_plugin_not_installed_offers_install_or_why_not() {
    let e = entry("1.0.0", false);
    assert_eq!(rows(&page(&e, None)).unwrap(), [Row::Install]);
    let mut input = page(&e, None);
    input.no_room = true;
    assert_eq!(rows(&input).unwrap(), [Row::NoRoom]);
    let newer_build = Entry {
        needs_newer: true,
        ..e.clone()
    };
    assert_eq!(rows(&page(&newer_build, None)).unwrap(), [Row::NeedsNewer]);
    let waits = Entry {
        waits_author: true,
        ..e.clone()
    };
    assert_eq!(rows(&page(&waits, None)).unwrap(), [Row::NoBuild]);
    // Unbuilt wins over no room.
    let mut input = page(&waits, None);
    input.no_room = true;
    assert_eq!(rows(&input).unwrap(), [Row::NoBuild]);
}

#[test]
fn a_job_or_the_notice_stands_above_the_rest() {
    let e = entry("1.0.0", true);
    let mut input = page(&e, None);
    input.own_job = true;
    assert_eq!(rows(&input).unwrap(), [Row::Stop]);
    let mut input = page(&e, None);
    input.notice = true;
    input.has_description = true;
    assert_eq!(
        rows(&input).unwrap(),
        [Row::Understood, Row::Cancel],
        "no reading rows over the notice"
    );
}

#[test]
fn an_installed_plugin_offers_update_use_store_or_remove() {
    let e = entry("1.1.0", false);
    let store = have(Kind::Data, "1.0.0", Origin::Store);
    assert_eq!(
        rows(&page(&e, Some(&store))).unwrap(),
        [Row::Update, Row::Remove]
    );
    let same = entry("1.0.0", false);
    assert_eq!(rows(&page(&same, Some(&store))).unwrap(), [Row::Remove]);
    // Put in place another way: never an update, the store's may be used.
    let local = have(Kind::Data, "1.0.0", Origin::Local);
    assert_eq!(
        rows(&page(&e, Some(&local))).unwrap(),
        [Row::UseStore, Row::Remove]
    );
    assert!(!newer(&e, &local, true));
    assert!(local_row(&e, &local));
    // Switched off: nothing to do here but what the plugin manager does.
    let off = Installed {
        enabled: false,
        ..store.clone()
    };
    assert!(rows(&page(&e, Some(&off))).unwrap().is_empty());
    // A restart waited for comes first.
    let waiting = Installed {
        restart_needed: true,
        ..store.clone()
    };
    assert_eq!(
        rows(&page(&e, Some(&waiting))).unwrap(),
        [Row::Restart, Row::Update, Row::Remove]
    );
}

#[test]
fn built_in_plugins_are_switched_on_never_downloaded() {
    let e = entry("9.0.0", false);
    let built_in = have(Kind::BuiltIn, "1.0.0", Origin::Store);
    assert!(rows(&page(&e, Some(&built_in))).unwrap().is_empty());
    let off = Installed {
        enabled: false,
        restart_needed: true,
        ..built_in
    };
    assert_eq!(
        rows(&page(&e, Some(&off))).unwrap(),
        [Row::Restart, Row::SwitchOn]
    );
}

#[test]
fn one_kind_does_not_replace_the_other() {
    assert!(may_install(None, true) && may_install(None, false));
    assert!(!may_install(Some(Kind::BuiltIn), false));
    assert!(may_install(Some(Kind::Data), false) && !may_install(Some(Kind::Data), true));
    assert!(may_install(Some(Kind::Code), true) && !may_install(Some(Kind::Code), false));
    let code = entry("2.0.0", true);
    let data = have(Kind::Data, "1.0.0", Origin::Store);
    assert!(rows(&page(&code, Some(&data))).unwrap().is_empty());
}

#[test]
fn an_unbuilt_update_keeps_what_is_installed() {
    let e = Entry {
        needs_newer: true,
        ..entry("2.0.0", true)
    };
    assert!(unbuilt(&e));
    let store = have(Kind::Code, "1.0.0", Origin::Store);
    assert_eq!(
        rows(&page(&e, Some(&store))).unwrap(),
        [Row::NeedsNewer, Row::Remove]
    );
    assert!(!newer(&e, &store, true));
}

#[test]
fn the_same_version_built_for_this_paperpaw_repairs_one_built_for_another() {
    let e = entry("1.0.0", true);
    let broken = Installed {
        trouble: Some(Trouble::OtherBuild),
        origin: Origin::Local,
        ..have(Kind::Code, "1.0.0", Origin::Local)
    };
    assert!(newer(&e, &broken, false), "whatever put it there");
    assert_eq!(
        rows(&page(&e, Some(&broken))).unwrap(),
        [Row::Update, Row::Remove]
    );
    // Data plugins have no build to repair.
    let data = entry("1.0.0", false);
    assert!(!newer(&data, &broken, false));
}

#[test]
fn betas_are_updates_in_their_order_where_they_are_taken() {
    assert!(offers("0.5.0-beta.2", "0.5.0-beta.1", true));
    assert!(!offers("0.5.0-beta.2", "0.5.0-beta.1", false));
    assert!(offers("0.5.0", "0.5.0-beta.2", false));
    assert!(!offers("0.5.0-beta.1", "0.5.0-beta.2", true));
    assert!(offers("0.5.0-beta.10", "0.5.0-beta.9", true));
    assert!(!offers("1.0.0", "1.0.0", true));
    assert!(!offers("not.a.version", "1.0.0", true));
    // The release as the store named it is what is compared.
    let e = entry("0.5.0-beta.2", true);
    let installed = Installed {
        release: Some("0.5.0-beta.1"),
        ..have(Kind::Code, "0.5.0", Origin::Store)
    };
    assert!(newer(&e, &installed, true));
    assert!(!newer(&e, &installed, false));
}

#[test]
fn what_there_is_to_read_follows_the_actions() {
    let e = entry("1.0.0", false);
    let mut input = page(&e, None);
    input.has_description = true;
    input.has_shots = true;
    input.has_source = true;
    assert_eq!(
        rows(&input).unwrap(),
        [Row::Install, Row::How, Row::Shots, Row::Source]
    );
    assert_eq!(actions(&input).unwrap(), [Row::Install]);
}

#[test]
fn every_reason_of_the_host_has_words_in_both_languages() {
    use paperpaw_i18n::Catalog;
    let en = Catalog::parse(include_str!("../i18n/en.ini")).unwrap();
    let de = Catalog::parse(include_str!("../i18n/de.ini")).unwrap();
    let mut said = 0;
    for reason in 0..=21 {
        for flash in [false, true] {
            match outcome(reason, flash) {
                Outcome::Said(key) => {
                    assert!(en.get(key).is_some(), "{key} in en");
                    assert!(de.get(key).is_some(), "{key} in de");
                    said += 1;
                }
                Outcome::Done => assert_eq!(reason, 0),
                Outcome::Silent => assert!([2, 3, 20].contains(&reason)),
                _ => unreachable!(),
            }
        }
    }
    assert!(said > 30);
    // A value this list does not know reads as "other".
    assert_eq!(outcome(99, false), Outcome::Said("store.refused.other"));
    assert_eq!(outcome(1, false), Outcome::Said("store.refused.other"));
    assert_eq!(
        outcome(13, true),
        Outcome::Said("store.refused.no-room-flash")
    );
    assert_eq!(outcome(13, false), Outcome::Said("store.refused.no-room"));
    assert!(after_restart(2) && !after_restart(1) && is_data(1) && !is_data(2));
}
