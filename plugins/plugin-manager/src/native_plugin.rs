//! The flat plugin manager: the list of plugins, *Screens*, the notices at
//! a start and the offer to use a screen newly offered for a place.
use crate::{
    declarations::{EVENTS_TOKEN, LIST_TOKEN, MORE, OPEN_TOKEN, SCREENS_OPEN_TOKEN, SCREENS_TOKEN},
    native_model::{
        self, Kind, NotInstalled, Offer, PLACEHOLDER, PluginRow, Problem, Run, SafeMode, Say,
    },
};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Handled,
    native::{
        Error, Host, Plugin,
        callbacks::{DrawFrame, Input},
        commands::{CommandSender, PluginEnabled},
        device::{Device, DeviceCursor, NativeSafeMode, NotInstalledReason},
        events::{AnswerOutcome, Event, Incoming, kinds},
        formatting::Arg,
        registrations,
        registry::{MenuQuery, PageCursor, Registry},
        reply::Reply,
        storage::Root,
        ui::{HandoverId, Instance, Notify, Open, Param, Question, RequestId, Ui},
    },
};
use paperpaw_ui_kit::read_event;
use paperpaw_ui_kit::{
    Footer, Item, ItemList, ItemStep, Messages, Screens, files,
    mem::{SERVICE_BUFFER, buffer, push},
    text::{concat, copy},
};

mod image {
    include!(concat!(env!("OUT_DIR"), "/plugin_manager_declarations.rs"));
}

/// Pages of one list read at most (the plugins are about seventy).
const MOST_PAGES: usize = 16;
/// The longest description kept for a row: a note shows two lines.
const MOST_DESCRIPTION: usize = 160;
/// The places already seen, one `slot screen` a line, kept in the data folder.
const KNOWN: &str = "screens.txt";
/// The parameter of *Screens* naming the place whose candidates are listed.
const SLOT: &str = "slot";
/// The user bound the place (`reg_slot.flags` bit 0).
const SLOT_BOUND: u32 = 1;

/// What a row of the list does.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Target {
    /// Runs the action of a place to get more.
    More(String),
    /// Switches the plugin at this index.
    Plugin(usize),
}

#[derive(Debug, Default)]
struct List {
    plugins: Vec<PluginRow>,
    targets: Vec<Target>,
    list: ItemList,
    restart: bool,
}

/// What a row of *Screens* does.
#[derive(Debug, PartialEq, Eq)]
enum Pick {
    /// Opens the candidates of this place.
    Slot(String),
    /// The plugins' own order for the place.
    Automatic,
    /// This screen for the place.
    Candidate(String),
}

#[derive(Debug, Default)]
struct Places {
    /// The place whose candidates are listed; empty for the places.
    slot: String,
    title: String,
    picks: Vec<Pick>,
    list: ItemList,
}

#[derive(Debug)]
enum View {
    List(List),
    Places(Places),
}

/// The interface-7 plugin manager.
#[derive(Debug, Default)]
pub struct NativePluginManager {
    registry: Option<Registry>,
    device: Option<Device>,
    ui: Option<Ui>,
    screens: Screens<View>,
    /// The question asked: the place and the screen it is about.
    asked: Option<(RequestId, String, String)>,
}

