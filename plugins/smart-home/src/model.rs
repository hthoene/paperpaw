//! What the plugin keeps of the home and decides without a host: the entities
//! the reader chose (id, name, room and the last state read), which of them
//! are values, switches or scenes, which service switches them, how a state
//! reads, and the small text file they are kept in (`entities.txt`).
//!
//! Everything a server sends is cut to a short length when it is taken
//! (`clean`), so the file's lines never need escaping and the heap the list
//! takes is bounded: [`MAX_ENTITIES`] entities of a few dozen bytes.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::native::Error;

/// The most entities kept.
pub const MAX_ENTITIES: usize = 24;
/// The kept file, in the plugin's data folder.
pub const FILE: &str = "entities.txt";
/// The most bytes the kept file may have; a larger one counts as none.
pub const FILE_CAP: usize = 8 * 1024;
/// The longest entity id, name, room, state, unit and device class kept.
pub const ID_MAX: usize = 100;
pub const NAME_MAX: usize = 48;
pub const ROOM_MAX: usize = 32;
pub const STATE_MAX: usize = 32;
pub const UNIT_MAX: usize = 12;
pub const CLASS_MAX: usize = 24;

/// What an entity is to the reader.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Kind {
    /// Read only: a sensor or a binary sensor.
    Value,
    /// On and off: a switch, a light, a fan, a cover.
    Switch,
    /// Run once: a scene, a script, a button.
    Scene,
}

/// The part of an entity id before its dot.
#[must_use]
pub fn domain(id: &str) -> &str {
    id.split_once('.').map_or(id, |(domain, _)| domain)
}

/// What `id` is, or `None` for a domain the plugin does not show (locks and
/// alarms are left out on purpose: they are not for a button on a shelf).
#[must_use]
pub fn kind_of(id: &str) -> Option<Kind> {
    match domain(id) {
        "sensor" | "binary_sensor" | "input_number" | "input_text" | "input_select" => {
            Some(Kind::Value)
        }
        "switch" | "light" | "input_boolean" | "fan" | "cover" | "automation" => Some(Kind::Switch),
        "scene" | "script" | "button" | "input_button" => Some(Kind::Scene),
        _ => None,
    }
}

/// The service a tap on `id` calls in its domain (`/api/services/<domain>/<service>`).
#[must_use]
pub fn service_of(id: &str) -> Option<&'static str> {
    match (kind_of(id)?, domain(id)) {
        (Kind::Switch, _) => Some("toggle"),
        (Kind::Scene, "button" | "input_button") => Some("press"),
        (Kind::Scene, _) => Some("turn_on"),
        (Kind::Value, _) => None,
    }
}

/// Whether `id` is the id of an entity (`domain.object_id`, lower case letters,
/// digits and underscores, as Home Assistant makes them) of a kind shown.
#[must_use]
pub fn valid_id(id: &str) -> bool {
    let Some((domain, object)) = id.split_once('.') else {
        return false;
    };
    let plain = |text: &str| {
        !text.is_empty()
            && text
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
    };
    id.len() <= ID_MAX && plain(domain) && plain(object) && kind_of(id).is_some()
}

/// `text` with control characters turned to spaces, trimmed and cut to `max`
/// bytes at a character's end, allocated fallibly.
///
/// # Errors
/// `NoMemory`.
pub fn clean(text: &str, max: usize) -> Result<String, Error> {
    let mut out = String::new();
    out.try_reserve_exact(text.len().min(max))
        .map_err(|_| Error::NoMemory)?;
    for c in text.trim().chars() {
        let c = if c.is_control() { ' ' } else { c };
        if out.len() + c.len_utf8() > max {
            break;
        }
        out.push(c);
    }
    out.truncate(out.trim_end().len());
    Ok(out)
}

/// One entity the reader chose, and what was last read of it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Entity {
    pub id: String,
    pub name: String,
    pub room: String,
    pub state: String,
    pub unit: String,
    pub class: String,
    /// A switch was asked and the answer is awaited (not kept).
    pub pending: bool,
}

/// How a state reads: a number with its unit, a word of the catalog or the
/// state as the server wrote it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reading {
    /// The text as it is (a number with its unit, or a word the server sent).
    Text(String),
    /// A catalog key.
    Word(&'static str),
}

