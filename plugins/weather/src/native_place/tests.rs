//! The place view's state, rows and decisions, without a host.
#![allow(clippy::unwrap_used, reason = "test assertions")]
use super::*;
use crate::fail_alloc::sweep;
use alloc::vec;

/// The catalog, as the keys and arguments written out: `key|name=value|…`.
struct Keys;

impl Words for Keys {
    fn copy(&mut self, key: &str) -> Result<String, Error> {
        text::copy(key)
    }

    fn format(&mut self, key: &str, args: &[Arg<'_>]) -> Result<String, Error> {
        let mut out = text::copy(key)?;
        let mut digits = [0u8; 20];
        for arg in args {
            let (name, value) = match *arg {
                Arg::Text { name, value } => (name, value),
                Arg::Count(n) => (
                    "count",
                    text::decimal(usize::try_from(n).unwrap(), &mut digits),
                ),
                _ => continue,
            };
            out.try_reserve(name.len() + value.len() + 2)
                .map_err(|_| Error::NoMemory)?;
            out.push('|');
            out.push_str(name);
            out.push('=');
            out.push_str(value);
        }
        Ok(out)
    }
}

fn place(name: &str, region: &str, country: &str) -> Place {
    Place {
        name: name.into(),
        region: region.into(),
        country: country.into(),
        latitude: "52.52".into(),
        longitude: "13.41".into(),
    }
}

fn berlin() -> Vec<Place> {
    vec![
        place("Berlin", "Land Berlin", "Germany"),
        place("Berlin", "New Hampshire", "United States"),
        place("Berlin", "Berlin", ""),
    ]
}

fn view(place: &str, has_place: bool) -> PlaceView {
    let mut view = PlaceView {
        places: Vec::new(),
        search: Search::Idle,
        place: place.into(),
        has_place,
        asked: false,
        offer: None,
        list: ItemList::default(),
        focus: None,
    };
    view.refresh(&mut Keys).unwrap();
    view
}

fn key(view: &mut PlaceView, action: &str) -> Act {
    let (act, run) = view.decide(&Input::Action { now_ms: 0, action });
    assert_eq!(run, None);
    act
}

fn tap(view: &mut PlaceView, x: i32, y: i32) -> Act {
    let input = Input::Gesture {
        now_ms: 0,
        position: paperpaw_sdk::Point::new(x, y),
        kind: paperpaw_sdk::native::callbacks::InputGestureKind::Tap,
    };
    view.decide(&input).0
}

fn texts(view: &PlaceView) -> Vec<(String, String, String)> {
    view.rows(&mut Keys)
        .unwrap()
        .into_iter()
        .map(|item| match item {
            Item::Row(row) => (row.title, row.value, row.note),
            Item::Heading(title) => (title, String::new(), String::new()),
        })
        .collect()
}

#[test]
fn the_first_row_says_what_is_set_or_how_to_search() {
    let rows = texts(&view("", false));
    assert_eq!(
        rows,
        vec![(
            "weather.place.search".into(),
            "›".into(),
            "weather.place.help".into()
        )]
    );
    let rows = texts(&view("Berlin", true));
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].2, "weather.place.now|place=Berlin");
    assert_eq!(
        rows[1],
        (
            "weather.place.none".into(),
            String::new(),
            "weather.place.none.help".into()
        )
    );
}

#[test]
fn each_state_of_the_search_has_its_note() {
    let mut v = view("", false);
    v.searching("Berlin".into()).unwrap();
    assert_eq!(v.note(&mut Keys).unwrap(), "weather.place.searching");
    assert_eq!(v.initial(), "Berlin");
    v.found(berlin(), "Berlin".into()).unwrap();
    assert_eq!(v.note(&mut Keys).unwrap(), "weather.place.found|count=3");
    assert_eq!(v.initial(), "");
    v.found(Vec::new(), "Xyz".into()).unwrap();
    assert_eq!(v.note(&mut Keys).unwrap(), "weather.place.nothing|name=Xyz");
    assert_eq!(v.initial(), "Xyz");
    v.ended(Search::Failed).unwrap();
    assert_eq!(v.note(&mut Keys).unwrap(), "weather.place.failed");
    v.ended(Search::NoNetwork).unwrap();
    assert_eq!(v.note(&mut Keys).unwrap(), "weather.place.no-network");
}

