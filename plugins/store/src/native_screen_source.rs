//! Where a plugin's source is (interface 6: `source.rs`): the plugin's name,
//! its address as a QR code to open on a phone, and the address written out
//! below it, with a line on how to scan it. Opened from a plugin's page with
//! the parameters `title` (the plugin's name) and `text` (the address).
//!
//! The rows are made once, when the screen opens; the code is made there too
//! (`QrCode::try_new` asks for its room and says no: the address is then
//! written out alone, never a failed screen).
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::Handled;
use paperpaw_sdk::builtin::params::{TEXT, TITLE};
use paperpaw_sdk::native::{
    Error,
    callbacks::{DrawFrame, Input},
};
use paperpaw_sdk::qr::QrCode;
use paperpaw_ui_kit::mem::push;
use paperpaw_ui_kit::{Footer, Item, ItemList, Messages, RowItem, text};

use crate::native_screens::{Ctx, Next, Screen, ids};

/// The address as the screen writes it: without `https://`.
#[must_use]
pub fn shown(url: &str) -> &str {
    url.strip_prefix("https://").unwrap_or(url)
}

/// What opens the screen for plugin `name` and its source at `url`.
///
/// # Errors
/// `NoMemory`.
pub fn open(name: &str, url: &str) -> Result<Next, Error> {
    let mut params = Vec::new();
    params.try_reserve_exact(2).map_err(|_| Error::NoMemory)?;
    params.push((TITLE, text::copy(name)?));
    params.push((TEXT, text::copy(url)?));
    Ok(Next::Open {
        screen: ids::SOURCE,
        params,
    })
}

/// The name and the address of the parameters.
///
/// # Errors
/// `Invalid` where there is no address (nothing to show).
pub fn read<'a>(params: &[(&str, &'a str)]) -> Result<(&'a str, &'a str), Error> {
    let get = |key: &str| {
        params
            .iter()
            .find(|(name, _)| *name == key)
            .map_or("", |(_, value)| *value)
    };
    let url = get(TEXT);
    if url.is_empty() {
        return Err(Error::Invalid);
    }
    Ok((get(TITLE), url))
}

/// The rows: who it is from (a heading), then how to scan the code (faint,
/// not chosen).
///
/// # Errors
/// `NoMemory`.
pub fn items(name: &str, help: &str) -> Result<Vec<Item>, Error> {
    let mut items = Vec::new();
    items.try_reserve_exact(2).map_err(|_| Error::NoMemory)?;
    if !name.is_empty() {
        push(&mut items, Item::heading(name)?)?;
    }
    push(
        &mut items,
        Item::Row(RowItem {
            title: String::new(),
            value: String::new(),
            note: text::copy(help)?,
            zone: None,
            stepper: false,
            disabled: true,
            on: None,
            field: false,
            whole: false,
        }),
    )?;
    Ok(items)
}

/// The screen.
#[derive(Debug)]
pub struct View {
    list: ItemList,
    /// The address, written out below the code (or alone where there is none).
    caption: String,
    qr: Option<QrCode>,
    /// The title, said once when the screen opens (a frame reads no text).
    title: String,
}

impl Screen for View {
    fn open(ctx: &Ctx<'_>, params: &[(&str, &str)]) -> Result<Self, Error> {
        let (name, url) = read(params)?;
        let mut messages = Messages::new()?;
        let help = messages.copy(ctx.host, "store.source.help")?;
        let title = messages.copy(ctx.host, "store.source")?;
        Ok(Self {
            title,
            list: ItemList::new(items(name, &help)?),
            caption: text::copy(shown(url))?,
            qr: QrCode::try_new(url.as_bytes()).ok(),
        })
    }

    fn draw(&mut self, ctx: &Ctx<'_>, frame: &mut DrawFrame<'_>) -> Result<(), Error> {
        let footer = match &self.qr {
            Some(code) => Footer::Code {
                code,
                caption: &self.caption,
            },
            None => Footer::Note(&self.caption),
        };
        self.list
            .draw_with(ctx.host, frame, &self.title, true, footer)
    }

    fn input(&mut self, ctx: &Ctx<'_>, input: &Input<'_>) -> Result<(Handled, Next), Error> {
        // Only the header's back arrow is the screen's; Back is the plugin's.
        let step = self.list.input(ctx.host, input)?;
        Ok((ItemList::handled(step), Next::None))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "test assertions")]
    use super::*;

    #[test]
    fn the_address_is_written_without_its_scheme() {
        assert_eq!(shown("https://github.com/a/b"), "github.com/a/b");
        assert_eq!(shown("http://x.example"), "http://x.example");
        assert_eq!(shown(""), "");
    }

    #[test]
    fn the_parameters_are_the_name_and_the_address() {
        let params = [("title", "Clock"), ("text", "https://x.example/c")];
        assert_eq!(read(&params).unwrap(), ("Clock", "https://x.example/c"));
        // No name is fine, no address is nothing to show.
        assert_eq!(read(&[("text", "https://x")]).unwrap().0, "");
        assert_eq!(read(&[("title", "Clock")]), Err(Error::Invalid));
        assert_eq!(read(&[]), Err(Error::Invalid));
    }

    #[test]
    fn opening_carries_the_name_and_the_address() {
        let Next::Open { screen, params } = open("Clock", "https://x.example").unwrap() else {
            panic!("an open");
        };
        assert_eq!(screen, "store.source");
        assert_eq!(
            params,
            [
                ("title", String::from("Clock")),
                ("text", String::from("https://x.example"))
            ]
        );
    }

    #[test]
    fn the_rows_are_the_name_and_a_faint_line_of_help() {
        let items = items("Clock", "Scan it.").unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0], Item::Heading(String::from("Clock")));
        let Item::Row(row) = &items[1] else {
            panic!("a row");
        };
        assert_eq!(row.note, "Scan it.");
        assert!(row.disabled);
        // Without a name only the help is there.
        assert_eq!(super::items("", "Scan it.").unwrap().len(), 1);
    }

    #[test]
    fn a_code_is_made_for_an_address() {
        assert!(QrCode::try_new(b"https://github.com/paperpaw/clock").is_ok());
    }
}
