//! The files of formats 3 that make actions, menus and steps of the
//! first-run setup: `actions.ini`, `menus.ini` and `setup.ini`.
//!
//! A key that takes several items takes one a line, the key repeated
//! (`run`, `entries`, `hide`), as `marks.ini` takes its patterns. A value
//! is one item as it is: a comma in it is part of it (the words of a
//! notification, a list a setting takes). Ids hold no comma, so a list of
//! ids may also be one line, the ids separated by commas (`entries`,
//! `hide`), as these files were first written.
//!
//! `menus.ini` tells a menu of another plugin from one of the plugin's
//! own by its section: `[menu core.reader-menu]` adds to or hides from
//! that menu; any other section is a menu of the plugin's own (`title`,
//! `entries`). A section that is no `[menu ...]` but adds or hides is
//! still read as the menu it names, as such files were first written.

use alloc::borrow::Cow;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::Staging;
use paperpaw_sdk::{Id, RegisterError};

use super::{own_id, parse_error, parsed_id};

/// Most steps of an action of `actions.ini`.
pub(super) const MAX_STEPS: usize = 16;

/// The prefix of a section of `menus.ini` that names another plugin's
/// menu.
const FOREIGN_MENU: &str = "menu ";

/// A step of an action of `actions.ini`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Step {
    /// `<setting>=<value>`, the value as written in `paperpaw.ini`.
    Set(Id, String),
    /// `<action>`.
    Run(Id),
    /// `open:<slot>`.
    Open(Id),
    /// `notify:<key>`: a translation key, the notification its text in the
    /// reader's language (a text that is no key shows as it is).
    Notify(String),
    /// `emit:<event>`.
    Emit(Id),
}

impl Step {
    pub(super) fn parse(text: &str, line: u32) -> Result<Step, RegisterError> {
        let text = text.trim();
        if let Some(key) = text.strip_prefix("notify:") {
            return Ok(Step::Notify(key.trim().to_string()));
        }
        if let Some(slot) = text.strip_prefix("open:") {
            return Ok(Step::Open(parsed_id(slot, line)?));
        }
        if let Some(event) = text.strip_prefix("emit:") {
            return Ok(Step::Emit(parsed_id(event, line)?));
        }
        match text.split_once('=') {
            Some((setting, value)) => Ok(Step::Set(
                parsed_id(setting, line)?,
                value.trim().to_string(),
            )),
            None => Ok(Step::Run(parsed_id(text, line)?)),
        }
    }

    /// The command of this step; a value not of the setting's kind (or a
    /// setting not there) makes none.
    pub(crate) fn command(&self, env: &crate::Place<'_>) -> Option<crate::Ask> {
        use crate::Ask;
        use paperpaw_sdk::Params;
        Some(match self {
            Step::Set(id, text) => {
                let value = env.settings().spec(id.as_str())?.kind.parse(text)?;
                Ask::SetSetting(id.clone(), value)
            }
            Step::Run(action) => Ask::run(action.clone()),
            Step::Open(slot) => Ask::OpenSlot(slot.clone(), Params::new()),
            Step::Notify(key) => Ask::Notify(env.i18n().tr(key).to_string()),
            Step::Emit(event) => Ask::Emit(event.clone(), Params::new()),
        })
    }
}

/// An action of `actions.ini` as read: its id, title, icon, steps and
/// the line it begins on.
struct ReadAction {
    id: Id,
    title: String,
    icon: Option<String>,
    steps: Vec<Step>,
    line: u32,
}

/// An action of `actions.ini`, with its icon.
pub(super) struct MadeAction {
    pub(super) id: Id,
    pub(super) spec: crate::kit::action::ActionSpec,
    /// What it does, a step a line: the kernel runs them.
    pub(super) steps: alloc::boxed::Box<[Step]>,
    pub(super) icon: Option<String>,
}

