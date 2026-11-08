//! What the weather plugin decides without the host: the numbers and dates of
//! Open-Meteo's answers, the places' coordinates, the addresses asked, the
//! weather kept in its small file, the conditions, the units, and when to
//! fetch. The same rules as the interface-6 plugin (`data.rs`, `lib.rs`),
//! with fallible allocation and the text given by the caller instead of read
//! from a storage. The answers' JSON, the fetching, the widgets and the
//! place screen are the native plugin's later (see `PORTING-7.md`).
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::{self, Write};
use paperpaw_sdk::native::Error;

/// Days of the forecast asked for and kept.
pub const DAYS: usize = 8;
/// Places found by name, at most.
pub const PLACES: usize = 10;
/// A place's name is cut at this many bytes (interface 6 kept it whole, up to
/// the parser's 4 KiB).
pub const PLACE_NAME_CAP: usize = 120;
/// So are its region and its country.
pub const PLACE_PART_CAP: usize = 80;
/// The most of the kept weather's file read, as in interface 6 (4 KiB): a
/// larger file counts as none and is written over.
pub const KEPT_CAP: usize = 4 * 1024;

/// A string whose growth may be refused.
struct Out(String);

impl Write for Out {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        self.0.try_reserve(text.len()).map_err(|_| fmt::Error)?;
        self.0.push_str(text);
        Ok(())
    }
}

fn written(f: impl FnOnce(&mut Out) -> fmt::Result) -> Result<String, Error> {
    let mut out = Out(String::new());
    f(&mut out).map_err(|_| Error::NoMemory)?;
    Ok(out.0)
}

fn copy(text: &str) -> Result<String, Error> {
    let mut out = String::new();
    out.try_reserve_exact(text.len())
        .map_err(|_| Error::NoMemory)?;
    out.push_str(text);
    Ok(out)
}

/// The conditions now.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Current {
    /// In tenths of a degree Celsius.
    pub temperature: i32,
    pub feels: Option<i32>,
    pub humidity: Option<u8>,
    /// The WMO weather code.
    pub code: u8,
    /// In tenths of km/h.
    pub wind: Option<i32>,
    pub day: bool,
}

/// One day of the forecast.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Day {
    /// Days since 1970-01-01.
    pub date: i32,
    pub code: u8,
    pub high: i32,
    pub low: i32,
    pub rain: Option<u8>,
    /// Minutes after midnight.
    pub sunrise: Option<u16>,
    pub sunset: Option<u16>,
}

/// The weather of a place: now and the next days.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Weather {
    /// When it was fetched, seconds since 1970 (UTC); 0 if not known.
    pub fetched: i64,
    /// The coordinates it is for, as asked (`52.52,13.41`).
    pub coordinates: String,
    pub current: Option<Current>,
    pub days: Vec<Day>,
}

/// A place found by name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Place {
    pub name: String,
    pub region: String,
    pub country: String,
    /// As sent, shortened to two decimals (about a kilometre).
    pub latitude: String,
    pub longitude: String,
}

impl Place {
    /// `latitude,longitude`, as kept in the settings.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn coordinates(&self) -> Result<String, Error> {
        written(|o| write!(o, "{},{}", self.latitude, self.longitude))
    }

    /// Where it is: "Land Berlin, Germany".
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn whereabouts(&self) -> Result<String, Error> {
        written(|o| {
            let mut first = true;
            let region = (!self.region.is_empty() && self.region != self.name)
                .then_some(self.region.as_str());
            let country = (!self.country.is_empty()).then_some(self.country.as_str());
            for part in [region, country].into_iter().flatten() {
                if !first {
                    o.write_str(", ")?;
                }
                first = false;
                o.write_str(part)?;
            }
            Ok(())
        })
    }
}

