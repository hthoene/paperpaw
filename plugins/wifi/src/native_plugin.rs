//! The interface-7 Wi-Fi plugin, the screens slice: the Wi-Fi page, the
//! networks in reach (choosing one, its password asked for and kept, forgetting
//! one) and the setup with a phone up to what the host cannot do yet. What
//! waits (the radio's own events, the served page, `wifi.ini`) is in
//! `PORTING-7.md`.
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::Handled;
use paperpaw_sdk::native::{
    Error, Host, Plugin,
    callbacks::{DrawFrame, Input},
    commands::{CommandSender, RedrawMode},
    device::{Device, DeviceCursor, WifiInfo, WifiState},
    events::{AnswerOutcome, Answered, Event, Incoming},
    formatting::Arg,
    net::Net,
    registrations,
    reply::Reply,
    secrets::Secrets,
    settings::{self, SettingValue},
    ui::{Instance, Notify, Open, Question, RequestId, Ui},
    work::Work,
};
use paperpaw_ui_kit::{
    Footer, Item, ItemList, ItemStep, Messages, Screens,
    mem::{SERVICE_BUFFER, buffer, wipe},
    text::{concat, copy},
};

use crate::declarations::{
    NETWORK_IDLE, NETWORK_WANTED, NETWORKS as NETWORKS_TOKEN, OPEN as OPEN_TOKEN,
    PAGE as PAGE_TOKEN, PHONE as PHONE_TOKEN, POWER_CHANGED, SERVED_REQUEST, SERVER_EVENT,
    STACK_CHANGED, STARTED, STATUS, STATUS_CHANGED, WIFI_CHANGED, WIFI_DONE, WIFI_SCANNED, WORK,
};
use crate::native_ini::{self, Content};
use crate::native_known::{self, Remember};
use crate::native_model::{
    self, Link, Network, PageRow, Pick, is_kept, next_idle, note_keys, page_rows, pick, room,
};
use crate::native_phone::Action as PhoneAction;
use crate::native_policy::{Password, Policy, Radio, Step};

mod events;
mod phone;
mod views;

use views::{Count, Found, Info, Mark, Named, Networks, Page, row_item};

mod image {
    include!(concat!(env!("OUT_DIR"), "/wifi_declarations.rs"));
}

/// The input of a `core.device` call: one record of at most 256 B
/// (`max_input`), where its reply buffer must hold 4,096 B (`max_reply`).
const DEVICE_INPUT: usize = 256;
const ON_DEMAND: &str = "wifi.on-demand";
const IDLE_OFF: &str = "wifi.idle-off";
const WHILE_CHARGING: &str = "wifi.while-charging";
/// Pages of networks read at most, and networks kept from them.
const MOST_PAGES: usize = 4;
const MOST_NETWORKS: usize = 64;
/// A password is at most this long (WPA).
const PASSWORD_MOST: usize = native_known::MAX_PASSWORD;

// The phone setup is the one big view; screens are few, so no indirection.
#[allow(
    clippy::large_enum_variant,
    reason = "the phone setup is the one big view and screens are few; a box would allocate unfallibly"
)]
#[derive(Debug)]
enum View {
    Page(Page),
    Networks(Networks),
    Phone(phone::PhoneView),
}

/// The interface-7 Wi-Fi plugin.
#[derive(Debug, Default)]
pub struct NativeWifi {
    device: Option<Device>,
    ui: Option<Ui>,
    secrets: Option<Secrets>,
    /// Opened with the setup with a phone.
    net: Option<Net>,
    screens: Screens<View>,
    policy: Policy,
    /// The password being asked for, and the network it is for.
    asked: Option<(RequestId, String)>,
    /// The number of the last join asked for.
    request: u32,
    /// The work service, for the idle-off and retry timers, and the generation
    /// of the policy each was set at.
    work: Option<Work>,
    idle_generation: Option<u32>,
    retry_generation: Option<u32>,
}

