use super::*;
use alloc::{string::ToString, vec};

/// A folder held in memory.
struct Listed(Vec<(&'static str, bool)>);

impl Folder for Listed {
    fn each(&mut self, visit: &mut dyn FnMut(&str, bool)) -> Result<(), Error> {
        for (name, is_dir) in &self.0 {
            visit(name, *is_dir);
        }
        Ok(())
    }
}

fn id(n: u32) -> ImageId {
    ImageId::new(n).unwrap()
}

#[test]
fn pictures_are_found_in_their_folder() {
    let mut folder = Listed(vec![
        ("b.png", false),
        ("a.JPG", false),
        ("notes.txt", false),
        (".hidden.jpg", false),
        ("sub.jpg", true),
        ("c.GIF", false),
        ("d.jpeg", false),
    ]);
    let names: Vec<String> = (0..5)
        .map(|n| picture_at(&mut folder, |_| n).unwrap().unwrap())
        .collect();
    // Four pictures, in the order listed, then round again.
    assert_eq!(names, ["b.png", "a.JPG", "c.GIF", "d.jpeg", "b.png"]);
    assert!(names.iter().all(|n| is_picture(n, false)));
    assert_eq!(picture_at(&mut Listed(vec![]), |_| 0), Ok(None));
    assert_eq!(
        picture_at(&mut Listed(vec![("x.txt", false)]), |_| 0),
        Ok(None)
    );
}

#[test]
fn what_is_a_picture() {
    assert!(is_picture("a.jpg", false) && is_picture("A.JPEG", false));
    assert!(is_picture("a.png", false) && is_picture("a.gif", false));
    assert!(!is_picture("a.jpg", true), "a folder");
    assert!(!is_picture(".a.jpg", false), "hidden");
    assert!(!is_picture("a.bmp", false) && !is_picture("jpg", false));
    assert!(!is_picture("é", false), "no cut in a character");
}

#[test]
fn captions_are_names() {
    assert_eq!(
        caption_of("Holiday_by_the_sea.jpg").unwrap(),
        "Holiday by the sea"
    );
    assert_eq!(caption_of("Anna 2024.png").unwrap(), "Anna 2024");
    assert_eq!(caption_of("plain").unwrap(), "plain");
    assert_eq!(caption_of("_x_.png").unwrap(), "x");
}

#[test]
fn a_picture_is_fit_in_its_room() {
    assert_eq!(fit_in((400, 200), (100, 100)), (100, 50));
    assert_eq!(fit_in((200, 400), (100, 100)), (50, 100));
    assert_eq!(fit_in((100, 100), (300, 50)), (50, 50));
    assert_eq!(fit_in((0, 5), (10, 10)), (0, 0));
    assert_eq!(fit_in((5, 5), (0, 10)), (0, 0));
    assert_eq!(fit_in((u32::MAX, 1), (u16::MAX, u16::MAX)).0, u16::MAX);
}

#[test]
fn a_widget_asks_gets_its_picture_and_shows_it() {
    let mut pictures = Pictures::default();
    pictures.made(7).unwrap();
    assert!(matches!(pictures.state(7), State::Loading));
    assert!(pictures.wants(7, 1));
    let cookie = pictures.asking(7, 1, "Sea".to_string()).unwrap();
    assert!(is_cookie(cookie));
    assert!(!pictures.wants(7, 1), "asked for this mark already");
    assert!(pictures.wants(7, 2), "another day, another picture");
    let next = pictures.ready((cookie, ImageStatus::Ready, Some(id(3)), (40, 30)));
    assert_eq!(
        next,
        Next::Ready {
            instance: 7,
            release: None
        }
    );
    let State::Shown(held) = pictures.state(7) else {
        panic!("shown")
    };
    assert_eq!(
        (held.image, held.size, held.caption.as_str()),
        (id(3), (40, 30), "Sea")
    );
    // The next picture: the one shown stays until it is ready, then goes back.
    let cookie = pictures.asking(7, 2, "Hill".to_string()).unwrap();
    assert!(matches!(pictures.state(7), State::Shown(_)));
    let next = pictures.ready((cookie, ImageStatus::Ready, Some(id(4)), (1, 1)));
    assert_eq!(
        next,
        Next::Ready {
            instance: 7,
            release: Some(id(3))
        }
    );
    assert_eq!(pictures.dropped(7), Some(id(4)));
    assert!(matches!(pictures.state(7), State::Loading));
}

#[test]
fn only_a_missing_or_forbidden_folder_means_there_is_no_picture() {
    for error in [
        Error::NotFound,
        Error::Denied,
        Error::Unsupported,
        Error::Invalid,
    ] {
        assert_eq!(refusal(error), Refusal::None, "{error:?}");
    }
    // No room for another image, a busy card or service: again at the minute.
    for error in [
        Error::Full,
        Error::Busy,
        Error::Io,
        Error::State,
        Error::NoSpace,
        Error::NoMemory,
        Error::Timeout,
        Error::Other(5),
    ] {
        assert_eq!(refusal(error), Refusal::Later, "{error:?}");
    }
}

#[test]
fn widgets_that_ask_alike_have_cookies_of_their_own() {
    let mut pictures = Pictures::default();
    // Numbers 256 apart: told apart all the same.
    pictures.made(1).unwrap();
    pictures.made(257).unwrap();
    let a = pictures.asking(1, 1, String::new()).unwrap();
    let b = pictures.asking(257, 1, String::new()).unwrap();
    assert_ne!(a, b);
    let answer = |cookie| (cookie, ImageStatus::Ready, Some(id(5)), (2, 2));
    assert_eq!(
        pictures.ready(answer(b)),
        Next::Ready {
            instance: 257,
            release: None
        }
    );
    assert_eq!(
        pictures.ready(answer(a)),
        Next::Ready {
            instance: 1,
            release: None
        }
    );
}

#[test]
fn an_answer_to_an_older_request_is_not_the_last_ones() {
    let mut pictures = Pictures::default();
    pictures.made(1).unwrap();
    let old = pictures.asking(1, 1, String::new()).unwrap();
    let new = pictures.asking(1, 2, String::new()).unwrap();
    assert_ne!(old, new);
    let answer = |cookie| (cookie, ImageStatus::Ready, Some(id(9)), (2, 2));
    assert_eq!(pictures.ready(answer(old)), Next::Ignore);
    assert!(matches!(pictures.ready(answer(new)), Next::Ready { .. }));
    // Another widget's request is told apart.
    pictures.made(2).unwrap();
    let other = pictures.asking(2, 1, String::new()).unwrap();
    assert_ne!(other, new);
    assert_eq!(pictures.ready(answer(0x1234_5678)), Next::Ignore);
}

#[test]
fn a_file_that_cannot_be_shown_leaves_the_folders_words() {
    let mut pictures = Pictures::default();
    pictures.made(1).unwrap();
    let cookie = pictures.asking(1, 1, String::new()).unwrap();
    let damaged = (cookie, ImageStatus::Damaged, None, (0, 0));
    assert_eq!(
        pictures.ready(damaged),
        Next::Gone {
            instance: 1,
            release: None
        }
    );
    assert!(matches!(pictures.state(1), State::Empty));
    assert!(!pictures.wants(1, 1), "not asked again for the same day");
}

#[test]
fn a_refusal_for_want_of_memory_is_chosen_again_at_the_next_minute() {
    let mut pictures = Pictures::default();
    pictures.made(1).unwrap();
    let cookie = pictures.asking(1, 5, String::new()).unwrap();
    let busy = (cookie, ImageStatus::NoMemory, None, (0, 0));
    assert_eq!(pictures.ready(busy), Next::Retry);
    assert!(pictures.wants(1, 5));
    // The host refused the request itself.
    pictures.asking(1, 5, String::new());
    pictures.refused(1);
    assert!(pictures.wants(1, 5));
}

#[test]
fn a_folder_without_pictures_says_so_and_gives_the_picture_back() {
    let mut pictures = Pictures::default();
    pictures.made(1).unwrap();
    let cookie = pictures.asking(1, 1, String::new()).unwrap();
    pictures.ready((cookie, ImageStatus::Ready, Some(id(2)), (3, 3)));
    assert_eq!(pictures.none(1, 2), Some(id(2)));
    assert!(matches!(pictures.state(1), State::Empty));
    assert!(!pictures.wants(1, 2));
    assert_eq!(pictures.none(99, 2), None, "a widget that is gone");
}
