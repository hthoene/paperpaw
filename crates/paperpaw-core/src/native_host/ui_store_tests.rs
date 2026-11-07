//! The hand-over store: states, limits, text parts, the seam calls.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
extern crate std;
use super::*;
use alloc::string::String;
use std::format;

fn who(owner: u64, epoch: u64) -> Stamp {
    Stamp::new(owner, epoch).unwrap()
}

fn p<'a>(key: &'a str, value: &'a str) -> Param<'a> {
    Param { key, value }
}

fn store() -> UiStore {
    UiStore::new(Limits::ELSEWHERE)
}

fn read_params(
    store: &UiStore,
    instance: u32,
    handover: u32,
) -> Result<Vec<(String, String)>, i32> {
    let mut out = std::vec![0; 4096];
    let length = store.params(instance, handover, &mut out)?;
    let mut rows = Vec::new();
    let mut at = 0;
    while at < length {
        let (record, used) = wire::record_decode_prefix(&out[at..length]).unwrap();
        let Record::ServiceUiParam(param) = record else {
            panic!()
        };
        rows.push((param.key.into(), param.value.into()));
        at += used;
    }
    Ok(rows)
}

fn read_text(store: &UiStore, instance: u32, handover: u32) -> Result<String, i32> {
    let mut all = Vec::new();
    let mut offset = 0;
    loop {
        let mut out = [0_u8; 7];
        let (total, n) = store.text_read(instance, handover, offset, &mut out)?;
        all.extend_from_slice(&out[..n]);
        offset += n;
        if offset >= total {
            return Ok(String::from_utf8(all).unwrap());
        }
        assert!(n > 0);
    }
}

#[test]
fn ids_are_nonzero_never_reused_and_full_at_the_top() {
    let s = store();
    let a = s.alloc_id().unwrap();
    let b = s.alloc_id().unwrap();
    assert!(a != 0 && b == a + 1);
    s.next.set(u32::MAX - 1);
    assert_eq!(s.alloc_id(), Ok(u32::MAX - 1));
    assert_eq!(s.alloc_id(), Err(ERR_FULL));
    assert_eq!(s.alloc_id(), Err(ERR_FULL));
}

#[test]
fn no_epoch_is_a_state_error() {
    assert_eq!(Stamp::new(1, 0), Err(ERR_STATE));
}

#[test]
fn make_claim_attach_params_and_the_old_one_goes_after() {
    let s = store();
    let first = s
        .make(who(1, 1), &[p("book", "a.epub"), p("page", "3")])
        .unwrap();
    s.check(who(1, 1), first).unwrap();
    s.claim(first, 7).unwrap();
    assert_eq!(
        s.info(7),
        Some(Info {
            handover: first,
            params: 2,
            text_bytes: 0
        })
    );
    assert_eq!(
        read_params(&s, 7, first).unwrap(),
        [
            ("book".into(), "a.epub".into()),
            ("page".into(), "3".into())
        ]
    );
    // A reopen gives the instance a new one: the old is freed at attach only.
    let second = s.make(who(1, 2), &[p("page", "9")]).unwrap();
    s.check(who(1, 2), second).unwrap();
    assert_eq!(s.len(), 2);
    s.claim(second, 7).unwrap();
    assert_eq!(s.len(), 1);
    assert_eq!(read_params(&s, 7, first).err(), Some(ERR_STATE));
    assert_eq!(read_params(&s, 7, second).unwrap().len(), 1);
    // Another instance does not read it.
    assert_eq!(read_params(&s, 8, second).err(), Some(ERR_STATE));
    // Hand-over 0 clears the old data; close frees it.
    s.release_instance(7);
    assert_eq!(s.len(), 0);
    assert_eq!(s.held(), (0, 0));
}

