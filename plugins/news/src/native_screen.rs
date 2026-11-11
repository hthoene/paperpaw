//! The list of feeds, a screen (one at a time): *Fetch the news* first, the
//! feeds, then *Add a feed*; before there is a feed only *Add a feed*, and what
//! feeds are. A tap opens a feed's book (or fetches the feed that has none), a
//! hold offers to open, fetch or remove it, *Add a feed* asks for its address
//! on the keyboard.
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::Handled;
use paperpaw_sdk::native::{
    Error, Host,
    callbacks::{DrawFrame, Input},
    commands::{CommandSender, RedrawMode},
    events::{AnswerOutcome, Answered},
    formatting::Arg,
    storage::Root,
    ui::{Open, Param, Question, TextMode},
};
use paperpaw_ui_kit::{
    Footer, Item, ItemList, ItemStep, Messages,
    files::read_all_on,
    mem::{SERVICE_BUFFER, buffer, push},
    text, uitext,
};

use crate::declarations::SCREEN;
use crate::native_books::{BOOKS_CAP, BOOKS_FILE, find};
use crate::native_fetch::{load, save};
use crate::native_news::{Feed, Mark};
use crate::native_plugin::{DataVolume, News, notice};
use crate::native_shelf::FOLDER;

/// The longest address typed.
const ADDRESS_MAX: usize = 1024;
/// The slot that reads a book, and the parameter that names it.
const READER: &str = "core.reader";
const PATH: &str = "path";

/// What a row does when chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Target {
    Fetch,
    Feed(usize),
    Add,
}

/// What a question asked on this screen is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Ask {
    /// The address of a new feed.
    Address,
    /// What to do with the feed at this place of the list.
    Option(usize),
}

/// The texts of the rows.
#[derive(Debug, Default)]
pub(crate) struct Words {
    pub(crate) fetch: String,
    pub(crate) fetch_help: String,
    pub(crate) add: String,
    pub(crate) add_help: String,
}

/// One open screen.
#[derive(Debug, Default)]
pub(crate) struct View {
    list: ItemList,
    feeds: Vec<Feed>,
    /// Whether the list of feeds could be read: if not, it is not changed
    /// either (what it holds would be lost).
    readable: bool,
    targets: Vec<Target>,
    /// What an empty list says: its title and its text.
    empty: (String, String),
}

/// The rows for `feeds`, and what each does: *Fetch the news*, the feeds, *Add
/// a feed*; with no feed only the last.
///
/// # Errors
/// `NoMemory`.
pub(crate) fn rows(feeds: &[Feed], words: &Words) -> Result<(Vec<Item>, Vec<Target>), Error> {
    let mut items = Vec::new();
    let mut targets = Vec::new();
    items
        .try_reserve_exact(feeds.len() + 2)
        .map_err(|_| Error::NoMemory)?;
    targets
        .try_reserve_exact(feeds.len() + 2)
        .map_err(|_| Error::NoMemory)?;
    if !feeds.is_empty() {
        items.push(Item::noted(&words.fetch, "", &words.fetch_help)?);
        targets.push(Target::Fetch);
        for (index, feed) in feeds.iter().enumerate() {
            items.push(Item::noted(feed.shown(), "", &feed.url)?);
            targets.push(Target::Feed(index));
        }
    }
    items.push(Item::noted(&words.add, "›", &words.add_help)?);
    targets.push(Target::Add);
    Ok((items, targets))
}

/// Whether `typed` can be a feed's address.
pub(crate) fn is_address(typed: &str) -> bool {
    (typed.starts_with("http://") || typed.starts_with("https://")) && typed.len() >= 10
}

fn redraw(host: &Host<'_>) -> Result<(), Error> {
    let mut scratch = [0; 64];
    CommandSender::new(host, &mut scratch).queue_redraw(RedrawMode::Partial)
}

impl News {
    /// The texts of the rows.
    fn words(host: &Host<'_>) -> Result<Words, Error> {
        let mut say = Messages::new()?;
        Ok(Words {
            fetch: say.copy(host, "news.fetch")?,
            fetch_help: say.format(
                host,
                "news.fetch.help",
                &[Arg::Text {
                    name: "folder",
                    value: FOLDER,
                }],
            )?,
            add: say.copy(host, "news.add")?,
            add_help: say.copy(host, "news.add.help")?,
        })
    }

    /// Reads the feeds again and makes the rows of `view` from them; the focus
    /// stays where it was, or on the last row.
    fn build(host: &Host<'_>, view: &mut View) -> Result<(), Error> {
        let (feeds, readable) = match load(&DataVolume(host)) {
            Ok(feeds) => (feeds, true),
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => (Vec::new(), false),
        };
        let (items, targets) = rows(&feeds, &Self::words(host)?)?;
        let focus = view.list.focus();
        let last = items.len().saturating_sub(1);
        view.list.replace(items);
        view.list.set_focus(focus.min(last));
        view.targets = targets;
        view.feeds = feeds;
        view.readable = readable;
        if view.feeds.is_empty() {
            let mut say = Messages::new()?;
            view.empty = (
                say.copy(host, "news.empty.title")?,
                say.copy(host, "news.empty")?,
            );
        }
        Ok(())
    }

