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

/// What Core's stack does with a made hand-over: check, claim and attach.
fn own(s: &UiStore, who: Stamp, id: u32, instance: u32) {
    s.handover_check(who, id).unwrap();
    let claims = s.claim(who, &[id]).unwrap();
    s.attach(Some(id), instance).unwrap();
    drop(claims);
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
        .handover_make(who(1, 1), &[p("book", "a.epub"), p("page", "3")])
        .unwrap();
    own(&s, who(1, 1), first, 7);
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
    let second = s.handover_make(who(1, 2), &[p("page", "9")]).unwrap();
    s.handover_check(who(1, 2), second).unwrap();
    let claims = s.claim(who(1, 2), &[second]).unwrap();
    assert_eq!(s.len(), 2, "a claim does not free the old one");
    s.attach(Some(second), 7).unwrap();
    drop(claims);
    assert_eq!(s.len(), 1);
    assert_eq!(read_params(&s, 7, first).err(), Some(ERR_STATE));
    assert_eq!(read_params(&s, 7, second).unwrap().len(), 1);
    // Another instance does not read it.
    assert_eq!(read_params(&s, 8, second).err(), Some(ERR_STATE));
    // Hand-over 0 clears the old data; close frees it.
    s.instance_closed(7).unwrap();
    assert_eq!(s.len(), 0);
    assert_eq!(s.held(), (0, 0));
}

#[test]
fn the_entry_list_grows_by_one_and_is_given_back_when_empty() {
    let s = store();
    let a = s.handover_make(who(1, 1), &[p("k", "a")]).unwrap();
    // One entry, room for exactly one (the usual growth takes four).
    assert_eq!(s.inner.borrow().entries.capacity(), 1);
    own(&s, who(1, 1), a, 5);
    s.attach(None, 5).unwrap();
    assert_eq!(s.inner.borrow().entries.len(), 0);
    assert_eq!(s.inner.borrow().entries.capacity(), 0);
}

#[test]
fn a_refused_batch_claims_nothing_and_the_old_data_stays() {
    let s = store();
    let old = s.handover_make(who(1, 1), &[p("k", "old")]).unwrap();
    own(&s, who(1, 1), old, 5);
    let kept = s.handover_make(who(1, 2), &[p("k", "kept")]).unwrap();
    let refused = s.handover_make(who(1, 2), &[p("k", "refused")]).unwrap();
    // The earlier accepted submit claims `kept`; a later submit that names
    // `kept` again claims nothing, not even its other, fine id.
    let first = s.claim(who(1, 2), &[kept]).unwrap();
    assert_eq!(s.claim(who(1, 2), &[refused, kept]).err(), Some(ERR_STATE));
    assert_eq!(
        s.claim(who(1, 2), &[refused, refused]).err(),
        Some(ERR_STATE)
    );
    s.handover_check(who(1, 2), refused).unwrap();
    // A refused submit rolls back its own claims only.
    let second = s.claim(who(1, 2), &[refused]).unwrap();
    s.rollback(second).unwrap();
    assert_eq!(s.handover_check(who(1, 2), refused), Err(ERR_NOT_FOUND));
    assert_eq!(s.handover_check(who(1, 2), kept), Err(ERR_STATE));
    assert_eq!(read_params(&s, 5, old).unwrap()[0].1, "old");
    // The callback returns: the claim stays until its open drains; a no-op
    // open releases it and the instance keeps its old hand-over.
    s.end_callback(who(1, 2)).unwrap();
    assert_eq!(s.len(), 2);
    s.release(kept).unwrap();
    drop(first);
    assert_eq!(s.len(), 1);
    assert_eq!(read_params(&s, 5, old).unwrap()[0].1, "old");
}