impl Entity {
    /// The entity's kind (a kept entity of an unknown domain reads as a value).
    #[must_use]
    pub fn kind(&self) -> Kind {
        kind_of(&self.id).unwrap_or(Kind::Value)
    }

    /// The name shown: the name, else the id's object part.
    #[must_use]
    pub fn title(&self) -> &str {
        if self.name.is_empty() {
            self.id.split_once('.').map_or(&self.id, |(_, o)| o)
        } else {
            &self.name
        }
    }

    /// Whether the entity is on, open or present (a switch that is on).
    #[must_use]
    pub fn is_on(&self) -> bool {
        matches!(self.state.as_str(), "on" | "open" | "opening")
    }

    /// Whether nothing is known of it (never read, unknown, unavailable).
    #[must_use]
    pub fn is_unknown(&self) -> bool {
        matches!(self.state.as_str(), "" | "unknown" | "unavailable")
    }

    /// How its state reads now.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn reading(&self) -> Result<Reading, Error> {
        if self.pending {
            return Ok(Reading::Word("smart-home.state.pending"));
        }
        if self.kind() == Kind::Scene {
            return Ok(Reading::Word("smart-home.state.scene"));
        }
        let door = matches!(
            self.class.as_str(),
            "door" | "window" | "garage_door" | "opening" | "garage" | "lock"
        );
        let presence = matches!(
            self.class.as_str(),
            "presence" | "occupancy" | "motion" | "moving" | "vibration" | "sound"
        );
        let alarm = matches!(
            self.class.as_str(),
            "smoke" | "gas" | "moisture" | "problem" | "safety" | "tamper" | "carbon_monoxide"
        );
        let word = match (self.state.as_str(), domain(&self.id)) {
            ("", _) => "smart-home.state.unknown",
            ("unknown", _) => "smart-home.state.unknown",
            ("unavailable", _) => "smart-home.state.unavailable",
            ("on", "binary_sensor") if door => "smart-home.state.open",
            ("off", "binary_sensor") if door => "smart-home.state.closed",
            ("on", "binary_sensor") if presence => "smart-home.state.present",
            ("off", "binary_sensor") if presence => "smart-home.state.clear",
            ("on", "binary_sensor") if alarm => "smart-home.state.alarm",
            ("off", "binary_sensor") if alarm => "smart-home.state.clear",
            ("on", _) => "smart-home.state.on",
            ("off", _) => "smart-home.state.off",
            ("open" | "opening", _) => "smart-home.state.open",
            ("closed" | "closing", _) => "smart-home.state.closed",
            _ => return self.number_or_text(),
        };
        Ok(Reading::Word(word))
    }

    fn number_or_text(&self) -> Result<Reading, Error> {
        let number = short_number(&self.state)?;
        let mut text = String::new();
        let shown = number.as_deref().unwrap_or(&self.state);
        text.try_reserve_exact(shown.len() + 1 + self.unit.len())
            .map_err(|_| Error::NoMemory)?;
        text.push_str(shown);
        if number.is_some() && !self.unit.is_empty() {
            // Percent sits against the number, other units apart.
            if self.unit != "%" {
                text.push(' ');
            }
            text.push_str(&self.unit);
        } else if number.is_none() && !self.unit.is_empty() && self.state.len() < 8 {
            text.push(' ');
            text.push_str(&self.unit);
        }
        Ok(Reading::Text(text))
    }
}

