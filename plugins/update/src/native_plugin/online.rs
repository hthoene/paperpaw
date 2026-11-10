//! The online flow of the page and of declared work: asking the update server
//! (`core.net`), reading its answer as it streams, keeping the update it names,
//! downloading it into the card's `updates` folder (the card flow takes over
//! from there), and the work that looks for updates by itself (`core.work`).
//! The decisions without the host are in `native_online`.
use super::{COMMANDS, NativeUpdate, PAGE, Running, View, fetch::Fetch};
use crate::{
    native_card::{MAX_NAME, UPDATE_DIR, percent},
    native_online::{
        self, Build, FIRST, Firmware, LAST, MAX_OFFER, OFFER, Offer, QUESTION, Reader, Trouble,
        counting, date_text, parse_date,
    },
    native_update::wait,
};
use alloc::string::String;
use paperpaw_sdk::native::{
    Error, Host,
    commands::CommandSender,
    device::DeviceCursor,
    files::Files,
    net::{
        BODY_PART, FileRoot, Header, HttpDone, HttpProgress, Net, REPLY, REQUEST_INPUT, Reason,
        Request, RequestId,
    },
    registry::{PageCursor, Registry, RegistryPluginKind},
    snapshot::capture_snapshot,
    storage::{self, Root},
    ui::Notify,
    work::{Moment, SPAN_MAX_MS, Work, WorkSpec},
};
use paperpaw_ui_kit::{
    files::{read_all, replace},
    mem::{SERVICE_BUFFER, buffer},
    text::{concat, copy},
    widget::settings::text_of,
};

/// The plugin's own numbers of its requests, in their `http_done` events.
pub const ASK: u32 = 1;
pub const LOAD: u32 = 2;
pub const FETCH: u32 = 3;
/// The keys of the work: the weekly look, and a check that waits for a cheap moment.
pub const LOOK_KEY: &str = "update.look-due";
pub const CHECK_KEY: &str = "update.check-due";
/// When it looked last, in seconds since 1970, where the clock was known.
const LOOKED: &str = "looked.txt";
/// The settings.
const SERVER: &str = "update.server";
pub const LOOK: &str = "update.look";
const BETAS: &str = "update.betas";
/// The longest answer read: below what a memory body is on the X4 (45,056).
const MAX_ANSWER: u32 = 40 * 1024;
const WEEK_MS: u32 = 7 * 24 * 3600 * 1000;
/// A check from the action comes within this long, at a moment the network is up.
const CHECK_WITHIN_MS: u32 = 3_600_000;
/// Pages of the plugin lists read at most.
const MOST_PAGES: usize = 8;

/// The update file being downloaded.
#[derive(Debug)]
pub struct Loading {
    id: RequestId,
    pub version: String,
    size: u64,
    /// Where it is saved on the card.
    path: String,
    /// The builds of code plugins the offer named, fetched once it is here.
    builds: alloc::vec::Vec<Build>,
    pub percent: Option<i64>,
}

/// The handles of the services and what is in flight.
#[derive(Debug, Default)]
pub struct Online {
    net: Option<Net>,
    work: Option<Work>,
    registry: Option<Registry>,
    /// The update server's question in flight.
    asking: Option<(RequestId, bool)>,
    pub loading: Option<Loading>,
    pub fetch: Option<Fetch>,
}

impl Online {
    /// Whether the firmware or the plugins for it are being downloaded.
    pub const fn downloading(&self) -> bool {
        self.loading.is_some() || self.fetch.is_some()
    }

    /// Whether a reader waits for the answer (the page asked for it).
    pub const fn awaited(&self) -> bool {
        matches!(self.asking, Some((_, false)))
    }

    /// The version being downloaded, and how far the firmware is (the plugins
    /// for it come after, with no percent).
    pub fn progress(&self) -> (&str, Option<i64>) {
        match (&self.loading, &self.fetch) {
            (Some(loading), _) => (&loading.version, loading.percent),
            (None, Some(fetch)) => (&fetch.version, None),
            (None, None) => ("", None),
        }
    }

    /// The service handle of `core.net`.
    pub fn net(&mut self, host: &Host<'_>) -> Result<&Net, Error> {
        if self.net.is_none() {
            self.net = Some(Net::open(host)?);
        }
        self.net.as_ref().ok_or(Error::State)
    }

    pub(super) fn work(&mut self, host: &Host<'_>) -> Result<&Work, Error> {
        if self.work.is_none() {
            self.work = Some(Work::open(host)?);
        }
        self.work.as_ref().ok_or(Error::State)
    }