#[test]
fn a_refused_batch_names_nothing_and_the_old_data_stays() {
    let s = store();
    let old = s.make(who(1, 1), &[p("k", "old")]).unwrap();
    s.check(who(1, 1), old).unwrap();
    s.claim(old, 5).unwrap();
    let kept = s.make(who(1, 2), &[p("k", "kept")]).unwrap();
    s.check(who(1, 2), kept).unwrap();
    let refused = s.make(who(1, 2), &[p("k", "refused")]).unwrap();
    s.check(who(1, 2), refused).unwrap();
    s.unname(who(1, 2), &[refused]);
    // The earlier accepted submit still has its id named; the refused one is
    // made again and can be named by a later submit.
    assert_eq!(s.check(who(1, 2), kept), Err(ERR_STATE));
    s.check(who(1, 2), refused).unwrap();
    assert_eq!(read_params(&s, 5, old).unwrap()[0].1, "old");
    // The callback returns: a no-op or stale open leaves nothing; the old stays.
    s.release_callback(who(1, 2));
    assert_eq!(s.len(), 1);
    assert_eq!(read_params(&s, 5, old).unwrap()[0].1, "old");
}

#[test]
fn check_claim_and_the_epoch_rules() {
    let s = store();
    let id = s.make(who(1, 4), &[p("a", "1")]).unwrap();
    assert_eq!(s.check(who(2, 4), id), Err(ERR_NOT_FOUND), "not its maker");
    assert_eq!(s.check(who(1, 5), id), Err(ERR_STATE), "another callback");
    assert_eq!(s.check(who(1, 4), 999), Err(ERR_NOT_FOUND));
    assert_eq!(s.claim(id, 3), Err(ERR_STATE), "not named yet");
    s.check(who(1, 4), id).unwrap();
    assert_eq!(s.check(who(1, 4), id), Err(ERR_STATE), "named twice");
    // A callback's end frees made and named ones of that epoch only.
    let other = s.make(who(1, 4), &[]).unwrap();
    let later = s.make(who(1, 5), &[]).unwrap();
    s.claim(id, 3).unwrap();
    assert_eq!(s.claim(id, 4), Err(ERR_STATE), "at most once");
    s.release_callback(who(1, 4));
    s.check(who(1, 5), later).unwrap();
    assert_eq!(s.check(who(1, 4), other), Err(ERR_NOT_FOUND));
    assert_eq!(s.len(), 2, "the owned one stays");
}

#[test]
fn an_owner_that_stopped_cannot_be_claimed_for() {
    let s = store();
    let id = s.make(who(1, 1), &[]).unwrap();
    s.check(who(1, 1), id).unwrap();
    s.owner_stopped(1);
    assert_eq!(s.claim(id, 3).err(), Some(ERR_STATE));
    assert_eq!(s.len(), 0);
}

#[test]
fn the_param_limits_32_1024_and_the_form() {
    let s = store();
    let keys: Vec<String> = (0..33).map(|n| format!("k{n}")).collect();
    let many: Vec<Param<'_>> = keys.iter().map(|k| p(k, "")).collect();
    s.make(who(1, 1), &many[..32]).unwrap();
    assert_eq!(s.make(who(1, 1), &many).err(), Some(ERR_NO_SPACE));
    // 1,024 bytes of keys and values in all fit, 1,025 do not.
    let v = "v".repeat(1_020);
    s.make(who(1, 1), &[p("key1", &v)]).unwrap();
    assert_eq!(
        s.make(who(1, 1), &[p("key1", &format!("{v}x"))]).err(),
        Some(ERR_NO_SPACE)
    );
    // Form: empty, long, upper case, duplicate, the host's own prefix.
    for bad in [
        &[p("", "x")][..],
        &[p(&"k".repeat(33), "x")],
        &[p("Key", "x")],
        &[p("a b", "x")],
        &[p("a", "1"), p("a", "2")],
        &[p("core.request", "1")],
    ] {
        assert_eq!(s.make(who(1, 1), bad).err(), Some(ERR_INVALID));
    }
    // A key of 32 and a value with newlines are fine.
    s.make(who(1, 1), &[p(&"k".repeat(32), "line\nbreak")])
        .unwrap();
}

