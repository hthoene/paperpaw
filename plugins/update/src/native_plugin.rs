//! The interface-7 update page for the card flow: the update files in the
//! card's `updates` folder, which the device checks one after another (version,
//! who signed it, newer, older, the same or another firmware, `native_check`),
//! the install the device checks again and asks the reader about (decisions 132
//! and 134), how it ended, and what is said after the restart. The online check,
//! the downloads, the plugin fetch and the weekly look are in `online`, `fetch`
//! and `waiting`; the plugins an update leaves out and the *New* mark are in
//! `leaves`. What is not native yet (the dual-boot notice) is named in
//! `PORTING-7.md`.
use crate::{
    declarations::{
        CHECK_TOKEN, FIRMWARE_CHANGED, HTTP_DONE, HTTP_PROGRESS, INSTALL_DONE, OPEN_TOKEN, PAGE,
        SETTING_CHANGED, STARTED, WORK_DUE,
    },
    native_card::{
        self, CHECK_REQUEST, Ended, MAX_PAIR, Offered, REQUEST, UPDATE_DIR, ended, percent,
        read_pair, reason,
    },
    native_check::{self, Evidence, Found, Image, Other, Signer},
    native_leaves::{self, Recheck, Retry, Turn},
    native_online::{Offer, Trouble},
};
use alloc::{string::String, vec::Vec};
use core::cmp::Ordering;
use paperpaw_sdk::firmware::compare_versions;
use paperpaw_sdk::{
    Handled,
    native::{
        Error, Host, Plugin,
        callbacks::{DrawFrame, Input},
        commands::{CheckKind, CommandSender, FirmwareOptions, InstallRoot, RedrawMode},
        device::{
            Device, DeviceCapability, DeviceOutcome, DeviceRequest, FirmwareState, InstallKind,
        },
        events::{Event, Incoming, InstallDone, kinds},
        files::Files,
        formatting::Arg,
        registrations,
        registry::PageCursor,
        reply::Reply,
        storage::{self, Root},
        ui::{Notify, Open},
    },
};
use paperpaw_ui_kit::{
    Footer, Item, ItemList, ItemStep, Messages, Screens,
    files::{read_all, replace},
    mem::{SERVICE_BUFFER, buffer, push},
    read_event,
    text::{concat, copy},
};

mod fetch;
mod leaves;
mod online;
mod waiting;

mod image {
    include!(concat!(env!("OUT_DIR"), "/update_declarations.rs"));
}

/// The data file that says an install went through, kept through the restart
/// (interface 6's, `docs/data-files.md`).
const INSTALLING: &str = "installing.txt";
/// The data file of the update offered at start last (path and version): an
/// update is offered once (interface 6's, `docs/data-files.md`).
const OFFERED: &str = "offered.txt";
/// Room for the commands of one callback (an install's path is up to 512 bytes).
const COMMANDS: usize = 1024;
/// Pages of the update folder read at most.
const MOST_PAGES: usize = 16;
/// Wi-Fi transfer's action: opens its screen.
const TRANSFER: &str = "transfer.open";

/// What the page shows.
#[derive(Debug, PartialEq, Eq)]
enum View {
    /// No update file on the card.
    Nothing,
    /// The device installs no firmware updates here.
    Unsupported,
    /// The device checks the update files of the card.
    Checking,
    /// The newest update that fits, with what the check said of it.
    Ready(Image),
    /// Another firmware's image on the card: the reader's choice, told so.
    Foreign(Other),
    /// No file of the card can be installed, for this reason.
    Unfit(DeviceOutcome),
    /// The device checks, asks and writes; how far, where it says.
    Installing(Option<i64>),
    /// In place; with a button to restart where the plugin did not do it.
    Installed(bool),
    /// In place, but the host refused to restart: says so, and the button asks again.
    RestartFailed,
    /// Not installed, for this reason.
    Failed(DeviceOutcome),
    /// The update server is asked.
    Searching,
    /// The server knows no newer update.
    UpToDate,
    /// The server could not be asked, or answered with an error.
    Unreachable(Trouble),
    /// An update the server named, to download, and the plugins built in now it
    /// leaves out (their names, empty for none).
    Offer(Offer, String),
    /// An update the server named that waits for plugins.
    Waiting(waiting::Waited),
    /// The same, which the reader chose to download anyway.
    Anyway(waiting::Waited),
    /// The update (then the plugins for it) is downloaded: its version, how far.
    Downloading(String, Option<i64>),
}

/// What a row of the page does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Button {
    /// Installs the file the page shows.
    Install,
    Restart,
    Close,
    /// Asks the update server.
    Search,
    /// Downloads the update the server named.
    Download,
    /// Opens Wi-Fi transfer, to put an update on the card.
    Transfer,
}

/// What the device offers the page: a network, and Wi-Fi transfer.
#[derive(Debug, Clone, Copy, Default)]
struct Caps {
    network: bool,
    transfer: bool,
}

#[derive(Debug)]
struct Page {
    view: View,
    buttons: Vec<Button>,
    list: ItemList,
    title: String,
    help: String,
}

/// The install in flight: the file the reader chose, as the check said it.
#[derive(Debug)]
struct Job {
    found: Found,
}

/// The check of the card's files, one at a time (the device runs one check at a
/// time and holds at most four requests).
#[derive(Debug)]
struct Scan {
    names: Vec<String>,
    found: Vec<Found>,
    /// The file whose check is running.
    next: usize,
    /// Whether a newer update is offered when this ends (at start).
    offer: bool,
    /// The re-submits of the check of `next`: when the list of what its image
    /// leaves out was asked while a check was running (op 16, `ERR_BUSY`) the
    /// file waits for that check to end and its check is submitted again; after
    /// a newer check replaced the result (`ERR_NOT_FOUND`) none runs, so it is
    /// submitted again at once (op 6 and op 16 answer only for the most recent
    /// check, so the old request is never read again), not offered and not
    /// given up before.
    recheck: Recheck,
}