    pub fn registry(&mut self, host: &Host<'_>) -> Result<&Registry, Error> {
        if self.registry.is_none() {
            self.registry = Some(Registry::open(host)?);
        }
        self.registry.as_ref().ok_or(Error::State)
    }

    /// The body of the request that ended, in pieces of at most [`BODY_PART`].
    pub fn read_body(
        &mut self,
        host: &Host<'_>,
        done: &HttpDone,
        mut sink: impl FnMut(&[u8]) -> Result<(), Error>,
    ) -> Result<(), Error> {
        let length = usize::try_from(done.body_len()).map_err(|_| Error::NoSpace)?;
        let (mut input, mut out) = (buffer(REQUEST_INPUT)?, buffer(REPLY)?);
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
}

/// The text of the setting `key`, trimmed (settings are short).
fn setting(host: &Host<'_>, key: &str) -> Result<String, Error> {
    text_of(host, key, |text| copy(text.trim()))?
}

/// Whether the switch `key` is on.
pub fn flag(host: &Host<'_>, key: &str) -> Result<bool, Error> {
    let mut bytes = [0; 64];
    Ok(matches!(
        paperpaw_sdk::native::settings::read_setting(host, key, &mut bytes)?,
        paperpaw_sdk::native::settings::SettingValue::Bool(true)
    ))
}

/// A device without declared work has no look: only a failure to allocate is one.
pub fn tolerate(result: Result<(), Error>) -> Result<(), Error> {
    match result {
        Err(Error::NoMemory) => Err(Error::NoMemory),
        _ => Ok(()),
    }
}

/// Says `text` where the reader is.
fn tell(host: &Host<'_>, text: &str) -> Result<(), Error> {
    let mut commands = buffer(COMMANDS)?;
    match CommandSender::new(host, &mut commands).notify(Notify::new(text)?) {
        // In a run of declared work nothing is said (the plugin's note takes it).
        Err(Error::State) => Ok(()),
        other => other,
    }
}

impl NativeUpdate {
    /// Asks the update server for a newer firmware; `quiet` where the reader did
    /// not ask (declared work). A question already in flight is not asked twice.
    pub(super) fn search(&mut self, host: &Host<'_>, quiet: bool) -> Result<(), Error> {
        self.searched = false;
        // A question in flight is not asked twice; a reader who asks meanwhile
        // waits for its answer.
        if let Some((_, waited)) = self.online.asking.as_mut() {
            *waited &= quiet;
            return self.show_if(host, View::Searching);
        }
        let Some(running) = self.firmware(host)? else {
            return Ok(());
        };
        match self.ask(host, &running, quiet) {
            Ok(()) => self.show_if(host, View::Searching),
            Err(Error::NoMemory) => Err(Error::NoMemory),
            // No network at all, none for this plugin, or the question refused.
            Err(Error::Unsupported | Error::Denied) => {
                self.show_if(host, View::Unreachable(Trouble::Reason("update.net.held")))
            }
            Err(_) => self.show_if(host, View::Unreachable(Trouble::Reason("update.net.other"))),
        }
    }

    fn ask(&mut self, host: &Host<'_>, running: &Running, quiet: bool) -> Result<(), Error> {
        let body = self.question(host, running, quiet)?;
        replace(host, Root::Data, QUESTION, body.as_bytes())?;
        drop(body);
        let server = setting(host, SERVER)?;
        let url = concat(&[native_online::server(&server), "/api/v1/updates"])?;
        let headers = [Header::new("content-type", "application/json")?];
        let request = Request::new(ASK, "POST", &url)?
            .headers(&headers)?
            .send_file(FileRoot::Data, QUESTION)?
            .max_body(MAX_ANSWER);
        let (mut input, mut out) = (buffer(REQUEST_INPUT)?, buffer(REPLY)?);
        let id = self
            .online
            .net(host)?
            .request(host, &request, &mut input, &mut out)?;
        self.online.asking = Some((id, quiet));
        // An update that waits is asked for again not before a day.
        tolerate(self.waiting_tried(host))
    }