#[test]
fn the_places_found_follow_the_first_row_and_no_place_comes_last() {
    let mut v = view("Paris", true);
    v.found(berlin(), "Berlin".into()).unwrap();
    let rows = texts(&v);
    assert_eq!(rows.len(), 5);
    assert_eq!(
        (rows[1].0.as_str(), rows[1].1.as_str(), rows[1].2.as_str()),
        ("Berlin", "", "Land Berlin, Germany")
    );
    // The region of the name is not said twice; no country, no comma.
    assert_eq!(rows[3].2, "");
    assert_eq!(rows[4].0, "weather.place.none");
    assert_eq!(v.count(), 5);
    assert_eq!(
        (v.row(0), v.row(1), v.row(3), v.row(4), v.row(5)),
        (
            Some(Row::Search),
            Some(Row::Found(0)),
            Some(Row::Found(2)),
            Some(Row::None),
            None
        )
    );
    // Without a place set there is no last row.
    v.set_place("", false).unwrap();
    assert_eq!(v.count(), 4);
    assert_eq!(v.row(4), None);
    assert_eq!(
        v.place(1).map(|p| p.country.as_str()),
        Some("United States")
    );
    assert!(v.place(3).is_none());
}

#[test]
fn a_changed_place_changes_the_note() {
    let mut v = view("", false);
    v.set_place("Oslo", true).unwrap();
    assert_eq!(texts(&v)[0].2, "weather.place.now|place=Oslo");
    v.set_place("", false).unwrap();
    assert_eq!(texts(&v)[0].2, "weather.place.help");
}

#[test]
fn the_keyboard_opens_at_once_only_where_no_place_is_set_and_once() {
    let mut v = view("", false);
    assert!(v.asks_at_once());
    assert!(!v.asks_at_once());
    let mut v = view("Berlin", true);
    assert!(!v.asks_at_once());
    assert!(!v.asks_at_once());
}

#[test]
fn a_search_or_its_answer_puts_the_focus_on_the_first_place() {
    let mut v = view("Paris", true);
    v.found(berlin(), "Berlin".into()).unwrap();
    v.refresh(&mut Keys).unwrap();
    assert_eq!(v.focus(), 1);
    // Down to the last row, then a new search brings it back to the first row.
    for _ in 0..3 {
        assert_eq!(key(&mut v, "core.down"), Act::Taken);
    }
    assert_eq!(v.focus(), 4);
    v.searching("Rome".into()).unwrap();
    v.refresh(&mut Keys).unwrap();
    assert_eq!(v.focus(), 0);
    // Found nothing: the first row stays focused.
    v.found(Vec::new(), "Rome".into()).unwrap();
    v.refresh(&mut Keys).unwrap();
    assert_eq!(v.focus(), 0);
    v.ended(Search::Failed).unwrap();
    v.refresh(&mut Keys).unwrap();
    assert_eq!(v.focus(), 0);
}

#[test]
fn select_on_a_row_gives_its_act() {
    let mut v = view("Paris", true);
    v.found(berlin(), "Berlin".into()).unwrap();
    v.refresh(&mut Keys).unwrap();
    // On the first place, as the answer left the focus.
    assert_eq!(key(&mut v, "core.confirm"), Act::Pick(0));
    assert_eq!(key(&mut v, "core.down"), Act::Taken);
    assert_eq!(key(&mut v, "core.confirm"), Act::Pick(1));
    for _ in 0..2 {
        key(&mut v, "core.next");
    }
    assert_eq!(key(&mut v, "core.confirm"), Act::Clear);
    // At the end the focus stays and the key is taken.
    assert_eq!(key(&mut v, "core.down"), Act::Taken);
    assert_eq!(v.focus(), 4);
    for _ in 0..4 {
        key(&mut v, "core.up");
    }
    assert_eq!(v.focus(), 0);
    assert_eq!(key(&mut v, "core.confirm"), Act::Ask(String::new()));
}

