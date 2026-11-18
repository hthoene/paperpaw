use super::*;
use alloc::string::String;
use paperpaw_sdk::Point;

fn grid() -> Geometry {
    // 4 columns of 100 px, 3 rows of 85 px, no gap.
    Geometry::of(Rect::new(0, 0, 400, 255), 4, 3, 0)
}

/// The host as a script: what it makes and composes, and what it refuses.
#[derive(Default)]
struct Script {
    next: u32,
    made: Vec<(u32, String)>,
    released: Vec<u32>,
    composed: Vec<(u32, Rect)>,
    make_error: Option<(&'static str, Error)>,
    compose_error: Option<(Option<u32>, Error)>,
}

impl Stage for Script {
    type Handle = u32;

    fn make(&mut self, id: &str, _area: Rect) -> Result<u32, Error> {
        if let Some((refused, error)) = &self.make_error
            && (*refused == id || refused.is_empty())
        {
            return Err(*error);
        }
        self.next += 1;
        self.made.push((self.next, String::from(id)));
        Ok(self.next)
    }

    fn release(&mut self, widget: u32) {
        self.released.push(widget);
    }

    fn compose(&mut self, widget: &u32, area: Rect) -> Result<(), Error> {
        if let Some((only, error)) = &self.compose_error
            && only.is_none_or(|only| only == *widget)
        {
            return Err(*error);
        }
        self.composed.push((*widget, area));
        Ok(())
    }
}

/// Five widgets of two rows each in an area that holds two of them: three pages.
fn gallery() -> Gallery<u32> {
    let widgets: Vec<(usize, (u8, u8))> = (0..5).map(|row| (row, (4, 2))).collect();
    let area = Rect::new(0, 40, 400, 400);
    let previews = lay_out(&widgets, &grid(), area, 20, 5).unwrap();
    Gallery::new(previews)
}

fn id_of(row: usize) -> Option<&'static str> {
    ["a", "b", "c", "d", "e"].get(row).copied()
}

#[test]
fn the_previews_fill_a_page_then_start_the_next() {
    let gallery = gallery();
    // 20 (name) + 170 (two rows) + 10 (gap) = 200 a preview in 400.
    let pages: Vec<usize> = gallery.previews().iter().map(|p| p.page).collect();
    assert_eq!(pages, [0, 0, 1, 1, 2]);
    assert_eq!(gallery.pages(), 3);
    let first = gallery.previews()[0];
    assert_eq!(first.title.y, 45);
    assert_eq!(first.area.y, 65);
    assert_eq!(first.area.height, 170);
    assert_eq!(gallery.page_of(3), 1);
    assert_eq!(gallery.first_row(2), Some(4));
}

#[test]
fn a_widget_is_as_high_as_it_would_be_up_to_three_rows() {
    let area = Rect::new(0, 0, 400, 1000);
    let previews = lay_out(&[(0, (4, 9)), (1, (2, 0))], &grid(), area, 10, 0).unwrap();
    assert_eq!(previews[0].cells, (4, PREVIEW_ROWS));
    assert_eq!(previews[0].area.height, 255);
    // A widget that is none high is one row; two columns are half the width.
    assert_eq!(previews[1].cells, (2, 1));
    assert_eq!(previews[1].area.width, 200);
}

#[test]
fn one_that_is_too_high_for_the_area_still_gets_a_page() {
    let area = Rect::new(0, 0, 400, 50);
    let previews = lay_out(&[(0, (4, 3)), (1, (4, 3))], &grid(), area, 10, 0).unwrap();
    assert_eq!(previews[0].page, 0);
    assert_eq!(previews[1].page, 1);
}

#[test]
fn a_tap_on_the_name_or_the_widget_reaches_its_row() {
    let gallery = gallery();
    let first = gallery.previews()[0];
    let on_title = Point::new(i32::from(first.title.x) + 1, i32::from(first.title.y) + 1);
    let on_area = Point::new(i32::from(first.area.x) + 1, i32::from(first.area.y) + 1);
    assert_eq!(gallery.row_at(0, on_title), Some(0));
    assert_eq!(gallery.row_at(0, on_area), Some(0));
    // Another page's widget is not there.
    assert_eq!(gallery.row_at(1, on_area), Some(2));
    assert_eq!(gallery.row_at(0, Point::new(1, 1)), None);
}

#[test]
fn the_widgets_of_the_page_shown_are_made_and_composed_into_their_places() {
    let (mut gallery, mut host) = (gallery(), Script::default());
    gallery.show(&mut host, 0, id_of).unwrap();
    let ids: Vec<&str> = host.made.iter().map(|(_, id)| id.as_str()).collect();
    assert_eq!(ids, ["a", "b"]);
    assert_eq!(gallery.compose(&mut host, 0), Drawn::Rows(Vec::new()));
    let areas: Vec<Rect> = host.composed.iter().map(|(_, area)| *area).collect();
    assert_eq!(
        areas,
        [gallery.previews()[0].area, gallery.previews()[1].area]
    );
    // Showing it again makes nothing twice.
    gallery.show(&mut host, 0, id_of).unwrap();
    assert_eq!(host.made.len(), 2);
}

#[test]
fn leaving_a_page_ends_its_widgets_and_the_screen_ends_the_rest() {
    let (mut gallery, mut host) = (gallery(), Script::default());
    gallery.show(&mut host, 0, id_of).unwrap();
    gallery.show(&mut host, 1, id_of).unwrap();
    assert_eq!(host.released, [1, 2]);
    assert_eq!(host.made.len(), 4);
    gallery.release(&mut host);
    assert_eq!(host.released, [1, 2, 3, 4]);
}

#[test]
fn a_widget_the_host_cannot_make_is_a_text_row_and_the_rest_still_show() {
    for error in [Error::Full, Error::NoMemory, Error::Io] {
        let (mut gallery, mut host) = (
            gallery(),
            Script {
                make_error: Some(("a", error)),
                ..Script::default()
            },
        );
        gallery.show(&mut host, 0, id_of).unwrap();
        assert_eq!(
            gallery.compose(&mut host, 0),
            Drawn::Rows(alloc::vec![0]),
            "{error:?}"
        );
        assert_eq!(host.composed.len(), 1);
    }
}

#[test]
fn a_widget_the_host_will_not_compose_is_a_text_row() {
    let (mut gallery, mut host) = (
        gallery(),
        Script {
            compose_error: Some((Some(2), Error::Io)),
            ..Script::default()
        },
    );
    gallery.show(&mut host, 0, id_of).unwrap();
    assert_eq!(gallery.compose(&mut host, 0), Drawn::Rows(alloc::vec![1]));
    assert_eq!(host.composed.len(), 1);
}

#[test]
fn a_host_that_composes_nothing_turns_the_whole_gallery_into_the_list() {
    // Unsupported at the compose slot (the X4 until its stack gate).
    let (mut gallery, mut host) = (
        gallery(),
        Script {
            compose_error: Some((None, Error::Unsupported)),
            ..Script::default()
        },
    );
    gallery.show(&mut host, 0, id_of).unwrap();
    assert_eq!(gallery.compose(&mut host, 0), Drawn::List);
    assert!(host.composed.is_empty());
    // Or where the widgets cannot even be placed.
    let (mut gallery, mut host) = (
        self::gallery(),
        Script {
            make_error: Some(("", Error::Unsupported)),
            ..Script::default()
        },
    );
    assert_eq!(gallery.show(&mut host, 0, id_of), Err(Error::Unsupported));
}
