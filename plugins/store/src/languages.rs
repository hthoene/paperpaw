//! The first-run setup's languages (#116): a step after Wi-Fi that lists
//! the languages the library has packs for, each with what it brings
//! (the letters of its script, hyphenation, the menus) and what that
//! takes, and fetches those chosen in the background. The same page is
//! in the settings, for later.
//!
//! - A language is a few packs of the library, named by a rule: the
//!   letters of its script (the table below), `hyphenation-<tag>` and
//!   `language-<tag>`. Those the library does not list for this device
//!   are left out, and a language with none is not shown. Packs are put
//!   in the store's queue letters first, so what a pack requires (the
//!   Arabic menus need the Arabic letters, #97) comes before it.
//! - What is on the device is shown as installed. A pack that could not
//!   come is simply not installed: choosing the language again is the
//!   retry.
//! - The page tells before it fetches whether what is chosen fits: the
//!   files on the card, and the code packs (hyphenation) in the room for
//!   code. The install checks again, as it always does.
//! - The list comes from the store's own index. Without one (a first
//!   start) it is fetched here, and without a connection the page says
//!   so and the setup goes on.
//!
//! The memory it holds is the rows (at most the table's sixteen, with at
//! most three packs each: ids and sizes) and the choice; the index is
//! scanned once, a line at a time.

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{actions, events, menus, settings};
use paperpaw_sdk::event::Event;
use paperpaw_sdk::replies::{Confirmed, HttpReply};
use paperpaw_sdk::setup::{SetupPage, SetupStepSpec};
use paperpaw_sdk::ui::{self, ListView, UiStyle};
use paperpaw_sdk::{
    Arg, Canvas, CloseCause, Color, Command, Cx, Env, FixedAction, FixedScreen, Gesture,
    GestureKind, Handled, HttpRequest, Id, ItemState, MenuEntry, Params, PluginKind, Rect,
    RefreshMode, RegisterError, Registrar, Screen, ask, files,
};

use crate::catalog::{self, Entry, Index};
use crate::room::Room;
use crate::{CATALOG, INSTALLER, queue, trust};

const SCREEN: Id = Id::new_static("store.languages");
const STEP: Id = Id::new_static("store.setup.languages");
const OPEN: Id = Id::new_static("store.languages.open");
/// The answer with the library's list, where the store has none yet.
const LOADED: Event<HttpReply> = Event::new("store.languages-loaded");
/// The reader's answer to the question whether to show the menus in a
/// language that just came.
const SWITCH_ANSWERED: Event<Confirmed> = Event::new("store.languages-switch");
/// The Wi-Fi page of the wifi plugin, where the radio is PaperPaw's.
const WIFI_PAGE: Id = Id::new_static("wifi.page");

/// The languages the library may have packs for: the tag, and the pack
/// of the letters its script needs where the firmware's own do not
/// reach. Their names are `store.lang.<tag>`, in the reader's language
/// (the UI font has no letters for the scripts before their packs are
/// here, so a language is not named in its own).
const LANGUAGES: [(&str, Option<&str>); 16] = [
    ("ar", Some("script-arabic")),
    ("da", None),
    ("de", None),
    ("el", Some("script-greek")),
    ("es", None),
    ("fr", None),
    ("he", Some("script-hebrew")),
    ("it", None),
    ("ko", Some("script-korean")),
    ("nb", None),
    ("nl", None),
    ("pl", None),
    ("pt", None),
    ("ru", Some("script-cyrillic")),
    ("sv", None),
    ("uk", Some("script-cyrillic")),
];

/// Where the ids chosen are kept, for the setup's card.
fn chosen_path() -> String {
    format!(
        "{}/languages-chosen.txt",
        Env::plugin_dir(crate::PLUGIN.as_str())
    )
}

/// What the device has of a pack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Have {
    /// Not on the device: fetched.
    No,
    /// Built in and switched off (Android builds the store's file
    /// plugins in): switched on, never fetched.
    SwitchedOff,
    /// There, and on (or one the plugin manager looks after).
    Yes,
}

fn have(env: &Env<'_>, id: &str) -> Have {
    match env.plugins().get(id) {
        None => Have::No,
        Some(info) if info.enabled || info.kind != PluginKind::BuiltIn => Have::Yes,
        Some(_) => Have::SwitchedOff,
    }
}

