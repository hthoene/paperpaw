use super::*;

#[test]
fn removal_and_replacement_invalidate_only_older_handles_from_that_provider() {
    let tracker = FontRemovals::new();
    let provider = Id::new_static("pack");
    let before = tracker.generation().unwrap();
    tracker.removed(&provider, 0);
    assert_eq!(tracker.generation(), Some(before));
    assert!(tracker.valid("pack", before));
    tracker.removed(&provider, 2);
    let replacement = tracker.generation().unwrap();
    assert!(!tracker.valid("pack", before));
    assert!(tracker.valid("other", before));
    assert!(tracker.valid("pack", replacement));
    // Re-registering the same id cannot revive the previous handle.
    assert!(!tracker.valid("pack", before));
    tracker.removed(&provider, 1);
    assert!(!tracker.valid("pack", replacement));
    assert!(tracker.valid("other", before));
}

#[test]
fn failed_map_growth_raises_floor_and_new_handles_can_be_acquired() {
    let tracker = FontRemovals::new();
    tracker.refuse_growth.set(true);
    tracker.removed(&Id::new_static("pack"), 1);
    assert_eq!(tracker.generation(), Some(1));
    assert!(!tracker.valid("pack", 0));
    assert!(!tracker.valid("other", 0));
    assert!(tracker.valid("pack", 1));
    assert!(tracker.valid("other", 1));
    tracker.removed(&Id::new_static("other"), 1);
    assert!(tracker.valid("pack", 1));
    assert!(!tracker.valid("other", 1));
    assert!(!tracker.valid("other", 3));
}

#[test]
fn saturation_permanently_refuses_named_handles() {
    let tracker = FontRemovals::new();
    tracker.generation.set(Some(u64::MAX - 1));
    tracker.removed(&Id::new_static("pack"), 1);
    assert_eq!(tracker.generation(), None);
    for acquired in [0, u64::MAX - 1, u64::MAX] {
        assert!(!tracker.valid("pack", acquired));
        assert!(!tracker.valid("other", acquired));
    }
    tracker.removed(&Id::new_static("other"), 1);
    assert_eq!(tracker.generation(), None);
}

#[test]
fn a_busy_map_fails_closed_and_records_a_floor_for_the_removal() {
    let tracker = FontRemovals::new();
    let borrowed = tracker.providers.borrow();
    tracker.removed(&Id::new_static("pack"), 1);
    drop(borrowed);
    assert!(!tracker.valid("other", 0));
    assert!(tracker.valid("other", 1));
}
