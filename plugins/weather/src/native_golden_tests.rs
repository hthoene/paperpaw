//! The decisions and readers of the plugin (`native_weather`, `native_answers`)
//! on the inputs interface 6 was compared on, as a transcript kept in
//! `testdata/model.v1`: the numbers, dates and coordinates, the addresses, the
//! kept weather, the conditions, the units and the timing, the forecast and the
//! places read from recorded and odd answers at any piece size and cut at every
//! length. The transcript was written while the interface-6 code was still
//! there and equal to it on every line; a change here is a change of what the
//! widgets show, so it is reviewed (`PAPERPAW_BLESS=1 cargo test` writes the
//! file anew).
use crate::native_answers::{Forecast, Places};
use crate::native_weather as native;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use core::fmt::Write;

const FORECAST: &str = include_str!("../testdata/forecast.json");
const SEARCH: &str = include_str!("../testdata/search.json");

fn weather_line(weather: &native::Weather) -> String {
    let current = weather
        .current
        .map(|c| (c.temperature, c.feels, c.humidity, c.code, c.wind, c.day));
    let days: Vec<_> = weather
        .days
        .iter()
        .map(|d| (d.date, d.code, d.high, d.low, d.rain, d.sunrise, d.sunset))
        .collect();
    format!(
        "{} {:?} {current:?} {days:?}",
        weather.fetched, weather.coordinates
    )
}

#[allow(clippy::too_many_lines, reason = "a transcript, read top to bottom")]
fn model(out: &mut String) {
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
        "\u{663}",
    ] {
        let _ = writeln!(out, "tenths {text:?} {:?}", native::tenths(text));
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
        "2026-10-14T24:00",
        "T9:5x",
        "T07:61",
        "x",
    ] {
        let _ = writeln!(
            out,
            "date {text:?} {:?} {:?}",
            native::parse_date(text),
            native::parse_minutes(text)
        );
    }
    for (y, m, d) in [
        (1970, 1, 1),
        (2026, 10, 14),
        (2000, 2, 29),
        (1900, 3, 1),
        (-1, 12, 31),
    ] {
        let _ = writeln!(out, "days {y} {m} {d} {}", native::days_from_civil(y, m, d));
    }
    for date in [-8, -1, 0, 1, 7, 20_000, 20_375] {
        let _ = writeln!(out, "weekday {date} {}", native::weekday(date));
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
        let _ = writeln!(out, "split {text:?} {:?}", native::split_coordinates(text));
    }
    for text in [
        "52.52437", "52.5", "52.50", "52.00", "52", " 13.405 ", "-0.001", "5.", ".5",
    ] {
        let _ = writeln!(out, "short {text:?} {:?}", native::short_coordinate(text));
    }
    for coordinates in ["52.52,13.41", "x", "", "91,0", " 1 , 2 "] {
        let _ = writeln!(
            out,
            "forecast-url {coordinates:?} {:?}",
            native::forecast_url(coordinates)
        );
    }
    for name in [
        "Berlin",
        " Sankt Gallen ",
        "Z\u{fc}rich",
        "a&b=c",
        "",
        "\u{65e5}\u{672c}",
        "~._-",
        "100%",
    ] {
        for language in [
            "de", "en-GB", "pt_BR", "", "1", "  ", "-de", "DE", "zh-Hans",
        ] {
            let _ = writeln!(
                out,
                "search-url {name:?} {language:?} {:?}",
                native::search_url(name, language)
            );
        }
    }
}

fn kept(out: &mut String) {
    let weather = Forecast::read(FORECAST.as_bytes(), "52.52,13.41", 1_760_000_000)
        .unwrap()
        .unwrap();
    let text = weather.to_text().unwrap();
    let _ = writeln!(out, "kept {text:?}");
    let many: String = (0..12).map(|i| format!("day {i} 1 2 3 - - -\n")).collect();
    let texts: Vec<String> = vec![
        text,
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
        match native::Weather::from_text(text).unwrap() {
            Some(weather) => {
                let _ = writeln!(out, "read {text:.40?} {}", weather_line(&weather));
                for today in [None, Some(0), Some(20_000)] {
                    let _ = writeln!(out, "  from {today:?} {}", weather.days_from(today).len());
                }
                let _ = writeln!(out, "  again {:?}", weather.to_text().unwrap());
            }
            None => {
                let _ = writeln!(out, "read {text:.40?} none");
            }
        }
    }
}