/// A pack of a language as the library lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Pack {
    pub id: String,
    pub size: u64,
    /// Has code: runs from the room for code plugins.
    pub code: bool,
}

impl Pack {
    fn of(entry: &Entry) -> Self {
        Self {
            id: entry.id.clone(),
            size: entry.size,
            code: entry.code,
        }
    }

    /// What it brings, as a key for a word: the letters, the menus,
    /// hyphenation.
    fn kind(&self) -> &'static str {
        if self.id.starts_with("script-") {
            "store.languages.letters"
        } else if self.id.starts_with("language-") {
            "store.languages.menus"
        } else {
            "store.languages.hyphenation"
        }
    }

    /// The order packs are put in the queue: letters first.
    fn rank(&self) -> u8 {
        if self.id.starts_with("script-") {
            0
        } else if self.id.starts_with("hyphenation-") {
            1
        } else {
            2
        }
    }
}

/// A language of the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Row {
    pub tag: &'static str,
    /// Its packs the library lists, letters first.
    pub packs: Vec<Pack>,
    /// Every one is on the device.
    pub done: bool,
    pub on: bool,
}

/// The packs of `listed` that make up each language of the table, those
/// on the device (`has`) marked: a language none of whose packs is
/// listed is left out.
pub(crate) fn rows_of(listed: &[Pack], has: &dyn Fn(&str) -> Have) -> Vec<Row> {
    LANGUAGES
        .iter()
        .filter_map(|&(tag, script)| {
            let ids = [
                script.map(ToString::to_string),
                Some(format!("hyphenation-{tag}")),
                Some(format!("language-{tag}")),
            ];
            let packs: Vec<Pack> = ids
                .into_iter()
                .flatten()
                .filter_map(|id| listed.iter().find(|p| p.id == id).cloned())
                .collect();
            if packs.is_empty() {
                return None;
            }
            let done = packs.iter().all(|p| has(&p.id) == Have::Yes);
            Some(Row {
                tag,
                packs,
                done,
                on: false,
            })
        })
        .collect()
}

/// The packs to fetch for the languages chosen in `rows`: each once
/// (two languages share their letters), not those on the device, letters
/// first.
pub(crate) fn wanted<'a>(rows: &'a [Row], has: &dyn Fn(&str) -> Have) -> Vec<&'a Pack> {
    let mut packs: Vec<&Pack> = Vec::new();
    for pack in rows.iter().filter(|r| r.on).flat_map(|r| &r.packs) {
        if has(&pack.id) != Have::Yes && !packs.iter().any(|p| p.id == pack.id) {
            packs.push(pack);
        }
    }
    packs.sort_by_key(|p| p.rank());
    packs
}

/// Whether `packs` fit in `room`, and if not, what is short: the files
/// on the storage, or the room for code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Fit {
    Yes,
    /// Files take this much; this much is free.
    Files {
        need: u64,
        free: u64,
    },
    /// Code takes this much; this much is left, and a full flash gives
    /// more (the X4 after an SD install).
    Code {
        need: u64,
        left: u64,
        flash: bool,
    },
}

pub(crate) fn fit(room: &Room, packs: &[&Pack]) -> Fit {
    let need: u64 = packs.iter().map(|p| p.size).sum();
    if let Some(free) = room.free
        && need > free
    {
        return Fit::Files { need, free };
    }
    if let Some(code) = &room.code {
        let (mut taken, mut count, mut need) = (room.taken, room.plugins, 0u64);
        for pack in packs.iter().filter(|p| p.code) {
            // The file is a little more than the image the room places;
            // the install checks it again, with its header.
            let place = code.needs(pack.size, 0);
            if !code.fits(taken, count, place) {
                return Fit::Code {
                    need: need + place,
                    left: code.left(room.taken),
                    flash: crate::room::stock_flash(code),
                };
            }
            taken += place;
            need += place;
            count += 1;
        }
    }
    Fit::Yes
}

pub(crate) fn register(r: &mut Registrar) -> Result<(), RegisterError> {
    r.add_screens(&SCREENS)?;
    r.add_actions(&ACTIONS)?;
    r.add_menu_entries(&ENTRIES)?;
    r.add_icons(&ICONS)?;
    r.add_setup_step(
        SetupStepSpec::new(STEP, "store.languages.title", SetupPage::Screen(SCREEN))
            .help("store.languages.help")
            .order(52)
            // Where PaperPaw loads packs from the library and can reach
            // the network.
            .applies(Box::new(|env| {
                env.caps().network && env.catalog_device().is_some()
            })),
    )?;
    // A menu pack that came: the menus are not changed without asking.
    r.on(events::PLUGINS_CHANGED, |env, (), out| {
        offer_switch(env, out)
    })?;
    r.on(SWITCH_ANSWERED, |_, answered, out| switched(&answered, out))?;
    r.set_item_state(STEP, |env| match badge(env) {
        Some(badge) => ItemState::new().badge(badge),
        None => ItemState::new(),
    })
}

