//! The record of an update that waits, what it waits for, and when it asks and
//! opens the page.
#![allow(clippy::unwrap_used, reason = "test assertions")]
use super::*;
use alloc::{format, string::ToString, vec};

fn pair(id: &str, version: &str) -> (String, String) {
    (id.to_string(), version.to_string())
}

#[test]
fn what_version_1_wrote_is_read() {
    let text = include_str!("../testdata/waiting.txt.v1");
    let waiting = Waiting::parse(text).unwrap().unwrap();
    assert_eq!(
        waiting,
        Waiting {
            version: "0.5.0".to_string(),
            since: Some(1_760_000_000),
            checked: Some(1_760_086_400),
            asked: false,
            seen: false,
            told: true,
            holding: Holding {
                plugins: vec![pair("dict-xl", "0.3.1")],
                crates: vec!["chess-for-paperpaw".to_string()],
            },
        }
    );
    // The text written is the text read (the file's format did not change).
    assert_eq!(waiting.to_text().unwrap(), text);
    assert_eq!(
        Waiting::parse(&waiting.to_text().unwrap()).unwrap(),
        Some(waiting)
    );
    assert_eq!(
        Waiting::parse("0.5.0\nsince -\nchecked -\n")
            .unwrap()
            .map(|w| w.since),
        Some(None)
    );
    assert_eq!(Waiting::parse("soon\n").unwrap(), None);
}

#[test]
fn the_flags_are_written_where_set() {
    let waiting = Waiting {
        version: "0.5.0".to_string(),
        asked: true,
        seen: true,
        told: true,
        ..Waiting::default()
    };
    assert_eq!(
        waiting.to_text().unwrap(),
        "0.5.0\nsince -\nchecked -\nasked\nseen\ntold\n"
    );
    assert_eq!(
        Waiting::parse(&waiting.to_text().unwrap()).unwrap(),
        Some(waiting)
    );
}

#[test]
fn a_damaged_record_is_none_and_a_large_one_is_read_whole() {
    assert_eq!(Waiting::parse("").unwrap(), None);
    assert_eq!(Waiting::parse("\u{0}\u{1}garbage\n").unwrap(), None);
    // Lines it does not know are passed over.
    assert!(
        Waiting::parse("0.5.0\nsince 12\nwhat is this\n")
            .unwrap()
            .is_some()
    );
    let many = Waiting {
        version: "0.5.0".to_string(),
        since: Some(1),
        checked: Some(2),
        holding: Holding {
            plugins: (0..MAX_ENTRIES)
                .map(|i| {
                    (
                        format!("plugin-with-a-long-name-{i}"),
                        "10.20.30".to_string(),
                    )
                })
                .collect(),
            crates: (0..MAX_ENTRIES)
                .map(|i| format!("paperpaw-plugin-with-a-long-name-{i}"))
                .collect(),
        },
        ..Waiting::default()
    };
    let text = many.to_text().unwrap();
    assert!(text.len() < MAX_LEN, "{} bytes", text.len());
    assert_eq!(Waiting::parse(&text).unwrap(), Some(many));
    // More lines than the plugins a device holds are not kept.
    let mut flood = String::from("0.5.0\n");
    for i in 0..MAX_ENTRIES + 50 {
        flood.push_str(&format!("plugin p{i} 1.0.0\ncrate c{i}\n"));
    }
    let kept = Waiting::parse(&flood).unwrap().unwrap();
    assert_eq!(kept.holding.plugins.len(), MAX_ENTRIES);
    assert_eq!(kept.holding.crates.len(), MAX_ENTRIES);
}

fn offer(waiting: &str, missing: &str, plugins: &str) -> Offer {
    Offer {
        version: "0.5.0".to_string(),
        url: "/u.bin".to_string(),
        waiting: waiting.to_string(),
        missing: missing.to_string(),
        plugins: plugins.to_string(),
        ..Offer::default()
    }
}

#[test]
fn the_answer_names_what_the_update_waits_for() {
    let offer = offer(
        "dict 0.3.1\nchess 1.2.0\nbroken\n",
        "paperpaw-plugin-stats not-ready\npaperpaw-plugin-old unknown\nlonely\n",
        "paperpaw-plugin-reader 0.5.0\n",
    );
    let holding = Holding::of_offer(&offer).unwrap();
    assert_eq!(
        holding,
        Holding {
            plugins: vec![pair("dict", "0.3.1"), pair("chess", "1.2.0")],
            crates: vec!["paperpaw-plugin-stats".to_string()],
        }
    );
    assert!(!holding.is_empty());
    assert!(Holding::of_offer(&Offer::default()).unwrap().is_empty());
    // The crates that never come: not in the library, and not in the update.
    assert_eq!(
        unknown_crates(&offer).unwrap(),
        ["paperpaw-plugin-old".to_string()]
    );
    let named = self::offer(
        "",
        "paperpaw-plugin-old unknown\n",
        "paperpaw-plugin-old 0.5.0\n",
    );
    assert!(unknown_crates(&named).unwrap().is_empty());
}