/// The latitude and longitude of `coordinates` (`lat,lon`), if both are
/// numbers in range.
pub fn split_coordinates(coordinates: &str) -> Option<(&str, &str)> {
    let (latitude, longitude) = coordinates.split_once(',')?;
    let (latitude, longitude) = (latitude.trim(), longitude.trim());
    let in_range = |text: &str, max: i32| {
        tenths(text).is_some_and(|v| (-max * 10..=max * 10).contains(&v))
            && text
                .chars()
                .all(|c| c.is_ascii_digit() || c == '.' || c == '-')
    };
    (in_range(latitude, 90) && in_range(longitude, 180)).then_some((latitude, longitude))
}

/// A coordinate as sent (`52.52437`) to two decimals (`52.52`).
///
/// # Errors
/// `NoMemory`.
pub fn short_coordinate(text: &str) -> Result<String, Error> {
    let text = text.trim();
    match text.split_once('.') {
        Some((whole, part)) => {
            let end = part.char_indices().nth(2).map_or(part.len(), |(i, _)| i);
            let part = part[..end].trim_end_matches('0');
            if part.is_empty() {
                copy(whole)
            } else {
                written(|o| write!(o, "{whole}.{part}"))
            }
        }
        None => copy(text),
    }
}

/// A number as text (`-3.46`) in tenths (`-35`), rounded; `None` for what is
/// no number (`null`).
pub fn tenths(text: &str) -> Option<i32> {
    let text = text.trim();
    let (negative, digits) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    let (whole, part) = digits.split_once('.').unwrap_or((digits, ""));
    if whole.is_empty() && part.is_empty() {
        return None;
    }
    if !whole
        .chars()
        .chain(part.chars())
        .all(|c| c.is_ascii_digit())
        || whole.len() > 6
    {
        return None;
    }
    let whole: i32 = if whole.is_empty() {
        0
    } else {
        whole.parse().ok()?
    };
    let mut part = part.bytes().map(|b| i32::from(b - b'0'));
    let first = part.next().unwrap_or(0);
    let round = i32::from(part.next().unwrap_or(0) >= 5);
    let value = whole * 10 + first + round;
    Some(if negative { -value } else { value })
}

/// A date (`2026-10-14`, also the start of `2026-10-14T07:23`) as days since
/// 1970-01-01.
pub fn parse_date(text: &str) -> Option<i32> {
    let mut parts = text.get(..10)?.split('-');
    let year: i32 = parts.next()?.parse().ok()?;
    let month: u8 = parts.next()?.parse().ok()?;
    let day: u8 = parts.next()?.parse().ok()?;
    ((1..=12).contains(&month) && (1..=31).contains(&day))
        .then(|| days_from_civil(year, month, day) as i32)
}

/// The time of day of `2026-10-14T07:23` in minutes after midnight.
pub fn parse_minutes(text: &str) -> Option<u16> {
    let (_, time) = text.split_once('T')?;
    let (hour, minute) = time.split_once(':')?;
    let hour: u16 = hour.parse().ok()?;
    let minute: u16 = minute.get(..2)?.parse().ok()?;
    (hour < 24 && minute < 60).then_some(hour * 60 + minute)
}

