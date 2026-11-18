//! The dashboard: the entities the reader chose as rows (their state at the
//! right, under the heading of their room), and below them *Refresh now*,
//! *Choose entities* and *Connection*. A switch, light or scene is chosen to
//! switch it; a value is chosen to read it again.
//!
//! The view holds no host handle: the plugin (`native_plugin.rs`) does what a
//! row means ([`Act`]). A changed state changes the text of its row in place
//! and only that row is drawn again ([`Dash::update`]); the rows are made again
//! only where the list of entities or their rooms changed.
use crate::model::{Entity, Store};
use crate::say::{Say, value_text};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Handled, Rect,
    native::{
        Error, Host,
        callbacks::{DrawFrame, Input},
    },
};
use paperpaw_ui_kit::{Footer, Item, ItemList, ItemStep, list::redraw_areas, mem::push, text};

/// What a row of the list is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    /// A heading over the rows of a room.
    Heading,
    /// The entity with this id.
    Entity(String),
    Refresh,
    Choose,
    Connect,
}

/// What the plugin has to do after an input.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Act {
    /// Not for the view.
    Ignored,
    /// Taken; nothing to do.
    Taken,
    /// The entity with this id was chosen.
    Chosen(String),
    Refresh,
    Choose,
    Connect,
    /// The back arrow.
    Back,
}

/// How an update changed the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Update {
    /// Nothing changed.
    Same,
    /// Texts changed in place: draw this area again (`None`: not on the page).
    Values(Option<Rect>),
    /// The rows were made anew: draw the screen.
    Rebuilt,
}

/// The dashboard's state.
#[derive(Debug, Default)]
pub struct Dash {
    list: ItemList,
    rows: Vec<Row>,
    /// The id, title and room each entity row was made from, in order: a change
    /// makes the rows again.
    shape: Vec<(String, String, String)>,
    /// What is said below the rows.
    note: String,
    empty: bool,
}

impl Dash {
    /// The rows for `store`, with the catalog's words.
    fn make(store: &Store, say: &mut impl Say) -> Result<(Vec<Item>, Vec<Row>), Error> {
        let mut items = Vec::new();
        let mut rows = Vec::new();
        let rooms = store.rooms()?;
        let mut order: Vec<&Entity> = Vec::new();
        for e in store.entities.iter().filter(|e| e.room.is_empty()) {
            push(&mut order, e)?;
        }
        let has_unplaced = !order.is_empty();
        for room in &rooms {
            for e in store.entities.iter().filter(|e| e.room == *room) {
                push(&mut order, e)?;
            }
        }
        let mut last: Option<&str> = None;
        for e in order {
            let room = e.room.as_str();
            // The entities without a room come first, under no heading; each
            // room has its own heading when there is more than one place.
            if last != Some(room) && !room.is_empty() && (has_unplaced || rooms.len() > 1) {
                push(&mut items, Item::heading(room)?)?;
                push(&mut rows, Row::Heading)?;
            }
            last = Some(room);
            push(&mut items, Item::row(e.title(), &value_text(say, e)?)?)?;
            push(&mut rows, Row::Entity(text::copy(&e.id)?))?;
        }
        for (row, key) in [
            (Row::Refresh, "smart-home.refresh"),
            (Row::Choose, "smart-home.choose"),
            (Row::Connect, "smart-home.connect.title"),
        ] {
            push(&mut items, Item::row(&say.copy(key)?, "›")?)?;
            push(&mut rows, row)?;
        }
        Ok((items, rows))
    }

    fn shape_of(store: &Store) -> Result<Vec<(String, String, String)>, Error> {
        let mut shape = Vec::new();
        for e in &store.entities {
            push(
                &mut shape,
                (
                    text::copy(&e.id)?,
                    text::copy(e.title())?,
                    text::copy(&e.room)?,
                ),
            )?;
        }
        Ok(shape)
    }

    /// Makes the rows anew for `store`, the focus kept where it was.
    ///
    /// # Errors
    /// `NoMemory`, or what the catalog says.
    pub fn rebuild(&mut self, store: &Store, say: &mut impl Say) -> Result<(), Error> {
        let (items, rows) = Self::make(store, say)?;
        self.shape = Self::shape_of(store)?;
        self.list.replace(items);
        self.rows = rows;
        self.empty = store.entities.is_empty();
        Ok(())
    }

