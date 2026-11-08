//! The library in a language that runs right to left: the cover of a book
//! on the right of its row, the covers of the grid from the right (the left
//! button on to the next), the row above the books starting at the right.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::*;
use paperpaw_sdk::Rect;

const TEXT: &[u8] = b"Some text of a book.\n\nAnd more of it.\n";

/// A card with `language` and some books; for `ar` the catalog says the
/// direction and nothing else (the other texts are English).
fn card(language: &str, view: &str) -> Shared {
    let files: Vec<(String, Vec<u8>)> = (1..=6)
        .map(|n| (format!("books/Book {n:02}.txt"), TEXT.to_vec()))
        .collect();
    let mut all: Vec<(&str, &[u8])> = files
        .iter()
        .map(|(p, d)| (p.as_str(), d.as_slice()))
        .collect();
    all.push((
        "plugins/language-ar/plugin.ini",
        b"[plugin]\nid = language-ar\nname = Arabic\nversion = 1.0.0\nformats = 3\n",
    ));
    all.push((
        "plugins/language-ar/i18n/ar.ini",
        b"[meta]\nlanguage = ar\nname = Arabic\nplural = arabic\ndirection = rtl\nscript = Arab\n",
    ));
    Shared::memory(
        &format!("[core]\nlanguage = {language}\n[files]\nview = {view}\n"),
        &all,
    )
}

fn library(device: paperpaw_platform::DeviceInfo, language: &str, view: &str) -> App {
    let mut app = App::new(device, card(language, view));
    app.run(actions::LIBRARY);
    assert_eq!(app.screen(), "files.browser");
    app
}

fn rows(app: &mut App) -> Vec<(String, Rect)> {
    app.parts()
        .into_iter()
        .filter(|p| p.role == Role::Row)
        .map(|p| (p.label, p.area))
        .collect()
}

/// The dark pixels of `area` left and right of its middle.
fn ink_sides(app: &mut App, area: Rect) -> (u32, u32) {
    let middle = i32::from(area.x) + i32::from(area.width) / 2;
    let (mut left, mut right) = (0, 0);
    for y in i32::from(area.y)..i32::from(area.y) + i32::from(area.height) {
        for x in i32::from(area.x)..i32::from(area.x) + i32::from(area.width) {
            if app.dark(x, y) {
                if x < middle {
                    left += 1;
                } else {
                    right += 1;
                }
            }
        }
    }
    (left, right)
}

#[test]
fn the_cover_of_a_book_is_on_the_right_where_the_interface_runs_right_to_left() {
    for device in DEVICES {
        let mut ltr = library(device, "en", "files.view.list");
        let mut rtl = library(device, "ar", "files.view.list");
        let books = |app: &mut App| -> Vec<Rect> {
            rows(app)
                .into_iter()
                .filter(|(l, _)| l.starts_with("Book "))
                .map(|(_, a)| a)
                .collect()
        };
        let (a, b) = (books(&mut ltr), books(&mut rtl));
        assert!(a.len() >= 3, "{a:?}");
        assert_eq!(a.len(), b.len());
        // The same rows in both; the ink of a row is the mirror of the
        // other: the side with the cover (and the title's start) is the
        // heavier one.
        let (l_left, l_right) = ink_sides(&mut ltr, a[0]);
        let (r_left, r_right) = ink_sides(&mut rtl, b[0]);
        assert_eq!(a[0], b[0]);
        assert_ne!((l_left, l_right), (r_left, r_right));
        assert_eq!(
            l_left > l_right,
            r_right > r_left,
            "{l_left}/{l_right} {r_left}/{r_right}"
        );
    }
}

#[test]
fn the_first_cover_of_the_grid_is_at_the_right_and_the_left_button_goes_on() {
    let covers = |app: &mut App| -> Vec<Rect> {
        rows(app)
            .into_iter()
            .filter(|(l, _)| l.starts_with("Book "))
            .map(|(_, a)| a)
            .collect()
    };
    let mut ltr = library(X4, "en", "files.view.covers");
    let mut rtl = library(X4, "ar", "files.view.covers");
    let (a, b) = (covers(&mut ltr), covers(&mut rtl));
    assert!(a.len() >= 3 && a.len() == b.len(), "{a:?} {b:?}");
    assert!(a[0].x < a[1].x, "{a:?}");
    assert!(b[0].x > b[1].x, "{b:?}");
    assert_eq!(a[0].y, b[0].y);
    // Where the first is on the right, the left button moves the focus on
    // to the next, the right button back.
    let focus = |app: &mut App| app.focused().unwrap_or_default();
    let first = focus(&mut rtl);
    rtl.run(actions::LEFT);
    let second = focus(&mut rtl);
    assert_ne!(first, second);
    rtl.run(actions::RIGHT);
    assert_eq!(focus(&mut rtl), first);
}

#[test]
fn the_row_above_the_books_starts_at_the_right() {
    for device in DEVICES {
        let all = |app: &mut App| -> Rect {
            app.parts()
                .into_iter()
                .find(|p| p.label.starts_with("All books"))
                .map(|p| p.area)
                .unwrap()
        };
        let mut ltr = library(device, "en", "files.view.list");
        let mut rtl = library(device, "ar", "files.view.list");
        let area = all(&mut ltr);
        assert_eq!(area, all(&mut rtl));
        // The label with the book count on the other side: the ink of the
        // row is heavier where the label is.
        let (l_left, l_right) = ink_sides(&mut ltr, area);
        let (r_left, r_right) = ink_sides(&mut rtl, area);
        assert!(l_left > l_right, "{l_left}/{l_right}");
        assert!(r_right > r_left, "{r_left}/{r_right}");
    }
}
