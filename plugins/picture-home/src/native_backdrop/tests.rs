//! The picture behind the page: what is asked, what an answer leads to, where
//! it is drawn.
use super::*;
use crate::fail_alloc::sweep;
use crate::native_picture::{Asked, Recipe};

fn asked<'a>(path: &'a str, fit: &'a str, zoom: i64, lighten: i64) -> Asked<'a> {
    Asked {
        path,
        bytes: 0,
        modified: 0,
        fit,
        anchor: "picture-home.anchor.center",
        zoom,
        lighten,
        brightness: "",
        contrast: "",
        render: "",
    }
}

fn recipe(path: &str, fit: &str, zoom: i64, lighten: i64) -> Option<Recipe> {
    Recipe::new(&asked(path, fit, zoom, lighten), 480, 800).unwrap()
}

fn id(n: u32) -> ImageId {
    ImageId::new(n).unwrap()
}

fn ready(back: &mut Backdrop, cookie: u32, size: (u32, u32)) -> Landed {
    back.ready(cookie, ImageStatus::Ready, Some(id(7)), size)
}

const FILL: &str = "picture-home.fit.fill";
const WHOLE: &str = "picture-home.fit.fit";
const TILE: &str = "picture-home.fit.tile";
const STRETCH: &str = "picture-home.fit.stretch";

#[test]
fn a_chosen_picture_is_asked_for_once_and_made_to_show() {
    let mut back = Backdrop::default();
    let mut cookie = 0;
    let change = back.want(recipe("p/a.jpg", FILL, 100, 0), &mut cookie).unwrap();
    let (number, side) = change.ask.unwrap();
    assert_eq!(change.release, None);
    assert_eq!(back.path(), "p/a.jpg");
    assert!(back.making() && back.waits_for(number) && !back.held());
    assert!(side > 0 && side <= SIDE_MAX);
    // Wanted again while it is made: nothing new is asked.
    assert_eq!(
        back.want(recipe("p/a.jpg", FILL, 100, 0), &mut cookie).unwrap(),
        Change::default()
    );
    assert_eq!(ready(&mut back, number, (480, 800)), Landed::Ready);
    assert!(back.held() && !back.making() && !back.waits_for(number));
    // And held: nothing new either.
    assert_eq!(
        back.want(recipe("p/a.jpg", FILL, 100, 0), &mut cookie).unwrap(),
        Change::default()
    );
}

#[test]
fn another_picture_or_setting_gives_the_held_one_back_and_asks_again() {
    let mut back = Backdrop::default();
    let mut cookie = 0;
    let (first, _) = back
        .want(recipe("a.png", FILL, 100, 0), &mut cookie)
        .unwrap()
        .ask
        .unwrap();
    ready(&mut back, first, (480, 800));
    let change = back.want(recipe("a.png", FILL, 150, 0), &mut cookie).unwrap();
    assert_eq!(change.release, Some(id(7)));
    let (second, _) = change.ask.unwrap();
    assert_ne!(first, second);
    // The answer of the old request is nobody's: it is given back.
    assert_eq!(
        back.ready(first, ImageStatus::Ready, Some(id(9)), (1, 1)),
        Landed::Ignore(Some(id(9)))
    );
    assert!(back.waits_for(second));
    // No picture chosen: what is held goes, nothing is asked.
    ready(&mut back, second, (480, 800));
    let change = back.want(None, &mut cookie).unwrap();
    assert_eq!(change, Change { release: Some(id(7)), ask: None });
    assert!(!back.held() && !back.making());
    assert_eq!(back.path(), "");
}

