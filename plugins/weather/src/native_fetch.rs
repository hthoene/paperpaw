//! Fetching the weather and the places: the state the widgets share (the
//! weather kept, a fetch under way, a failed one), when to look (`core.work`,
//! `core.device`), and the two requests of `core.net`. No screen: the plugin
//! (`native_plugin.rs`) says what each answer means to the widgets and to the
//! place screen.
//!
//! The same rules as interface 6 (`lib.rs`): the weather is fetched while a
//! weather widget is placed, at most each hour or as set, and only while there
//! is a network (the radio is never switched on for it); a tap fetches it at
//! once; the last answer is kept in a small file so the widgets show it after a
//! start.
use crate::{
    declarations::WORK_TOKEN,
    native_answers::{Forecast, Places},
    native_shown::{Shown, Waiting},
    native_weather::{self as model, KEPT_CAP, Place, Weather},
};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::native::{
    Error, Host,
    device::{Device, PowerLevel},
    net::{BODY_PART, Header, HttpDone, Net, REPLY, REQUEST_INPUT, Reason, Request, RequestId},
    snapshot::capture_snapshot,
    storage::{self, Root},
    work::{Moment, Work, WorkSpec},
};
use paperpaw_ui_kit::{files, mem, text, widget::settings::text_of};

/// The plugin's own number of the forecast's request, in its `http_done` event.
pub const FETCH_REQUEST: u32 = 1;
/// The number of the search's request.
pub const SEARCH_REQUEST: u32 = 2;
/// The key of the work that looks again.
pub const WORK_KEY: &str = "weather.refresh";
/// Where the last weather is kept, in the data folder.
pub const FILE: &str = "weather.txt";
/// The settings.
pub const COORDINATES: &str = "weather.coordinates";
pub const PLACE: &str = "weather.place";
pub const EVERY: &str = "weather.every";
/// The longest answer of either service read, in bytes.
pub const MAX_BODY: u32 = 16 * 1024;
/// The `User-Agent` of interface 6.
const USER_AGENT: &str = "PaperPaw/0.1 (e-reader)";
/// No new try sooner after a fetch failed, in milliseconds.
const RETRY_MS: u64 = 15 * 60 * 1000;
/// A fetch under way is not asked again for this long, in milliseconds.
const PATIENCE_MS: u64 = 2 * 60 * 1000;
/// The longest a setting's text is read (coordinates and names are far shorter).
const SETTING_CAP: usize = 256;

/// Why the weather is looked at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Why {
    /// The timer, a start, a wake or a new widget: fetched if due.
    Due,
    /// The reader asked for it (a tap, the action, a new place).
    Asked,
}

/// What a look came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Looked {
    /// Nothing was sent.
    Nothing,
    /// A fetch was sent; `told` when the reader asked for it and is told so.
    Fetching { told: bool },
}

/// How a fetch ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Fetched {
    /// New weather, kept.
    Kept,
    /// None came; `told` when the reader asked and is told.
    Failed { told: bool },
}

/// How a search ended.
#[derive(Debug)]
#[non_exhaustive]
pub enum Searched {
    /// The places, none when the name found nothing.
    Found(Vec<Place>),
    /// The offline power mode refused it; the reader may go online.
    Offline,
    /// It failed.
    Failed,
}

/// The request in flight, and what it was for.
#[derive(Debug)]
struct Fetch {
    id: RequestId,
    coordinates: String,
    told: bool,
}

/// The shared state and the handles of the services.
#[derive(Debug, Default)]
pub struct Fetcher {
    weather: Option<Weather>,
    loaded: bool,
    /// A fetch is under way, since then (milliseconds on the host's clock).
    asked: Option<u64>,
    /// The last fetch failed, then.
    failed: Option<u64>,
    /// The weather was fetched then, in this run (for its age where the time
    /// of day is not known).
    fetched_at: Option<u64>,
    fetch: Option<Fetch>,
    search: Option<RequestId>,
    net: Option<Net>,
    work: Option<Work>,
    device: Option<Device>,
}

