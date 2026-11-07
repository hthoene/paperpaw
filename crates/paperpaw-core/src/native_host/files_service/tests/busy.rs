//! `ERR_BUSY` (`files.md` 2.6 and 3): the matrix against an active library
//! move job, the paths the host holds in use, and the re-entry rule.
use super::*;
use paperpaw_native::host_glue::{ERR_BUSY, ERR_DENIED};

fn status(host: &Rc<TestHost>, op: u32, record: Record<'_>) -> Result<(), i32> {
    call(host, op, record).map(|_| ())
}

/// A card with books, and a move job open for `Books/A` -> `Books/B`.
fn job() -> (Rc<Card>, Rc<TestHost>) {
    let (card, host) = fixture();
    for p in [
        "Books/A/one.epub",
        "Books/A/Sub/two.epub",
        "Books/C/three.epub",
        "Books/loose.epub",
        "Other/x",
    ] {
        card.file(p, 1);
    }
    status(&host, 6, move_record(1, "Books/A", 1, "Books/B", 0)).unwrap();
    assert!(host.moves.active(), "the job is open until its steps ran");
    (card, host)
}

#[test]
fn every_move_is_busy_while_the_job_runs_wherever_it_points() {
    let (card, host) = job();
    card.file(&std::format!("{DATA}/a"), 1);
    for (from, to) in [
        ((1, "Other/x"), (1, "Other/y")),
        ((1, "Books/C/three.epub"), (1, "Books/C/four.epub")),
        ((0, "a"), (0, "b")),
        ((1, "Books/B/one.epub"), (1, "Books/B/uno.epub")),
        ((1, "Books/A"), (1, "Books/Z")),
        ((1, "Books/B"), (1, "Books/Z")),
        ((1, "Books"), (1, "Elsewhere")),
    ] {
        assert_eq!(
            status(&host, 6, move_record(from.0, from.1, to.0, to.1, 0)),
            Err(ERR_BUSY),
            "{from:?} {to:?}"
        );
    }
    host.finish_job();
    assert_eq!(
        status(&host, 6, move_record(1, "Other/x", 1, "Other/y", 0)),
        Ok(())
    );
}

#[test]
fn removals_at_below_or_holding_either_name_are_busy() {
    let (_card, host) = job();
    let remove = |p: &'static str| status(&host, 5, list_record(1, p, 0));
    for busy in [
        "Books/A",
        "Books/A/Sub",
        "Books/A/one.epub",
        "Books/B",
        "Books/B/Sub/two.epub",
        "Books",
    ] {
        assert_eq!(remove(busy), Err(ERR_BUSY), "{busy}");
    }
    // Others go on, also beside the names.
    assert_eq!(remove("Books/C"), Ok(()));
    assert_eq!(remove("Books/loose.epub"), Ok(()));
    assert_eq!(remove("Other"), Ok(()));
}

#[test]
fn make_dir_at_or_below_either_name_is_busy_ancestors_are_not() {
    let (card, host) = job();
    let make = |p: &'static str| status(&host, 4, path(1, p));
    for busy in [
        "Books/A",
        "Books/A/New",
        "Books/A/Sub/Deeper",
        "Books/B",
        "Books/B/New",
    ] {
        assert_eq!(make(busy), Err(ERR_BUSY), "{busy}");
    }
    assert_eq!(make("Books"), Ok(()), "an ancestor exists");
    assert_eq!(make("Books/Other"), Ok(()));
    assert_eq!(make("Books/C/New"), Ok(()));
    assert!(card.is_dir("Books/Other") && !card.has("Books/A/New"));
}

#[test]
fn reads_and_the_rest_go_on_during_the_job() {
    let (_card, host) = job();
    assert_eq!(list_all(&host, 1, "Books/B").unwrap().len(), 2);
    assert_eq!(list_all(&host, 1, "Books").unwrap().len(), 3);
    assert!(call(&host, 2, path(1, "Books/B/one.epub")).is_ok());
    assert!(call(&host, 3, path(1, "")).is_ok());
    // Another plugin-level change: the data folder is not named by the job.
    assert_eq!(status(&host, 4, path(0, "cache")), Ok(()));
    host.finish_job();
    assert_eq!(status(&host, 4, path(1, "Books/A/New")), Ok(()));
    assert_eq!(status(&host, 5, list_record(1, "Books/B", 0)), Ok(()));
}

#[test]
fn paths_the_host_holds_in_use_refuse_changes_not_reads() {
    let (card, host) = fixture();
    card.file("Books/Reading/open.epub", 1);
    card.file(&std::format!("{DATA}/.received/body"), 1);
    host.in_use
        .borrow_mut()
        .push("Books/Reading/open.epub".into());
    host.in_use
        .borrow_mut()
        .push(std::format!("{DATA}/.received/body"));
    // Over it, a folder holding it, and a move onto it.
    assert_eq!(
        status(&host, 5, list_record(1, "Books/Reading/open.epub", 0)),
        Err(ERR_BUSY)
    );
    assert_eq!(
        status(&host, 5, list_record(1, "Books/Reading", 0)),
        Err(ERR_BUSY)
    );
    assert_eq!(
        status(&host, 5, list_record(0, "", 0)),
        Err(ERR_BUSY),
        "the data folder holds a body"
    );
    assert_eq!(
        status(&host, 5, list_record(0, ".received", 0)),
        Err(ERR_BUSY)
    );
    assert_eq!(
        status(
            &host,
            6,
            move_record(1, "Books/Reading/open.epub", 1, "Books/gone.epub", 0)
        ),
        Err(ERR_BUSY)
    );
    assert_eq!(
        status(
            &host,
            6,
            move_record(1, "Books/Reading", 1, "Books/Elsewhere", 0)
        ),
        Err(ERR_BUSY)
    );
    card.file("Books/other.epub", 1);
    assert_eq!(
        status(
            &host,
            6,
            move_record(1, "Books/other.epub", 1, "Books/Reading/open.epub", 1)
        ),
        Err(ERR_BUSY)
    );
    assert_eq!(
        status(&host, 4, path(1, "Books/Reading/open.epub")),
        Err(ERR_BUSY)
    );
    // The body being served is the host's; moving it out waits too.
    assert_eq!(
        status(&host, 6, move_record(0, ".received/body", 0, "mine", 0)),
        Err(ERR_BUSY)
    );
    // Reading is never busy.
    assert!(call(&host, 2, path(1, "Books/Reading/open.epub")).is_ok());
    assert!(list_all(&host, 1, "Books/Reading").is_ok());
    assert!(
        card.has("Books/Reading/open.epub") && card.has("Books/other.epub"),
        "nothing changed"
    );
    // Free again.
    host.in_use.borrow_mut().clear();
    assert_eq!(
        status(&host, 6, move_record(0, ".received/body", 0, "mine", 0)),
        Ok(())
    );
}

#[test]
fn the_busy_rule_comes_after_the_root_and_path_rules() {
    let (_card, host) = job();
    // A refusal that does not depend on the host's state is the same busy or not.
    assert_eq!(
        status(&host, 6, move_record(2, "x", 0, "y", 0)),
        Err(ERR_DENIED)
    );
    assert_eq!(
        status(&host, 6, move_record(1, "../x", 1, "y", 0)),
        Err(paperpaw_native::host_glue::ERR_INVALID)
    );
    assert_eq!(
        status(&host, 6, move_record(3, "x", 1, "y", 0)),
        Err(ERR_UNSUPPORTED)
    );
}
