//! The picture behind the page: what is asked, what an answer leads to, where
//! it is drawn.
use super::*;
use crate::fail_alloc::sweep;
use crate::native_picture::{Asked, Recipe};
use alloc::vec::Vec;

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
    back.ready(cookie, ImageStatus::Ready, Some(id(7)), size, None)
}

const FILL: &str = "picture-home.fit.fill";
const WHOLE: &str = "picture-home.fit.fit";
const TILE: &str = "picture-home.fit.tile";
const STRETCH: &str = "picture-home.fit.stretch";

#[test]
fn a_chosen_picture_is_asked_for_once_and_made_to_show() {
    let mut back = Backdrop::default();
    let mut cookie = 0;
    let change = back
        .want(recipe("p/a.jpg", FILL, 100, 0), &mut cookie)
        .unwrap();
    let (number, side) = change.ask.unwrap();
    assert_eq!(change.release, None);
    assert_eq!(back.path(), "p/a.jpg");
    assert!(back.making() && back.waits_for(number) && !back.held());
    assert!(side > 0 && side <= SIDE_MAX);
    // Wanted again while it is made: nothing new is asked.
    assert_eq!(
        back.want(recipe("p/a.jpg", FILL, 100, 0), &mut cookie)
            .unwrap(),
        Change::default()
    );
    assert_eq!(ready(&mut back, number, (480, 800)), Landed::Ready);
    assert!(back.held() && !back.making() && !back.waits_for(number));
    // And held: nothing new either.
    assert_eq!(
        back.want(recipe("p/a.jpg", FILL, 100, 0), &mut cookie)
            .unwrap(),
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
    let change = back
        .want(recipe("a.png", FILL, 150, 0), &mut cookie)
        .unwrap();
    assert_eq!(change.release, Some(id(7)));
    let (second, _) = change.ask.unwrap();
    assert_ne!(first, second);
    // The answer of the old request is nobody's: it is given back.
    assert_eq!(
        back.ready(first, ImageStatus::Ready, Some(id(9)), (1, 1), None),
        Landed::Ignore(Some(id(9)))
    );
    assert!(back.waits_for(second));
    // No picture chosen: what is held goes, nothing is asked.
    ready(&mut back, second, (480, 800));
    let change = back.want(None, &mut cookie).unwrap();
    assert_eq!(
        change,
        Change {
            release: Some(id(7)),
            ask: None
        }
    );
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
        assert_eq!(
            back.ready(number, status, None, (0, 0), None),
            Landed::Failed(reason)
        );
        assert!(!back.making() && !back.held());
        assert_eq!(back.drawing(), None);
        assert_eq!(
            back.want(recipe("a.png", FILL, 100, 0), &mut cookie)
                .unwrap(),
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
        back.ready(number, ImageStatus::Ready, Some(id(3)), (0, 0), None),
        Landed::Ignore(Some(id(3)))
    );
    assert_eq!(back.drawing(), None);
    // Ready without an id is a failure.
    let mut back = Backdrop::default();
    let (number, _) = back
        .want(recipe("a.png", FILL, 100, 0), &mut cookie)
        .unwrap()
        .ask
        .unwrap();
    assert_eq!(
        back.ready(number, ImageStatus::Ready, None, (4, 4), None),
        Landed::Failed(Reason::Unreadable)
    );
}

#[test]
fn a_request_the_host_refuses_leaves_the_page_plain() {
    let mut back = Backdrop::default();
    let mut cookie = 0;
    back.want(recipe("a.png", FILL, 100, 0), &mut cookie)
        .unwrap();
    back.refused();
    assert!(!back.making() && !back.held());
    assert_eq!(back.drawing(), None);
    // The settings are as they were: not asked again and again.
    assert_eq!(
        back.want(recipe("a.png", FILL, 100, 0), &mut cookie)
            .unwrap(),
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
    back.drawing().unwrap()
}

/// Every place of a drawing, over the whole screen.
fn places(drawing: &Drawing) -> Vec<Place> {
    drawing.tiles.within((i32::MIN, i32::MAX)).collect()
}

#[test]
fn a_picture_that_fills_is_drawn_as_the_plan_places_it() {
    // A wide picture on a tall screen: as high as the screen, cut at the sides.
    let drawing = held(FILL, 100, 0, (600, 400));
    assert_eq!(drawing.image, id(7));
    assert_eq!(drawing.screen, (480, 800));
    assert_eq!(
        places(&drawing),
        [Place {
            x: -360,
            y: 0,
            width: 1200,
            height: 800
        }]
    );
    // Covering the screen and opaque: no paper below it.
    assert!(!drawing.paper && !drawing.stretch);
    assert_eq!(drawing.transparency, 0);
    // The zoom enlarges it about the place.
    let zoomed = held(FILL, 200, 0, (600, 400));
    assert_eq!(
        places(&zoomed),
        [Place {
            x: -960,
            y: -400,
            width: 2400,
            height: 1600
        }]
    );
}

#[test]
fn a_whole_picture_sits_on_paper_and_a_paler_one_is_blended_with_it() {
    let whole = held(WHOLE, 100, 0, (600, 400));
    assert_eq!(
        places(&whole),
        [Place {
            x: 0,
            y: 240,
            width: 480,
            height: 320
        }]
    );
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
    assert_eq!(
        places(&stretch),
        [Place {
            x: 0,
            y: 0,
            width: 480,
            height: 800
        }]
    );
    assert!(stretch.stretch && !stretch.paper);
}

#[test]
fn a_tile_repeats_to_cover_the_screen_or_is_not_drawn() {
    // At most half the screen: 240 by 160 here, from the middle's place.
    let drawing = held(TILE, 100, 0, (600, 400));
    assert!(drawing.paper);
    let all = places(&drawing);
    assert!(all.len() as i64 <= TILES_MAX);
    assert_eq!(all.len() as u64, drawing.tiles.count());
    let width = all[0].width as i64;
    let height = all[0].height as i64;
    assert_eq!((width, height), (240, 160));
    // Every pixel of the screen is under a tile.
    for (x, y) in [(0, 0), (479, 0), (0, 799), (479, 799), (240, 400)] {
        assert!(
            all.iter().any(|p| {
                (i64::from(p.x)..i64::from(p.x) + width).contains(&x)
                    && (i64::from(p.y)..i64::from(p.y) + height).contains(&y)
            }),
            "({x}, {y})"
        );
    }
    // Tiles on the grid of the first: no gaps, no overlaps.
    assert!(
        all.iter()
            .all(|p| (i64::from(p.x) - i64::from(all[0].x)) % width == 0)
    );
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
    back.drawing()
}

#[test]
fn a_small_pattern_repeats_over_the_screen_as_on_interface_6() {
    // 64 by 64: 9 by 14 tiles, past the 64 there were while every draw asked
    // for each tile; drawn now.
    let mut back = Backdrop::default();
    let mut cookie = 0;
    let (number, _) = back
        .want(recipe("a.png", TILE, 100, 0), &mut cookie)
        .unwrap()
        .ask
        .unwrap();
    ready(&mut back, number, (64, 64));
    let drawing = back.drawing().unwrap();
    let all = places(&drawing);
    assert!(
        all.len() > 64 && all.len() as i64 <= TILES_MAX,
        "{}",
        all.len()
    );
    for (x, y) in [(0, 0), (479, 0), (0, 799), (479, 799), (240, 400)] {
        assert!(
            all.iter()
                .any(|p| (p.x..p.x + 64).contains(&x) && (p.y..p.y + 64).contains(&y)),
            "({x}, {y})"
        );
    }
    // A tile of the same size by the side of each: no gaps, no overlaps.
    assert!(
        all.iter()
            .all(|p| (p.x - all[0].x) % 64 == 0 && (p.y - all[0].y) % 64 == 0)
    );
}

#[test]
fn a_draw_asks_only_for_the_tiles_in_the_rows_it_may_change() {
    let drawing = held(TILE, 100, 0, (600, 400));
    let all = places(&drawing);
    // The X4's bands of 192 rows: each tile is asked for in every band it
    // meets, and only there.
    let mut seen = Vec::new();
    for top in (0..800).step_by(192) {
        let band = (top, (top + 192).min(800));
        for place in drawing.tiles.within(band) {
            assert!(place.y < band.1 && place.y + place.height as i32 > band.0);
            seen.push(place);
        }
        let meeting = all
            .iter()
            .filter(|p| p.y < band.1 && p.y + p.height as i32 > band.0)
            .count();
        assert_eq!(drawing.tiles.within(band).count(), meeting);
    }
    // Every tile on the screen is asked for (one in the row above its top edge,
    // where the place starts a tile below it, is not).
    let on_screen = |p: &&Place| p.y < 800 && p.y + p.height as i32 > 0;
    assert!(all.iter().filter(on_screen).all(|p| seen.contains(p)));
    // Rows that no tile meets: nothing.
    assert_eq!(drawing.tiles.within((900, 1000)).count(), 0);
    // One picture: asked for only where it is.
    let one = held(WHOLE, 100, 0, (600, 400));
    assert_eq!(one.tiles.within((0, 192)).count(), 0);
    assert_eq!(one.tiles.within((192, 384)).count(), 1);
}

#[test]
fn a_picture_that_would_repeat_too_often_or_too_many_in_a_row_is_not_drawn() {
    let tiled = |size: (u32, u32)| {
        let mut back = Backdrop::default();
        let mut cookie = 0;
        let (number, _) = back
            .want(recipe("a.png", TILE, 100, 0), &mut cookie)
            .unwrap()
            .ask
            .unwrap();
        ready(&mut back, number, size);
        back.drawing()
    };
    // 30 by 30: 17 across, 27 down, 459 in all.
    assert_eq!(tiled((30, 30)), None);
    // 14 by 400: 105 in all, but 35 in a row.
    assert_eq!(tiled((14, 400)), None);
    // 16 by 400: 31 in a row, 93 in all.
    let drawing = tiled((16, 400)).unwrap();
    assert_eq!((drawing.tiles.across, drawing.tiles.down), (31, 3));
    assert!(i64::from(drawing.tiles.across) <= TILES_ACROSS_MAX);
}

#[test]
fn a_file_replaced_by_another_of_the_same_name_is_asked_for_anew() {
    let mut back = Backdrop::default();
    let mut cookie = 0;
    let with = |bytes, modified| {
        Recipe::new(
            &Asked {
                bytes,
                modified,
                ..asked("p/a.jpg", FILL, 100, 0)
            },
            480,
            800,
        )
        .unwrap()
    };
    let (number, _) = back.want(with(1000, 50), &mut cookie).unwrap().ask.unwrap();
    assert_eq!(ready(&mut back, number, (480, 800)), Landed::Ready);
    // The same file: nothing new.
    assert_eq!(
        back.want(with(1000, 50), &mut cookie).unwrap(),
        Change::default()
    );
    // Another size, or the same size changed later: the held picture goes back
    // and the new file is asked for.
    for (bytes, modified) in [(1200, 50), (1200, 90)] {
        let change = back.want(with(bytes, modified), &mut cookie).unwrap();
        assert!(change.ask.is_some(), "{bytes} {modified}");
        let (number, _) = change.ask.unwrap();
        ready(&mut back, number, (480, 800));
    }
    let change = back.want(with(1200, 50), &mut cookie).unwrap();
    assert_eq!(change.release, Some(id(7)));
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
    assert_eq!(back.drawing(), None);
    let mut cookie = 0;
    back.want(recipe("a.png", FILL, 100, 0), &mut cookie)
        .unwrap();
    assert_eq!(back.drawing(), None);
}

#[test]
fn every_allocation_may_be_refused() {
    let mut cookie = 0;
    sweep(|| {
        let mut back = Backdrop::default();
        let wish = Recipe::new(&asked("p/a.jpg", TILE, 100, 20), 480, 800)?;
        let (number, _) = back.want(wish, &mut cookie)?.ask.ok_or(Error::Invalid)?;
        ready(&mut back, number, (600, 400));
        back.drawing().ok_or(Error::Invalid)?;
        Ok(())
    });
}

fn made_with(asked: Asked<'_>, size: (u32, u32), grid: Option<&[u8; 64]>) -> Backdrop {
    let mut back = Backdrop::default();
    let mut cookie = 0;
    let wish = Recipe::new(&asked, 480, 800).unwrap();
    let (number, _) = back.want(wish, &mut cookie).unwrap().ask.unwrap();
    back.ready(number, ImageStatus::Ready, Some(id(7)), size, grid);
    back
}

#[test]
fn the_tone_curve_is_asked_for_only_where_brightness_or_contrast_change() {
    let plain = made_with(asked("a.png", FILL, 100, 0), (600, 400), None);
    assert_eq!(plain.tone(), None);
    let brighter = Asked {
        brightness: crate::settings::BRIGHTNESSES[4],
        ..asked("a.png", FILL, 100, 0)
    };
    let curve = made_with(brighter, (600, 400), None).tone().unwrap();
    assert_eq!(curve, crate::native_picture::tone(2, 0));
    assert!(curve[128] > 128);
    let harder = Asked {
        contrast: crate::settings::CONTRASTS[2],
        ..asked("a.png", FILL, 100, 0)
    };
    assert_eq!(
        made_with(harder, (600, 400), None).tone(),
        Some(crate::native_picture::tone(0, 1))
    );
    // Nothing is asked for where no picture is wanted.
    assert_eq!(Backdrop::default().tone(), None);
}

#[test]
fn a_change_of_brightness_contrast_or_render_asks_for_the_picture_again() {
    let mut back = Backdrop::default();
    let mut cookie = 0;
    let first = back
        .want(recipe("a.png", FILL, 100, 0), &mut cookie)
        .unwrap();
    assert!(first.ask.is_some());
    let darker = Asked {
        brightness: crate::settings::BRIGHTNESSES[0],
        ..asked("a.png", FILL, 100, 0)
    };
    let again = back
        .want(Recipe::new(&darker, 480, 800).unwrap(), &mut cookie)
        .unwrap();
    assert!(again.ask.is_some());
    let grays = Asked {
        render: crate::settings::RENDERS[1],
        ..darker
    };
    let once_more = back
        .want(Recipe::new(&grays, 480, 800).unwrap(), &mut cookie)
        .unwrap();
    assert!(once_more.ask.is_some());
}

#[test]
fn the_render_choice_is_the_draw_flag() {
    let with = |render: &str| {
        let asked = Asked {
            render,
            ..asked("a.png", FILL, 100, 0)
        };
        made_with(asked, (600, 400), None).drawing().unwrap().render
    };
    assert_eq!(with(crate::settings::RENDERS[0]), Rendering::Panel);
    assert_eq!(with(crate::settings::RENDERS[1]), Rendering::Grays);
    assert_eq!(with(crate::settings::RENDERS[2]), Rendering::Dots);
}

/// A grid dark on its left half and light on its right.
fn halves() -> [u8; 64] {
    let mut grid = [0u8; 64];
    for (at, cell) in grid.iter_mut().enumerate() {
        *cell = if at % 8 < 4 { 10 } else { 240 };
    }
    grid
}

#[test]
fn the_brightness_behind_a_plate_is_read_from_the_hosts_grid() {
    let grid = halves();
    // Stretched over the 480 by 800 screen: the left half is dark.
    let back = made_with(asked("a.png", STRETCH, 100, 0), (480, 800), Some(&grid));
    let left = back.brightness_behind(Rect::new(0, 700, 200, 40)).unwrap();
    let right = back
        .brightness_behind(Rect::new(280, 700, 200, 40))
        .unwrap();
    assert!(left < 30, "{left}");
    assert!(right > 220, "{right}");
    assert_eq!(crate::native_picture::ink_over(Some(left)), 255);
    assert_eq!(crate::native_picture::ink_over(Some(right)), 0);
    // Paler by the lightening: the dark side is lifted towards the paper.
    let pale = made_with(asked("a.png", STRETCH, 100, 60), (480, 800), Some(&grid));
    assert!(pale.brightness_behind(Rect::new(0, 700, 200, 40)).unwrap() > 150);
}

#[test]
fn the_paper_is_what_the_picture_does_not_cover_and_no_grid_gives_no_brightness() {
    let grid = [0u8; 64];
    // A whole picture 600 by 400 is 480 by 320 on the screen, centred: the
    // strip at the foot of the screen is paper.
    let back = made_with(asked("a.png", WHOLE, 100, 0), (600, 400), Some(&grid));
    assert_eq!(
        back.brightness_behind(Rect::new(0, 760, 480, 40)),
        Some(255)
    );
    assert!(back.brightness_behind(Rect::new(0, 400, 480, 40)).unwrap() < 5);
    // The host made no grid, or no picture is held.
    let none = made_with(asked("a.png", WHOLE, 100, 0), (600, 400), None);
    assert_eq!(none.brightness_behind(Rect::new(0, 400, 480, 40)), None);
    assert_eq!(
        Backdrop::default().brightness_behind(Rect::new(0, 0, 10, 10)),
        None
    );
}
