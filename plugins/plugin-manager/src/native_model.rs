//! What the plugin manager shows, free of host calls: the state of a plugin
//! and the note below its name, what a start in safe mode says, and which
//! screens may be chosen for a place.
use alloc::{string::String, vec::Vec};
use core::cmp::Ordering;
use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::{
    mem::{join, push, stable_sort_by},
    text::{cmp_lowercase, concat, copy, decimal},
};

/// The host's own row in the list of plugins.
pub const CORE: &str = "core";
/// This plugin: without it plugins could not be switched on again.
pub const SELF_ID: &str = "plugin-manager";

/// `reg_plugin.flags` bits (`docs/abi/registry.md`).
const ENABLED: u32 = 1;
const RESTART_NEEDED: u32 = 1 << 1;
const REQUIRED: u32 = 1 << 2;

/// Where a plugin's code is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Kind {
    #[default]
    BuiltIn,
    Data,
    Code,
}

/// What the host says it is doing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Run {
    #[default]
    Running,
    Off,
    Failed,
}

/// Why a plugin with code does not run (the interface-6 order, which
/// `reg_plugin.problem` keeps; 0 is none).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Problem {
    OtherBuild,
    Damaged,
    Mismatch,
    NoRoom,
    Unsupported,
    SafeMode,
    Crashed,
    NotInstalled,
    Changed,
    Older,
    NeedsNewer,
    Revoked,
    /// The plugin's batch asked for more book formats than a plugin or the
    /// device may have (`registry.md`, problem 14; the arg is the cap).
    TooManyFormats,
    /// As above for reading apps (problem 15).
    TooManyReaders,
    /// A number this plugin does not know.
    Other,
}

impl Problem {
    /// The problem a wire number names; none for 0.
    #[must_use]
    pub const fn from_wire(code: u32) -> Option<Self> {
        Some(match code {
            0 => return None,
            1 => Self::OtherBuild,
            2 => Self::Damaged,
            3 => Self::Mismatch,
            4 => Self::NoRoom,
            5 => Self::Unsupported,
            6 => Self::SafeMode,
            7 => Self::Crashed,
            8 => Self::NotInstalled,
            9 => Self::Changed,
            10 => Self::Older,
            11 => Self::NeedsNewer,
            12 => Self::Revoked,
            14 => Self::TooManyFormats,
            15 => Self::TooManyReaders,
            _ => Self::Other,
        })
    }

    /// The text that says it.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::OtherBuild => "plugin-manager.code.other-build",
            Self::Damaged => "plugin-manager.code.damaged",
            Self::Mismatch => "plugin-manager.code.mismatch",
            // The stock-layout wording (the web flasher gives much more
            // room) needs the code room's larger layout, which the device
            // reads do not tell: `code-room-larger-layout` in
            // `tests/acceptance_pending.rs`.
            Self::NoRoom => "plugin-manager.code.no-room",
            Self::Unsupported => "plugin-manager.code.unsupported",
            Self::SafeMode => "plugin-manager.code.safe-mode",
            Self::Crashed => "plugin-manager.code.crashed",
            Self::NotInstalled => "plugin-manager.code.not-installed",
            Self::Changed => "plugin-manager.code.changed",
            Self::Older => "plugin-manager.code.older",
            Self::NeedsNewer => "plugin-manager.code.needs-newer",
            Self::Revoked => "plugin-manager.code.revoked",
            Self::TooManyFormats => "plugin-manager.formats.too-many",
            Self::TooManyReaders => "plugin-manager.readers.too-many",
            Self::Other => "plugin-manager.failed",
        }
    }
}

/// One plugin of the list.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PluginRow {
    pub id: String,
    pub name: String,
    pub version: String,
    pub kind: Kind,
    pub run: Run,
    pub flags: u32,
    pub problem: Option<Problem>,
    /// What the problem carries (the cap a refused batch ran into); 0 for none.
    pub problem_arg: u32,
    /// The version that takes over after a restart; empty for none.
    pub update: String,
}