#[test]
fn only_what_is_still_in_use_and_built_in_holds() {
    let mut holding = Holding {
        plugins: vec![pair("dict", "0.3.1"), pair("chess", "1.2.0")],
        crates: vec!["kept-crate".to_string(), "gone-crate".to_string()],
    };
    holding.keep(&[true, false], &[true, false]);
    assert_eq!(
        holding,
        Holding {
            plugins: vec![pair("dict", "0.3.1")],
            crates: vec!["kept-crate".to_string()],
        }
    );
    // Flags that are missing count as no.
    holding.keep(&[], &[]);
    assert!(holding.is_empty());
}

fn waiting(since: Option<i64>) -> Waiting {
    Waiting {
        version: "0.5.0".to_string(),
        since,
        ..Waiting::default()
    }
}

#[test]
fn asked_once_a_day_and_opened_once_after_a_week() {
    let day = DAY_SECONDS;
    let mut w = waiting(Some(1000));
    w.tried(Some(1000));
    assert_eq!(w.start(Some(1000 + day / 2)), (false, false), "asked today");
    assert_eq!(w.start(Some(1000 + day)), (true, false), "a day on");
    w.tried(Some(1000 + day));
    // Every wake of the day after: not again.
    assert!(!w.start(Some(1000 + day + 60)).0);
    assert_eq!(w.start(Some(1000 + 7 * day)), (true, true), "a week");
    assert!(!w.start(Some(1000 + 8 * day)).1, "once");
    // Looked at: never by itself.
    let mut w = waiting(Some(1000));
    w.seen = true;
    assert!(!w.start(Some(1000 + 20 * day)).1);
}

#[test]
fn without_a_clock_it_asks_once_and_counts_from_when_it_is_known() {
    let day = DAY_SECONDS;
    let mut w = waiting(None);
    assert_eq!(w.start(None), (true, false));
    w.tried(None);
    assert_eq!(w.start(None), (false, false), "not at every wake");
    // The clock known: a question (never one with a time yet), the week from now.
    assert_eq!(w.start(Some(50 * day)), (true, false));
    w.tried(Some(50 * day));
    assert!(!w.start(Some(56 * day)).1);
    assert!(w.start(Some(57 * day)).1);
}

#[test]
fn a_clock_set_back_counts_from_now() {
    let day = DAY_SECONDS;
    let mut w = waiting(Some(100 * day));
    w.tried(Some(100 * day));
    // Set back by years: asked again, the week anew.
    assert_eq!(w.start(Some(10 * day)), (true, false));
    assert_eq!(w.since, Some(10 * day));
    w.tried(Some(10 * day));
    assert!(w.start(Some(11 * day)).0);
    // Times at the ends of the range do not overflow.
    let mut w = waiting(Some(i64::MIN));
    w.checked = Some(i64::MIN);
    let _ = w.start(Some(i64::MAX));
}

#[test]
fn an_answer_keeps_the_first_start_and_counts_as_a_question() {
    let mut w = waiting(None);
    w.answered(Holding::default(), Some(500));
    assert_eq!((w.since, w.checked, w.asked), (Some(500), Some(500), false));
    w.answered(
        Holding {
            plugins: vec![pair("dict", "0.3.1")],
            crates: Vec::new(),
        },
        Some(900),
    );
    assert_eq!((w.since, w.checked), (Some(500), Some(900)));
    assert_eq!(w.holding.plugins.len(), 1);
    // Without a clock: asked, from no time.
    let mut w = waiting(None);
    w.answered(Holding::default(), None);
    assert_eq!((w.since, w.checked, w.asked), (None, None, true));
}

#[test]
fn only_a_newer_update_waits() {
    let w = waiting(None);
    assert!(w.newer_than("0.4.0"));
    assert!(!w.newer_than("0.5.0"), "installed meanwhile");
    assert!(!w.newer_than("0.6.0"));
    assert!(
        Waiting::new("0.5.0", Holding::default())
            .unwrap()
            .newer_than("0.5.0-beta.1")
    );
}
