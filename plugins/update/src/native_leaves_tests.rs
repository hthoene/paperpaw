//! What an update leaves out: the ids the answer's crate list lacks, the reasons
//! the server gives, and every refused allocation on the way.
#![allow(clippy::unwrap_used, reason = "test assertions")]
use super::*;
use crate::fail_alloc::sweep;
use alloc::{string::ToString, vec};

fn offer(plugins: &str, missing: &str) -> Offer {
    Offer {
        version: "0.5.0".to_string(),
        plugins: plugins.to_string(),
        missing: missing.to_string(),
        ..Offer::default()
    }
}

const BUILT_IN: [&str; 4] = ["stats", "wifi", "dictionary", "clock"];

#[test]
fn what_is_built_in_and_not_among_the_crates_of_the_answer_is_left_out() {
    let answer = offer(
        "paperpaw-plugin-stats 0.5.0\npaperpaw-plugin-wifi 0.5.0\n",
        "",
    );
    assert_eq!(
        of_offer(BUILT_IN, &answer).unwrap(),
        vec!["dictionary".to_string(), "clock".to_string()]
    );
    // All of it comes along: nothing is left out.
    let all = offer(
        "paperpaw-plugin-stats 1\npaperpaw-plugin-wifi 1\npaperpaw-plugin-dictionary 1\npaperpaw-plugin-clock 1\n",
        "",
    );
    assert!(of_offer(BUILT_IN, &all).unwrap().is_empty());
}

#[test]
fn an_answer_that_lists_no_crate_says_nothing_of_what_it_has() {
    // The device's check of the file says it once the update is on the card.
    assert!(of_offer(BUILT_IN, &offer("", "")).unwrap().is_empty());
    assert!(
        of_offer(BUILT_IN, &offer("\n  \nnoversion\n", ""))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn a_crate_is_named_by_its_prefix_and_a_plugin_is_named_once() {
    let answer = offer("paperpaw-plugin-stats 1\nstats 1\nother-wifi 2\n", "");
    // `stats` and `wifi` without the first-party prefix are not the plugin's crates.
    assert_eq!(
        of_offer(["stats", "wifi", "wifi", "stat"], &answer).unwrap(),
        vec!["wifi".to_string(), "stat".to_string()]
    );
}

#[test]
fn no_more_plugins_are_named_than_a_device_holds() {
    let ids: Vec<String> = (0..MAX_LEAVES + 40).map(|n| n.to_string()).collect();
    let answer = offer("paperpaw-plugin-x 1\n", "");
    let left = of_offer(ids.iter().map(String::as_str), &answer).unwrap();
    assert_eq!(left.len(), MAX_LEAVES);
    assert_eq!(left.first().map(String::as_str), Some("0"));
}

#[test]
fn the_server_says_why_a_crate_is_missing_where_it_says() {
    let answer = offer(
        "paperpaw-plugin-stats 1\n",
        "paperpaw-plugin-wifi not-ready\npaperpaw-plugin-dictionary unknown\npaperpaw-plugin-clock soon\nstats not-ready\n",
    );
    assert_eq!(why(&answer, "wifi"), Some(Why::NotReady));
    assert_eq!(why(&answer, "dictionary"), Some(Why::Unknown));
    // A word it does not know, a crate without the prefix, a plugin not named.
    assert_eq!(why(&answer, "clock"), None);
    assert_eq!(why(&answer, "stats"), None);
    assert_eq!(why(&answer, "none"), None);
    assert_eq!(Why::Unknown.key(), "update.missing.unknown");
    assert_eq!(Why::NotReady.key(), "update.missing.not-ready");
}

#[test]
fn a_reason_follows_the_name_in_brackets() {
    assert_eq!(
        with_reason("Stats", "not ready yet").unwrap(),
        "Stats (not ready yet)"
    );
}

#[test]
fn every_refused_allocation_is_no_memory_and_the_run_recovers() {
    let answer = offer(
        "paperpaw-plugin-stats 1\n",
        "paperpaw-plugin-wifi not-ready\n",
    );
    let count = sweep(|| of_offer(BUILT_IN, &answer).map(|left| left.len()));
    assert!(count > 0);
    sweep(|| with_reason("Stats", "not ready yet").map(|text| text.len()));
}

/// The device's list as pages of ids; `script` answers the n-th call of the
/// reader (a cursor is the page number, 0 the start).
type Answer<'a> = Result<(&'a [&'a str], Option<u8>), Error>;

fn read(script: &[Answer<'_>]) -> (Result<Vec<String>, Error>, Vec<u8>) {
    let mut asked = Vec::new();
    let mut calls = script.iter();
    let ids = read_all(0_u8, |cursor, ids| {
        asked.push(cursor);
        let (rows, next) = calls.next().cloned().unwrap_or(Err(Error::Invalid))?;
        ids.extend(rows.iter().map(ToString::to_string));
        Ok(next)
    });
    (ids, asked)
}

#[test]
fn the_pages_of_the_list_are_read_to_the_end() {
    let (ids, asked) = read(&[Ok((&["a", "b"], Some(1))), Ok((&["c"], None))]);
    assert_eq!(ids.unwrap(), ["a", "b", "c"]);
    assert_eq!(asked, [0, 1]);
    // None left out: no rows.
    assert!(read(&[Ok((&[], None))]).0.unwrap().is_empty());
}

#[test]
fn a_check_still_running_is_not_an_answer_nor_a_list() {
    // ERR_BUSY on the first page: no list, for the caller to ask again later.
    let (ids, asked) = read(&[Err(Error::Busy)]);
    assert_eq!(ids, Err(Error::Busy));
    assert_eq!(asked, [0]);
    // Also in the middle: what was read is not returned as the list.
    let (ids, _) = read(&[Ok((&["a"], Some(1))), Err(Error::Busy)]);
    assert_eq!(ids, Err(Error::Busy));
}

#[test]
fn a_cursor_of_an_earlier_check_starts_the_read_again_at_page_zero() {
    // ERR_STATE on page 1: again at page 0 (generation 0), the first page's
    // rows are not kept twice.
    let (ids, asked) = read(&[
        Ok((&["old"], Some(1))),
        Err(Error::State),
        Ok((&["a"], Some(1))),
        Ok((&["b"], None)),
    ]);
    assert_eq!(ids.unwrap(), ["a", "b"]);
    assert_eq!(asked, [0, 1, 0, 1]);
}

#[test]
fn a_cursor_that_stays_stale_is_given_up_after_a_few_restarts() {
    let (ids, asked) = read(&[Err(Error::State); 8]);
    assert_eq!(ids, Err(Error::State));
    assert_eq!(asked.len(), usize::from(MOST_RESTARTS) + 1);
    assert!(asked.iter().all(|cursor| *cursor == 0));
    // The last restart still reads to the end.
    let (ids, _) = read(&[Err(Error::State), Err(Error::State), Ok((&["a"], None))]);
    assert_eq!(ids.unwrap(), ["a"]);
}

#[test]
fn the_other_refusals_are_returned_for_the_file_to_be_unfit() {
    for error in [Error::NotFound, Error::Unsupported, Error::NoMemory] {
        let (ids, asked) = read(&[Err(error)]);
        assert_eq!(ids, Err(error));
        assert_eq!(asked, [0]);
    }
}