impl PluginRow {
    #[must_use]
    pub const fn enabled(&self) -> bool {
        self.flags & ENABLED != 0
    }

    #[must_use]
    pub const fn restart_needed(&self) -> bool {
        self.flags & RESTART_NEEDED != 0
    }

    /// Another enabled plugin needs it (or the host does).
    #[must_use]
    pub const fn required(&self) -> bool {
        self.flags & REQUIRED != 0
    }

    /// Whether the plugin must stay on: the host says so, or it is this
    /// manager.
    #[must_use]
    pub fn locked(&self) -> bool {
        self.id == CORE || self.required() || self.id == SELF_ID
    }

    /// The state shown on the right of the row.
    #[must_use]
    pub fn state_key(&self) -> &'static str {
        if self.id == CORE {
            "plugin-manager.always-on"
        } else if self.run == Run::Failed && self.enabled() {
            "plugin-manager.error"
        } else if self.locked() {
            "plugin-manager.required"
        } else if self.enabled() {
            "plugin-manager.on"
        } else {
            "plugin-manager.off"
        }
    }
}

/// Orders the rows: the host first, then by name without regard to case.
pub fn sort(rows: &mut [PluginRow]) {
    stable_sort_by(rows, |a, b| -> Ordering {
        (a.id != CORE)
            .cmp(&(b.id != CORE))
            .then_with(|| cmp_lowercase(&a.name, &b.name))
    });
}

/// Says a text of the catalog, with or without one value.
pub trait Say {
    /// # Errors
    /// Memory and host errors.
    fn say(&mut self, key: &str) -> Result<String, Error>;
    /// The text with `{name}` replaced by `value`.
    ///
    /// # Errors
    /// Memory and host errors.
    fn say_with(&mut self, key: &str, name: &str, value: &str) -> Result<String, Error>;
}

/// The note below a plugin's name: what happens to it, its description and
/// version. The parts are joined by a space.
///
/// # Errors
/// Memory and host errors.
pub fn note(say: &mut impl Say, row: &PluginRow, description: &str) -> Result<String, Error> {
    let what = if row.id == CORE {
        None
    } else if let Some(problem) = row.problem {
        // Switched off by safe mode, or only held back at this start.
        let key = match problem {
            Problem::SafeMode if !row.enabled() => "plugin-manager.code.safe-mode-off",
            other => other.key(),
        };
        match problem {
            // The limit the batch ran into, where the host says it.
            Problem::TooManyFormats | Problem::TooManyReaders if row.problem_arg != 0 => {
                let mut digits = [0u8; 20];
                let cap = decimal(row.problem_arg as usize, &mut digits);
                Some(say.say_with(key, "cap", cap)?)
            }
            // No limit said: the text would have a hole where it stands.
            Problem::TooManyFormats | Problem::TooManyReaders => {
                Some(say.say("plugin-manager.failed")?)
            }
            _ => Some(say.say(key)?),
        }
    } else if row.id == SELF_ID && !row.required() {
        Some(say.say("plugin-manager.self")?)
    } else if row.required() {
        // Which places it is needed for is not told yet:
        // `reg-plugin-required-for` in `tests/acceptance_pending.rs`.
        Some(say.say("plugin-manager.needed")?)
    } else if !row.update.is_empty() {
        Some(say.say_with(
            "plugin-manager.updates-after-restart",
            "version",
            &row.update,
        )?)
    } else if row.restart_needed() {
        Some(say.say(if row.enabled() {
            "plugin-manager.starts-after-restart"
        } else {
            "plugin-manager.stops-after-restart"
        })?)
    } else if row.run == Run::Failed {
        Some(say.say("plugin-manager.failed")?)
    } else {
        None
    };
    let code = if row.kind == Kind::Code {
        Some(say.say("plugin-manager.code")?)
    } else {
        None
    };
    let version = say.say_with("plugin-manager.version", "version", &row.version)?;
    let mut parts: Vec<String> = Vec::new();
    for part in [what, Some(copy(description)?), code, Some(version)]
        .into_iter()
        .flatten()
    {
        if !part.is_empty() {
            push(&mut parts, part)?;
        }
    }
    join(&parts, " ")
}