#[test]
fn the_x4_params_share_1536_bytes() {
    let s = UiStore::new(Limits::X4);
    let v = "v".repeat(500);
    for key in ["a", "b", "c"] {
        s.make(who(1, 1), &[p(key, &v)]).unwrap();
    }
    assert_eq!(s.make(who(1, 1), &[p("d", &v)]).err(), Some(ERR_FULL));
    s.release_callback(who(1, 1));
    assert_eq!(s.held().0, 0);
    s.make(who(1, 1), &[p("d", &v)]).unwrap();
}

#[test]
fn a_text_is_written_in_parts_committed_and_read_by_its_instance_only() {
    let s = store();
    let id = s.make(who(1, 1), &[p("t", "x")]).unwrap();
    s.text_write(who(1, 1), id, 0, "häl".as_bytes(), false)
        .unwrap();
    // Uncommitted: the open naming it is ERR_STATE.
    assert_eq!(s.check(who(1, 1), id), Err(ERR_STATE));
    assert_eq!(
        s.text_write(who(1, 1), id, 9, b"x", false),
        Err(ERR_INVALID)
    );
    s.text_write(who(1, 1), id, 4, "lo wörld".as_bytes(), true)
        .unwrap();
    s.check(who(1, 1), id).unwrap();
    // Others cannot write it; after the claim not even the maker.
    assert_eq!(s.text_write(who(2, 1), id, 0, b"", true), Err(ERR_STATE));
    assert_eq!(s.text_write(who(1, 2), id, 0, b"", true), Err(ERR_STATE));
    s.check(who(1, 1), id).unwrap();
    assert_eq!(s.text_write(who(1, 1), id, 0, b"", true), Err(ERR_STATE));
    s.claim(id, 4).unwrap();
    assert_eq!(s.info(4).unwrap().text_bytes, "hällo wörld".len() as u32);
    assert_eq!(read_text(&s, 4, id).unwrap(), "hällo wörld");
    assert_eq!(s.text_read(5, id, 0, &mut [0; 8]).err(), Some(ERR_STATE));
    // Cut only at a UTF-8 boundary.
    let mut out = [0_u8; 2];
    let (total, n) = s.text_read(4, id, 0, &mut out).unwrap();
    assert_eq!((total, n), ("hällo wörld".len(), 1));
    assert_eq!(s.text_read(4, id, 99, &mut out).err(), Some(ERR_INVALID));
    // Freed with the instance.
    s.release_instance(4);
    assert_eq!(s.held(), (0, 0));
}

#[test]
fn a_write_at_zero_starts_over_also_after_last_and_an_empty_text_is_one_last() {
    let s = store();
    let id = s.make(who(1, 1), &[]).unwrap();
    s.text_write(who(1, 1), id, 0, b"first", true).unwrap();
    s.text_write(who(1, 1), id, 0, b"se", false).unwrap();
    assert_eq!(s.check(who(1, 1), id), Err(ERR_STATE), "uncommitted again");
    s.text_write(who(1, 1), id, 2, b"cond", true).unwrap();
    s.text_write(who(1, 1), id, 0, b"", true).unwrap();
    s.check(who(1, 1), id).unwrap();
    s.claim(id, 2).unwrap();
    assert_eq!(read_text(&s, 2, id).unwrap(), "");
    assert_eq!(s.held().1, 0);
}

#[test]
fn the_text_cap_24576_and_the_part_cap_3072() {
    let s = store();
    let id = s.make(who(1, 1), &[]).unwrap();
    let part = [b'a'; 3_072];
    for n in 0..8 {
        s.text_write(who(1, 1), id, n * 3_072, &part, false)
            .unwrap();
    }
    assert_eq!(
        s.text_write(who(1, 1), id, 24_576, b"x", true),
        Err(ERR_NO_SPACE)
    );
    assert_eq!(
        s.text_write(who(1, 1), id, 24_576, &[0; 3_073], true),
        Err(ERR_NO_SPACE)
    );
    // Not cut: the text stays at the parts before.
    s.text_write(who(1, 1), id, 24_576, b"", true).unwrap();
    assert_eq!(s.held().1, 24_576);
}

