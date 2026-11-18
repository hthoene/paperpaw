//! The entity picker: a search row and the entities of the server that match
//! it, a tick on those already chosen. The list of the server is read from the
//! saved answer a piece at a time ([`Finder`] keeps the [`LIMIT`] first matches
//! in order of their id and counts the rest), so a server with thousands of
//! entities takes a few kilobytes of heap.
//!
//! The view holds no host handle: the plugin does what a row means ([`Act`]).
use crate::link::Problem;
use crate::model::{ID_MAX, NAME_MAX, kind_of};
use crate::say::Say;
use crate::states::Record;
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Handled,
    native::{
        Error, Host,
        callbacks::{DrawFrame, DrawPurpose, Input},
        formatting::Arg,
    },
};
use paperpaw_ui_kit::{Footer, Item, ItemList, ItemStep, mem::push, text};

/// The most matches listed.
pub const LIMIT: usize = 40;

/// One entity of the server that matches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    pub id: String,
    pub name: String,
}

/// A search over the records of the server's list.
#[derive(Debug)]
pub struct Finder {
    query: String,
    pub matches: Vec<Match>,
    /// How many entities matched in all (the list shows [`LIMIT`] at most).
    pub total: usize,
}

impl Finder {
    /// A search for `query` (all entities of a kind shown where it is empty).
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn new(query: &str) -> Result<Self, Error> {
        Ok(Self {
            query: paperpaw_ui_kit::mem::lowercase(query.trim())?,
            matches: Vec::new(),
            total: 0,
        })
    }

    /// Considers one record.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn add(&mut self, record: Record) -> Result<(), Error> {
        if kind_of(&record.id).is_none() || record.id.len() > ID_MAX {
            return Ok(());
        }
        if !self.query.is_empty() {
            let id = paperpaw_ui_kit::mem::lowercase(&record.id)?;
            let name = paperpaw_ui_kit::mem::lowercase(&record.name)?;
            if !id.contains(self.query.as_str()) && !name.contains(self.query.as_str()) {
                return Ok(());
            }
        }
        self.total += 1;
        let at = self
            .matches
            .iter()
            .position(|m| m.id.as_str() > record.id.as_str())
            .unwrap_or(self.matches.len());
        if at >= LIMIT {
            return Ok(());
        }
        if self.matches.len() >= LIMIT {
            self.matches.pop();
        }
        self.matches.try_reserve(1).map_err(|_| Error::NoMemory)?;
        self.matches.insert(
            at,
            Match {
                name: crate::model::clean(&record.name, NAME_MAX)?,
                id: record.id,
            },
        );
        Ok(())
    }
}

/// How the list of the server is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Load {
    /// Being fetched.
    #[default]
    Loading,
    /// Listed.
    Ready,
    /// It failed.
    Failed(Problem),
}

/// What the plugin has to do after an input.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Act {
    Ignored,
    Taken,
    /// Ask for what to search.
    Ask,
    /// The entity with this id was chosen.
    Chosen(String),
    Back,
}

/// The picker's state.
#[derive(Debug, Default)]
pub struct Pick {
    list: ItemList,
    load: Load,
    query: String,
    matches: Vec<Match>,
    total: usize,
    /// The ids chosen (ticked).
    ticked: Vec<String>,
    focus: Option<usize>,
}

impl Pick {
    /// The state of the load.
    pub fn loading(&mut self, load: Load) {
        self.load = load;
        self.focus = Some(0);
    }

    /// The matches of a search for `query`.
    pub fn found(&mut self, query: &str, finder: Finder) -> Result<(), Error> {
        self.query = text::copy(query)?;
        self.matches = finder.matches;
        self.total = finder.total;
        self.load = Load::Ready;
        self.focus = Some(usize::from(!self.matches.is_empty()));
        Ok(())
    }

    /// The query of the last search.
    #[must_use]
    pub fn query(&self) -> &str {
        &self.query
    }

