//! The startup migration in bounded steps, the uninstall removal, and the
//! notice for a save that has no strong nonce (the X4 with its radio off).
use super::*;

/// `count` interface-6 lines of two owners (at most 32 each).
fn old_file(count: usize) -> Vec<u8> {
    let names: Vec<String> = (0..count)
        .map(|n| std::format!("{}.k{n:02}", if n % 2 == 0 { "mail" } else { "wifi" }))
        .collect();
    let values: Vec<String> = (0..count).map(|n| std::format!("value-{n:02}")).collect();
    let lines: Vec<(&str, &str, bool, &str)> = (0..count)
        .map(|n| (names[n].as_str(), "", false, values[n].as_str()))
        .collect();
    interface6(&lines)
}

fn all_readable(v: &View<'_>, count: usize) {
    for n in 0..count {
        let owner = if n % 2 == 0 { "mail" } else { "wifi" };
        let got = v.get(owner, &std::format!("k{n:02}")).unwrap();
        assert_eq!(got.0, std::format!("value-{n:02}"));
    }
}

#[test]
fn migration_seals_a_bounded_batch_per_call_and_ends() {
    with_view(|v| {
        v.host.card.put(file::FILE, &old_file(20));
        let (mut from, mut steps, mut total) = (0, 0, 0);
        loop {
            let before = v.host.seal.seals_left.get();
            v.host.seal.seals_left.set(MIGRATE_BATCH + 1);
            let step = v.services().migrate(from).unwrap();
            let used = MIGRATE_BATCH + 1 - v.host.seal.seals_left.get();
            assert_eq!(used, step.sealed + 1, "one seal per value and one trial");
            assert!(step.sealed <= MIGRATE_BATCH);
            v.host.seal.seals_left.set(before);
            steps += 1;
            total += step.sealed;
            // Readable between the steps: sealed or still veiled.
            all_readable(v, 20);
            match step.next {
                Some(next) => {
                    assert!(next > from, "progress");
                    from = next;
                }
                None => break,
            }
        }
        assert_eq!((steps, total), (3, 20));
        assert_eq!(v.host.card.renames.get(), 3);
        assert_eq!(text(&v.host.card.file()).matches("s1:").count(), 20);
        assert!(v.host.card.get(file::TEMP).is_none());
        // Nothing left: a step writes nothing.
        let (file, renames) = (v.host.card.file(), v.host.card.renames.get());
        assert_eq!(v.services().migrate(0).unwrap().next, None);
        assert_eq!(
            (v.host.card.file(), v.host.card.renames.get()),
            (file, renames)
        );
    });
}

#[test]
fn a_step_that_finds_nothing_writes_nothing_and_a_veiling_device_is_done() {
    with_view(|v| {
        // No file, then only sealed lines.
        assert_eq!(upgrade(v.services()), Ok(false));
        v.keep("mail", "a", "", "x").unwrap();
        let renames = v.host.card.renames.get();
        let appends_before = v.host.card.appends_left.get();
        assert_eq!(upgrade(v.services()), Ok(false));
        assert_eq!(v.host.card.renames.get(), renames);
        assert_eq!(v.host.card.appends_left.get(), appends_before);
    });
    let mut seal = Seal::new(9);
    seal.seals = false;
    with_host(host(seal), true, |v| {
        v.host.card.put(file::FILE, &old_file(4));
        assert_eq!(v.services().migrate(0).unwrap().next, None);
        assert_eq!(v.host.card.file(), old_file(4));
    });
}

#[test]
fn corrupt_lines_are_kept_byte_for_byte_and_passed_over() {
    with_view(|v| {
        let mut file = Vec::new();
        file.extend_from_slice(b"no tabs at all\n");
        // Valid hex that unveils to bytes that are not text: absent, kept.
        file.extend_from_slice(b"mail.bad\t\t\tff\n");
        // A line longer than any line the host takes in one piece.
        file.extend_from_slice(b"wifi.long\t\t\t");
        file.extend(core::iter::repeat_n(b'a', 3_000));
        file.push(b'\n');
        file.extend_from_slice(b"mail.sealed\t\t\ts1:00aa\n");
        file.extend_from_slice(&[0xff, 0xfe, b'\n']);
        file.extend_from_slice(&old_file(3));
        v.host.card.put(file::FILE, &file);
        assert_eq!(upgrade(v.services()), Ok(true));
        let after = v.host.card.file();
        for kept in [
            &b"no tabs at all\n"[..],
            b"mail.bad\t\t\tff\n",
            b"mail.sealed\t\t\ts1:00aa\n",
            &[0xff, 0xfe, b'\n'],
        ] {
            assert!(after.windows(kept.len()).any(|w| w == kept), "{kept:?}");
        }
        assert!(after.windows(3_000).any(|w| w.iter().all(|b| *b == b'a')));
        assert_eq!(text(&after).matches("s1:").count(), 3 + 1);
        all_readable(v, 3);
        assert_eq!(v.get("mail", "bad").err(), Some(ERR_NOT_FOUND));
        // Done, and stable.
        assert_eq!(upgrade(v.services()), Ok(false));
    });
}