static SCREENS: [FixedScreen; 1] = [FixedScreen::new(
    SCREEN,
    "store.languages.title",
    |env, _| Box::new(LanguagesScreen::new(env)),
)];
static ACTIONS: [FixedAction; 1] = [FixedAction::new(OPEN, "store.languages.title")
    .handler(|_, _, out| out.push(Command::OpenScreen(SCREEN, Params::new())))];
// In the device's part of the settings, before *Set up PaperPaw*.
static ENTRIES: [MenuEntry; 1] = [MenuEntry::new(menus::SETTINGS, OPEN).order(95)];
static ICONS: [(Id, &str); 1] = [(OPEN, "typography")];

/// What the setup's card says of the languages chosen: how many packs
/// wait, or that they came; nothing before any was chosen.
fn badge(env: &Env<'_>) -> Option<String> {
    let text = paperpaw_sdk::read_file(env.storage(), &chosen_path(), 1024)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())?;
    let waiting = queue::waiting(env)
        .iter()
        .filter(|id| text.lines().any(|l| l == id.as_str()))
        .count();
    if waiting > 0 {
        return Some(
            env.i18n()
                .plural("store.offer.being-added", waiting as i64, &[]),
        );
    }
    Some(env.i18n().tr("store.offer.added").to_string())
}

/// Why there is no list to show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Trouble {
    None,
    Loading,
    Offline,
    Failed,
}

/// Where a language's packs are in the store's queue.
enum Turn {
    /// Being downloaded: its files done, and in all.
    Downloading { done: usize, total: usize },
    /// Waits for its turn.
    Waiting,
}

/// Whether a pack of `row` is being fetched or waits for it.
fn turn(env: &Env<'_>, row: &Row) -> Option<Turn> {
    let current = INSTALLER.progress(env.storage());
    let waiting = queue::waiting(env);
    row.packs
        .iter()
        .filter(|p| have(env, &p.id) != Have::Yes)
        .find_map(|pack| {
            if let Some(p) = current.as_ref().filter(|p| p.plugin == pack.id) {
                Some(Turn::Downloading {
                    done: p.done,
                    total: p.total,
                })
            } else if waiting.contains(&pack.id) {
                Some(Turn::Waiting)
            } else {
                None
            }
        })
}

/// The ids chosen with *Download*, as kept.
fn chosen_ids(env: &Env<'_>) -> Vec<String> {
    paperpaw_sdk::read_file(env.storage(), &chosen_path(), 1024)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .map(|text| text.lines().map(ToString::to_string).collect())
        .unwrap_or_default()
}

fn asked_path() -> String {
    format!(
        "{}/languages-asked.txt",
        Env::plugin_dir(crate::PLUGIN.as_str())
    )
}

/// A menu pack chosen with *Download* came, and every pack chosen with it
/// is here or has failed: asks whether to show the menus in it (once for
/// each language, and not for the one in use). The language is not
/// changed without the answer.
fn offer_switch(env: &Env<'_>, out: &mut paperpaw_sdk::Commands) {
    let ids = chosen_ids(env);
    if ids.is_empty() {
        return;
    }
    let waiting = queue::waiting(env);
    // The packs of the letters come first: what still waits is not here.
    if ids
        .iter()
        .any(|id| waiting.contains(id) && env.plugins().get(id).is_none())
    {
        return;
    }
    let i18n = env.i18n();
    let languages = i18n.languages();
    let current = i18n.language().split('-').next().unwrap_or("");
    let asked = paperpaw_sdk::read_file(env.storage(), &asked_path(), 256)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .unwrap_or_default();
    let Some(tag) = ids
        .iter()
        .filter_map(|id| id.strip_prefix("language-"))
        .find(|tag| {
            *tag != current
                && env.plugins().get(&format!("language-{tag}")).is_some()
                && languages
                    .iter()
                    .any(|(known, _)| known.split('-').next() == Some(tag))
                && !asked.lines().any(|line| line == *tag)
        })
    else {
        return;
    };
    let _ = env
        .storage()
        .create_dir(&Env::plugin_dir(crate::PLUGIN.as_str()));
    let _ = env
        .storage()
        .append(&asked_path(), format!("{tag}\n").as_bytes());
    let key = format!("store.lang.{tag}");
    let name = i18n.tr(&key);
    out.push(
        ask::confirm(
            i18n.format("store.languages.switch", &[("language", Arg::Str(name))]),
            &SWITCH_ANSWERED,
        )
        .with("tag", tag.to_string())
        .yes(i18n.tr("store.languages.switch.yes"))
        .no(i18n.tr("store.request.not-now"))
        .focus_yes()
        .command(),
    );
}

