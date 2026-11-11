//! The first-run offer (interface 6: `setup.rs`): the setup's page that offers
//! a few plugins many readers add (reading statistics, the widgets for the home
//! screen, Calibre, KOReader's progress sync). The reader chooses any, or none;
//! *Done* (or leaving the page with some chosen) adds them: built-in ones are
//! switched on, the others are queued quietly for the library and fetched in
//! the background, and a notice says what happens.
//!
//! The rules are in [`native_ask`](crate::native_ask) (`rows`, `choose`,
//! `keep_chosen`, the words of the badge); this module is the page: rows with
//! what each is, *Chosen* in the value of those chosen, a last row *Done*, the
//! hint under them.
//!
//! The setup step is declared with the screen (`declarations::setup`); where
//! no plugin can be added here the plugin says so through
//! `command.set_item_state` (the step does not apply, [`applies`]); opened all
//! the same, the page shows only *Done*, as Beta 3's did.
//!
//! Memory: the words are read once, when the page opens (four rows, two hints);
//! a toggle builds the few rows again from them; a frame reads nothing.
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::native::{
    Error,
    callbacks::{DrawFrame, Input},
    snapshot,
};
use paperpaw_sdk::{Handled, Storage};
use paperpaw_ui_kit::mem::push;
use paperpaw_ui_kit::{Footer, Item, ItemList, ItemStep, RowItem, text};

use crate::native_ask::{Have, OFFERED, choose, hint_key, keep_chosen, rows as offered};
use crate::native_page::{Installed, Kind};
use crate::native_screen_list::{HostSay, Say};
use crate::native_screens::{Ctx, Installs, Next, Screen};

/// What the device has of a plugin, as the offer reads it.
#[must_use]
pub fn have_of(installed: Option<Installed<'_>>) -> Have {
    match installed {
        None => Have::Absent,
        Some(info) if info.enabled => Have::On,
        Some(info) if info.kind == Kind::BuiltIn => Have::OffBuiltIn,
        // A code plugin there but off is the plugin manager's.
        Some(_) => Have::OffOther,
    }
}

/// Whether the setup step applies: some plugin of the offer can be added here.
#[must_use]
pub fn applies(installs: &dyn Installs) -> bool {
    let have = |id: &str| have_of(installs.have(id));
    OFFERED
        .iter()
        .any(|id| crate::native_ask::can_add(&have, installs.can_install(), id))
}

/// The words of the page, read once.
#[derive(Debug, Default)]
pub struct Texts {
    /// The title of the page.
    pub title: String,
    /// The name and what it is of each plugin offered, in the order of the rows.
    pub rows: Vec<(String, String)>,
    /// The value of a row chosen.
    pub chosen: String,
    /// The label of the last row.
    pub done: String,
    /// How to choose, for this device.
    pub hint: String,
    /// What happens to those chosen.
    pub hint_chosen: String,
}

/// Reads the words of the rows `ids` (`buttons`: a device with keys, no touch).
///
/// # Errors
/// `NoMemory`, or what the catalog says.
pub fn texts(say: &mut dyn Say, ids: &[&str], buttons: bool) -> Result<Texts, Error> {
    let mut rows = Vec::new();
    rows.try_reserve_exact(ids.len())
        .map_err(|_| Error::NoMemory)?;
    for id in ids {
        let (name, help) = crate::native_ask::row_keys(id).ok_or(Error::Invalid)?;
        rows.push((say.copy(name)?, say.copy(help)?));
    }
    Ok(Texts {
        title: say.copy("store.offer.title")?,
        rows,
        chosen: say.copy("store.offer.chosen")?,
        done: say.copy("store.offer.finish")?,
        hint: say.copy(hint_key(false, buttons))?,
        hint_chosen: say.copy(hint_key(true, buttons))?,
    })
}

/// The rows: each plugin offered (what it is below its name, *Chosen* at the
/// right where it is), then *Done*.
///
/// # Errors
/// `NoMemory`.
pub fn items(on: &[bool], texts: &Texts) -> Result<Vec<Item>, Error> {
    let mut items = Vec::new();
    items
        .try_reserve_exact(texts.rows.len() + 1)
        .map_err(|_| Error::NoMemory)?;
    for (index, (name, help)) in texts.rows.iter().enumerate() {
        let chosen = on.get(index).copied().unwrap_or(false);
        push(
            &mut items,
            Item::Row(RowItem {
                title: text::copy(name)?,
                value: if chosen {
                    text::copy(&texts.chosen)?
                } else {
                    String::new()
                },
                note: text::copy(help)?,
                ..RowItem::default()
            }),
        )?;
    }
    push(&mut items, Item::row(&texts.done, "")?)?;
    Ok(items)
}