    /// The question: what runs, what is built in, the code plugins installed, the
    /// interface they are built for, and the counting. The check counts when it
    /// is asked, whatever its answer.
    fn question(
        &mut self,
        host: &Host<'_>,
        running: &Running,
        quiet: bool,
    ) -> Result<String, Error> {
        let device = Firmware {
            project: &running.project,
            chip: running.chip,
            version: &running.version,
        };
        let mut question = native_online::Question::new(&device)?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let mut cursor = DeviceCursor::start();
        let dev = self.device(host)?;
        for _ in 0..MOST_PAGES {
            let page = match dev.builtins(host, cursor, &mut input, &mut out) {
                Ok(page) => page,
                Err(Error::NotFound) => break,
                Err(error) => return Err(error),
            };
            for plugin in page.rows() {
                question.built_in(plugin.id(), plugin.version())?;
            }
            match page.end().next_cursor() {
                Some(next) => cursor = next,
                None => break,
            }
        }
        let interface = match dev.native(host, &mut input, &mut out) {
            Ok(native) => Some((native.abi_major(), native.abi_draft())),
            Err(Error::NotFound) => None,
            Err(error) => return Err(error),
        };
        question.code_begin()?;
        self.code_plugins(host, |id, version| question.code(id, version))?;
        let betas = flag(host, BETAS)?;
        let today = capture_snapshot(host)?
            .unix_seconds()
            .map(|unix| unix.div_euclid(86_400));
        let date = |name: &str| -> Result<Option<i64>, Error> {
            Ok(read_all(host, Root::Data, name, 64)?.and_then(|bytes| parse_date(&bytes)))
        };
        let (counted, keep) = counting(quiet, today, date(LAST)?, date(FIRST)?);
        for (name, days) in [(LAST, keep.last), (FIRST, keep.first)] {
            if let Some(days) = days {
                replace(host, Root::Data, name, date_text(days)?.as_bytes())?;
            }
        }
        question.finish(interface, betas, &counted)
    }

    /// Calls `each` with the id and the version of every code plugin installed.
    pub(super) fn code_plugins(
        &mut self,
        host: &Host<'_>,
        mut each: impl FnMut(&str, &str) -> Result<(), Error>,
    ) -> Result<(), Error> {
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let registry = self.online.registry(host)?;
        let mut cursor = PageCursor::start();
        for _ in 0..MOST_PAGES {
            let page = registry.plugins(host, cursor, &mut input, &mut out)?;
            for plugin in page.rows() {
                if plugin.kind() == RegistryPluginKind::Code {
                    each(plugin.id(), plugin.version())?;
                }
            }
            match page.end().next_cursor() {
                Some(next) => cursor = next,
                None => break,
            }
        }
        Ok(())
    }

    /// The answer came, or the request failed: the update it names kept (or the
    /// one kept before let go), and shown where the page is open.
    pub(super) fn answered(&mut self, host: &Host<'_>, done: &HttpDone) -> Result<(), Error> {
        if self
            .online
            .asking
            .take_if(|(id, _)| *id == done.id())
            .is_none()
        {
            return Ok(());
        }
        // The question has been sent: it is not kept.
        storage::remove(host, Root::Data, QUESTION).ok();
        if matches!(done.reason(), Reason::Cancelled | Reason::Replaced) {
            return Ok(());
        }
        let Some(running) = self.firmware(host)? else {
            return Ok(());
        };
        if !done.succeeded() || done.truncated() {
            let trouble = if done.truncated() && done.succeeded() {
                Trouble::Reason("update.net.other")
            } else {
                Trouble::of(done.reason(), done.status())
            };
            tolerate(self.waiting_failed(host))?;
            return self.show_if(host, View::Unreachable(trouble));
        }
        let mut reader = Reader::default();
        self.online
            .read_body(host, done, |piece| reader.feed(piece))?;
        let offer = reader.finish(&running.version, flag(host, BETAS)?)?;
        match &offer {
            Some(offer) => {
                let text = offer.to_text()?;
                replace(host, Root::Data, OFFER, text.as_bytes())?;
                // Where it waits for plugins: kept, and asked for again in a day.
                tolerate(self.waiting_answered(host, offer))?;
            }
            None => {
                storage::remove(host, Root::Data, OFFER).ok();
                self.waiting_none(host);
            }
        }
        // The *New* mark follows the offer kept.
        tolerate(Self::mark_new(host))?;
        // Where nobody reads (a question asked in the background), the offer
        // waits on the page.
        if offer.is_some() {
            self.refresh(host)
        } else {
            self.up_to_date(host)
        }
    }

    /// The server knows no newer update: said, unless the card has one to install.
    fn up_to_date(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if self.screens.top(PAGE).is_none() {
            return Ok(());
        }
        let mut page = self.look(host)?;
        if super::no_news(&page.view) {
            page = super::build(host, View::UpToDate, &self.current, self.caps)?;
        }
        // The card's files are still being checked: what they find is told so too.
        self.searched = matches!(page.view, View::Checking);
        self.show_page(host, page)
    }