#[test]
fn rollback_frees_only_its_own_batch_and_attach_none_clears() {
    let s = store();
    let a = s.handover_make(who(1, 1), &[]).unwrap();
    let ca = s.claim(who(1, 1), &[a]).unwrap();
    s.end_callback(who(1, 1)).unwrap();
    // A later callback's claims roll back without touching the earlier batch.
    let b = s.handover_make(who(1, 2), &[]).unwrap();
    let cb = s.claim(who(1, 2), &[b]).unwrap();
    s.rollback(cb).unwrap();
    assert_eq!(s.len(), 1);
    s.attach(Some(a), 6).unwrap();
    drop(ca);
    assert_eq!(s.info(6).map(|i| i.handover), Some(a));
    // A reopen without a hand-over clears the old params and text.
    s.attach(None, 6).unwrap();
    assert_eq!(s.info(6), None);
    assert_eq!(s.len(), 0);
    // Attaching an id nothing claimed is a state error and changes nothing.
    let m = s.handover_make(who(1, 3), &[]).unwrap();
    assert_eq!(s.attach(Some(m), 6), Err(ERR_STATE));
    assert_eq!(s.attach(Some(999), 6), Err(ERR_STATE));
    assert_eq!(s.len(), 1);
}

#[test]
fn check_claim_and_the_epoch_rules() {
    let s = store();
    let id = s.handover_make(who(1, 4), &[p("a", "1")]).unwrap();
    assert_eq!(
        s.handover_check(who(2, 4), id),
        Err(ERR_NOT_FOUND),
        "not its maker"
    );
    assert_eq!(
        s.handover_check(who(1, 5), id),
        Err(ERR_STATE),
        "another callback"
    );
    assert_eq!(s.handover_check(who(1, 4), 999), Err(ERR_NOT_FOUND));
    assert_eq!(s.claim(who(1, 5), &[id]).err(), Some(ERR_STATE));
    s.handover_check(who(1, 4), id).unwrap();
    s.handover_check(who(1, 4), id).unwrap();
    // A callback's end frees made ones of that epoch only; claimed stay.
    let other = s.handover_make(who(1, 4), &[]).unwrap();
    let later = s.handover_make(who(1, 5), &[]).unwrap();
    let held = s.claim(who(1, 4), &[id]).unwrap();
    assert_eq!(s.claim(who(1, 4), &[id]).err(), Some(ERR_STATE), "once");
    assert_eq!(s.handover_check(who(1, 4), id), Err(ERR_STATE), "claimed");
    s.attach(Some(id), 3).unwrap();
    drop(held);
    s.end_callback(who(1, 4)).unwrap();
    s.handover_check(who(1, 5), later).unwrap();
    assert_eq!(s.handover_check(who(1, 4), other), Err(ERR_NOT_FOUND));
    assert_eq!(s.len(), 2, "the owned one stays");
}

#[test]
fn an_owner_that_stopped_cannot_be_claimed_for() {
    let s = store();
    let id = s.handover_make(who(1, 1), &[]).unwrap();
    let claims = s.claim(who(1, 1), &[id]).unwrap();
    // A made or claimed entry keeps its plugin from sleeping again (#248, D3).
    assert_eq!(s.owner_holds(1), Ok(true));
    s.owner_stopped(1).unwrap();
    assert_eq!(s.owner_holds(1), Ok(false));
    assert_eq!(s.attach(Some(id), 3), Err(ERR_STATE));
    drop(claims);
    assert_eq!(s.len(), 0);
}

#[test]
fn the_param_limits_32_1024_and_the_form() {
    let s = store();
    let keys: Vec<String> = (0..33).map(|n| format!("k{n}")).collect();
    let many: Vec<Param<'_>> = keys.iter().map(|k| p(k, "")).collect();
    s.handover_make(who(1, 1), &many[..32]).unwrap();
    assert_eq!(s.handover_make(who(1, 1), &many).err(), Some(ERR_NO_SPACE));
    // 1,024 bytes of keys and values in all fit, 1,025 do not.
    let v = "v".repeat(1_020);
    s.handover_make(who(1, 1), &[p("key1", &v)]).unwrap();
    assert_eq!(
        s.handover_make(who(1, 1), &[p("key1", &format!("{v}x"))])
            .err(),
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
        assert_eq!(s.handover_make(who(1, 1), bad).err(), Some(ERR_INVALID));
    }
    // A key of 32 and a value with newlines are fine.
    s.handover_make(who(1, 1), &[p(&"k".repeat(32), "line\nbreak")])
        .unwrap();
}

