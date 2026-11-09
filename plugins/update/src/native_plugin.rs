//! The interface-7 update page for the card flow: the update files in the
//! card's `updates` folder, the install the device checks and asks the reader
//! about (decisions 132 and 134), how it ended, and what is said after the
//! restart. The online check, the downloads, the plugin fetch, the weekly
//! look, the badge and the states that need what the device knows of a file
//! (its version and signer) are named in `PORTING-7.md`.
use crate::{
    declarations::{CHECK_TOKEN, FIRMWARE_CHANGED, INSTALL_DONE, OPEN_TOKEN, PAGE, STARTED},
    native_card::{
        self, Ended, MAX_PAIR, Offered, REQUEST, UPDATE_DIR, ended, percent, read_pair, reason,
    },
};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Handled,
    native::{
        Error, Host, Plugin,
        callbacks::{DrawFrame, Input},
        commands::{CommandSender, FirmwareOptions, InstallRoot, RedrawMode},
        device::{Device, DeviceOutcome, DeviceRequest, FirmwareState, InstallKind},
        events::{Event, Incoming, InstallDone, kinds},
        files::Files,
        formatting::Arg,
        registrations,
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
    text::copy,
};

mod image {
    include!(concat!(env!("OUT_DIR"), "/update_declarations.rs"));
}

/// The data file that says an install went through, kept through the restart
/// (interface 6's, `docs/data-files.md`).
const INSTALLING: &str = "installing.txt";
/// Room for the commands of one callback (an install's path is up to 512 bytes).
const COMMANDS: usize = 1024;
/// Pages of the update folder read at most.
const MOST_PAGES: usize = 16;

/// What the page shows.
#[derive(Debug, PartialEq, Eq)]
enum View {
    /// No update file on the card.
    Nothing,
    /// The device installs no firmware updates here.
    Unsupported,
    /// Update files on the card, to choose from.
    Files(Vec<String>),
    /// The device said this file is not newer than the one running: going back
    /// (or installing the same again) is the reader's choice.
    NotNewer(String),
    /// The device checks, asks and writes; how far, where it says.
    Installing(Option<i64>),
    /// In place; with a button to restart where the plugin did not do it.
    Installed(bool),
    /// Not installed, for this reason.
    Failed(DeviceOutcome),
}

/// What a row of the page does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Button {
    /// Installs the file of this number.
    Install(usize),
    /// Installs the file the device said is not newer.
    InstallOlder,
    Restart,
    Close,
}

#[derive(Debug)]
struct Page {
    view: View,
    buttons: Vec<Button>,
    list: ItemList,
    title: String,
    help: String,
}

/// The install in flight: its file, and whether the reader chose an older one.
#[derive(Debug)]
struct Job {
    path: String,
    older: bool,
}

/// What `core.device` says of the firmware.
#[derive(Debug)]
struct Running {
    version: String,
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
    job: Option<Job>,
    /// The version running, as the page says it.
    current: String,
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
            kinds::ScreenClosed
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
            // BLOCKED (#228): asking the update server is the online flow
            // (`core.net`, the download, the plugin fetch): `PORTING-7.md`.
            Event::ActionRun {
                action: "update.check",
                ..
            } if token == CHECK_TOKEN => Err(Error::Unsupported),
            Event::Started { .. } if token == STARTED => self.started(host),
            Event::InstallDone(done) if token == INSTALL_DONE => self.install_done(host, &done),
            Event::FirmwareChanged { .. } if token == FIRMWARE_CHANGED => self.progress(host),
            Event::ScreenOpened { instance, .. } if token == PAGE => {
                let page = self.look(host)?;
                self.screens.open(instance, page)
            }
            Event::ScreenShown {
                instance: Some(instance),
                ..
            } if token == PAGE => self.screens.shown(token, instance),
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
    fn look(&mut self, host: &Host<'_>) -> Result<Page, Error> {
        let Some(running) = self.firmware(host)? else {
            return build(host, View::Unsupported, "");
        };
        let mut messages = Messages::new()?;
        self.current = shown_version(host, &mut messages, &running.version)?;
        let view = match running.state {
            _ if self.job.is_some() => View::Installing(percent(running.done, running.total)),
            FirmwareState::Waiting | FirmwareState::Installing => {
                View::Installing(percent(running.done, running.total))
            }
            FirmwareState::Installed => View::Installed(true),
            _ => {
                let names = card_files(host)?;
                if names.is_empty() {
                    View::Nothing
                } else {
                    View::Files(names)
                }
            }
        };
        build(host, view, &self.current)
    }

    /// Shows `view` on the page that is open, if one is.
    fn show(&mut self, host: &Host<'_>, view: View) -> Result<(), Error> {
        let page = build(host, view, &self.current)?;
        self.show_page(host, page)
    }

    /// What choosing a row does.
    fn press(&mut self, host: &Host<'_>, button: Option<Button>) -> Result<(), Error> {
        let mut commands = buffer(COMMANDS)?;
        match button {
            Some(Button::Close) => CommandSender::new(host, &mut commands).close_screen(),
            Some(Button::Restart) => CommandSender::new(host, &mut commands).firmware_restart(),
            Some(Button::Install(index)) => {
                let name = match self.screens.top(PAGE).map(|page| &page.view) {
                    Some(View::Files(names)) => names.get(index),
                    _ => None,
                };
                let Some(name) = name else {
                    return Ok(());
                };
                let path = native_card::path(name)?;
                self.start(host, path, false)
            }
            Some(Button::InstallOlder) => {
                let path = match self.screens.top(PAGE).map(|page| &page.view) {
                    Some(View::NotNewer(path)) => Some(copy(path)?),
                    _ => None,
                };
                match path {
                    Some(path) => self.start(host, path, true),
                    None => Ok(()),
                }
            }
            None => Ok(()),
        }
    }

