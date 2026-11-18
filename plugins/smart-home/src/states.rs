//! Home Assistant's states, read as they stream: `/api/states` is an array of
//! objects (`entity_id`, `state`, `attributes`), `/api/states/<id>` one of
//! them, a service call answers with the array of those that changed. The
//! reader keeps nothing but the record being read and hands each finished
//! record to the caller, who keeps what it needs (the entities the reader
//! chose, or the ones a search matches), so a list of thousands of entities
//! takes a few hundred bytes of heap and no more.
//!
//! The JSON is read by `paperpaw_sdk::json::stream::Parser`'s `try_feed`: memory
//! refused is [`Error::NoMemory`] and the reading is over then.
use crate::model::{CLASS_MAX, ID_MAX, NAME_MAX, STATE_MAX, UNIT_MAX, clean};
use alloc::string::String;
use paperpaw_sdk::json::stream::{Event, Parser};
use paperpaw_sdk::native::Error;

/// Pieces a file is read in (three card sectors, as the SDK's own reader).
pub const PIECE: usize = 1536;

/// One state as Home Assistant wrote it, each text cut short.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Record {
    pub id: String,
    pub state: String,
    pub name: String,
    pub unit: String,
    pub class: String,
}

/// A reader of states: feed it the body in pieces.
#[derive(Debug, Default)]
pub struct States {
    parser: Parser,
    current: Record,
    spent: Option<Error>,
}

impl States {
    /// A reader with nothing read.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The next piece of the body; each record that is complete is given to
    /// `sink`. A `sink` that fails ends the reading with its error.
    ///
    /// # Errors
    /// `NoMemory`, or what `sink` says; the reading is over then.
    pub fn feed(
        &mut self,
        piece: &[u8],
        sink: &mut dyn FnMut(Record) -> Result<(), Error>,
    ) -> Result<(), Error> {
        if let Some(error) = self.spent {
            return Err(error);
        }
        let Self {
            parser,
            current,
            spent,
        } = self;
        let fed = parser.try_feed(piece, &mut |event| {
            if spent.is_none()
                && let Err(error) = take(current, event, sink)
            {
                *spent = Some(error);
            }
        });
        if fed.is_err() {
            self.spent = Some(Error::NoMemory);
        }
        self.spent.map_or(Ok(()), Err)
    }
}

/// The path of a value inside a record: `["[]", "attributes", "x"]` and
/// `["attributes", "x"]` both give `["attributes", "x"]`.
fn inside(path: &[String]) -> &[String] {
    match path.first() {
        Some(first) if first == "[]" => path.get(1..).unwrap_or(&[]),
        _ => path,
    }
}

fn take(
    current: &mut Record,
    event: Event<'_>,
    sink: &mut dyn FnMut(Record) -> Result<(), Error>,
) -> Result<(), Error> {
    match event {
        Event::Value(path, text) => match inside(path) {
            [key] if key == "entity_id" => current.id = clean(text, ID_MAX)?,
            [key] if key == "state" => current.state = clean(text, STATE_MAX)?,
            [section, key] if section == "attributes" => match key.as_str() {
                "friendly_name" => current.name = clean(text, NAME_MAX)?,
                "unit_of_measurement" => current.unit = clean(text, UNIT_MAX)?,
                "device_class" => current.class = clean(text, CLASS_MAX)?,
                _ => {}
            },
            _ => {}
        },
        // An object of the record's depth ends (an array item, or the single
        // object itself): the record is whole.
        Event::Close(path) if inside(path).is_empty() && !current.id.is_empty() => {
            sink(core::mem::take(current))?;
        }
        Event::Close(_) | Event::Open(_) => {}
    }
    Ok(())
}

/// What reads a piece of a file: the offset and the buffer to fill; the bytes read.
pub type Reader<'a> = &'a mut dyn FnMut(u64, &mut [u8]) -> Result<usize, Error>;

/// Reads a whole body kept in a file, `read(at, buffer)` giving its bytes (the
/// card's `read_at`), in pieces of [`PIECE`].
///
/// # Errors
/// `NoMemory`, what `read` or `sink` say.
pub fn read_file(
    read: Reader<'_>,
    sink: &mut dyn FnMut(Record) -> Result<(), Error>,
) -> Result<(), Error> {
    let mut states = States::new();
    let mut piece = [0u8; PIECE];
    let mut at = 0u64;
    loop {
        let got = read(at, &mut piece)?;
        if got == 0 {
            return Ok(());
        }
        states.feed(piece.get(..got).ok_or(Error::Invalid)?, sink)?;
        at += got as u64;
    }
}

#[cfg(test)]
pub(crate) mod tests {
    #![allow(clippy::unwrap_used, reason = "test assertions")]
    use super::*;
    use alloc::{format, vec::Vec};