/// The actions of an `actions.ini`, a section each: `title`, `icon` and
/// `run`, a step a line. None runs an action of the same file: they would
/// run each other without end.
pub(super) fn actions(plugin: &Id, source: &str) -> Result<Vec<MadeAction>, RegisterError> {
    let mut found: Vec<ReadAction> = Vec::new();
    for entry in paperpaw_config::parse(source) {
        let entry = entry.map_err(parse_error)?;
        if entry.section.is_empty() {
            continue;
        }
        let id = own_id(plugin, entry.section, entry.line)?;
        if found.last().is_none_or(|last| last.id != id) {
            let (title, icon, steps, line) = (String::new(), None, Vec::new(), entry.line);
            found.push(ReadAction {
                id,
                title,
                icon,
                steps,
                line,
            });
        }
        let Some(ReadAction {
            title, icon, steps, ..
        }) = found.last_mut()
        else {
            continue;
        };
        let value = entry.value.as_str();
        match entry.key {
            "title" => *title = value.trim().to_string(),
            "icon" => *icon = Some(value.trim().to_string()),
            // One step a line: a comma is part of the step.
            "run" if !value.trim().is_empty() => steps.push(Step::parse(&value, entry.line)?),
            _ => {}
        }
    }
    vet(plugin, &found)?;
    let mut made = Vec::new();
    for ReadAction {
        id,
        title,
        icon,
        steps,
        ..
    } in found
    {
        let title = if title.is_empty() {
            id.as_str().to_string()
        } else {
            title
        };
        let spec = crate::kit::action::ActionSpec::new(id.clone(), title).run_by_kernel();
        let steps = steps.into_boxed_slice();
        made.push(MadeAction {
            id,
            spec,
            steps,
            icon,
        });
    }
    Ok(made)
}

/// What refuses the actions `found` of `plugin`'s file: an event not of
/// its own, too many steps, or an action of the same file run.
fn vet(plugin: &Id, found: &[ReadAction]) -> Result<(), RegisterError> {
    let ids: Vec<&Id> = found.iter().map(|action| &action.id).collect();
    for action in found {
        let id = &action.id;
        let problem =
            if action.steps.iter().any(
                |step| matches!(step, Step::Emit(event) if event.namespace() != plugin.as_str()),
            ) {
                Some(format!("`{id}`: emits only events of its own"))
            } else if action.steps.len() > MAX_STEPS {
                Some(format!("`{id}`: more than {MAX_STEPS} steps"))
            } else if action
                .steps
                .iter()
                .any(|step| matches!(step, Step::Run(other) if ids.contains(&other)))
            {
                Some(format!("`{id}`: runs an action of the same file"))
            } else {
                None
            };
        if let Some(message) = problem {
            return Err(RegisterError::Parse {
                line: action.line,
                message: Cow::Owned(message),
            });
        }
    }
    Ok(())
}

/// The ids of a value of a list of ids: one, or several separated by
/// commas.
fn ids_of(value: &str, line: u32) -> Result<Vec<Id>, RegisterError> {
    value
        .split(',')
        .filter(|item| !item.trim().is_empty())
        .map(|item| parsed_id(item, line))
        .collect()
}