    /// Sends the install of `path`. The device checks the file and asks the
    /// reader (an older version only when `older` says the reader chose it).
    fn start(&mut self, host: &Host<'_>, path: String, older: bool) -> Result<(), Error> {
        if self.job.is_some() {
            return Ok(());
        }
        let options = if older {
            FirmwareOptions::new().allow_older()
        } else {
            FirmwareOptions::new()
        };
        let mut commands = buffer(COMMANDS)?;
        let sent = CommandSender::new(host, &mut commands).install_firmware(
            REQUEST,
            InstallRoot::Files,
            &path,
            options,
        );
        match sent {
            Ok(()) => {
                self.job = Some(Job { path, older });
                self.show(host, View::Installing(None))
            }
            Err(Error::NoMemory) => Err(Error::NoMemory),
            // Refused to send (no room in the device's queue, a feature the
            // device does not give): said as any other failure.
            Err(_) => self.show(host, View::Failed(DeviceOutcome::Other)),
        }
    }

    /// An install ended (exactly once, to this plugin).
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
                let kept = pair_file(host, &job.path);
                let mut commands = buffer(COMMANDS)?;
                let restarted = CommandSender::new(host, &mut commands).firmware_restart();
                self.show(host, View::Installed(false))?;
                kept?;
                restarted
            }
            Ended::Refused => {
                let page = self.look(host)?;
                self.show_page(host, page)
            }
            Ended::NotNewer if !job.older => self.show(host, View::NotNewer(job.path)),
            Ended::NotNewer => self.show(host, View::Failed(DeviceOutcome::NotNewer)),
            Ended::Pending => self.show(host, View::Installed(true)),
            Ended::Failed(outcome) => self.show(host, View::Failed(outcome)),
        }
    }

    fn show_page(&mut self, host: &Host<'_>, page: Page) -> Result<(), Error> {
        if let Some(open) = self.screens.top(PAGE) {
            *open = page;
            let mut scratch = [0; 64];
            CommandSender::new(host, &mut scratch).queue_redraw(RedrawMode::Partial)?;
        }
        Ok(())
    }

    /// The device's progress (a hint, at most every 250 ms): the bar of the page.
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

    /// At start: how an install went, said once, and its update file removed.
    fn started(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let Some(running) = self.firmware(host)? else {
            return Ok(());
        };
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
        }
        Ok(())
    }
}

/// Keeps what the restart says of the install: its file, the version still
/// unknown (`PORTING-7.md`).
fn pair_file(host: &Host<'_>, path: &str) -> Result<(), Error> {
    let text = native_card::pair(path, "")?;
    replace(host, Root::Data, INSTALLING, text.as_bytes())
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

/// The page of `view`: its rows, the large line and the words under it.
fn build(host: &Host<'_>, view: View, current: &str) -> Result<Page, Error> {
    let mut messages = Messages::new()?;
    let with_current = [Arg::Text {
        name: "current",
        value: current,
    }];
    let mut rows = (Vec::new(), Vec::new());
    let (title, help) = match &view {
        View::Nothing => (
            messages.copy(host, "update.nothing.title")?,
            messages.format(host, "update.card.nothing.help", &with_current)?,
        ),
        View::Unsupported => (
            messages.copy(host, "update.unfit.title")?,
            messages.copy(host, "update.reason.unsupported")?,
        ),
        View::Files(names) => {
            for (index, name) in names.iter().enumerate() {
                row(
                    host,
                    &mut messages,
                    &mut rows,
                    "update.install",
                    name,
                    Button::Install(index),
                )?;
            }
            (
                messages.copy(host, "update.card.found.title")?,
                messages.format(host, "update.card.found.help", &with_current)?,
            )
        }
        View::NotNewer(_) => {
            row(
                host,
                &mut messages,
                &mut rows,
                "update.install-older",
                "",
                Button::InstallOlder,
            )?;
            (
                messages.copy(host, "update.not-newer.title")?,
                messages.format(host, "update.not-newer.help", &with_current)?,
            )
        }
        View::Installing(percent) => (
            match percent {
                Some(value) => messages.format(
                    host,
                    "update.installing.percent",
                    &[Arg::Int {
                        name: "percent",
                        value: *value,
                    }],
                )?,
                None => messages.copy(host, "update.installing.title")?,
            },
            messages.copy(host, "update.installing.help")?,
        ),
        View::Installed(restart) => {
            if *restart {
                row(
                    host,
                    &mut messages,
                    &mut rows,
                    "update.restart",
                    "",
                    Button::Restart,
                )?;
            }
            (
                messages.copy(host, "update.installed.title")?,
                messages.copy(
                    host,
                    if *restart {
                        "update.installed.restart-help"
                    } else {
                        "update.installed.help"
                    },
                )?,
            )
        }
        View::Failed(outcome) => (messages.copy(host, "update.failed.title")?, {
            let why = messages.copy(host, reason(*outcome))?;
            messages.format(
                host,
                "update.failed.help",
                &[
                    Arg::Text {
                        name: "reason",
                        value: &why,
                    },
                    Arg::Text {
                        name: "current",
                        value: current,
                    },
                ],
            )?
        }),
    };
    // The way on first where an update waits for the reader's word; *Not now*
    // or *OK* last.
    let later = match view {
        View::Files(_) | View::NotNewer(_) => "update.later",
        View::Installing(_) | View::Installed(false) => "",
        _ => "update.ok",
    };
    if !later.is_empty() {
        row(host, &mut messages, &mut rows, later, "", Button::Close)?;
    }
    Ok(Page {
        view,
        buttons: rows.1,
        list: ItemList::new(rows.0),
        title,
        help,
    })
}