/// Days since 1970-01-01 of a date.
pub fn days_from_civil(year: i32, month: u8, day: u8) -> i64 {
    let y = i64::from(year) - i64::from(month <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = i64::from(month);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The weekday of a day since 1970, 0 (Sunday) to 6.
pub fn weekday(date: i32) -> u8 {
    // In 64 bits: a date near `i32::MAX` (a bad answer) must not overflow.
    (i64::from(date) + 4).rem_euclid(7) as u8
}

/// `text` for a query: letters, digits and `-_.~` as they are, spaces as `+`,
/// everything else percent-encoded.
fn encode_into(out: &mut Out, text: &str) -> fmt::Result {
    for byte in text.bytes() {
        match byte {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.write_char(char::from(byte))?;
            }
            b' ' => out.write_char('+')?,
            byte => write!(out, "%{byte:02X}")?,
        }
    }
    Ok(())
}

/// The address of the forecast for `coordinates` (`lat,lon`): only the values
/// shown, in Celsius and km/h, the days in the place's time. `Ok(None)` for
/// coordinates that are none.
///
/// # Errors
/// `NoMemory`.
pub fn forecast_url(coordinates: &str) -> Result<Option<String>, Error> {
    let Some((latitude, longitude)) = split_coordinates(coordinates) else {
        return Ok(None);
    };
    written(|o| {
        write!(
            o,
            "https://api.open-meteo.com/v1/forecast?latitude={latitude}&longitude={longitude}\
             &current=temperature_2m,apparent_temperature,relative_humidity_2m,is_day,weather_code,wind_speed_10m\
             &daily=weather_code,temperature_2m_max,temperature_2m_min,precipitation_probability_max,sunrise,sunset\
             &timezone=auto&forecast_days={DAYS}"
        )
    })
    .map(Some)
}

/// The address of a search for places called `name`, their names in
/// `language` where known.
///
/// # Errors
/// `NoMemory`.
pub fn search_url(name: &str, language: &str) -> Result<String, Error> {
    let tag = language.split(['-', '_']).next().unwrap_or("en");
    let letters = tag.chars().filter(char::is_ascii_alphabetic);
    let empty = letters.clone().next().is_none();
    written(|o| {
        o.write_str("https://geocoding-api.open-meteo.com/v1/search?name=")?;
        encode_into(o, name.trim())?;
        write!(o, "&count={PLACES}&language=")?;
        if empty {
            o.write_str("en")?;
        } else {
            for c in letters {
                o.write_char(c.to_ascii_lowercase())?;
            }
        }
        o.write_str("&format=json")
    })
}

/// The first line of the kept weather; a later version of the file starts
/// with another.
const HEADER: &str = "weather 1";

struct Optional<T>(Option<T>);

impl<T: fmt::Display> fmt::Display for Optional<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            Some(v) => v.fmt(f),
            None => f.write_str("-"),
        }
    }
}

