//! The flat Reading profiles plugin: one list screen, host dialogs for names
//! and questions, profiles kept in the plugin's data folder.
use crate::native_data::{self, Reader};
use crate::store::{self, Profile};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Handled,
    native::{
        Error, Host, Plugin,
        callbacks::{DrawFrame, Input},
        commands::{CommandSender, RedrawMode},
        events::{AnswerOutcome, Event, Incoming},
        formatting::Arg,
        registrations,
        reply::Reply,
        storage::Root,
        ui::{Instance, Notify, Open, Question, RequestId, TextWrite, Ui},
    },
};
use paperpaw_ui_kit::{
    Footer, Item, ItemList, ItemStep, Messages, Screens, files,
    mem::{SERVICE_BUFFER, buffer, push},
    text::{concat, copy},
    uitext,
};

mod image {
    include!(concat!(env!("OUT_DIR"), "/profiles_declarations.rs"));
}

const OPEN: u32 = 1;
const NEXT: u32 = 2;
const SETTINGS: u32 = 3;
const LIST: u32 = 10;

const FILE: &str = "profiles.ini";
const LAST: &str = "last.txt";
const MAX_FILE: usize = 64 * 1024;
const OPTIONS: [&str; 3] = ["update", "rename", "remove"];

/// The longest profile name the name dialog takes.
const NAME_LIMIT: usize = 256;

/// What an open question is for.
#[derive(Debug, PartialEq, Eq)]
enum Asked {
    /// A name for a new profile.
    NewName,
    /// A new name for profile `index`.
    Rename { index: usize },
    /// What to do with profile `index` (named `name` when asked).
    Options { index: usize, name: String },
    /// Whether to do `option` ("update" or "remove") to profile `index`.
    Sure {
        option: &'static str,
        index: usize,
        name: String,
    },
}

/// The list of profiles.
#[derive(Debug, Default)]
struct View {
    profiles: Vec<Profile>,
    /// Whether the file could be read: if not, nothing is saved over it.
    readable: bool,
    /// Whether the settings are as each profile has them.
    in_use: Vec<bool>,
    hint: String,
    empty: (String, String),
    list: ItemList,
    touch: bool,
}

/// The interface-7 Reading profiles plugin.
#[derive(Debug, Default)]
pub struct NativeProfiles {
    ui: Option<Ui>,
    screens: Screens<View>,
    asked: Option<(RequestId, Asked)>,
}

impl Plugin for NativeProfiles {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::SCREENS
            .iter()
            .chain(image::NAVIGATION)
            .chain(image::EVENTS)
        {
            registrations::submit(host, batch)?;
        }
        // `core.ui` is opened by the first callback: opening needs a live frame.
        Ok(Self::default())
    }

    fn event(
        &mut self,
        host: &Host<'_>,
        token: u32,
        incoming: Incoming<'_>,
        _: &mut Reply<'_>,
    ) -> Result<(), Error> {
        let event = incoming.decode()?;
        self.ensure_ui(host)?;
        match event {
            Event::ActionRun {
                action: "profiles.open",
                ..
            } if token == OPEN => {
                let mut scratch = [0; 128];
                CommandSender::new(host, &mut scratch).open(Open::screen("profiles.list")?)
            }
            Event::ActionRun {
                action: "profiles.next",
                ..
            } if token == NEXT => Self::next(host),
            Event::ScreenOpened { instance, .. } => self.opened(host, instance),
            Event::ScreenShown {
                instance: Some(instance),
                ..
            } => self.screens.shown(LIST, instance),
            Event::ScreenClosed { instance, .. } => {
                self.screens.close(instance);
                Ok(())
            }
            // "In use" may have changed.
            Event::SettingChanged { .. } if token == SETTINGS => self.refresh(host),
            Event::Answered { answer, .. } => self.answered(host, answer),
            _ => Ok(()),
        }
    }

    fn draw(&mut self, host: &Host<'_>, token: u32, mut frame: DrawFrame<'_>) -> Result<(), Error> {
        let view = self.screens.top(token).ok_or(Error::State)?;
        let title = Messages::new()?.copy(host, "profiles.title")?;
        let footer = if view.profiles.is_empty() {
            Footer::Empty {
                title: &view.empty.0,
                text: &view.empty.1,
            }
        } else {
            Footer::Note(&view.hint)
        };
        view.list.draw_with(host, &mut frame, &title, true, footer)
    }

    fn input(&mut self, host: &Host<'_>, token: u32, input: Input<'_>) -> Result<Handled, Error> {
        self.ensure_ui(host)?;
        let Some(view) = self.screens.top(token) else {
            return Ok(Handled::No);
        };
        // The menu button offers what else can be done with a profile.
        if matches!(
            input,
            Input::Action {
                action: "core.menu",
                ..
            }
        ) {
            let focus = view.list.focus();
            self.offer(host, focus)?;
            return Ok(Handled::Yes);
        }
        let step = view.list.input(host, &input)?;
        match step {
            ItemStep::Chosen(index) => self.activate(host, index)?,
            ItemStep::Held(index) => self.offer(host, index)?,
            _ => {}
        }
        Ok(ItemList::handled(step))
    }
}