impl Plugin for NativeWifi {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::SETTINGS
            .iter()
            .chain(image::SCREENS)
            .chain(image::NAVIGATION)
            .chain(image::EVENTS)
        {
            registrations::submit(host, batch)?;
        }
        // The services are opened by the first callback: opening needs a live frame.
        Ok(Self::default())
    }

    fn event(
        &mut self,
        host: &Host<'_>,
        token: u32,
        incoming: Incoming<'_>,
        reply: &mut Reply<'_>,
    ) -> Result<(), Error> {
        let event = incoming.decode()?;
        match event {
            // The status item: the host asks which of its items show.
            Event::StageCall { stage, payload, .. } if token == STATUS => {
                self.status(host, stage, payload, reply)
            }
            Event::ActionRun { .. } if token == OPEN_TOKEN => {
                let mut scratch = [0; 128];
                CommandSender::new(host, &mut scratch).open(Open::screen("wifi.page")?)
            }
            Event::Started { now_ms, .. } if token == STARTED => {
                self.keep_file(host)?;
                self.charging(host, now_ms, true)
            }
            Event::WifiChanged { now_ms, .. } if token == WIFI_CHANGED => {
                self.radio_changed(host, now_ms)
            }
            Event::NetworkWanted { now_ms, .. } if token == NETWORK_WANTED => {
                self.network_wanted(host, now_ms)
            }
            Event::NetworkIdle { now_ms, .. } if token == NETWORK_IDLE => {
                self.network_idle(host, now_ms)
            }
            Event::WifiScanned { now_ms, .. } if token == WIFI_SCANNED => {
                self.wifi_scanned(host, now_ms)
            }
            Event::WifiDone(done) if token == WIFI_DONE => self.wifi_done(host, &done),
            Event::PowerChanged(change) if token == POWER_CHANGED => {
                self.power_changed(host, change.now_ms())
            }
            Event::StatusChanged { now_ms, .. } if token == STATUS_CHANGED => {
                self.charging(host, now_ms, false)
            }
            Event::WorkDue(due) if token == WORK => self.work_due(host, &due),
            Event::Server(server) if token == SERVER_EVENT => {
                self.phone_server(host, server.state())
            }
            Event::ServedRequest(request) if token == SERVED_REQUEST => {
                self.phone_request(host, &request)
            }
            Event::Answered { answer, .. } => self.answered(host, answer),
            Event::ScreenOpened {
                instance, now_ms, ..
            } if token == PAGE_TOKEN || token == NETWORKS_TOKEN || token == PHONE_TOKEN => {
                self.opened(host, token, instance, now_ms)
            }
            Event::ScreenShown {
                instance: Some(instance),
                ..
            } if token == PAGE_TOKEN || token == NETWORKS_TOKEN || token == PHONE_TOKEN => {
                self.screens.shown(token, instance)?;
                if token == PHONE_TOKEN {
                    return self.phone_shown(host);
                }
                self.reread(host)
            }
            Event::ScreenClosed { instance, .. } => {
                // The setup screen showed a password: the page is not served
                // any more, the own network closed, and what showed it wiped.
                let phone = matches!(self.screens.get(instance), Some(View::Phone(_)));
                let result = if phone {
                    self.phone_closed(host)
                } else {
                    Ok(())
                };
                if let Some(View::Phone(view)) = self.screens.get(instance) {
                    view.wipe();
                }
                self.screens.close(instance);
                result
            }
            // Any screen on top (`screen_shown` is the declared screens' own,
            // `screens-input.md` 2): back home or in a book, the radio goes
            // off now.
            Event::StackChanged { now_ms, .. } if token == STACK_CHANGED => {
                self.stack_changed(host, now_ms)
            }
            _ => Ok(()),
        }
    }

    fn draw(&mut self, host: &Host<'_>, token: u32, mut frame: DrawFrame<'_>) -> Result<(), Error> {
        // The list translates a title that is a catalog key: no catalog
        // buffers of ours are made for a frame.
        match self.screens.top(token).ok_or(Error::State)? {
            View::Page(view) => {
                view.list
                    .draw_with(host, &mut frame, "wifi.title", true, Footer::None)
            }
            View::Networks(view) => {
                view.list
                    .draw_with(host, &mut frame, "wifi.networks.title", true, Footer::None)
            }
            View::Phone(view) => {
                let (list, footer) = view.drawn();
                list.draw_with(host, &mut frame, "wifi.phone.title", true, footer)
            }
        }
    }

    fn input(&mut self, host: &Host<'_>, token: u32, input: Input<'_>) -> Result<Handled, Error> {
        let Some(view) = self.screens.top(token) else {
            return Ok(Handled::No);
        };
        // The setup with a phone goes by steps: the page keys are its own.
        if matches!(view, View::Phone(_))
            && let Input::Action { action, .. } = input
        {
            let step = match action {
                "core.next-page" => Some(PhoneAction::NextPage),
                "core.prev-page" => Some(PhoneAction::PrevPage),
                _ => None,
            };
            if let Some(step) = step
                && self.phone_action(host, step)?
            {
                return Ok(Handled::Yes);
            }
        }
        let Some(view) = self.screens.top(token) else {
            return Ok(Handled::No);
        };
        let list = match view {
            View::Page(view) => &mut view.list,
            View::Networks(view) => &mut view.list,
            View::Phone(view) => &mut view.list,
        };
        let step = list.input(host, &input)?;
        match step {
            ItemStep::Chosen(index) => self.chosen(host, token, index)?,
            ItemStep::Held(index) => self.held(host, token, index)?,
            _ => {}
        }
        Ok(ItemList::handled(step))
    }
}