#[test]
fn a_failure_leaves_the_page_plain_and_is_not_asked_again() {
    let cases = [
        (ImageStatus::Unsupported, Reason::Unreadable),
        (ImageStatus::Damaged, Reason::Unreadable),
        (ImageStatus::TooLarge, Reason::Unreadable),
        (ImageStatus::NotFound, Reason::Unreadable),
        (ImageStatus::CardFailure, Reason::Short),
        (ImageStatus::NoMemory, Reason::Short),
    ];
    for (status, reason) in cases {
        let mut back = Backdrop::default();
        let mut cookie = 0;
        let (number, _) = back
            .want(recipe("a.png", FILL, 100, 0), &mut cookie)
            .unwrap()
            .ask
            .unwrap();
        assert_eq!(back.ready(number, status, None, (0, 0)), Landed::Failed(reason));
        assert!(!back.making() && !back.held());
        assert_eq!(back.drawing().unwrap(), None);
        assert_eq!(
            back.want(recipe("a.png", FILL, 100, 0), &mut cookie).unwrap(),
            Change::default()
        );
    }
    assert_eq!(Reason::Unreadable.message(), "picture-home.unreadable");
    assert_eq!(Reason::Short.message(), "picture-home.short");
}

#[test]
fn a_ready_answer_without_a_picture_is_a_failure_that_gives_back_what_it_holds() {
    let mut back = Backdrop::default();
    let mut cookie = 0;
    let (number, _) = back
        .want(recipe("a.png", FILL, 100, 0), &mut cookie)
        .unwrap()
        .ask
        .unwrap();
    // A picture of no size cannot be drawn: given back, the page is plain.
    assert_eq!(
        back.ready(number, ImageStatus::Ready, Some(id(3)), (0, 0)),
        Landed::Ignore(Some(id(3)))
    );
    assert_eq!(back.drawing().unwrap(), None);
    // Ready without an id is a failure.
    let mut back = Backdrop::default();
    let (number, _) = back
        .want(recipe("a.png", FILL, 100, 0), &mut cookie)
        .unwrap()
        .ask
        .unwrap();
    assert_eq!(
        back.ready(number, ImageStatus::Ready, None, (4, 4)),
        Landed::Failed(Reason::Unreadable)
    );
}

#[test]
fn a_request_the_host_refuses_leaves_the_page_plain() {
    let mut back = Backdrop::default();
    let mut cookie = 0;
    back.want(recipe("a.png", FILL, 100, 0), &mut cookie).unwrap();
    back.refused();
    assert!(!back.making() && !back.held());
    assert_eq!(back.drawing().unwrap(), None);
    // The settings are as they were: not asked again and again.
    assert_eq!(
        back.want(recipe("a.png", FILL, 100, 0), &mut cookie).unwrap(),
        Change::default()
    );
}

#[test]
fn the_requests_have_their_own_numbers() {
    let mut back = Backdrop::default();
    let mut cookie = 0xffff;
    let (one, _) = back
        .want(recipe("a.png", FILL, 100, 0), &mut cookie)
        .unwrap()
        .ask
        .unwrap();
    let (two, _) = back
        .want(recipe("b.png", FILL, 100, 0), &mut cookie)
        .unwrap()
        .ask
        .unwrap();
    assert_eq!(one & 0xffff_0000, COOKIE);
    assert_eq!(two & 0xffff_0000, COOKIE);
    assert_ne!(one, two);
}

fn held(fit: &str, zoom: i64, lighten: i64, size: (u32, u32)) -> Drawing {
    let mut back = Backdrop::default();
    let mut cookie = 0;
    let (number, _) = back
        .want(recipe("a.png", fit, zoom, lighten), &mut cookie)
        .unwrap()
        .ask
        .unwrap();
    ready(&mut back, number, size);
    back.drawing().unwrap().unwrap()
}

#[test]
fn a_picture_that_fills_is_drawn_as_the_plan_places_it() {
    // A wide picture on a tall screen: as high as the screen, cut at the sides.
    let drawing = held(FILL, 100, 0, (600, 400));
    assert_eq!(drawing.image, id(7));
    assert_eq!(drawing.screen, (480, 800));
    assert_eq!(
        drawing.places,
        [Place { x: -360, y: 0, width: 1200, height: 800 }]
    );
    // Covering the screen and opaque: no paper below it.
    assert!(!drawing.paper && !drawing.fill);
    assert_eq!(drawing.transparency, 0);
    // The zoom enlarges it about the place.
    let zoomed = held(FILL, 200, 0, (600, 400));
    assert_eq!(
        zoomed.places,
        [Place { x: -960, y: -400, width: 2400, height: 1600 }]
    );
}