    /// The update the server named last, if it is still newer than `running`.
    pub(super) fn offer(host: &Host<'_>, running: &str) -> Result<Option<Offer>, Error> {
        let Some(bytes) = read_all(host, Root::Data, OFFER, MAX_OFFER)? else {
            return Ok(None);
        };
        let Ok(text) = core::str::from_utf8(&bytes) else {
            return Ok(None);
        };
        Ok(Offer::from_text(text)?.filter(|offer| {
            paperpaw_sdk::firmware::compare_versions(&offer.version, running).is_gt()
        }))
    }

    /// Downloads the update `offer` names into `updates`, where the card flow
    /// finds it once it is whole.
    pub(super) fn download(&mut self, host: &Host<'_>, offer: Offer) -> Result<(), Error> {
        let Some(running) = self.firmware(host)? else {
            return self.refresh(host);
        };
        let path = concat(&[
            UPDATE_DIR,
            "/",
            &running.project,
            "-",
            &offer.version,
            ".bin",
        ])?;
        if path.len() > UPDATE_DIR.len() + 1 + MAX_NAME {
            return self.download_failed(host);
        }
        let started = self.start_download(host, &offer, &path);
        let id = match started {
            Ok(id) => id,
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => return self.download_failed(host),
        };
        let wanted = self.wanted(host, &offer.builds)?;
        let loading = Loading {
            id,
            version: offer.version,
            size: offer.size,
            path,
            builds: wanted,
            percent: None,
        };
        self.online.loading = Some(loading);
        self.show_downloading(host)
    }

    /// Shows the download on the page that is open, if one is.
    pub(super) fn show_downloading(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if self.screens.top(PAGE).is_none() {
            return Ok(());
        }
        let view = self.downloading()?;
        self.show(host, view)
    }

    fn start_download(
        &mut self,
        host: &Host<'_>,
        offer: &Offer,
        path: &str,
    ) -> Result<RequestId, Error> {
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let files = Files::open(host)?;
        let made = files.make_dir(host, Root::Files, UPDATE_DIR, &mut input, &mut out);
        files.release(host)?;
        made?;
        let server = setting(host, SERVER)?;
        let url = native_online::address(native_online::server(&server), &offer.url)?;
        let mut request = Request::get(LOAD, &url)?
            .save_to(FileRoot::Files, path)?
            .progress();
        if offer.size > 0 {
            request = request.max_file(offer.size)?;
        }
        let (mut input, mut out) = (buffer(REQUEST_INPUT)?, buffer(REPLY)?);
        self.online
            .net(host)?
            .request(host, &request, &mut input, &mut out)
    }

    /// The download's progress: the percent of the page.
    pub(super) fn load_progress(
        &mut self,
        host: &Host<'_>,
        progress: &HttpProgress,
    ) -> Result<(), Error> {
        let Some(loading) = self.online.loading.as_mut() else {
            return Ok(());
        };
        if loading.id != progress.id() {
            return Ok(());
        }
        let total = progress.total().unwrap_or(loading.size);
        let now = percent(progress.received(), total);
        if now != loading.percent {
            loading.percent = now;
            self.show_downloading(host)?;
        }
        Ok(())
    }

    /// The download ended: whole, it is an update file of the card; otherwise it
    /// is removed and said so.
    pub(super) fn loaded(&mut self, host: &Host<'_>, done: &HttpDone) -> Result<(), Error> {
        let Some(loading) = self
            .online
            .loading
            .take_if(|loading| loading.id == done.id())
        else {
            return Ok(());
        };
        if matches!(done.reason(), Reason::Cancelled | Reason::Replaced) {
            return self.refresh(host);
        }
        let arrived = done.succeeded() && done.saved();
        // A failed request leaves an older file of that name as it was.
        if !arrived || !Self::whole(host, &loading)? {
            if arrived {
                storage::remove(host, Root::Files, &loading.path).ok();
            }
            return self.download_failed(host);
        }
        storage::remove(host, Root::Data, OFFER).ok();
        tolerate(Self::mark_new(host))?;
        tolerate(self.waiting_done(host))?;
        let builds = loading.builds;
        if !builds.is_empty() {
            return self.fetch_begin(host, &loading.version, builds);
        }
        self.downloaded(host, &loading.version)
    }

    /// Whether the saved file is as large as the server said (where it said).
    fn whole(host: &Host<'_>, loading: &Loading) -> Result<bool, Error> {
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let files = Files::open(host)?;
        let stat = files
            .stat(host, Root::Files, &loading.path, &mut input, &mut out)
            .map(|entry| entry.size());
        files.release(host)?;
        Ok(match stat {
            Ok(size) => size > 0 && (loading.size == 0 || size == loading.size),
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => false,
        })
    }