impl Plugin for NativePluginManager {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::SCREENS
            .iter()
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
        _: &mut Reply<'_>,
    ) -> Result<(), Error> {
        let Some(event) = read_event!(incoming; kinds::ActionRun, kinds::Started, kinds::PluginsChanged, kinds::Answered, kinds::ScreenOpened, kinds::ScreenShown, kinds::ScreenClosed)
        else {
            return Ok(());
        };
        match event {
            Event::ActionRun { action, .. } if token == OPEN_TOKEN && action.ends_with(".open") => {
                let mut scratch = [0; 128];
                CommandSender::new(host, &mut scratch).open(Open::screen("plugin-manager.list")?)
            }
            Event::ActionRun { .. } if token == SCREENS_OPEN_TOKEN => {
                let mut scratch = [0; 128];
                CommandSender::new(host, &mut scratch).open(Open::screen("plugin-manager.screens")?)
            }
            Event::Started { .. } if token == EVENTS_TOKEN => {
                self.ensure(host)?;
                self.notices(host)?;
                self.offer(host)
            }
            Event::PluginsChanged { .. } if token == EVENTS_TOKEN => {
                self.ensure(host)?;
                self.offer(host)?;
                self.reread(host)
            }
            Event::Answered { answer, .. } => self.answered(host, answer),
            Event::ScreenOpened {
                instance, handover, ..
            } if token == LIST_TOKEN || token == SCREENS_TOKEN => {
                self.opened(host, token, instance, handover)
            }
            Event::ScreenShown {
                instance: Some(instance),
                ..
            } if token == LIST_TOKEN || token == SCREENS_TOKEN => {
                self.screens.shown(token, instance)?;
                // A choice made in the candidates of a place shows in the places.
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
            View::List(view) => {
                let title = messages.copy(host, "plugin-manager.title")?;
                let hint = messages.copy(
                    host,
                    if view.restart {
                        "plugin-manager.restart.short"
                    } else {
                        "plugin-manager.hint"
                    },
                )?;
                view.list
                    .draw_with(host, &mut frame, &title, true, Footer::Note(&hint))
            }
            View::Places(view) => {
                view.list
                    .draw_with(host, &mut frame, &view.title, true, Footer::None)
            }
        }
    }

    fn input(&mut self, host: &Host<'_>, token: u32, input: Input<'_>) -> Result<Handled, Error> {
        let Some(view) = self.screens.top(token) else {
            return Ok(Handled::No);
        };
        let list = match view {
            View::List(view) => &mut view.list,
            View::Places(view) => &mut view.list,
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

impl NativePluginManager {
    fn ensure(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if self.registry.is_none() {
            self.registry = Some(Registry::open(host)?);
        }
        if self.device.is_none() {
            self.device = Some(Device::open(host)?);
        }
        if self.ui.is_none() {
            self.ui = Some(Ui::open(host)?);
        }
        Ok(())
    }

    fn registry(&self) -> Result<&Registry, Error> {
        self.registry.as_ref().ok_or(Error::State)
    }

    fn opened(
        &mut self,
        host: &Host<'_>,
        token: u32,
        instance: Instance,
        handover: Option<HandoverId>,
    ) -> Result<(), Error> {
        self.ensure(host)?;
        let view = if token == LIST_TOKEN {
            View::List(self.read_list(host)?)
        } else {
            let slot = self.slot_param(host, handover)?;
            View::Places(self.read_places(host, slot)?)
        };
        self.screens.open(instance, view)
    }

    /// The place a *Screens* instance was opened for; empty for the places.
    fn slot_param(&self, host: &Host<'_>, handover: Option<HandoverId>) -> Result<String, Error> {
        let Some(handover) = handover else {
            return Ok(String::new());
        };
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let ui = self.ui.as_ref().ok_or(Error::State)?;
        for param in ui.params(host, handover, &mut input, &mut out)? {
            let param = param?;
            if param.key() == SLOT {
                return copy(param.value());
            }
        }
        Ok(String::new())
    }

    /// The rows as they are now: plugins come and go while a screen is open.
    fn reread(&mut self, host: &Host<'_>) -> Result<(), Error> {
        // The top screen of each kind is read anew; the others when shown.
        for token in [LIST_TOKEN, SCREENS_TOKEN] {
            let slot = match self.screens.top(token) {
                Some(View::Places(view)) => Some(copy(&view.slot)?),
                Some(View::List(_)) => None,
                None => continue,
            };
            let fresh = match slot {
                Some(slot) => View::Places(self.read_places(host, slot)?),
                None => View::List(self.read_list(host)?),
            };
            let Some(view) = self.screens.top(token) else {
                continue;
            };
            match (view, fresh) {
                (View::List(old), View::List(new)) => {
                    let focus = old.list.focus();
                    *old = new;
                    old.list.set_focus(focus);
                }
                (View::Places(old), View::Places(new)) => {
                    let focus = old.list.focus();
                    *old = new;
                    old.list.set_focus(focus);
                }
                _ => {}
            }
        }
        redraw(host)
    }

    /// Every plugin the host knows: the host first, then by name.
    fn read_plugins(&self, host: &Host<'_>) -> Result<Vec<PluginRow>, Error> {
        let registry = self.registry()?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        'again: for _ in 0..2 {
            let mut rows = Vec::new();
            let mut cursor = PageCursor::start();
            for _ in 0..MOST_PAGES {
                let page = match registry.plugins(host, cursor, &mut input, &mut out) {
                    Ok(page) => page,
                    // The list changed under the reading: start over.
                    Err(Error::State) => continue 'again,
                    Err(error) => return Err(error),
                };
                for plugin in page.rows() {
                    push(
                        &mut rows,
                        PluginRow {
                            id: copy(plugin.id())?,
                            name: copy(plugin.name())?,
                            version: copy(plugin.version())?,
                            kind: match plugin.kind() {
                                paperpaw_sdk::native::registry::RegistryPluginKind::Code => {
                                    Kind::Code
                                }
                                paperpaw_sdk::native::registry::RegistryPluginKind::Data => {
                                    Kind::Data
                                }
                                _ => Kind::BuiltIn,
                            },
                            run: match plugin.state() {
                                paperpaw_sdk::native::registry::RegistryPluginState::Off => {
                                    Run::Off
                                }
                                paperpaw_sdk::native::registry::RegistryPluginState::Failed => {
                                    Run::Failed
                                }
                                _ => Run::Running,
                            },
                            flags: plugin.flags(),
                            problem: Problem::from_wire(plugin.problem()),
                            problem_arg: plugin.problem_arg(),
                            update: copy(plugin.update())?,
                        },
                    )?;
                }
                match page.end().next_cursor() {
                    Some(next) => cursor = next,
                    None => {
                        native_model::sort(&mut rows);
                        return Ok(rows);
                    }
                }
            }
            return Err(Error::NoSpace);
        }
        Err(Error::State)
    }

    /// The description of a plugin, cut to what a note shows.
    fn description(&self, host: &Host<'_>, id: &str) -> Result<String, Error> {
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        match self.registry()?.plugin(host, id, &mut input, &mut out) {
            Ok(detail) => cut(detail.description().unwrap_or("")),
            Err(Error::NotFound) => Ok(String::new()),
            Err(error) => Err(error),
        }
    }

    fn read_list(&self, host: &Host<'_>) -> Result<List, Error> {
        let plugins = self.read_plugins(host)?;
        let mut say = Speaker::new(host)?;
        let mut items = Vec::new();
        let mut targets = Vec::new();
        // The places to get more, above the plugins.
        for (action, title) in self.more(host)? {
            let owner = self.owner_description(host, &action)?;
            push(&mut items, Item::noted(&title, "›", &owner)?)?;
            push(&mut targets, Target::More(action))?;
        }
        for (index, row) in plugins.iter().enumerate() {
            let description = self.description(host, &row.id)?;
            let note = native_model::note(&mut say, row, &description)?;
            let state = say.say(row.state_key())?;
            push(&mut items, Item::noted(&row.name, &state, &note)?)?;
            push(&mut targets, Target::Plugin(index))?;
        }
        // BLOCKED (#228): no *Restart now* row until `restart` has an SDK
        // sender (the typed DEV commands, #342); the footer says it applies
        // after a restart (`restart-now-sender` in `tests/acceptance_pending.rs`).
        let restart = plugins.iter().any(PluginRow::restart_needed);
        Ok(List {
            plugins,
            targets,
            list: ItemList::new(items),
            restart,
        })
    }

    /// The actions of the menu of places to get more: the action and its title.
    fn more(&self, host: &Host<'_>) -> Result<Vec<(String, String)>, Error> {
        let registry = self.registry()?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let mut found = Vec::new();
        let mut cursor = PageCursor::start();
        for _ in 0..MOST_PAGES {
            let page =
                match registry.menu(host, MORE, MenuQuery::Current, cursor, &mut input, &mut out) {
                    Ok(page) => page,
                    Err(Error::NotFound) => return Ok(found),
                    Err(error) => return Err(error),
                };
            for row in page.rows() {
                // Actions only: a section is a heading, a setting is not a place.
                if row.kind() == paperpaw_sdk::native::registry::RegistryMenuKind::Action {
                    push(&mut found, (copy(row.item())?, copy(row.title())?))?;
                }
            }
            match page.end().next_cursor() {
                Some(next) => cursor = next,
                None => break,
            }
        }
        Ok(found)
    }

    /// What the plugin that added a place says of it: its description.
    fn owner_description(&self, host: &Host<'_>, action: &str) -> Result<String, Error> {
        let owner = action.split_once('.').map_or(action, |(plugin, _)| plugin);
        self.description(host, owner)
    }

    fn chosen(&mut self, host: &Host<'_>, token: u32, index: usize) -> Result<(), Error> {
        self.ensure(host)?;
        let mut scratch = [0; 512];
        match self.screens.top(token) {
            Some(View::List(view)) => match view.targets.get(index) {
                Some(Target::More(action)) => {
                    CommandSender::new(host, &mut scratch).run_action(action)
                }
                Some(Target::Plugin(at)) => {
                    let Some(row) = view.plugins.get(*at) else {
                        return Ok(());
                    };
                    // The description below says why it must stay on.
                    if row.enabled() && row.locked() {
                        return Ok(());
                    }
                    let enabled = if row.enabled() {
                        PluginEnabled::Off
                    } else {
                        PluginEnabled::On
                    };
                    // Nothing else after the command: an error here would drop
                    // the staged switch. The states follow `PluginsChanged`
                    // (and `ScreenShown`), which read the rows again.
                    CommandSender::new(host, &mut scratch).set_plugin_enabled(&row.id, enabled)
                }
                _ => Ok(()),
            },
            Some(View::Places(view)) => match view.picks.get(index) {
                Some(Pick::Slot(place)) => {
                    let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
                    let handover = self.ui.as_ref().ok_or(Error::State)?.handover(
                        host,
                        &[Param::new(SLOT, place)?],
                        &mut input,
                        &mut out,
                    )?;
                    CommandSender::new(host, &mut scratch)
                        .open(Open::screen("plugin-manager.screens")?.handover(handover))
                }
                Some(Pick::Automatic) => {
                    let mut sender = CommandSender::new(host, &mut scratch);
                    sender.bind_slot(&view.slot, None)?;
                    sender.back()
                }
                Some(Pick::Candidate(screen)) => {
                    let mut sender = CommandSender::new(host, &mut scratch);
                    sender.bind_slot(&view.slot, Some(screen))?;
                    sender.back()
                }
                None => Ok(()),
            },
            None => Ok(()),
        }
    }

    /// Holding a row: how the plugin works.
    fn held(&mut self, host: &Host<'_>, token: u32, _index: usize) -> Result<(), Error> {
        if !matches!(self.screens.top(token), Some(View::List(_))) {
            return Ok(());
        }
        // BLOCKED (#228): the registry gives no "how it works" text of a
        // plugin (`plugin-how-text-read` in `tests/acceptance_pending.rs`).
        // Said on screen, never faked.
        let text = Messages::new()?.copy(host, "plugin-manager.how-unavailable")?;
        let mut scratch = [0; 256];
        CommandSender::new(host, &mut scratch).notify(Notify::new(&text)?)
    }

    /// What happened at the start to the plugins with code: a notice each,
    /// the more important, safe mode's, last.
    fn notices(&self, host: &Host<'_>) -> Result<(), Error> {
        let device = self.device.as_ref().ok_or(Error::State)?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let mut say = Speaker::new(host)?;
        // A plugin's name is asked of the registry where a notice names it: the
        // whole list (about seventy rows) is not read, and nothing of the model
        // is held at a start. The list is read when its screen opens.
        let name = |id: &str| self.plugin_name(host, id);
        let mut scratch = [0; 512];
        let mut skipped = Vec::new();
        let mut cursor = DeviceCursor::start();
        for _ in 0..MOST_PAGES {
            let page = match device.not_installed(host, cursor, &mut input, &mut out) {
                Ok(page) => page,
                Err(Error::NotFound) => break,
                Err(error) => return Err(error),
            };
            for row in page.rows() {
                let reason = match row.reason() {
                    NotInstalledReason::OtherAuthor => NotInstalled::OtherAuthor,
                    _ => NotInstalled::Damaged,
                };
                push(&mut skipped, (copy(row.plugin())?, reason))?;
            }
            match page.end().next_cursor() {
                Some(next) => cursor = next,
                None => break,
            }
        }
        if let Some(text) = native_model::not_installed_notice(&mut say, &skipped, name)? {
            CommandSender::new(host, &mut scratch).notify(Notify::new(&text)?.announce())?;
        }
        let native = match device.native(host, &mut input, &mut out) {
            Ok(native) => native,
            Err(Error::NotFound) => return Ok(()),
            Err(error) => return Err(error),
        };
        let reason = match native.safe_mode() {
            NativeSafeMode::None => return Ok(()),
            NativeSafeMode::User => SafeMode::User,
            _ => SafeMode::Failures,
        };
        let text = native_model::safe_mode_notice(
            &mut say,
            reason,
            native.crashed(),
            native.last_loaded(),
            name,
        )?;
        CommandSender::new(host, &mut scratch).notify(Notify::new(&text)?.announce())
    }

    /// The places that have several screens, with their offers: asks once
    /// whether to use a screen newly offered (not where the user chose, nor
    /// where it is used already, nor for a screen bound lower than the one
    /// used, e.g. a fallback). The first start only notes what there is.
    fn offer(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let registry = self.registry()?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        // The places: id, the screen used, whether the user chose, the title.
        let mut slots: Vec<(String, String, bool, String)> = Vec::new();
        let mut cursor = PageCursor::start();
        for _ in 0..MOST_PAGES {
            let page = registry.slots(host, cursor, &mut input, &mut out)?;
            for slot in page.rows() {
                push(
                    &mut slots,
                    (
                        copy(slot.id())?,
                        copy(slot.screen())?,
                        slot.flags() & SLOT_BOUND != 0,
                        copy(slot.title())?,
                    ),
                )?;
            }
            match page.end().next_cursor() {
                Some(next) => cursor = next,
                None => break,
            }
        }
        // `now`: the lines of every place with several screens; `fresh`: the
        // first new one that may be offered.
        let mut now: Vec<String> = Vec::new();
        let mut candidates: Vec<(String, String, bool, String)> = Vec::new();
        for (slot, used, bound, title) in &slots {
            let offers = self.offers(host, slot, &mut input, &mut out)?;
            if native_model::choosable(&offers) < 2 {
                continue;
            }
            let used_at = offers.iter().position(|o| o.screen == *used);
            for (at, offer) in offers.iter().enumerate() {
                if offer.screen == PLACEHOLDER {
                    continue;
                }
                let line = native_model::line(slot, &offer.screen)?;
                let higher = matches!((used_at, at), (Some(used_at), at) if at < used_at);
                push(&mut now, line)?;
                push(
                    &mut candidates,
                    (
                        copy(slot)?,
                        copy(&offer.screen)?,
                        *bound || !higher,
                        copy(title)?,
                    ),
                )?;
            }
        }
        let joined = paperpaw_ui_kit::mem::join(&now, "\n")?;
        let known = files::read_all(host, Root::Data, KNOWN, 1 << 14)?
            .and_then(|bytes| String::from_utf8(bytes).ok());
        if known.as_deref() != Some(joined.as_str()) {
            files::replace(host, Root::Data, KNOWN, joined.as_bytes())?;
        }
        // The first start only notes what there is.
        let Some(known) = known else {
            return Ok(());
        };
        let new = now.iter().zip(&candidates).find(|(line, (_, _, skip, _))| {
            !*skip && !known.lines().any(|known| known == line.as_str())
        });
        let Some((_, (slot, screen, _, title))) = new else {
            return Ok(());
        };
        if self.asked.is_some() {
            return Ok(());
        }
        let name = self.screen_name(host, screen)?;
        let mut messages = Messages::new()?;
        let text = messages.format(
            host,
            "plugin-manager.screens.use",
            &[
                Arg::Text {
                    name: "name",
                    value: &name,
                },
                Arg::Text {
                    name: "slot",
                    value: title,
                },
            ],
        )?;
        let heading = messages.copy(host, "plugin-manager.screens")?;
        let yes = messages.copy(host, "plugin-manager.screens.use.yes")?;
        let no = messages.copy(host, "plugin-manager.screens.use.no")?;
        let labels = concat(&[&yes, "\n", &no])?;
        let question = Question::confirm(&heading, &text)?.confirm_labels(&labels)?;
        let request = self
            .ui
            .as_ref()
            .ok_or(Error::State)?
            .ask(host, question, &mut input, &mut out)?;
        self.asked = Some((request, copy(slot)?, copy(screen)?));
        Ok(())
    }

    /// The yes of the question of [`Self::offer`] binds the place to the screen.
    fn answered(
        &mut self,
        host: &Host<'_>,
        answer: paperpaw_sdk::native::events::Answered,
    ) -> Result<(), Error> {
        let Some((request, slot, screen)) = self.asked.take() else {
            return Ok(());
        };
        if request != answer.request() {
            self.asked = Some((request, slot, screen));
            return Ok(());
        }
        if answer.outcome() == AnswerOutcome::Answered && answer.choice() == 1 {
            let mut scratch = [0; 512];
            CommandSender::new(host, &mut scratch).bind_slot(&slot, Some(&screen))?;
        }
        Ok(())
    }

    /// The screens offered for a place, highest priority first.
    ///
    /// Reads with the caller's buffers: a start that holds its own pair
    /// would otherwise hold a second one beside them (X4 heap).
    fn offers(
        &self,
        host: &Host<'_>,
        slot: &str,
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<Vec<Offer>, Error> {
        let registry = self.registry()?;
        let mut offers = Vec::new();
        let mut cursor = PageCursor::start();
        for _ in 0..MOST_PAGES {
            let page = registry.slot_offers(host, slot, cursor, input, out)?;
            for item in page.rows() {
                push(
                    &mut offers,
                    Offer {
                        screen: copy(item.id())?,
                        plugin: copy(item.plugin())?,
                    },
                )?;
            }
            match page.end().next_cursor() {
                Some(next) => cursor = next,
                None => break,
            }
        }
        Ok(offers)
    }

    /// What a plugin is called: its name, else its id (a plugin the registry
    /// does not know, such as one that was not installed).
    fn plugin_name(&self, host: &Host<'_>, id: &str) -> Result<String, Error> {
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        match self.registry()?.plugin(host, id, &mut input, &mut out) {
            Ok(detail) => copy(detail.plugin().name()),
            Err(Error::NotFound) => copy(id),
            Err(error) => Err(error),
        }
    }

    /// What a candidate screen is called: its plugin's name, else its own id.
    fn screen_name(&self, host: &Host<'_>, screen: &str) -> Result<String, Error> {
        let owner = screen.split_once('.').map_or(screen, |(plugin, _)| plugin);
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        match self.registry()?.plugin(host, owner, &mut input, &mut out) {
            Ok(detail) => copy(detail.plugin().name()),
            Err(Error::NotFound) => copy(screen),
            Err(error) => Err(error),
        }
    }

    /// The places with several screens, or the candidates of one `slot`.
    fn read_places(&self, host: &Host<'_>, slot: String) -> Result<Places, Error> {
        let registry = self.registry()?;
        let mut messages = Messages::new()?;
        let mut picks = Vec::new();
        let mut items = Vec::new();
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let automatic = messages.copy(host, "plugin-manager.screens.automatic")?;
        let mut title = String::new();
        let mut cursor = PageCursor::start();
        for _ in 0..MOST_PAGES {
            let page = registry.slots(host, cursor, &mut input, &mut out)?;
            let mut rows = Vec::new();
            for found in page.rows() {
                push(
                    &mut rows,
                    (
                        copy(found.id())?,
                        copy(found.screen())?,
                        found.flags() & SLOT_BOUND != 0,
                        copy(found.title())?,
                    ),
                )?;
            }
            let next = page.end().next_cursor();
            for (id, used, bound, name) in rows {
                let offers = self.offers(host, &id, &mut input, &mut out)?;
                if slot.is_empty() {
                    if native_model::choosable(&offers) < 2 {
                        continue;
                    }
                    let value = if bound {
                        concat(&[&self.screen_name(host, &used)?, " ›"])?
                    } else {
                        concat(&[&automatic, " ›"])?
                    };
                    push(&mut items, Item::row(&name, &value)?)?;
                    push(&mut picks, Pick::Slot(id))?;
                } else if id == slot {
                    title = copy(&name)?;
                    let mark = messages.copy(host, "plugin-manager.screens.chosen")?;
                    let help = messages.copy(host, "plugin-manager.screens.automatic.help")?;
                    let value = if bound { "" } else { mark.as_str() };
                    push(&mut items, Item::noted(&automatic, value, &help)?)?;
                    push(&mut picks, Pick::Automatic)?;
                    for offer in &offers {
                        if offer.screen == PLACEHOLDER {
                            continue;
                        }
                        let value = if bound && offer.screen == used {
                            mark.as_str()
                        } else {
                            ""
                        };
                        let called = self.screen_name(host, &offer.screen)?;
                        push(&mut items, Item::noted(&called, value, &offer.screen)?)?;
                        push(&mut picks, Pick::Candidate(copy(&offer.screen)?))?;
                    }
                }
            }
            match next {
                Some(next) => cursor = next,
                None => break,
            }
        }
        if slot.is_empty() {
            title = messages.copy(host, "plugin-manager.screens")?;
        }
        Ok(Places {
            slot,
            title,
            picks,
            list: ItemList::new(items),
        })
    }
}

/// The first [`MOST_DESCRIPTION`] bytes of `text`, cut at a letter's edge.
fn cut(text: &str) -> Result<String, Error> {
    let mut end = text.len().min(MOST_DESCRIPTION);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    copy(text.get(..end).unwrap_or(""))
}

fn redraw(host: &Host<'_>) -> Result<(), Error> {
    let mut scratch = [0; 64];
    CommandSender::new(host, &mut scratch)
        .queue_redraw(paperpaw_sdk::native::commands::RedrawMode::Partial)
}

/// The catalog's words for [`native_model`], in the reader's language.
struct Speaker<'h, 'a> {
    host: &'h Host<'a>,
    messages: Messages,
}

impl<'h, 'a> Speaker<'h, 'a> {
    fn new(host: &'h Host<'a>) -> Result<Self, Error> {
        Ok(Self {
            host,
            messages: Messages::new()?,
        })
    }
}

impl Say for Speaker<'_, '_> {
    fn say(&mut self, key: &str) -> Result<String, Error> {
        self.messages.copy(self.host, key)
    }

    fn say_with(&mut self, key: &str, name: &str, value: &str) -> Result<String, Error> {
        self.messages
            .format(self.host, key, &[Arg::Text { name, value }])
    }
}
