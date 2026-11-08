//! What the plugin reads of Open-Meteo's answers, as they stream: the
//! forecast ([`Forecast`]: the conditions now and the days ahead) and the
//! places found by a search ([`Places`]). The same rules as the interface-6
//! readers (`parse_forecast` and `parse_places` in `data.rs`), with fallible
//! allocation: the JSON is read by `paperpaw_sdk::json::stream::Parser`'s
//! `try_feed`, what the plugin keeps of it is allocated with `try_reserve`,
//! and a refused allocation ends the reading in [`Error::NoMemory`] with
//! nothing returned in part. After an error the reading is over: every later
//! `feed` says the same. As in interface 6 the parser is never finished: a
//! body cut inside a number (`12` of `12.5`) gives no value for it, not a
//! wrong one. A place's name, region and country are cut at
//! [`PLACE_NAME_CAP`] and [`PLACE_PART_CAP`] bytes (at a character's end).
//!
//! The caller gives the answer's body in pieces (`feed`), then asks for the
//! result (`finish`); [`Forecast::read`] and [`Places::read`] do both for a
//! body already in memory. Interface 6 took the body whole, as the host
//! bounds it; no cap of the plugin's applies.
use crate::native_weather::{
    Current, DAYS, Day, PLACE_NAME_CAP, PLACE_PART_CAP, PLACES, Place, Weather, parse_date,
    parse_minutes, short_coordinate, split_coordinates, tenths,
};
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::json::stream::{Event, Parser};
use paperpaw_sdk::native::Error;

/// Pieces a body is fed in by `read` (as a reply's body is taken).
const PIECE: usize = 2048;

/// What a reading takes of the parser's events.
trait Sink: core::fmt::Debug {
    fn event(&mut self, event: Event<'_>);
    /// Why the reading cannot go on, if it cannot.
    fn failed(&self) -> Option<Error>;
}

/// A parser and what takes its events.
#[derive(Debug)]
struct Feed<S: Sink> {
    parser: Parser,
    sink: S,
    /// The reading is over: memory was refused.
    spent: Option<Error>,
}

impl<S: Sink> Feed<S> {
    fn new(sink: S) -> Self {
        Self {
            parser: Parser::default(),
            sink,
            spent: None,
        }
    }

    fn feed(&mut self, piece: &[u8]) -> Result<(), Error> {
        self.check()?;
        let Self { parser, sink, .. } = self;
        if parser.try_feed(piece, &mut |e| sink.event(e)).is_err() {
            self.spent = Some(Error::NoMemory);
        }
        self.check()
    }

    /// The reading is over. The parser is not finished (see the top).
    fn finish(self) -> Result<S, Error> {
        self.check()?;
        Ok(self.sink)
    }