#[test]
fn the_x4_params_share_1536_bytes() {
    let s = UiStore::new(Limits::X4);
    let v = "v".repeat(500);
    for key in ["a", "b", "c"] {
        s.handover_make(who(1, 1), &[p(key, &v)]).unwrap();
    }
    assert_eq!(
        s.handover_make(who(1, 1), &[p("d", &v)]).err(),
        Some(ERR_FULL)
    );
    s.end_callback(who(1, 1)).unwrap();
    assert_eq!(s.held().0, 0);
    s.handover_make(who(1, 1), &[p("d", &v)]).unwrap();
}

#[test]
fn a_text_is_written_in_parts_committed_and_read_by_its_instance_only() {
    let s = store();
    let id = s.handover_make(who(1, 1), &[p("t", "x")]).unwrap();
    s.text_write(who(1, 1), id, 0, "häl".as_bytes(), false)
        .unwrap();
    // Uncommitted: the open naming it is ERR_STATE.
    assert_eq!(s.handover_check(who(1, 1), id), Err(ERR_STATE));
    assert_eq!(
        s.text_write(who(1, 1), id, 9, b"x", false),
        Err(ERR_INVALID)
    );
    s.text_write(who(1, 1), id, 4, "lo wörld".as_bytes(), true)
        .unwrap();
    s.handover_check(who(1, 1), id).unwrap();
    // Others cannot write it; after the claim not even the maker.
    assert_eq!(
        s.text_write(who(2, 1), id, 0, b"", true),
        Err(ERR_NOT_FOUND)
    );
    assert_eq!(s.text_write(who(1, 2), id, 0, b"", true), Err(ERR_STATE));
    let claims = s.claim(who(1, 1), &[id]).unwrap();
    assert_eq!(s.text_write(who(1, 1), id, 0, b"", true), Err(ERR_STATE));
    s.attach(Some(id), 4).unwrap();
    drop(claims);
    assert_eq!(s.info(4).unwrap().text_bytes, "hällo wörld".len() as u32);
    assert_eq!(read_text(&s, 4, id).unwrap(), "hällo wörld");
    assert_eq!(s.text_read(5, id, 0, &mut [0; 8]).err(), Some(ERR_STATE));
    // Cut only at a UTF-8 boundary.
    let mut out = [0_u8; 2];
    let (total, n) = s.text_read(4, id, 0, &mut out).unwrap();
    assert_eq!((total, n), ("hällo wörld".len(), 1));
    assert_eq!(s.text_read(4, id, 99, &mut out).err(), Some(ERR_INVALID));
    // Freed with the instance.
    s.instance_closed(4).unwrap();
    assert_eq!(s.held(), (0, 0));
}

#[test]
fn a_write_at_zero_starts_over_also_after_last_and_an_empty_text_is_one_last() {
    let s = store();
    let id = s.handover_make(who(1, 1), &[]).unwrap();
    s.text_write(who(1, 1), id, 0, b"first", true).unwrap();
    s.text_write(who(1, 1), id, 0, b"se", false).unwrap();
    assert_eq!(
        s.handover_check(who(1, 1), id),
        Err(ERR_STATE),
        "uncommitted again"
    );
    s.text_write(who(1, 1), id, 2, b"cond", true).unwrap();
    s.text_write(who(1, 1), id, 0, b"", true).unwrap();
    own(&s, who(1, 1), id, 2);
    assert_eq!(read_text(&s, 2, id).unwrap(), "");
    assert_eq!(s.held().1, 0);
}

