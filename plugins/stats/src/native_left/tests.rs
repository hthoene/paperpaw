use super::*;

fn context<'a>(params: &[(&'a str, &'a str)]) -> Context<'a> {
    Context::of(params.iter().map(|(key, value)| Param::new(key, value)))
}

#[test]
fn pages_left_to_the_next_chapter() {
    let params = [
        ("pages", "200"),
        ("progress", "25.0"),
        ("chapters", "0,100,300,700"),
    ];
    let at = context(&params);
    assert_eq!(at.pages_left(false), Some(150.0));
    // From 250 to 300 thousandths: 10 of 200 pages.
    assert_eq!(at.pages_left(true), Some(10.0));
    // In the last chapter: to the end.
    let last = context(&[
        ("pages", "200"),
        ("progress", "80.0"),
        ("chapters", "0,100,300,700"),
    ]);
    assert_eq!(last.pages_left(true), Some(40.0));
    // Pages not counted yet: nothing to say.
    assert_eq!(context(&[("progress", "5")]).pages_left(false), None);
    // Progress not known, or not a number.
    assert_eq!(context(&[("pages", "10")]).pages_left(false), None);
    assert_eq!(
        context(&[("pages", "10"), ("progress", "far")]).pages_left(false),
        None
    );
    // Chapters not known, or with marks that are no numbers: the book's end.
    assert_eq!(
        context(&[("pages", "100"), ("progress", "50")]).pages_left(true),
        Some(50.0)
    );
    assert_eq!(
        context(&[("pages", "100"), ("progress", "50"), ("chapters", "a,,b")]).pages_left(true),
        Some(50.0)
    );
    // Past the end: nothing left, never less.
    assert_eq!(
        context(&[("pages", "100"), ("progress", "120")]).pages_left(false),
        Some(0.0)
    );
}

#[test]
fn the_first_of_a_key_that_comes_twice_is_the_one_read() {
    let at = context(&[("pages", "100"), ("pages", "7"), ("progress", "0")]);
    assert_eq!(at.pages, Some(100.0));
}

#[test]
fn a_param_that_cannot_be_read_is_skipped() {
    let params: [Result<Param<'_>, Error>; 3] = [
        Err(Error::Invalid),
        Param::new("pages", "40"),
        Param::new("progress", "50"),
    ];
    let at = Context::of(params.into_iter());
    assert_eq!(at.pages_left(false), Some(20.0));
}

#[test]
fn the_speed_is_worked_out_per_book_not_per_showing() {
    let mut speed = Left::default();
    assert_eq!(speed.opened("books/a.epub"), Ok(true));
    speed.known = Some(Some(30.0));
    // The same book after a menu: the speed stays.
    assert_eq!(speed.opened("books/a.epub"), Ok(false));
    assert_eq!(speed.opened("books/b.epub"), Ok(true));
    speed.known = Some(Some(30.0));
    let recounts = (0..RECOUNT * 2).filter(|_| speed.turned()).count();
    assert_eq!(recounts, 2);
    // Without a speed yet, more often, but not at every page.
    speed.known = Some(None);
    let recounts = (0..MIN_PAGES * 2).filter(|_| speed.turned()).count();
    assert_eq!(recounts, 2);
}

#[test]
fn a_book_that_was_shown_without_a_speed_is_worked_out_again() {
    // `known` is None until the first sum: the same path counts as new.
    let mut speed = Left::default();
    assert_eq!(speed.opened("books/a.epub"), Ok(true));
    assert_eq!(speed.opened("books/a.epub"), Ok(true));
}

#[test]
fn turning_pages_never_overflows() {
    let mut speed = Left {
        turns: u32::MAX,
        known: Some(Some(1.0)),
        ..Left::default()
    };
    // Wraps to 0, which is a multiple of the interval.
    assert!(speed.turned());
}

#[test]
fn a_number_that_is_not_finite_is_skipped() {
    for bad in ["inf", "-inf", "NaN"] {
        assert_eq!(
            context(&[("pages", bad), ("progress", "10")]).pages_left(false),
            None
        );
        assert_eq!(
            context(&[("pages", "10"), ("progress", bad)]).pages_left(false),
            None
        );
    }
}

#[test]
fn the_status_items_have_a_token_of_their_own() {
    use crate::declarations::{HTTP_TOKEN, STATUS_TOKEN, TICK_TOKEN};
    assert_ne!(STATUS_TOKEN, HTTP_TOKEN);
    assert_ne!(STATUS_TOKEN, TICK_TOKEN);
}