impl Weather {
    /// As kept in its file: a line for now and one a day, numbers only.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn to_text(&self) -> Result<String, Error> {
        written(|o| {
            write!(
                o,
                "{HEADER}\nfetched {}\ncoordinates {}\n",
                self.fetched, self.coordinates
            )?;
            if let Some(c) = self.current {
                writeln!(
                    o,
                    "now {} {} {} {} {} {}",
                    c.temperature,
                    Optional(c.feels),
                    Optional(c.humidity),
                    c.code,
                    Optional(c.wind),
                    u8::from(c.day)
                )?;
            }
            for d in &self.days {
                writeln!(
                    o,
                    "day {} {} {} {} {} {} {}",
                    d.date,
                    d.code,
                    d.high,
                    d.low,
                    Optional(d.rain),
                    Optional(d.sunrise),
                    Optional(d.sunset)
                )?;
            }
            Ok(())
        })
    }

    /// The weather kept in `text` ([`Weather::to_text`]); `Ok(None)` if it is
    /// not such a file.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn from_text(text: &str) -> Result<Option<Weather>, Error> {
        let mut lines = text.lines();
        if lines.next().map(str::trim) != Some(HEADER) {
            return Ok(None);
        }
        let mut weather = Weather::default();
        for line in lines {
            let mut words = line.split_whitespace();
            let kind = words.next().unwrap_or("");
            // The words after the kind; no line has more than seven.
            let mut values = [""; 7];
            for (slot, word) in values.iter_mut().zip(words) {
                *slot = word;
            }
            let number = |i: usize| values.get(i).and_then(|v| v.parse::<i32>().ok());
            match kind {
                "fetched" => match values[0].parse().ok() {
                    Some(fetched) => weather.fetched = fetched,
                    None => return Ok(None),
                },
                "coordinates" => {
                    if values[0].is_empty() {
                        return Ok(None);
                    }
                    weather.coordinates = copy(values[0])?;
                }
                "now" => {
                    let Some(temperature) = number(0) else {
                        return Ok(None);
                    };
                    let Some(code) = number(3) else {
                        return Ok(None);
                    };
                    weather.current = Some(Current {
                        temperature,
                        feels: number(1),
                        humidity: number(2).map(|v| v.clamp(0, 100) as u8),
                        code: code.clamp(0, 255) as u8,
                        wind: number(4),
                        day: number(5) != Some(0),
                    });
                }
                "day" if weather.days.len() < DAYS => {
                    let Some((date, code, high, low)) = number(0)
                        .zip(number(1))
                        .zip(number(2).zip(number(3)))
                        .map(|((d, c), (h, l))| (d, c, h, l))
                    else {
                        continue;
                    };
                    weather.days.try_reserve(1).map_err(|_| Error::NoMemory)?;
                    weather.days.push(Day {
                        date,
                        code: code.clamp(0, 255) as u8,
                        high,
                        low,
                        rain: number(4).map(|v| v.clamp(0, 100) as u8),
                        sunrise: number(5).map(|v| v.clamp(0, 1439) as u16),
                        sunset: number(6).map(|v| v.clamp(0, 1439) as u16),
                    });
                }
                _ => {}
            }
        }
        Ok(Some(weather))
    }

    /// The weather kept in the file as it was read: `None` if it is larger
    /// than [`KEPT_CAP`] or not such a file (a bad one is written over, as in
    /// interface 6); invalid UTF-8 is replaced as `String::from_utf8_lossy`
    /// does.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn from_file(bytes: &[u8]) -> Result<Option<Weather>, Error> {
        if bytes.len() > KEPT_CAP {
            return Ok(None);
        }
        let size = bytes
            .utf8_chunks()
            .map(|c| c.valid().len() + 3 * usize::from(!c.invalid().is_empty()))
            .sum();
        let mut text = String::new();
        text.try_reserve_exact(size).map_err(|_| Error::NoMemory)?;
        for chunk in bytes.utf8_chunks() {
            text.push_str(chunk.valid());
            if !chunk.invalid().is_empty() {
                text.push('\u{fffd}');
            }
        }
        Self::from_text(&text)
    }

    /// The days from `today` on (all if today is not known): a forecast kept a
    /// while ago still shows what is ahead.
    pub fn days_from(&self, today: Option<i32>) -> &[Day] {
        let Some(today) = today else {
            return &self.days;
        };
        let start = self
            .days
            .iter()
            .position(|d| d.date >= today)
            .unwrap_or(self.days.len());
        &self.days[start..]
    }
}

/// What a symbol shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Symbol {
    Clear,
    FewClouds,
    PartlyCloudy,
    Cloudy,
    Fog,
    Drizzle,
    Rain,
    HeavyRain,
    Sleet,
    Snow,
    Showers,
    SnowShowers,
    Thunder,
    Hail,
}

/// The words and the symbol of a WMO weather code.
pub fn condition(code: u8) -> (&'static str, Symbol) {
    match code {
        0 => ("weather.condition.clear", Symbol::Clear),
        1 => ("weather.condition.mainly-clear", Symbol::FewClouds),
        2 => ("weather.condition.partly-cloudy", Symbol::PartlyCloudy),
        3 => ("weather.condition.overcast", Symbol::Cloudy),
        45 | 48 => ("weather.condition.fog", Symbol::Fog),
        51 | 53 | 55 => ("weather.condition.drizzle", Symbol::Drizzle),
        56 | 57 => ("weather.condition.freezing-drizzle", Symbol::Sleet),
        61 => ("weather.condition.light-rain", Symbol::Rain),
        63 => ("weather.condition.rain", Symbol::Rain),
        65 => ("weather.condition.heavy-rain", Symbol::HeavyRain),
        66 | 67 => ("weather.condition.freezing-rain", Symbol::Sleet),
        71 => ("weather.condition.light-snow", Symbol::Snow),
        73 | 77 => ("weather.condition.snow", Symbol::Snow),
        75 => ("weather.condition.heavy-snow", Symbol::Snow),
        80 | 81 => ("weather.condition.showers", Symbol::Showers),
        82 => ("weather.condition.heavy-showers", Symbol::Showers),
        85 | 86 => ("weather.condition.snow-showers", Symbol::SnowShowers),
        95 => ("weather.condition.thunder", Symbol::Thunder),
        96 | 99 => ("weather.condition.hail", Symbol::Hail),
        _ => ("weather.condition.unknown", Symbol::Cloudy),
    }
}

