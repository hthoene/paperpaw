//! The native decisions (`native_weather`) against the interface-6 ones
//! (`data.rs`, `lib.rs`): equal on the same input, and refusing instead of
//! aborting when memory runs short. The read cap of the kept weather is that
//! of interface 6.
use crate::data::{self, Temperature, Wind};
use crate::fail_alloc::sweep;
use crate::native_weather as native;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

const FORECAST: &str = include_str!("../testdata/forecast.json");

#[test]
fn numbers_dates_and_coordinates_are_those_of_interface_6() {
    for text in [
        "12.3",
        "-3.46",
        "-0.4",
        "7",
        ".5",
        "null",
        "",
        "1e5",
        " 4.55 ",
        "-",
        ".",
        "1234567",
        "123456.78",
        "-.05",
        "0.04",
        "9.95",
        "--1",
        "1.2.3",
        "٣",
    ] {
        assert_eq!(native::tenths(text), data::tenths(text), "{text:?}");
    }
    for text in [
        "2026-10-14",
        "2026-10-14T07:23",
        "1970-01-02",
        "14.10.2026",
        "2026-13-01",
        "2026-02-31",
        "20261014",
        "",
        "2026-1-1xxxxx",
    ] {
        assert_eq!(native::parse_date(text), data::parse_date(text), "{text:?}");
        assert_eq!(
            native::parse_minutes(text),
            data::parse_minutes(text),
            "{text:?}"
        );
    }
    for text in [
        "2026-10-14T07:23",
        "2026-10-14T24:00",
        "T9:5x",
        "T07:61",
        "x",
    ] {
        assert_eq!(native::parse_minutes(text), data::parse_minutes(text));
    }
    for (y, m, d) in [
        (1970, 1, 1),
        (2026, 10, 14),
        (2000, 2, 29),
        (1900, 3, 1),
        (-1, 12, 31),
    ] {
        assert_eq!(
            native::days_from_civil(y, m, d),
            data::days_from_civil(y, m, d)
        );
    }
    for date in [-8, -1, 0, 1, 7, 20_000, 20_375] {
        assert_eq!(native::weekday(date), data::weekday(date));
    }
    for text in [
        "52.52,13.41",
        " 52.52 , -13.41 ",
        "90,180",
        "90.1,0",
        "0,180.1",
        "-90,-180",
        "x,y",
        "52.52",
        "",
        ",",
        "1e1,2",
        "52.5.2,1",
        "+5,1",
    ] {
        assert_eq!(
            native::split_coordinates(text),
            data::split_coordinates(text),
            "{text:?}"
        );
    }
    for text in [
        "52.52437", "52.5", "52.50", "52.00", "52", " 13.405 ", "-0.001", "5.", ".5",
    ] {
        assert_eq!(
            native::short_coordinate(text).unwrap(),
            data::short_coordinate(text),
            "{text:?}"
        );
    }
}

#[test]
fn addresses_are_those_of_interface_6() {
    for coordinates in ["52.52,13.41", "x", "", "91,0", " 1 , 2 "] {
        assert_eq!(
            native::forecast_url(coordinates).unwrap(),
            data::forecast_url(coordinates),
            "{coordinates:?}"
        );
    }
    assert_eq!((native::DAYS, native::PLACES), (data::DAYS, data::PLACES));
    for name in [
        "Berlin",
        " Sankt Gallen ",
        "Zürich",
        "a&b=c",
        "",
        "日本",
        "~._-",
        "100%",
    ] {
        for language in [
            "de", "en-GB", "pt_BR", "", "1", "  ", "-de", "DE", "zh-Hans",
        ] {
            assert_eq!(
                native::search_url(name, language).unwrap(),
                data::search_url(name, language),
                "{name:?} {language:?}"
            );
        }
    }
}

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