    /// The screen opened: its rows are what the card holds.
    pub(crate) fn screen_opened(
        &mut self,
        host: &Host<'_>,
        instance: paperpaw_sdk::native::ui::Instance,
    ) -> Result<(), Error> {
        let mut view = View::default();
        Self::build(host, &mut view)?;
        self.screens.open(instance, view)
    }

    /// The screen is on top again (a fetch or a book may have changed what it
    /// shows): the rows are read again.
    pub(crate) fn screen_shown(
        &mut self,
        host: &Host<'_>,
        instance: paperpaw_sdk::native::ui::Instance,
    ) -> Result<(), Error> {
        self.screens.shown(SCREEN, instance)?;
        self.refresh(host)
    }

    /// Reads the rows of the screen on top again and asks for a redraw.
    pub(crate) fn refresh(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let Some(view) = self.screens.top(SCREEN) else {
            return Ok(());
        };
        Self::build(host, view)?;
        redraw(host)
    }

    /// Draws the screen on top.
    pub(crate) fn draw_screen(
        &mut self,
        host: &Host<'_>,
        mut frame: DrawFrame<'_>,
    ) -> Result<(), Error> {
        let title = Messages::new()?.copy(host, "news.title")?;
        let view = self.screens.top(SCREEN).ok_or(Error::State)?;
        let footer = if view.feeds.is_empty() {
            Footer::Empty {
                title: &view.empty.0,
                text: &view.empty.1,
            }
        } else {
            Footer::None
        };
        view.list.draw_with(host, &mut frame, &title, true, footer)
    }

    /// An input for the screen on top.
    pub(crate) fn input_screen(
        &mut self,
        host: &Host<'_>,
        input: &Input<'_>,
    ) -> Result<Handled, Error> {
        let Some(view) = self.screens.top(SCREEN) else {
            return Ok(Handled::No);
        };
        let step = view.list.input(host, input)?;
        match step {
            ItemStep::Chosen(row) => {
                let target = view.targets.get(row).copied();
                match target {
                    Some(Target::Fetch) => self.start(host, 0, true)?,
                    Some(Target::Feed(index)) => self.open_feed(host, index)?,
                    Some(Target::Add) => self.ask_address(host)?,
                    None => {}
                }
            }
            ItemStep::Held(row) => {
                if let Some(Target::Feed(index)) = view.targets.get(row).copied() {
                    self.ask_option(host, index)?;
                }
            }
            _ => {}
        }
        Ok(ItemList::handled(step))
    }

    /// Opens the book of the feed at `index`; a feed with no book yet is
    /// fetched.
    fn open_feed(&mut self, host: &Host<'_>, index: usize) -> Result<(), Error> {
        let Some(feed) = self
            .screens
            .top(SCREEN)
            .and_then(|view| view.feeds.get(index))
        else {
            return Ok(());
        };
        let mark = Mark::of(&feed.url);
        let kept = match read_all_on(&DataVolume(host), BOOKS_FILE, BOOKS_CAP) {
            Ok(kept) => kept,
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => None,
        };
        let kept = kept.as_deref().map(String::from_utf8_lossy);
        let book = match kept.as_deref().and_then(|kept| find(kept, mark.as_str())) {
            Some(path) if paperpaw_sdk::native::storage::size(host, Root::Files, path).is_ok() => {
                Some(text::copy(path)?)
            }
            _ => None,
        };
        match book {
            Some(path) => self.open_book(host, &path),
            None => self.start(host, index, false),
        }
    }

    /// Opens the book at `path` in the reader the reader chose.
    pub(crate) fn open_book(&mut self, host: &Host<'_>, path: &str) -> Result<(), Error> {
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let params = [Param::new(PATH, path)?];
        let handover = self
            .handles
            .ui(host)?
            .handover(host, &params, &mut input, &mut out)?;
        let mut scratch = [0; 256];
        CommandSender::new(host, &mut scratch).open(Open::slot(READER)?.handover(handover))
    }

    /// Asks for the address of a new feed, from `https://`.
    fn ask_address(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let title = Messages::new()?.copy(host, "news.address")?;
        let question = Question::text(&title, "")?.mode(TextMode::Url)?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let ui = self.handles.ui(host)?;
        let request = ui.ask(host, question, &mut input, &mut out)?;
        uitext::write_all(ui, host, request.text(), "https://")?;
        self.asked = Some((request, Ask::Address));
        Ok(())
    }

