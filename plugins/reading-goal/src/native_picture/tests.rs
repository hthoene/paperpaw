use super::*;
use alloc::vec::Vec;

fn id(n: u32) -> ImageId {
    ImageId::new(n).unwrap()
}

#[test]
fn one_s_own_pictures_come_before_the_cat() {
    let all: Vec<_> = (0..steps(true))
        .map(|step| candidate("pics/cats", Mood::Happy, step).unwrap())
        .collect();
    assert_eq!(all.len(), 7);
    assert_eq!(all[0].path, "pics/cats/happy.png");
    assert_eq!(all[0].source, Source::Card);
    assert_eq!(all[2].path, "pics/cats/happy.jpeg");
    assert_eq!(all[5].path, "pics/cats/happy.JPEG");
    assert_eq!(all[6].path, "pictures/happy.png");
    assert_eq!(all[6].source, Source::Bundle);
    assert_eq!(candidate("pics/cats", Mood::Happy, 7), None);
}

#[test]
fn without_a_folder_only_the_cat_is_asked_for() {
    assert_eq!(steps(false), 1);
    let cat = candidate("", Mood::Sad, 0).unwrap();
    assert_eq!(
        (cat.source, cat.path.as_str()),
        (Source::Bundle, "pictures/sad.png")
    );
    assert_eq!(candidate("", Mood::Sad, 1), None);
}

#[test]
fn a_path_too_long_for_a_request_is_left_out() {
    let folder = "a".repeat(PATH_MAX);
    assert_eq!(candidate(&folder, Mood::Meh, 0), None);
    // The cat after it is still there.
    assert!(candidate(&folder, Mood::Meh, 6).is_some());
}

#[test]
fn an_answer_of_the_open_request_holds_the_picture_and_releases_the_old_one() {
    let mut pictures = Pictures::default();
    assert!(pictures.wants(Mood::Sad));
    pictures.asked(Mood::Sad, 0);
    assert!(!pictures.wants(Mood::Sad), "asked for already");
    let next = pictures.ready(
        (cookie(Mood::Sad, 0), ImageStatus::Ready, Some(id(1))),
        false,
    );
    assert_eq!(
        next,
        Next::Ready {
            image: id(1),
            release: None
        }
    );
    assert_eq!(pictures.of(Mood::Sad), Some(id(1)));
    assert_eq!(pictures.of(Mood::Happy), None);
    // The mood changes: another picture, the first given back.
    assert!(pictures.wants(Mood::Happy));
    pictures.asked(Mood::Happy, 0);
    let next = pictures.ready(
        (cookie(Mood::Happy, 0), ImageStatus::Ready, Some(id(2))),
        false,
    );
    assert_eq!(
        next,
        Next::Ready {
            image: id(2),
            release: Some(id(1))
        }
    );
    assert_eq!(pictures.of(Mood::Sad), None);
}

#[test]
fn an_answer_that_is_not_the_open_one_is_ignored() {
    let mut pictures = Pictures::default();
    let stray = (cookie(Mood::Sad, 0), ImageStatus::Ready, Some(id(1)));
    assert_eq!(pictures.ready(stray, false), Next::Ignore);
    pictures.asked(Mood::Meh, 2);
    assert_eq!(pictures.ready(stray, false), Next::Ignore);
    assert!(!pictures.wants(Mood::Meh), "still open");
}

#[test]
fn a_file_that_cannot_be_used_leads_to_the_next_and_at_last_to_words() {
    let mut pictures = Pictures::default();
    pictures.asked(Mood::Content, 0);
    for step in 0..steps(true) - 1 {
        let next = pictures.ready(
            (cookie(Mood::Content, step), ImageStatus::Damaged, None),
            true,
        );
        assert_eq!(
            next,
            Next::Again {
                mood: Mood::Content,
                step: step + 1
            }
        );
        pictures.asked(Mood::Content, step + 1);
    }
    let last = steps(true) - 1;
    let next = pictures.ready(
        (cookie(Mood::Content, last), ImageStatus::Unsupported, None),
        true,
    );
    assert_eq!(next, Next::Words);
    assert!(!pictures.wants(Mood::Content), "not asked again and again");
    assert!(pictures.wants(Mood::Happy));
    // Another folder: asked anew.
    assert_eq!(pictures.forget(), None);
    assert!(pictures.wants(Mood::Content));
}

#[test]
fn a_refusal_for_memory_is_tried_again_later_not_given_up() {
    let mut pictures = Pictures::default();
    for status in [ImageStatus::NoMemory, ImageStatus::CardFailure] {
        pictures.asked(Mood::Sad, 0);
        assert_eq!(
            pictures.ready((cookie(Mood::Sad, 0), status, None), false),
            Next::Retry
        );
        assert!(pictures.wants(Mood::Sad));
    }
}

#[test]
fn a_changed_folder_gives_the_held_picture_back() {
    let mut pictures = Pictures::default();
    pictures.asked(Mood::Sad, 0);
    pictures.ready(
        (cookie(Mood::Sad, 0), ImageStatus::Ready, Some(id(7))),
        false,
    );
    assert_eq!(pictures.forget(), Some(id(7)));
    assert_eq!(pictures.of(Mood::Sad), None);
}

#[test]
fn the_cookie_tells_mood_and_step_apart() {
    let mut seen = Vec::new();
    for mood in Mood::ALL {
        for step in 0..steps(true) {
            seen.push(cookie(mood, step));
        }
    }
    let count = seen.len();
    seen.sort_unstable();
    seen.dedup();
    assert_eq!(seen.len(), count);
}

// The cats installed with the plugin are read from the plugin's own install
// folder, which the SDK cannot name yet.
#[test]
#[ignore = "needs PrepareRoot::Bundle (image_prepare root 2: the plugin's install folder), Services todo 308 and Host todo 309"]
fn the_installed_cats_are_prepared_from_the_install_folder() {
    unreachable!("a mood without a picture of one's own asks root 2 for pictures/<mood>.png");
}

#[test]
fn a_changed_folder_drops_the_request_open_and_its_answer_is_ignored() {
    let mut pictures = Pictures::default();
    pictures.asked(Mood::Sad, 1);
    assert_eq!(pictures.forget(), None);
    assert!(pictures.wants(Mood::Sad), "asked anew");
    let late = (cookie(Mood::Sad, 1), ImageStatus::Ready, Some(id(3)));
    assert_eq!(pictures.ready(late, true), Next::Ignore);
    assert_eq!(pictures.of(Mood::Sad), None);
}