#[test]
fn the_text_cap_24576_and_the_part_cap_3072() {
    let s = store();
    let id = s.handover_make(who(1, 1), &[]).unwrap();
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
    // Eight chunks of 3,072 bytes and their descriptors are charged.
    assert_eq!(s.held().1, 8 * (3_072 + DESCRIPTOR));
}

#[test]
fn the_x4_has_four_texts_and_2_kib_without_a_card() {
    let s = UiStore::new(Limits::X4);
    let mut ids = Vec::new();
    for _ in 0..4 {
        let id = s.handover_make(who(1, 1), &[]).unwrap();
        s.text_write(who(1, 1), id, 0, b"t", true).unwrap();
        ids.push(id);
    }
    let fifth = s.handover_make(who(1, 1), &[]).unwrap();
    assert_eq!(s.text_write(who(1, 1), fifth, 0, b"t", true), Err(ERR_FULL));
    // Each small text is charged a 256-byte chunk and its descriptor.
    assert_eq!(s.held().1, 4 * (CHUNK + DESCRIPTOR));
    s.text_write(who(1, 1), ids[0], 0, &[b'z'; 1_100], true)
        .unwrap();
    assert_eq!(
        s.text_write(who(1, 1), ids[1], 0, &[b'z'; 400], true),
        Err(ERR_IO)
    );
    assert_eq!(
        s.handover_check(who(1, 1), ids[1]),
        Ok(()),
        "kept at the text before"
    );
}

#[test]
fn teardown_frees_what_a_plugin_made_and_keeps_what_another_instance_owns() {
    let s = store();
    let handed = s.handover_make(who(1, 1), &[p("a", "1")]).unwrap();
    own(&s, who(1, 1), handed, 9);
    let made = s.handover_make(who(1, 2), &[p("b", "2")]).unwrap();
    let claimed = s.handover_make(who(1, 2), &[p("c", "3")]).unwrap();
    let claims = s.claim(who(1, 2), &[claimed]).unwrap();
    let other = s.handover_make(who(2, 1), &[]).unwrap();
    s.owner_stopped(1).unwrap();
    assert_eq!(s.len(), 2, "the owned one and the other plugin's");
    assert_eq!(read_params(&s, 9, handed).unwrap().len(), 1);
    assert_eq!(s.handover_check(who(1, 2), made), Err(ERR_NOT_FOUND));
    assert_eq!(
        s.attach(Some(claimed), 3),
        Err(ERR_STATE),
        "claimed ones go"
    );
    drop(claims);
    s.handover_check(who(2, 1), other).unwrap();
    // The instance closes: the last owner is gone.
    s.instance_closed(9).unwrap();
    assert_eq!(s.len(), 1);
}

#[test]
fn nothing_shows_in_a_debug_print() {
    let s = store();
    let id = s.handover_make(who(1, 1), &[p("k", "hunter2")]).unwrap();
    s.text_write(who(1, 1), id, 0, b"hunter2", true).unwrap();
    s.handover_check(who(1, 1), id).unwrap();
    assert!(!format!("{s:?}").contains("hunter2"));
}

#[test]
fn every_failed_allocation_leaves_the_store_unchanged() {
    use crate::native_host::shell::tests::FAIL_NTH;
    let s = store();
    let id = s.handover_make(who(1, 1), &[p("k", "v")]).unwrap();
    for nth in 1..=4 {
        FAIL_NTH.with(|n| n.set(nth));
        let made = s.handover_make(who(1, 1), &[p("a", "1")]);
        let wrote = s.text_write(who(1, 1), id, 0, b"text", true);
        FAIL_NTH.with(|n| n.set(0));
        for error in [made.err(), wrote.err()].into_iter().flatten() {
            assert_eq!(error, ERR_NO_MEMORY, "allocation {nth}");
        }
        s.end_callback(who(1, 99)).unwrap();
    }
    // Unchanged ledger: only what succeeded is held.
    let (params, text) = s.held();
    assert!(params > 0 && text <= CHUNK + DESCRIPTOR);
    let count = s.len();
    FAIL_NTH.with(|n| n.set(1));
    let refused = s.handover_make(who(1, 1), &[p("zz", "1")]);
    FAIL_NTH.with(|n| n.set(0));
    assert_eq!(refused.err(), Some(ERR_NO_MEMORY));
    assert_eq!((s.held().0, s.len()), (params, count));
}