    /// The ids ticked (the entities chosen), to show.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn tick(&mut self, ids: impl Iterator<Item = String>) -> Result<(), Error> {
        self.ticked.clear();
        for id in ids {
            push(&mut self.ticked, id)?;
        }
        Ok(())
    }

    /// The first row's note: how the load or the search is.
    ///
    /// # Errors
    /// `NoMemory`, or what the catalog says.
    pub fn note(&self, say: &mut impl Say) -> Result<String, Error> {
        match self.load {
            Load::Loading => say.copy("smart-home.pick.loading"),
            Load::Failed(problem) => say.copy(problem.key()),
            Load::Ready if self.total == 0 && self.query.is_empty() => {
                say.copy("smart-home.pick.none")
            }
            Load::Ready if self.query.is_empty() => say.format(
                "smart-home.pick.all",
                &[Arg::Count(i64::try_from(self.total).unwrap_or(i64::MAX))],
            ),
            Load::Ready => say.format(
                "smart-home.pick.found",
                &[
                    Arg::Count(i64::try_from(self.total).unwrap_or(i64::MAX)),
                    Arg::Text {
                        name: "query",
                        value: &self.query,
                    },
                ],
            ),
        }
    }

    /// The rows.
    ///
    /// # Errors
    /// `NoMemory`, or what the catalog says.
    pub fn rows(&self, say: &mut impl Say) -> Result<Vec<Item>, Error> {
        let mut items = Vec::new();
        let title = say.copy("smart-home.pick.search")?;
        items
            .try_reserve_exact(1 + self.matches.len())
            .map_err(|_| Error::NoMemory)?;
        items.push(Item::noted(&title, "›", &self.note(say)?)?);
        for m in &self.matches {
            let mark = if self.ticked.contains(&m.id) {
                "✓"
            } else {
                ""
            };
            let name = if m.name.is_empty() {
                m.id.as_str()
            } else {
                m.name.as_str()
            };
            items.push(Item::noted(name, mark, &m.id)?);
        }
        Ok(items)
    }

    /// Draws the screen.
    ///
    /// # Errors
    /// What the host says; `NoMemory`.
    pub fn draw(
        &mut self,
        host: &Host<'_>,
        frame: &mut DrawFrame<'_>,
        say: &mut impl Say,
        title: &str,
    ) -> Result<(), Error> {
        if !matches!(frame.purpose, DrawPurpose::Screen) {
            return Ok(());
        }
        let items = self.rows(say)?;
        self.list.replace(items);
        if let Some(to) = self.focus.take() {
            self.list.set_focus(to);
        }
        self.list.draw_with(host, frame, title, true, Footer::None)
    }

    /// A key or a tap.
    ///
    /// # Errors
    /// What the host says.
    pub fn input(&mut self, host: &Host<'_>, input: &Input<'_>) -> Result<Act, Error> {
        let step = self.list.input(host, input)?;
        Ok(match step {
            ItemStep::Chosen(0) => Act::Ask,
            ItemStep::Chosen(index) => match self.matches.get(index - 1) {
                Some(m) => Act::Chosen(text::copy(&m.id)?),
                None => Act::Taken,
            },
            ItemStep::Header("core.back") => Act::Back,
            ItemStep::Ignored => Act::Ignored,
            _ => Act::Taken,
        })
    }

    /// Whether `act` was taken.
    #[must_use]
    pub const fn handled(act: &Act) -> Handled {
        if matches!(act, Act::Ignored) {
            Handled::No
        } else {
            Handled::Yes
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "test assertions")]
    use super::*;
    use crate::say::tests::Keys;
    use crate::states::{States, tests::big_list};

    fn find(query: &str, body: &str) -> Finder {
        let mut finder = Finder::new(query).unwrap();
        let mut states = States::new();
        for piece in body.as_bytes().chunks(1536) {
            states.feed(piece, &mut |r| finder.add(r)).unwrap();
        }
        finder
    }

    #[test]
    fn the_first_matches_are_kept_in_order_and_the_rest_counted() {
        let body = big_list(1200);
        let all = find("", &body);
        // Sensors, lights and binary sensors and automations: all four kinds shown.
        assert_eq!(all.total, 1200);
        assert_eq!(all.matches.len(), LIMIT);
        let ids: Vec<_> = all.matches.iter().map(|m| m.id.clone()).collect();
        let mut sorted = ids.clone();
        sorted.sort();
        assert_eq!(ids, sorted);
        let first = (0..1200)
            .filter(|i| i % 4 == 3)
            .map(|i| alloc::format!("automation.thing_{i}"))
            .min();
        assert_eq!(Some(ids[0].clone()), first);
    }

    #[test]
    fn a_search_matches_names_and_ids_without_case() {
        let body = big_list(400);
        let lamps = find("LAMP 13", &body);
        let expected = (0..400)
            .filter(|i| i % 4 == 1 && alloc::format!("{i}").starts_with("13"))
            .count();
        assert_eq!(lamps.total, expected);
        assert!(
            lamps
                .matches
                .iter()
                .any(|m| m.id == "light.thing_13" && m.name == "Lamp 13")
        );
        let doors = find("tür 2", &body);
        assert!(
            doors
                .matches
                .iter()
                .any(|m| m.id == "binary_sensor.thing_2")
        );
        let ids = (0..400)
            .filter(|i| i % 4 == 1 && alloc::format!("{i}").starts_with('1'))
            .count();
        assert_eq!(find("light.thing_1", &body).total, ids);
        assert_eq!(find("nothing like it", &body).total, 0);
    }

    #[test]
    fn unsupported_domains_are_left_out() {
        let mut finder = Finder::new("").unwrap();
        for id in ["lock.front", "climate.hall", "light.desk", "weather.home"] {
            finder
                .add(Record {
                    id: id.into(),
                    ..Record::default()
                })
                .unwrap();
        }
        assert_eq!(finder.total, 1);
    }

    #[test]
    fn rows_tick_the_chosen() {
        let body = big_list(8);
        let mut pick = Pick::default();
        pick.found("lamp", find("lamp", &body)).unwrap();
        assert_eq!(pick.query(), "lamp");
        pick.tick(["light.thing_1".into()].into_iter()).unwrap();
        let rows = pick.rows(&mut Keys).unwrap();
        assert_eq!(rows.len(), 3);
        let Item::Row(first) = &rows[1] else { panic!() };
        assert_eq!(first.title, "Lamp 1");
        assert_eq!(first.value, "✓");
        assert_eq!(first.note, "light.thing_1");
        let Item::Row(second) = &rows[2] else {
            panic!()
        };
        assert_eq!(second.value, "");
        let Item::Row(search) = &rows[0] else {
            panic!()
        };
        assert_eq!(search.note, "smart-home.pick.found 2 lamp");
    }
}
