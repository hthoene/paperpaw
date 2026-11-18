//! Fetching the feeds: one feed after the other over `core.net` (each answer
//! saved to a file of its own in the data folder, at most 4 MiB), made into
//! its page (`native_book`) and put on the shelf (the library, the next slice),
//! and the work that does it in the background (`core.work`). No screen: the
//! plugin (`native_plugin.rs`) tells the reader what each step came to.
//!
//! The same rules as interface 6 (`lib.rs`): the feeds are found again by
//! their address after each answer (the list may change meanwhile), a feed
//! that failed does not stop the others, an answer that held no article leaves
//! the book there is, and a refused allocation stops the fetch (the rest would
//! fail as well).
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::native::{
    Error,
    net::{Reason, RequestId},
};
use paperpaw_ui_kit::{
    files::{Volume, read_all_on, replace_on},
    text,
};

use crate::native_book::{Failure, make_page};
use crate::native_convert::{Converted, DateText};
use crate::native_news::{FEEDS_CAP, Feed, Mark, feeds, feeds_text, short_title};

/// The plugin's own number of a feed's request, in its `http_done` event.
pub const FEED_REQUEST: u32 = 1;
/// The list of feeds, in the data folder.
pub const FEEDS_FILE: &str = "feeds.txt";
/// Where the page of a feed is made, in the data folder.
pub const PAGE_FILE: &str = "page.tmp";

/// Where a feed's answer is kept until it is read: a file of its own, as the
/// list may hold feeds of the same name.
///
/// # Errors
/// `NoMemory`.
pub fn download(url: &str) -> Result<String, Error> {
    text::format(format_args!("feed-{}.tmp", Mark::of(url).as_str()))
}

/// What the shelf is: where a finished page goes. The library's `import` in the
/// plugin; a stand-in in tests.
pub trait Shelf {
    /// Puts `page` (a file in the data folder) on the shelf as the book of
    /// `feed` (named by `name`, which `converted` may have brought), replacing
    /// the book it was.
    ///
    /// # Errors
    /// [`Failure::Failed`] when the book is not there; [`Failure::Memory`].
    fn put(
        &mut self,
        feed: &Feed,
        name: &str,
        page: &str,
        converted: &Converted,
    ) -> Result<(), Failure>;
}

/// What the fetch needs of the world: the data folder, the network and the
/// work. The host's in the plugin; a stand-in in tests.
pub trait Wire {
    /// The plugin's data folder.
    fn volume(&self) -> &dyn Volume;
    /// Asks for `url`, its answer saved to `file` in the data folder (at most
    /// 4 MiB), the end told with token [`FEED_REQUEST`].
    ///
    /// # Errors
    /// `Unsupported` or `Denied` without the network; what the host says.
    fn get(&mut self, url: &str, file: &str) -> Result<RequestId, Error>;
    /// Ends the requests of [`FEED_REQUEST`]; one that cannot be ended ends
    /// by itself and is ignored.
    fn cancel(&mut self);
    /// Declares the work that fetches all the feeds when it is cheap.
    ///
    /// # Errors
    /// What the work service says.
    fn declare_work(&mut self) -> Result<(), Error>;
}

/// A request that ended, as far as the fetch reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Done {
    /// The request.
    pub id: RequestId,
    /// The plugin's own number of it.
    pub token: u32,
    /// Why it ended.
    pub reason: Reason,
    /// A 2xx answer came and its body is in the file.
    pub saved: bool,
}

impl Done {
    /// The end of the request `id`.
    #[must_use]
    pub const fn new(id: RequestId, token: u32, reason: Reason, saved: bool) -> Self {
        Self {
            id,
            token,
            reason,
            saved,
        }
    }
}

/// What a start came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Started {
    /// The first request is sent.
    Fetching,
    /// There is no network service for it.
    NoNetwork,
    /// The list could not be read (and is not to be changed).
    Unreadable,
    /// There is no feed to fetch.
    None,
}

/// What the end of a request came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Ended {
    /// Not a request under way (cancelled, replaced, not ours).
    Ignored,
    /// The next feed is asked for.
    Next,
    /// The fetch is over: how many feeds made their book, how many did not.
    Finished { done: usize, failed: usize },
    /// The list could not be read.
    Unreadable,
    /// The heap had no room: the fetch is stopped.
    Memory,
    /// The next request was refused for want of a network (a run's end, the
    /// Wi-Fi off): the fetch is over, with what it had come to.
    NoNetwork { done: usize, failed: usize },
}

