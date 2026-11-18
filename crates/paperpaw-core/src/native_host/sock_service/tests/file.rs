//! Reads to a file: the card rules, the paths in use, the staged file and
//! the rename, what ends a read and what it leaves behind.
use super::*;

fn read_file(f: &Fake, owner: u64, id: u32, len: u64, root: u32, path: &str) -> Result<(), i32> {
    let hosts = HostServices::new();
    table::read(
        f,
        &caller(owner, &hosts),
        &wire::ServiceSockRead {
            socket: id,
            mode: 1,
            flags: 0,
            max: 0,
            len,
            root,
            path,
        },
    )
}

fn key(root: u32, path: &str) -> (u32, String) {
    (root, path.to_string())
}

fn clean(f: &Fake) {
    assert!(f.claims.borrow().is_empty(), "no claim left");
    assert!(f.parts.borrow().is_empty(), "no staged file left");
}

#[test]
fn a_whole_file_is_written_renamed_and_told_once() {
    let f = Fake::x4();
    let id = open(&f, OWNER, 4);
    f.files
        .borrow_mut()
        .insert(key(0, "book.epub"), b"old".to_vec());
    read_file(&f, OWNER, id, 10, 0, "book.epub").unwrap();
    assert_eq!(&*f.reads.borrow(), &[(id, 10)]);
    assert_eq!(
        f.claims.borrow().len(),
        1,
        "the path is in use while it runs"
    );
    assert_eq!(received(&f, id, b"0123").unwrap(), 4);
    assert_eq!(f.parts.borrow()[&key(0, "book.epub")], b"0123");
    assert_eq!(
        f.files.borrow()[&key(0, "book.epub")],
        b"old",
        "untouched until whole"
    );
    assert_eq!(next(&f, OWNER), None);
    assert_eq!(
        received(&f, id, b"456789xx").unwrap(),
        6,
        "never more than len"
    );
    assert_eq!(f.files.borrow()[&key(0, "book.epub")], b"0123456789");
    clean(&f);
    assert_eq!(
        drain(&f, OWNER),
        std::vec![Ev::new(id, 4, kind::SAVED, 0, 0)]
    );
    // The socket lives on: bytes can be read again.
    read_bytes(&f, OWNER, id, 5, false).unwrap();
    assert_eq!(received(&f, id, b"ok").unwrap(), 2);
    assert_eq!(f.used(), 8_192);
}

#[test]
fn a_long_file_is_taken_in_read_parts() {
    let f = Fake::x4();
    let id = open(&f, OWNER, 4);
    read_file(&f, OWNER, id, 10_000, 1, "Books/a.cbz").unwrap();
    assert_eq!(f.reads.borrow().as_slice(), &[(id, 4_096)]);
    assert_eq!(received(&f, id, &[1; 4_096]).unwrap(), 4_096);
    assert_eq!(received(&f, id, &[2; 4_096]).unwrap(), 4_096);
    assert_eq!(
        f.reads.borrow().as_slice(),
        &[(id, 4_096), (id, 4_096), (id, 1_808)]
    );
    // In small pieces too: the allowance shrinks, nothing is lost.
    assert_eq!(received(&f, id, &[3; 1_000]).unwrap(), 1_000);
    assert_eq!(received(&f, id, &[3; 808]).unwrap(), 808);
    let file = f.files.borrow()[&key(1, "Books/a.cbz")].clone();
    assert_eq!(file.len(), 10_000);
    assert_eq!(file[4_095..4_097], [1, 2]);
    assert_eq!(drain(&f, OWNER).len(), 1);
    clean(&f);
}