#[test]
fn empty_hand_overs_are_bounded_too() {
    let s = UiStore::new(Limits::X4);
    let cap = Limits::X4.handovers;
    for _ in 0..cap {
        s.handover_make(who(1, 1), &[]).unwrap();
    }
    assert_eq!(s.handover_make(who(1, 1), &[]).err(), Some(ERR_FULL));
    assert_eq!(s.len(), cap);
    // The callback ends: the made ones are freed and making works again.
    s.end_callback(who(1, 1)).unwrap();
    assert_eq!(s.len(), 0);
    s.handover_make(who(1, 2), &[]).unwrap();
}

/// Cleanup that meets a busy store says `ERR_BUSY`, changes nothing and works
/// when called again: nothing is dropped silently.
#[test]
fn cleanup_on_a_busy_store_is_busy_and_retried_never_dropped() {
    use paperpaw_native::host_glue::ERR_BUSY;
    let s = store();
    let made = s.handover_make(who(1, 1), &[p("a", "1")]).unwrap();
    let claimed = s.handover_make(who(1, 1), &[p("b", "2")]).unwrap();
    let owned = s.handover_make(who(2, 1), &[p("c", "3")]).unwrap();
    own(&s, who(2, 1), owned, 8);
    let claims = s.claim(who(1, 1), &[claimed]).unwrap();
    assert_eq!(s.len(), 3);
    let held = s.held();
    let busy = s.inner.borrow_mut();
    // Plugin stop, callback end, screen close, one release: all busy.
    assert_eq!(s.owner_stopped(1), Err(ERR_BUSY));
    assert_eq!(s.end_callback(who(1, 1)), Err(ERR_BUSY));
    assert_eq!(s.instance_closed(8), Err(ERR_BUSY));
    assert_eq!(s.release(claimed), Err(ERR_BUSY));
    // A submit rollback gives the claims back to try again.
    let Err((status, claims)) = s.rollback(claims) else {
        panic!("a busy store must not drop the claims")
    };
    assert_eq!(status, ERR_BUSY);
    drop(busy);
    assert_eq!((s.len(), s.held()), (3, held), "nothing changed");
    // The retries work.
    s.rollback(claims).unwrap();
    assert_eq!(s.len(), 2);
    s.end_callback(who(1, 1)).unwrap();
    assert_eq!(s.len(), 1);
    assert_eq!(s.instance_closed(8), Ok(()));
    assert_eq!(s.len(), 0);
    let again = s.handover_make(who(3, 1), &[]).unwrap();
    let claim = s.claim(who(3, 1), &[again]).unwrap();
    s.release(again).unwrap();
    drop(claim);
    assert_eq!(s.owner_stopped(3), Ok(()));
    let _ = made;
}

#[test]
fn only_the_small_memory_class_gets_the_x4_profile() {
    assert!(is_x4(MemoryClass::Small));
    assert!(!is_x4(MemoryClass::Large));
}

/// The nested-callback gate is closed on the X4 profile only, and only
/// without the feature `x4-nested` (#250); a large device never has it.
#[test]
fn the_nested_gate_is_the_x4_profiles_until_the_feature_opens_it() {
    assert_eq!(
        nested_closed(MemoryClass::Small),
        !cfg!(feature = "x4-nested")
    );
    assert!(!nested_closed(MemoryClass::Large));
}