impl NativeWifi {
    fn ensure(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if self.device.is_none() {
            self.device = Some(Device::open(host)?);
        }
        if self.ui.is_none() {
            self.ui = Some(Ui::open(host)?);
        }
        Ok(())
    }

    /// The secrets service, `None` where the device has none to give (the
    /// networks are then not kept, nor asked for).
    fn secrets(&mut self, host: &Host<'_>) -> Result<Option<&Secrets>, Error> {
        if self.secrets.is_none() {
            match Secrets::open(host) {
                Ok(secrets) => self.secrets = Some(secrets),
                Err(Error::Unsupported | Error::NotFound | Error::Denied) => return Ok(None),
                Err(error) => return Err(error),
            }
        }
        Ok(self.secrets.as_ref())
    }

    /// Calls `f` with what the radio says, the reply borrowed from buffers
    /// that live as long as the call; `None` where the device has no radio.
    fn with_wifi<T>(
        &self,
        host: &Host<'_>,
        f: impl FnOnce(&WifiInfo<'_>) -> Result<T, Error>,
    ) -> Result<Option<T>, Error> {
        let device = self.device.as_ref().ok_or(Error::State)?;
        let (mut input, mut out) = (buffer(DEVICE_INPUT)?, buffer(SERVICE_BUFFER)?);
        match device.wifi(host, &mut input, &mut out) {
            Ok(info) => f(&info).map(Some),
            Err(Error::NotFound | Error::Unsupported) => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// Where the radio is; no radio gives `present == false`.
    fn read_info(&self, host: &Host<'_>) -> Result<Info, Error> {
        let info = self.with_wifi(host, |info| Ok(Self::info_of(info)))?;
        Ok(info.unwrap_or_default())
    }

    fn info_of(info: &WifiInfo<'_>) -> Info {
        Info {
            present: true,
            radio: match info.state() {
                WifiState::Off => Radio::Off,
                WifiState::Joining => Radio::Joining,
                WifiState::Online => Radio::Online,
                WifiState::Hosting => Radio::Hosting,
                _ => Radio::On,
            },
            scanning: info.scanning(),
            busy: info.busy(),
            wanted: info.wanted(),
            guests: info.guests(),
        }
    }

    /// As [`Self::read_info`], with the name of the network and the address
    /// copied in the states that have them.
    fn read_named(&self, host: &Host<'_>) -> Result<Named, Error> {
        let named = self.with_wifi(host, |wifi| {
            let info = Self::info_of(wifi);
            let (ssid, address) = (
                if matches!(info.radio, Radio::Joining | Radio::Online) {
                    copy(wifi.ssid())?
                } else {
                    String::new()
                },
                if info.radio == Radio::Online {
                    copy(wifi.address())?
                } else {
                    String::new()
                },
            );
            Ok(Named {
                info,
                ssid,
                address,
            })
        })?;
        Ok(named.unwrap_or_default())
    }

    /// Whether the radio's network name is `ssid`, in any state: compared
    /// where it is read, not copied.
    fn ssid_is(&self, host: &Host<'_>, ssid: &str) -> Result<bool, Error> {
        let same = self.with_wifi(host, |info| Ok(info.ssid() == ssid))?;
        Ok(same.unwrap_or(false))
    }

    /// The networks in reach, strongest first, owned.
    fn read_networks(&self, host: &Host<'_>) -> Result<Vec<Network>, Error> {
        let mut networks = Vec::new();
        self.walk_networks(host, &mut networks)?;
        Ok(networks)
    }

    /// How many networks are in reach (as far as are read), none kept.
    fn count_networks(&self, host: &Host<'_>) -> Result<usize, Error> {
        let mut count = Count(0);
        self.walk_networks(host, &mut count)?;
        Ok(count.0)
    }

    /// Reads the networks in reach into `found`: at most [`MOST_PAGES`] pages
    /// and [`MOST_NETWORKS`] networks; a list that changed under the reading
    /// is read again from the start.
    fn walk_networks(&self, host: &Host<'_>, found: &mut impl Found) -> Result<(), Error> {
        let device = self.device.as_ref().ok_or(Error::State)?;
        let (mut input, mut out) = (buffer(DEVICE_INPUT)?, buffer(SERVICE_BUFFER)?);
        'again: for _ in 0..2 {
            found.restart();
            let mut cursor = DeviceCursor::start();
            for _ in 0..MOST_PAGES {
                let page = match device.networks(host, cursor, &mut input, &mut out) {
                    Ok(page) => page,
                    Err(Error::NotFound | Error::Unsupported) => {
                        found.restart();
                        return Ok(());
                    }
                    // The list changed under the reading: start over.
                    Err(Error::State) => continue 'again,
                    Err(error) => return Err(error),
                };
                let room = MOST_NETWORKS.saturating_sub(found.len());
                found.reserve(page.rows().count().min(room))?;
                for network in page.rows().take(room) {
                    found.add(network.ssid(), network.signal(), network.secured())?;
                }
                match page.end().next_cursor() {
                    Some(next) if found.len() < MOST_NETWORKS => cursor = next,
                    _ => return Ok(()),
                }
            }
            return Ok(());
        }
        Err(Error::State)
    }

    /// The names of the networks kept; none where the device keeps none.
    fn read_kept(&mut self, host: &Host<'_>) -> Result<Vec<String>, Error> {
        match self.secrets(host)? {
            Some(secrets) => match native_known::names(secrets, host) {
                Ok(names) => Ok(names),
                Err(Error::Unsupported | Error::NotFound | Error::Denied) => Ok(Vec::new()),
                Err(error) => Err(error),
            },
            None => Ok(Vec::new()),
        }
    }

    /// Whether any network is kept, no name read.
    fn read_kept_any(&mut self, host: &Host<'_>) -> Result<bool, Error> {
        match self.secrets(host)? {
            Some(secrets) => match native_known::any(secrets, host) {
                Ok(any) => Ok(any),
                Err(Error::Unsupported | Error::NotFound | Error::Denied) => Ok(false),
                Err(error) => Err(error),
            },
            None => Ok(false),
        }
    }

    fn flag(host: &Host<'_>, key: &str) -> Result<bool, Error> {
        let mut scratch = [0; 64];
        Ok(matches!(
            settings::read_setting(host, key, &mut scratch)?,
            SettingValue::Bool(true)
        ))
    }

    fn idle(host: &Host<'_>) -> Result<String, Error> {
        let mut scratch = [0; 64];
        match settings::read_setting(host, IDLE_OFF, &mut scratch)? {
            SettingValue::Text(key) => copy(key),
            _ => copy("wifi.idle-off.1m"),
        }
    }

    /// Where the radio is, in words.
    fn state_text(host: &Host<'_>, say: &mut Messages, info: &Named) -> Result<String, Error> {
        match info.radio {
            Radio::Off => say.copy(host, "wifi.state.off"),
            Radio::On if info.scanning => say.copy(host, "wifi.state.searching"),
            Radio::Joining => say.format(
                host,
                "wifi.state.joining",
                &[Arg::Text {
                    name: "ssid",
                    value: &info.ssid,
                }],
            ),
            Radio::Online => say.format(
                host,
                "wifi.state.online",
                &[
                    Arg::Text {
                        name: "ssid",
                        value: &info.ssid,
                    },
                    Arg::Text {
                        name: "address",
                        value: &info.address,
                    },
                ],
            ),
            _ => say.copy(host, "wifi.state.on"),
        }
    }

    fn read_page(&mut self, host: &Host<'_>) -> Result<Page, Error> {
        let info = self.read_named(host)?;
        if !info.present {
            // No radio: the page has nothing to say.
            return Ok(Page::empty());
        }
        // Only what the rows say: whether a network is kept, and (while the
        // radio is on) how many are in reach.
        let kept = self.read_kept_any(host)?;
        let reach = if info.radio.is_on() {
            self.count_networks(host)?
        } else {
            0
        };
        let rows = page_rows(kept, info.radio.is_on())?;
        let mut say = Messages::new()?;
        let mut items = room(rows.len())?;
        for row in &rows {
            items.push(Self::page_item(host, &mut say, &info, reach, *row)?);
        }
        Ok(Page {
            rows,
            list: ItemList::new(items),
        })
    }

    fn page_item(
        host: &Host<'_>,
        say: &mut Messages,
        info: &Named,
        reach: usize,
        row: PageRow,
    ) -> Result<Item, Error> {
        let switch = |say: &mut Messages, on: bool| {
            say.copy(
                host,
                if on {
                    "wifi.switch.on"
                } else {
                    "wifi.switch.off"
                },
            )
        };
        match row {
            PageRow::Radio => {
                let title = say.copy(host, "wifi.title")?;
                let state = Self::state_text(host, say, info)?;
                // As in Beta 3 the row reads "Wi-Fi, <where the radio is>": the
                // note says whether it is on, so no word of the switch is
                // added; it is told as a switch (`on`) to whoever asks.
                let mut item = Item::noted(&title, "", &state)?;
                if let Item::Row(row) = &mut item {
                    row.on = Some(info.radio.is_on());
                }
                Ok(item)
            }
            PageRow::Networks => {
                let title = say.copy(host, "wifi.networks.title")?;
                let reached = if reach == 0 && info.scanning {
                    say.copy(host, "wifi.state.searching")?
                } else if reach == 0 {
                    say.copy(host, "wifi.no-networks")?
                } else {
                    say.format(
                        host,
                        "wifi.networks.reach",
                        &[Arg::Int {
                            name: "count",
                            value: i64::try_from(reach).unwrap_or(i64::MAX),
                        }],
                    )?
                };
                let note = if info.radio == Radio::Online {
                    concat(&[&info.ssid, " · ", &reached])?
                } else {
                    reached
                };
                Item::noted(&title, "›", &note)
            }
            PageRow::Phone => {
                let title = say.copy(host, "wifi.phone.title")?;
                let note = say.copy(host, "wifi.phone.note")?;
                Item::noted(&title, "›", &note)
            }
            PageRow::OnDemand | PageRow::WhileCharging => {
                let (key, setting) = if row == PageRow::OnDemand {
                    ("wifi.setting.on-demand", ON_DEMAND)
                } else {
                    ("wifi.setting.while-charging", WHILE_CHARGING)
                };
                let title = say.copy(host, key)?;
                let help = say.copy(host, &concat(&[key, ".help"])?)?;
                let value = switch(say, Self::flag(host, setting)?)?;
                Item::noted(&title, &value, &help)
            }
            PageRow::IdleOff => {
                let title = say.copy(host, "wifi.setting.idle-off")?;
                let help = say.copy(host, "wifi.setting.idle-off.help")?;
                let value = say.copy(host, &Self::idle(host)?)?;
                Item::noted(&title, &value, &help)
            }
        }
    }

    /// The rows of the networks in reach, each made from the network read
    /// and its name moved into the row, not copied; `info` is where the radio
    /// is (read by the caller).
    fn read_networks_view(&mut self, host: &Host<'_>, info: &Named) -> Result<Networks, Error> {
        let networks = if info.radio.is_on() {
            self.read_networks(host)?
        } else {
            Vec::new()
        };
        let kept = self.read_kept(host)?;
        let mut say = Messages::new()?;
        let mut items = room(networks.len().saturating_add(1))?;
        let mut marks = room(networks.len())?;
        let empty = networks.is_empty();
        for network in networks {
            let same = info.ssid == network.ssid;
            let link = if info.radio == Radio::Online && same {
                Link::Connected
            } else if info.radio == Radio::Joining && same {
                Link::Connecting
            } else {
                Link::Apart
            };
            let mut note = String::new();
            for (n, key) in note_keys(link, is_kept(&kept, &network.ssid), &network)?
                .iter()
                .enumerate()
            {
                let word = say.copy(host, key)?;
                note = if n == 0 {
                    word
                } else {
                    concat(&[&note, " · ", &word])?
                };
            }
            marks.push(Mark {
                signal: network.signal,
                secured: network.secured,
            });
            items.push(row_item(network.ssid, note));
        }
        let again = say.copy(host, "wifi.search")?;
        let note = if info.scanning {
            say.copy(host, "wifi.state.searching")?
        } else if empty {
            say.copy(host, "wifi.no-networks")?
        } else {
            String::new()
        };
        items.push(row_item(again, note));
        Ok(Networks {
            marks,
            list: ItemList::new(items),
        })
    }

    fn opened(
        &mut self,
        host: &Host<'_>,
        token: u32,
        instance: Instance,
        now: u64,
    ) -> Result<(), Error> {
        self.ensure(host)?;
        let view = if token == PAGE_TOKEN {
            self.search_if_idle(host)?;
            View::Page(self.read_page(host)?)
        } else if token == NETWORKS_TOKEN {
            let info = self.read_named(host)?;
            View::Networks(self.read_networks_view(host, &info)?)
        } else {
            match self.phone_view(host, now) {
                Ok(view) => View::Phone(view),
                Err(_) => {
                    // The setup could not be made (no memory, no device): said,
                    // and the screen closed, not a failed callback.
                    native_model::best_effort(Self::notice(host, "wifi.phone.not-opened"));
                    return CommandSender::new(host, &mut [0; 64]).back();
                }
            }
        };
        self.screens.open(instance, view)
    }

    /// Looks for networks if the radio is on and nothing else goes on (a scan
    /// takes the radio's time).
    fn search_if_idle(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let info = self.read_info(host)?;
        if info.radio.is_on()
            && !info.scanning
            && self.policy.goal == crate::native_policy::Goal::Nothing
        {
            let mut scratch = [0; 64];
            CommandSender::new(host, &mut scratch).wifi_scan()?;
        }
        Ok(())
    }

    /// The rows as they are now: the radio and the networks change while a
    /// screen is open. The old rows are freed before the new are made: never
    /// both at once (a refusal leaves the view without rows until the next
    /// event).
    fn reread(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if let Some(View::Page(old)) = self.screens.top(PAGE_TOKEN) {
            let focus = old.list.focus();
            *old = Page::empty();
            let fresh = self.read_page(host)?;
            if let Some(View::Page(view)) = self.screens.top(PAGE_TOKEN) {
                *view = fresh;
                view.list.set_focus(focus);
            }
        }
        if self.screens.top(NETWORKS_TOKEN).is_some() {
            let info = self.read_named(host)?;
            if !info.radio.is_on() {
                // Nothing to choose from here: this page goes, not what
                // lies above it.
                let mut scratch = [0; 64];
                return CommandSender::new(host, &mut scratch).back();
            }
            let mut focus = 0;
            if let Some(View::Networks(old)) = self.screens.top(NETWORKS_TOKEN) {
                focus = old.list.focus();
                *old = Networks::empty();
            }
            let fresh = self.read_networks_view(host, &info)?;
            if let Some(View::Networks(view)) = self.screens.top(NETWORKS_TOKEN) {
                *view = fresh;
                view.list.set_focus(focus);
            }
        }
        redraw(host)
    }

    /// Keeps the password typed for the network `ssid` the device is in now.
    /// Where the device cannot keep it (no secrets service, or `Unsupported`
    /// from the one that is there: owner decision 141, the X4 Classic's
    /// table has no room for passwords), the user is told with
    /// the shared `core.secrets-unsupported` and asked again next time.
    fn keep_secret(&mut self, host: &Host<'_>, ssid: &str, password: &str) -> Result<(), Error> {
        if password.is_empty() {
            return Ok(());
        }
        let remember = Remember::new(ssid, password)?;
        let saved = match self.secrets(host)? {
            Some(secrets) => native_known::remember(secrets, host, &remember),
            None => Err(Error::Unsupported),
        };
        match saved {
            Ok(()) => Ok(()),
            Err(Error::NoMemory) => Err(Error::NoMemory),
            // Not saved: said, best effort.
            Err(_) => {
                native_model::best_effort(Self::notice(host, "core.secrets-unsupported"));
                Ok(())
            }
        }
    }

    /// The networks in `wifi.ini` on the card, read once at a start: kept
    /// among the secrets, then the file is removed. A file that cannot be kept
    /// (the device keeps no passwords, or a call failed) stays for the next
    /// start; a network that cannot be one (a name over 32 bytes, a password
    /// over 63) is dropped with the file.
    fn keep_file(&mut self, host: &Host<'_>) -> Result<(), Error> {
        self.ensure(host)?;
        if !self.read_info(host)?.present {
            return Ok(());
        }
        let found = match native_ini::read(host)? {
            Content::Nothing => return Ok(()),
            Content::TooLarge => {
                native_model::best_effort(Self::notice(host, "wifi.file-too-large"));
                return Ok(());
            }
            Content::Networks(found) => found,
        };
        let kept = self.read_kept(host)?;
        for network in &found {
            // A network without a password does not take the password kept.
            if network.password.is_empty() && is_kept(&kept, &network.ssid) {
                continue;
            }
            let Ok(remember) = Remember::new(&network.ssid, network.password.as_str()) else {
                continue;
            };
            let saved = match self.secrets(host)? {
                Some(secrets) => native_known::remember(secrets, host, &remember),
                None => Err(Error::Unsupported),
            };
            match saved {
                Ok(()) => {}
                Err(Error::NoMemory) => return Err(Error::NoMemory),
                Err(Error::Unsupported) => {
                    native_model::best_effort(Self::notice(host, "core.secrets-unsupported"));
                    return Ok(());
                }
                Err(_) => return Ok(()),
            }
        }
        native_ini::remove(host);
        Ok(())
    }

    /// Does what the policy says to the radio.
    fn run(&mut self, host: &Host<'_>, steps: Vec<Step>) -> Result<(), Error> {
        for step in steps {
            let mut scratch = [0; 512];
            let mut sender = CommandSender::new(host, &mut scratch);
            match step {
                Step::On => sender.wifi_on()?,
                Step::Scan => sender.wifi_scan()?,
                Step::Join { ssid, password } => {
                    self.request = self.request.wrapping_add(1).max(1);
                    let result = sender.wifi_join(self.request, &ssid, password.as_str(), false);
                    // The command buffer held the password.
                    wipe(&mut scratch);
                    result?;
                }
            }
        }
        Ok(())
    }

    fn radio_for_policy(&self, host: &Host<'_>) -> Result<Radio, Error> {
        Ok(self.read_info(host)?.radio)
    }

    fn chosen(&mut self, host: &Host<'_>, token: u32, index: usize) -> Result<(), Error> {
        self.ensure(host)?;
        match self.screens.top(token) {
            Some(View::Page(view)) => {
                let Some(row) = view.rows.get(index).copied() else {
                    return Ok(());
                };
                self.page_row(host, row)
            }
            Some(View::Networks(view)) => {
                // A copy: the choice reads the services, which borrow the plugin.
                if let Some(network) = view.network(index)? {
                    self.network(host, &network)
                } else {
                    // The last row: look again, unless a scan runs.
                    if !self.read_info(host)?.scanning {
                        let mut scratch = [0; 64];
                        CommandSender::new(host, &mut scratch).wifi_scan()?;
                    }
                    redraw(host)
                }
            }
            Some(View::Phone(_)) => {
                // Only the button's row is selectable in effect: choosing it
                // goes to the other step.
                self.phone_action(host, PhoneAction::Confirm).map(|_| ())
            }
            _ => Ok(()),
        }
    }

    fn page_row(&mut self, host: &Host<'_>, row: PageRow) -> Result<(), Error> {
        let mut scratch = [0; 128];
        match row {
            PageRow::Radio => {
                let on = self.read_info(host)?.radio.is_on();
                self.policy.manual = !on;
                let mut sender = CommandSender::new(host, &mut scratch);
                if on {
                    sender.wifi_off()?
                } else {
                    sender.wifi_on()?
                }
            }
            PageRow::Networks => {
                return CommandSender::new(host, &mut scratch).open(Open::screen("wifi.networks")?);
            }
            PageRow::Phone => {
                return CommandSender::new(host, &mut scratch).open(Open::screen("wifi.phone")?);
            }
            PageRow::OnDemand | PageRow::WhileCharging => {
                let key = if row == PageRow::OnDemand {
                    ON_DEMAND
                } else {
                    WHILE_CHARGING
                };
                let on = Self::flag(host, key)?;
                CommandSender::new(host, &mut scratch).set_setting(key, SettingValue::Bool(!on))?;
            }
            PageRow::IdleOff => {
                let next = next_idle(&Self::idle(host)?);
                CommandSender::new(host, &mut scratch)
                    .set_setting(IDLE_OFF, SettingValue::Text(next))?;
            }
        }
        // The rows follow the radio's and the settings' events; drawn at once too.
        self.reread(host)
    }

    /// A network was chosen: joined (a kept one at once, an open one once,
    /// a secured one after its password is typed).
    fn network(&mut self, host: &Host<'_>, network: &Network) -> Result<(), Error> {
        let info = self.read_named(host)?;
        let kept = self.read_kept(host)?;
        let saved = is_kept(&kept, &network.ssid);
        let at = matches!(info.radio, Radio::Online | Radio::Joining).then_some(info.ssid.as_str());
        match pick(at, saved, network) {
            Pick::Nothing => Ok(()),
            Pick::Kept => {
                let password = match self.secrets(host)? {
                    Some(secrets) => native_known::password_of(secrets, host, &network.ssid)?,
                    None => None,
                };
                let password = match password {
                    Some(password) => password,
                    None => Password::new("")?,
                };
                let steps = self
                    .policy
                    .choose(info.radio, &network.ssid, password.as_str())?;
                self.run(host, steps)?;
                redraw(host)
            }
            Pick::Open => {
                let steps = self.policy.choose(info.radio, &network.ssid, "")?;
                self.run(host, steps)?;
                redraw(host)
            }
            Pick::Ask => self.ask_password(host, &network.ssid),
        }
    }

    /// Asks for the password of `ssid` (typed masked), offering the one typed
    /// before and not taken.
    fn ask_password(&mut self, host: &Host<'_>, ssid: &str) -> Result<(), Error> {
        let mut say = Messages::new()?;
        let title = say.format(
            host,
            "wifi.password",
            &[Arg::Text {
                name: "ssid",
                value: ssid,
            }],
        )?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let question = Question::text(&title, "")?.secret()?;
        let ui = self.ui.as_ref().ok_or(Error::State)?;
        let asked = ui
            .ask(host, question, &mut input, &mut out)
            .and_then(|request| {
                if let Some((pending, typed)) = &self.policy.pending
                    && pending == ssid
                {
                    ui.text_write(
                        host,
                        request.text(),
                        0,
                        typed.as_str(),
                        paperpaw_sdk::native::ui::TextWrite::Last,
                        &mut input,
                        &mut out,
                    )?;
                }
                Ok(request)
            });
        // The typed password went through both buffers: wiped on every path.
        wipe(&mut input);
        wipe(&mut out);
        let request = asked?;
        let ssid = copy(ssid)?;
        self.asked = Some((request, ssid));
        Ok(())
    }

    /// The password was typed: the network is joined with it, and it is
    /// kept once the device is in the network.
    fn answered(&mut self, host: &Host<'_>, answer: Answered) -> Result<(), Error> {
        let Some((request, _)) = &self.asked else {
            return Ok(());
        };
        if *request != answer.request() || answer.outcome() == AnswerOutcome::Typing {
            return Ok(());
        }
        let Some((request, ssid)) = self.asked.take() else {
            return Ok(());
        };
        if answer.outcome() != AnswerOutcome::Answered {
            return Ok(());
        }
        let Some(password) = self.read_password(host, request)? else {
            // Longer than a network's password can be: said, not joined.
            native_model::best_effort(Self::tell(host, "wifi.password.too-long", &ssid));
            return Ok(());
        };
        let radio = self.radio_for_policy(host)?;
        let steps = self.policy.choose(radio, &ssid, password.as_str())?;
        self.policy.pending = Some((ssid, password));
        self.run(host, steps)?;
        redraw(host)
    }

    /// The password typed, read with buffers that are wiped; `None` for one
    /// longer than a network's password can be.
    fn read_password(
        &self,
        host: &Host<'_>,
        request: RequestId,
    ) -> Result<Option<Password>, Error> {
        let ui = self.ui.as_ref().ok_or(Error::State)?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let result = ui
            .text_read(host, request.text(), 0, &mut input, &mut out)
            .and_then(|part| {
                let total = usize::try_from(part.total()).map_err(|_| Error::NoSpace)?;
                if total > PASSWORD_MOST {
                    return Ok(None);
                }
                if part.data().len() != total {
                    return Err(Error::Invalid);
                }
                Password::new(part.data()).map(Some)
            });
        wipe(&mut input);
        wipe(&mut out);
        result
    }

    /// A long press (or the menu button) forgets a network that is kept.
    fn held(&mut self, host: &Host<'_>, token: u32, index: usize) -> Result<(), Error> {
        if token != NETWORKS_TOKEN {
            return Ok(());
        }
        let Some(View::Networks(view)) = self.screens.top(token) else {
            return Ok(());
        };
        let Some(ssid) = view.ssid(index).map(copy) else {
            return Ok(());
        };
        let ssid = ssid?;
        if !is_kept(&self.read_kept(host)?, &ssid) {
            return Ok(());
        }
        if let Some(secrets) = self.secrets(host)? {
            native_known::forget(secrets, host, &ssid)?;
        }
        if self.ssid_is(host, &ssid)? {
            let mut scratch = [0; 64];
            CommandSender::new(host, &mut scratch).wifi_leave()?;
        }
        native_model::best_effort(Self::tell(host, "wifi.forgotten", &ssid));
        self.reread(host)
    }

    /// A notice of one text.
    fn notice(host: &Host<'_>, key: &str) -> Result<(), Error> {
        let text = Messages::new()?.copy(host, key)?;
        let mut scratch = [0; 512];
        CommandSender::new(host, &mut scratch).notify(Notify::new(&text)?)
    }

    /// A notice with the network's `ssid`.
    fn tell(host: &Host<'_>, key: &str, ssid: &str) -> Result<(), Error> {
        let mut say = Messages::new()?;
        let text = say.format(
            host,
            key,
            &[Arg::Text {
                name: "ssid",
                value: ssid,
            }],
        )?;
        let mut scratch = [0; 512];
        CommandSender::new(host, &mut scratch).notify(Notify::new(&text)?)
    }
}

fn redraw(host: &Host<'_>) -> Result<(), Error> {
    let mut scratch = [0; 64];
    CommandSender::new(host, &mut scratch).queue_redraw(RedrawMode::Partial)
}