impl NativeProfiles {
    /// Opens `core.ui` in the first live callback; the handle serves later ones.
    fn ensure_ui(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if self.ui.is_none() {
            self.ui = Some(Ui::open(host)?);
        }
        Ok(())
    }

    fn ui(&self) -> Result<&Ui, Error> {
        self.ui.as_ref().ok_or(Error::State)
    }

    fn opened(&mut self, host: &Host<'_>, instance: Instance) -> Result<(), Error> {
        let mut view = View::default();
        let loaded = load(host)?;
        view.readable = loaded.is_some();
        view.profiles = loaded.unwrap_or_default();
        view.touch = paperpaw_sdk::native::snapshot::capture_snapshot(host)?.has_touch();
        view.rebuild(host)?;
        self.screens.open(instance, view)
    }

    /// Rebuilds every open list from the settings as they are.
    fn refresh(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if self.screens.is_empty() {
            return Ok(());
        }
        for view in self.screens.all() {
            view.rebuild(host)?;
        }
        Self::redraw(host)
    }

    fn redraw(host: &Host<'_>) -> Result<(), Error> {
        let mut scratch = [0; 64];
        CommandSender::new(host, &mut scratch).queue_redraw(RedrawMode::Partial)
    }

    fn notify(host: &Host<'_>, text: &str) -> Result<(), Error> {
        let mut scratch = buffer(1024)?;
        CommandSender::new(host, &mut scratch).notify(Notify::new(text)?)
    }

    /// Applies `profile` and says so.
    fn apply(host: &Host<'_>, profile: &Profile) -> Result<(), Error> {
        let mut reader = Reader::open(host)?;
        let changes = (|| {
            let current = reader.current()?;
            reader.differences(profile, &current)
        })();
        let changes = reader.finish(changes)?;
        let mut scratch = buffer(2048)?;
        let mut commands = CommandSender::new(host, &mut scratch);
        native_data::apply(&mut commands, &changes)?;
        // A failure to note the profile loses only "next" going round.
        let _ = files::replace(host, Root::Data, LAST, profile.name.as_bytes());
        let text = Messages::new()?.format(
            host,
            "profiles.applied",
            &[Arg::Text {
                name: "name",
                value: &profile.name,
            }],
        )?;
        commands.notify(Notify::new(&text)?)
    }

    /// The profile after the one applied last.
    fn next(host: &Host<'_>) -> Result<(), Error> {
        let profiles = load(host)?.unwrap_or_default();
        let last = files::read_all(host, Root::Data, LAST, 1024)?.unwrap_or_default();
        let last = core::str::from_utf8(&last).unwrap_or("");
        let at = profiles.iter().position(|p| p.name == last.trim());
        let next = at.map_or(0, |at| (at + 1) % profiles.len().max(1));
        match profiles.get(next) {
            Some(profile) => Self::apply(host, profile),
            None => Ok(()),
        }
    }

    fn activate(&mut self, host: &Host<'_>, index: usize) -> Result<(), Error> {
        let Some(view) = self.screens.top(LIST) else {
            return Ok(());
        };
        let profiles = view.profiles.len();
        match view.row_profile(index) {
            Some(i) => {
                Self::apply(host, &view.profiles[i])?;
                Self::redraw(host)
            }
            None if index == profiles => {
                let mut messages = Messages::new()?;
                let name = messages.format(
                    host,
                    "profiles.default-name",
                    &[Arg::Int {
                        name: "number",
                        value: profiles as i64 + 1,
                    }],
                )?;
                self.ask_name(host, &name, Asked::NewName)
            }
            None => Ok(()),
        }
    }