/// Why the plugins with code are off, at a start in safe mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SafeMode {
    /// The key was held at the start.
    User,
    /// The last starts failed.
    Failures,
}

/// What a start in safe mode says: why, the plugin that crashed or was
/// loaded last. `name` is the plugin's name, or its id.
///
/// # Errors
/// Memory and host errors.
pub fn safe_mode_notice(
    say: &mut impl Say,
    reason: SafeMode,
    crashed: &str,
    last_loaded: &str,
    name: impl Fn(&str) -> Result<String, Error>,
) -> Result<String, Error> {
    // Short: a notice has a few lines.
    if reason == SafeMode::User {
        return say.say("plugin-manager.safe-mode.key");
    }
    if !crashed.is_empty() {
        return say.say_with("plugin-manager.safe-mode.crashed", "name", &name(crashed)?);
    }
    if !last_loaded.is_empty() {
        return say.say_with("plugin-manager.safe-mode.last", "name", &name(last_loaded)?);
    }
    say.say("plugin-manager.safe-mode.failed")
}

/// Why the shell did not install a plugin at the start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotInstalled {
    OtherAuthor,
    Damaged,
}

/// What is said of the plugins the shell did not install at the start:
/// their names, and why. None for none.
///
/// # Errors
/// Memory and host errors.
pub fn not_installed_notice(
    say: &mut impl Say,
    plugins: &[(String, NotInstalled)],
    name: impl Fn(&str) -> Result<String, Error>,
) -> Result<Option<String>, Error> {
    Ok(Some(match plugins {
        [] => return Ok(None),
        [(id, reason)] => say.say_with(
            match reason {
                NotInstalled::OtherAuthor => "plugin-manager.not-installed.other-author",
                NotInstalled::Damaged => "plugin-manager.not-installed.damaged",
            },
            "name",
            &name(id)?,
        )?,
        several => {
            let mut names: Vec<String> = Vec::new();
            for (id, _) in several {
                push(&mut names, name(id)?)?;
            }
            say.say_with(
                "plugin-manager.not-installed.several",
                "name",
                &join(&names, ", ")?,
            )?
        }
    }))
}

/// A screen offered for a place, as the offers list it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Offer {
    pub screen: String,
    /// The plugin that offers it.
    pub plugin: String,
}

/// The offers to choose from: the host's placeholder shows only where
/// nothing else does.
pub const PLACEHOLDER: &str = "core.placeholder";

#[must_use]
pub fn choosable(offers: &[Offer]) -> usize {
    offers.iter().filter(|o| o.screen != PLACEHOLDER).count()
}