/// A number as the server wrote it, cut to two decimals (rounded) and without
/// trailing zeros: `21.5321` is `21.53`, `21.50` is `21.5`. `None` for a text
/// that is no plain decimal number.
///
/// # Errors
/// `NoMemory`.
pub fn short_number(text: &str) -> Result<Option<String>, Error> {
    let body = text.strip_prefix('-').unwrap_or(text);
    let negative = body.len() != text.len();
    let (whole, fraction) = body.split_once('.').unwrap_or((body, ""));
    let digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
    if whole.is_empty() || !digits(whole) || !digits(fraction) || whole.len() > 12 {
        return Ok(None);
    }
    let mut hundredths: i64 = whole.parse().map_err(|_| Error::Invalid)?;
    hundredths *= 100;
    let mut fraction = fraction.bytes().map(|b| i64::from(b - b'0'));
    let tens = fraction.next().unwrap_or(0);
    let ones = fraction.next().unwrap_or(0);
    hundredths += tens * 10 + ones;
    if fraction.next().unwrap_or(0) >= 5 {
        hundredths += 1;
    }
    let mut out = String::new();
    out.try_reserve_exact(24).map_err(|_| Error::NoMemory)?;
    if negative && hundredths != 0 {
        out.push('-');
    }
    push_int(&mut out, hundredths / 100);
    let part = hundredths % 100;
    if part != 0 {
        out.push('.');
        out.push(char::from(b'0' + u8::try_from(part / 10).unwrap_or(0)));
        if part % 10 != 0 {
            out.push(char::from(b'0' + u8::try_from(part % 10).unwrap_or(0)));
        }
    }
    Ok(Some(out))
}

fn push_int(out: &mut String, mut value: i64) {
    let mut digits = [0u8; 20];
    let mut at = digits.len();
    loop {
        at -= 1;
        digits[at] = b'0' + u8::try_from(value % 10).unwrap_or(0);
        value /= 10;
        if value == 0 {
            break;
        }
    }
    for digit in digits.get(at..).unwrap_or(&[]) {
        out.push(char::from(*digit));
    }
}

/// The entities chosen, and when they were last read.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Store {
    pub entities: Vec<Entity>,
    /// When the states were read, in seconds since 1970 (0 where the clock
    /// was not known).
    pub synced: i64,
    /// Whether any state was read in this run or kept in the file.
    pub read: bool,
}

impl Store {
    /// The entity with `id`.
    #[must_use]
    pub fn find(&self, id: &str) -> Option<&Entity> {
        self.entities.iter().find(|e| e.id == id)
    }

    /// The entity with `id`, to change.
    pub fn find_mut(&mut self, id: &str) -> Option<&mut Entity> {
        self.entities.iter_mut().find(|e| e.id == id)
    }

    /// Adds an entity (one that is there is left as it is).
    ///
    /// # Errors
    /// `NoSpace` past [`MAX_ENTITIES`]; `NoMemory`.
    pub fn add(&mut self, entity: Entity) -> Result<(), Error> {
        if self.find(&entity.id).is_some() {
            return Ok(());
        }
        if self.entities.len() >= MAX_ENTITIES {
            return Err(Error::NoSpace);
        }
        self.entities.try_reserve(1).map_err(|_| Error::NoMemory)?;
        self.entities.push(entity);
        Ok(())
    }

