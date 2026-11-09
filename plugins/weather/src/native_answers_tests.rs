//! The native readers of Open-Meteo's answers (`native_answers`): refusing
//! instead of aborting when memory runs short, a reading that stays spent, the
//! weekday of dates at the ends of the range and the caps on a place's text.
//! What they read from recorded and odd answers is the transcript in
//! `native_golden_tests`.
use crate::fail_alloc::sweep;
use crate::native_answers::{Forecast, Places};
use crate::native_weather as native;
use alloc::format;
use paperpaw_sdk::native::Error;

const FORECAST: &str = include_str!("../testdata/forecast.json");
const SEARCH: &str = include_str!("../testdata/search.json");

#[test]
fn the_weekday_does_not_overflow_at_the_ends() {
    for date in [
        i32::MIN,
        i32::MIN + 1,
        -8,
        -1,
        0,
        1,
        6,
        20_000,
        i32::MAX - 4,
        i32::MAX - 3,
        i32::MAX,
    ] {
        let want = (i64::from(date) + 4).rem_euclid(7) as u8;
        assert_eq!(native::weekday(date), want, "{date}");
        assert!(native::weekday(date) < 7);
    }
}

#[test]
fn a_spent_reading_stays_spent() {
    // Memory refused inside a piece ends the reading: every later piece, and
    // the result, say so.
    let mut failed = false;
    sweep(|| {
        let mut reading = Forecast::new();
        for piece in FORECAST.as_bytes().chunks(40) {
            let result = reading.feed(piece);
            if result.is_err() {
                failed = true;
                assert_eq!(reading.feed(b" "), Err(Error::NoMemory));
                assert_eq!(
                    reading.finish("1,2", 0).err(),
                    Some(Error::NoMemory),
                    "finish after an error"
                );
                return result.map(|()| 0);
            }
        }
        reading
            .finish("1,2", 0)
            .map(|w| w.map_or(0, |w| w.days.len()))
    });
    assert!(failed);
}

#[test]
fn refused_allocations_end_in_no_memory() {
    sweep(|| {
        Forecast::read(FORECAST.as_bytes(), "52.52,13.41", 1).map(|w| w.map(|w| w.days.len()))
    });
    sweep(|| Places::read(SEARCH.as_bytes()).map(|p| p.len()));
}

#[test]
fn a_places_name_region_and_country_are_cut() {
    // Multi-byte letters, so that a cut falls inside a character if made
    // carelessly.
    let long = "\u{e4}".repeat(2000);
    let body = format!(
        r#"{{"results":[{{"name":"{long}","admin1":"{long}","country":"{long}","latitude":1,"longitude":2}}]}}"#
    );
    let places = Places::read(body.as_bytes()).unwrap();
    assert_eq!(places.len(), 1);
    let place = &places[0];
    assert!(!place.name.is_empty());
    assert!(place.name.len() <= native::PLACE_NAME_CAP);
    assert!(place.region.len() <= native::PLACE_PART_CAP);
    assert!(place.country.len() <= native::PLACE_PART_CAP);
    assert!(place.name.chars().all(|c| c == '\u{e4}'));
    // Names under the caps are untouched.
    assert_eq!(Places::read(SEARCH.as_bytes()).unwrap().len(), 3);
    sweep(|| Places::read(body.as_bytes()).map(|p| p.len()));
}

#[test]
fn the_models_allocations_end_in_no_memory_when_refused() {
    let weather = Forecast::read(FORECAST.as_bytes(), "52.52,13.41", 1_760_000_000)
        .unwrap()
        .unwrap();
    let text = weather.to_text().unwrap();
    sweep(|| native::Weather::from_text(&text).map(|w| w.map(|w| w.days.len())));
    sweep(|| {
        native::Weather::from_file(b"weather 1\nfetched 3\ncoordinates 1,2\n\xff\n")
            .map(|w| w.is_some())
    });
    sweep(|| weather.to_text().map(|t| t.len()));
    sweep(|| native::forecast_url("52.52,13.41").map(|u| u.map(|u| u.len())));
    sweep(|| native::search_url("Sankt Gallen \u{fc}", "de-CH").map(|u| u.len()));
    sweep(|| native::short_coordinate("52.52437").map(|c| c.len()));
    sweep(|| native::degrees_text(native::Temperature::Fahrenheit, -355, true).map(|t| t.len()));
    let place = native::Place {
        name: "Berlin".into(),
        region: "Land Berlin".into(),
        country: "Germany".into(),
        latitude: "52.52".into(),
        longitude: "13.41".into(),
    };
    sweep(|| place.whereabouts().map(|t| t.len()));
    sweep(|| place.coordinates().map(|t| t.len()));
}

#[test]
fn the_kept_file_is_read_to_its_cap_only() {
    let weather = Forecast::read(FORECAST.as_bytes(), "52.52,13.41", 1_760_000_000)
        .unwrap()
        .unwrap();
    let mut text = weather.to_text().unwrap();
    assert!(text.len() < native::KEPT_CAP);
    assert!(
        native::Weather::from_file(text.as_bytes())
            .unwrap()
            .is_some()
    );
    // At the cap it is read; one byte over counts as none.
    while text.len() < native::KEPT_CAP {
        text.push('\n');
    }
    assert!(
        native::Weather::from_file(text.as_bytes())
            .unwrap()
            .is_some()
    );
    text.push('\n');
    assert!(
        native::Weather::from_file(text.as_bytes())
            .unwrap()
            .is_none()
    );
    // Invalid UTF-8 is replaced, not refused.
    let bytes = b"weather 1\nfetched 3\ncoordinates 1,2\n\xff\xfe\n";
    assert!(native::Weather::from_file(bytes).unwrap().is_some());
    assert!(native::Weather::from_file(&bytes[..3]).unwrap().is_none());
}
