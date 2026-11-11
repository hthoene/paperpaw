//! The page's decisions (host-free): its facts, what *Install* does, the line
//! under the rows, what a row says at its end, sizes, and that every text it
//! says is in both languages.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "fixture errors fail the tests"
)]
use paperpaw_plugin_store::native_item::More;
use paperpaw_plugin_store::native_model::Entry;
use paperpaw_plugin_store::native_page::{Installed, Kind, Origin, Row};
use paperpaw_plugin_store::native_screen_page::{
    Check, Choice, Fact, FactInput, Go, Note, Value, facts, first_action, go, how_text, note,
    progress_text, short_key, size_text, title_key, value_of,
};
use paperpaw_plugin_store::native_screens::{Step, Trouble};

fn entry() -> Entry {
    Entry {
        id: "clock".into(),
        name: "Clock".into(),
        summary: "Tells the time.".into(),
        author: "Ann".into(),
        licence: "MIT".into(),
        version: "1.2.0".into(),
        sdk: "0.4".into(),
        size: 2048,
        ..Entry::default()
    }
}

fn installed(kind: Kind, enabled: bool) -> Installed<'static> {
    Installed {
        kind,
        enabled,
        restart_needed: false,
        version: "1.0.0",
        origin: Origin::Store,
        release: None,
        trouble: None,
    }
}

fn input<'a>(e: &'a Entry, m: Option<&'a More>, i: Option<&'a Installed<'a>>) -> FactInput<'a> {
    FactInput {
        entry: e,
        more: m,
        installed: i,
        revoked: false,
        small: false,
    }
}

#[test]
fn a_data_plugin_of_the_list_states_what_the_list_says() {
    let e = entry();
    let f = facts(&input(&e, None, None)).unwrap();
    assert_eq!(
        f,
        [
            Fact::Kind {
                code: false,
                sdk: "0.4"
            },
            Fact::By {
                author: "Ann",
                since: None
            },
            Fact::Version {
                version: "1.2.0",
                size: 2048,
                licence: "MIT",
                built_in: false
            },
        ]
    );
}

#[test]
fn the_facts_come_in_beta_3s_order() {
    let key = "ab".repeat(32);
    let e = Entry {
        code: true,
        unreviewed: true,
        ..entry()
    };
    let more = More {
        repository: "https://git.example/x".into(),
        author_key: key.clone(),
        author_since: 20240102,
        author_plugins: 3,
        verified: "failed".into(),
        heap: Some(40 * 1024),
        ..More::default()
    };
    let have = installed(Kind::Code, true);
    let mut i = input(&e, Some(&more), Some(&have));
    i.revoked = true;
    i.small = true;
    let f = facts(&i).unwrap();
    assert_eq!(
        f,
        [
            Fact::Revoked,
            Fact::Unreviewed {
                repository: "git.example/x"
            },
            Fact::Kind {
                code: true,
                sdk: "0.4"
            },
            Fact::Check(Check::Failed),
            Fact::By {
                author: "Ann",
                since: Some((2024, 3))
            },
            Fact::Key(&key),
            Fact::Version {
                version: "1.2.0",
                size: 2048,
                licence: "MIT",
                built_in: false
            },
            Fact::Heap {
                bytes: 40 * 1024,
                tight: true
            },
            Fact::Installed("1.0.0"),
        ]
    );
    // Where the memory is not small, the same heap is not tight.
    i.small = false;
    assert!(facts(&i).unwrap().contains(&Fact::Heap {
        bytes: 40 * 1024,
        tight: false
    }));
}

#[test]
fn official_plugins_name_no_author_and_show_no_key() {
    let e = Entry {
        code: true,
        official: true,
        ..entry()
    };
    let more = More {
        author_key: "cd".repeat(32),
        ..More::default()
    };
    let f = facts(&input(&e, Some(&more), None)).unwrap();
    assert!(f.contains(&Fact::Official));
    assert!(
        !f.iter()
            .any(|f| matches!(f, Fact::By { .. } | Fact::Key(_)))
    );
    // A key that is not 64 characters long is not shown.
    let e = Entry {
        code: true,
        ..entry()
    };
    let short = More {
        author_key: "cd".repeat(31),
        ..More::default()
    };
    let f = facts(&input(&e, Some(&short), None)).unwrap();
    assert!(!f.iter().any(|f| matches!(f, Fact::Key(_))));
}

#[test]
fn a_built_in_switched_off_has_no_download_and_no_installed_version() {
    let e = entry();
    let have = installed(Kind::BuiltIn, false);
    let f = facts(&input(&e, None, Some(&have))).unwrap();
    assert!(
        f.iter()
            .any(|f| matches!(f, Fact::Version { built_in: true, .. }))
    );
    assert!(!f.iter().any(|f| matches!(f, Fact::Installed(_))));
    let on = installed(Kind::BuiltIn, true);
    let f = facts(&input(&e, None, Some(&on))).unwrap();
    assert!(f.iter().any(|f| matches!(f, Fact::Installed("1.0.0"))));
}