/// How far the checks are.
enum Step {
    Running,
    /// All answered: the file to offer, and whether this one offers.
    Done(Option<Found>, bool),
}

/// What `core.device` says of the firmware.
#[derive(Debug)]
struct Running {
    version: String,
    project: String,
    chip: u32,
    /// The bytes of the app slot an update goes into.
    room: u64,
    state: FirmwareState,
    done: u64,
    total: u64,
    undone: bool,
}

/// The interface-7 update plugin.
#[derive(Debug, Default)]
pub struct NativeUpdate {
    device: Option<Device>,
    screens: Screens<Page>,
    job: Option<alloc::boxed::Box<Job>>,
    scan: Option<alloc::boxed::Box<Scan>>,
    /// What an offer at start found, for the page it opens.
    settled: Option<alloc::boxed::Box<Found>>,
    /// The version running, as the page says it.
    current: String,
    /// The version running, as the device says it.
    version: String,
    /// What the device offers the page.
    caps: Caps,
    /// The server was asked and knows no newer update: what the card's checks find
    /// is *up to date* where it is no news.
    searched: bool,
    /// The reader left for Wi-Fi transfer: back here, the card is looked at again.
    away: bool,
    /// The reader chose to update anyway, while it waits for plugins.
    anyway: bool,
    online: online::Online,
}