/// The reader's word on the menus: changed only on *yes*.
fn switched(answered: &Confirmed, out: &mut paperpaw_sdk::Commands) {
    if !answered.yes {
        return;
    }
    if let Some(tag) = answered.context.get("tag") {
        out.push(Command::set(
            &settings::LANGUAGE,
            Cow::Owned(tag.to_string()),
        ));
    }
}

struct LanguagesScreen {
    rows: Vec<Row>,
    trouble: Trouble,
    room: Room,
    style: UiStyle,
    header: ui::Header,
    list: ListView,
    bar: Rect,
    area: Rect,
    on_button: bool,
    /// What was chosen was handed to the queue (*Download*, or the page
    /// left): not again.
    started: bool,
    /// The offer to go online, in the Offline power mode.
    offer: ui::offline::OfflineOffer,
}

impl LanguagesScreen {
    fn new(env: &Env<'_>) -> Self {
        let style = UiStyle::new(env);
        let display = env.device().display;
        let header = ui::Header::new(env, &style, true);
        let top = header.area().height + style.gap();
        let bar_height = ui::edge_bar_height(&style);
        let bar = Rect::new(
            0,
            display.height.saturating_sub(bar_height),
            display.width,
            bar_height,
        );
        let area = Rect::new(
            0,
            top,
            display.width,
            display.height.saturating_sub(top + bar_height),
        );
        let mut screen = Self {
            rows: Vec::new(),
            trouble: Trouble::None,
            room: Room::of(env, None),
            style,
            header,
            list: ListView::new(area, style.noted_row()).focus_on_keys(env),
            bar,
            area,
            on_button: false,
            started: false,
            offer: ui::offline::OfflineOffer::new(),
        };
        screen.read(env);
        screen
    }

    /// The rows from the index on the storage, if there is one; the
    /// language of the UI is chosen to begin with.
    fn read(&mut self, env: &Env<'_>) {
        let Some(index) = Index::kept(env.storage()) else {
            self.trouble = if env.caps().network {
                Trouble::Loading
            } else {
                Trouble::Offline
            };
            return;
        };
        let mut listed: Vec<Pack> = Vec::new();
        index.scan(env.storage(), &mut |_, entry: Entry| {
            if entry.id.starts_with("script-")
                || entry.id.starts_with("hyphenation-")
                || entry.id.starts_with("language-")
            {
                listed.push(Pack::of(&entry));
            }
        });
        let mut rows = rows_of(&listed, &|id| have(env, id));
        let mine = env.i18n().language().split('-').next().unwrap_or("");
        for row in rows.iter_mut().filter(|r| !r.done && r.tag == mine) {
            row.on = turn(env, row).is_none();
        }
        // By name in the reader's language.
        rows.sort_by_cached_key(|r| env.i18n().tr(&format!("store.lang.{}", r.tag)).to_string());
        self.rows = rows;
        self.trouble = Trouble::None;
        self.room = Room::of(env, None);
    }

    /// Packs chosen and not on the device, switched on or fetched.
    fn chosen(&self, env: &Env<'_>) -> Vec<&Pack> {
        wanted(&self.rows, &|id| have(env, id))
    }

    /// Of those chosen, the ones to fetch (the others are built in).
    fn to_fetch(&self, env: &Env<'_>) -> Vec<&Pack> {
        let mut packs = self.chosen(env);
        packs.retain(|p| have(env, &p.id) == Have::No);
        packs
    }

    fn fit(&self, env: &Env<'_>) -> Fit {
        fit(&self.room, &self.to_fetch(env))
    }