/// The plugins of `ids` that are chosen (`on`), in the order of the rows.
///
/// # Errors
/// `NoMemory`.
pub fn chosen_of(ids: &[&'static str], on: &[bool]) -> Result<Vec<&'static str>, Error> {
    let mut chosen = Vec::new();
    chosen
        .try_reserve_exact(on.iter().filter(|on| **on).count())
        .map_err(|_| Error::NoMemory)?;
    for (id, _) in ids.iter().zip(on).filter(|(_, on)| **on) {
        push(&mut chosen, *id)?;
    }
    Ok(chosen)
}

/// Flips choice `index` and makes the rows that show it; where the rows cannot
/// be made the choice is put back, so what *Done* adds is always what the
/// reader sees chosen. `None` where there is no such row.
///
/// # Errors
/// `NoMemory`.
pub fn toggled(on: &mut [bool], index: usize, texts: &Texts) -> Result<Option<Vec<Item>>, Error> {
    let Some(flag) = on.get_mut(index) else {
        return Ok(None);
    };
    *flag = !*flag;
    match items(on, texts) {
        Ok(rows) => Ok(Some(rows)),
        Err(error) => {
            if let Some(flag) = on.get_mut(index) {
                *flag = !*flag;
            }
            Err(error)
        }
    }
}

/// The page.
#[derive(Debug)]
pub struct View {
    /// The plugins offered here, in the order of the rows.
    ids: Vec<&'static str>,
    /// Whether each is chosen.
    on: Vec<bool>,
    texts: Texts,
    list: ItemList,
    /// Those chosen were added (*Done*, or the page left): not again.
    added: bool,
}

impl View {
    fn any_chosen(&self) -> bool {
        self.on.iter().any(|on| *on)
    }

    /// Flips row `index`.
    fn toggle(&mut self, index: usize) -> Result<Next, Error> {
        let Some(rows) = toggled(&mut self.on, index, &self.texts)? else {
            return Ok(Next::None);
        };
        self.list.replace(rows);
        Ok(Next::Redraw)
    }

    /// Adds the plugins chosen, if any, once (`close`: and the page closes).
    fn finish(&mut self, ctx: &Ctx<'_>, close: bool) -> Result<Next, Error> {
        if self.added {
            return Ok(if close { Next::Close } else { Next::None });
        }
        let chosen = chosen_of(&self.ids, &self.on)?;
        let next = add(ctx.installs, ctx.data, &chosen, close)?;
        self.added = !matches!(next, Next::Close | Next::None);
        Ok(next)
    }
}

/// Adds the plugins `chosen`: kept for the setup's card, the built-in ones
/// switched on, the others queued quietly for the library (the plugin starts
/// the queue). Nothing chosen is nothing to do (`close`: the page still
/// closes).
///
/// # Errors
/// `NoMemory`; `Io` where the card refuses the queue.
pub fn add(
    installs: &dyn Installs,
    data: &dyn Storage,
    chosen: &[&'static str],
    close: bool,
) -> Result<Next, Error> {
    if chosen.is_empty() {
        return Ok(if close { Next::Close } else { Next::None });
    }
    let have = |id: &str| have_of(installs.have(id));
    let plan = choose(chosen, &have, installs.can_install())?;
    keep_chosen(data, &plan)?;
    Ok(Next::Offered {
        switch_on: plan.switch_on,
        queued: !plan.fetch.is_empty(),
        notify: plan.notify,
        close,
    })
}

impl Screen for View {
    fn open(ctx: &Ctx<'_>, _: &[(&str, &str)]) -> Result<Self, Error> {
        let have = |id: &str| have_of(ctx.installs.have(id));
        let ids = offered(&have, ctx.installs.can_install())?;
        let buttons = !snapshot::capture_snapshot(ctx.host)?.has_touch();
        let texts = texts(&mut HostSay::new(ctx.host)?, &ids, buttons)?;
        let mut on = Vec::new();
        on.try_reserve_exact(ids.len())
            .map_err(|_| Error::NoMemory)?;
        on.resize(ids.len(), false);
        let list = ItemList::new(items(&on, &texts)?);
        Ok(Self {
            ids,
            on,
            texts,
            list,
            added: false,
        })
    }

    fn draw(&mut self, ctx: &Ctx<'_>, frame: &mut DrawFrame<'_>) -> Result<(), Error> {
        let hint = if self.any_chosen() {
            &self.texts.hint_chosen
        } else {
            &self.texts.hint
        };
        self.list
            .draw_with(ctx.host, frame, &self.texts.title, true, Footer::Note(hint))
    }

    fn input(&mut self, ctx: &Ctx<'_>, input: &Input<'_>) -> Result<(Handled, Next), Error> {
        let step = self.list.input(ctx.host, input)?;
        let next = match step {
            ItemStep::Chosen(index) if index < self.ids.len() => self.toggle(index)?,
            ItemStep::Chosen(_) => self.finish(ctx, true)?,
            _ => Next::None,
        };
        Ok((ItemList::handled(step), next))
    }

    /// Left with plugins chosen (*Back*, the header's arrow): they are added as
    /// *Done* adds them.
    fn closing(&mut self, ctx: &Ctx<'_>) -> Result<Next, Error> {
        self.finish(ctx, false)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "test assertions")]
    use super::*;
    use crate::native_screen_list::Say;
    use alloc::string::ToString;

    struct Catalog;

    impl Say for Catalog {
        fn copy(&mut self, key: &str) -> Result<String, Error> {
            Ok(key.to_string())
        }
        fn format(
            &mut self,
            key: &str,
            _: &[paperpaw_sdk::native::formatting::Arg<'_>],
        ) -> Result<String, Error> {
            Ok(key.to_string())
        }
        fn digits(&mut self, text: &str) -> Result<String, Error> {
            Ok(text.to_string())
        }
    }

    #[test]
    fn what_the_device_has_decides_the_row() {
        let installed = |kind, enabled| {
            Some(Installed {
                kind,
                enabled,
                restart_needed: false,
                version: "1.0.0",
                origin: crate::native_page::Origin::Store,
                release: None,
                trouble: None,
            })
        };
        assert_eq!(have_of(None), Have::Absent);
        assert_eq!(have_of(installed(Kind::Code, true)), Have::On);
        assert_eq!(have_of(installed(Kind::BuiltIn, false)), Have::OffBuiltIn);
        assert_eq!(have_of(installed(Kind::Code, false)), Have::OffOther);
        assert_eq!(have_of(installed(Kind::Data, false)), Have::OffOther);
    }

    #[test]
    fn the_words_are_read_for_the_rows_and_the_device() {
        let words = texts(&mut Catalog, &["stats", "kosync"], true).unwrap();
        assert_eq!(words.rows.len(), 2);
        assert_eq!(words.rows[0].0, "store.offer.stats");
        assert_eq!(words.rows[1].1, "store.offer.kosync.help");
        assert_eq!(words.hint, "store.offer.hint.buttons");
        assert_eq!(words.hint_chosen, "store.offer.hint.chosen");
        let touch = texts(&mut Catalog, &["stats"], false).unwrap();
        assert_eq!(touch.hint, "store.offer.hint");
        // A plugin the offer does not know is no row.
        assert_eq!(
            texts(&mut Catalog, &["nope"], false).unwrap_err(),
            Error::Invalid
        );
    }

    #[test]
    fn the_rows_are_the_plugins_then_done_and_chosen_shows_at_the_right() {
        let words = texts(&mut Catalog, &["stats", "widgets"], false).unwrap();
        let rows = items(&[false, true], &words).unwrap();
        assert_eq!(rows.len(), 3);
        let Item::Row(first) = &rows[0] else {
            panic!("a row");
        };
        assert_eq!(first.title, "store.offer.stats");
        assert_eq!(first.note, "store.offer.stats.help");
        assert_eq!(first.value, "");
        let Item::Row(second) = &rows[1] else {
            panic!("a row");
        };
        assert_eq!(second.value, "store.offer.chosen");
        let Item::Row(last) = &rows[2] else {
            panic!("a row");
        };
        assert_eq!(last.title, "store.offer.finish");
    }
}