    fn offer(&mut self, host: &Host<'_>, index: usize) -> Result<(), Error> {
        let Some(view) = self.screens.top(LIST) else {
            return Ok(());
        };
        let Some(i) = view.row_profile(index) else {
            return self.activate(host, index);
        };
        let name = copy(&view.profiles[i].name)?;
        let mut messages = Messages::new()?;
        let mut choices = String::new();
        for (n, option) in OPTIONS.iter().enumerate() {
            let label = messages.copy(host, &concat(&["profiles.option.", option])?)?;
            let separator = if n > 0 { "\n" } else { "" };
            choices = concat(&[&choices, separator, &label])?;
        }
        let question = Question::choice(&name, "", &choices)?;
        let asked = Asked::Options {
            index: i,
            name: copy(&name)?,
        };
        self.ask(host, question, asked)
    }

    fn ask(&mut self, host: &Host<'_>, question: Question<'_>, asked: Asked) -> Result<(), Error> {
        let mut input = buffer(SERVICE_BUFFER)?;
        let mut output = buffer(SERVICE_BUFFER)?;
        let request = self.ui()?.ask(host, question, &mut input, &mut output)?;
        self.asked = Some((request, asked));
        Ok(())
    }

    fn ask_name(&mut self, host: &Host<'_>, initial: &str, asked: Asked) -> Result<(), Error> {
        let title = Messages::new()?.copy(host, "profiles.name")?;
        let mut input = buffer(SERVICE_BUFFER)?;
        let mut output = buffer(SERVICE_BUFFER)?;
        let request = self
            .ui()?
            .ask(host, Question::text(&title, "")?, &mut input, &mut output)?;
        self.ui()?.text_write(
            host,
            request.text(),
            0,
            initial,
            TextWrite::Last,
            &mut input,
            &mut output,
        )?;
        self.asked = Some((request, asked));
        Ok(())
    }

    fn ask_sure(
        &mut self,
        host: &Host<'_>,
        option: &'static str,
        index: usize,
        name: String,
    ) -> Result<(), Error> {
        let mut messages = Messages::new()?;
        let arg = [Arg::Text {
            name: "name",
            value: &name,
        }];
        let question =
            messages.format(host, &concat(&["profiles.", option, ".question"])?, &arg)?;
        let text = messages.copy(host, &concat(&["profiles.", option, ".text"])?)?;
        let (yes, no) = match option {
            "update" => ("profiles.replace", "profiles.cancel"),
            _ => ("profiles.option.remove", "profiles.keep"),
        };
        let labels = concat(&[
            "",
            &messages.copy(host, yes)?,
            "\n",
            &messages.copy(host, no)?,
        ])?;
        let asked = Question::confirm(&question, &text)?.confirm_labels(&labels)?;
        self.ask(
            host,
            asked,
            Asked::Sure {
                option,
                index,
                name,
            },
        )
    }

    fn answered(
        &mut self,
        host: &Host<'_>,
        answer: paperpaw_sdk::native::events::Answered,
    ) -> Result<(), Error> {
        let Some((request, _)) = &self.asked else {
            return Ok(());
        };
        if *request != answer.request() || answer.outcome() == AnswerOutcome::Typing {
            return Ok(());
        }
        let Some((request, asked)) = self.asked.take() else {
            return Ok(());
        };
        if answer.outcome() != AnswerOutcome::Answered {
            return Ok(());
        }
        match asked {
            Asked::NewName | Asked::Rename { .. } => {
                let typed = uitext::read_all(self.ui()?, host, request.text(), NAME_LIMIT)?;
                let Some(name) = store::clean(&typed)? else {
                    return Ok(());
                };
                match asked {
                    Asked::Rename { index } => self.rename(host, index, &name),
                    _ => self.add(host, &name),
                }
            }
            Asked::Options { index, name } => match OPTIONS.get(answer.choice() as usize) {
                Some(&"update") => self.ask_sure(host, "update", index, name),
                Some(&"remove") => self.ask_sure(host, "remove", index, name),
                Some(&"rename") => {
                    let initial = copy(&name)?;
                    self.ask_name(host, &initial, Asked::Rename { index })
                }
                _ => Ok(()),
            },
            Asked::Sure {
                option,
                index,
                name,
            } if answer.choice() == 1 => {
                // Still the profile asked about.
                let Some(view) = self.screens.top(LIST) else {
                    return Ok(());
                };
                if view.profiles.get(index).is_none_or(|p| p.name != name) {
                    return Ok(());
                }
                if option == "update" {
                    self.update(host, index)
                } else {
                    self.remove(host, index)
                }
            }
            Asked::Sure { .. } => Ok(()),
        }
    }

