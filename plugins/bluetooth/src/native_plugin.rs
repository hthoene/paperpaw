//! The flat Bluetooth plugin: the page of the bonded devices, adding a
//! device, when remotes' keys go to the key maps, and the status item.
use crate::{
    declarations::{EVENTS_TOKEN, OPEN_TOKEN, PAGE_TOKEN, REMOTES, SEARCH_TOKEN, STATUS_TOKEN},
    native_model::{
        self, Adding, Bond, Choice, Closing, Edge, Found, Kind, Plan, Policy, Row, Search, Step,
        User,
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
        events::{BluetoothKind, BookKind, Event, Incoming},
        formatting::Arg,
        registry::{PageCursor, Registry, RegistryNeedKind},
        reply::Reply,
        settings::{self, SettingValue},
        ui::{
            ButtonHint, Instance, Notify, Open,
            stages::{StatusAnswer, StatusCall},
        },
    },
};
use paperpaw_ui_kit::{
    Footer, Item, ItemList, ItemStep, Messages, Screens,
    mem::{SERVICE_BUFFER, buffer, push},
    text::{concat, copy},
};

mod status_icon;

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
/// The status item's own id: the plugin's `bluetooth.status`.
const STATUS_ITEM: &str = "bluetooth.status";
/// The menu button, whose key-bar name is *Forget* on a device.
const MENU: &str = "core.menu";

#[derive(Debug)]
struct Page {
    bonds: Vec<Bond>,
    users: Vec<User>,
    rows: Vec<Row>,
    list: ItemList,
    /// The key bar names *Forget* for the menu button now.
    forget_named: bool,
}

impl Page {
    /// Whether row `index` is a device (which can be forgotten).
    fn device_at(&self, index: usize) -> bool {
        matches!(self.rows.get(index), Some(Row::Device(_)))
    }
}