    /// The offer to go online shows: no list, Offline power mode.
    fn offering(&self, env: &Env<'_>) -> bool {
        self.rows.is_empty() && self.trouble == Trouble::Offline && self.offer.shows(env)
    }

    /// The Wi-Fi page is offered: no list for want of a connection, and
    /// PaperPaw runs the radio.
    fn wifi_button(&self, env: &Env<'_>) -> bool {
        self.rows.is_empty()
            && self.trouble == Trouble::Offline
            && env.wifi().is_some()
            && !self.offering(env)
    }

    /// The word of the button of the bar, if there is one: *Download*
    /// (or *Add* where all is built in) when something is chosen and fits,
    /// *Wi-Fi* where the page waits for a network.
    fn label(&self, env: &Env<'_>) -> Option<&'static str> {
        if self.wifi_button(env) {
            return Some("store.languages.join-wifi");
        }
        if self.chosen(env).is_empty() || self.fit(env) != Fit::Yes {
            return None;
        }
        Some(if self.to_fetch(env).is_empty() {
            "store.languages.add"
        } else {
            "store.languages.download"
        })
    }

    fn button(&self, env: &Env<'_>) -> Option<Rect> {
        let label = env.i18n().tr(self.label(env)?);
        let font = self.style.font(env)?;
        let pad = self.style.margin();
        let width = font.measure(self.style.text(), label) as u16 + 2 * pad;
        let band = ui::edge_bar_band(&self.style, self.bar, false);
        Some(Rect::new(
            self.bar.width.saturating_sub(width + pad),
            band.y,
            width,
            band.height,
        ))
    }

    fn redraw(cx: &mut Cx<'_, '_>) {
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Fast);
    }

    fn fetch(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let Some(device) = env.catalog_device() else {
            return;
        };
        let list = env.settings().read(&CATALOG);
        let url = catalog::list_url(
            list.trim(),
            device,
            env.i18n().language(),
            crate::community(&env),
        );
        let request = HttpRequest::new("GET", &url, LOADED)
            .save_to(catalog::list_part())
            .max_file(catalog::MAX_LIST_FILE);
        cx.command(Command::http(request));
        self.trouble = Trouble::Loading;
        Self::redraw(cx);
    }

    fn loaded(&mut self, reply: &HttpReply, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let fetched = catalog::list_part();
        let ok = (200..300).contains(&reply.status)
            && catalog::Device::of(&env)
                .and_then(|device| catalog::take(env.storage(), &fetched, &device))
                .is_some();
        // Another screen of the store may have taken the same answer.
        if ok || (reply.status / 100 == 2 && !env.storage().exists(&fetched)) {
            self.read(&env);
        } else {
            let _ = env.storage().remove(&fetched);
            self.trouble = if reply.status == 0 {
                Trouble::Offline
            } else {
                Trouble::Failed
            };
        }
        Self::redraw(cx);
    }

    fn toggle(&mut self, index: usize, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        if let Some(row) = self
            .rows
            .get_mut(index)
            .filter(|r| !r.done && turn(&env, r).is_none())
        {
            row.on = !row.on;
            Self::redraw(cx);
        }
    }

    /// What the button does: the Wi-Fi page, or the packs chosen.
    fn press(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        if self.wifi_button(&env) {
            return cx.command(Command::OpenScreen(WIFI_PAGE, Params::new()));
        }
        self.download(cx);
    }

    /// Hands the packs chosen to the queue (and switches on those built
    /// in), once, and goes back.
    fn download(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        if core::mem::replace(&mut self.started, true) {
            return cx.command(Command::Back);
        }
        let fetch: Vec<String> = self.to_fetch(&env).iter().map(|p| p.id.clone()).collect();
        let switch: Vec<String> = self
            .chosen(&env)
            .iter()
            .filter(|p| have(&env, &p.id) == Have::SwitchedOff)
            .map(|p| p.id.clone())
            .collect();
        if (!fetch.is_empty() || !switch.is_empty()) && self.fit(&env) == Fit::Yes {
            let _ = env
                .storage()
                .create_dir(&Env::plugin_dir(crate::PLUGIN.as_str()));
            let all: Vec<&str> = fetch.iter().chain(&switch).map(String::as_str).collect();
            let _ = env
                .storage()
                .write(&chosen_path(), all.join("\n").as_bytes());
            for id in &switch {
                if let Ok(id) = Id::parse(id) {
                    cx.command(Command::SetPluginEnabled(id, true));
                }
            }
            if !fetch.is_empty() {
                // This page said what they are.
                trust::tell(env.storage());
                let mut out = paperpaw_sdk::Commands::new();
                queue::add_quietly(&env, &fetch, &mut out);
                out.into_iter().for_each(|command| cx.command(command));
            }
            let key = if fetch.is_empty() {
                "store.languages.switched-on"
            } else {
                "store.languages.started"
            };
            cx.command(Command::Notify(env.i18n().tr(key).to_string()));
        }
        cx.command(Command::Back);
    }