#[test]
fn the_x4_has_four_texts_and_2_kib_without_a_card() {
    let s = UiStore::new(Limits::X4);
    let mut ids = Vec::new();
    for _ in 0..4 {
        let id = s.make(who(1, 1), &[]).unwrap();
        s.text_write(who(1, 1), id, 0, b"t", true).unwrap();
        ids.push(id);
    }
    let fifth = s.make(who(1, 1), &[]).unwrap();
    assert_eq!(s.text_write(who(1, 1), fifth, 0, b"t", true), Err(ERR_FULL));
    s.text_write(who(1, 1), ids[0], 0, &[b'z'; 1_900], true)
        .unwrap();
    assert_eq!(
        s.text_write(who(1, 1), ids[1], 0, &[b'z'; 200], true),
        Err(ERR_IO)
    );
    assert_eq!(
        s.check(who(1, 1), ids[1]),
        Ok(()),
        "kept at the text before"
    );
}

#[test]
fn teardown_frees_what_a_plugin_made_and_keeps_what_another_instance_owns() {
    let s = store();
    let handed = s.make(who(1, 1), &[p("a", "1")]).unwrap();
    s.check(who(1, 1), handed).unwrap();
    s.claim(handed, 9).unwrap();
    let made = s.make(who(1, 2), &[p("b", "2")]).unwrap();
    let claimed = s.make(who(1, 2), &[p("c", "3")]).unwrap();
    s.check(who(1, 2), claimed).unwrap();
    let other = s.make(who(2, 1), &[]).unwrap();
    s.owner_stopped(1);
    assert_eq!(s.len(), 2, "the owned one and the other plugin's");
    assert_eq!(read_params(&s, 9, handed).unwrap().len(), 1);
    assert_eq!(s.check(who(1, 2), made), Err(ERR_NOT_FOUND));
    assert_eq!(s.claim(claimed, 3).err(), Some(ERR_STATE));
    s.check(who(2, 1), other).unwrap();
    // The instance closes: the last owner is gone.
    s.release_instance(9);
    assert_eq!(s.len(), 1);
}

#[test]
fn nothing_shows_in_a_debug_print() {
    let s = store();
    let id = s.make(who(1, 1), &[p("k", "hunter2")]).unwrap();
    s.text_write(who(1, 1), id, 0, b"hunter2", true).unwrap();
    s.check(who(1, 1), id).unwrap();
    assert!(!format!("{s:?}").contains("hunter2"));
}

#[test]
fn every_failed_allocation_leaves_the_store_unchanged() {
    use crate::native_host::shell::tests::FAIL_NTH;
    let s = store();
    let id = s.make(who(1, 1), &[p("k", "v")]).unwrap();
    for nth in 1..=4 {
        FAIL_NTH.with(|n| n.set(nth));
        let made = s.make(who(1, 1), &[p("a", "1")]);
        let wrote = s.text_write(who(1, 1), id, 0, b"text", true);
        FAIL_NTH.with(|n| n.set(0));
        for error in [made.err(), wrote.err()].into_iter().flatten() {
            assert_eq!(error, ERR_NO_MEMORY, "allocation {nth}");
        }
        s.release_callback(who(1, 99));
    }
    // Unchanged ledger: only what succeeded is held.
    let (params, text) = s.held();
    assert!(params > 0 && text <= 4);
    let count = s.len();
    FAIL_NTH.with(|n| n.set(1));
    let refused = s.make(who(1, 1), &[p("zz", "1")]);
    FAIL_NTH.with(|n| n.set(0));
    assert_eq!(refused.err(), Some(ERR_NO_MEMORY));
    assert_eq!((s.held().0, s.len()), (params, count));
}