impl Plugin for NativeUpdate {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::SETTINGS
            .iter()
            .chain(image::SCREENS)
            .chain(image::NAVIGATION)
            .chain(image::EVENTS)
        {
            registrations::submit(host, batch)?;
        }
        Ok(Self::default())
    }

    fn event(
        &mut self,
        host: &Host<'_>,
        token: u32,
        incoming: Incoming<'_>,
        _: &mut Reply<'_>,
    ) -> Result<(), Error> {
        let Some(event) = read_event!(
            incoming;
            kinds::ActionRun,
            kinds::Started,
            kinds::InstallDone,
            kinds::FirmwareChanged,
            kinds::ScreenOpened,
            kinds::ScreenShown,
            kinds::ScreenClosed,
            kinds::HttpDone,
            kinds::HttpProgress,
            kinds::WorkDue,
            kinds::SettingChanged
        ) else {
            return Ok(());
        };
        match event {
            Event::ActionRun {
                action: "update.open",
                ..
            } if token == OPEN_TOKEN => {
                let mut scratch = [0; 128];
                CommandSender::new(host, &mut scratch).open(Open::screen("update.page")?)
            }
            // A rule's check: asked at a cheap moment, nobody waits for it.
            Event::ActionRun {
                action: "update.check",
                ..
            } if token == CHECK_TOKEN => self.check_later(host),
            Event::Started { .. } if token == STARTED => self.started(host),
            Event::InstallDone(done) if token == INSTALL_DONE => match done.kind() {
                InstallKind::Check => self.checked(host, &done),
                InstallKind::Plugin => self.fetch_install_done(host, &done),
                _ => self.install_done(host, &done),
            },
            Event::HttpDone(done) if token == HTTP_DONE => self.http_done(host, &done),
            Event::HttpProgress(progress) if token == HTTP_PROGRESS => {
                self.load_progress(host, &progress)
            }
            Event::WorkDue(due) if token == WORK_DUE => self.work_due(host, due.key()),
            Event::SettingChanged { key, .. } if token == SETTING_CHANGED => {
                self.setting_changed(host, key)
            }
            Event::FirmwareChanged { .. } if token == FIRMWARE_CHANGED => self.progress(host),
            Event::ScreenOpened { instance, .. } if token == PAGE => {
                self.anyway = false;
                let page = self.look(host)?;
                let seen = self.waiting_seen(host, &page.view);
                self.screens.open(instance, page)?;
                online::tolerate(seen)
            }
            Event::ScreenShown {
                instance: Some(instance),
                ..
            } if token == PAGE => {
                self.screens.shown(token, instance)?;
                // Back from Wi-Fi transfer: a file may have come.
                if core::mem::take(&mut self.away) {
                    self.refresh(host)?;
                }
                Ok(())
            }
            Event::ScreenClosed { instance, .. } => {
                self.screens.close(instance);
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn draw(&mut self, host: &Host<'_>, token: u32, mut frame: DrawFrame<'_>) -> Result<(), Error> {
        let page = self.screens.top(token).ok_or(Error::State)?;
        page.list.draw_with(
            host,
            &mut frame,
            "update.title",
            true,
            Footer::Empty {
                title: &page.title,
                text: &page.help,
            },
        )
    }

    fn input(&mut self, host: &Host<'_>, token: u32, input: Input<'_>) -> Result<Handled, Error> {
        let Some(page) = self.screens.top(token) else {
            return Ok(Handled::No);
        };
        let step = page.list.input(host, &input)?;
        if let ItemStep::Chosen(index) = step {
            let button = page.buttons.get(index).copied();
            return self.press(host, button).map(|()| Handled::Yes);
        }
        Ok(ItemList::handled(step))
    }
}

impl NativeUpdate {
    fn device(&mut self, host: &Host<'_>) -> Result<&Device, Error> {
        if self.device.is_none() {
            self.device = Some(Device::open(host)?);
        }
        self.device.as_ref().ok_or(Error::State)
    }

    /// The firmware as the device says it; none where it has no firmware updates.
    fn firmware(&mut self, host: &Host<'_>) -> Result<Option<Running>, Error> {
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        match self.device(host)?.firmware(host, &mut input, &mut out) {
            Ok(info) => Ok(Some(Running {
                version: copy(info.version())?,
                project: copy(info.project())?,
                chip: info.chip(),
                room: info.room(),
                state: info.state(),
                done: info.done(),
                total: info.total(),
                undone: info.undone(),
            })),
            Err(Error::NotFound) => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// What there is now: an install going on, one waiting for its restart, or
    /// the files on the card.
    #[inline(never)]
    fn look(&mut self, host: &Host<'_>) -> Result<Page, Error> {
        let Some(running) = self.firmware(host)? else {
            return build(host, View::Unsupported, "", Caps::default());
        };
        let mut messages = Messages::new()?;
        self.current = shown_version(host, &mut messages, &running.version)?;
        self.version = copy(&running.version)?;
        self.caps = self.read_caps(host)?;
        let view = match running.state {
            _ if self.job.is_some() => View::Installing(percent(running.done, running.total)),
            FirmwareState::Waiting | FirmwareState::Installing => {
                View::Installing(percent(running.done, running.total))
            }
            FirmwareState::Installed => View::Installed(true),
            _ if self.online.downloading() => self.downloading()?,
            _ if self.online.awaited() => View::Searching,
            _ => self.card(host)?,
        };
        build(host, view, &self.current, self.caps)
    }

    /// Whether the device has a network, and Wi-Fi transfer is there to open.
    fn read_caps(&mut self, host: &Host<'_>) -> Result<Caps, Error> {
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let network = self
            .device(host)?
            .device(host, &mut input, &mut out)?
            .supports(DeviceCapability::Network);
        let mut transfer = false;
        let registry = self.online.registry(host)?;
        let mut cursor = PageCursor::start();
        for _ in 0..MOST_PAGES {
            let page = registry.actions(host, "transfer", cursor, &mut input, &mut out)?;
            transfer |= page.rows().any(|action| action.id() == TRANSFER);
            match page.end().next_cursor() {
                Some(next) => cursor = next,
                None => break,
            }
        }
        Ok(Caps { network, transfer })
    }

    /// The download going on, as the page shows it.
    fn downloading(&self) -> Result<View, Error> {
        let (version, percent) = self.online.progress();
        Ok(View::Downloading(copy(version)?, percent))
    }

    /// What the card's files found and the update the server named, as the page
    /// shows them: an update on the card that is as new as the one the server
    /// named (or another firmware's image) comes first; else the server's.
    fn view_for(&mut self, host: &Host<'_>, found: Option<Found>) -> Result<View, Error> {
        let view = view_of(found);
        if matches!(view, View::Foreign(_)) {
            return Ok(view);
        }
        let Some(offer) = Self::offer(host, &self.version)? else {
            return Ok(view);
        };
        match view {
            View::Ready(image)
                if image.order.is_gt()
                    && !compare_versions(&image.version, &offer.version).is_lt() =>
            {
                Ok(View::Ready(image))
            }
            View::Nothing | View::Unfit(_) | View::Ready(_) => self.offer_view(host, offer),
            other => Ok(other),
        }
    }

    /// The card's files: what an offer at start found, a check going on, or the
    /// checks of the files there are now.
    fn card(&mut self, host: &Host<'_>) -> Result<View, Error> {
        if let Some(found) = self.settled.take() {
            return self.view_for(host, Some(*found));
        }
        if self.scan.is_some() {
            return Ok(View::Checking);
        }
        match self.begin(host, false)? {
            Step::Running => Ok(View::Checking),
            Step::Done(found, _) => self.view_for(host, found),
        }
    }

    /// Starts the checks of the files of `updates`; they end with `Done`
    /// at once where there are none.
    fn begin(&mut self, host: &Host<'_>, offer: bool) -> Result<Step, Error> {
        let names = card_files(host)?;
        if names.is_empty() {
            return Ok(Step::Done(None, offer));
        }
        self.scan = Some(paperpaw_ui_kit::mem::boxed(Scan {
            names,
            found: Vec::new(),
            next: 0,
            offer,
            recheck: Recheck::default(),
        })?);
        self.advance(host)
    }

    /// Sends the check of the next file; a file that cannot be asked about
    /// counts as one that failed. A failure drops the scan.
    fn advance(&mut self, host: &Host<'_>) -> Result<Step, Error> {
        let step = self.send_next(host);
        if step.is_err() {
            self.scan = None;
        }
        step
    }

    fn send_next(&mut self, host: &Host<'_>) -> Result<Step, Error> {
        loop {
            let Some(scan) = self.scan.as_mut() else {
                return Ok(Step::Done(None, false));
            };
            let Some(name) = scan.names.get(scan.next) else {
                let offer = scan.offer;
                let found = self.scan.take().map(|scan| scan.found);
                return Ok(Step::Done(found.and_then(native_check::choose), offer));
            };
            let path = native_card::path(name)?;
            let request = u32::try_from(scan.next)
                .ok()
                .and_then(|next| CHECK_REQUEST.checked_add(next))
                .unwrap_or(0);
            let mut commands = buffer(COMMANDS)?;
            let sent = CommandSender::new(host, &mut commands).check_file(
                request,
                InstallRoot::Files,
                &path,
                CheckKind::Firmware,
            );
            match sent {
                Ok(()) => return Ok(Step::Running),
                Err(Error::NoMemory) => return Err(Error::NoMemory),
                // In a run of declared work no check goes out: not now.
                Err(Error::State) => {
                    self.scan = None;
                    return Ok(Step::Done(None, false));
                }
                Err(_) => {
                    scan.recheck = Recheck::default();
                    push(&mut scan.found, Found::Unfit(DeviceOutcome::Other))?;
                    scan.next = scan.next.saturating_add(1);
                }
            }
        }
    }

    /// A check ended: what the device says of the file, then the next check, or
    /// the end of them all.
    #[inline(never)]
    fn checked(&mut self, host: &Host<'_>, done: &InstallDone<'_>) -> Result<(), Error> {
        let Some(scan) = self.scan.as_ref() else {
            return Ok(());
        };
        let wanted = u32::try_from(scan.next)
            .ok()
            .and_then(|next| CHECK_REQUEST.checked_add(next));
        if scan.next >= scan.names.len() {
            return Ok(());
        }
        let Some(request) = wanted.and_then(DeviceRequest::new) else {
            return Ok(());
        };
        let ours = done.request().map(DeviceRequest::get) == Some(request.get());
        let turn = match self.scan.as_mut() {
            Some(scan) => scan
                .recheck
                .ended(ours, done.status().is_ok(), done.outcome()),
            None => return Ok(()),
        };
        let step = match turn {
            Turn::Skip => return Ok(()),
            // The running check that was waited for ended: the file's check
            // goes out again, a new one.
            Turn::Submit => self.advance(host),
            Turn::Unfit(outcome) => self.found(host, Found::Unfit(outcome)),
            Turn::Read => {
                let Some(name) = self.scan.as_ref().and_then(|s| s.names.get(s.next)) else {
                    return Ok(());
                };
                let path = native_card::path(name)?;
                match self.read_check(host, request, &path) {
                    Err(error) if native_leaves::is_stale(error) => self.stale(host, error),
                    Ok(found) => self.found(host, found),
                    Err(error) => {
                        self.scan = None;
                        Err(error)
                    }
                }
            }
        };
        match step? {
            Step::Running => Ok(()),
            Step::Done(found, offer) => self.settle(host, found, offer),
        }
    }

    /// The file's result is not there for its request (`error`: op 16 answered
    /// `ERR_BUSY`, a check is running, or `ERR_NOT_FOUND`, a newer one replaced
    /// it): for `Busy` wait for the running check to end and submit the file's
    /// check again; for `NotFound` no check runs and no end would come, so
    /// submit it at once. After a few times the file is unfit.
    fn stale(&mut self, host: &Host<'_>, error: Error) -> Result<Step, Error> {
        let retry = self
            .scan
            .as_mut()
            .map_or(Retry::Unfit, |scan| scan.recheck.stale(error));
        match retry {
            Retry::Wait => Ok(Step::Running),
            Retry::Now => self.advance(host),
            Retry::Unfit => self.found(host, Found::Unfit(DeviceOutcome::Other)),
        }
    }

    /// The file is settled: on to the next check, or the end of them all.
    fn found(&mut self, host: &Host<'_>, found: Found) -> Result<Step, Error> {
        if let Some(scan) = self.scan.as_mut() {
            scan.recheck = Recheck::default();
            push(&mut scan.found, found)?;
            scan.next = scan.next.saturating_add(1);
        }
        self.advance(host)
    }

    /// Reads what the device found of the file `path` it has checked.
    fn read_check(
        &mut self,
        host: &Host<'_>,
        request: DeviceRequest,
        path: &str,
    ) -> Result<Found, Error> {
        let Some(running) = self.firmware(host)? else {
            return Ok(Found::Unfit(DeviceOutcome::Unsupported));
        };
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let check = match self
            .device(host)?
            .check(host, request, &mut input, &mut out)
        {
            Ok(check) => check,
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => return Ok(Found::Unfit(DeviceOutcome::Other)),
        };
        let evidence = Evidence {
            reason: check.reason(),
            paperpaw: check.paperpaw_signed(),
            foreign: check.foreign_firmware(),
            key: check.signing_key().copied(),
            id: check.id(),
            version: check.version(),
        };
        let found = native_check::judge(path, &evidence, &running.version)?;
        self.with_leaves(host, found)
    }

    /// An image the device says leaves plugins out names them (its list is the
    /// last check's only, so it is read now). Where the list cannot be read the
    /// file is not offered: its plugins would go without a word. A check still
    /// running (`Busy`) or one that replaced the result (`NotFound`) is returned as it is: not an answer.
    fn with_leaves(&mut self, host: &Host<'_>, found: Found) -> Result<Found, Error> {
        let Found::Image(mut image) = found else {
            return Ok(found);
        };
        match self.left_by_check(host) {
            Ok(names) => {
                image.leaves = names;
                Ok(Found::Image(image))
            }
            Err(Error::NoMemory) => Err(Error::NoMemory),
            // A check is running or a newer one replaced the result: stale.
            Err(error) if native_leaves::is_stale(error) => Err(error),
            Err(_) => Ok(Found::Unfit(DeviceOutcome::Other)),
        }
    }

    /// All files are checked: the page that is open shows what is offered; at
    /// start a newer update is offered once, by opening the page.
    fn settle(&mut self, host: &Host<'_>, found: Option<Found>, offer: bool) -> Result<(), Error> {
        let offered = match &found {
            Some(found) if offer && native_check::is_offer(found) => offered_now(host, found)?,
            _ => false,
        };
        if self.screens.top(PAGE).is_some() {
            let mut view = self.view_for(host, found)?;
            if core::mem::take(&mut self.searched) && no_news(&view) {
                view = View::UpToDate;
            }
            return self.show(host, view);
        }
        if offered {
            self.settled = match found {
                Some(found) => Some(paperpaw_ui_kit::mem::boxed(found)?),
                None => None,
            };
            let mut scratch = [0; 128];
            CommandSender::new(host, &mut scratch).open(Open::screen("update.page")?)?;
        }
        Ok(())
    }

    /// Shows `view` on the page that is open, if one is.
    fn show(&mut self, host: &Host<'_>, view: View) -> Result<(), Error> {
        let page = build(host, view, &self.current, self.caps)?;
        self.show_page(host, page)
    }

    /// What choosing a row does.
    fn press(&mut self, host: &Host<'_>, button: Option<Button>) -> Result<(), Error> {
        let mut commands = buffer(COMMANDS)?;
        match button {
            Some(Button::Close) => CommandSender::new(host, &mut commands).close_screen(),
            Some(Button::Search) => {
                drop(commands);
                self.show(host, View::Searching)?;
                self.search(host, false)
            }
            Some(Button::Download) => {
                drop(commands);
                let Some(page) = self.screens.top(PAGE) else {
                    return Ok(());
                };
                match core::mem::replace(&mut page.view, View::Searching) {
                    View::Offer(offer, _) => self.download(host, offer),
                    View::Anyway(waited) => self.download(host, waited.offer),
                    // Chosen anyway: the offer first, which says what stops.
                    View::Waiting(waited) => {
                        self.anyway = true;
                        self.show(host, View::Anyway(waited))
                    }
                    other => {
                        page.view = other;
                        Ok(())
                    }
                }
            }
            Some(Button::Transfer) => {
                self.away = true;
                CommandSender::new(host, &mut commands).run_action(TRANSFER)
            }
            Some(Button::Restart) => {
                if CommandSender::new(host, &mut commands)
                    .firmware_restart()
                    .is_err()
                {
                    drop(commands);
                    return self.show(host, View::RestartFailed);
                }
                Ok(())
            }
            Some(Button::Install) => {
                if self.job.is_some() {
                    return Ok(());
                }
                let Some(page) = self.screens.top(PAGE) else {
                    return Ok(());
                };
                match core::mem::replace(&mut page.view, View::Installing(None)) {
                    View::Ready(image) => self.start(host, Found::Image(image)),
                    View::Foreign(other) => self.start(host, Found::Foreign(other)),
                    other => {
                        page.view = other;
                        Ok(())
                    }
                }
            }
            None => Ok(()),
        }
    }

    /// Sends the install of the file the reader chose, as the page offered it:
    /// an older or the same version only where the page said so (the reader
    /// chose it), an own build with its key (the reader saw its fingerprint),
    /// another firmware as one. The device checks the file again and asks.
    fn start(&mut self, host: &Host<'_>, found: Found) -> Result<(), Error> {
        let (path, options) = match &found {
            Found::Image(image) => {
                let mut options = FirmwareOptions::new();
                if !image.order.is_gt() {
                    options = options.allow_older();
                }
                if let Signer::Own(key) = &image.signer {
                    options = options.key(key);
                }
                (image.path.as_str(), options)
            }
            Found::Foreign(other) => (other.path.as_str(), FirmwareOptions::new().foreign()),
            Found::Unfit(_) => return Ok(()),
        };
        let mut commands = buffer(COMMANDS)?;
        let sent = CommandSender::new(host, &mut commands).install_firmware(
            REQUEST,
            InstallRoot::Files,
            path,
            options,
        );
        match sent {
            Ok(()) => {
                self.job = Some(paperpaw_ui_kit::mem::boxed(Job { found })?);
                self.show(host, View::Installing(None))
            }
            Err(Error::NoMemory) => Err(Error::NoMemory),
            // Refused to send (no room in the device's queue, a feature the
            // device does not give): said as any other failure.
            Err(_) => self.show(host, View::Failed(DeviceOutcome::Other)),
        }
    }

    /// An install ended (exactly once, to this plugin).
    #[inline(never)]
    fn install_done(&mut self, host: &Host<'_>, done: &InstallDone<'_>) -> Result<(), Error> {
        if done.kind() != InstallKind::Firmware
            || done.request().map(DeviceRequest::get) != Some(REQUEST)
        {
            return Ok(());
        }
        let Some(job) = self.job.take() else {
            return Ok(());
        };
        match ended(done.status().is_ok(), done.outcome()) {
            Ended::Done => {
                // What is said after the restart; the restart starts the new
                // firmware (the reader said yes at the device's question).
                let kept = match &job.found {
                    Found::Image(image) => pair_file(host, &image.path, &image.version),
                    _ => Ok(()),
                };
                let mut commands = buffer(COMMANDS)?;
                let restarted = CommandSender::new(host, &mut commands).firmware_restart();
                let view = if restarted.is_ok() {
                    View::Installed(false)
                } else {
                    View::RestartFailed
                };
                self.show(host, view)?;
                kept
            }
            // Nothing happened: the file is offered as it was.
            Ended::Refused => self.show(host, view_of(Some(job.found))),
            Ended::NotNewer => self.show(host, View::Failed(DeviceOutcome::NotNewer)),
            Ended::Pending => self.show(host, View::Installed(true)),
            Ended::Failed(outcome) => self.show(host, View::Failed(outcome)),
        }
    }

    fn show_page(&mut self, host: &Host<'_>, page: Page) -> Result<(), Error> {
        if self.screens.top(PAGE).is_some() {
            online::tolerate(self.waiting_seen(host, &page.view))?;
        }
        if let Some(open) = self.screens.top(PAGE) {
            *open = page;
            let mut scratch = [0; 64];
            CommandSender::new(host, &mut scratch).queue_redraw(RedrawMode::Partial)?;
        }
        Ok(())
    }

    /// The device's progress (a hint, at most every 250 ms): the bar of the page.
    #[inline(never)]
    fn progress(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let before = match self.screens.top(PAGE).map(|page| &page.view) {
            Some(View::Installing(before)) => *before,
            _ => return Ok(()),
        };
        let Some(running) = self.firmware(host)? else {
            return Ok(());
        };
        if running.state != FirmwareState::Installing {
            return Ok(());
        }
        let now = percent(running.done, running.total);
        if now != before {
            self.show(host, View::Installing(now))?;
        }
        Ok(())
    }

    /// At start: how an install went, said once, its update file removed, the
    /// weekly look declared.
    #[inline(never)]
    fn started(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let card = self.started_card(host);
        // Where the setting is on; a device without declared work has no look.
        online::tolerate(self.plan_look(host))?;
        // The *New* mark as the offer kept says (the host kept the last one).
        online::tolerate(Self::mark_new(host))?;
        card
    }

    fn started_card(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let Some(running) = self.firmware(host)? else {
            return Ok(());
        };
        self.version = copy(&running.version)?;
        let mut messages = Messages::new()?;
        let version = shown_version(host, &mut messages, &running.version)?;
        let arguments = [Arg::Text {
            name: "version",
            value: &version,
        }];
        let kept = read_all(host, Root::Data, INSTALLING, MAX_PAIR)?;
        if let Some(bytes) = &kept {
            storage::remove(host, Root::Data, INSTALLING)?;
            if let Some((path, named)) = read_pair(bytes)
                && native_card::came(running.undone, named, &running.version)
            {
                let text = messages.format(host, "update.done", &arguments)?;
                announce(host, &text)?;
                return remove_update(host, path);
            }
        }
        if running.undone {
            let text = messages.format(host, "update.undone", &arguments)?;
            announce(host, &text)?;
            return Ok(());
        }
        // An update that waits for plugins: asked for again where due; a week
        // unseen, the page opens once.
        let open = match self.waiting_start(host, &running) {
            Ok(open) => open,
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => false,
        };
        if open {
            return Self::open_page(host);
        }
        // A newer update on the card, not offered before: checked now, offered
        // when the device has said what it is.
        let free = !matches!(
            running.state,
            FirmwareState::Waiting | FirmwareState::Installing | FirmwareState::Installed
        );
        if free
            && self.job.is_none()
            && self.scan.is_none()
            && let Step::Done(found, offer) = self.begin(host, true)?
        {
            self.settle(host, found, offer)?;
        }
        Ok(())
    }
}

/// Keeps what the restart says of the install: its file and its version.
fn pair_file(host: &Host<'_>, path: &str, version: &str) -> Result<(), Error> {
    let text = native_card::pair(path, version)?;
    replace(host, Root::Data, INSTALLING, text.as_bytes())
}

/// Whether the update `found` is one not offered before; the file `offered.txt`
/// (the same as interface 6's: path and version) keeps what was.
fn offered_now(host: &Host<'_>, found: &Found) -> Result<bool, Error> {
    let Found::Image(image) = found else {
        return Ok(false);
    };
    let text = native_card::pair(&image.path, &image.version)?;
    let kept = read_all(host, Root::Data, OFFERED, MAX_PAIR)?;
    if kept.as_deref() == Some(text.as_bytes()) {
        return Ok(false);
    }
    replace(host, Root::Data, OFFERED, text.as_bytes())?;
    Ok(true)
}

/// The page's view of what the checks found.
fn view_of(found: Option<Found>) -> View {
    match found {
        None => View::Nothing,
        Some(Found::Image(image)) => View::Ready(image),
        Some(Found::Foreign(other)) => View::Foreign(other),
        Some(Found::Unfit(why)) => View::Unfit(why),
    }
}

/// Says `text` where the reader is, also while no screen of the plugin shows.
fn announce(host: &Host<'_>, text: &str) -> Result<(), Error> {
    let mut commands = buffer(COMMANDS)?;
    CommandSender::new(host, &mut commands).notify(Notify::new(text)?.announce())
}

/// Removes the update that went in, as interface 6 did: only a file of
/// `updates` (a data file does not name any other), and not one that is gone.
fn remove_update(host: &Host<'_>, path: &str) -> Result<(), Error> {
    if !native_card::is_update_path(path) {
        return Ok(());
    }
    let files = Files::open(host)?;
    let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
    let removed = files.remove_tree(host, Root::Files, path, &mut input, &mut out);
    let released = files.release(host);
    match removed {
        Ok(_) | Err(Error::NotFound) => released,
        Err(error) => Err(error),
    }
}

/// The update files in `updates`, sorted, as many as are offered. A folder that
/// is not there has none.
fn card_files(host: &Host<'_>) -> Result<Vec<String>, Error> {
    let files = Files::open(host)?;
    let found = read_folder(host, &files);
    let released = files.release(host);
    let found = found?;
    released?;
    Ok(found)
}

fn read_folder(host: &Host<'_>, files: &Files) -> Result<Vec<String>, Error> {
    let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
    let mut offered = Offered::default();
    let mut cursor = 0;
    for _ in 0..MOST_PAGES {
        let page = match files.list(host, Root::Files, UPDATE_DIR, cursor, &mut input, &mut out) {
            Ok(page) => page,
            Err(Error::NotFound | Error::Invalid | Error::NoSpace) => break,
            Err(error) => return Err(error),
        };
        for entry in page.entries() {
            offered.add(entry.name(), entry.is_folder(), entry.unaddressable())?;
        }
        match page.next() {
            Some(next) => cursor = next,
            None => break,
        }
    }
    Ok(offered.finish())
}

/// `version` as the update's texts show it: a beta as `0.5.0 Beta 1`, every
/// other version as it is.
fn shown_version(host: &Host<'_>, messages: &mut Messages, version: &str) -> Result<String, Error> {
    match paperpaw_sdk::firmware::beta(version) {
        Some((release, number)) => messages.format(
            host,
            "update.version.beta",
            &[
                Arg::Text {
                    name: "version",
                    value: release,
                },
                Arg::Text {
                    name: "number",
                    value: number,
                },
            ],
        ),
        None => copy(version),
    }
}

/// One row: a button with its label, and a file's name at its end.
fn row(
    host: &Host<'_>,
    messages: &mut Messages,
    page: &mut (Vec<Item>, Vec<Button>),
    key: &str,
    value: &str,
    button: Button,
) -> Result<(), Error> {
    push(&mut page.0, Item::row(&messages.copy(host, key)?, value)?)?;
    push(&mut page.1, button)
}

/// What the button of an update says: an own build installs with its key, an
/// older or the same version says so, one that leaves plugins out says that.
const fn install_label(image: &Image) -> &'static str {
    match (&image.signer, image.order) {
        (Signer::Own(_), _) => "update.install-with-key",
        (_, Ordering::Less) => "update.install-older",
        (_, Ordering::Equal) => "update.reinstall",
        _ if !image.leaves.is_empty() => "update.install-without",
        _ => "update.install",
    }
}

/// Whether a page's view says nothing a search for an update did not: nothing on
/// the card, an image that is not newer, another firmware's.
fn no_news(view: &View) -> bool {
    match view {
        View::Nothing | View::Unfit(_) | View::Foreign(_) => true,
        View::Ready(image) => !image.order.is_gt(),
        _ => false,
    }
}

/// A row with a button and no value.
fn plain(
    host: &Host<'_>,
    messages: &mut Messages,
    page: &mut (Vec<Item>, Vec<Button>),
    key: &str,
    button: Button,
) -> Result<(), Error> {
    row(host, messages, page, key, "", button)
}

/// The rows of `view`: the update's button where one waits for the reader's
/// word (the other firmware's after *Not now*); *Not now* or *OK* last.
fn rows_of(
    host: &Host<'_>,
    messages: &mut Messages,
    view: &View,
    caps: Caps,
) -> Result<(Vec<Item>, Vec<Button>), Error> {
    let mut rows = (Vec::new(), Vec::new());
    match view {
        View::Ready(image) => {
            let label = install_label(image);
            row(
                host,
                messages,
                &mut rows,
                label,
                &image.version,
                Button::Install,
            )?;
            // Before going back or installing the same again, the way to look
            // for a newer one; not beside one's own build, which needs the room
            // to say its key.
            if caps.network && !image.order.is_gt() && image.signer == Signer::PaperPaw {
                plain(host, messages, &mut rows, "update.check", Button::Search)?;
            }
        }
        View::Nothing => {
            if caps.network {
                plain(host, messages, &mut rows, "update.check", Button::Search)?;
            }
            if caps.transfer {
                plain(
                    host,
                    messages,
                    &mut rows,
                    "update.transfer",
                    Button::Transfer,
                )?;
            }
        }
        View::Offer(_, leaves) => {
            let label = if leaves.is_empty() {
                "update.download"
            } else {
                "update.download-without"
            };
            plain(host, messages, &mut rows, label, Button::Download)?;
        }
        View::Waiting(_) => return waiting::rows(host, messages, false),
        View::Anyway(_) => return waiting::rows(host, messages, true),
        View::Unreachable(_) => {
            plain(host, messages, &mut rows, "update.retry", Button::Search)?;
        }
        // Not installing is the way on: first; the other firmware second.
        View::Foreign(_) => {
            plain(host, messages, &mut rows, "update.later", Button::Close)?;
            plain(
                host,
                messages,
                &mut rows,
                "update.install-foreign",
                Button::Install,
            )?;
            if caps.network {
                plain(host, messages, &mut rows, "update.check", Button::Search)?;
            }
            return Ok(rows);
        }
        View::Installed(true) | View::RestartFailed => {
            plain(host, messages, &mut rows, "update.restart", Button::Restart)?;
        }
        _ => {}
    }
    let close = match view {
        View::Ready(_) | View::Checking | View::Offer(..) => "update.later",
        View::Installing(_) | View::Searching | View::Downloading(..) => return Ok(rows),
        _ => "update.ok",
    };
    row(host, messages, &mut rows, close, "", Button::Close)?;
    Ok(rows)
}

/// The words of an update that fits: its version, who signed it, and where it
/// stands against the one running.
fn ready_texts(
    host: &Host<'_>,
    messages: &mut Messages,
    image: &Image,
    current: &str,
) -> Result<(String, String), Error> {
    let version = [Arg::Text {
        name: "version",
        value: &image.version,
    }];
    let current = [Arg::Text {
        name: "current",
        value: current,
    }];
    let Signer::Own(key) = &image.signer else {
        let (title, help) = match image.order {
            Ordering::Greater => ("update.ready.title", "update.ready.help"),
            Ordering::Equal => ("update.same.title", "update.same.help"),
            Ordering::Less => ("update.older.title", "update.older.help"),
        };
        return Ok((
            messages.format(host, title, &version)?,
            messages.format(host, help, &current)?,
        ));
    };
    let print = native_check::fingerprint(key)?;
    let fingerprint = [Arg::Text {
        name: "key",
        value: &print,
    }];
    let title = messages.format(host, "update.unknown.title", &version)?;
    let help = messages.format(host, "update.unknown.help", &fingerprint)?;
    if image.order.is_gt() {
        return Ok((title, help));
    }
    let note = messages.format(host, "update.older.note", &current)?;
    Ok((title, concat(&[&help, " ", &note])?))
}

/// The large line and the words under it.
fn texts(
    host: &Host<'_>,
    messages: &mut Messages,
    view: &View,
    current: &str,
) -> Result<(String, String), Error> {
    let with_current = [Arg::Text {
        name: "current",
        value: current,
    }];
    let (title, help, names_current) = match view {
        View::Nothing => ("update.nothing.title", "update.card.nothing.help", true),
        View::Unsupported => ("update.unfit.title", "update.reason.unsupported", false),
        View::Checking => (
            "update.card.checking.title",
            "update.card.checking.help",
            true,
        ),
        View::Unfit(outcome) => ("update.unfit.title", reason(*outcome), false),
        View::Ready(image) => {
            let (title, help) = ready_texts(host, messages, image, current)?;
            let help = leaves::with_leaves(host, messages, &help, "update.leaves", &image.leaves)?;
            return Ok((title, help));
        }
        View::Foreign(other) => {
            let named = [Arg::Text {
                name: "name",
                value: &other.name,
            }];
            let file = [Arg::Text {
                name: "file",
                value: &other.path,
            }];
            return Ok((
                messages.format(host, "update.foreign.title", &named)?,
                messages.format(host, "update.foreign.help", &file)?,
            ));
        }
        View::Installing(Some(value)) => {
            let arguments = [Arg::Int {
                name: "percent",
                value: *value,
            }];
            return Ok((
                messages.format(host, "update.installing.percent", &arguments)?,
                messages.copy(host, "update.installing.help")?,
            ));
        }
        View::Installing(None) => ("update.installing.title", "update.installing.help", false),
        View::Searching => {
            let server = server_name(host)?;
            let arguments = [Arg::Text {
                name: "server",
                value: &server,
            }];
            return Ok((
                messages.copy(host, "update.checking.title")?,
                messages.format(host, "update.checking.help", &arguments)?,
            ));
        }
        View::UpToDate => ("update.up-to-date.title", "update.up-to-date.help", true),
        View::Unreachable(trouble) => return unreachable_texts(host, messages, *trouble),
        View::Offer(offer, leaves) => return offer_texts(host, messages, offer, leaves, current),
        View::Waiting(waited) => return waiting::texts(host, messages, waited, false),
        View::Anyway(waited) => return waiting::texts(host, messages, waited, true),
        View::Downloading(version, percent) => {
            return downloading_texts(host, messages, version, *percent);
        }
        View::Installed(false) => ("update.installed.title", "update.installed.help", false),
        View::Installed(true) => (
            "update.installed.title",
            "update.installed.restart-help",
            false,
        ),
        View::RestartFailed => ("update.installed.title", "update.restart-failed", false),
        View::Failed(outcome) => {
            let why = messages.copy(host, reason(*outcome))?;
            let arguments = [
                Arg::Text {
                    name: "reason",
                    value: &why,
                },
                Arg::Text {
                    name: "current",
                    value: current,
                },
            ];
            return Ok((
                messages.copy(host, "update.failed.title")?,
                messages.format(host, "update.failed.help", &arguments)?,
            ));
        }
    };
    // Only the texts that name the version running take it.
    let help = if names_current {
        messages.format(host, help, &with_current)?
    } else {
        messages.copy(host, help)?
    };
    Ok((messages.copy(host, title)?, help))
}

/// Why the update server could not be asked, or what it answered.
fn unreachable_texts(
    host: &Host<'_>,
    messages: &mut Messages,
    trouble: Trouble,
) -> Result<(String, String), Error> {
    let server = server_name(host)?;
    let title = messages.copy(host, "update.unreachable.title")?;
    let named = Arg::Text {
        name: "server",
        value: &server,
    };
    let help = match trouble {
        Trouble::Status(status) => messages.format(
            host,
            "update.server-error.help",
            &[
                named,
                Arg::Int {
                    name: "status",
                    value: i64::from(status),
                },
            ],
        )?,
        Trouble::Reason(key) => {
            let reason = messages.copy(host, key)?;
            let reason = Arg::Text {
                name: "reason",
                value: &reason,
            };
            messages.format(host, "update.unreachable.help", &[named, reason])?
        }
    };
    Ok((title, help))
}

/// An update the server named: its version, what is installed, and its size.
fn offer_texts(
    host: &Host<'_>,
    messages: &mut Messages,
    offer: &Offer,
    leaves: &str,
    current: &str,
) -> Result<(String, String), Error> {
    let version = shown_version(host, messages, &offer.version)?;
    let title = messages.format(
        host,
        "update.offer.title",
        &[Arg::Text {
            name: "version",
            value: &version,
        }],
    )?;
    let current = Arg::Text {
        name: "current",
        value: current,
    };
    // What cannot come along comes first: where the footer is cut, the size goes.
    if offer.size == 0 {
        let help = messages.format(host, "update.offer.help-unsized", &[current])?;
        let help = leaves::leaves_first(host, messages, &help, "update.leaves.download", leaves)?;
        return Ok((title, help));
    }
    let (key, value) = match offer.size / 1024 {
        0..1000 => ("update.size.kb", offer.size.div_ceil(1024)),
        _ => ("update.size.mb", offer.size.div_ceil(1024 * 1024)),
    };
    let size = messages.format(
        host,
        key,
        &[Arg::Int {
            name: "value",
            value: i64::try_from(value).unwrap_or(i64::MAX),
        }],
    )?;
    let size = Arg::Text {
        name: "size",
        value: &size,
    };
    let help = messages.format(host, "update.offer.help", &[current, size])?;
    let help = leaves::leaves_first(host, messages, &help, "update.leaves.download", leaves)?;
    Ok((title, help))
}

/// The download of the update (and then of the plugins for it).
fn downloading_texts(
    host: &Host<'_>,
    messages: &mut Messages,
    version: &str,
    percent: Option<i64>,
) -> Result<(String, String), Error> {
    let shown = shown_version(host, messages, version)?;
    let version = Arg::Text {
        name: "version",
        value: &shown,
    };
    let help = messages.copy(host, "update.downloading.help")?;
    let Some(percent) = percent else {
        return Ok((
            messages.format(host, "update.downloading.title", &[version])?,
            help,
        ));
    };
    let percent = Arg::Int {
        name: "percent",
        value: percent,
    };
    Ok((
        messages.format(host, "update.downloading.percent", &[version, percent])?,
        help,
    ))
}

/// The server's name as the reader knows it, e.g. `paperpaw.org`.
fn server_name(host: &Host<'_>) -> Result<String, Error> {
    let name = paperpaw_ui_kit::widget::settings::text_of(host, "update.server", |setting| {
        copy(crate::native_online::server_name(
            crate::native_online::server(setting),
        ))
    })??;
    Ok(name)
}

/// The page of `view`: its rows, the large line and the words under it.
fn build(host: &Host<'_>, view: View, current: &str, caps: Caps) -> Result<Page, Error> {
    let mut messages = Messages::new()?;
    let (items, buttons) = rows_of(host, &mut messages, &view, caps)?;
    let (title, help) = texts(host, &mut messages, &view, current)?;
    Ok(Page {
        view,
        buttons,
        list: ItemList::new(items),
        title,
        help,
    })
}