    /// What a row says under its name.
    fn note(env: &Env<'_>, row: &Row) -> String {
        let i18n = env.i18n();
        if row.done {
            return i18n.tr("store.offer.added").to_string();
        }
        match turn(env, row) {
            Some(Turn::Downloading { done, total }) => {
                return i18n.format(
                    "store.languages.downloading",
                    &[
                        ("done", Arg::Str(&done.to_string())),
                        ("total", Arg::Str(&total.to_string())),
                    ],
                );
            }
            Some(Turn::Waiting) => return i18n.tr("store.languages.waiting").to_string(),
            None => {}
        }
        let mut words: Vec<&str> = Vec::new();
        let (mut size, mut fetched) = (0, false);
        for pack in row.packs.iter().filter(|p| have(env, &p.id) != Have::Yes) {
            if have(env, &pack.id) == Have::No {
                size += pack.size;
                fetched = true;
            }
            words.push(i18n.tr(pack.kind()));
        }
        let size = if fetched {
            files::shown_size(env, size)
        } else {
            i18n.tr("store.languages.built-in").to_string()
        };
        format!("{} · {size}", words.join(", "))
    }

    /// The line beside the button: what is chosen, what is short, or how
    /// far the downloads are.
    fn hint(&self, env: &Env<'_>) -> String {
        let chosen = self.chosen(env);
        let i18n = env.i18n();
        if chosen.is_empty() {
            let left = self.rows.iter().filter(|r| turn(env, r).is_some()).count();
            if left > 0 {
                return i18n.plural("store.languages.left", left as i64, &[]);
            }
            return ui::tr_input(env, "store.languages.hint").to_string();
        }
        let need: u64 = self.to_fetch(env).iter().map(|p| p.size).sum();
        match self.fit(env) {
            Fit::Yes if need == 0 => i18n.tr("store.languages.no-download").to_string(),
            Fit::Yes => i18n.format(
                "store.languages.total",
                &[("size", Arg::Str(&files::shown_size(env, need)))],
            ),
            Fit::Files { need, free } => i18n.format(
                "store.no-room.files",
                &[
                    ("size", Arg::Str(&files::shown_size(env, need))),
                    ("free", Arg::Str(&files::shown_size(env, free))),
                ],
            ),
            Fit::Code { need, left, .. } => i18n.format(
                "store.languages.no-code-room",
                &[
                    ("size", Arg::Str(&files::shown_size(env, need))),
                    ("left", Arg::Str(&files::shown_size(env, left))),
                ],
            ),
        }
    }
}