/// `slot screen`, the form kept between starts.
///
/// # Errors
/// Memory.
pub fn line(slot: &str, screen: &str) -> Result<String, Error> {
    concat(&[slot, " ", screen])
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "fixture errors fail the tests")]
    use super::*;

    /// Says each text as its key, a value as `key[name=value]`.
    struct Keys;
    impl Say for Keys {
        fn say(&mut self, key: &str) -> Result<String, Error> {
            copy(key)
        }
        fn say_with(&mut self, key: &str, name: &str, value: &str) -> Result<String, Error> {
            concat(&[key, "[", name, "=", value, "]"])
        }
    }

    fn row(id: &str, name: &str, flags: u32) -> PluginRow {
        PluginRow {
            id: id.into(),
            name: name.into(),
            version: "1.2.3".into(),
            kind: Kind::BuiltIn,
            run: Run::Running,
            flags,
            problem: None,
            problem_arg: 0,
            update: String::new(),
        }
    }

    fn note_of(row: &PluginRow, description: &str) -> String {
        note(&mut Keys, row, description).unwrap()
    }

    #[test]
    fn the_wire_problems_keep_the_interface_6_order() {
        assert_eq!(Problem::from_wire(0), None);
        let order = [
            Problem::OtherBuild,
            Problem::Damaged,
            Problem::Mismatch,
            Problem::NoRoom,
            Problem::Unsupported,
            Problem::SafeMode,
            Problem::Crashed,
            Problem::NotInstalled,
            Problem::Changed,
            Problem::Older,
            Problem::NeedsNewer,
            Problem::Revoked,
        ];
        for (at, problem) in order.into_iter().enumerate() {
            assert_eq!(Problem::from_wire(at as u32 + 1), Some(problem));
        }
        // 13 is not named; 14 and 15 are the caps of formats and readers.
        assert_eq!(Problem::from_wire(13), Some(Problem::Other));
        assert_eq!(Problem::from_wire(14), Some(Problem::TooManyFormats));
        assert_eq!(Problem::from_wire(15), Some(Problem::TooManyReaders));
        assert_eq!(Problem::from_wire(99), Some(Problem::Other));
        assert_eq!(Problem::Other.key(), "plugin-manager.failed");
    }

    #[test]
    fn the_state_follows_the_flags() {
        assert_eq!(
            row("core", "Core", 1).state_key(),
            "plugin-manager.always-on"
        );
        assert_eq!(row("a", "A", 1).state_key(), "plugin-manager.on");
        assert_eq!(row("a", "A", 0).state_key(), "plugin-manager.off");
        // Needed by another enabled plugin, or this manager itself.
        assert_eq!(row("a", "A", 1 | 4).state_key(), "plugin-manager.required");
        assert_eq!(row(SELF_ID, "M", 1).state_key(), "plugin-manager.required");
        let failed = PluginRow {
            run: Run::Failed,
            ..row("a", "A", 1)
        };
        assert_eq!(failed.state_key(), "plugin-manager.error");
        // A failed plugin that is off is just off.
        let off = PluginRow {
            run: Run::Failed,
            ..row("a", "A", 0)
        };
        assert_eq!(off.state_key(), "plugin-manager.off");
        assert!(row("core", "Core", 1).locked());
        assert!(!row("a", "A", 1).locked());
    }

    #[test]
    fn the_host_comes_first_then_names_without_regard_to_case() {
        let mut rows = [
            row("b", "beta", 1),
            row("core", "PaperPaw", 1),
            row("a", "Alpha", 1),
            row("z", "alpine", 1),
        ];
        sort(&mut rows);
        let ids: Vec<&str> = rows.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, ["core", "a", "z", "b"]);
    }

    #[test]
    fn the_note_says_what_happens_then_the_description_and_version() {
        assert_eq!(
            note_of(&row("a", "A", 1), "Does a."),
            "Does a. plugin-manager.version[version=1.2.3]"
        );
        // Nothing is said of the host itself but its description.
        assert_eq!(
            note_of(&row("core", "Core", 1), ""),
            "plugin-manager.version[version=1.2.3]"
        );
        let waiting = row("a", "A", 1 | 2);
        assert!(note_of(&waiting, "d").starts_with("plugin-manager.starts-after-restart d"));
        let stopping = row("a", "A", 2);
        assert!(note_of(&stopping, "d").starts_with("plugin-manager.stops-after-restart d"));
        let update = PluginRow {
            update: "2.0.0".into(),
            ..row("a", "A", 1)
        };
        assert!(
            note_of(&update, "d")
                .starts_with("plugin-manager.updates-after-restart[version=2.0.0] d")
        );
        assert!(note_of(&row("a", "A", 1 | 4), "d").starts_with("plugin-manager.needed d"));
        assert!(note_of(&row(SELF_ID, "M", 1), "d").starts_with("plugin-manager.self d"));
        let code = PluginRow {
            kind: Kind::Code,
            ..row("a", "A", 1)
        };
        assert_eq!(
            note_of(&code, "d"),
            "d plugin-manager.code plugin-manager.version[version=1.2.3]"
        );
    }

    #[test]
    fn a_problem_is_said_and_safe_mode_off_has_its_own_words() {
        let held = PluginRow {
            problem: Some(Problem::Damaged),
            ..row("a", "A", 1)
        };
        assert!(note_of(&held, "d").starts_with("plugin-manager.code.damaged d"));
        let safe = PluginRow {
            problem: Some(Problem::SafeMode),
            ..row("a", "A", 0)
        };
        assert!(note_of(&safe, "d").starts_with("plugin-manager.code.safe-mode-off d"));
        let held_back = PluginRow {
            problem: Some(Problem::SafeMode),
            ..row("a", "A", 1)
        };
        assert!(note_of(&held_back, "d").starts_with("plugin-manager.code.safe-mode d"));
        let failed = PluginRow {
            run: Run::Failed,
            ..row("a", "A", 1)
        };
        assert!(note_of(&failed, "d").starts_with("plugin-manager.failed d"));
    }

    #[test]
    fn a_refused_format_or_reader_batch_says_the_cap() {
        let formats = PluginRow {
            run: Run::Failed,
            problem: Some(Problem::TooManyFormats),
            problem_arg: 32,
            ..row("a", "A", 1)
        };
        assert!(
            note_of(&formats, "d").starts_with("plugin-manager.formats.too-many[cap=32] d"),
            "{}",
            note_of(&formats, "d")
        );
        let readers = PluginRow {
            problem: Some(Problem::TooManyReaders),
            problem_arg: 16,
            ..formats.clone()
        };
        assert!(note_of(&readers, "d").starts_with("plugin-manager.readers.too-many[cap=16] d"));
        // Without a limit from the host the words would have a hole: the
        // plain "could not be loaded" is said.
        let bare = PluginRow {
            problem_arg: 0,
            ..formats
        };
        assert!(note_of(&bare, "d").starts_with("plugin-manager.failed d"));
    }

    fn name(id: &str) -> Result<String, Error> {
        concat(&["<", id, ">"])
    }

    #[test]
    fn safe_mode_says_why_in_a_few_words() {
        let say = |reason, crashed, last| {
            safe_mode_notice(&mut Keys, reason, crashed, last, name).unwrap()
        };
        assert_eq!(
            say(SafeMode::User, "x", "y"),
            "plugin-manager.safe-mode.key"
        );
        assert_eq!(
            say(SafeMode::Failures, "x", "y"),
            "plugin-manager.safe-mode.crashed[name=<x>]"
        );
        assert_eq!(
            say(SafeMode::Failures, "", "y"),
            "plugin-manager.safe-mode.last[name=<y>]"
        );
        assert_eq!(
            say(SafeMode::Failures, "", ""),
            "plugin-manager.safe-mode.failed"
        );
    }

    #[test]
    fn plugins_not_installed_are_named_with_the_reason() {
        let said =
            |list: &[(String, NotInstalled)]| not_installed_notice(&mut Keys, list, name).unwrap();
        assert_eq!(said(&[]), None);
        let a = (String::from("a"), NotInstalled::OtherAuthor);
        let b = (String::from("b"), NotInstalled::Damaged);
        assert_eq!(
            said(core::slice::from_ref(&a)).unwrap(),
            "plugin-manager.not-installed.other-author[name=<a>]"
        );
        assert_eq!(
            said(core::slice::from_ref(&b)).unwrap(),
            "plugin-manager.not-installed.damaged[name=<b>]"
        );
        assert_eq!(
            said(&[a, b]).unwrap(),
            "plugin-manager.not-installed.several[name=<a>, <b>]"
        );
    }

    #[test]
    fn the_placeholder_is_not_a_choice() {
        let offer = |screen: &str| Offer {
            screen: screen.into(),
            plugin: "p".into(),
        };
        assert_eq!(choosable(&[offer(PLACEHOLDER), offer("a.b")]), 1);
        assert_eq!(choosable(&[offer("a.b"), offer("c.d")]), 2);
        assert_eq!(
            line("core.home", "home.screen").unwrap(),
            "core.home home.screen"
        );
    }
}