    /// Keeps the profiles of the list on the card; whether it could. Where it
    /// could not (too large, a card refusal, no memory) `undo` puts the list
    /// back as it was and it shows what the card holds.
    fn save(&mut self, host: &Host<'_>, undo: Undo) -> Result<bool, Error> {
        let Some(view) = self.screens.top(LIST) else {
            return Ok(false);
        };
        // The file may have changed since it was read: a newer version is left as
        // it is, whatever was read before.
        if view.readable {
            view.readable = match files::read_head(host, Root::Data, FILE, store::HEAD) {
                Ok(Some(head)) => store::may_save_over(&head),
                Ok(None) => true,
                Err(Error::NoMemory) => {
                    undo.apply(&mut view.profiles);
                    return Err(Error::NoMemory);
                }
                Err(_) => false,
            };
        }
        // The text is made and measured whole before anything is replaced.
        let saved = view.readable
            && match store::encode_within(&view.profiles, MAX_FILE) {
                Ok(text) => files::replace(host, Root::Data, FILE, text.as_bytes()).is_ok(),
                Err(_) => false,
            };
        if saved {
            return Ok(true);
        }
        undo.apply(&mut view.profiles);
        let key = if view.readable {
            "profiles.not-saved"
        } else {
            "profiles.unreadable"
        };
        let text = Messages::new()?.copy(host, key)?;
        Self::notify(host, &text)?;
        // What the card holds, when it can be read now.
        if let Ok(loaded) = load(host) {
            view.readable = loaded.is_some();
            view.profiles = loaded.unwrap_or_default();
        }
        view.rebuild(host)?;
        Ok(false)
    }

    fn saved_note(host: &Host<'_>, name: &str) -> Result<(), Error> {
        let text = Messages::new()?.format(
            host,
            "profiles.saved",
            &[Arg::Text {
                name: "name",
                value: name,
            }],
        )?;
        Self::notify(host, &text)
    }

    fn taken(&mut self, host: &Host<'_>, name: &str, but: Option<usize>) -> Result<bool, Error> {
        let Some(view) = self.screens.top(LIST) else {
            return Ok(true);
        };
        let taken = view
            .profiles
            .iter()
            .enumerate()
            .any(|(i, p)| Some(i) != but && store::same_name(&p.name, name));
        if taken {
            let text = Messages::new()?.format(
                host,
                "profiles.taken",
                &[Arg::Text {
                    name: "name",
                    value: name,
                }],
            )?;
            Self::notify(host, &text)?;
        }
        Ok(taken)
    }

    fn capture(host: &Host<'_>) -> Result<Vec<(String, String)>, Error> {
        let mut reader = Reader::open(host)?;
        let current = reader.current();
        let current = reader.finish(current)?;
        let mut values = Vec::new();
        values
            .try_reserve_exact(current.len())
            .map_err(|_| Error::NoMemory)?;
        for setting in current {
            values.push((setting.key, setting.value.to_text()?));
        }
        Ok(values)
    }

    fn add(&mut self, host: &Host<'_>, name: &str) -> Result<(), Error> {
        if self.taken(host, name, None)? {
            return Ok(());
        }
        let values = Self::capture(host)?;
        let Some(view) = self.screens.top(LIST) else {
            return Ok(());
        };
        push(
            &mut view.profiles,
            Profile {
                name: copy(name)?,
                values,
            },
        )?;
        let last = view.profiles.len() - 1;
        if self.save(host, Undo::Added)? {
            let _ = files::replace(host, Root::Data, LAST, name.as_bytes());
            if let Some(view) = self.screens.top(LIST) {
                view.rebuild(host)?;
                view.focus_profile(last);
            }
            Self::saved_note(host, name)?;
        }
        Self::redraw(host)
    }