#[test]
fn an_unreviewed_plugin_says_where_it_comes_from_only_when_told() {
    let e = Entry {
        unreviewed: true,
        ..entry()
    };
    let f = facts(&input(&e, None, None)).unwrap();
    assert_eq!(f.first(), Some(&Fact::Unreviewed { repository: "" }));
}

#[test]
fn the_key_is_cut_to_four_groups_and_never_panics() {
    let key = "0123456789abcdef".repeat(4);
    assert_eq!(
        short_key(&key).unwrap().as_deref(),
        Some("0123 4567 89ab cdef")
    );
    assert_eq!(short_key("abc").unwrap(), None);
    // A multi-byte character across a group's edge: no key, no panic.
    let odd = format!("a{}b", "\u{e9}".repeat(31));
    assert_eq!(odd.len(), 64);
    assert_eq!(short_key(&odd).unwrap(), None);
    let wide = "\u{e9}".repeat(32);
    assert_eq!(wide.len(), 64);
    assert_eq!(
        short_key(&wide).unwrap().as_deref(),
        Some("\u{e9}\u{e9} \u{e9}\u{e9} \u{e9}\u{e9} \u{e9}\u{e9}")
    );
}

fn choice() -> Choice {
    Choice {
        here: true,
        state: Trouble::None,
        other_job: false,
        code: false,
        told: false,
        asked: false,
    }
}

#[test]
fn install_goes_in_the_order_of_beta_3() {
    assert_eq!(go(&choice()), Go::Start);
    let c = Choice {
        here: false,
        state: Trouble::Offline,
        other_job: true,
        code: true,
        ..choice()
    };
    assert_eq!(go(&c), Go::Offline);
    let c = Choice {
        other_job: true,
        code: true,
        ..choice()
    };
    assert_eq!(go(&c), Go::Busy);
    let c = Choice {
        code: true,
        ..choice()
    };
    assert_eq!(go(&c), Go::Notice);
    let c = Choice {
        code: true,
        told: true,
        ..choice()
    };
    assert_eq!(go(&c), Go::Start);
    // The page is awaited, asked for once.
    let c = Choice {
        here: false,
        state: Trouble::Loading,
        ..choice()
    };
    assert_eq!(go(&c), Go::Wait { fetch: true });
    let asked = Choice { asked: true, ..c };
    assert_eq!(go(&asked), Go::Wait { fetch: false });
    let gone = Choice {
        here: false,
        state: Trouble::Failed,
        ..choice()
    };
    assert_eq!(go(&gone), Go::Gone);
    // The notice comes before the page is known.
    let code = Choice { code: true, ..c };
    assert_eq!(go(&code), Go::Notice);
}

#[test]
fn the_note_follows_the_job_then_the_others_then_the_restart() {
    let idle = Step::Idle;
    assert_eq!(
        note(Some(Row::Stop), &idle, Some("Other"), false),
        Note::Downloading
    );
    assert_eq!(
        note(Some(Row::Install), &idle, Some("Other"), false),
        Note::Busy("Other")
    );
    assert_eq!(note(Some(Row::Restart), &idle, None, false), Note::Restart);
    assert_eq!(note(Some(Row::Install), &idle, None, false), Note::Nothing);
    assert_eq!(
        note(Some(Row::Install), &Step::Failed(13), None, true),
        Note::Ended("store.refused.no-room-flash")
    );
    assert_eq!(
        note(Some(Row::Install), &Step::Failed(2), None, false),
        Note::Nothing
    );
    assert_eq!(note(None, &Step::Installed, None, false), Note::Nothing);
}

#[test]
fn rows_say_what_beta_3_said_at_their_end() {
    assert_eq!(value_of(Row::Install, false), Value::Size);
    assert_eq!(value_of(Row::Install, true), Value::Preparing);
    assert_eq!(value_of(Row::Update, false), Value::Version);
    assert_eq!(value_of(Row::UseStore, true), Value::Preparing);
    assert_eq!(value_of(Row::Stop, false), Value::Progress);
    assert_eq!(value_of(Row::NoRoom, false), Value::Size);
    assert_eq!(value_of(Row::Shots, false), Value::Shots);
    assert_eq!(value_of(Row::Source, false), Value::Arrow);
    assert_eq!(value_of(Row::How, false), Value::Arrow);
    assert_eq!(value_of(Row::Remove, false), Value::Nothing);
    assert_eq!(value_of(Row::Stop, true), Value::Progress);
}

