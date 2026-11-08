//! The native readers of Open-Meteo's answers (`native_answers`) against the
//! interface-6 ones (`parse_forecast` and `parse_places` in `data.rs`): equal
//! on the same body at any piece size, refusing instead of aborting when
//! memory runs short; and the weekday of dates at the ends of the range.
use crate::data;
use crate::fail_alloc::sweep;
use crate::native_answers::{Forecast, Places};
use crate::native_weather as native;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use paperpaw_sdk::native::Error;

const FORECAST: &str = include_str!("../testdata/forecast.json");
const SEARCH: &str = include_str!("../testdata/search.json");

fn same_weather(old: &data::Weather, new: &native::Weather) {
    assert_eq!(
        (old.fetched, &old.coordinates),
        (new.fetched, &new.coordinates)
    );
    assert_eq!(old.current.is_some(), new.current.is_some());
    if let (Some(o), Some(n)) = (old.current, new.current) {
        assert_eq!(
            (o.temperature, o.feels, o.humidity, o.code, o.wind, o.day),
            (n.temperature, n.feels, n.humidity, n.code, n.wind, n.day)
        );
    }
    assert_eq!(old.days.len(), new.days.len());
    for (o, n) in old.days.iter().zip(&new.days) {
        assert_eq!(
            (o.date, o.code, o.high, o.low, o.rain, o.sunrise, o.sunset),
            (n.date, n.code, n.high, n.low, n.rain, n.sunrise, n.sunset)
        );
    }
}

fn forecasts() -> Vec<String> {
    let days = |n: usize| {
        let times: Vec<String> = (0..n)
            .map(|i| format!("\"2026-10-{:02}\"", i + 1))
            .collect();
        let nums = |v: &str| vec![v; n].join(",");
        format!(
            r#"{{"daily":{{"time":[{}],"weather_code":[{}],"temperature_2m_max":[{}],"temperature_2m_min":[{}],"precipitation_probability_max":[{}],"sunrise":[{}],"sunset":[{}]}}}}"#,
            times.join(","),
            nums("61"),
            nums("12.34"),
            nums("-1.5"),
            nums("40"),
            nums("\"2026-10-01T07:23\""),
            nums("\"2026-10-01T17:45\"")
        )
    };
    vec![
        FORECAST.to_string(),
        days(8),
        days(20),
        days(3),
        r#"{"current":{"temperature_2m":12.34,"weather_code":61}}"#.to_string(),
        r#"{"current":{"temperature_2m":12.34}}"#.to_string(),
        r#"{"current":{"temperature_2m":null,"weather_code":3,"is_day":0,"wind_speed_10m":15.5,"relative_humidity_2m":101,"apparent_temperature":-3.46}}"#.to_string(),
        r#"{"daily":{"time":["2026-10-01","bad","2026-10-03"],"weather_code":[1,2,3],"temperature_2m_max":[1,null,3],"temperature_2m_min":[0,0,0]}}"#.to_string(),
        r#"{"daily":{"time":["2026-10-01"],"weather_code":[null],"temperature_2m_max":[1],"temperature_2m_min":[0]}}"#.to_string(),
        r#"{"error":true,"reason":"Latitude must be in range of -90 to 90"}"#.to_string(),
        "<html>Bad gateway</html>".to_string(),
        String::new(),
        "{".to_string(),
    ]
}

#[test]
fn the_forecast_is_read_as_before() {
    for body in forecasts() {
        let old = data::parse_forecast(body.as_bytes(), "52.52,13.41", 1_760_000_000);
        let new = Forecast::read(body.as_bytes(), "52.52,13.41", 1_760_000_000).unwrap();
        assert_eq!(old.is_some(), new.is_some(), "{body:.80}");
        if let (Some(o), Some(n)) = (&old, &new) {
            same_weather(o, n);
        }
        // Whatever the size of the pieces the body comes in.
        for size in [1, 3, 64, 2048] {
            let mut reading = Forecast::new();
            for piece in body.as_bytes().chunks(size) {
                reading.feed(piece).unwrap();
            }
            let again = reading.finish("52.52,13.41", 1_760_000_000).unwrap();
            assert_eq!(old.is_some(), again.is_some(), "{size} {body:.80}");
            if let (Some(o), Some(n)) = (&old, &again) {
                same_weather(o, n);
            }
        }
    }
    let whole = Forecast::read(FORECAST.as_bytes(), "1,2", 5)
        .unwrap()
        .unwrap();
    assert!(whole.current.is_some());
    assert!(!whole.days.is_empty());
    assert!(whole.days.len() <= native::DAYS);
}