/// The entries of a `menus.ini`: `[menu <menu>]` for a menu of another
/// plugin, `add = <item>` (also `<item> after <other>`) and
/// `hide = <item>`; any other section a menu of one's own, `title` and
/// `entries`, an action of that name that shows it.
pub(super) fn menus(r: &mut Staging, plugin: &Id, source: &str) -> Result<(), RegisterError> {
    let mut order: i16 = 0;
    for entry in paperpaw_config::parse(source) {
        let entry = entry.map_err(parse_error)?;
        if entry.section.is_empty() {
            continue;
        }
        let value = entry.value.as_str();
        let line = entry.line;
        let foreign = entry.section.strip_prefix(FOREIGN_MENU);
        match (entry.key, foreign) {
            ("title", None) => {
                let menu = own_id(plugin, entry.section, line)?;
                r.add_action(crate::kit::action::ActionSpec::new(
                    menu.clone(),
                    value.trim().to_string(),
                ))?;
            }
            ("entries", None) => {
                let menu = own_id(plugin, entry.section, line)?;
                for item in ids_of(&value, line)? {
                    order = order.saturating_add(10);
                    let entry = crate::kit::menus::MenuEntry::new(menu.clone(), item);
                    r.add_menu_entry(entry.order(order))?;
                }
            }
            ("add", _) => {
                let menu = parsed_id(foreign.unwrap_or(entry.section), line)?;
                let (item, after) = match value.split_once(" after ") {
                    Some((item, after)) => (item, Some(after)),
                    None => (value.as_ref(), None),
                };
                let item = parsed_id(item, line)?;
                match after {
                    Some(after) => {
                        let place = crate::kit::menus::MenuPlace::After(parsed_id(after, line)?);
                        r.place_menu_entry(menu, item, place)?;
                    }
                    None => r.add_menu_entry(crate::kit::menus::MenuEntry::new(menu, item))?,
                }
            }
            ("hide", _) => {
                let menu = parsed_id(foreign.unwrap_or(entry.section), line)?;
                for item in ids_of(&value, line)? {
                    r.hide_menu_entry(menu.clone(), item)?;
                }
            }
            (key @ ("title" | "entries"), Some(menu)) => {
                return Err(RegisterError::Parse {
                    line,
                    message: Cow::Owned(format!(
                        "`{key}` makes a menu of one's own, not `{menu}`: a section of its own name"
                    )),
                });
            }
            _ => {}
        }
    }
    Ok(())
}