    /// Offers what can be done with the feed at `index`: open it, fetch it,
    /// remove it.
    fn ask_option(&mut self, host: &Host<'_>, index: usize) -> Result<(), Error> {
        let Some(feed) = self
            .screens
            .top(SCREEN)
            .and_then(|view| view.feeds.get(index))
        else {
            return Ok(());
        };
        let title = text::copy(feed.shown())?;
        let mut say = Messages::new()?;
        let choices = text::concat(&[
            &say.copy(host, "news.option.open")?,
            "\n",
            &say.copy(host, "news.option.fetch")?,
            "\n",
            &say.copy(host, "news.option.remove")?,
        ])?;
        let question = Question::choice(&title, "", &choices)?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let request = self
            .handles
            .ui(host)?
            .ask(host, question, &mut input, &mut out)?;
        self.asked = Some((request, Ask::Option(index)));
        Ok(())
    }

    /// An answer to a question of the screen.
    pub(crate) fn answered(&mut self, host: &Host<'_>, answer: Answered) -> Result<(), Error> {
        let Some((request, ask)) = self.asked else {
            return Ok(());
        };
        if request != answer.request() || answer.outcome() == AnswerOutcome::Typing {
            return Ok(());
        }
        self.asked = None;
        if answer.outcome() != AnswerOutcome::Answered {
            return Ok(());
        }
        match ask {
            Ask::Address => {
                let typed =
                    uitext::read_all(self.handles.ui(host)?, host, request.text(), ADDRESS_MAX);
                match typed {
                    Ok(typed) => self.add(host, typed.trim()),
                    // Longer than any address: it cannot be one.
                    Err(Error::NoSpace) => notice(host, "news.bad-address"),
                    Err(error) => Err(error),
                }
            }
            Ask::Option(index) => match answer.choice() {
                0 => self.open_feed(host, index),
                1 => self.start(host, index, false),
                2 => self.remove(host, index),
                _ => Ok(()),
            },
        }
    }

    /// The feed with address `typed` is added (if it is not in the list) and
    /// fetched.
    fn add(&mut self, host: &Host<'_>, typed: &str) -> Result<(), Error> {
        if !is_address(typed) {
            return notice(host, "news.bad-address");
        }
        let Some(view) = self.screens.top(SCREEN) else {
            return Ok(());
        };
        if !view.readable {
            return notice(host, "news.unreadable");
        }
        let index = match view.feeds.iter().position(|feed| feed.url == typed) {
            Some(index) => index,
            None => {
                push(
                    &mut view.feeds,
                    Feed {
                        url: text::copy(typed)?,
                        name: String::new(),
                    },
                )?;
                // A card that does not take the list does not get the feed:
                // a fetch of a feed the list does not hold would ask for a
                // book that is not there.
                if save(&DataVolume(host), &view.feeds).is_err() {
                    view.feeds.pop();
                    return notice(host, "news.unsaved");
                }
                view.feeds.len() - 1
            }
        };
        self.refresh(host)?;
        self.start(host, index, false)
    }

    /// The feed at `index` is removed from the list. Its book stays, a book
    /// like any other now: the reader may have notes in it, and deletes it as
    /// any.
    fn remove(&mut self, host: &Host<'_>, index: usize) -> Result<(), Error> {
        let Some(view) = self.screens.top(SCREEN) else {
            return Ok(());
        };
        if !view.readable {
            return notice(host, "news.unreadable");
        }
        if index < view.feeds.len() {
            view.feeds.remove(index);
            save(&DataVolume(host), &view.feeds).ok();
        }
        self.refresh(host)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "test assertions")]
    use super::*;

    fn words() -> Words {
        Words {
            fetch: String::from("Fetch the news"),
            fetch_help: String::from("fetch help"),
            add: String::from("Add a feed"),
            add_help: String::from("add help"),
        }
    }

    fn feed(url: &str, name: &str) -> Feed {
        Feed {
            url: String::from(url),
            name: String::from(name),
        }
    }

    #[test]
    fn with_no_feed_only_add_shows() {
        let (items, targets) = rows(&[], &words()).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(targets, [Target::Add]);
    }

    #[test]
    fn the_rows_are_fetch_the_feeds_and_add() {
        let feeds = [
            feed("https://a.example/rss", "A"),
            feed("https://www.b.example/feed", ""),
        ];
        let (items, targets) = rows(&feeds, &words()).unwrap();
        assert_eq!(items.len(), 4);
        assert_eq!(
            targets,
            [Target::Fetch, Target::Feed(0), Target::Feed(1), Target::Add]
        );
        let titles: Vec<&str> = items
            .iter()
            .map(|item| match item {
                Item::Row(row) => row.title.as_str(),
                _ => "",
            })
            .collect();
        // A feed with no name yet shows its host without `www.`.
        assert_eq!(titles, ["Fetch the news", "A", "b.example", "Add a feed"]);
    }

    #[test]
    fn an_address_is_a_web_address() {
        assert!(is_address("https://a.example/rss"));
        assert!(is_address("http://a.example"));
        assert!(!is_address("https://"));
        assert!(!is_address("ftp://a.example/rss"));
        assert!(!is_address("a.example/rss"));
        assert!(!is_address(""));
    }
}