fn units(out: &mut String) {
    for code in 0..=255u8 {
        let (key, symbol) = native::condition(code);
        let _ = writeln!(out, "condition {code} {key} {symbol:?}");
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
    for unit in [
        native::Temperature::Celsius,
        native::Temperature::Fahrenheit,
    ] {
        for tenths in values {
            let _ = writeln!(
                out,
                "degrees {unit:?} {tenths} {} {:?} {:?}",
                unit.degrees(tenths),
                native::degrees_text(unit, tenths, true),
                native::degrees_text(unit, tenths, false)
            );
        }
    }
    for unit in [
        native::Wind::Kmh,
        native::Wind::Mph,
        native::Wind::Ms,
        native::Wind::Knots,
    ] {
        for tenths in values {
            let _ = writeln!(out, "wind {} {tenths} {}", unit.key(), unit.speed(tenths));
        }
    }
    for value in [
        "weather.every.1",
        "",
        "weather.every.0",
        "weather.every.99",
        "x",
    ] {
        let _ = writeln!(out, "every {value:?} {}", native::every(value));
    }
    for every in [3600, 7200, 21_600] {
        for age in [
            None,
            Some(-5),
            Some(0),
            Some(59),
            Some(3599),
            Some(3600),
            Some(10_000),
            Some(40_000),
        ] {
            let _ = writeln!(
                out,
                "timing {every} {age:?} {} {} {} {} {:?}",
                native::due(age, every),
                native::until_due(age, every, 0),
                native::until_due(age, every, 60),
                native::until_due(age, every, 900),
                age.map(|age| native::is_old(age, every))
            );
        }
    }
    for (name, region, country) in [
        ("Berlin", "Land Berlin", "Germany"),
        ("Berlin", "Berlin", "Germany"),
        ("Berlin", "", "Germany"),
        ("Berlin", "Land Berlin", ""),
        ("Berlin", "", ""),
    ] {
        let place = native::Place {
            name: name.to_string(),
            region: region.to_string(),
            country: country.to_string(),
            latitude: "52.52".to_string(),
            longitude: "13.41".to_string(),
        };
        let _ = writeln!(
            out,
            "place {name:?} {region:?} {country:?} {:?} {:?}",
            place.whereabouts(),
            place.coordinates()
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

fn answers(out: &mut String) {
    for body in forecasts() {
        let whole = Forecast::read(body.as_bytes(), "52.52,13.41", 1_760_000_000).unwrap();
        let line = whole.as_ref().map(weather_line);
        let _ = writeln!(out, "forecast {body:.60?} {line:?}");
        // Whatever the size of the pieces the body comes in.
        for size in [1, 3, 64, 2048] {
            let mut reading = Forecast::new();
            for piece in body.as_bytes().chunks(size) {
                reading.feed(piece).unwrap();
            }
            let again = reading
                .finish("52.52,13.41", 1_760_000_000)
                .unwrap()
                .as_ref()
                .map(weather_line);
            assert_eq!(line, again, "{size} {body:.60}");
        }
    }
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
        let places = Places::read(body.as_bytes()).unwrap();
        let line: Vec<_> = places
            .iter()
            .map(|p| (&p.name, &p.region, &p.country, &p.latitude, &p.longitude))
            .collect();
        let _ = writeln!(out, "places {body:.60?} {line:?}");
        for size in [1, 7, 2048] {
            let mut reading = Places::new();
            for piece in body.as_bytes().chunks(size) {
                reading.feed(piece).unwrap();
            }
            assert_eq!(places.len(), reading.finish().unwrap().len(), "{size}");
        }
    }
    // A body cut at every length: which ones give a weather, how many places.
    let cut = |body: &str| -> (String, String) {
        let (mut weathers, mut counts) = (String::new(), String::new());
        for end in 0..body.len() {
            if !body.is_char_boundary(end) {
                continue;
            }
            let part = &body.as_bytes()[..end];
            let weather = Forecast::read(part, "1,2", 5).unwrap();
            weathers.push(match weather {
                Some(w) if w.current.is_some() => 'c',
                Some(_) => 'd',
                None => '-',
            });
            let n = Places::read(part).unwrap().len();
            counts.push(char::from_digit(u32::try_from(n).unwrap_or(0), 16).unwrap_or('?'));
        }
        (weathers, counts)
    };
    let (weathers, _) = cut(FORECAST);
    let _ = writeln!(out, "forecast-cuts {weathers}");
    let (_, counts) = cut(SEARCH);
    let _ = writeln!(out, "search-cuts {counts}");
    // Names cut at a character's end, not inside one.
    let long = "\u{e4}".repeat(2000);
    let body = format!(
        r#"{{"results":[{{"name":"{long}","admin1":"{long}","country":"{long}","latitude":1,"longitude":2}}]}}"#
    );
    for place in Places::read(body.as_bytes()).unwrap() {
        let _ = writeln!(
            out,
            "long {} {} {}",
            place.name.len(),
            place.region.len(),
            place.country.len()
        );
        assert!(place.name.chars().all(|c| c == '\u{e4}'));
    }
}

fn transcript() -> String {
    let mut out = String::new();
    model(&mut out);
    kept(&mut out);
    units(&mut out);
    answers(&mut out);
    out
}

#[test]
fn the_model_is_the_reviewed_transcript() {
    let now = transcript();
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/testdata/model.v1");
    if std::env::var_os("PAPERPAW_BLESS").is_some() {
        std::fs::write(path, &now).unwrap();
    }
    let kept = std::fs::read_to_string(path).unwrap();
    if now != kept {
        let first = now
            .lines()
            .zip(kept.lines())
            .position(|(a, b)| a != b)
            .unwrap_or(0);
        panic!(
            "the transcript differs from testdata/model.v1 at line {}:\n now  {:?}\n kept {:?}",
            first + 1,
            now.lines().nth(first),
            kept.lines().nth(first)
        );
    }
}
