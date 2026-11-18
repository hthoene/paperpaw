//! The connection screen: the server's address, the access token and a test
//! of both. The token is a secret: it is never shown, only whether one is kept.
use crate::say::Say;
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Handled,
    native::{
        Error, Host,
        callbacks::{DrawFrame, DrawPurpose, Input},
    },
};
use paperpaw_ui_kit::{Footer, Item, ItemList, ItemStep, text};

/// What a row of the screen is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    Server,
    Token,
    Test,
}

/// What the plugin has to do after an input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Act {
    Ignored,
    Taken,
    /// Ask for this row's text.
    Ask(Row),
    /// Fetch to see that the connection works.
    Test,
    Back,
}

/// What a kept token is shown as.
const DOTS: &str = "••••••";

/// The connection screen's state.
#[derive(Debug, Default)]
pub struct Connect {
    list: ItemList,
    /// What is said below the rows.
    note: String,
}

impl Connect {
    /// Makes the rows: `server` is the address set, `token` whether one is kept.
    ///
    /// # Errors
    /// `NoMemory`, or what the catalog says.
    pub fn rebuild(&mut self, say: &mut impl Say, server: &str, token: bool) -> Result<(), Error> {
        let mut items: Vec<Item> = Vec::new();
        items.try_reserve_exact(3).map_err(|_| Error::NoMemory)?;
        items.push(Item::row(&say.copy("smart-home.setting.server")?, server)?);
        items.push(Item::row(
            &say.copy("smart-home.connect.token")?,
            if token { DOTS } else { "" },
        )?);
        items.push(Item::row(&say.copy("smart-home.connect.test")?, "›")?);
        self.list.replace(items);
        Ok(())
    }

    /// What is said below the rows.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn say(&mut self, note: &str) -> Result<(), Error> {
        self.note = text::copy(note)?;
        Ok(())
    }

    /// Where the focus goes (a row's index).
    pub fn focus_on(&mut self, row: Row) {
        self.list.set_focus(match row {
            Row::Server => 0,
            Row::Token => 1,
            Row::Test => 2,
        });
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
    ) -> Result<(), Error> {
        if !matches!(frame.purpose, DrawPurpose::Screen) {
            return Ok(());
        }
        let footer = if self.note.is_empty() {
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
        Ok(match self.list.input(host, input)? {
            ItemStep::Chosen(0) => Act::Ask(Row::Server),
            ItemStep::Chosen(1) => Act::Ask(Row::Token),
            ItemStep::Chosen(2) => Act::Test,
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
