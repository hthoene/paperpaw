//! Reading profiles: the settings for reading, the display and the status
//! lines saved under a name ("Night", "Commute", "Large print") and
//! applied again with a tap.
//!
//! - *Profiles* (in the reader's menu under *More*, and in Settings ›
//!   Customise) lists them; the one in use is marked. Tapping one applies
//!   it; holding it offers to save the current settings into it, rename
//!   it or remove it. *Save as new profile* asks for a name.
//! - *Next profile* applies the profile after the one applied last: bind
//!   it to a button or use it in a rule.
//! - Names are unique (whatever their case): a name in use is refused,
//!   so that every profile can be told apart and *Next profile* goes
//!   round them all.
//! - Where the profiles cannot be read, the list says so and saves
//!   nothing over them.

#![no_std]

extern crate alloc;

#[cfg(test)]
extern crate std;
/// Test-only: refuses chosen allocations to prove the fallible paths.
#[cfg(test)]
#[path = "../../test-support/fail_alloc.rs"]
mod fail_alloc;
#[cfg(test)]
#[global_allocator]
static ALLOC: fail_alloc::Failing = fail_alloc::Failing;

mod profile;

mod native_data;
mod native_entry;
/// The file of profiles and their values, without the host.
pub mod store;
#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;
/// The interface-7 plugin.
pub mod native_plugin;

/// Catalog sources published before native screen callbacks.
pub static CATALOGS: [(&str, &str); 2] = [
    ("en", include_str!("../i18n/en.ini")),
    ("de", include_str!("../i18n/de.ini")),
];

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::ask;
use paperpaw_sdk::builtin::{actions, menus, params};
use paperpaw_sdk::event::Event;
use paperpaw_sdk::replies::{Confirmed, Typed};
use paperpaw_sdk::ui::choice;
use paperpaw_sdk::ui::{self, ListView, UiStyle};
use paperpaw_sdk::{
    Arg, Canvas, Color, Command, Commands, Cx, Env, FixedAction, FixedScreen, Gesture, GestureKind,
    Handled, Id, Manifest, MenuEntry, Params, Plugin, Rect, RefreshMode, RegisterError, Registrar,
    Screen,
};

use crate::profile::Profile;

pub const PLUGIN: Id = Id::new_static("profiles");
/// Opens the list of profiles.
pub const OPEN: Id = Id::new_static("profiles.open");
/// Applies the profile after the one applied last.
pub const NEXT: Id = Id::new_static("profiles.next");
const LIST_SCREEN: Id = Id::new_static("profiles.list");
/// Replies: a name for a new profile, a new name, an option.
const NAMED: Event<Typed> = Event::new("profiles.named");
const RENAMED: Event<Typed> = Event::new("profiles.renamed");
const CHOSEN: Event<choice::Picked> = Event::new("profiles.chosen");
/// The answer to "replace?" or "remove?": the option, the profile's index
/// and name in its context.
const CONFIRMED: Event<Confirmed> = Event::new("profiles.confirmed");
/// Keys of [`CONFIRMED`]'s context.
const OPTION: &str = "option";
const NAME: &str = "name";

/// Applies `profile` and says so.
fn apply(env: &Env<'_>, profile: &Profile, out: &mut Commands) {
    out.extend(profile::apply(env, profile));
    profile::set_last(env.storage(), &profile.name);
    out.push(Command::Notify(env.i18n().format(
        "profiles.applied",
        &[("name", Arg::Str(&profile.name))],
    )));
}

/// The profiles plugin.
#[derive(Debug, Default)]
pub struct Profiles;

/// How it works, by language (see `Manifest::how`).
pub static HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/en.txt")),
    ("de", include_str!("../how/de.txt")),
];

static ACTIONS: [FixedAction; 2] = [
    FixedAction::new(OPEN, "profiles.title")
        .handler(|_, _, out| out.push(Command::OpenScreen(LIST_SCREEN, Params::new()))),
    FixedAction::new(NEXT, "profiles.next").handler(|env, _, out| {
        let profiles = profile::load(env.storage()).unwrap_or_default();
        let last = profile::last(env.storage());
        let at = profiles.iter().position(|p| p.name == last);
        let next = at.map_or(0, |at| (at + 1) % profiles.len().max(1));
        if let Some(profile) = profiles.get(next) {
            apply(env, profile, out);
        }
    }),
];
static ENTRIES: [MenuEntry; 2] = [
    MenuEntry::new(menus::READER_MORE, OPEN).order(15),
    MenuEntry::new(menus::SETTINGS, OPEN).order(205),
];
static ICONS: [(Id, &str); 1] = [(OPEN, "stack-2")];

