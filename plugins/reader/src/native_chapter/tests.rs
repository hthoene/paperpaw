use super::*;

const TABLE: [((u32, u32), &str); 3] = [((0, 0), "One"), ((2, 0), " Two "), ((4, 10), "Three")];

#[test]
fn the_chapter_is_the_last_start_at_or_before_the_page() {
    let one = Chapter::scan(TABLE.into_iter(), (1, 50)).unwrap();
    assert_eq!((one.title(), one.next()), ("One", Some((2, 0))));
    // A chapter start is the page of that chapter; the title is trimmed.
    let two = Chapter::scan(TABLE.into_iter(), (2, 0)).unwrap();
    assert_eq!((two.title(), two.next()), ("Two", Some((4, 10))));
    // The last chapter has no next start.
    let three = Chapter::scan(TABLE.into_iter(), (9, 0)).unwrap();
    assert_eq!((three.title(), three.next()), ("Three", None));
}

#[test]
fn the_cached_chapter_covers_its_pages_only() {
    let two = Chapter::scan(TABLE.into_iter(), (3, 5)).unwrap();
    assert!(two.covers((2, 0)) && two.covers((4, 9)));
    assert!(!two.covers((1, 99)) && !two.covers((4, 10)));
}

#[test]
fn a_page_before_the_first_entry_has_no_chapter() {
    let table = [((1, 0), "One"), ((3, 0), "Two")];
    let before = Chapter::scan(table.into_iter(), (0, 5)).unwrap();
    assert_eq!((before.title(), before.next()), ("", Some((1, 0))));
    assert!(before.covers((0, 0)) && !before.covers((1, 0)));
    // No table at all: empty, for the whole book.
    let none = Chapter::scan(core::iter::empty(), (7, 7)).unwrap();
    assert_eq!((none.title(), none.next()), ("", None));
    assert!(none.covers((9, 9)));
}

#[test]
fn an_unordered_table_and_equal_starts_give_the_same_chapter() {
    let table = [((4, 0), "C"), ((0, 0), "A"), ((2, 0), "B1"), ((2, 0), "B2")];
    let found = Chapter::scan(table.into_iter(), (3, 0)).unwrap();
    assert_eq!((found.title(), found.next()), ("B2", Some((4, 0))));
}

#[test]
fn a_long_title_is_cut_at_a_character_boundary() {
    let long: String = core::iter::repeat_n('ä', 200).collect();
    let found = Chapter::scan([((0, 0), long.as_str())].into_iter(), (0, 0)).unwrap();
    assert_eq!(found.title().len(), 256);
    assert!(found.title().chars().all(|c| c == 'ä'));
}

#[test]
fn the_pages_left_count_to_the_page_before_the_next_start() {
    // 10 pages; the next chapter starts with page index 6 (the 7th page).
    assert_eq!(pages_left(3, 10, Some((6, true))), Some(3));
    // The last page of the chapter: none left.
    assert_eq!(pages_left(6, 10, Some((6, true))), Some(0));
    // The next chapter starts in the middle of page index 6: that page is
    // still in this chapter.
    assert_eq!(pages_left(3, 10, Some((6, false))), Some(4));
    // The last chapter ends with the book.
    assert_eq!(pages_left(8, 10, None), Some(2));
    assert_eq!(pages_left(10, 10, None), Some(0));
}

#[test]
fn the_pages_left_never_pass_what_the_host_accepts() {
    // A next start past the book, or a page outside it.
    assert_eq!(pages_left(9, 10, Some((40, false))), Some(1));
    assert_eq!(pages_left(0, 10, None), None);
    assert_eq!(pages_left(11, 10, None), None);
    // A next chapter on the first page leaves nothing before it.
    assert_eq!(pages_left(1, 10, Some((0, true))), None);
}

#[test]
fn a_turn_into_the_next_chapter_changes_title_and_count() {
    let mut cached = Chapter::scan(TABLE.into_iter(), (0, 5)).unwrap();
    // Pages 1-2 are chapter one (next start on page index 2).
    assert_eq!(pages_left(1, 6, cached.next().map(|_| (2, true))), Some(1));
    let at = (2, 0);
    if !cached.covers(at) {
        cached = Chapter::scan(TABLE.into_iter(), at).unwrap();
    }
    assert_eq!(cached.title(), "Two");
    assert_eq!(pages_left(3, 6, cached.next().map(|_| (4, true))), Some(1));
}

#[test]
fn every_allocation_may_fail() {
    crate::fail_alloc::sweep(|| Chapter::scan(TABLE.into_iter(), (3, 0)).map(|c| c.title().len()));
}