#[test]
fn a_refused_seal_leaves_the_old_file_readable_and_the_migration_resumes() {
    with_view(|v| {
        let old = old_file(12);
        v.host.card.put(file::FILE, &old);
        // The third real seal has no strong nonce (one trial came first): the
        // whole step is refused.
        v.host.seal.seals_left.set(3);
        assert_eq!(v.services().migrate(0).err(), Some(ERR_STATE));
        assert_eq!(v.host.card.file(), old);
        assert!(v.host.card.get(file::TEMP).is_none());
        all_readable(v, 12);
        // The migration is not a user's save: no notice.
        assert_eq!(v.host.radio_notices.get(), 0);
        assert_eq!(v.host.notices.get(), 0);
        v.host.seal.seals_left.set(u32::MAX);
        assert_eq!(upgrade(v.services()), Ok(true));
        all_readable(v, 12);
        assert_eq!(text(&v.host.card.file()).matches("s1:").count(), 12);
    });
}

#[test]
fn a_card_that_fails_mid_step_leaves_the_old_file_and_a_stale_temp_is_harmless() {
    with_view(|v| {
        let old = old_file(10);
        v.host.card.put(file::FILE, &old);
        // The write of the new file stops half way.
        v.host.card.appends_left.set(4);
        assert_eq!(v.services().migrate(0).err(), Some(ERR_IO));
        assert_eq!(v.host.card.file(), old);
        assert!(v.host.card.get(file::TEMP).is_none());
        all_readable(v, 10);
        // The rename fails.
        v.host.card.appends_left.set(u32::MAX);
        v.host.card.fail_rename.set(true);
        assert_eq!(v.services().migrate(0).err(), Some(ERR_IO));
        assert_eq!(v.host.card.file(), old);
        assert!(v.host.card.get(file::TEMP).is_none());
        // A power cut left half a new file behind: the next step starts over.
        v.host.card.fail_rename.set(false);
        v.host.card.put(file::TEMP, b"mail.k00\t\t\ts1:0011223344");
        assert_eq!(upgrade(v.services()), Ok(true));
        assert!(v.host.card.get(file::TEMP).is_none());
        all_readable(v, 10);
    });
}

#[test]
fn an_unreadable_file_is_not_overwritten_by_the_migration_and_is_told_once() {
    with_view(|v| {
        let old = old_file(4);
        v.host.card.put(file::FILE, &old);
        v.host.card.fail_read.set(true);
        assert_eq!(v.services().migrate(0).err(), Some(ERR_IO));
        assert_eq!(v.services().migrate(0).err(), Some(ERR_IO));
        assert_eq!(v.host.notices.get(), 1);
        v.host.card.fail_read.set(false);
        assert_eq!(v.host.card.file(), old);
    });
}

#[test]
fn a_cursor_the_file_no_longer_fits_ends_or_leaves_veiled_values_for_a_save() {
    with_view(|v| {
        v.host.card.put(file::FILE, &old_file(6));
        // Past the end: nothing to do, nothing written.
        let step = v.services().migrate(1_000).unwrap();
        assert_eq!((step.sealed, step.next), (0, None));
        assert_eq!(v.host.card.file(), old_file(6));
        // A cursor into the middle seals from there on.
        let step = v.services().migrate(4).unwrap();
        assert_eq!((step.sealed, step.next), (2, None));
        assert_eq!(text(&v.host.card.file()).matches("s1:").count(), 2);
        // Any save seals the rest.
        v.keep("stats", "webdav", "", "d").unwrap();
        assert_eq!(text(&v.host.card.file()).matches("s1:").count(), 7);
        all_readable(v, 6);
    });
}