#[test]
fn the_keyboard_starts_with_the_name_searched_for() {
    let mut v = view("", false);
    v.found(Vec::new(), "Xyz".into()).unwrap();
    v.refresh(&mut Keys).unwrap();
    assert_eq!(key(&mut v, "core.confirm"), Act::Ask("Xyz".into()));
}

#[test]
fn keys_of_the_hosts_own_are_not_taken() {
    let mut v = view("", false);
    for action in ["core.back", "core.menu", "core.home"] {
        let act = key(&mut v, action);
        assert_eq!(act, Act::Ignored);
        assert_eq!(PlaceView::handled(&act), Handled::No);
    }
    assert_eq!(PlaceView::handled(&Act::Taken), Handled::Yes);
    assert_eq!(PlaceView::handled(&Act::Back), Handled::Yes);
}

#[test]
fn a_row_that_is_gone_is_not_taken_for_another() {
    let mut v = view("Paris", true);
    v.found(berlin(), "Berlin".into()).unwrap();
    v.refresh(&mut Keys).unwrap();
    for _ in 0..4 {
        key(&mut v, "core.down");
    }
    // The places went (a new search) before the list was drawn again.
    v.searching("Rome".into()).unwrap();
    assert_eq!(v.count(), 2);
    assert_eq!(key(&mut v, "core.confirm"), Act::Ignored);
}

#[test]
fn taps_off_the_rows_are_not_taken() {
    // Nothing was drawn: no hits.
    let mut v = view("", false);
    assert_eq!(tap(&mut v, 10, 10), Act::Ignored);
}

#[test]
fn the_offer_is_shown_instead_of_the_list_and_gets_the_keys() {
    let mut v = view("", false);
    v.searching("Berlin".into()).unwrap();
    v.offer("Berlin".into());
    assert!(v.offering());
    assert_eq!(v.offer_name(), Some("Berlin"));
    // The list's keys go to the buttons; *Go online* is the first.
    assert_eq!(key(&mut v, "core.confirm"), Act::Online("Berlin".into()));
    assert!(!v.offering());

    v.offer("Berlin".into());
    assert_eq!(key(&mut v, "core.down"), Act::Taken);
    assert_eq!(v.offer.as_ref().map(Offer::focus), Some(1));
    assert_eq!(key(&mut v, "core.confirm"), Act::NotNow);
    assert!(!v.offering());

    v.offer("Berlin".into());
    key(&mut v, "core.next");
    assert_eq!(key(&mut v, "core.up"), Act::Taken);
    assert_eq!(key(&mut v, "core.confirm"), Act::Online("Berlin".into()));
}

#[test]
fn the_offer_leaves_the_hosts_own_keys_and_swallows_taps() {
    let mut v = view("", false);
    v.offer("Berlin".into());
    assert_eq!(key(&mut v, "core.back"), Act::Ignored);
    assert_eq!(key(&mut v, "core.menu"), Act::Ignored);
    assert!(v.offering());
    // No drawing yet: a tap does nothing, but it is not the list's.
    assert_eq!(tap(&mut v, 10, 10), Act::Taken);
    assert!(v.offering());
}

#[test]
fn an_answer_or_a_new_search_ends_the_offer() {
    let mut v = view("", false);
    v.offer("A".into());
    v.searching("A".into()).unwrap();
    assert!(!v.offering());
    v.offer("A".into());
    v.ended(Search::NoNetwork).unwrap();
    assert!(!v.offering());
    v.offer("A".into());
    v.found(berlin(), "A".into()).unwrap();
    assert!(!v.offering());
}

#[test]
fn making_the_rows_survives_every_refused_allocation() {
    let places = berlin();
    let mut v = view("Paris", true);
    v.places = places;
    v.search = Search::Found;
    let total = sweep(|| v.rows(&mut Keys).map(|rows| rows.len()));
    assert!(total > 0);
    let mut v = view("Paris", true);
    sweep(|| v.set_place("Oslo", true));
}