#[test]
fn a_whole_picture_sits_on_paper_and_a_paler_one_is_blended_with_it() {
    let whole = held(WHOLE, 100, 0, (600, 400));
    assert_eq!(whole.places, [Place { x: 0, y: 240, width: 480, height: 320 }]);
    assert!(whole.paper);
    let pale = held(FILL, 100, 40, (600, 400));
    assert!(pale.paper);
    assert_eq!(pale.transparency, 102);
    assert_eq!(transparency(0), 0);
    assert_eq!(transparency(100), 255);
    assert_eq!(transparency(60), 153);
    assert_eq!(transparency(u32::MAX), 255);
}

#[test]
fn a_stretched_picture_fills_the_screen() {
    let stretch = held(STRETCH, 100, 0, (600, 400));
    assert_eq!(stretch.places, [Place { x: 0, y: 0, width: 480, height: 800 }]);
    assert!(stretch.fill && !stretch.paper);
}

#[test]
fn a_tile_repeats_to_cover_the_screen_or_is_not_drawn() {
    // At most half the screen: 240 by 160 here, from the middle's place.
    let drawing = held(TILE, 100, 0, (600, 400));
    assert!(drawing.paper);
    assert!(drawing.places.len() as i64 <= TILES_MAX);
    let width = drawing.places[0].width as i64;
    let height = drawing.places[0].height as i64;
    assert_eq!((width, height), (240, 160));
    // Every pixel of the screen is under a tile.
    for (x, y) in [(0, 0), (479, 0), (0, 799), (479, 799), (240, 400)] {
        assert!(
            drawing.places.iter().any(|p| {
                (i64::from(p.x)..i64::from(p.x) + width).contains(&x)
                    && (i64::from(p.y)..i64::from(p.y) + height).contains(&y)
            }),
            "({x}, {y})"
        );
    }
    // Tiles on the grid of the first: no gaps, no overlaps.
    assert!(drawing
        .places
        .iter()
        .all(|p| (i64::from(p.x) - i64::from(drawing.places[0].x)) % width == 0));
    // A tiny picture would repeat a thousand times: not drawn.
    assert_eq!(held_tiny(), None);
}

fn held_tiny() -> Option<Drawing> {
    let mut back = Backdrop::default();
    let mut cookie = 0;
    let (number, _) = back
        .want(recipe("a.png", TILE, 100, 0), &mut cookie)
        .unwrap()
        .ask
        .unwrap();
    ready(&mut back, number, (16, 16));
    back.drawing().unwrap()
}

#[test]
fn the_longest_side_asked_follows_the_fit_and_stays_within_the_hosts_limit() {
    let side_of = |fit: &str, zoom| side(&recipe("a.png", fit, zoom, 0).unwrap());
    assert_eq!(side_of(FILL, 100), 1200);
    assert_eq!(side_of(FILL, 300), SIDE_MAX);
    assert_eq!(side_of(WHOLE, 100), 800);
    assert_eq!(side_of(WHOLE, 300), SIDE_MAX);
    assert_eq!(side_of(STRETCH, 300), 800);
    assert_eq!(side_of("picture-home.fit.original", 100), SIDE_MAX);
    assert_eq!(side_of(TILE, 100), 400);
}

#[test]
fn nothing_is_drawn_before_the_picture_is_made() {
    let mut back = Backdrop::default();
    assert_eq!(back.drawing().unwrap(), None);
    let mut cookie = 0;
    back.want(recipe("a.png", FILL, 100, 0), &mut cookie).unwrap();
    assert_eq!(back.drawing().unwrap(), None);
}

#[test]
fn every_allocation_may_be_refused() {
    let mut cookie = 0;
    sweep(|| {
        let mut back = Backdrop::default();
        let wish = Recipe::new(&asked("p/a.jpg", TILE, 100, 20), 480, 800)?;
        let (number, _) = back.want(wish, &mut cookie)?.ask.ok_or(Error::Invalid)?;
        ready(&mut back, number, (600, 400));
        back.drawing()?;
        Ok(())
    });
}