    fn check(&self) -> Result<(), Error> {
        if let Some(error) = self.spent {
            return Err(error);
        }
        self.sink.failed().map_or(Ok(()), Err)
    }
}

fn copy(text: &str) -> Result<String, Error> {
    let mut out = String::new();
    out.try_reserve_exact(text.len())
        .map_err(|_| Error::NoMemory)?;
    out.push_str(text);
    Ok(out)
}

/// `text` cut to at most `max` bytes, at a character's end, kept fallibly.
fn copy_cut(text: &str, max: usize) -> Result<String, Error> {
    let mut end = text.len().min(max);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    copy(&text[..end])
}

fn percent(text: &str) -> Option<u8> {
    tenths(text).map(|v| ((v + 5) / 10).clamp(0, 100) as u8)
}

/// Adds `value` to a column, each cut on its own at twice the days kept, so
/// more days than asked for leave the first ones.
fn add<T>(column: &mut Vec<T>, value: T) -> Result<(), Error> {
    if column.len() < DAYS * 2 {
        column.try_reserve(1).map_err(|_| Error::NoMemory)?;
        column.push(value);
    }
    Ok(())
}

/// What a forecast's arrays hold, gathered before they are put together: the
/// columns stay in step, a day that could not be read is left out.
#[derive(Debug, Default)]
struct Columns {
    dates: Vec<Option<i32>>,
    codes: Vec<u8>,
    highs: Vec<Option<i32>>,
    lows: Vec<Option<i32>>,
    rain: Vec<Option<u8>>,
    sunrise: Vec<Option<u16>>,
    sunset: Vec<Option<u16>>,
}

#[derive(Debug)]
struct ForecastSink {
    temperature: Option<i32>,
    feels: Option<i32>,
    humidity: Option<u8>,
    code: Option<u8>,
    wind: Option<i32>,
    day: bool,
    columns: Columns,
    failed: Option<Error>,
}

impl ForecastSink {
    fn take(&mut self, path: &[String], text: &str) -> Result<(), Error> {
        match path {
            [section, key] if section == "current" => match key.as_str() {
                "temperature_2m" => self.temperature = tenths(text),
                "apparent_temperature" => self.feels = tenths(text),
                "relative_humidity_2m" => self.humidity = percent(text),
                "weather_code" => self.code = tenths(text).map(|c| (c / 10) as u8),
                "wind_speed_10m" => self.wind = tenths(text),
                "is_day" => self.day = text.trim() != "0",
                _ => {}
            },
            [section, key, item] if section == "daily" && item == "[]" => {
                let columns = &mut self.columns;
                match key.as_str() {
                    "time" => add(&mut columns.dates, parse_date(text))?,
                    "weather_code" => add(
                        &mut columns.codes,
                        tenths(text).map_or(255, |c| (c / 10) as u8),
                    )?,
                    "temperature_2m_max" => add(&mut columns.highs, tenths(text))?,
                    "temperature_2m_min" => add(&mut columns.lows, tenths(text))?,
                    "precipitation_probability_max" => add(&mut columns.rain, percent(text))?,
                    "sunrise" => add(&mut columns.sunrise, parse_minutes(text))?,
                    "sunset" => add(&mut columns.sunset, parse_minutes(text))?,
                    _ => {}
                }
            }
            _ => {}
        }
        Ok(())
    }
}

impl Sink for ForecastSink {
    fn event(&mut self, event: Event<'_>) {
        if self.failed.is_none()
            && let Event::Value(path, text) = event
            && let Err(error) = self.take(path, text)
        {
            self.failed = Some(error);
        }
    }

    fn failed(&self) -> Option<Error> {
        self.failed
    }
}

/// A reading of the forecast Open-Meteo sent.
#[derive(Debug)]
pub struct Forecast(Feed<ForecastSink>);

impl Default for Forecast {
    fn default() -> Self {
        Self::new()
    }
}

impl Forecast {
    pub fn new() -> Self {
        Self(Feed::new(ForecastSink {
            temperature: None,
            feels: None,
            humidity: None,
            code: None,
            wind: None,
            day: true,
            columns: Columns::default(),
            failed: None,
        }))
    }

    /// The next piece of the body.
    ///
    /// # Errors
    /// `NoMemory`; the reading is over then.
    pub fn feed(&mut self, piece: &[u8]) -> Result<(), Error> {
        self.0.feed(piece)
    }

