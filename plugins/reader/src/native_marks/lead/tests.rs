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
    // Not enough of it to tell: the last seven bytes only.
    assert!(!found_in(stored, "and so on. Alice w"));
    assert!(!found_in(stored, "and so on. Alice"));
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