/// A request in flight, and the fetch it belongs to.
#[derive(Debug)]
struct Running {
    id: RequestId,
    url: String,
    index: usize,
    all: bool,
    done: usize,
    failed: usize,
}

/// The fetch under way.
#[derive(Debug, Default)]
pub struct Fetcher {
    running: Option<Running>,
}

/// The feeds; `NoSpace` for a file over its cap and `Utf8` for one that is
/// not text: the list is then not to be written over.
///
/// # Errors
/// `NoMemory`, `NoSpace`, `Utf8` or what the card says.
pub fn load(volume: &dyn Volume) -> Result<Vec<Feed>, Error> {
    let Some(bytes) = read_all_on(volume, FEEDS_FILE, FEEDS_CAP)? else {
        return Ok(Vec::new());
    };
    feeds(core::str::from_utf8(&bytes).map_err(|_| Error::Utf8)?)
}

/// Keeps the feeds. A card that does not take it costs the change only.
///
/// # Errors
/// `NoMemory`; the host's refusal.
pub fn save(volume: &dyn Volume, list: &[Feed]) -> Result<(), Error> {
    replace_on(volume, FEEDS_FILE, feeds_text(list)?.as_bytes())
}

impl Fetcher {
    /// A new one, with nothing under way.
    #[must_use]
    pub const fn new() -> Self {
        Self { running: None }
    }

    /// Whether a fetch is under way.
    #[must_use]
    pub const fn busy(&self) -> bool {
        self.running.is_some()
    }

    /// Starts fetching the feed at `from`, and with `all` those after it.
    ///
    /// # Errors
    /// `NoMemory`; what the services say, other than that there is no network.
    pub fn start(&mut self, wire: &mut dyn Wire, from: usize, all: bool) -> Result<Started, Error> {
        let list = match load(wire.volume()) {
            Ok(list) => list,
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => return Ok(Started::Unreadable),
        };
        let Some(feed) = list.get(from) else {
            return Ok(Started::None);
        };
        // A fetch under way is replaced: its request ends, its end is ignored.
        self.cancel(wire);
        match self.send(wire, feed, (from, all), (0, 0)) {
            Ok(()) => Ok(Started::Fetching),
            Err(Error::Unsupported | Error::Denied) => Ok(Started::NoNetwork),
            Err(error) => Err(error),
        }
    }

    /// Asks for `feed`, at `index` in the list, with the counts so far.
    fn send(
        &mut self,
        wire: &mut dyn Wire,
        feed: &Feed,
        (index, all): (usize, bool),
        (done, failed): (usize, usize),
    ) -> Result<(), Error> {
        let file = download(&feed.url)?;
        let id = wire.get(&feed.url, &file)?;
        self.running = Some(Running {
            id,
            url: text::copy(&feed.url)?,
            index,
            all,
            done,
            failed,
        });
        Ok(())
    }

    /// Cancels the fetch under way; its end is told apart and ignored.
    pub fn cancel(&mut self, wire: &mut dyn Wire) {
        if self.running.take().is_some() {
            wire.cancel();
        }
    }