impl Screen for LanguagesScreen {
    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        if self.started {
            return;
        }
        // Back from the Wi-Fi page with a network: the list is fetched.
        let online = env.wifi().is_some_and(|w| w.state.is_online());
        if self.trouble == Trouble::Loading || (self.trouble == Trouble::Offline && online) {
            self.fetch(cx);
        }
    }

    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let len = self.rows.len();
        self.list.set_len(len);
        let button = self.button(env);
        self.on_button &= button.is_some();
        self.list.set_active(!self.on_button);
        let offering = self.offering(env);
        let mut ui = ui::Ui::new(canvas, env, &style);
        self.header.draw(
            &mut ui,
            env.i18n().tr("store.languages.title"),
            None,
            self.list.paging(len),
        );
        if offering {
            self.offer.draw(&mut ui, self.area);
        } else if self.rows.is_empty() {
            let (icon, title, text) = match self.trouble {
                Trouble::Offline => (
                    "wifi-off",
                    "store.languages.offline",
                    "store.languages.offline.help",
                ),
                Trouble::Failed => ("cloud", "store.unreachable", "store.unreachable.help"),
                Trouble::Loading => ("cloud-download", "store.loading", ""),
                Trouble::None => ("cloud", "store.languages.none", ""),
            };
            let text = if text.is_empty() {
                ""
            } else {
                ui::tr_input(env, text)
            };
            ui.empty(self.area, icon, env.i18n().tr(title), text);
        } else {
            let rows = &self.rows;
            self.list.draw(&mut ui, len, |ui, index, rect, focused| {
                let row = &rows[index];
                let key = format!("store.lang.{}", row.tag);
                let name = env.i18n().tr(&key);
                let on = row.on || row.done || turn(env, row).is_some();
                ui::Row::switch(name, on)
                    .note(&Self::note(env, row))
                    .focused(focused)
                    .draw(ui, rect);
            });
        }
        let line = Rect::new(0, self.bar.y, self.bar.width, style.line().max(1));
        ui.canvas().fill(line, Color::gray(150));
        if let (Some(button), Some(label)) = (button, self.label(env)) {
            ui.button(button, env.i18n().tr(label), true, self.on_button);
        }
        let right = button.map_or(self.bar.width, |b| b.x);
        let area = Rect::new(
            style.margin(),
            self.bar.y,
            right.saturating_sub(style.gap() + style.margin()),
            self.bar.height,
        );
        let hint = if self.rows.is_empty() {
            String::new()
        } else {
            self.hint(env)
        };
        ui::Label::new(&hint)
            .px(style.small())
            .color(style.faint(ui::Ink::NOTE))
            .lines(2)
            .draw(&mut ui, area);
    }

    fn on_event(&mut self, event: &Id, params: &Params, cx: &mut Cx<'_, '_>) {
        if let Some(reply) = LOADED.read(event, params) {
            self.loaded(&reply, cx);
        } else if *event == events::PLUGINS_CHANGED {
            // A pack came: its row says so, and the rows are counted anew.
            let env = *cx.env();
            if !self.rows.is_empty() {
                let on: Vec<&'static str> =
                    self.rows.iter().filter(|r| r.on).map(|r| r.tag).collect();
                self.read(&env);
                for row in self.rows.iter_mut().filter(|r| on.contains(&r.tag)) {
                    row.on = !row.done;
                }
            }
            Self::redraw(cx);
        }
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let env = *cx.env();
        let len = self.rows.len();
        let back = [actions::UP, actions::LEFT, actions::PREVIOUS].contains(action);
        let forward = [actions::DOWN, actions::RIGHT, actions::NEXT].contains(action);
        if *action != actions::BACK && self.offering(&env) {
            if self.offer.on_action(action, cx).done() == Some(ui::offline::Offer::Online) {
                self.fetch(cx);
            }
            return Handled::Yes;
        }
        if self.rows.is_empty() {
            // The page of a trouble: *Select* opens the Wi-Fi page where
            // the radio is PaperPaw's, else tries again.
            if *action == actions::CONFIRM && self.trouble != Trouble::Loading {
                if self.wifi_button(&env) {
                    self.press(cx);
                } else {
                    self.fetch(cx);
                }
                return Handled::Yes;
            }
            return Handled::No;
        }
        if self.on_button {
            if *action == actions::CONFIRM {
                self.press(cx);
            } else if back {
                self.on_button = false;
                Self::redraw(cx);
            } else if !forward {
                return Handled::No;
            }
            return Handled::Yes;
        }
        if forward && self.list.focus() + 1 >= len && self.button(&env).is_some() {
            self.on_button = true;
            Self::redraw(cx);
            return Handled::Yes;
        }
        let delta = if back {
            -1
        } else if forward {
            1
        } else if *action == actions::CONFIRM {
            self.toggle(self.list.focus(), cx);
            return Handled::Yes;
        } else {
            return Handled::No;
        };
        self.list.move_focus(delta, len).request(cx);
        Handled::Yes
    }

    /// Leaving the page does not fetch: only *Download* does, as what is
    /// chosen takes megabytes.
    fn on_close_request(&mut self, _: CloseCause, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if gesture.kind != GestureKind::Tap {
            return Handled::No;
        }
        let env = *cx.env();
        let paging = self.list.paging(self.rows.len());
        if let Some(action) = self.header.tap(&env, &self.style, gesture.start, paging) {
            cx.command(Command::run(action));
            return Handled::Yes;
        }
        if self.offering(&env) {
            if self.offer.on_gesture(&gesture, cx).done() == Some(ui::offline::Offer::Online) {
                self.fetch(cx);
            }
            return Handled::Yes;
        }
        if self.button(&env).is_some_and(|b| b.contains(gesture.start)) {
            self.press(cx);
            return Handled::Yes;
        }
        if self.rows.is_empty() {
            if self.trouble != Trouble::Loading && self.area.contains(gesture.start) {
                self.fetch(cx);
                return Handled::Yes;
            }
            return Handled::No;
        }
        let Some(index) = self.list.hit(gesture.start, self.rows.len()) else {
            return Handled::No;
        };
        self.list.set_focus(index);
        self.on_button = false;
        self.toggle(index, cx);
        Handled::Yes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_sdk::native::CodeRoom;

    fn pack(id: &str, size: u64, code: bool) -> Pack {
        Pack {
            id: id.to_string(),
            size,
            code,
        }
    }

    fn have_only(id: &str, there: &str) -> Have {
        if id == there { Have::Yes } else { Have::No }
    }

    fn listed() -> Vec<Pack> {
        alloc::vec![
            pack("hyphenation-de", 370_500, true),
            pack("hyphenation-ru", 94_228, true),
            pack("hyphenation-uk", 66_964, true),
            pack("script-cyrillic", 1_283_379, false),
            pack("script-arabic", 900_000, false),
            pack("language-ar", 210_000, false),
            pack("script-greek", 3_670_464, false),
            pack("script-korean-extra", 30_000_000, false),
        ]
    }

    #[test]
    fn a_language_is_its_letters_hyphenation_and_menus_the_library_lists() {
        let rows = rows_of(&listed(), &|_| Have::No);
        let tags: Vec<_> = rows.iter().map(|r| r.tag).collect();
        // Not those with no pack listed (Korean: only its rare syllables).
        assert_eq!(tags, ["ar", "de", "el", "ru", "uk"]);
        let ids = |tag: &str| -> Vec<&str> {
            rows.iter()
                .find(|r| r.tag == tag)
                .unwrap()
                .packs
                .iter()
                .map(|p| p.id.as_str())
                .collect()
        };
        // Letters first, so that what the menus need is there before them.
        assert_eq!(ids("ar"), ["script-arabic", "language-ar"]);
        assert_eq!(ids("ru"), ["script-cyrillic", "hyphenation-ru"]);
        assert_eq!(ids("de"), ["hyphenation-de"]);
    }

    #[test]
    fn two_languages_share_their_letters_and_what_is_there_is_not_fetched() {
        let mut rows = rows_of(&listed(), &|id| have_only(id, "hyphenation-uk"));
        assert!(rows.iter().find(|r| r.tag == "ru").is_some_and(|r| !r.done));
        for row in rows.iter_mut().filter(|r| ["ru", "uk"].contains(&r.tag)) {
            row.on = true;
        }
        let ids: Vec<_> = wanted(&rows, &|id| have_only(id, "hyphenation-uk"))
            .iter()
            .map(|p| p.id.clone())
            .collect();
        assert_eq!(ids, ["script-cyrillic", "hyphenation-ru"]);
        // Done where every pack is there.
        let done = rows_of(&listed(), &|id| have_only(id, "hyphenation-de"));
        assert!(done.iter().find(|r| r.tag == "de").unwrap().done);
    }

    #[test]
    fn it_says_what_does_not_fit_before_fetching() {
        let big = [pack("script-greek", 3_670_464, false)];
        let refs: Vec<&Pack> = big.iter().collect();
        let mut room = Room {
            free: Some(10 << 20),
            code: None,
            taken: 0,
            plugins: 0,
            off: 0,
            off_plugins: 0,
        };
        assert_eq!(fit(&room, &refs), Fit::Yes);
        room.free = Some(1 << 20);
        assert_eq!(
            fit(&room, &refs),
            Fit::Files {
                need: 3_670_464,
                free: 1 << 20
            }
        );
        // Code packs: the room for code, one pack after another.
        let code = [
            pack("hyphenation-de", 370_500, true),
            pack("hyphenation-nb", 271_616, true),
        ];
        let refs: Vec<&Pack> = code.iter().collect();
        room.free = None;
        room.code = Some(CodeRoom::new(500 * 1024, 0, 4096));
        assert!(matches!(fit(&room, &refs), Fit::Code { left, .. } if left == 500 * 1024));
        room.code = Some(CodeRoom::new(900 * 1024, 0, 4096));
        assert_eq!(fit(&room, &refs), Fit::Yes);
    }
}