    fn rename(&mut self, host: &Host<'_>, index: usize, name: &str) -> Result<(), Error> {
        if self.taken(host, name, Some(index))? {
            return Ok(());
        }
        let Some(view) = self.screens.top(LIST) else {
            return Ok(());
        };
        // The card is read and both copies are made before the name is touched, so
        // a refusal on the way leaves the list as it was.
        let last = files::read_all(host, Root::Data, LAST, 1024)?.unwrap_or_default();
        let Some(old) = rename_in(&mut view.profiles, index, name)? else {
            return Ok(());
        };
        let was_last = is_last(&last, &old);
        if self.save(host, Undo::Renamed { index, old })? {
            // The profile applied last keeps being it, by its new name.
            if was_last {
                let _ = files::replace(host, Root::Data, LAST, name.as_bytes());
            }
            if let Some(view) = self.screens.top(LIST) {
                view.rebuild(host)?;
            }
        }
        Self::redraw(host)
    }

    fn update(&mut self, host: &Host<'_>, index: usize) -> Result<(), Error> {
        let values = Self::capture(host)?;
        let Some(view) = self.screens.top(LIST) else {
            return Ok(());
        };
        let Some(profile) = view.profiles.get_mut(index) else {
            return Ok(());
        };
        let name = copy(&profile.name)?;
        let old = core::mem::replace(&mut profile.values, values);
        if self.save(host, Undo::Updated { index, old })? {
            if let Some(view) = self.screens.top(LIST) {
                view.rebuild(host)?;
            }
            Self::saved_note(host, &name)?;
        }
        Self::redraw(host)
    }

    fn remove(&mut self, host: &Host<'_>, index: usize) -> Result<(), Error> {
        let Some(view) = self.screens.top(LIST) else {
            return Ok(());
        };
        if index < view.profiles.len() {
            let removed = view.profiles.remove(index);
            self.save(
                host,
                Undo::Removed {
                    index,
                    profile: removed,
                },
            )?;
        }
        if let Some(view) = self.screens.top(LIST) {
            view.rebuild(host)?;
        }
        Self::redraw(host)
    }
}

/// Gives profile `index` the name `name`; returns the name it had.
///
/// Both copies are made before anything is changed: a refused allocation leaves the
/// list untouched. `None` when there is no such profile.
fn rename_in(profiles: &mut [Profile], index: usize, name: &str) -> Result<Option<String>, Error> {
    let Some(profile) = profiles.get_mut(index) else {
        return Ok(None);
    };
    let new = copy(name)?;
    let old = copy(&profile.name)?;
    profile.name = new;
    Ok(Some(old))
}

/// Whether the bytes of `last.txt` name `profile`. Bytes that are not UTF-8 name none;
/// nothing is allocated.
fn is_last(last: &[u8], profile: &str) -> bool {
    core::str::from_utf8(last).is_ok_and(|last| last.trim() == profile)
}

/// How to put the list back where a save was refused.
enum Undo {
    /// The last profile was added.
    Added,
    Renamed {
        index: usize,
        old: String,
    },
    Updated {
        index: usize,
        old: Vec<(String, String)>,
    },
    /// Room for it is still there: it was just taken out.
    Removed {
        index: usize,
        profile: Profile,
    },
}

impl Undo {
    fn apply(self, profiles: &mut Vec<Profile>) {
        match self {
            Self::Added => {
                profiles.pop();
            }
            Self::Renamed { index, old } => {
                if let Some(profile) = profiles.get_mut(index) {
                    profile.name = old;
                }
            }
            Self::Updated { index, old } => {
                if let Some(profile) = profiles.get_mut(index) {
                    profile.values = old;
                }
            }
            Self::Removed { index, profile } => {
                if index <= profiles.len() {
                    profiles.insert(index, profile);
                }
            }
        }
    }
}