    /// The weather in the answer for `coordinates`, fetched at `fetched`
    /// (seconds since 1970); `None` if it holds none.
    ///
    /// # Errors
    /// `NoMemory`, or the error of an earlier `feed`.
    pub fn finish(self, coordinates: &str, fetched: i64) -> Result<Option<Weather>, Error> {
        let ForecastSink {
            temperature,
            feels,
            humidity,
            code,
            wind,
            day,
            columns,
            ..
        } = self.0.finish()?;
        let current = match (temperature, code) {
            (Some(temperature), Some(code)) => Some(Current {
                temperature,
                feels,
                humidity,
                code,
                wind,
                day,
            }),
            _ => None,
        };
        let mut days: Vec<Day> = Vec::new();
        for (i, &date) in columns.dates.iter().enumerate() {
            if days.len() >= DAYS {
                break;
            }
            let day = (|| {
                Some(Day {
                    date: date?,
                    code: *columns.codes.get(i)?,
                    high: (*columns.highs.get(i)?)?,
                    low: (*columns.lows.get(i)?)?,
                    rain: columns.rain.get(i).copied().flatten(),
                    sunrise: columns.sunrise.get(i).copied().flatten(),
                    sunset: columns.sunset.get(i).copied().flatten(),
                })
            })();
            if let Some(day) = day {
                days.try_reserve(1).map_err(|_| Error::NoMemory)?;
                days.push(day);
            }
        }
        if current.is_none() && days.is_empty() {
            return Ok(None);
        }
        Ok(Some(Weather {
            fetched,
            coordinates: copy(coordinates)?,
            current,
            days,
        }))
    }

    /// Reads `body`, the whole answer, in pieces.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn read(body: &[u8], coordinates: &str, fetched: i64) -> Result<Option<Weather>, Error> {
        let mut reading = Self::new();
        for piece in body.chunks(PIECE) {
            reading.feed(piece)?;
        }
        reading.finish(coordinates, fetched)
    }
}

#[derive(Debug, Default)]
struct PlacesSink {
    places: Vec<Place>,
    place: Place,
    failed: Option<Error>,
}

impl PlacesSink {
    fn take(&mut self, event: Event<'_>) -> Result<(), Error> {
        match event {
            Event::Value(path, text) if path.len() == 3 && path[0] == "results" => {
                let text = text.trim();
                match path[2].as_str() {
                    "name" => self.place.name = copy_cut(text, PLACE_NAME_CAP)?,
                    "admin1" => self.place.region = copy_cut(text, PLACE_PART_CAP)?,
                    "country" => self.place.country = copy_cut(text, PLACE_PART_CAP)?,
                    "latitude" => self.place.latitude = short_coordinate(text)?,
                    "longitude" => self.place.longitude = short_coordinate(text)?,
                    _ => {}
                }
            }
            Event::Close(path) if path.len() == 2 && path[0] == "results" => {
                let done = core::mem::take(&mut self.place);
                if !done.name.is_empty()
                    && split_coordinates(&done.coordinates()?).is_some()
                    && self.places.len() < PLACES
                {
                    self.places.try_reserve(1).map_err(|_| Error::NoMemory)?;
                    self.places.push(done);
                }
            }
            _ => {}
        }
        Ok(())
    }
}

impl Sink for PlacesSink {
    fn event(&mut self, event: Event<'_>) {
        if self.failed.is_none()
            && let Err(error) = self.take(event)
        {
            self.failed = Some(error);
        }
    }

    fn failed(&self) -> Option<Error> {
        self.failed
    }
}

/// A reading of the places Open-Meteo found.
#[derive(Debug)]
pub struct Places(Feed<PlacesSink>);

impl Default for Places {
    fn default() -> Self {
        Self::new()
    }
}

impl Places {
    pub fn new() -> Self {
        Self(Feed::new(PlacesSink::default()))
    }

    /// The next piece of the body.
    ///
    /// # Errors
    /// `NoMemory`; the reading is over then.
    pub fn feed(&mut self, piece: &[u8]) -> Result<(), Error> {
        self.0.feed(piece)
    }

    /// The places found, at most [`PLACES`], those with a name and
    /// coordinates in range.
    ///
    /// # Errors
    /// `NoMemory`, or the error of an earlier `feed`.
    pub fn finish(self) -> Result<Vec<Place>, Error> {
        Ok(self.0.finish()?.places)
    }

    /// Reads `body`, the whole answer, in pieces.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn read(body: &[u8]) -> Result<Vec<Place>, Error> {
        let mut reading = Self::new();
        for piece in body.chunks(PIECE) {
            reading.feed(piece)?;
        }
        reading.finish()
    }
}