/// What the widgets show, owned: the parts of [`Shown`] that are not the weather.
#[derive(Debug)]
pub struct Context {
    waiting: Waiting,
    place: String,
    coordinates: String,
    old: Option<i64>,
    today: Option<i32>,
}

/// A setting's text, trimmed.
///
/// # Errors
/// `NoMemory`; what the settings say.
pub fn setting(host: &Host<'_>, key: &str) -> Result<String, Error> {
    text_of(host, key, |value| {
        let value = value.trim();
        let value = value.get(..value.len().min(SETTING_CAP)).unwrap_or("");
        text::copy(value)
    })?
}

impl Fetcher {
    /// A new one, with nothing read.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            weather: None,
            loaded: false,
            asked: None,
            failed: None,
            fetched_at: None,
            fetch: None,
            search: None,
            net: None,
            work: None,
            device: None,
        }
    }

    fn net(&mut self, host: &Host<'_>) -> Result<&Net, Error> {
        if self.net.is_none() {
            self.net = Some(Net::open(host)?);
        }
        self.net.as_ref().ok_or(Error::State)
    }

    fn work(&mut self, host: &Host<'_>) -> Result<&Work, Error> {
        if self.work.is_none() {
            self.work = Some(Work::open(host)?);
        }
        self.work.as_ref().ok_or(Error::State)
    }

    fn device(&mut self, host: &Host<'_>) -> Result<&Device, Error> {
        if self.device.is_none() {
            self.device = Some(Device::open(host)?);
        }
        self.device.as_ref().ok_or(Error::State)
    }

    /// The weather kept, read from its file once. A file that is too large or
    /// bad counts as none (and is written over by the next fetch).
    fn load(&mut self, host: &Host<'_>) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        self.weather = files::read_all(host, Root::Data, FILE, KEPT_CAP)
            .ok()
            .flatten()
            .and_then(|bytes| Weather::from_file(&bytes).ok().flatten());
    }

    /// The weather kept for `coordinates`, if any.
    fn weather_for(&self, coordinates: &str) -> Option<&Weather> {
        self.weather
            .as_ref()
            .filter(|weather| weather.coordinates == coordinates)
    }

    /// How old the weather for `coordinates` is, in seconds, if that is known.
    fn age(&self, coordinates: &str, now: u64, unix: Option<i64>) -> Option<i64> {
        let fetched = self.weather_for(coordinates).map_or(0, |w| w.fetched);
        match (unix, fetched) {
            (Some(unix), fetched) if fetched > 0 => Some(unix - fetched),
            _ => self
                .fetched_at
                .map(|at| i64::try_from(now.saturating_sub(at) / 1000).unwrap_or(i64::MAX)),
        }
    }

    /// Whether the network can be used now: where PaperPaw manages Wi-Fi, only
    /// while it is in a network (the radio is not switched on for the weather).
    fn online(&mut self, host: &Host<'_>) -> bool {
        let Ok(mut input) = mem::buffer(256) else {
            return false;
        };
        let Ok(mut out) = mem::buffer(512) else {
            return false;
        };
        let Ok(device) = self.device(host) else {
            return self.net(host).is_ok();
        };
        match device.wifi(host, &mut input, &mut out) {
            Ok(info) => matches!(
                info.state(),
                paperpaw_sdk::native::device::WifiState::Online
                    | paperpaw_sdk::native::device::WifiState::Hosting
            ),
            // No radio of PaperPaw's: a network is there when the service is.
            Err(_) => self.net(host).is_ok(),
        }
    }

    /// The interval between fetches the power mode allows, in seconds; `None`
    /// in the offline mode (nothing by itself).
    fn power_every(&mut self, host: &Host<'_>, base: i64) -> Result<Option<i64>, Error> {
        let base_ms = u32::try_from(base.saturating_mul(1000)).unwrap_or(u32::MAX);
        let mut input = mem::buffer(256)?;
        let mut out = mem::buffer(512)?;
        let device = self.device(host)?;
        let info = device.power(host, base_ms, &mut input, &mut out)?;
        if info.level() == PowerLevel::Offline {
            return Ok(None);
        }
        Ok(Some(i64::from(info.every_ms() / 1000).max(base)))
    }

    /// Looks whether the weather is to be fetched, sets the timer for the next
    /// look, and sends the fetch if `why` calls for it. `placed`: a weather
    /// widget is on a page (or a screen of widgets).
    ///
    /// # Errors
    /// `NoMemory`; what the services say. The timer failing is no error.
    pub fn check(&mut self, host: &Host<'_>, why: Why, placed: bool) -> Result<Looked, Error> {
        let coordinates = setting(host, COORDINATES)?;
        let Some(url) = model::forecast_url(&coordinates)? else {
            return Ok(Looked::Nothing);
        };
        let base = model::every(&setting(host, EVERY)?);
        // Longer while saving; in Offline nothing by itself.
        let every = match self.power_every(host, base) {
            Ok(Some(every)) => every,
            Ok(None) if why == Why::Due => return Ok(Looked::Nothing),
            Ok(None) | Err(_) => base,
        };
        let snapshot = capture_snapshot(host)?;
        let now = snapshot.instant().0;
        self.load(host);
        let age = self.age(&coordinates, now, snapshot.unix_seconds());
        let new_place = self.weather_for(&coordinates).is_none();
        let next = model::until_due(age, every, 5 * 60);
        // The next look: once due, when the radio is on anyway; nothing wakes
        // the device or the radio for the weather. A timer that cannot be set
        // (a full table, the network turned off) costs only the next look.
        self.look_again(host, next).ok();
        if self
            .asked
            .is_some_and(|at| now.saturating_sub(at) < PATIENCE_MS)
        {
            return Ok(Looked::Nothing);
        }
        if why == Why::Due {
            let failed_lately = self
                .failed
                .is_some_and(|at| now.saturating_sub(at) < RETRY_MS);
            if !placed
                || failed_lately
                || !(new_place || model::due(age, every))
                || !self.online(host)
            {
                return Ok(Looked::Nothing);
            }
        }
        self.send(host, &url, coordinates, why == Why::Asked, now)?;
        Ok(Looked::Fetching {
            told: why == Why::Asked,
        })
    }

    /// Declares the work that looks again `seconds` from now.
    fn look_again(&mut self, host: &Host<'_>, seconds: i64) -> Result<(), Error> {
        let after = u32::try_from(seconds.saturating_mul(1000)).unwrap_or(u32::MAX);
        let spec = WorkSpec::when_cheap(WORK_KEY, WORK_TOKEN)?
            .after_ms(after.min(paperpaw_sdk::native::work::SPAN_MAX_MS))
            .at_moment(Moment::Wake)
            .at_moment(Moment::Online);
        let mut input = mem::buffer(256)?;
        let mut out = mem::buffer(64)?;
        self.work(host)?.declare(host, &spec, &mut input, &mut out)
    }

    fn send(
        &mut self,
        host: &Host<'_>,
        url: &str,
        coordinates: String,
        told: bool,
        now: u64,
    ) -> Result<(), Error> {
        let mut input = mem::buffer(REQUEST_INPUT)?;
        let mut out = mem::buffer(REPLY)?;
        let headers = [Header::new("User-Agent", USER_AGENT)?];
        let request = Request::get(FETCH_REQUEST, url)?
            .headers(&headers)?
            .max_body(MAX_BODY);
        self.net(host)?;
        // One at a time: an older one ends with its own `http_done`, which is
        // told apart by the id and ignored.
        if self.fetch.take().is_some() {
            self.cancel(host, FETCH_REQUEST);
        }
        let id = self
            .net(host)?
            .request(host, &request, &mut input, &mut out)?;
        self.fetch = Some(Fetch {
            id,
            coordinates,
            told,
        });
        self.asked = Some(now);
        Ok(())
    }

    /// The forecast's request ended: `None` for one that is not the one under
    /// way (cancelled, replaced, another place).
    ///
    /// # Errors
    /// `NoMemory`; what the services say while the answer is read.
    pub fn fetched(&mut self, host: &Host<'_>, done: &HttpDone) -> Result<Option<Fetched>, Error> {
        if done.token() != FETCH_REQUEST || self.fetch.as_ref().map(|f| f.id) != Some(done.id()) {
            return Ok(None);
        }
        let Some(Fetch {
            coordinates, told, ..
        }) = self.fetch.take()
        else {
            return Ok(None);
        };
        self.asked = None;
        if matches!(done.reason(), Reason::Cancelled | Reason::Replaced) {
            return Ok(None);
        }
        let snapshot = capture_snapshot(host)?;
        let unix = snapshot.unix_seconds();
        let weather = if done.succeeded() {
            let mut reading = Forecast::new();
            let read = self.read_body(host, done, |piece| reading.feed(piece));
            match read {
                Ok(()) => reading.finish(&coordinates, unix.unwrap_or(0))?,
                // A body that does not fit or cannot be read is no weather.
                Err(Error::NoMemory) => return Err(Error::NoMemory),
                Err(_) => None,
            }
        } else {
            None
        };
        let Some(weather) = weather else {
            self.failed = Some(snapshot.instant().0);
            return Ok(Some(Fetched::Failed { told }));
        };
        // Kept in its small file; not kept (a full card) still shows.
        if let Ok(text) = weather.to_text() {
            files::replace(host, Root::Data, FILE, text.as_bytes()).ok();
        }
        self.weather = Some(weather);
        self.loaded = true;
        self.failed = None;
        self.fetched_at = Some(snapshot.instant().0);
        Ok(Some(Fetched::Kept))
    }

    /// The place changed: the weather of another place goes. Whether there is
    /// a place to fetch for is the caller's to see (`check`).
    pub fn place_changed(&mut self, host: &Host<'_>, has_place: bool) {
        self.failed = None;
        self.asked = None;
        if self.fetch.take().is_some() {
            self.cancel(host, FETCH_REQUEST);
        }
        if !has_place {
            self.weather = None;
            self.loaded = true;
            self.fetched_at = None;
            storage::remove(host, Root::Data, FILE).ok();
        }
    }

    /// The body of the request that ended, in pieces of at most [`BODY_PART`].
    fn read_body(
        &mut self,
        host: &Host<'_>,
        done: &HttpDone,
        mut sink: impl FnMut(&[u8]) -> Result<(), Error>,
    ) -> Result<(), Error> {
        let length = usize::try_from(done.body_len()).map_err(|_| Error::NoSpace)?;
        let mut input = mem::buffer(REQUEST_INPUT)?;
        let mut out = mem::buffer(REPLY)?;
        let net = self.net(host)?;
        let mut offset = 0usize;
        while offset < length {
            let at = u32::try_from(offset).map_err(|_| Error::NoSpace)?;
            let part = net.body(host, done.id(), at, &mut input, &mut out)?;
            if part.is_empty() {
                break;
            }
            let take = part.len().min(BODY_PART).min(length - offset);
            sink(part.get(..take).ok_or(Error::Invalid)?)?;
            offset += take;
        }
        Ok(())
    }

    /// Cancels the requests of `token` (their ends are told apart and ignored).
    /// A host that cannot is no error: the end is ignored all the same.
    fn cancel(&mut self, host: &Host<'_>, token: u32) {
        if let (Ok(mut input), Ok(mut out)) = (mem::buffer(REQUEST_INPUT), mem::buffer(REPLY))
            && let Ok(net) = self.net(host)
        {
            net.cancel(host, token, &mut input, &mut out).ok();
        }
    }

    /// Whether the network service is there at all.
    pub fn has_network(&mut self, host: &Host<'_>) -> bool {
        self.net(host).is_ok()
    }

    /// Searches for the places called `name` (names in `language` where known).
    ///
    /// # Errors
    /// `Unsupported` without the network service; `NoMemory`; what the host says.
    pub fn search(&mut self, host: &Host<'_>, name: &str, language: &str) -> Result<(), Error> {
        let url = model::search_url(name, language)?;
        let mut input = mem::buffer(REQUEST_INPUT)?;
        let mut out = mem::buffer(REPLY)?;
        let headers = [Header::new("User-Agent", USER_AGENT)?];
        let request = Request::get(SEARCH_REQUEST, &url)?
            .headers(&headers)?
            .max_body(MAX_BODY);
        self.net(host)?;
        if self.search.take().is_some() {
            self.cancel(host, SEARCH_REQUEST);
        }
        self.search = Some(
            self.net(host)?
                .request(host, &request, &mut input, &mut out)?,
        );
        Ok(())
    }

    /// Stops the search under way (its end is told apart and ignored).
    pub fn cancel_search(&mut self, host: &Host<'_>) {
        if self.search.take().is_some() {
            self.cancel(host, SEARCH_REQUEST);
        }
    }

    /// The search's request ended; `None` for one that is not the one under way.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn searched(
        &mut self,
        host: &Host<'_>,
        done: &HttpDone,
    ) -> Result<Option<Searched>, Error> {
        if done.token() != SEARCH_REQUEST || self.search != Some(done.id()) {
            return Ok(None);
        }
        self.search = None;
        match done.reason() {
            Reason::Cancelled | Reason::Replaced => return Ok(None),
            Reason::Offline => return Ok(Some(Searched::Offline)),
            _ => {}
        }
        if !done.succeeded() {
            return Ok(Some(Searched::Failed));
        }
        let mut reading = Places::new();
        match self.read_body(host, done, |piece| reading.feed(piece)) {
            Ok(()) => reading.finish().map(|places| Some(Searched::Found(places))),
            Err(Error::NoMemory) => Err(Error::NoMemory),
            Err(_) => Ok(Some(Searched::Failed)),
        }
    }

    /// What the widgets show besides the weather, read now. Notes nothing; a
    /// widget that is drawn is no reason to fetch.
    ///
    /// # Errors
    /// `NoMemory`; what the settings and the clock say.
    pub fn context(&mut self, host: &Host<'_>) -> Result<Context, Error> {
        self.load(host);
        let coordinates = setting(host, COORDINATES)?;
        let place = setting(host, PLACE)?;
        let snapshot = capture_snapshot(host)?;
        let now = snapshot.instant().0;
        let no_place = model::split_coordinates(&coordinates).is_none();
        let online = if no_place || self.fetch.is_some() || self.failed.is_some() {
            false
        } else {
            self.online(host)
        };
        let waiting = if no_place {
            Waiting::NoPlace
        } else if self.fetch.is_some() {
            Waiting::Fetching
        } else if self.failed.is_some() {
            Waiting::Failed
        } else if online {
            Waiting::Fetching
        } else {
            Waiting::NoNetwork
        };
        let every = model::every(&setting(host, EVERY)?);
        let has = self.weather_for(&coordinates).is_some();
        // Its age, in whole minutes, so that the minutes between changes draw
        // nothing anew.
        let old = self
            .age(&coordinates, now, snapshot.unix_seconds())
            .filter(|&age| has && model::is_old(age, every))
            .map(|age| age / 60 * 60);
        let today = paperpaw_ui_kit::widget::cx::local_time_in(host, &snapshot)?
            .map(|t| model::days_from_civil(t.year, t.month, t.day))
            .and_then(|days| i32::try_from(days).ok());
        Ok(Context {
            waiting,
            place,
            coordinates,
            old,
            today,
        })
    }

    /// What the widgets show: the weather kept, or why there is none.
    #[must_use]
    pub fn shown<'a>(&'a self, context: &'a Context) -> Shown<'a> {
        let weather = if context.waiting == Waiting::NoPlace {
            None
        } else {
            self.weather_for(&context.coordinates)
        };
        let mut shown = Shown::waiting(context.waiting);
        shown.weather = weather;
        shown.place = &context.place;
        shown.old = context.old;
        shown.today = context.today;
        shown
    }
}