#[test]
fn places_are_read_as_before() {
    let many: String = format!(
        r#"{{"results":[{}]}}"#,
        (0..15)
            .map(|i| format!(
                r#"{{"name":"Place {i}","latitude":{i}.123,"longitude":-{i}.987,"admin1":"Region","country":"Land"}}"#
            ))
            .collect::<Vec<_>>()
            .join(",")
    );
    for body in [
        SEARCH,
        many.as_str(),
        r#"{"results":[{"name":"","latitude":1,"longitude":2},{"name":"Far","latitude":91,"longitude":2},{"name":"Ok","latitude":" 5.5 ","longitude":6}]}"#,
        r#"{"results":[{"name":"No coordinates"}]}"#,
        r#"{"generationtime_ms":0.5}"#,
        "<html></html>",
        "",
    ] {
        let old = data::parse_places(body.as_bytes());
        let new = Places::read(body.as_bytes()).unwrap();
        assert_eq!(old.len(), new.len(), "{body:.80}");
        for (o, n) in old.iter().zip(&new) {
            assert_eq!(
                (&o.name, &o.region, &o.country, &o.latitude, &o.longitude),
                (&n.name, &n.region, &n.country, &n.latitude, &n.longitude)
            );
        }
        for size in [1, 7, 2048] {
            let mut reading = Places::new();
            for piece in body.as_bytes().chunks(size) {
                reading.feed(piece).unwrap();
            }
            assert_eq!(old.len(), reading.finish().unwrap().len(), "{size}");
        }
    }
    assert_eq!(Places::read(many.as_bytes()).unwrap().len(), native::PLACES);
}

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
    for date in [-8, -1, 0, 1, 20_000, 20_375] {
        assert_eq!(native::weekday(date), data::weekday(date));
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
fn a_body_cut_inside_a_number_gives_no_value_as_before() {
    // `12` of `12.5` is not a temperature: the parser is never finished.
    let cut = r#"{"current":{"temperature_2m":12"#;
    let old = data::parse_forecast(cut.as_bytes(), "1,2", 5);
    let new = Forecast::read(cut.as_bytes(), "1,2", 5).unwrap();
    assert_eq!(old.is_some(), new.is_some());
    assert!(new.is_none_or(|w| w.current.is_none()));
    // The same at every length of the forecast and the search answers.
    for end in 0..FORECAST.len() {
        if !FORECAST.is_char_boundary(end) {
            continue;
        }
        let body = &FORECAST[..end];
        let old = data::parse_forecast(body.as_bytes(), "1,2", 5);
        let new = Forecast::read(body.as_bytes(), "1,2", 5).unwrap();
        assert_eq!(old.is_some(), new.is_some(), "forecast cut at {end}");
        if let (Some(o), Some(n)) = (&old, &new) {
            same_weather(o, n);
        }
    }
    for end in 0..SEARCH.len() {
        if !SEARCH.is_char_boundary(end) {
            continue;
        }
        let body = &SEARCH[..end];
        let old = data::parse_places(body.as_bytes());
        let new = Places::read(body.as_bytes()).unwrap();
        assert_eq!(old.len(), new.len(), "search cut at {end}");
    }
    let places = r#"{"results":[{"name":"Ok","latitude":5.5,"longitude":6"#;
    assert!(Places::read(places.as_bytes()).unwrap().is_empty());
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
    let short = Places::read(SEARCH.as_bytes()).unwrap();
    let old = data::parse_places(SEARCH.as_bytes());
    assert_eq!(old.len(), short.len());
    sweep(|| Places::read(body.as_bytes()).map(|p| p.len()));
}