/// The unit of temperatures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Temperature {
    Celsius,
    Fahrenheit,
}

impl Temperature {
    /// The unit a setting value names (Celsius for any other).
    pub fn of(value: &str) -> Self {
        if value == "weather.unit.fahrenheit" {
            Self::Fahrenheit
        } else {
            Self::Celsius
        }
    }

    /// `tenths` of a degree Celsius, in whole degrees of this unit.
    pub fn degrees(self, tenths: i32) -> i32 {
        let tenths = i64::from(tenths);
        let tenths = match self {
            Temperature::Celsius => tenths,
            Temperature::Fahrenheit => tenths * 9 / 5 + 320,
        };
        round_tenths(tenths)
    }

    pub fn sign(self) -> &'static str {
        match self {
            Temperature::Celsius => "\u{b0}C",
            Temperature::Fahrenheit => "\u{b0}F",
        }
    }
}

/// The unit of wind speeds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wind {
    Kmh,
    Mph,
    Ms,
    Knots,
}

impl Wind {
    /// The unit a setting value names (km/h for any other).
    pub fn of(value: &str) -> Self {
        match value {
            "weather.unit.mph" => Self::Mph,
            "weather.unit.ms" => Self::Ms,
            "weather.unit.knots" => Self::Knots,
            _ => Self::Kmh,
        }
    }

    /// `tenths` of km/h, in whole units of this one.
    pub fn speed(self, tenths: i32) -> i32 {
        let tenths = i64::from(tenths);
        let tenths = match self {
            Wind::Kmh => tenths,
            Wind::Mph => tenths * 1000 / 1609,
            Wind::Ms => tenths * 10 / 36,
            Wind::Knots => tenths * 1000 / 1852,
        };
        round_tenths(tenths)
    }

    pub fn key(self) -> &'static str {
        match self {
            Wind::Kmh => "weather.speed.kmh",
            Wind::Mph => "weather.speed.mph",
            Wind::Ms => "weather.speed.ms",
            Wind::Knots => "weather.speed.knots",
        }
    }
}

fn round_tenths(tenths: i64) -> i32 {
    let whole = if tenths >= 0 {
        (tenths + 5) / 10
    } else {
        -((-tenths + 5) / 10)
    };
    whole.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

/// A temperature as text: `-3°C`, or `-3°` without the unit.
///
/// # Errors
/// `NoMemory`.
pub fn degrees_text(unit: Temperature, tenths: i32, with_unit: bool) -> Result<String, Error> {
    let degrees = unit.degrees(tenths);
    written(|o| {
        if with_unit {
            write!(o, "{degrees}{}", unit.sign())
        } else {
            write!(o, "{degrees}\u{b0}")
        }
    })
}

/// Seconds between fetches at most, by the setting's value
/// (`weather.every.3`): one to 24 hours.
pub fn every(value: &str) -> i64 {
    value
        .rsplit('.')
        .next()
        .and_then(|h| h.parse::<i64>().ok())
        .unwrap_or(1)
        .clamp(1, 24)
        * 3600
}

/// Whether the weather is due: never fetched, as old as `every`, or from the
/// future.
pub fn due(age: Option<i64>, every: i64) -> bool {
    age.is_none_or(|age| age >= every || age < 0)
}

/// Seconds until the weather is due, at least `least`.
pub fn until_due(age: Option<i64>, every: i64, least: i64) -> i64 {
    match age {
        Some(age) if (0..every).contains(&age) => (every - age).max(least),
        _ => least,
    }
}

/// Whether the weather is old enough for its age to be shown.
pub fn is_old(age: i64, every: i64) -> bool {
    age >= (3 * 3600).max(2 * every)
}
