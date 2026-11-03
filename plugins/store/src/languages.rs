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

use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{actions, menus};
use paperpaw_sdk::event::Event;
use paperpaw_sdk::replies::HttpReply;
use paperpaw_sdk::setup::{SetupPage, SetupStepSpec};
use paperpaw_sdk::ui::{self, ListView, UiStyle};
use paperpaw_sdk::{
    Arg, Canvas, CloseCause, Color, Command, Cx, Env, FixedAction, FixedScreen, Gesture,
    GestureKind, Handled, HttpRequest, Id, ItemState, MenuEntry, Params, Rect, RefreshMode,
    RegisterError, Registrar, Screen, files,
};

use crate::catalog::{self, Entry, Index};
use crate::room::Room;
use crate::{CATALOG, queue, trust};

const SCREEN: Id = Id::new_static("store.languages");
const STEP: Id = Id::new_static("store.setup.languages");
const OPEN: Id = Id::new_static("store.languages.open");
/// The answer with the library's list, where the store has none yet.
const LOADED: Event<HttpReply> = Event::new("store.languages-loaded");

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
pub(crate) fn rows_of(listed: &[Pack], has: &dyn Fn(&str) -> bool) -> Vec<Row> {
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
            let done = packs.iter().all(|p| has(&p.id));
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
pub(crate) fn wanted<'a>(rows: &'a [Row], has: &dyn Fn(&str) -> bool) -> Vec<&'a Pack> {
    let mut packs: Vec<&Pack> = Vec::new();
    for pack in rows.iter().filter(|r| r.on).flat_map(|r| &r.packs) {
        if !has(&pack.id) && !packs.iter().any(|p| p.id == pack.id) {
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
    Files { need: u64, free: u64 },
    /// Code takes this much; this much is left, and a full flash gives
    /// more (the X4 after an SD install).
    Code { need: u64, left: u64, flash: bool },
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
                env.native().is_some() && env.caps().network && env.catalog_device().is_some()
            })),
    )?;
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
        let has = |id: &str| env.plugins().get(id).is_some();
        let mut rows = rows_of(&listed, &has);
        let mine = env.i18n().language().split('-').next().unwrap_or("");
        for row in rows.iter_mut().filter(|r| !r.done && r.tag == mine) {
            row.on = true;
        }
        // By name in the reader's language.
        rows.sort_by_cached_key(|r| env.i18n().tr(&format!("store.lang.{}", r.tag)).to_string());
        self.rows = rows;
        self.trouble = Trouble::None;
        self.room = Room::of(env, None);
    }

    fn has<'a>(env: &'a Env<'_>) -> impl Fn(&str) -> bool + 'a {
        |id| env.plugins().get(id).is_some()
    }

    fn chosen(&self, env: &Env<'_>) -> Vec<&Pack> {
        wanted(&self.rows, &Self::has(env))
    }

    fn fit(&self, env: &Env<'_>) -> Fit {
        fit(&self.room, &self.chosen(env))
    }

    /// *Download*: there when something is chosen and fits.
    fn button(&self, env: &Env<'_>) -> Option<Rect> {
        if self.chosen(env).is_empty() || self.fit(env) != Fit::Yes {
            return None;
        }
        let font = self.style.font(env)?;
        let label = env.i18n().tr("store.languages.download");
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
        if let Some(row) = self.rows.get_mut(index).filter(|r| !r.done) {
            row.on = !row.on;
            Self::redraw(cx);
        }
    }

    /// Hands the packs chosen to the queue, once, and goes back.
    fn download(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        if core::mem::replace(&mut self.started, true) {
            return cx.command(Command::Back);
        }
        let ids: Vec<String> = self.chosen(&env).iter().map(|p| p.id.clone()).collect();
        if !ids.is_empty() && self.fit(&env) == Fit::Yes {
            let _ = env
                .storage()
                .create_dir(&Env::plugin_dir(crate::PLUGIN.as_str()));
            let _ = env.storage().write(&chosen_path(), ids.join("\n").as_bytes());
            // This page said what they are.
            trust::tell(env.storage());
            let mut out = paperpaw_sdk::Commands::new();
            queue::add_quietly(&env, &ids, &mut out);
            out.into_iter().for_each(|command| cx.command(command));
            cx.command(Command::Notify(
                env.i18n().tr("store.languages.started").to_string(),
            ));
        }
        cx.command(Command::Back);
    }

    /// What a row says under its name.
    fn note(env: &Env<'_>, row: &Row) -> String {
        if row.done {
            return env.i18n().tr("store.offer.added").to_string();
        }
        let has = Self::has(env);
        let mut words: Vec<&str> = Vec::new();
        let mut size = 0;
        for pack in row.packs.iter().filter(|p| !has(&p.id)) {
            size += pack.size;
            words.push(env.i18n().tr(pack.kind()));
        }
        let shown = files::shown_size(env, size);
        format!("{} · {shown}", words.join(", "))
    }

    /// The line beside the button: what is chosen, or what is short.
    fn hint(&self, env: &Env<'_>) -> String {
        let chosen = self.chosen(env);
        let i18n = env.i18n();
        if chosen.is_empty() {
            return ui::tr_input(env, "store.languages.hint").to_string();
        }
        let need: u64 = chosen.iter().map(|p| p.size).sum();
        match self.fit(env) {
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
        if self.trouble == Trouble::Loading && !self.started {
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
        let mut ui = ui::Ui::new(canvas, env, &style);
        self.header.draw(
            &mut ui,
            env.i18n().tr("store.languages.title"),
            None,
            self.list.paging(len),
        );
        if self.rows.is_empty() {
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
                ui::Row::switch(name, row.on || row.done)
                    .note(&Self::note(env, row))
                    .focused(focused)
                    .draw(ui, rect);
            });
        }
        let line = Rect::new(0, self.bar.y, self.bar.width, style.line().max(1));
        ui.canvas().fill(line, Color::gray(150));
        if let Some(button) = button {
            let label = env.i18n().tr("store.languages.download");
            ui.button(button, label, true, self.on_button);
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
        }
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let env = *cx.env();
        let len = self.rows.len();
        let back = [actions::UP, actions::LEFT, actions::PREVIOUS].contains(action);
        let forward = [actions::DOWN, actions::RIGHT, actions::NEXT].contains(action);
        if self.rows.is_empty() {
            // The page of a trouble: *Select* tries again.
            if *action == actions::CONFIRM && self.trouble != Trouble::Loading {
                self.fetch(cx);
                return Handled::Yes;
            }
            return Handled::No;
        }
        if self.on_button {
            if *action == actions::CONFIRM {
                self.download(cx);
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
        if self.rows.is_empty() {
            if self.trouble != Trouble::Loading && self.area.contains(gesture.start) {
                self.fetch(cx);
                return Handled::Yes;
            }
            return Handled::No;
        }
        if self.button(&env).is_some_and(|b| b.contains(gesture.start)) {
            self.download(cx);
            return Handled::Yes;
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
        let rows = rows_of(&listed(), &|_| false);
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
        let mut rows = rows_of(&listed(), &|id| id == "hyphenation-uk");
        assert!(rows.iter().find(|r| r.tag == "ru").is_some_and(|r| !r.done));
        for row in rows.iter_mut().filter(|r| ["ru", "uk"].contains(&r.tag)) {
            row.on = true;
        }
        let ids: Vec<_> = wanted(&rows, &|id| id == "hyphenation-uk")
            .iter()
            .map(|p| p.id.clone())
            .collect();
        assert_eq!(ids, ["script-cyrillic", "hyphenation-ru"]);
        // Done where every pack is there.
        let done = rows_of(&listed(), &|id| id == "hyphenation-de");
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