#[test]
fn every_refusal_changes_nothing() {
    let f = Fake::x4();
    let id = open(&f, OWNER, 4);
    let other = open(&f, OTHER, 5);
    let ask = |len, root, path| read_file(&f, OWNER, id, len, root, path);
    // Bad fields.
    assert_eq!(ask(0, 0, "a"), Err(ERR_INVALID));
    assert_eq!(ask((1 << 40) + 1, 0, "a"), Err(ERR_INVALID));
    assert_eq!(ask(5, 2, "a"), Err(ERR_INVALID), "root 2 is read-only");
    assert_eq!(ask(5, 3, "a"), Err(ERR_INVALID));
    assert_eq!(ask(5, 0, ""), Err(ERR_INVALID));
    for name in ["a.pp-staged", "a.PP-STAGED", "dir/b.Pp-Staged"] {
        assert_eq!(ask(5, 0, name), Err(ERR_INVALID), "{name}: the host's own");
    }
    let hosts = HostServices::new();
    for (flags, max) in [(1, 0), (0, 1)] {
        let r = wire::ServiceSockRead {
            socket: id,
            mode: 1,
            flags,
            max,
            len: 5,
            root: 0,
            path: "a",
        };
        assert_eq!(
            table::read(&f, &caller(OWNER, &hosts), &r),
            Err(ERR_INVALID)
        );
    }
    // The card rules say no.
    f.deny.set(Some(paperpaw_native::host_glue::ERR_DENIED));
    assert_eq!(
        ask(5, 1, "plugins/x"),
        Err(paperpaw_native::host_glue::ERR_DENIED)
    );
    f.deny.set(Some(ERR_INVALID));
    assert_eq!(ask(5, 0, "../x"), Err(ERR_INVALID));
    f.deny.set(None);
    // Not enough room after the reserve.
    f.free.set(99);
    assert_eq!(ask(100, 0, "a"), Err(paperpaw_native::host_glue::ERR_FULL));
    f.free.set(u64::MAX);
    // A path another writer holds.
    f.in_use.borrow_mut().push(key(0, "busy.txt"));
    assert_eq!(ask(5, 0, "busy.txt"), Err(ERR_BUSY));
    // Another plugin's socket, an unknown one, a socket not open yet.
    assert_eq!(read_file(&f, OTHER, id, 5, 0, "a"), Err(ERR_NOT_FOUND));
    assert_eq!(read_file(&f, OWNER, 999, 5, 0, "a"), Err(ERR_NOT_FOUND));
    let _ = other;
    clean(&f);
    assert!(f.reads.borrow().is_empty(), "the transport was never asked");
    // Nothing above spoiled the socket: one read at a time now applies.
    ask(5, 0, "a").unwrap();
    assert_eq!(ask(5, 0, "b"), Err(ERR_BUSY));
    assert_eq!(read_bytes(&f, OWNER, id, 5, false), Err(ERR_BUSY));
    assert_eq!(f.claims.borrow().len(), 1);
}

#[test]
fn a_file_read_needs_an_open_socket_and_a_free_read() {
    let f = Fake::x4();
    let id = connect(&f, OWNER, 1).unwrap();
    assert_eq!(read_file(&f, OWNER, id, 5, 0, "a"), Err(ERR_STATE));
    connected(&f, id).unwrap();
    read_bytes(&f, OWNER, id, 5, false).unwrap();
    assert_eq!(read_file(&f, OWNER, id, 5, 0, "a"), Err(ERR_BUSY));
    clean(&f);
}

#[test]
fn a_peer_that_stops_early_leaves_the_old_file_and_no_part() {
    let f = Fake::x4();
    let id = open(&f, OWNER, 4);
    f.files
        .borrow_mut()
        .insert(key(0, "a.txt"), b"old".to_vec());
    read_file(&f, OWNER, id, 10, 0, "a.txt").unwrap();
    received(&f, id, b"0123").unwrap();
    peer_closed(&f, id).unwrap();
    assert_eq!(
        drain(&f, OWNER),
        std::vec![Ev::new(id, 4, kind::FAILED, reason::CLOSED_EARLY, 0)]
    );
    assert_eq!(f.files.borrow()[&key(0, "a.txt")], b"old");
    clean(&f);
    assert_eq!(f.used(), 0);
    // The peer had closed before the read: the same end, at once.
    let id = open(&f, OWNER, 5);
    peer_closed(&f, id).unwrap();
    read_file(&f, OWNER, id, 10, 0, "b.txt").unwrap();
    assert_eq!(
        drain(&f, OWNER),
        std::vec![Ev::new(id, 5, kind::FAILED, reason::CLOSED_EARLY, 0)]
    );
    clean(&f);
}

#[test]
fn a_card_that_fills_or_fails_ends_the_socket_and_cleans_up() {
    for (setup, why) in [
        (0, reason::FULL),
        (1, reason::FULL),
        (2, reason::IO),
        (3, reason::IO),
    ] {
        let f = Fake::x4();
        let id = open(&f, OWNER, 4);
        f.files
            .borrow_mut()
            .insert(key(0, "a.txt"), b"old".to_vec());
        read_file(&f, OWNER, id, 10, 0, "a.txt").unwrap();
        received(&f, id, b"01").unwrap();
        match setup {
            0 => f.free.set(1),
            1 => f
                .write_fails
                .set(Some(paperpaw_native::host_glue::ERR_NO_SPACE)),
            2 => f.write_fails.set(Some(paperpaw_native::host_glue::ERR_IO)),
            _ => {
                received(&f, id, b"234567").unwrap();
                f.commit_fails.set(Some(paperpaw_native::host_glue::ERR_IO));
            }
        }
        // The report that meets the failure is consumed, the socket ends.
        assert!(received(&f, id, b"89").is_ok(), "setup {setup}");
        assert_eq!(
            drain(&f, OWNER),
            std::vec![Ev::new(id, 4, kind::FAILED, why, 0)],
            "setup {setup}"
        );
        assert_eq!(f.files.borrow()[&key(0, "a.txt")], b"old", "setup {setup}");
        clean(&f);
        assert_eq!(f.used(), 0, "setup {setup}");
        assert_eq!(read_file(&f, OWNER, id, 5, 0, "a"), Err(ERR_NOT_FOUND));
    }
}