static SCREENS: [FixedScreen; 1] = [FixedScreen::new(LIST_SCREEN, "profiles.title", |env, _| {
    Box::new(ListScreen::new(env))
})
.instances(paperpaw_sdk::Instances::SINGLE)];

impl Plugin for Profiles {
    fn manifest(&self) -> Manifest {
        Manifest::new(PLUGIN, "Reading profiles", paperpaw_sdk::package_version!())
            .description("Named sets of reading and display settings, switched with a tap.")
            .how(&HOW)
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_screens(&SCREENS)?;
        r.add_actions(&ACTIONS)?;
        r.add_menu_entries(&ENTRIES)?;
        r.add_icons(&ICONS)?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Row {
    Profile(usize),
    Add,
}

struct ListScreen {
    style: UiStyle,
    header: ui::Header,
    list: ListView,
    /// The line at the bottom.
    status: Rect,
    profiles: Vec<Profile>,
    /// Whether the profiles could be read: if not, nothing is saved over
    /// them.
    readable: bool,
    /// Whether the settings are as each profile has them, found when the
    /// profiles or the settings change.
    in_use: Vec<bool>,
}

/// The height of the row that adds a profile, `width` wide.
fn add_row_height(env: &Env<'_>, style: &UiStyle, width: u16) -> u32 {
    let title = env.i18n().tr("profiles.add");
    let note = env.i18n().tr("profiles.add.help");
    u32::from(
        ui::Row::value(title, "+")
            .note(note)
            .height(env, style, width),
    )
}

impl ListScreen {
    fn new(env: &Env<'_>) -> Self {
        let style = UiStyle::new(env);
        let header = ui::Header::new(env, &style, true);
        let display = env.device().display;
        let top = header.area().height + style.gap();
        // The note below as high as its words need, at least a row and a half.
        let hint = ui::tr_input(env, "profiles.hint");
        let help = ui::note_line_height(env, &style, display.width, hint);
        let line = (style.row() * 3 / 2).max(help);
        let status = Rect::new(0, display.height - line, display.width, line);
        let area = Rect::new(
            0,
            top,
            display.width,
            display.height.saturating_sub(top + line),
        );
        let loaded = profile::load(env.storage());
        let mut screen = Self {
            // As high as the row that adds one needs, its note whole (the
            // largest text).
            list: ListView::new(area, add_row_height(env, &style, display.width))
                .focus_on_keys(env),
            status,
            readable: loaded.is_some(),
            profiles: loaded.unwrap_or_default(),
            in_use: Vec::new(),
            style,
            header,
        };
        screen.find_in_use(env);
        screen
    }

    fn find_in_use(&mut self, env: &Env<'_>) {
        self.in_use = self
            .profiles
            .iter()
            .map(|p| profile::in_use(env, p))
            .collect();
    }

    fn rows(&self) -> Vec<Row> {
        let mut rows: Vec<Row> = (0..self.profiles.len()).map(Row::Profile).collect();
        rows.push(Row::Add);
        rows
    }

    fn redraw(&self, cx: &mut Cx<'_, '_>) {
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Fast);
    }

    fn ask_name(&self, cx: &mut Cx<'_, '_>, reply: &Event<Typed>, text: &str, context: &str) {
        let env = *cx.env();
        cx.command(
            ask::text(env.i18n().tr("profiles.name"), reply)
                .initial(text)
                .with(params::PATH, context.to_string())
                .command(),
        );
    }

    fn activate(&mut self, index: usize, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        match self.rows().get(index).copied() {
            Some(Row::Profile(i)) => {
                let mut out = paperpaw_sdk::Commands::new();
                apply(&env, &self.profiles[i], &mut out);
                for command in out {
                    cx.command(command);
                }
                self.redraw(cx);
            }
            Some(Row::Add) => {
                let name = env.i18n().format(
                    "profiles.default-name",
                    &[("number", Arg::Int(self.profiles.len() as i64 + 1))],
                );
                self.ask_name(cx, &NAMED, &name, "");
            }
            None => {}
        }
    }

    fn offer(&mut self, index: usize, cx: &mut Cx<'_, '_>) {
        let Some(Row::Profile(i)) = self.rows().get(index).copied() else {
            return self.activate(index, cx);
        };
        cx.command(
            paperpaw_sdk::ask::choice(&self.profiles[i].name, &CHOSEN)
                .about(i.to_string())
                .options("profiles.option.", &["update", "rename", "remove"])
                .command(),
        );
    }

    fn move_focus(&mut self, delta: isize, cx: &mut Cx<'_, '_>) {
        let len = self.rows().len();
        self.list.move_focus_in(cx.env(), delta, len).request(cx);
    }

    /// Keeps the profiles; not kept, the list goes back to what the card
    /// has and a notice says why.
    fn save(&mut self, cx: &mut Cx<'_, '_>) -> bool {
        let env = *cx.env();
        if self.readable && profile::save(env.storage(), &self.profiles) {
            return true;
        }
        let key = if self.readable {
            "profiles.not-saved"
        } else {
            "profiles.unreadable"
        };
        cx.command(Command::Notify(env.i18n().tr(key).to_string()));
        let loaded = profile::load(env.storage());
        self.readable = loaded.is_some();
        self.profiles = loaded.unwrap_or_default();
        self.list
            .set_focus(self.list.focus().min(self.profiles.len()));
        false
    }

    /// Asks whether to do `option` (update or remove) to the `i`-th
    /// profile: either loses what it kept.
    fn ask_first(&self, option: &str, i: usize, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let name = &self.profiles[i].name;
        let (yes, no) = match option {
            "update" => ("profiles.replace", "profiles.cancel"),
            _ => ("profiles.option.remove", "profiles.keep"),
        };
        let arg = [("name", Arg::Str(name))];
        cx.command(
            ask::confirm(
                env.i18n()
                    .format(&alloc::format!("profiles.{option}.question"), &arg),
                &CONFIRMED,
            )
            .text(env.i18n().tr(&alloc::format!("profiles.{option}.text")))
            .yes(env.i18n().tr(yes))
            .no(env.i18n().tr(no))
            .with(params::PATH, i.to_string())
            .with(OPTION, option)
            .with(NAME, name.clone())
            .command(),
        );
    }

    /// Saves the current settings in the `i`-th profile.
    fn update(&mut self, i: usize, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        self.profiles[i].values = profile::capture(&env);
        let name = self.profiles[i].name.clone();
        if self.save(cx) {
            cx.command(Command::Notify(
                env.i18n()
                    .format("profiles.saved", &[("name", Arg::Str(&name))]),
            ));
        }
        self.redraw(cx);
    }

    /// Whether a profile but the `but`-th is called `name`; if so, says
    /// so.
    fn taken(&self, name: &str, but: Option<usize>, cx: &mut Cx<'_, '_>) -> bool {
        let taken = self
            .profiles
            .iter()
            .enumerate()
            .any(|(i, p)| Some(i) != but && profile::same_name(&p.name, name));
        if taken {
            let env = *cx.env();
            cx.command(Command::Notify(
                env.i18n()
                    .format("profiles.taken", &[("name", Arg::Str(name))]),
            ));
        }
        taken
    }
}

/// A profile's name as entered: one line, never empty.
fn clean(name: &str) -> Option<String> {
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    let name = name.replace(['[', ']'], "");
    (!name.trim().is_empty()).then(|| name.trim().to_string())
}

impl Screen for ListScreen {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let mut ui = ui::Ui::new(canvas, env, &style);
        let rows = self.rows();
        self.header.draw(
            &mut ui,
            env.i18n().tr("profiles.title"),
            None,
            self.list.paging(rows.len()),
        );
        let (profiles, in_use) = (&self.profiles, &self.in_use);
        self.list
            .draw(&mut ui, rows.len(), |ui, index, rect, focused| {
                match rows[index] {
                    Row::Profile(i) => {
                        let p = &profiles[i];
                        let value = if in_use.get(i).copied().unwrap_or(false) {
                            env.i18n().tr("profiles.in-use").to_string()
                        } else {
                            String::new()
                        };
                        let note =
                            env.i18n()
                                .plural("profiles.settings", p.values.len() as i64, &[]);
                        ui::Row::value(&p.name, &value)
                            .note(&note)
                            .focused(focused)
                            .draw(ui, rect);
                    }
                    Row::Add => {
                        let title = env.i18n().tr("profiles.add");
                        let note = env.i18n().tr("profiles.add.help");
                        ui::Row::value(title, "+")
                            .note(note)
                            .focused(focused)
                            .draw(ui, rect);
                    }
                }
            });
        if self.profiles.is_empty() {
            // Below the row that saves one: what profiles are for.
            let top = self.list.area().y + self.list.row() as u16;
            let area = Rect::new(
                0,
                top,
                self.list.area().width,
                self.status.y.saturating_sub(top),
            );
            ui.empty(
                area,
                "stack-2",
                env.i18n().tr("profiles.empty.title"),
                env.i18n().tr("profiles.empty"),
            );
            return;
        }
        ui.canvas().fill(self.status, Color::WHITE);
        ui.note_line(self.status, ui::tr_input(env, "profiles.hint"));
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let a = action;
        if let Some(delta) = ui::list_step(a, self.list.rows() as isize) {
            self.move_focus(delta, cx);
        } else if *a == actions::CONFIRM {
            self.activate(self.list.focus(), cx);
        } else if *a == actions::MENU {
            self.offer(self.list.focus(), cx);
        } else if *a == actions::BACK {
            cx.command(Command::Back);
        } else {
            return Handled::No;
        }
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        let len = self.rows().len();
        let paging = self.list.paging(len);
        if gesture.kind == GestureKind::Tap
            && let Some(action) = self
                .header
                .tap(cx.env(), &self.style, gesture.start, paging)
        {
            cx.command(Command::run(action));
            return Handled::Yes;
        }
        let Some(index) = self.list.hit(gesture.start, len) else {
            return Handled::No;
        };
        self.list.set_focus(index);
        match gesture.kind {
            GestureKind::Tap => self.activate(index, cx),
            GestureKind::LongPress => self.offer(index, cx),
            _ => return Handled::No,
        }
        Handled::Yes
    }

