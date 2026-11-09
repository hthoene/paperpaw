//! The sync's screen (`stats.sync`, reached from the overview and from the
//! settings of the folder and the user): the folder, the user, the password,
//! *Sync now*, and *Take over from KOReader* where KOReader's statistics are
//! on the card; below them the message of the last sync, or the help.
//!
//! The view holds what it shows and tells the shell which row was chosen; the
//! shell asks for the typed values, keeps them and starts the sync
//! (`native_plugin`).
use crate::native_webdav::folder;
use crate::texts::Words;
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::Handled;
use paperpaw_sdk::native::callbacks::{DrawFrame, Input};
use paperpaw_sdk::native::{Error, Host};
use paperpaw_ui_kit::{Footer, Item, ItemList, ItemStep};

/// The rows, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Row {
    Folder,
    User,
    Password,
    SyncNow,
    Import,
}

/// What the rows show.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Shown<'a> {
    /// The folder setting as stored, and the user.
    pub folder: &'a str,
    pub user: &'a str,
    /// Whether a password is kept.
    pub password: bool,
    /// Whether KOReader's statistics are on this device.
    pub koreader: bool,
}

/// What the password row shows where one is kept.
const DOTS: &str = "••••••";
/// What the action rows show.
const GO: &str = "›";

/// The sync's screen.
#[derive(Debug)]
pub(crate) struct SyncView {
    list: ItemList,
    rows: Vec<Row>,
    /// What the last sync said, or the help.
    message: String,
}

impl SyncView {
    /// A view of what is set now, with the help below.
    ///
    /// # Errors
    /// [`Error::NoMemory`] and what the catalog says.
    pub(crate) fn new(words: &mut impl Words, shown: Shown<'_>) -> Result<Self, Error> {
        let mut view = Self {
            list: ItemList::default(),
            rows: Vec::new(),
            message: words.copy("stats.sync.help")?,
        };
        view.refresh(words, shown)?;
        Ok(view)
    }

    /// Shows what is set now, keeping the focus where it was.
    ///
    /// # Errors
    /// [`Error::NoMemory`] and what the catalog says; the view is as it was.
    pub(crate) fn refresh(
        &mut self,
        words: &mut impl Words,
        shown: Shown<'_>,
    ) -> Result<(), Error> {
        let mut rows = Vec::new();
        rows.try_reserve_exact(5).map_err(|_| Error::NoMemory)?;
        rows.extend([Row::Folder, Row::User, Row::Password, Row::SyncNow]);
        if shown.koreader {
            rows.push(Row::Import);
        }
        let mut items = Vec::new();
        items
            .try_reserve_exact(rows.len())
            .map_err(|_| Error::NoMemory)?;
        for row in &rows {
            let (title, value) = match row {
                Row::Folder => {
                    let url = folder(shown.folder)?;
                    let shown = url.strip_prefix("https://").unwrap_or(&url);
                    (
                        words.copy("stats.sync.folder")?,
                        paperpaw_ui_kit::text::copy(shown)?,
                    )
                }
                Row::User => (
                    words.copy("stats.sync.user")?,
                    paperpaw_ui_kit::text::copy(shown.user)?,
                ),
                Row::Password => (
                    words.copy("stats.sync.password")?,
                    paperpaw_ui_kit::text::copy(if shown.password { DOTS } else { "" })?,
                ),
                Row::SyncNow => (
                    words.copy("stats.sync.now")?,
                    paperpaw_ui_kit::text::copy(GO)?,
                ),
                Row::Import => (
                    words.copy("stats.import")?,
                    paperpaw_ui_kit::text::copy(GO)?,
                ),
            };
            items.push(Item::row(&title, &value)?);
        }
        self.rows = rows;
        self.list.replace(items);
        Ok(())
    }

    /// Shows `message` below the rows (what a sync or an import said).
    pub(crate) fn say(&mut self, message: String) {
        self.message = message;
    }

    /// The rows shown, in order.
    #[cfg(test)]
    pub(crate) fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// Moves the focus to row `row`.
    pub(crate) fn focus(&mut self, row: Row) {
        if let Some(at) = self.rows.iter().position(|r| *r == row) {
            self.list.set_focus(at);
        }
    }

    pub(crate) fn draw(&mut self, host: &Host<'_>, frame: &mut DrawFrame<'_>) -> Result<(), Error> {
        self.list.draw_with(
            host,
            frame,
            "stats.sync.title",
            true,
            Footer::Note(&self.message),
        )
    }

    /// What `input` is: the row chosen, if one was.
    ///
    /// # Errors
    /// Propagates command queueing errors.
    pub(crate) fn input(
        &mut self,
        host: &Host<'_>,
        input: &Input<'_>,
    ) -> Result<(Handled, Option<Row>), Error> {
        let step = self.list.input(host, input)?;
        let chosen = match step {
            ItemStep::Chosen(index) => self.rows.get(index).copied(),
            _ => None,
        };
        Ok((ItemList::handled(step), chosen))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screens_tests::words;
    use alloc::string::ToString;
    use paperpaw_ui_kit::Item;

    fn texts(view: &SyncView) -> Vec<(String, String)> {
        view.list
            .items()
            .iter()
            .map(|item| match item {
                Item::Row(row) => (row.title.clone(), row.value.clone()),
                other => panic!("{other:?}"),
            })
            .collect()
    }

    fn shown<'a>(folder: &'a str, user: &'a str) -> Shown<'a> {
        Shown {
            folder,
            user,
            password: false,
            koreader: false,
        }
    }

    #[test]
    fn the_rows_say_what_is_set_in_both_languages() {
        for (language, folder_title, now) in [
            ("en", "Folder (WebDAV)", "Sync now"),
            ("de", "Ordner (WebDAV)", "Jetzt abgleichen"),
        ] {
            let mut w = words(language);
            let mut view = SyncView::new(&mut w, shown("cloud.example/dav/", "me")).unwrap();
            let rows = texts(&view);
            assert_eq!(rows.len(), 4, "{language}");
            assert_eq!(
                rows[0],
                (folder_title.to_string(), "cloud.example/dav".to_string()),
                "{language}"
            );
            assert_eq!(rows[1].1, "me");
            assert_eq!(rows[2].1, "");
            assert_eq!(rows[3], (now.to_string(), "›".to_string()), "{language}");
            // A password kept shows as dots, KOReader's file adds a row.
            view.refresh(
                &mut w,
                Shown {
                    password: true,
                    koreader: true,
                    ..shown("http://nas/dav", "")
                },
            )
            .unwrap();
            let rows = texts(&view);
            assert_eq!(rows.len(), 5);
            assert_eq!(rows[0].1, "http://nas/dav");
            assert_eq!(rows[2].1, "••••••");
            assert_eq!(view.rows()[4], Row::Import);
        }
    }

    #[test]
    fn the_help_shows_until_a_message_replaces_it() {
        let mut w = words("en");
        let mut view = SyncView::new(&mut w, shown("", "")).unwrap();
        assert!(view.message.starts_with("Enter the WebDAV address"));
        view.say("Syncing …".to_string());
        assert_eq!(view.message, "Syncing …");
    }
}