/// The profiles kept: none where there is no file yet, `None` where it cannot
/// be read (then nothing may be saved over it).
fn load(host: &Host<'_>) -> Result<Option<Vec<Profile>>, Error> {
    match files::read_all(host, Root::Data, FILE, MAX_FILE) {
        Ok(Some(bytes)) => store::decode(&bytes),
        Ok(None) => Ok(Some(Vec::new())),
        Err(Error::NoMemory) => Err(Error::NoMemory),
        Err(_) => Ok(None),
    }
}

impl View {
    /// The profile an item is, if it is one (the last item adds one).
    fn row_profile(&self, item: usize) -> Option<usize> {
        (item < self.profiles.len()).then_some(item)
    }

    fn focus_profile(&mut self, index: usize) {
        self.list.set_focus(index);
    }

    fn rebuild(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let mut messages = Messages::new()?;
        let mut reader = Reader::open(host)?;
        let facts = (|| {
            let current = reader.current()?;
            let mut in_use = Vec::new();
            in_use
                .try_reserve_exact(self.profiles.len())
                .map_err(|_| Error::NoMemory)?;
            for profile in &self.profiles {
                in_use.push(
                    !profile.values.is_empty() && reader.differences(profile, &current)?.is_empty(),
                );
            }
            Ok(in_use)
        })();
        self.in_use = reader.finish(facts)?;
        let mut items = Vec::new();
        items
            .try_reserve_exact(self.profiles.len() + 1)
            .map_err(|_| Error::NoMemory)?;
        for (i, profile) in self.profiles.iter().enumerate() {
            let value = if self.in_use.get(i).copied().unwrap_or(false) {
                messages.copy(host, "profiles.in-use")?
            } else {
                String::new()
            };
            let note = messages.format(
                host,
                "profiles.settings",
                &[Arg::Count(profile.values.len() as i64)],
            )?;
            items.push(Item::noted(&profile.name, &value, &note)?);
        }
        items.push(Item::noted(
            &messages.copy(host, "profiles.add")?,
            "+",
            &messages.copy(host, "profiles.add.help")?,
        )?);
        let hint = if self.touch {
            "profiles.hint"
        } else {
            "profiles.hint.buttons"
        };
        self.hint = messages.copy(host, hint)?;
        self.empty = (
            messages.copy(host, "profiles.empty.title")?,
            messages.copy(host, "profiles.empty")?,
        );
        self.list.replace(items);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn list() -> Vec<Profile> {
        vec![
            Profile {
                name: "Night".into(),
                values: Vec::new(),
            },
            Profile {
                name: "Other".into(),
                values: Vec::new(),
            },
        ]
    }

    #[test]
    fn a_refused_allocation_leaves_the_names_as_they_were_and_a_retry_succeeds() {
        let mut profiles = list();
        // Each run flips the first name between "Night" and "Day".
        crate::fail_alloc::sweep(|| {
            let night = profiles[0].name == "Night";
            let target = if night { "Day" } else { "Night" };
            let result = rename_in(&mut profiles, 0, target);
            match &result {
                Ok(Some(old)) => {
                    assert_eq!(old == "Night", night);
                    assert_eq!(profiles[0].name, target);
                }
                Ok(None) => panic!("the profile exists"),
                // The list is exactly as before the call.
                Err(_) => assert_eq!(profiles[0].name == "Night", night),
            }
            assert_eq!(profiles[1].name, "Other");
            result
        });
    }

    #[test]
    fn renaming_a_profile_that_is_not_there_changes_nothing() {
        let mut profiles = list();
        assert_eq!(rename_in(&mut profiles, 2, "X"), Ok(None));
        assert_eq!(profiles[0].name, "Night");
    }

    #[test]
    fn last_txt_names_a_profile_only_when_it_is_valid_utf8_and_equal() {
        assert!(is_last(b"Night\n", "Night"));
        assert!(!is_last(b"Other", "Night"));
        assert!(!is_last(b"", "Night"));
        // Malformed bytes name nothing, and nothing is allocated for them.
        assert!(!is_last(&[0xff, 0xfe, b'N'], "Night"));
        assert!(!is_last(b"Nig\xc3", "Night"));
        crate::fail_alloc::sweep(|| {
            let _ = is_last(&[0xff, 0xfe], "Night");
            Ok::<_, Error>(())
        });
    }
}