#[derive(Debug)]
struct Adder {
    search: Search,
    list: ItemList,
    /// The ids of the devices bonded with this one: *paired before* in the search.
    bonded: Vec<u32>,
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
        incoming: Incoming<'_>,
        reply: &mut Reply<'_>,
    ) -> Result<(), Error> {
        let event = incoming.decode()?;
        match event {
            // The status item: the host asks which of its items show.
            Event::StageCall { stage, payload, .. } if token == STATUS_TOKEN => {
                self.status(host, stage, payload, reply)
            }
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
            // The page shows only its own setting: no other one reads it anew.
            Event::SettingChanged { key, .. } if token == EVENTS_TOKEN && key == REMOTES => {
                self.reread(host)
            }
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
                let closing = match self.screens.get(instance) {
                    Some(View::Search(view)) => Some(view.search.closing()),
                    _ => None,
                };
                self.screens.close(instance);
                if let Some(closing) = closing {
                    Self::search_closed(host, closing);
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn draw(&mut self, host: &Host<'_>, token: u32, mut frame: DrawFrame<'_>) -> Result<(), Error> {
        // The status item is drawn by its owner into the place the line gives it.
        if token == STATUS_TOKEN {
            return status_icon::draw(host, &mut frame);
        }
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
        let step = match view {
            // Select held, or the menu button, forgets the device that has the
            // focus, as a long press does on a touch screen (with the buttons
            // alone nothing else can); elsewhere they are the host's.
            View::Page(view) if forgets(&input) && view.device_at(view.list.focus()) => {
                ItemStep::Held(view.list.focus())
            }
            View::Page(view) => view.list.input(host, &input)?,
            View::Search(view) => view.list.input(host, &input)?,
        };
        match step {
            ItemStep::Chosen(index) => self.chosen(host, token, index)?,
            ItemStep::Held(index) => self.held(host, token, index)?,
            _ => {}
        }
        self.page_hint(host);
        Ok(ItemList::handled(step))
    }
}

/// Select held or the menu button: what forgets a device.
fn forgets(input: &Input<'_>) -> bool {
    matches!(
        input,
        Input::Action {
            action: "core.long-confirm" | "core.menu",
            ..
        }
    )
}

/// The key bar names *Forget* for the menu button while the focus is on a
/// device, as Beta 3 did, and the button's usual name elsewhere. Sent only
/// where that changes; a hint the host refuses leaves the bar as it was.
fn forget_hint(host: &Host<'_>, page: &mut Page) {
    let forget = page.device_at(page.list.focus());
    if forget == page.forget_named {
        return;
    }
    let word = if forget {
        match Messages::new().and_then(|mut say| say.copy(host, "bluetooth.forget")) {
            Ok(word) => Some(word),
            Err(_) => return,
        }
    } else {
        None
    };
    let hint = match &word {
        Some(word) => ButtonHint::words(MENU, word),
        None => ButtonHint::usual(MENU),
    };
    if let Ok(hint) = hint
        && CommandSender::new(host, &mut [0; 256])
            .set_hint(hint)
            .is_ok()
    {
        page.forget_named = forget;
    }
}

/// The answer for the status item: drawn (the sign, `status_icon`) while a
/// device is connected; no text hides the item.
fn status_answer(item: &str, connected: bool) -> Result<StatusAnswer<'_>, Error> {
    if connected {
        StatusAnswer::drawn(item)
    } else {
        StatusAnswer::text(item, "")
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
            forget_named: false,
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
        // The scan is asked for first: a refusal leaves no search showing.
        let mut scratch = [0; 128];
        CommandSender::new(host, &mut scratch).bt_scan(SEARCH_MS)?;
        let mut search = Search::default();
        search.start();
        let bonded = self.bonded_ids(host)?;
        let items = Self::search_items(host, &search)?;
        self.screens.open(
            instance,
            View::Search(Adder {
                search,
                list: ItemList::new(items),
                bonded,
            }),
        )
    }

    /// The ids of the bonded devices.
    fn bonded_ids(&self, host: &Host<'_>) -> Result<Vec<u32>, Error> {
        let mut ids = Vec::new();
        for bond in self.read_bonds(host)? {
            push(&mut ids, bond.id)?;
        }
        Ok(ids)
    }

    /// The rows as they are now: devices come, go and change while a screen is open.
    fn reread(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if !self.showing() {
            return Ok(());
        }
        if self.screens.top(PAGE_TOKEN).is_some() {
            let mut fresh = self.read_page(host)?;
            if let Some(View::Page(old)) = self.screens.top(PAGE_TOKEN) {
                fresh.list.set_focus(old.list.focus());
                fresh.forget_named = old.forget_named;
                *old = fresh;
            }
            self.page_hint(host);
        }
        self.refresh_search(host)?;
        redraw(host)
    }

    /// The page's key bar ([`forget_hint`]), while the page is the plugin's
    /// screen on top: the search opens over it, and a hint goes to the top one.
    fn page_hint(&mut self, host: &Host<'_>) {
        if self.screens.top(SEARCH_TOKEN).is_some() {
            return;
        }
        if let Some(View::Page(page)) = self.screens.top(PAGE_TOKEN) {
            forget_hint(host, page);
        }
    }

    /// Whether the page or the search is open: nothing is read or drawn otherwise.
    fn showing(&mut self) -> bool {
        self.screens.top(PAGE_TOKEN).is_some() || self.screens.top(SEARCH_TOKEN).is_some()
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
        // Nothing is read or drawn while neither screen is open.
        if !self.showing() {
            return Ok(());
        }
        let mut step = Step::Nothing;
        let device = change.device();
        let mut searched = false;
        // A bond came or went: *paired before* follows the bonded list.
        let fresh = if self.screens.top(SEARCH_TOKEN).is_some()
            && matches!(
                change.kind(),
                BluetoothKind::Changed | BluetoothKind::Paired
            ) {
            Some(self.bonded_ids(host)?)
        } else {
            None
        };
        if let Some(View::Search(view)) = self.screens.top(SEARCH_TOKEN) {
            searched = true;
            if let Some(fresh) = fresh {
                view.bonded = fresh;
                for known in &mut view.search.found {
                    known.bonded = view.bonded.contains(&known.id);
                }
            }
            let bonded = &view.bonded;
            let search = &mut view.search;
            match change.kind() {
                BluetoothKind::Found => search.found(Found {
                    id: device,
                    name: copy(change.name())?,
                    kind: Kind::from_event(change.device_kind()),
                    signal: change.signal(),
                    bonded: bonded.contains(&device),
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
                let mut scratch = [0; 512];
                if typing {
                    CommandSender::new(host, &mut scratch).bt_keys(id, true)?;
                }
                CommandSender::new(host, &mut scratch).bt_disconnect(id)?;
                // The search stays: an event's callback has no screen stack,
                // so a `back` here is refused, and the failed callback would
                // drop the commands above with it (#456).
                // The notice is last and best effort: if it cannot be said, the
                // pairing is done all the same.
                native_model::best_effort(Self::tell(host, "bluetooth.paired", &name));
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
                // The state follows the command: a refused command shows nothing new.
                Choice::Search => {
                    CommandSender::new(host, &mut scratch).bt_scan(SEARCH_MS)?;
                    view.search.start();
                    self.refresh_search(host)?;
                    redraw(host)
                }
                Choice::Add(id) => {
                    let mut sender = CommandSender::new(host, &mut scratch);
                    sender.bt_stop_scan()?;
                    sender.bt_connect(id)?;
                    view.search.begin_add(id);
                    self.refresh_search(host)?;
                    redraw(host)
                }
                Choice::Nothing => Ok(()),
            },
            None => Ok(()),
        }
    }

    /// The search's screen closed: the scan stops where it runs, and the
    /// device being added is let go. Each on its own, best effort: one the
    /// host refuses (the scan ended meanwhile) has nothing to end, and a
    /// failed callback would take the other, staged, with it.
    fn search_closed(host: &Host<'_>, closing: Closing) {
        let mut scratch = [0; 64];
        let mut sender = CommandSender::new(host, &mut scratch);
        if let Some(device) = closing.disconnect {
            native_model::best_effort(sender.bt_disconnect(device));
        }
        if closing.stop_scan {
            native_model::best_effort(sender.bt_stop_scan());
        }
    }

    /// The status item: drawn by the plugin (the Bluetooth sign,
    /// `status_icon`) while a device is connected, as in Beta 3, nothing
    /// else. A call that is not the host's `core.status` is not ours.
    fn status(
        &mut self,
        host: &Host<'_>,
        stage: &str,
        payload: &[u8],
        reply: &mut Reply<'_>,
    ) -> Result<(), Error> {
        if stage != "core.status" {
            return Err(Error::Unsupported);
        }
        let call = StatusCall::decode(payload)?;
        if self.device.is_none() {
            self.device = Some(Device::open(host)?);
        }
        let connected = self.any_connected(host)?;
        let mut answers = [None; 1];
        for item in call.items() {
            if item == STATUS_ITEM && answers[0].is_none() {
                answers[0] = Some(status_answer(item, connected)?);
            }
        }
        let [Some(answer)] = answers else {
            return Ok(());
        };
        let mut out = [0; 512];
        let length = call.reply(&[answer], &mut out)?;
        reply.write(out.get(..length).ok_or(Error::NoSpace)?)
    }

    /// Whether a device bonded with this one is connected now, no name
    /// copied; none where there is no radio.
    fn any_connected(&self, host: &Host<'_>) -> Result<bool, Error> {
        let device = self.device.as_ref().ok_or(Error::State)?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        'again: for _ in 0..2 {
            let mut cursor = DeviceCursor::start();
            for _ in 0..MOST_PAGES {
                let page = match device.bonded(host, cursor, &mut input, &mut out) {
                    Ok(page) => page,
                    Err(Error::NotFound) => return Ok(false),
                    // The list changed under the reading: start over.
                    Err(Error::State) => continue 'again,
                    Err(error) => return Err(error),
                };
                if page.rows().any(|bond| bond.connected()) {
                    return Ok(true);
                }
                match page.end().next_cursor() {
                    Some(next) => cursor = next,
                    None => return Ok(false),
                }
            }
            return Ok(false);
        }
        Err(Error::State)
    }

    /// A notice with the device's `name`.
    fn tell(host: &Host<'_>, key: &str, name: &str) -> Result<(), Error> {
        let mut say = Messages::new()?;
        let text = say.format(
            host,
            key,
            &[Arg::Text {
                name: "name",
                value: name,
            }],
        )?;
        let mut scratch = [0; 512];
        CommandSender::new(host, &mut scratch).notify(Notify::new(&text)?)
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
        let mut scratch = [0; 64];
        CommandSender::new(host, &mut scratch).bt_forget(id)?;
        // Said after the device is forgotten, best effort.
        native_model::best_effort(Self::tell(host, "bluetooth.forgotten", &name));
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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "test assertions")]
    use super::*;
    use alloc::format;

    #[test]
    fn select_held_and_the_menu_button_forget_and_other_keys_do_not() {
        let action = |action| Input::Action { now_ms: 0, action };
        assert!(forgets(&action("core.long-confirm")));
        assert!(forgets(&action("core.menu")));
        assert!(!forgets(&action("core.confirm")));
        assert!(!forgets(&action("core.back")));
    }

    #[test]
    fn only_a_device_row_is_forgotten() {
        let page = Page {
            bonds: Vec::new(),
            users: Vec::new(),
            rows: alloc::vec![Row::Device(0), Row::Give(0, 0), Row::Add, Row::Remotes],
            list: ItemList::new(Vec::new()),
            forget_named: false,
        };
        let forgettable: Vec<bool> = (0..5).map(|row| page.device_at(row)).collect();
        assert_eq!(forgettable, [true, false, false, false, false]);
    }

    #[test]
    fn the_status_item_is_drawn_while_a_device_is_connected_and_hidden_otherwise() {
        let connected = format!("{:?}", status_answer(STATUS_ITEM, true).unwrap());
        assert!(
            connected.contains("flags: 1") && connected.contains("text: \"\""),
            "{connected}"
        );
        let none = format!("{:?}", status_answer(STATUS_ITEM, false).unwrap());
        assert!(
            none.contains("flags: 0") && none.contains("text: \"\""),
            "{none}"
        );
    }
}
