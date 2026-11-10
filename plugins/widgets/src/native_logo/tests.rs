use super::*;

fn id(n: u32) -> ImageId {
    ImageId::new(n).unwrap()
}

#[test]
fn the_picture_set_is_tried_before_paperpaws_own() {
    assert_eq!(steps(""), 1);
    assert_eq!(steps("logos/mine.png"), 2);
    assert_eq!(
        candidate("", 0),
        Some((PrepareRoot::Bundle, "pictures/logo.png"))
    );
    assert_eq!(
        candidate("logos/mine.png", 0),
        Some((PrepareRoot::Library, "logos/mine.png"))
    );
    assert_eq!(
        candidate("logos/mine.png", 1),
        Some((PrepareRoot::Bundle, "pictures/logo.png"))
    );
    assert_eq!(candidate("logos/mine.png", 2), None);
    assert_eq!(candidate("", 1), None);
}

#[test]
fn a_path_too_long_for_a_request_is_left_out() {
    let long = "a".repeat(PATH_MAX + 1);
    assert_eq!(candidate(&long, 0), None);
    assert_eq!(
        candidate(&long, 1),
        Some((PrepareRoot::Bundle, "pictures/logo.png"))
    );
}

#[test]
fn a_ready_picture_is_held_and_the_one_before_is_given_back() {
    let mut logo = Logo::default();
    assert!(logo.wants());
    logo.asked(0);
    assert!(!logo.wants());
    let next = logo.ready((cookie(0), ImageStatus::Ready, Some(id(7)), (300, 300)), "");
    assert_eq!(
        next,
        Next::Ready {
            image: id(7),
            release: None
        }
    );
    assert_eq!(logo.held(), Some((id(7), (300, 300))));
    assert!(!logo.wants());
    // A request made after the setting changed, answered while one is held.
    logo.asked(0);
    let next = logo.ready((cookie(0), ImageStatus::Ready, Some(id(8)), (100, 50)), "");
    assert_eq!(
        next,
        Next::Ready {
            image: id(8),
            release: Some(id(7))
        }
    );
}

#[test]
fn an_answer_nobody_waits_for_is_ignored() {
    let mut logo = Logo::default();
    let answer = (cookie(0), ImageStatus::Ready, Some(id(1)), (1, 1));
    assert_eq!(logo.ready(answer, ""), Next::Ignore);
    logo.asked(1);
    assert_eq!(logo.ready(answer, "x.png"), Next::Ignore, "another step");
    // The setting changed meanwhile: the answer is not the one waited for.
    assert_eq!(logo.forget(), None);
    let answer = (cookie(1), ImageStatus::Ready, Some(id(1)), (1, 1));
    assert_eq!(logo.ready(answer, "x.png"), Next::Ignore);
}

#[test]
fn a_file_that_cannot_be_read_leads_to_the_next_then_to_the_name_alone() {
    let mut logo = Logo::default();
    logo.asked(0);
    let missing = |step| (cookie(step), ImageStatus::NotFound, None, (0, 0));
    assert_eq!(logo.ready(missing(0), "x.png"), Next::Again(1));
    logo.asked(1);
    let damaged = (cookie(1), ImageStatus::Damaged, None, (0, 0));
    assert_eq!(logo.ready(damaged, "x.png"), Next::Gone);
    assert!(!logo.wants(), "not asked again until the setting changes");
    assert_eq!(logo.forget(), None);
    assert!(logo.wants());
}

#[test]
fn a_refusal_for_want_of_memory_is_asked_again_later() {
    let mut logo = Logo::default();
    logo.asked(0);
    let answer = (cookie(0), ImageStatus::NoMemory, None, (0, 0));
    assert_eq!(logo.ready(answer, ""), Next::Retry);
    assert!(logo.wants());
}

#[test]
fn forgetting_gives_the_held_picture_back() {
    let mut logo = Logo::default();
    logo.asked(0);
    logo.ready((cookie(0), ImageStatus::Ready, Some(id(3)), (9, 9)), "");
    assert_eq!(logo.forget(), Some(id(3)));
    assert_eq!(logo.held(), None);
}

#[test]
fn a_picture_is_shown_as_large_as_fits_with_its_proportions() {
    assert_eq!(fit((300, 300), 60), (60, 60));
    assert_eq!(fit((200, 100), 60), (60, 30));
    assert_eq!(fit((100, 200), 60), (30, 60));
    assert_eq!(fit((1000, 1), 60), (60, 1));
    assert_eq!(fit((0, 5), 60), (0, 0));
    assert_eq!(fit((5, 5), 0), (0, 0));
    assert_eq!(fit((u32::MAX, u32::MAX), u16::MAX).0, u16::MAX);
}