    /// A request ended: its feed is made into its book and the next one is
    /// asked for, or the fetch is over. `settings` is how many articles a feed
    /// keeps and how large an article's text is at most.
    ///
    /// # Errors
    /// `NoMemory` while the file name is made; a refused next request is an
    /// [`Ended`], not an error.
    pub fn ended(
        &mut self,
        wire: &mut dyn Wire,
        shelf: &mut dyn Shelf,
        done: &Done,
        (articles, max_text): (usize, usize),
        date_text: DateText<'_>,
    ) -> Result<Ended, Error> {
        if done.token != FEED_REQUEST || self.running.as_ref().map(|r| r.id) != Some(done.id) {
            return Ok(Ended::Ignored);
        }
        let Some(mut run) = self.running.take() else {
            return Ok(Ended::Ignored);
        };
        if matches!(done.reason, Reason::Cancelled | Reason::Replaced) {
            return Ok(Ended::Ignored);
        }
        let file = download(&run.url)?;
        if done.reason == Reason::WorkEnded {
            // The run's 10 minutes are over: no request is to follow outside it.
            wire.volume().remove(&file).ok();
            return Ok(Ended::Finished {
                done: run.done,
                failed: run.failed,
            });
        }
        let ended = made(wire, shelf, &mut run, done, (articles, max_text), date_text);
        // The answer is read: its file goes, whatever came of it.
        wire.volume().remove(&file).ok();
        let next = match ended {
            Ok(next) => next,
            Err(Failure::Memory) => return Ok(Ended::Memory),
            Err(Failure::Failed) => return Ok(Ended::Unreadable),
        };
        if run.all
            && let Some(feed) = next
        {
            return match self.send(wire, &feed, (run.index, true), (run.done, run.failed)) {
                Ok(()) => Ok(Ended::Next),
                Err(Error::NoMemory) => Ok(Ended::Memory),
                // Counts are not lost to a refused request.
                Err(Error::Unsupported | Error::Denied) => Ok(Ended::NoNetwork {
                    done: run.done,
                    failed: run.failed,
                }),
                Err(_) => Ok(Ended::Finished {
                    done: run.done,
                    failed: run.failed + 1,
                }),
            };
        }
        Ok(Ended::Finished {
            done: run.done,
            failed: run.failed,
        })
    }
}

/// The answer made into the book of its feed and counted; with the feed to
/// ask for next, if there is one. `Failed` here means the list cannot be read.
fn made(
    wire: &mut dyn Wire,
    shelf: &mut dyn Shelf,
    run: &mut Running,
    done: &Done,
    settings: (usize, usize),
    date_text: DateText<'_>,
) -> Result<Option<Feed>, Failure> {
    let mut list = match load(wire.volume()) {
        Ok(list) => list,
        Err(Error::NoMemory) => return Err(Failure::Memory),
        Err(_) => return Err(Failure::Failed),
    };
    // A feed removed meanwhile is left out.
    let at = list.iter().position(|f| f.url == run.url);
    if let Some(at) = at {
        let result = if done.reason == Reason::Ok && done.saved {
            book(wire.volume(), shelf, &list, at, settings, date_text)
        } else {
            Err(Failure::Failed)
        };
        match result {
            Ok(feed) => {
                run.done += 1;
                if list.get(at) != Some(&feed) {
                    if let Some(slot) = list.get_mut(at) {
                        *slot = feed;
                    }
                    // A card that does not take the names costs the names.
                    save(wire.volume(), &list).ok();
                }
            }
            Err(Failure::Failed) => run.failed += 1,
            // The rest would fail as well: stopped.
            Err(Failure::Memory) => return Err(Failure::Memory),
        }
    }
    // The next one: after this feed, or in its place if it was removed.
    let next = at.map_or(run.index, |at| at + 1);
    run.index = next;
    Ok(match list.get(next) {
        Some(feed) => Some(Feed {
            url: text::copy(&feed.url)?,
            name: text::copy(&feed.name)?,
        }),
        None => None,
    })
}

/// Makes the answer for `list[at]` its book: the feed, with the name it may
/// have brought; a failure when the answer held no article (as a Wi-Fi's
/// sign-in page does): then the book there is stays.
fn book(
    volume: &dyn Volume,
    shelf: &mut dyn Shelf,
    list: &[Feed],
    at: usize,
    (articles, max_text): (usize, usize),
    date_text: DateText<'_>,
) -> Result<Feed, Failure> {
    let feed = list.get(at).ok_or(Failure::Failed)?;
    let mut feed = Feed {
        url: text::copy(&feed.url)?,
        name: text::copy(&feed.name)?,
    };
    let file = download(&feed.url)?;
    let converted = make_page(volume, (&file, PAGE_FILE), (articles, max_text), date_text)?;
    if feed.name.is_empty() && !converted.title.is_empty() {
        feed.name = short_title(&converted.title)?;
    }
    let name = text::copy(&feed.name)?;
    let put = shelf.put(&feed, &name, PAGE_FILE, &converted);
    volume.remove(PAGE_FILE).ok();
    put?;
    Ok(feed)
}