    fn on_event(&mut self, event: &Id, params: &Params, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let index = |context: &str| {
            context
                .parse::<usize>()
                .ok()
                .filter(|&i| i < self.profiles.len())
        };
        if let Some(typed) = NAMED.read(event, params) {
            let Some(name) = clean(&typed.text) else {
                return;
            };
            if self.taken(&name, None, cx) {
                return;
            }
            self.profiles.push(Profile {
                values: profile::capture(&env),
                name: name.clone(),
            });
            if self.save(cx) {
                profile::set_last(env.storage(), &name);
                self.list.set_focus(self.profiles.len() - 1);
                cx.command(Command::Notify(
                    env.i18n()
                        .format("profiles.saved", &[("name", Arg::Str(&name))]),
                ));
            }
            self.redraw(cx);
        } else if let Some(typed) = RENAMED.read(event, params) {
            let at = index(typed.context.get(params::PATH).unwrap_or_default());
            if let (Some(i), Some(name)) = (at, clean(&typed.text)) {
                if self.taken(&name, Some(i), cx) {
                    return;
                }
                let old = core::mem::replace(&mut self.profiles[i].name, name.clone());
                // The profile applied last keeps being it, by its new name.
                if self.save(cx) && profile::last(env.storage()) == old {
                    profile::set_last(env.storage(), &name);
                }
                self.redraw(cx);
            }
        } else if let Some(picked) = CHOSEN.read(event, params)
            && let Some(i) = index(&picked.path)
        {
            match picked.option.as_str() {
                option @ ("update" | "remove") => self.ask_first(option, i, cx),
                "rename" => {
                    let name = self.profiles[i].name.clone();
                    self.ask_name(cx, &RENAMED, &name, &i.to_string());
                }
                _ => {}
            }
        } else if let Some(answer) = CONFIRMED.read(event, params) {
            let context = &answer.context;
            // Still the profile asked about.
            let at = index(context.get(params::PATH).unwrap_or_default())
                .filter(|&i| Some(self.profiles[i].name.as_str()) == context.get(NAME));
            match (answer.yes, at, context.get(OPTION)) {
                (true, Some(i), Some("update")) => self.update(i, cx),
                (true, Some(i), Some("remove")) => {
                    self.profiles.remove(i);
                    self.save(cx);
                    self.list.set_len(self.rows().len());
                    self.redraw(cx);
                }
                _ => {}
            }
        } else if *event == paperpaw_sdk::builtin::events::SETTING_CHANGED {
            // "In use" may have changed.
            self.redraw(cx);
        } else {
            return;
        }
        self.find_in_use(&env);
    }
}

#[cfg(test)]
mod tests {
    use paperpaw_i18n::{Catalog, check};

    #[test]
    fn translations_are_complete() {
        let en = Catalog::parse(include_str!("../i18n/en.ini")).unwrap();
        let de = Catalog::parse(include_str!("../i18n/de.ini")).unwrap();
        let issues = check::compare(&en, &de);
        assert!(issues.is_empty(), "{issues:?}");
    }
}