#[test]
fn the_kept_weather_is_read_and_written_as_before() {
    let weather = data::parse_forecast(FORECAST.as_bytes(), "52.52,13.41", 1_760_000_000).unwrap();
    let text = weather.to_text();
    let new = native::Weather::from_text(&text).unwrap().unwrap();
    same_weather(&weather, &new);
    assert_eq!(new.to_text().unwrap(), text);
    let many: String = (0..12).map(|i| format!("day {i} 1 2 3 - - -\n")).collect();
    let texts: Vec<String> = vec![
        text.clone(),
        String::new(),
        "weather 2\nfetched 1\n".into(),
        "weather 1\n".into(),
        "  weather 1  \nfetched 5\ncoordinates 1,2\n".into(),
        "weather 1\nfetched x\n".into(),
        "weather 1\ncoordinates\n".into(),
        "weather 1\nnow 12 - - 3 - 1\nnow 99\n".into(),
        "weather 1\nnow 12 - 300 999 -5 0\n".into(),
        "weather 1\nnow 12 - - 3\n".into(),
        "weather 1\nfetched 1 2 3 4 5 6 7 8 9\nunknown 1\n\n".into(),
        "weather 1\nday 1 2 3 4 5 6 7\nday 1 2 3\nday x 2 3 4\nday 5 600 7 8 -9 99999 -1\n".into(),
        format!("weather 1\n{many}"),
    ];
    for text in &texts {
        let old = data::Weather::from_text(text);
        let new = native::Weather::from_text(text).unwrap();
        assert_eq!(old.is_some(), new.is_some(), "{text:?}");
        if let (Some(o), Some(n)) = (&old, &new) {
            same_weather(o, n);
            let today = [None, Some(0), Some(20_000)];
            for today in today {
                assert_eq!(
                    o.days_from(today).len(),
                    n.days_from(today).len(),
                    "{text:?}"
                );
            }
        }
    }
    assert!(native::Weather::from_text(&many).unwrap().is_none());
}

#[test]
fn the_kept_file_is_read_to_its_cap_only() {
    assert_eq!(native::KEPT_CAP, 4 * 1024);
    let weather = data::parse_forecast(FORECAST.as_bytes(), "52.52,13.41", 1_760_000_000).unwrap();
    let mut text = weather.to_text();
    assert!(text.len() < native::KEPT_CAP);
    assert!(
        native::Weather::from_file(text.as_bytes())
            .unwrap()
            .is_some()
    );
    // At the cap it is read; one byte over counts as none, as in interface 6.
    while text.len() < native::KEPT_CAP {
        text.push('\n');
    }
    assert_eq!(text.len(), native::KEPT_CAP);
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
    let mut bytes = b"weather 1\nfetched 3\ncoordinates 1,2\n\xff\xfe\n".to_vec();
    let new = native::Weather::from_file(&bytes).unwrap().unwrap();
    let old = data::Weather::from_text(&String::from_utf8_lossy(&bytes)).unwrap();
    same_weather(&old, &new);
    bytes.truncate(3);
    assert!(native::Weather::from_file(&bytes).unwrap().is_none());
}

