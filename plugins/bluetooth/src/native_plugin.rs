//! The flat Bluetooth plugin: the page of the bonded devices, adding a
//! device and when remotes' keys go to the key maps.
use crate::{
    declarations::{EVENTS_TOKEN, OPEN_TOKEN, PAGE_TOKEN, REMOTES, SEARCH_TOKEN},
    native_model::{
        self, Adding, Bond, Choice, Edge, Found, Kind, Plan, Policy, Row, Search, Step, User,
    },
};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Handled,
    native::{
        Error, Host, Plugin,
        callbacks::{DrawFrame, Input},
        commands::CommandSender,
        device::{Device, DeviceCursor},
        events::{BluetoothKind, BookKind, Event},
        formatting::Arg,
        registry::{PageCursor, Registry, RegistryNeedKind},
        reply::Reply,
        settings::{self, SettingValue},
        ui::{Instance, Notify, Open},
    },
};
use paperpaw_ui_kit::{
    Footer, Item, ItemList, ItemStep, Messages, Screens,
    mem::{SERVICE_BUFFER, buffer, push},
    text::{concat, copy},
};

mod image {
    include!(concat!(env!("OUT_DIR"), "/bluetooth_declarations.rs"));
}

/// How long a search runs: the reader's own, so half the time, asking
/// devices for their names.
const SEARCH_MS: u32 = 30_000;
/// Pages of the bonded devices read at most (the host lists up to 16).
const MOST_PAGES: usize = 4;
/// Pages of the plugins read at most (the plugins are about seventy).
const MOST_PLUGIN_PAGES: usize = 16;
/// The feature a plugin that uses Bluetooth needs.
const USES: &str = "uses.bluetooth";
/// This plugin: a sensor is not given to the one that manages it.
const SELF_ID: &str = "bluetooth";

#[derive(Debug)]
struct Page {
    bonds: Vec<Bond>,
    users: Vec<User>,
    rows: Vec<Row>,
    list: ItemList,
}

#[derive(Debug)]
struct Adder {
    search: Search,
    list: ItemList,
}

#[derive(Debug)]
enum View {
    Page(Page),
    Search(Adder),
}

/// The interface-7 Bluetooth plugin.
#[derive(Debug, Default)]
pub struct NativeBluetooth {
    device: Option<Device>,
    registry: Option<Registry>,
    screens: Screens<View>,
    /// A book is open.
    reading: bool,
}