/// The steps of a `setup.ini`, a section each: `setting` (or `screen`),
/// `order`, `title` and `help`.
pub(super) fn setup_steps(
    plugin: &Id,
    source: &str,
) -> Result<Vec<crate::kit::setup::SetupStepSpec>, RegisterError> {
    use crate::kit::setup::{SetupPage, SetupStepSpec};
    let mut steps: Vec<(Id, Option<SetupPage>, i32, String, String, u32)> = Vec::new();
    for entry in paperpaw_config::parse(source) {
        let entry = entry.map_err(parse_error)?;
        if entry.section.is_empty() {
            continue;
        }
        let id = own_id(plugin, entry.section, entry.line)?;
        if steps.last().is_none_or(|(last, ..)| *last != id) {
            steps.push((id, None, 100, String::new(), String::new(), entry.line));
        }
        let Some((_, page, order, title, help, _)) = steps.last_mut() else {
            continue;
        };
        let value = entry.value.as_str();
        match entry.key {
            "setting" => *page = Some(SetupPage::Setting(parsed_id(&value, entry.line)?)),
            "screen" => *page = Some(SetupPage::Screen(parsed_id(&value, entry.line)?)),
            "order" => {
                *order = value.trim().parse().map_err(|_| RegisterError::Parse {
                    line: entry.line,
                    message: Cow::Owned(format!("invalid order `{value}`")),
                })?;
            }
            "title" => *title = value.trim().to_string(),
            "help" => *help = value.trim().to_string(),
            _ => {}
        }
    }
    steps
        .into_iter()
        .map(|(id, page, order, title, help, line)| {
            let page = page.ok_or_else(|| RegisterError::Parse {
                line,
                message: Cow::Owned(format!("`{id}`: no setting or screen")),
            })?;
            Ok(SetupStepSpec::new(id, title, page).order(order).help(help))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Steps of an action as written; an action with too many steps or
    /// one running an action of its own file is refused.
    #[test]
    fn actions_are_made_of_steps() {
        let step = |text| Step::parse(text, 1).unwrap();
        assert_eq!(
            step(" notify:night.good-night "),
            Step::Notify("night.good-night".into())
        );
        assert_eq!(
            step("open:core.library"),
            Step::Open(Id::new_static("core.library"))
        );
        assert_eq!(
            step("emit:my.event"),
            Step::Emit(Id::new_static("my.event"))
        );
        assert_eq!(
            step("core.font-size = 14"),
            Step::Set(Id::new_static("core.font-size"), "14".into())
        );
        assert_eq!(step("core.home"), Step::Run(Id::new_static("core.home")));
        assert!(Step::parse("Not An Id", 1).is_err());
        let plugin = Id::new_static("cosy");
        // Only its own events, no setting of the lock.
        assert!(actions(&plugin, "[a]\nrun = emit:core.started\n").is_err());
        assert!(actions(&plugin, "[a]\nrun = emit:cosy.done\n").is_ok());
        assert!(
            actions(&plugin, "[a]\nrun = lock.kind=lock.kind.off\n").is_ok(),
            "refused as it runs, see vet_reserved"
        );
        let many = alloc::vec!["run = core.home"; MAX_STEPS + 1].join("\n");
        assert!(actions(&plugin, &format!("[a]\n{many}\n")).is_err());
        assert!(actions(&plugin, "[a]\nrun = cosy.b\n[b]\nrun = core.home\n").is_err());
        let made = actions(&plugin, "[cosy.c]\ntitle = C\nrun = core.home\n").unwrap();
        assert_eq!(made[0].id.as_str(), "cosy.c");
        assert!(setup_steps(&plugin, "[s]\norder = 3\n").is_err());
    }

    /// A step a line: a comma inside a step is part of it, as in the
    /// words of a notification or a list a setting takes.
    #[test]
    fn a_step_is_a_line_and_keeps_its_commas() {
        let plugin = Id::new_static("cosy");
        let source = "[night]\nrun = core.night-mode=on\nrun = notify:Good night, sleep well\nrun = home.footer.items=a.b,c.d\n";
        let mut found = Vec::new();
        for entry in paperpaw_config::parse(source).map(Result::unwrap) {
            if entry.key == "run" {
                found.push(Step::parse(&entry.value.as_str(), entry.line).unwrap());
            }
        }
        assert_eq!(
            found,
            [
                Step::Set(Id::new_static("core.night-mode"), "on".into()),
                Step::Notify("Good night, sleep well".into()),
                Step::Set(Id::new_static("home.footer.items"), "a.b,c.d".into()),
            ]
        );
        assert_eq!(actions(&plugin, source).unwrap().len(), 1);
    }

    /// `notify:` names a translation key: the notification says its text
    /// in the reader's language; a text that is no key shows as it is.
    #[test]
    fn a_notification_is_said_in_the_reader_s_language() {
        use crate::Ask;
        use paperpaw_platform::Instant;
        use paperpaw_sdk::MemoryStorage;
        let storage = MemoryStorage::new()
            .with("plugins/cosy/plugin.ini", "[plugin]\nname = Cosy\nformats = 3\n")
            .with(
                "plugins/cosy/i18n/en.ini",
                "[meta]\nlanguage = en\nname = English\nplural = one-other\n\n[messages]\ncosy.good-night = Good night, sleep well\n",
            );
        let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
            .storage(storage)
            .build()
            .unwrap();
        kernel.start(Instant(0));
        let env = kernel.shared.place(Instant(0));
        let said = |key: &str| match Step::Notify(key.into()).command(&env) {
            Some(Ask::Notify(text)) => text,
            other => panic!("{other:?}"),
        };
        assert_eq!(said("cosy.good-night"), "Good night, sleep well");
        assert_eq!(said("Good night"), "Good night");
    }

    /// Ids of a list one a line, or several on one line.
    #[test]
    fn ids_one_a_line_or_separated_by_commas() {
        let ids = ids_of("a.x, b.y,, c.z", 1).unwrap();
        assert_eq!(ids.len(), 3);
        assert!(ids_of("a.x, Not An Id", 1).is_err());
    }
}
