//! No allocation, so none to fail: every op runs with the first allocation
//! after the call armed to fail, and answers as if it were not. (The fake card
//! keeps fixed tables and never allocates while it answers; the dispatcher's
//! scratch and the input are made before the call.)
use super::*;
use crate::native_host::shell::tests::FAIL_NTH;
use paperpaw_native::host_glue::ERR_NOT_FOUND;

fn armed(host: &Rc<TestHost>, id: u32, record: Record<'_>) -> Result<usize, i32> {
    let op = schema_op(id);
    let bytes = input(id, record);
    let mut out = std::vec![0; op.max_reply];
    with(host, NORMAL, |frame| {
        FAIL_NTH.with(|n| n.set(1));
        let result = frame.run(op, &bytes, &mut out);
        let attempted = FAIL_NTH.with(|n| n.replace(0)) != 1;
        assert!(!attempted, "op {id} allocated");
        result
    })
}

#[test]
fn no_op_allocates() {
    let (card, host) = fixture();
    card.file("Books/a.epub", 1);
    card.file("Books/Sub/b.epub", 1);
    card.file("T/x", 1);
    card.file("T/y/z", 1);
    card.dir("Other");
    for i in 0..40 {
        card.file(&std::format!("Many/f{i}"), 1);
    }
    assert!(armed(&host, 1, list_record(1, "Many", 0)).is_ok());
    assert!(armed(&host, 1, list_record(1, "", 0)).is_ok());
    assert!(armed(&host, 2, path(1, "Books/a.epub")).is_ok());
    assert!(armed(&host, 2, path(0, "")).is_ok());
    assert!(armed(&host, 3, path(1, "")).is_ok());
    assert_eq!(armed(&host, 4, path(1, "New/Deep/Folder")), Ok(0));
    assert_eq!(armed(&host, 4, path(0, "cache")), Ok(0));
    card.file(&std::format!("{DATA}/x"), 1);
    assert_eq!(armed(&host, 6, move_record(0, "x", 0, "y", 0)), Ok(0));
    assert_eq!(
        armed(&host, 6, move_record(0, "none", 0, "y", 0)),
        Err(ERR_NOT_FOUND)
    );
}

/// `remove_tree` walks the tree in the storage, which holds the path it walks
/// on the heap: that allocation is fallible, a refusal before the first
/// removal is `ERR_IO` and changes nothing, and the next call removes the tree.
#[test]
fn a_tree_removal_that_cannot_keep_its_path_changes_nothing() {
    let (card, host) = fixture();
    card.file("Books/a.epub", 1);
    card.file("T/x", 1);
    card.file("T/y/z", 1);
    let refused = armed_allowing(&host, 5, list_record(1, "T", 0));
    assert_eq!(refused, Err(paperpaw_native::host_glue::ERR_IO));
    assert!(card.has("T/x") && card.has("T/y/z") && card.has("Books/a.epub"));
    assert!(call(&host, 5, list_record(1, "T", 0)).is_ok());
    assert!(card.has("Books") && !card.has("T/x"));
}

/// A refusal at any allocation of the walk, also after entries were removed,
/// is `ERR_IO`: what was removed stays removed, the rest and everything
/// outside the tree is whole, and calling again removes what is left.
#[test]
fn a_refusal_at_any_allocation_of_the_walk_leaves_a_tree_to_go_on_with() {
    for nth in 1..64 {
        let (card, host) = fixture();
        card.file("Books/a.epub", 1);
        card.file("T/x", 1);
        card.file("T/y/z", 1);
        card.file("T/y/w/v", 1);
        card.file("T/q", 1);
        let op = schema_op(5);
        let bytes = input(5, list_record(1, "T", 0));
        let mut out = std::vec![0; op.max_reply];
        let (result, left) = with(&host, NORMAL, |frame| {
            FAIL_NTH.with(|n| n.set(nth));
            let result = frame.run(op, &bytes, &mut out);
            (result, FAIL_NTH.with(|n| n.replace(0)))
        });
        if left > 0 {
            // The call allocated fewer times than `nth`: all were tried.
            assert!(result.is_ok());
            assert!(nth > 2, "{nth}: allocations tried");
            return;
        }
        assert_eq!(result, Err(paperpaw_native::host_glue::ERR_IO), "{nth}");
        assert!(card.has("Books/a.epub"), "{nth}: outside the tree");
        assert!(call(&host, 5, list_record(1, "T", 0)).is_ok(), "{nth}");
        assert!(card.has("Books/a.epub") && !card.has("T/x"), "{nth}");
        assert!(!card.has("T/y/w/v") && !card.has("T/q"), "{nth}");
    }
    panic!("the walk allocates more than 63 times");
}

/// A move of the user's files starts the library's job, which keeps its two
/// names: that allocation is fallible, and a refusal changes nothing.
#[test]
fn a_library_move_that_cannot_keep_its_names_changes_nothing() {
    let (card, host) = fixture();
    card.file("Books/Sub/b.epub", 1);
    card.dir("Other");
    let refused = armed_allowing(&host, 6, move_record(1, "Books/Sub", 1, "Other/Sub", 0));
    assert_eq!(refused, Err(paperpaw_native::host_glue::ERR_NO_MEMORY));
    assert!(card.has("Books/Sub/b.epub") && !card.has("Other/Sub"));
    assert!(host.journal.borrow().is_none() && !host.moves.active());
    assert_eq!(
        call(&host, 6, move_record(1, "Books/Sub", 1, "Other/Sub", 0)),
        Ok(Vec::new())
    );
}

fn armed_allowing(host: &Rc<TestHost>, id: u32, record: Record<'_>) -> Result<usize, i32> {
    let op = schema_op(id);
    let bytes = input(id, record);
    let mut out = std::vec![0; op.max_reply];
    with(host, NORMAL, |frame| {
        FAIL_NTH.with(|n| n.set(1));
        let result = frame.run(op, &bytes, &mut out);
        FAIL_NTH.with(|n| n.set(0));
        result
    })
}