    /// Brings the rows up to date with `store`: the text of a row that changed
    /// in place, the rows anew where entities or rooms changed.
    ///
    /// # Errors
    /// As [`Self::rebuild`].
    pub fn update(&mut self, store: &Store, say: &mut impl Say) -> Result<Update, Error> {
        if self.shape != Self::shape_of(store)? || self.rows.is_empty() {
            self.rebuild(store, say)?;
            return Ok(Update::Rebuilt);
        }
        let mut area: Option<Rect> = None;
        let mut changed = false;
        let mut off_page = false;
        for (index, row) in self.rows.iter().enumerate() {
            let Row::Entity(id) = row else { continue };
            let Some(entity) = store.find(id) else {
                continue;
            };
            let value = value_text(say, entity)?;
            let same =
                matches!(self.list.items().get(index), Some(Item::Row(r)) if r.value == value);
            if same {
                continue;
            }
            self.list.set_value(index, value);
            changed = true;
            match self.list.rect_of(index) {
                Some(rect) => area = Some(area.map_or(rect, |a| a.union(&rect))),
                None => off_page = true,
            }
        }
        Ok(match (changed, off_page) {
            (false, _) => Update::Same,
            (true, true) => Update::Values(None),
            (true, false) => Update::Values(area),
        })
    }

    /// What is said below the rows (empty: nothing).
    pub fn say(&mut self, note: String) -> bool {
        let changed = self.note != note;
        self.note = note;
        changed
    }

    /// Asks for `area` of the screen (or all of it) to be drawn again.
    ///
    /// # Errors
    /// Propagates command queueing errors.
    pub fn redraw(host: &Host<'_>, area: Option<Rect>) -> Result<(), Error> {
        redraw_areas(host, (0, 0), [area, area])
    }

    /// Draws the screen.
    ///
    /// # Errors
    /// What the host says; `NoMemory`.
    pub fn draw(
        &mut self,
        host: &Host<'_>,
        frame: &mut DrawFrame<'_>,
        title: &str,
        empty: (&str, &str),
    ) -> Result<(), Error> {
        let footer = if self.empty && self.note.is_empty() {
            Footer::Empty {
                title: empty.0,
                text: empty.1,
            }
        } else if self.note.is_empty() {
            Footer::None
        } else {
            Footer::Note(&self.note)
        };
        self.list.draw_with(host, frame, title, true, footer)
    }

    /// A key or a tap.
    ///
    /// # Errors
    /// What the host says.
    pub fn input(&mut self, host: &Host<'_>, input: &Input<'_>) -> Result<Act, Error> {
        let step = self.list.input(host, input)?;
        Ok(match step {
            ItemStep::Chosen(index) => match self.rows.get(index) {
                Some(Row::Entity(id)) => Act::Chosen(text::copy(id)?),
                Some(Row::Refresh) => Act::Refresh,
                Some(Row::Choose) => Act::Choose,
                Some(Row::Connect) => Act::Connect,
                _ => Act::Taken,
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

    fn entity(id: &str, name: &str, room: &str, state: &str) -> Entity {
        Entity {
            id: id.into(),
            name: name.into(),
            room: room.into(),
            state: state.into(),
            ..Entity::default()
        }
    }

    fn store() -> Store {
        let mut store = Store::default();
        for e in [
            entity("light.desk", "Desk", "Study", "on"),
            entity("sensor.t", "Temperature", "Kitchen", "21.5"),
            entity("light.hall", "Hall", "", "off"),
            entity("sensor.h", "Humidity", "Kitchen", "48"),
        ] {
            store.add(e).unwrap();
        }
        store
    }

    #[test]
    fn rows_are_grouped_under_rooms_with_the_controls_last() {
        let mut dash = Dash::default();
        dash.rebuild(&store(), &mut Keys).unwrap();
        let rows: Vec<_> = dash
            .rows
            .iter()
            .map(|r| match r {
                Row::Heading => "#".into(),
                Row::Entity(id) => id.clone(),
                other => alloc::format!("{other:?}"),
            })
            .collect();
        assert_eq!(
            rows,
            [
                "light.hall",
                "#",
                "light.desk",
                "#",
                "sensor.t",
                "sensor.h",
                "Refresh",
                "Choose",
                "Connect"
            ]
        );
    }

    #[test]
    fn a_changed_state_changes_its_row_only() {
        let mut dash = Dash::default();
        let mut store = store();
        dash.rebuild(&store, &mut Keys).unwrap();
        assert_eq!(dash.update(&store, &mut Keys).unwrap(), Update::Same);
        store.find_mut("light.desk").unwrap().state = "off".into();
        // No frame was drawn: no row has a place yet, so the whole screen is asked for.
        assert_eq!(
            dash.update(&store, &mut Keys).unwrap(),
            Update::Values(None)
        );
        assert_eq!(dash.update(&store, &mut Keys).unwrap(), Update::Same);
        store.remove("light.hall");
        assert_eq!(dash.update(&store, &mut Keys).unwrap(), Update::Rebuilt);
        store.find_mut("sensor.t").unwrap().room = "Cellar".into();
        assert_eq!(dash.update(&store, &mut Keys).unwrap(), Update::Rebuilt);
    }
}