    /// Takes the entity with `id` away; whether there was one.
    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.entities.len();
        self.entities.retain(|e| e.id != id);
        self.entities.len() != before
    }

    /// The rooms, in the order of their first entity, each once.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn rooms(&self) -> Result<Vec<&str>, Error> {
        let mut rooms: Vec<&str> = Vec::new();
        for entity in &self.entities {
            if !entity.room.is_empty() && !rooms.contains(&entity.room.as_str()) {
                rooms.try_reserve(1).map_err(|_| Error::NoMemory)?;
                rooms.push(&entity.room);
            }
        }
        Ok(rooms)
    }

    /// The entities of `room` (all of them where it is empty), in order.
    pub fn in_room<'a>(&'a self, room: &'a str) -> impl Iterator<Item = &'a Entity> {
        self.entities
            .iter()
            .filter(move |e| room.is_empty() || e.room.eq_ignore_ascii_case(room))
    }

    /// The first entity of `kind`.
    #[must_use]
    pub fn first_of(&self, kind: Kind) -> Option<&Entity> {
        self.entities.iter().find(|e| e.kind() == kind)
    }

    /// The entity a widget shows: the one its setting names (an id or a name),
    /// else the first of `kind`.
    #[must_use]
    pub fn pick(&self, named: &str, kind: Kind) -> Option<&Entity> {
        let named = named.trim();
        if named.is_empty() {
            return self.first_of(kind);
        }
        self.entities
            .iter()
            .find(|e| e.id == named || e.name.eq_ignore_ascii_case(named))
    }

    /// The room a widget shows: the one its setting names, else the first
    /// room, else every entity (room `""`).
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn room_named(&self, named: &str) -> Result<String, Error> {
        let named = named.trim();
        let rooms = self.rooms()?;
        let found = rooms.iter().find(|r| r.eq_ignore_ascii_case(named));
        let chosen = match (found, named.is_empty()) {
            (Some(room), _) => room,
            (None, true) => rooms.first().unwrap_or(&""),
            // A room that is not there shows nothing (so a typo is seen).
            (None, false) => return clean(named, ROOM_MAX),
        };
        clean(chosen, ROOM_MAX)
    }

    /// The file's text.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn to_text(&self) -> Result<String, Error> {
        let mut out = String::new();
        out.try_reserve_exact(32 + self.entities.len() * 160)
            .map_err(|_| Error::NoMemory)?;
        out.push_str("v1\t");
        push_int(&mut out, self.synced.max(0));
        out.push('\n');
        for e in &self.entities {
            for (i, field) in [&e.id, &e.name, &e.room, &e.state, &e.unit, &e.class]
                .into_iter()
                .enumerate()
            {
                if i > 0 {
                    out.push('\t');
                }
                out.push_str(field);
            }
            out.push('\n');
        }
        Ok(out)
    }

    /// The store the file's text holds; a damaged or foreign file is none.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn from_text(text: &str) -> Result<Option<Self>, Error> {
        let mut lines = text.lines();
        let Some(head) = lines.next() else {
            return Ok(None);
        };
        let Some(synced) = head.strip_prefix("v1\t") else {
            return Ok(None);
        };
        let mut store = Self {
            synced: synced.parse().unwrap_or(0),
            read: true,
            ..Self::default()
        };
        for line in lines {
            let mut fields = line.split('\t');
            let (Some(id), Some(name), Some(room), Some(state), Some(unit), Some(class)) = (
                fields.next(),
                fields.next(),
                fields.next(),
                fields.next(),
                fields.next(),
                fields.next(),
            ) else {
                return Ok(None);
            };
            if !valid_id(id) {
                return Ok(None);
            }
            let entity = Entity {
                id: clean(id, ID_MAX)?,
                name: clean(name, NAME_MAX)?,
                room: clean(room, ROOM_MAX)?,
                state: clean(state, STATE_MAX)?,
                unit: clean(unit, UNIT_MAX)?,
                class: clean(class, CLASS_MAX)?,
                pending: false,
            };
            match store.add(entity) {
                Ok(()) => {}
                Err(Error::NoSpace) => return Ok(None),
                Err(error) => return Err(error),
            }
        }
        Ok(Some(store))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "test assertions")]
    use super::*;

    fn entity(id: &str, state: &str, unit: &str, class: &str) -> Entity {
        Entity {
            id: id.into(),
            state: state.into(),
            unit: unit.into(),
            class: class.into(),
            ..Entity::default()
        }
    }

    #[test]
    fn kinds_and_services() {
        assert_eq!(kind_of("sensor.kitchen_temperature"), Some(Kind::Value));
        assert_eq!(kind_of("light.desk"), Some(Kind::Switch));
        assert_eq!(kind_of("scene.evening"), Some(Kind::Scene));
        assert_eq!(kind_of("lock.front_door"), None);
        assert_eq!(kind_of("alarm_control_panel.home"), None);
        assert_eq!(service_of("light.desk"), Some("toggle"));
        assert_eq!(service_of("scene.evening"), Some("turn_on"));
        assert_eq!(service_of("button.doorbell"), Some("press"));
        assert_eq!(service_of("sensor.x"), None);
    }

    #[test]
    fn ids_are_checked() {
        assert!(valid_id("light.living_room_2"));
        assert!(!valid_id("light"));
        assert!(!valid_id("light."));
        assert!(!valid_id("Light.desk"));
        assert!(!valid_id("light.desk/../x"));
        assert!(!valid_id("lock.front_door"));
        assert!(!valid_id(&alloc::format!("light.{}", "a".repeat(120))));
    }

    #[test]
    fn numbers_are_short() {
        let n = |t: &str| short_number(t).unwrap();
        assert_eq!(n("21.5321").as_deref(), Some("21.53"));
        assert_eq!(n("21.50").as_deref(), Some("21.5"));
        assert_eq!(n("21.999").as_deref(), Some("22"));
        assert_eq!(n("-0.001").as_deref(), Some("0"));
        assert_eq!(n("-3.25").as_deref(), Some("-3.25"));
        assert_eq!(n("0").as_deref(), Some("0"));
        assert_eq!(n("1234567.8").as_deref(), Some("1234567.8"));
        assert_eq!(n("unavailable"), None);
        assert_eq!(n("1e5"), None);
        assert_eq!(n(""), None);
        assert_eq!(n("."), None);
        assert_eq!(n("12.3.4"), None);
        assert_eq!(n("1234567890123456"), None);
    }

    #[test]
    fn states_read_by_their_class() {
        let word = |e: &Entity| e.reading().unwrap();
        assert_eq!(
            word(&entity("binary_sensor.door", "on", "", "door")),
            Reading::Word("smart-home.state.open")
        );
        assert_eq!(
            word(&entity("binary_sensor.hall", "off", "", "motion")),
            Reading::Word("smart-home.state.clear")
        );
        assert_eq!(
            word(&entity("light.desk", "on", "", "")),
            Reading::Word("smart-home.state.on")
        );
        assert_eq!(
            word(&entity("sensor.t", "21.504", "°C", "temperature")),
            Reading::Text("21.5 °C".into())
        );
        assert_eq!(
            word(&entity("sensor.power", "340.2", "W", "power")),
            Reading::Text("340.2 W".into())
        );
        assert_eq!(
            word(&entity("sensor.t", "unavailable", "°C", "")),
            Reading::Word("smart-home.state.unavailable")
        );
        assert_eq!(
            word(&entity("scene.evening", "2026-11-18T10:00:00", "", "")),
            Reading::Word("smart-home.state.scene")
        );
        let mut asked = entity("light.desk", "off", "", "");
        asked.pending = true;
        assert_eq!(word(&asked), Reading::Word("smart-home.state.pending"));
    }

    #[test]
    fn the_file_round_trips_and_a_damaged_one_is_none() {
        let mut store = Store {
            synced: 1_800_000_000,
            ..Store::default()
        };
        let mut a = entity("sensor.kitchen_temperature", "21.5", "°C", "temperature");
        a.name = "Kitchen".into();
        a.room = "Kitchen".into();
        store.add(a).unwrap();
        store.add(entity("light.desk", "on", "", "")).unwrap();
        let text = store.to_text().unwrap();
        let back = Store::from_text(&text).unwrap().unwrap();
        assert_eq!(back.entities, store.entities);
        assert_eq!(back.synced, 1_800_000_000);
        assert_eq!(Store::from_text("v2\t0\n").unwrap(), None);
        assert_eq!(Store::from_text("v1\t0\nlight.desk\tonly\n").unwrap(), None);
        assert_eq!(
            Store::from_text("v1\t0\nlock.door\t\t\t\t\t\n").unwrap(),
            None
        );
    }

    #[test]
    fn the_list_is_bounded_and_text_is_cleaned() {
        let mut store = Store::default();
        for n in 0..MAX_ENTITIES {
            store
                .add(entity(&alloc::format!("light.l{n}"), "on", "", ""))
                .unwrap();
        }
        assert_eq!(
            store.add(entity("light.more", "on", "", "")),
            Err(Error::NoSpace)
        );
        assert_eq!(clean("  a\tb\nc  ", 10).unwrap(), "a b c");
        assert_eq!(clean("ääää", 5).unwrap(), "ää");
    }

    #[test]
    fn widgets_pick_by_name_or_first() {
        let mut store = Store::default();
        let mut a = entity("sensor.t", "1", "", "");
        a.name = "Hall".into();
        a.room = "Hall".into();
        store.add(a).unwrap();
        store.add(entity("light.desk", "on", "", "")).unwrap();
        assert_eq!(store.pick("", Kind::Switch).unwrap().id, "light.desk");
        assert_eq!(store.pick("hall", Kind::Value).unwrap().id, "sensor.t");
        assert_eq!(
            store.pick("light.desk", Kind::Value).unwrap().id,
            "light.desk"
        );
        assert!(store.pick("nope", Kind::Value).is_none());
        assert_eq!(store.room_named("").unwrap(), "Hall");
        assert_eq!(store.room_named("hall").unwrap(), "Hall");
    }
}