impl Plugin for NativeBluetooth {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::SETTINGS
            .iter()
            .chain(image::SCREENS)
            .chain(image::NAVIGATION)
            .chain(image::EVENTS)
        {
            paperpaw_sdk::native::registrations::submit(host, batch)?;
        }
        // The services are opened by the first callback: opening needs a live frame.
        Ok(Self::default())
    }

    fn event(
        &mut self,
        host: &Host<'_>,
        token: u32,
        event: Event<'_>,
        _: &mut Reply<'_>,
    ) -> Result<(), Error> {
        match event {
            Event::ActionRun { .. } if token == OPEN_TOKEN => {
                let mut scratch = [0; 128];
                CommandSender::new(host, &mut scratch).open(Open::screen("bluetooth.page")?)
            }
            Event::Started { .. } | Event::Resume { .. } if token == EVENTS_TOKEN => {
                self.remotes(host, Edge::Awake)
            }
            Event::Book { kind, .. } if token == EVENTS_TOKEN => match kind {
                BookKind::Opened => {
                    self.reading = true;
                    self.remotes(host, Edge::BookOpened)
                }
                BookKind::Closed => {
                    self.reading = false;
                    self.remotes(host, Edge::BookClosed)
                }
                _ => Ok(()),
            },
            Event::SettingChanged { .. } if token == EVENTS_TOKEN => self.reread(host),
            Event::Bluetooth(change) => self.changed(host, change),
            Event::ScreenOpened { instance, .. }
                if token == PAGE_TOKEN || token == SEARCH_TOKEN =>
            {
                self.opened(host, token, instance)
            }
            Event::ScreenShown {
                instance: Some(instance),
                ..
            } if token == PAGE_TOKEN || token == SEARCH_TOKEN => {
                self.screens.shown(token, instance)?;
                self.reread(host)
            }
            Event::ScreenClosed { instance, .. } => {
                self.screens.close(instance);
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn draw(&mut self, host: &Host<'_>, token: u32, mut frame: DrawFrame<'_>) -> Result<(), Error> {
        let mut messages = Messages::new()?;
        match self.screens.top(token).ok_or(Error::State)? {
            View::Page(view) => {
                let title = messages.copy(host, "bluetooth.title")?;
                view.list
                    .draw_with(host, &mut frame, &title, true, Footer::None)
            }
            View::Search(view) => {
                let title = messages.copy(host, "bluetooth.add")?;
                view.list
                    .draw_with(host, &mut frame, &title, true, Footer::None)
            }
        }
    }

    fn input(&mut self, host: &Host<'_>, token: u32, input: Input<'_>) -> Result<Handled, Error> {
        let Some(view) = self.screens.top(token) else {
            return Ok(Handled::No);
        };
        let list = match view {
            View::Page(view) => &mut view.list,
            View::Search(view) => &mut view.list,
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

impl NativeBluetooth {
    fn ensure(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if self.device.is_none() {
            self.device = Some(Device::open(host)?);
        }
        if self.registry.is_none() {
            self.registry = Some(Registry::open(host)?);
        }
        Ok(())
    }

    fn policy(host: &Host<'_>) -> Result<Policy, Error> {
        let mut scratch = [0; 64];
        Ok(match settings::read_setting(host, REMOTES, &mut scratch)? {
            SettingValue::Text(key) => Policy::from_key(key),
            _ => Policy::ByHand,
        })
    }

    /// Asks the bonded remotes what the edge calls for under the setting.
    fn remotes(&mut self, host: &Host<'_>, edge: Edge) -> Result<(), Error> {
        let plan = native_model::plan(Self::policy(host)?, edge, self.reading);
        if plan == Plan::Nothing {
            return Ok(());
        }
        self.ensure(host)?;
        let bonds = self.read_bonds(host)?;
        let mut scratch = [0; 128];
        for bond in &bonds {
            if let Some(on) = native_model::switched(plan, bond) {
                CommandSender::new(host, &mut scratch).bt_keys(bond.id, on)?;
            }
        }
        Ok(())
    }

    /// The devices bonded with this one; none where there is no radio.
    fn read_bonds(&self, host: &Host<'_>) -> Result<Vec<Bond>, Error> {
        let device = self.device.as_ref().ok_or(Error::State)?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        'again: for _ in 0..2 {
            let mut bonds = Vec::new();
            let mut cursor = DeviceCursor::start();
            for _ in 0..MOST_PAGES {
                let page = match device.bonded(host, cursor, &mut input, &mut out) {
                    Ok(page) => page,
                    Err(Error::NotFound) => return Ok(Vec::new()),
                    // The list changed under the reading: start over.
                    Err(Error::State) => continue 'again,
                    Err(error) => return Err(error),
                };
                for bond in page.rows() {
                    push(
                        &mut bonds,
                        Bond {
                            id: bond.device(),
                            name: copy(bond.name())?,
                            kind: Kind::from_wire(bond.kind()),
                            connected: bond.connected(),
                            keys: bond.remote_keys(),
                            given: copy(bond.given())?,
                        },
                    )?;
                }
                match page.end().next_cursor() {
                    Some(next) => cursor = next,
                    None => return Ok(bonds),
                }
            }
            return Err(Error::NoSpace);
        }
        Err(Error::State)
    }

    /// The plugins besides this one that use Bluetooth: a sensor may be
    /// given to them.
    fn read_users(&self, host: &Host<'_>) -> Result<Vec<User>, Error> {
        let registry = self.registry.as_ref().ok_or(Error::State)?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let mut ids: Vec<(String, String)> = Vec::new();
        let mut cursor = PageCursor::start();
        for _ in 0..MOST_PLUGIN_PAGES {
            let page = registry.plugins(host, cursor, &mut input, &mut out)?;
            for plugin in page.rows() {
                if plugin.id() != SELF_ID {
                    push(&mut ids, (copy(plugin.id())?, copy(plugin.name())?))?;
                }
            }
            match page.end().next_cursor() {
                Some(next) => cursor = next,
                None => break,
            }
        }
        let mut users = Vec::new();
        for (id, name) in ids {
            if self.uses_bluetooth(host, &id, &mut input, &mut out)? {
                push(&mut users, User { id, name })?;
            }
        }
        Ok(users)
    }

    fn uses_bluetooth(
        &self,
        host: &Host<'_>,
        id: &str,
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<bool, Error> {
        let registry = self.registry.as_ref().ok_or(Error::State)?;
        let mut cursor = PageCursor::start();
        for _ in 0..MOST_PLUGIN_PAGES {
            let page = match registry.needs(host, id, cursor, input, out) {
                Ok(page) => page,
                Err(Error::NotFound) => return Ok(false),
                Err(error) => return Err(error),
            };
            if page
                .rows()
                .any(|need| need.kind() == RegistryNeedKind::Feature && need.key() == USES)
            {
                return Ok(true);
            }
            match page.end().next_cursor() {
                Some(next) => cursor = next,
                None => return Ok(false),
            }
        }
        Ok(false)
    }

    fn read_page(&self, host: &Host<'_>) -> Result<Page, Error> {
        let bonds = self.read_bonds(host)?;
        let users = if native_model::wants_users(&bonds) {
            self.read_users(host)?
        } else {
            Vec::new()
        };
        let rows = native_model::rows(&bonds, &users)?;
        let mut messages = Messages::new()?;
        let mut items = Vec::new();
        for row in &rows {
            push(
                &mut items,
                self.page_item(host, &mut messages, &bonds, &users, *row)?,
            )?;
        }
        Ok(Page {
            bonds,
            users,
            rows,
            list: ItemList::new(items),
        })
    }

    fn page_item(
        &self,
        host: &Host<'_>,
        say: &mut Messages,
        bonds: &[Bond],
        users: &[User],
        row: Row,
    ) -> Result<Item, Error> {
        match row {
            Row::Device(at) => {
                let bond = bonds.get(at).ok_or(Error::State)?;
                let kind = say.copy(host, bond.kind.key())?;
                let note = if bond.connected {
                    let connected = say.copy(host, "bluetooth.connected")?;
                    concat(&[&kind, " · ", &connected])?
                } else {
                    kind
                };
                let state = if bond.kind.can_type() {
                    say.copy(host, switch(bond.keys))?
                } else {
                    String::new()
                };
                Item::noted(&bond.name, &state, &note)
            }
            Row::Give(at, user) => {
                let bond = bonds.get(at).ok_or(Error::State)?;
                let user = users.get(user).ok_or(Error::State)?;
                let title = say.format(
                    host,
                    "bluetooth.give",
                    &[Arg::Text {
                        name: "plugin",
                        value: &user.name,
                    }],
                )?;
                let state = say.copy(host, switch(bond.given == user.id))?;
                Item::row(&title, &state)
            }
            Row::Add => {
                let title = say.copy(host, "bluetooth.add")?;
                let help = say.copy(host, "bluetooth.add.help")?;
                Item::noted(&title, "›", &help)
            }
            Row::Remotes => {
                let title = say.copy(host, "bluetooth.remotes")?;
                let help = say.copy(host, "bluetooth.remotes.help")?;
                let value = say.copy(host, Self::policy(host)?.key())?;
                Item::noted(&title, &value, &help)
            }
        }
    }

    fn search_items(host: &Host<'_>, search: &Search) -> Result<Vec<Item>, Error> {
        let mut say = Messages::new()?;
        let mut items = Vec::new();
        for device in &search.found {
            let name = if device.name.is_empty() {
                say.copy(host, "bluetooth.unnamed")?
            } else {
                copy(&device.name)?
            };
            let kind = say.copy(host, device.kind.key())?;
            let state = match search.adding {
                Some(Adding::Connecting(id)) if id == device.id => {
                    Some(say.copy(host, "bluetooth.connecting")?)
                }
                Some(Adding::Pairing(id)) if id == device.id => {
                    Some(say.copy(host, "bluetooth.pairing")?)
                }
                _ if device.bonded => Some(say.copy(host, "bluetooth.paired-before")?),
                _ => None,
            };
            let note = match state {
                Some(state) => concat(&[&kind, " · ", &state])?,
                None => kind,
            };
            push(&mut items, Item::noted(&name, "›", &note)?)?;
        }
        let note = match search.problem {
            Some(problem) => say.copy(host, problem.key())?,
            None if search.searching => say.copy(host, "bluetooth.searching")?,
            None if search.found.is_empty() => say.copy(host, "bluetooth.none-found")?,
            None => String::new(),
        };
        let again = say.copy(host, "bluetooth.search-again")?;
        push(&mut items, Item::noted(&again, "", &note)?)?;
        Ok(items)
    }

    fn opened(&mut self, host: &Host<'_>, token: u32, instance: Instance) -> Result<(), Error> {
        self.ensure(host)?;
        if token == PAGE_TOKEN {
            let page = self.read_page(host)?;
            return self.screens.open(instance, View::Page(page));
        }
        let mut search = Search::default();
        search.start();
        let items = Self::search_items(host, &search)?;
        self.screens.open(
            instance,
            View::Search(Adder {
                search,
                list: ItemList::new(items),
            }),
        )?;
        let mut scratch = [0; 128];
        CommandSender::new(host, &mut scratch).bt_scan(SEARCH_MS)
    }

    /// The rows as they are now: devices come, go and change while a screen is open.
    fn reread(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if self.screens.top(PAGE_TOKEN).is_some() {
            let mut fresh = self.read_page(host)?;
            if let Some(View::Page(old)) = self.screens.top(PAGE_TOKEN) {
                fresh.list.set_focus(old.list.focus());
                *old = fresh;
            }
        }
        self.refresh_search(host)?;
        redraw(host)
    }

    fn refresh_search(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let items = match self.screens.top(SEARCH_TOKEN) {
            Some(View::Search(view)) => Self::search_items(host, &view.search)?,
            _ => return Ok(()),
        };
        if let Some(View::Search(view)) = self.screens.top(SEARCH_TOKEN) {
            let focus = view.list.focus();
            view.list.replace(items);
            view.list.set_focus(focus);
        }
        Ok(())
    }

    /// An event of the radio: sent to the plugin bound to `core.bluetooth`.
    fn changed(
        &mut self,
        host: &Host<'_>,
        change: paperpaw_sdk::native::events::BluetoothChange<'_>,
    ) -> Result<(), Error> {
        let mut step = Step::Nothing;
        let device = change.device();
        let mut searched = false;
        if let Some(View::Search(view)) = self.screens.top(SEARCH_TOKEN) {
            searched = true;
            let search = &mut view.search;
            match change.kind() {
                BluetoothKind::Found => search.found(Found {
                    id: device,
                    name: copy(change.name())?,
                    kind: Kind::from_event(change.device_kind()),
                    signal: change.signal(),
                    bonded: false,
                })?,
                BluetoothKind::ScanEnded => search.ended(),
                BluetoothKind::Failed => search.failed(device, change.reason()),
                BluetoothKind::Disconnected => {
                    if let Some(reason) = change.reason() {
                        search.lost(device, reason);
                    }
                }
                BluetoothKind::Connected => step = search.connected(device),
                BluetoothKind::Paired => {
                    step = search.paired(
                        device,
                        Kind::from_event(change.device_kind()),
                        change.name(),
                    )?;
                }
                _ => {}
            }
        }
        match step {
            Step::Pair(id) => {
                let mut scratch = [0; 128];
                CommandSender::new(host, &mut scratch).bt_pair(id)?;
            }
            Step::Paired { id, typing, name } => {
                let mut scratch = [0; 256];
                if typing {
                    CommandSender::new(host, &mut scratch).bt_keys(id, true)?;
                }
                CommandSender::new(host, &mut scratch).bt_disconnect(id)?;
                let mut say = Messages::new()?;
                let text = say.format(
                    host,
                    "bluetooth.paired",
                    &[Arg::Text {
                        name: "name",
                        value: &name,
                    }],
                )?;
                let mut sender = CommandSender::new(host, &mut scratch);
                sender.notify(Notify::new(&text)?)?;
                sender.back()?;
            }
            Step::Nothing => {}
        }
        if searched {
            self.refresh_search(host)?;
        }
        if matches!(
            change.kind(),
            BluetoothKind::Changed | BluetoothKind::Done | BluetoothKind::Paired
        ) {
            return self.reread(host);
        }
        redraw(host)
    }

    fn chosen(&mut self, host: &Host<'_>, token: u32, index: usize) -> Result<(), Error> {
        self.ensure(host)?;
        let mut scratch = [0; 512];
        match self.screens.top(token) {
            Some(View::Page(view)) => {
                let Some(row) = view.rows.get(index).copied() else {
                    return Ok(());
                };
                match row {
                    Row::Device(at) => {
                        if let Some(bond) = view.bonds.get(at).filter(|b| b.kind.can_type()) {
                            CommandSender::new(host, &mut scratch).bt_keys(bond.id, !bond.keys)?;
                        }
                    }
                    Row::Give(at, user) => {
                        let (Some(bond), Some(user)) = (view.bonds.get(at), view.users.get(user))
                        else {
                            return Ok(());
                        };
                        let mut sender = CommandSender::new(host, &mut scratch);
                        if bond.given == user.id {
                            sender.bt_take_back(bond.id)?;
                        } else {
                            sender.bt_give(bond.id, &user.id)?;
                        }
                    }
                    Row::Add => {
                        return CommandSender::new(host, &mut scratch)
                            .open(Open::screen("bluetooth.search")?);
                    }
                    Row::Remotes => {
                        let next = Self::policy(host)?.next();
                        CommandSender::new(host, &mut scratch)
                            .set_setting(REMOTES, SettingValue::Text(next))?;
                    }
                }
                // The rows follow the answer of the host (`Done`, `Changed`).
                redraw(host)
            }
            Some(View::Search(view)) => match view.search.choose(index) {
                Choice::Search => {
                    view.search.start();
                    CommandSender::new(host, &mut scratch).bt_scan(SEARCH_MS)?;
                    self.refresh_search(host)?;
                    redraw(host)
                }
                Choice::Add(id) => {
                    let mut sender = CommandSender::new(host, &mut scratch);
                    sender.bt_stop_scan()?;
                    sender.bt_connect(id)?;
                    self.refresh_search(host)?;
                    redraw(host)
                }
                Choice::Nothing => Ok(()),
            },
            None => Ok(()),
        }
    }

    /// A long press (or the menu button) forgets a device.
    fn held(&mut self, host: &Host<'_>, token: u32, index: usize) -> Result<(), Error> {
        if token != PAGE_TOKEN {
            return Ok(());
        }
        let Some(View::Page(view)) = self.screens.top(token) else {
            return Ok(());
        };
        let Some(Row::Device(at)) = view.rows.get(index).copied() else {
            return Ok(());
        };
        let Some(bond) = view.bonds.get(at) else {
            return Ok(());
        };
        let (id, name) = (bond.id, copy(&bond.name)?);
        let mut say = Messages::new()?;
        let text = say.format(
            host,
            "bluetooth.forgotten",
            &[Arg::Text {
                name: "name",
                value: &name,
            }],
        )?;
        let mut scratch = [0; 512];
        let mut sender = CommandSender::new(host, &mut scratch);
        sender.bt_forget(id)?;
        sender.notify(Notify::new(&text)?)?;
        redraw(host)
    }
}

/// The words of a switch.
const fn switch(on: bool) -> &'static str {
    if on {
        "bluetooth.switch.on"
    } else {
        "bluetooth.switch.off"
    }
}

fn redraw(host: &Host<'_>) -> Result<(), Error> {
    let mut scratch = [0; 64];
    CommandSender::new(host, &mut scratch)
        .queue_redraw(paperpaw_sdk::native::commands::RedrawMode::Partial)
}