/// What `handed_file` shows for `instance`: the maker, the root and the path.
fn handed(s: &UiStore, instance: u32) -> Result<(u64, u32, String), i32> {
    let mut seen = None;
    s.handed_file(instance, &mut |h| {
        seen = Some((h.maker, h.root, String::from(h.path)));
    })?;
    seen.ok_or(ERR_STATE)
}

#[test]
fn the_handed_file_is_the_root_and_path_of_the_instances_hand_over_and_its_maker() {
    let s = store();
    let id = s
        .handover_make(
            who(9, 1),
            &[p("page", "2"), p("root", "2"), p("path", "licences/a.txt")],
        )
        .unwrap();
    own(&s, who(9, 1), id, 4);
    assert_eq!(handed(&s, 4), Ok((9, 2, String::from("licences/a.txt"))));
    // The data folder is root 0.
    let data = s
        .handover_make(who(9, 2), &[p("root", "0"), p("path", "notes.txt")])
        .unwrap();
    own(&s, who(9, 2), data, 5);
    assert_eq!(handed(&s, 5), Ok((9, 0, String::from("notes.txt"))));
    // The maker stopping does not take it from the instance.
    s.owner_stopped(9).unwrap();
    assert_eq!(handed(&s, 5), Ok((9, 0, String::from("notes.txt"))));
}