#[test]
fn whatever_ends_a_read_removes_the_part_and_gives_the_claim_back() {
    for how in 0..7 {
        let f = Fake::x4();
        let id = open(&f, OWNER, 4);
        read_file(&f, OWNER, id, 100, 0, "a.txt").unwrap();
        received(&f, id, b"0123").unwrap();
        assert_eq!(f.parts.borrow().len(), 1);
        match how {
            0 => close(&f, OWNER, id).unwrap(),
            1 => {
                f.advance(60_000);
                tick(&f).unwrap();
            }
            2 => owner_stopped(&f, OWNER).unwrap(),
            3 => screen_closed(&f, OWNER, 5).unwrap(),
            4 => network_lost(&f).unwrap(),
            5 => failed(&f, id, reason::LOST).unwrap(),
            _ => {
                f.run.set(Some(9));
                work_ended(&f, OWNER, 9).unwrap();
                // Not this socket's run: it goes on.
                assert_eq!(f.claims.borrow().len(), 1);
                close(&f, OWNER, id).unwrap();
            }
        }
        clean(&f);
        assert!(
            !f.files.borrow().contains_key(&key(0, "a.txt")),
            "how {how}"
        );
        owner_stopped(&f, OWNER).unwrap();
        assert_eq!(f.used(), 0, "how {how}");
    }
}

#[test]
fn a_stalled_file_read_times_out_by_idle_and_bytes_keep_it_alive() {
    let f = Fake::x4();
    let id = open(&f, OWNER, 4);
    read_file(&f, OWNER, id, 100, 0, "a.txt").unwrap();
    f.advance(59_000);
    received(&f, id, b"0").unwrap();
    f.advance(59_000);
    tick(&f).unwrap();
    assert_eq!(next(&f, OWNER), None, "bytes moved");
    f.advance(1_000);
    tick(&f).unwrap();
    assert_eq!(
        drain(&f, OWNER),
        std::vec![Ev::new(id, 4, kind::FAILED, reason::TIMEOUT, 0)]
    );
    clean(&f);
}

#[test]
fn an_allocation_that_fails_in_a_file_read_claims_nothing() {
    let mut refused = 0;
    for nth in 1..4 {
        let f = Fake::x4();
        let id = open(&f, OWNER, 4);
        fail_nth(nth);
        let asked = read_file(&f, OWNER, id, 10, 0, "a.txt");
        fail_nth(0);
        if asked.is_err() {
            refused += 1;
            assert_eq!(asked, Err(ERR_NO_MEMORY), "gate {nth}");
            clean(&f);
            assert!(f.reads.borrow().is_empty());
            // The socket is as it was.
            read_file(&f, OWNER, id, 10, 0, "a.txt").unwrap();
        }
        owner_stopped(&f, OWNER).unwrap();
        clean(&f);
        assert_eq!(f.used(), 0);
    }
    assert_eq!(refused, 1, "the path's copy is the one allocation");
}

#[test]
fn a_target_already_claimed_is_busy_for_everyone_the_same_plugin_too() {
    let f = Fake::new(SockProfile::OTHER, 0);
    let a = open(&f, OWNER, 1);
    let b = open(&f, OWNER, 2);
    let c = open(&f, OTHER, 3);
    read_file(&f, OWNER, a, 5, 0, "book.epub").unwrap();
    // The same plugin's second socket, the other plugin's, any case.
    assert_eq!(read_file(&f, OWNER, b, 5, 0, "book.epub"), Err(ERR_BUSY));
    assert_eq!(read_file(&f, OWNER, b, 5, 0, "BOOK.EPUB"), Err(ERR_BUSY));
    assert_eq!(read_file(&f, OTHER, c, 5, 0, "book.epub"), Err(ERR_BUSY));
    assert_eq!(f.claims.borrow().len(), 1, "the refusals claimed nothing");
    assert!(
        read_file(&f, 9, open(&f, 9, 4), 5, 0, "book.part").is_ok(),
        "`.part` is an ordinary name again"
    );
    // Another root's file of the same name is another file.
    assert!(read_file(&f, 10, open(&f, 10, 5), 5, 1, "book.epub").is_ok());
    // Once the first read is gone the target is free again.
    close(&f, OWNER, a).unwrap();
    assert!(read_file(&f, OTHER, c, 5, 0, "book.epub").is_ok());
}