    /// The update is in `updates`: said, and the card's files checked again, so
    /// that the page offers it.
    pub(super) fn downloaded(&mut self, host: &Host<'_>, version: &str) -> Result<(), Error> {
        let mut messages = paperpaw_ui_kit::Messages::new()?;
        let shown = super::shown_version(host, &mut messages, version)?;
        let text = messages.format(
            host,
            "update.downloaded",
            &[paperpaw_sdk::native::formatting::Arg::Text {
                name: "version",
                value: &shown,
            }],
        )?;
        tell(host, &text)?;
        self.refresh(host)
    }

    pub(super) fn download_failed(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let mut messages = paperpaw_ui_kit::Messages::new()?;
        let text = messages.copy(host, "update.download-failed.title")?;
        tell(host, &text)?;
        self.refresh(host)
    }

    /// Shows what there is now on the page, if it is open: the files of the card
    /// and the update the server named.
    pub(super) fn refresh(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if self.screens.top(PAGE).is_none() {
            return Ok(());
        }
        let page = self.look(host)?;
        self.show_page(host, page)
    }

    /// Shows `view` on the page that is open, if one is.
    pub(super) fn show_if(&mut self, host: &Host<'_>, view: View) -> Result<(), Error> {
        if self.screens.top(PAGE).is_none() {
            return Ok(());
        }
        self.show(host, view)
    }

    /// The end of a request of this flow, whichever it is.
    pub(super) fn http_done(&mut self, host: &Host<'_>, done: &HttpDone) -> Result<(), Error> {
        match done.token() {
            ASK => self.answered(host, done),
            LOAD => self.loaded(host, done),
            _ => self.fetched(host, done),
        }
    }

    /// Declares the weekly look after what is left of the week since the last
    /// one, or takes it back where the setting is off. Declared again it
    /// replaces itself. A work that cannot be declared (a full table, the network
    /// turned off) costs this week's look.
    pub(super) fn plan_look(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if self.firmware(host)?.is_none() {
            return Ok(());
        }
        let (mut input, mut out) = (buffer(256)?, buffer(64)?);
        if !flag(host, LOOK)? {
            let work = self.online.work(host)?;
            return work.cancel(host, LOOK_KEY, &mut input, &mut out);
        }
        let now = capture_snapshot(host)?.unix_seconds();
        // A look that left no readable date counts as none: the week starts now.
        let last = match read_all(host, Root::Data, LOOKED, 64) {
            Ok(kept) => {
                kept.and_then(|bytes| core::str::from_utf8(&bytes).ok()?.trim().parse().ok())
            }
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => None,
        };
        let after = u32::try_from(wait(last, now).saturating_mul(1000))
            .unwrap_or(u32::MAX)
            .min(SPAN_MAX_MS);
        let spec = WorkSpec::when_cheap(LOOK_KEY, super::WORK_DUE)?
            .after_ms(after)
            .every_ms(WEEK_MS)
            .at_moment(Moment::Online)
            .at_moment(Moment::Charging);
        let declared = self
            .online
            .work(host)?
            .declare(host, &spec, &mut input, &mut out);
        match declared {
            Err(Error::Full | Error::Denied | Error::Unsupported) => Ok(()),
            other => other,
        }
    }

    /// A check at a cheap moment (the action `update.check`, which a rule runs).
    pub(super) fn check_later(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let spec = WorkSpec::new(CHECK_KEY, super::WORK_DUE)?
            .network()
            .within_ms(CHECK_WITHIN_MS)
            .at_moment(Moment::Online);
        let (mut input, mut out) = (buffer(256)?, buffer(64)?);
        match self
            .online
            .work(host)?
            .declare(host, &spec, &mut input, &mut out)
        {
            Err(Error::Full | Error::Denied | Error::Unsupported) => Ok(()),
            other => other,
        }
    }

    /// Declared work came due: the weekly look (counted from now on) or a check.
    pub(super) fn work_due(&mut self, host: &Host<'_>, key: &str) -> Result<(), Error> {
        if key == LOOK_KEY {
            if !flag(host, LOOK)? {
                return Ok(());
            }
            if let Some(now) = capture_snapshot(host)?.unix_seconds() {
                let mut text = String::new();
                paperpaw_ui_kit::text::write_to(&mut text, format_args!("{now}\n"))?;
                replace(host, Root::Data, LOOKED, text.as_bytes())?;
            }
        } else if key != CHECK_KEY {
            return Ok(());
        }
        self.search(host, true)
    }

    /// A setting changed: the weekly look is declared anew or taken back.
    pub(super) fn setting_changed(&mut self, host: &Host<'_>, key: &str) -> Result<(), Error> {
        if key == LOOK {
            return tolerate(self.plan_look(host));
        }
        Ok(())
    }
}
