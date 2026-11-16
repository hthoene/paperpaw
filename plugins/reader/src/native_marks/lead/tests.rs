use super::*;

#[test]
fn a_lead_is_the_start_of_the_text_with_its_white_space_as_one_space() {
    let lead = Lead::of("  Alice was   beginning\n to get very tired of sitting").unwrap();
    assert_eq!(lead.text(), "Alice was beginning to get very");
    assert!(lead.len as usize <= LEAD_MAX);
}

#[test]
fn a_text_too_short_to_tell_a_place_has_no_lead() {
    assert!(Lead::of("").is_none());
    assert!(Lead::of("   ").is_none());
    assert!(Lead::of("Chapter").is_none());
    assert!(Lead::of("Chapter I").is_some());
}

#[test]
fn a_lead_is_cut_at_a_character_not_in_the_middle_of_one() {
    let text = "é".repeat(40);
    let lead = Lead::of(&text).unwrap();
    assert_eq!(lead.text().len(), 32);
    assert!(lead.text().chars().all(|c| c == 'é'));
}

#[test]
fn a_page_with_the_text_has_the_mark_wherever_the_text_is_on_it() {
    let stored = "Alice was beginning to get very tired of sitting by her sister";
    // The page starts earlier than the mark did: the text is in the middle.
    let page = "Down the rabbit hole. Alice was  beginning to get\nvery tired of sitting by her";
    assert!(found_in(stored, page));
    // Another page.
    assert!(!found_in(
        stored,
        "The Queen of Hearts she made some tarts."
    ));
    // Nearly the text: one word differs inside the compared start.
    assert!(!found_in(
        stored,
        "Alice was beginning to get quite tired of"
    ));
}

#[test]
fn a_page_that_ends_in_the_text_has_the_mark_by_what_it_has() {
    let stored = "Alice was beginning to get very tired of sitting by her sister";
    assert!(found_in(stored, "and then Alice was beginning to get ve"));
    // From a word's start four bytes tell (the place is on the page too).
    assert!(found_in(stored, "and so on. Alic"));
    // Not enough of it to tell: three bytes, or a word's middle, or no word.
    assert!(!found_in(stored, "and so on. Ali"));
    assert!(!found_in(stored, "and so on. Malice w"));
    assert!(!found_in(stored, "and so on. Alice w, no"));
}

#[test]
fn nothing_is_found_without_a_lead_or_on_an_empty_page() {
    assert!(!found_in("", "Alice was beginning"));
    assert!(!found_in("short", "short"));
    assert!(!found_in("Alice was beginning", ""));
}

#[test]
fn a_word_broken_at_the_end_of_a_line_is_whole_on_both_sides() {
    // Beta 3 broke it there, this build somewhere else, or not at all.
    assert!(found_in(
        "the use of anyone any- where",
        "the use of anyone anywhere"
    ));
    assert!(found_in(
        "the use of anyone anywhere",
        "the use of anyone any- where"
    ));
    assert!(found_in(
        "aloud. I must be get- ting",
        "aloud. I must be getting somewhere"
    ));
    // A dash between words stays one.
    assert!(!found_in("rabbit - hole of the", "rabbithole of the"));
    // Not a break: a hyphen inside a word keeps it as it is.
    assert!(found_in(
        "the rabbit-hole went on",
        "all the rabbit-hole went on and on"
    ));
}

#[test]
fn a_soft_hyphen_or_a_hyphen_sign_does_not_tell_one_conversion_from_another() {
    let lead = Lead::of("an extra\u{ad}ordinary thing").unwrap();
    assert!(lead.is_in("what an extraordinary thing it was"));
    let plain = Lead::of("an extraordinary thing").unwrap();
    assert!(plain.is_in("what an extra\u{ad}ordinary thing it was"));
    let lead = Lead::of("a well\u{2010}known place").unwrap();
    assert!(lead.is_in("so a well-known place"));
}

#[test]
fn a_mark_that_starts_a_word_a_few_bytes_before_the_page_end_is_found() {
    let lead = Lead::of("rabbit hole she fell").unwrap();
    // Four bytes of it are on the page, from a word's start.
    assert!(lead.is_in("down the rabb"));
    // Not fewer than four bytes, not from the middle of a word.
    assert!(!lead.is_in("down the rab"));
    assert!(!lead.is_in("down the burrabb"));
}