#[test]
fn a_save_without_a_strong_nonce_is_refused_unchanged_and_told() {
    with_view(|v| {
        v.keep("mail", "a", "", "x").unwrap();
        let before = v.host.card.file();
        v.host.seal.seals_left.set(0);
        assert_eq!(v.keep("mail", "b", "", "y").err(), Some(ERR_STATE));
        assert_eq!(v.host.radio_notices.get(), 1);
        assert_eq!(v.host.card.file(), before);
        assert!(v.host.card.get(file::TEMP).is_none());
        assert_eq!(v.host.notices.get(), 0, "not the unreadable-file notice");
        // Reads and the random op are not saves: no notice.
        v.host.card.put(file::FILE, &old_file(2));
        assert_eq!(v.get("mail", "k00").unwrap().0, "value-00");
        v.host.source.refuse.set(ERR_STATE);
        let random = Record::ServiceSecRandom(wire::ServiceSecRandom { len: 8 });
        assert_eq!(v.call("mail", NONE, 5, random).err(), Some(ERR_STATE));
        assert_eq!(v.host.radio_notices.get(), 1);
        // With the radio on the same save works.
        v.host.seal.seals_left.set(u32::MAX);
        v.keep("mail", "b", "", "y").unwrap();
        assert_eq!(v.host.radio_notices.get(), 1);
    });
}

#[test]
fn removals_need_no_nonce_so_the_radio_off_never_keeps_a_deleted_secret() {
    with_view(|v| {
        // Veiled values of two owners (interface 6), and no strong nonce.
        let old = old_file(8);
        v.host.card.put(file::FILE, &old);
        v.host.seal.seals_left.set(0);
        // `remove` of one name: only that line goes, the other veiled lines stay.
        v.remove("mail", "", "k00").unwrap();
        assert_eq!(v.get("mail", "k00").err(), Some(ERR_NOT_FOUND));
        assert_eq!(v.get("mail", "k02").unwrap().0, "value-02");
        // Uninstall with the data: every row of the plugin goes, also one that
        // does not open here; the other owner's veiled lines stay as they were.
        let mut file = v.host.card.file();
        file.extend_from_slice(b"mail.foreign\t\t\ts1:00\n");
        v.host.card.put(file::FILE, &file);
        v.services().forget_plugin("mail").unwrap();
        let after = text(&v.host.card.file());
        assert!(!after.contains("mail."), "{after}");
        assert_eq!(after.matches("wifi.").count(), 4);
        assert!(!after.contains("s1:"), "nothing was sealed");
        for n in [1, 3, 5, 7] {
            assert_eq!(
                v.get("wifi", &std::format!("k{n:02}")).unwrap().0,
                std::format!("value-{n:02}")
            );
        }
        assert_eq!((v.host.radio_notices.get(), v.host.notices.get()), (0, 0));
        assert!(v.host.card.get(file::TEMP).is_none());
        // A keep still needs the nonce and is told.
        assert_eq!(v.keep("wifi", "n", "", "x").err(), Some(ERR_STATE));
        assert_eq!(v.host.radio_notices.get(), 1);
    });
}

#[test]
fn a_migration_step_without_a_strong_nonce_is_refused_before_anything_is_written() {
    with_view(|v| {
        let old = old_file(5);
        v.host.card.put(file::FILE, &old);
        v.host.seal.seals_left.set(0);
        let writes = v.host.card.writes.get();
        assert_eq!(v.services().migrate(0).err(), Some(ERR_STATE));
        assert_eq!(v.host.card.writes.get(), writes, "no temp file written");
        assert!(v.host.card.get(file::TEMP).is_none());
        assert_eq!(v.host.card.file(), old);
        assert_eq!((v.host.radio_notices.get(), v.host.notices.get()), (0, 0));
    });
}

#[test]
fn forgetting_a_plugin_without_rows_writes_nothing() {
    with_view(|v| {
        v.host.card.put(file::FILE, &old_file(4));
        let writes = v.host.card.writes.get();
        v.services().forget_plugin("stats").unwrap();
        // A foreign line and a corrupt one are not rows of the plugin either.
        let mut file = v.host.card.file();
        file.extend_from_slice(b"junk\nstats\n");
        v.host.card.put(file::FILE, &file);
        v.services().forget_plugin("stats").unwrap();
        assert_eq!(v.host.card.writes.get(), writes);
        assert_eq!(v.host.card.renames.get(), 0);
        // No file at all.
        v.host.card.files.borrow_mut().clear();
        v.services().forget_plugin("mail").unwrap();
        assert!(v.host.card.get(file::TEMP).is_none() && v.host.card.get(file::FILE).is_none());
    });
}