#[test]
fn conditions_units_and_timing_are_those_of_interface_6() {
    for code in 0..=255u8 {
        let (old_key, old_symbol) = data::condition(code);
        let (new_key, new_symbol) = native::condition(code);
        assert_eq!(old_key, new_key);
        assert_eq!(
            format!("{old_symbol:?}"),
            format!("{new_symbol:?}"),
            "{code}"
        );
    }
    let values = [
        i32::MIN,
        -5000,
        -355,
        -35,
        -5,
        -4,
        0,
        4,
        5,
        6,
        35,
        355,
        1000,
        4000,
        i32::MAX,
    ];
    for (old, new) in [
        (Temperature::Celsius, native::Temperature::Celsius),
        (Temperature::Fahrenheit, native::Temperature::Fahrenheit),
    ] {
        assert_eq!(old.sign(), new.sign());
        for tenths in values {
            assert_eq!(old.degrees(tenths), new.degrees(tenths), "{tenths}");
            for with_unit in [true, false] {
                assert_eq!(
                    native::degrees_text(new, tenths, with_unit).unwrap(),
                    data::degrees_text(old, tenths, with_unit)
                );
            }
        }
    }
    for (old, new) in [
        (Wind::Kmh, native::Wind::Kmh),
        (Wind::Mph, native::Wind::Mph),
        (Wind::Ms, native::Wind::Ms),
        (Wind::Knots, native::Wind::Knots),
    ] {
        assert_eq!(old.key(), new.key());
        for tenths in values {
            assert_eq!(old.speed(tenths), new.speed(tenths), "{tenths}");
        }
    }
    // The units a setting's value names, as `lib.rs` reads them.
    for (i, value) in crate::WINDS.iter().enumerate() {
        let old = match i {
            1 => Wind::Mph,
            2 => Wind::Ms,
            3 => Wind::Knots,
            _ => Wind::Kmh,
        };
        assert_eq!(native::Wind::of(value).key(), old.key());
    }
    assert_eq!(native::Wind::of("other").key(), Wind::Kmh.key());
    assert_eq!(
        native::Temperature::of(crate::TEMPERATURES[1]),
        native::Temperature::Fahrenheit
    );
    assert_eq!(
        native::Temperature::of(crate::TEMPERATURES[0]),
        native::Temperature::Celsius
    );
    assert_eq!(native::Temperature::of("x"), native::Temperature::Celsius);
}

#[test]
fn the_timing_is_that_of_interface_6() {
    for value in crate::EVERY_HOURS {
        let hours: i64 = value.rsplit('.').next().unwrap().parse().unwrap();
        assert_eq!(native::every(value), hours * 3600);
    }
    for (value, seconds) in [
        ("", 3600),
        ("weather.every.0", 3600),
        ("weather.every.99", 86_400),
        ("x", 3600),
    ] {
        assert_eq!(native::every(value), seconds, "{value:?}");
    }
    let ages = [
        None,
        Some(-5),
        Some(0),
        Some(59),
        Some(3599),
        Some(3600),
        Some(10_000),
        Some(40_000),
    ];
    for every in [3600, 7200, 21_600] {
        for age in ages {
            assert_eq!(native::due(age, every), data::due(age, every));
            for least in [0, 60, 900] {
                assert_eq!(
                    native::until_due(age, every, least),
                    data::until_due(age, every, least)
                );
            }
            if let Some(age) = age {
                assert_eq!(native::is_old(age, every), data::is_old(age, every));
            }
        }
    }
}

#[test]
fn places_are_written_as_before() {
    for (name, region, country) in [
        ("Berlin", "Land Berlin", "Germany"),
        ("Berlin", "Berlin", "Germany"),
        ("Berlin", "", "Germany"),
        ("Berlin", "Land Berlin", ""),
        ("Berlin", "", ""),
    ] {
        let old = data::Place {
            name: name.to_string(),
            region: region.to_string(),
            country: country.to_string(),
            latitude: "52.52".to_string(),
            longitude: "13.41".to_string(),
        };
        let new = native::Place {
            name: old.name.clone(),
            region: old.region.clone(),
            country: old.country.clone(),
            latitude: old.latitude.clone(),
            longitude: old.longitude.clone(),
        };
        assert_eq!(new.whereabouts().unwrap(), old.whereabouts());
        assert_eq!(new.coordinates().unwrap(), old.coordinates());
    }
}

#[test]
fn refused_allocations_end_in_no_memory() {
    let weather = data::parse_forecast(FORECAST.as_bytes(), "52.52,13.41", 1_760_000_000).unwrap();
    let text = weather.to_text();
    sweep(|| native::Weather::from_text(&text).map(|w| w.map(|w| w.days.len())));
    sweep(|| {
        native::Weather::from_file(b"weather 1\nfetched 3\ncoordinates 1,2\n\xff\n")
            .map(|w| w.is_some())
    });
    let kept = native::Weather::from_text(&text).unwrap().unwrap();
    sweep(|| kept.to_text().map(|t| t.len()));
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
