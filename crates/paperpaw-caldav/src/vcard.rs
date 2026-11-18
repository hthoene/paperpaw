//! Contacts (`VCARD`) as far as birthdays and anniversaries need them: a
//! name, the day born and the day married, read as a text streams by.
use alloc::string::String;

use crate::{
    Error,
    ical::{Property, UpperName, unescape_into},
};

/// A year, month and day; a year of 0 is not known.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Day {
    pub year: i32,
    pub month: u8,
    pub day: u8,
}

impl Day {
    /// Reads `19850412`, `1985-04-12`, `--0412`, `--04-12` or such a date
    /// followed by a time. A year of 1604 stands for none (what some address
    /// books write for a year that is not known).
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        let date = value.split(['T', 't']).next()?;
        let digits: String = date.chars().filter(|c| *c != '-').collect();
        let (year, rest) = if date.starts_with("--") {
            (0, digits.as_str())
        } else {
            let (year, rest) = (digits.get(..4)?, digits.get(4..)?);
            (year.parse::<i32>().ok()?, rest)
        };
        let month: u8 = rest.get(..2)?.parse().ok()?;
        let day: u8 = rest.get(2..4)?.parse().ok()?;
        if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
            return None;
        }
        Some(Self {
            year: if year == 1604 { 0 } else { year },
            month,
            day,
        })
    }
}

/// What is read of a contact.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Card {
    /// Its `UID`, or empty.
    pub uid: String,
    /// The name to show: `FN`, else the parts of `N`.
    pub name: String,
    pub birthday: Option<Day>,
    pub anniversary: Option<Day>,
}

/// Reads the contacts of a text from its content lines.
#[derive(Debug)]
pub struct CardReader {
    inside: bool,
    current: Card,
    /// The name made of `N`, kept in case there is no `FN`.
    fallback: String,
    name_max: usize,
}

impl CardReader {
    /// A reader that keeps `name_max` bytes of a name.
    #[must_use]
    pub const fn new(name_max: usize) -> Self {
        Self {
            inside: false,
            current: Card {
                uid: String::new(),
                name: String::new(),
                birthday: None,
                anniversary: None,
            },
            fallback: String::new(),
            name_max,
        }
    }

    /// Back to the start of a text.
    pub fn reset(&mut self) {
        *self = Self::new(self.name_max);
    }

    /// Takes a content line; gives a contact when the line ends one.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn line(&mut self, line: &[u8]) -> Result<Option<Card>, Error> {
        let Some(property) = Property::parse(line) else {
            return Ok(None);
        };
        let value = property.value.trim();
        if property.is("BEGIN") {
            if value.eq_ignore_ascii_case("VCARD") {
                self.inside = true;
                self.current = Card::default();
                self.fallback.clear();
            }
            return Ok(None);
        }
        if !self.inside {
            return Ok(None);
        }
        if property.is("END") {
            self.inside = false;
            let mut card = core::mem::take(&mut self.current);
            if card.name.is_empty() {
                card.name = core::mem::take(&mut self.fallback);
            }
            return Ok(Some(card));
        }
        // The group a property may be in (`item1.BDAY`) is not its name.
        let name = property.name.rsplit('.').next().unwrap_or("");
        match name.to_ascii_uppercase_buf().as_str() {
            "UID" => {
                self.current.uid.clear();
                unescape_into(&mut self.current.uid, value, 200, true)?;
            }
            "FN" => {
                self.current.name.clear();
                unescape_into(&mut self.current.name, value, self.name_max, true)?;
            }
            "N" => {
                // Family;Given;Additional;Prefix;Suffix.
                let mut parts = value.split(';');
                let (family, given) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
                self.fallback.clear();
                for part in [given, family] {
                    if part.is_empty() {
                        continue;
                    }
                    if !self.fallback.is_empty() {
                        self.fallback.push(' ');
                    }
                    unescape_into(&mut self.fallback, part, self.name_max, true)?;
                }
            }
            "BDAY" => self.current.birthday = Day::parse(value),
            "ANNIVERSARY" | "X-ANNIVERSARY" => self.current.anniversary = Day::parse(value),
            // Some address books say the year of a birthday is not known so.
            "X-APPLE-OMIT-YEAR" => {
                if let Some(day) = self.current.birthday.as_mut() {
                    day.year = 0;
                }
            }
            _ => {}
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "fixtures")]
    use super::*;
    use crate::ical::Lines;
    use alloc::vec::Vec;

    fn read(text: &str) -> Vec<Card> {
        let mut lines = Lines::new();
        let mut reader = CardReader::new(60);
        let mut found = Vec::new();
        let mut visit = |line: &[u8]| -> Result<(), Error> {
            if let Some(card) = reader.line(line)? {
                found.push(card);
            }
            Ok(())
        };
        lines.feed(text.as_bytes(), &mut visit).unwrap();
        lines.finish(&mut visit).unwrap();
        found
    }

    #[test]
    fn days_are_read_in_every_form() {
        let day = |y, m, d| {
            Some(Day {
                year: y,
                month: m,
                day: d,
            })
        };
        assert_eq!(Day::parse("19850412"), day(1985, 4, 12));
        assert_eq!(Day::parse("1985-04-12"), day(1985, 4, 12));
        assert_eq!(Day::parse("--0412"), day(0, 4, 12));
        assert_eq!(Day::parse("--04-12"), day(0, 4, 12));
        assert_eq!(Day::parse("16040412"), day(0, 4, 12));
        assert_eq!(Day::parse("1985-04-12T00:00:00Z"), day(1985, 4, 12));
        assert_eq!(Day::parse("19851312"), None);
        assert_eq!(Day::parse("soon"), None);
    }

    #[test]
    fn contacts_with_a_birthday() {
        let cards = read(
            "BEGIN:VCARD\r\nVERSION:3.0\r\nN:Doe;Jane;;;\r\nitem1.BDAY;VALUE=date:1985-04-12\r\n\
ANNIVERSARY:20100605\r\nUID:u1\r\nEND:VCARD\r\n\
BEGIN:VCARD\r\nFN:Max Mustermann\r\nN:Mustermann;Max\r\nBDAY:--0229\r\nEND:VCARD\r\n\
BEGIN:VCARD\r\nFN:No Day\r\nEND:VCARD\r\n",
        );
        assert_eq!(cards.len(), 3);
        assert_eq!(cards[0].name, "Jane Doe");
        assert_eq!(cards[0].birthday.unwrap().year, 1985);
        assert_eq!(cards[0].anniversary.unwrap().month, 6);
        assert_eq!(cards[1].name, "Max Mustermann");
        assert_eq!(cards[1].birthday.unwrap().year, 0);
        assert!(cards[2].birthday.is_none());
    }
}