    /// A states list of `n` entities as Home Assistant writes it: nested
    /// attributes, escapes, non-ASCII, a context object after the state.
    pub(crate) fn big_list(n: usize) -> String {
        let mut out = String::from("[");
        for i in 0..n {
            if i > 0 {
                out.push(',');
            }
            let (domain, state, attrs) = match i % 4 {
                0 => (
                    "sensor",
                    format!("{}.{}", 15 + i % 10, i % 100),
                    format!(
                        r#""unit_of_measurement": "°C", "device_class": "temperature", "friendly_name": "Zimmer \"{i}\" Temperatur", "state_class": "measurement", "icon": "mdi:thermometer", "attribution": {{"a": [1, 2, {{"deep": [[[]]]}}]}}"#
                    ),
                ),
                1 => (
                    "light",
                    "on".into(),
                    format!(
                        r#""supported_color_modes": ["brightness", "color_temp"], "friendly_name": "Lamp {i}", "brightness": 180"#
                    ),
                ),
                2 => (
                    "binary_sensor",
                    "off".into(),
                    format!(r#""device_class": "door", "friendly_name": "Tür {i}""#),
                ),
                _ => (
                    "automation",
                    "on".into(),
                    format!(r#""friendly_name": "Rule {i}", "last_triggered": null"#),
                ),
            };
            out.push_str(&format!(
                r#"{{"entity_id": "{domain}.thing_{i}", "state": "{state}", "attributes": {{{attrs}}}, "last_changed": "2026-11-18T01:02:03.000000+00:00", "last_updated": "2026-11-18T01:02:03.000000+00:00", "context": {{"id": "01H", "parent_id": null, "user_id": null}}}}"#
            ));
        }
        out.push(']');
        out
    }

    fn read_all(body: &[u8], piece: usize) -> Vec<Record> {
        let mut records = Vec::new();
        let mut states = States::new();
        for part in body.chunks(piece) {
            states
                .feed(part, &mut |r| {
                    records.push(r);
                    Ok(())
                })
                .unwrap();
        }
        records
    }

    #[test]
    fn a_big_list_streams_in_any_pieces() {
        let body = big_list(1200);
        assert!(body.len() > 400_000);
        let whole = read_all(body.as_bytes(), body.len());
        assert_eq!(whole.len(), 1200);
        for piece in [1, 7, 64, 1536, 3840] {
            // Byte pieces over the whole body are slow: use a slice of it.
            let part = if piece < 64 {
                &body[..20_000]
            } else {
                &body[..]
            };
            let got = read_all(part.as_bytes(), piece);
            let same = read_all(part.as_bytes(), part.len());
            assert_eq!(got, same, "pieces of {piece}");
        }
        let first = &whole[0];
        assert_eq!(first.id, "sensor.thing_0");
        assert_eq!(first.state, "15.0");
        assert_eq!(first.unit, "°C");
        assert_eq!(first.class, "temperature");
        assert_eq!(first.name, "Zimmer \"0\" Temperatur");
        assert_eq!(whole[2].name, "Tür 2");
        assert_eq!(whole[2].class, "door");
        assert_eq!(whole[3].id, "automation.thing_3");
        assert_eq!(whole[3].state, "on");
    }

    #[test]
    fn a_single_object_and_a_service_answer() {
        let one =
            r#"{"entity_id":"light.desk","state":"on","attributes":{"friendly_name":"Desk"}}"#;
        let got = read_all(one.as_bytes(), 5);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].id, "light.desk");
        assert_eq!(got[0].name, "Desk");
        let changed = format!("[{one},{one}]");
        assert_eq!(read_all(changed.as_bytes(), 3).len(), 2);
        assert!(read_all(b"[]", 1).is_empty());
        assert!(read_all(br#"{"message": "401: Unauthorized"}"#, 4).is_empty());
    }

    #[test]
    fn a_sink_that_fails_ends_the_reading() {
        let body = big_list(5);
        let mut states = States::new();
        let mut n = 0;
        let failed = states.feed(body.as_bytes(), &mut |_| {
            n += 1;
            if n == 2 { Err(Error::NoSpace) } else { Ok(()) }
        });
        assert_eq!(failed, Err(Error::NoSpace));
        assert_eq!(states.feed(b"x", &mut |_| Ok(())), Err(Error::NoSpace));
    }

    #[test]
    fn a_file_is_read_in_pieces() {
        let body = big_list(100);
        let bytes = body.as_bytes();
        let mut longest = 0;
        let mut read = |at: u64, out: &mut [u8]| {
            let from = usize::try_from(at).unwrap().min(bytes.len());
            let n = out.len().min(bytes.len() - from);
            longest = longest.max(n);
            out[..n].copy_from_slice(&bytes[from..from + n]);
            Ok(n)
        };
        let mut count = 0;
        read_file(&mut read, &mut |_| {
            count += 1;
            Ok(())
        })
        .unwrap();
        assert_eq!(count, 100);
        assert!(longest <= PIECE);
    }
}