#[test]
fn sizes_and_progress_are_written_without_floats() {
    assert_eq!(size_text(820, ".").unwrap(), "820 B");
    assert_eq!(size_text(14 * 1024, ".").unwrap(), "14 KB");
    assert_eq!(size_text(2_411_724, ",").unwrap(), "2,3 MB");
    assert!(size_text(u64::MAX, ".").is_ok());
    assert_eq!(progress_text(1024, 4096, ".").unwrap(), "1.0 KB / 4.0 KB");
    assert_eq!(progress_text(100, 0, ".").unwrap(), "100 B");
}

#[test]
fn how_it_works_opens_the_name_and_the_description() {
    assert_eq!(
        how_text("Clock", "It ticks.").unwrap(),
        "Clock\n\nIt ticks.\n"
    );
}

#[test]
fn the_first_action_is_where_the_focus_starts() {
    assert_eq!(
        first_action(&[None, None, Some(Row::Install), Some(Row::Shots)]),
        2
    );
    assert_eq!(first_action(&[None]), 0);
    assert_eq!(first_action(&[]), 0);
}

#[test]
fn every_row_and_text_has_both_languages() {
    let (en, de) = (
        include_str!("../i18n/en.ini"),
        include_str!("../i18n/de.ini"),
    );
    let has = |ini: &str, key: &str| {
        ini.lines().any(|l| {
            l.split_once('=').is_some_and(|(k, _)| {
                let k = k.trim();
                k == key
                    || k.strip_prefix(key)
                        .is_some_and(|rest| rest == ".one" || rest == ".other")
            })
        })
    };
    let rows = [
        Row::Install,
        Row::Stop,
        Row::Update,
        Row::Remove,
        Row::Restart,
        Row::Understood,
        Row::Cancel,
        Row::UseStore,
        Row::SwitchOn,
        Row::NoRoom,
        Row::NeedsNewer,
        Row::NoBuild,
        Row::Shots,
        Row::Source,
        Row::How,
    ];
    let mut keys: Vec<&str> = rows.iter().map(|r| title_key(*r)).collect();
    keys.extend([
        "store.revoked",
        "store.unreviewed.fact",
        "store.unreviewed.from",
        "store.kind.code",
        "store.kind.data",
        "store.verified",
        "store.verified.failed",
        "store.verified.not",
        "store.official",
        "store.by",
        "store.by-since",
        "store.author-key",
        "store.version",
        "store.version-built-in",
        "store.licence",
        "store.heap",
        "store.heap.tight",
        "store.installed-version",
        "store.preparing",
        "store.downloading",
        "store.busy",
        "store.restart.help",
        "store.code.notice",
        "store.gone",
        "store.gone.help",
        "store.offline",
        "store.offline.page",
        "store.loading-plugin",
        "store.needs-newer.help",
        "store.no-build.help",
        "store.refused.no-room",
        "store.refused.no-room-flash",
        "store.refused.other",
        "store.how-failed",
        "core.decimal",
    ]);
    for key in keys {
        // `core.*` texts are the host's.
        if key.starts_with("core.") {
            continue;
        }
        assert!(has(en, key), "{key} is missing in en");
        assert!(has(de, key), "{key} is missing in de");
    }
}

#[test]
fn the_bytes_of_a_download_do_not_make_the_rows_again() {
    use paperpaw_plugin_store::native_screen_page::calm;
    use paperpaw_plugin_store::native_screens::Step;
    // Two moments of one download are the one signature: only the progress
    // row's value is made again, not the rows (about 13 KiB of texts).
    assert_eq!(
        calm(Step::Downloading {
            done: 10,
            total: 900
        }),
        calm(Step::Downloading {
            done: 500,
            total: 900
        })
    );
    // What does change the rows still does.
    assert_ne!(
        calm(Step::Downloading { done: 1, total: 2 }),
        calm(Step::Installing)
    );
    assert_ne!(calm(Step::Idle), calm(Step::Failed(5)));
    assert_eq!(calm(Step::Failed(5)), Step::Failed(5));
}

#[test]
fn remove_asks_and_only_the_first_option_removes() {
    use paperpaw_plugin_store::native_screen_page::{REMOVE, confirms_remove};
    use paperpaw_plugin_store::native_screens::Reply;
    assert!(confirms_remove(REMOVE, &Reply::Choice(0)));
    // Keep, a text, a closed question and another question's tag do not.
    assert!(!confirms_remove(REMOVE, &Reply::Choice(1)));
    assert!(!confirms_remove(REMOVE, &Reply::Text("0")));
    assert!(!confirms_remove(REMOVE + 1, &Reply::Choice(0)));
}