#[test]
fn no_hand_over_or_one_without_a_usable_root_and_path_is_denied() {
    let s = store();
    // An instance without a hand-over, an unknown instance.
    assert_eq!(handed(&s, 1), Err(ERR_DENIED));
    let cases: [&[Param<'_>]; 5] = [
        &[p("path", "a.txt")],
        &[p("root", "0")],
        &[p("root", "1"), p("path", "a.txt")],
        &[p("root", "5"), p("path", "a.txt")],
        &[p("root", "x"), p("path", "a.txt")],
    ];
    for (n, params) in cases.iter().enumerate() {
        let instance = 10 + n as u32;
        let id = s.handover_make(who(3, 1), params).unwrap();
        own(&s, who(3, 1), id, instance);
        assert_eq!(handed(&s, instance), Err(ERR_DENIED), "case {n}");
    }
}

#[test]
fn a_handed_file_goes_with_its_instance_and_a_busy_store_is_a_state_error() {
    let s = store();
    let id = s
        .handover_make(who(3, 1), &[p("root", "0"), p("path", "a.txt")])
        .unwrap();
    own(&s, who(3, 1), id, 6);
    assert!(handed(&s, 6).is_ok());
    let hold = s.inner.borrow_mut();
    assert_eq!(handed(&s, 6), Err(ERR_STATE));
    drop(hold);
    s.instance_closed(6).unwrap();
    assert_eq!(handed(&s, 6), Err(ERR_DENIED));
}

#[test]
fn the_host_makes_the_hand_over_of_a_dialog_with_the_params_a_plugin_cannot() {
    let s = store();
    // Op 16 refuses a `core.` key to a plugin.
    assert_eq!(
        s.handover_make(who(1, 1), &[p("core.request", "7")]),
        Err(ERR_INVALID)
    );
    let id = s.handover_dialog("7", "0").unwrap();
    // Claimed, so the stack's attach gives it to the dialog's instance.
    s.attach(Some(id), 5).unwrap();
    assert_eq!(
        read_params(&s, 5, id).unwrap(),
        [
            ("core.request".into(), "7".into()),
            ("core.kind".into(), "0".into())
        ]
    );
    // One no screen took is freed by the release.
    let loose = s.handover_dialog("8", "0").unwrap();
    s.release(loose).unwrap();
    assert_eq!(s.attach(Some(loose), 6), Err(ERR_STATE));
}

#[test]
fn a_full_table_of_plugins_hand_overs_leaves_room_for_the_hosts_dialogs() {
    let mut limits = Limits::ELSEWHERE;
    limits.handovers = 1;
    let s = UiStore::new(limits);
    s.handover_make(who(1, 1), &[p("k", "v")]).unwrap();
    // The plugins' table is full: another of theirs is refused.
    assert_eq!(s.handover_make(who(1, 1), &[p("k", "w")]), Err(ERR_FULL));
    // The host's dialogs still open, up to the room it has.
    s.handover_dialog("1", "0").unwrap();
    s.handover_dialog("2", "0").unwrap();
    assert_eq!(s.handover_dialog("3", "0"), Err(ERR_FULL));
}

#[test]
fn a_plugin_that_fills_the_params_pool_leaves_room_for_the_hosts_dialogs() {
    let mut limits = Limits::ELSEWHERE;
    limits.params_bytes = 300;
    let s = UiStore::new(limits);
    // One long value fills the pool as far as it goes.
    let long = "x".repeat(250);
    s.handover_make(who(1, 1), &[p("k", &long)]).unwrap();
    assert_eq!(s.handover_make(who(1, 1), &[p("j", &long)]), Err(ERR_FULL));
    // The host's dialog still gets its hand-over.
    let id = s.handover_dialog("1", "0").unwrap();
    s.attach(Some(id), 3).unwrap();
    assert_eq!(read_params(&s, 3, id).unwrap().len(), 2);
}

#[test]
fn the_hosts_key_is_no_maker_to_pass_a_handed_file_on_for() {
    let s = store();
    let params = [p("root", "5"), p("path", "a.txt")];
    // A file the host made a hand-over for has the host's key as its maker:
    // it would resolve against the passing plugin's own folder. The "no
    // maker" mark is a value of its own, never the host's.
    assert!(is_host_owner(u64::MAX));
    assert_eq!(
        s.handover_passing_on(who(3, 1), &params, u64::MAX),
        Err(ERR_DENIED)
    );
    assert!(s.handover_passing_on(who(3, 1), &params, 9).is_ok());
}

#[test]
fn the_host_hands_over_the_params_of_an_open_no_plugin_made() {
    let s = store();
    let id = s
        .handover_params(&[
            p("id", "core.rotation"),
            // What op 16 refuses a plugin is left out, as is a key again.
            p("core.request", "7"),
            p("Bad Key", "x"),
            p("id", "again"),
            p("text", "A note."),
        ])
        .unwrap();
    s.attach(Some(id), 4).unwrap();
    assert_eq!(
        read_params(&s, 4, id).unwrap(),
        [
            ("id".into(), "core.rotation".into()),
            ("text".into(), "A note.".into())
        ]
    );
    // The text is also the hand-over's text.
    assert_eq!(read_text(&s, 4, id).unwrap(), "A note.");
    // Closing the screen frees all of it.
    s.instance_closed(4).unwrap();
    assert_eq!((s.len(), s.held()), (0, (0, 0)));
}

#[test]
fn a_long_text_is_handed_over_as_the_text_only() {
    let s = store();
    let long = "ä".repeat(2_000);
    let id = s
        .handover_params(&[p("title", "Name"), p("text", &long)])
        .unwrap();
    s.attach(Some(id), 2).unwrap();
    assert_eq!(
        read_params(&s, 2, id).unwrap(),
        [("title".into(), "Name".into())]
    );
    assert_eq!(read_text(&s, 2, id).unwrap(), long);
    s.instance_closed(2).unwrap();
    assert_eq!((s.len(), s.held()), (0, (0, 0)));
    // One no screen took is freed by the release.
    let loose = s.handover_params(&[p("save", "1")]).unwrap();
    s.release(loose).unwrap();
    assert_eq!((s.len(), s.held()), (0, (0, 0)));
}

#[test]
fn params_over_the_caps_of_op_16_are_not_handed_over() {
    let s = store();
    let keys: Vec<String> = (0..=PARAMS_MAX).map(|n| format!("k{n}")).collect();
    let many: Vec<Param<'_>> = keys.iter().map(|key| p(key, "v")).collect();
    assert_eq!(s.handover_params(&many), Err(ERR_NO_SPACE));
    let long = "x".repeat(PARAM_BYTES_MAX);
    assert_eq!(s.handover_params(&[p("id", &long)]), Err(ERR_NO_SPACE));
    assert_eq!((s.len(), s.held()), (0, (0, 0)));
    // A text longer than a hand-over's is left out, the params kept.
    let text = "x".repeat(TEXT_MAX + 1);
    let id = s
        .handover_params(&[p("id", "a"), p("text", &text)])
        .unwrap();
    s.attach(Some(id), 1).unwrap();
    assert_eq!(read_params(&s, 1, id).unwrap(), [("id".into(), "a".into())]);
    assert_eq!(read_text(&s, 1, id).unwrap(), "");
}

#[test]
fn on_the_x4_a_long_text_is_left_out_and_the_params_kept() {
    let s = UiStore::new(Limits::X4);
    // Past the 2 KiB the X4 keeps of texts.
    let text = "x".repeat(4_000);
    let id = s
        .handover_params(&[p("id", "a"), p("text", &text)])
        .unwrap();
    // Its memory is given back whole.
    assert_eq!(s.held().1, 0);
    s.attach(Some(id), 1).unwrap();
    assert_eq!(read_params(&s, 1, id).unwrap(), [("id".into(), "a".into())]);
    assert_eq!(read_text(&s, 1, id).unwrap(), "");
    // One that fits is kept, as its text and as a param.
    let id = s.handover_params(&[p("text", "A note.")]).unwrap();
    s.attach(Some(id), 2).unwrap();
    assert_eq!(read_text(&s, 2, id).unwrap(), "A note.");
    // A hand-over whose attach failed is released: nothing is left of it.
    let loose = s.handover_params(&[p("text", "Another.")]).unwrap();
    let before = s.len();
    s.release(loose).unwrap();
    assert_eq!(s.len(), before - 1);
    s.instance_closed(1).unwrap();
    s.instance_closed(2).unwrap();
    assert_eq!((s.len(), s.held()), (0, (0, 0)));
    // The plugins' table full of such hand-overs: the host's dialogs still
    // have their room.
    let mut ids = Vec::new();
    while let Ok(id) = s.handover_params(&[p("id", "a")]) {
        ids.push(id);
    }
    assert_eq!(ids.len(), Limits::X4.handovers);
    s.handover_dialog("1", "0").unwrap();
    s.handover_dialog("2", "0").unwrap();
}

#[test]
fn hand_overs_of_params_leave_the_room_of_the_hosts_dialogs_alone() {
    let mut limits = Limits::ELSEWHERE;
    limits.handovers = 2;
    let s = UiStore::new(limits);
    // Two screens opened with params take the plugins' table.
    let one = s.handover_params(&[p("id", "core.rotation")]).unwrap();
    let two = s.handover_params(&[p("text", "A note.")]).unwrap();
    s.attach(Some(one), 1).unwrap();
    s.attach(Some(two), 2).unwrap();
    // A third is refused (that screen opens without), not put in the room
    // kept for the host's dialogs.
    assert_eq!(s.handover_params(&[p("id", "x")]), Err(ERR_FULL));
    // The host's dialogs still open, both of them.
    let first = s.handover_dialog("1", "0").unwrap();
    let second = s.handover_dialog("2", "0").unwrap();
    s.attach(Some(first), 3).unwrap();
    s.attach(Some(second), 4).unwrap();
    assert_eq!(read_params(&s, 4, second).unwrap().len(), 2);
    // So also when the params pool is what is full.
    let mut limits = Limits::ELSEWHERE;
    limits.params_bytes = 100;
    let s = UiStore::new(limits);
    let long = "x".repeat(80);
    s.handover_params(&[p("id", &long)]).unwrap();
    assert_eq!(s.handover_params(&[p("id", &long)]), Err(ERR_FULL));
    s.handover_dialog("1", "0").unwrap();
    s.handover_dialog("2", "0").unwrap();
}
