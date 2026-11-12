//! Declaration records for native plugins (interface 7): a builder that writes
//! the records `setting.group`, `setting.spec`, `setting.extra`,
//! `resource.hyphenation`, `resource.font_face`, `resource.font_extension`,
//! `screen.spec`, `action.spec`, menu and icon records (tags 5-18), and event/draw callback
//! registrations (tags 1-2) with the generated encoder, as the batches a plugin submits
//! through `registrations.submit` while it is made.
//!
//! A plugin creates [`Declarations`] with a submission sink, adds borrowed
//! typed items, then calls [`Declarations::submit`]. Items are encoded immediately
//! into one reusable buffer and sent in order, split into as few
//! batches as the wire allows (at most [`BATCH_MAX_RECORDS`] records and
//! [`BATCH_MAX_BYTES`] bytes each), every one encoded by the generated codec.
//! The builder checks what it can without a registry: a field longer than its
//! cap, an empty title or group, a kind and a default that do not fit, a choice
//! list the host would refuse. The host checks the rest (names, paths, whether
//! a group or a setting exists) when it stages the batch; the tests of
//! `paperpaw-core` run every batch built here through its validator, so the two
//! do not drift apart.
//!
//! Nothing here panics on any input: a refusal is a [`DeclareError`] naming the
//! field. A sink failure is returned as [`DeclareError::Sink`] with its unchanged
//! status and permanently poisons the builder. Every later addition or submission
//! returns that same error without calling the sink, even for invalid input.
//! Every local add refusal also latches permanently. The next `submit` sends
//! exactly one empty batch to abort host construction, discards pending records,
//! and returns the original refusal even if the sink returns a different error.
//! Later adds and submissions return that same refusal without calling the sink.
//! This includes allocation failure. Dropping never calls the sink; callers must
//! submit even after an ignored add error to communicate the abort to the host.

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;
use core::mem::MaybeUninit;

#[rustfmt::skip]
#[allow(
    clippy::all,
    clippy::pedantic,
    missing_docs,
    dead_code,
    reason = "generated declaration record codec (abi/generated/records_wire.rs, byte for byte)"
)]
pub(crate) mod records_wire;
#[rustfmt::skip]
#[allow(
    clippy::all,
    clippy::pedantic,
    missing_docs,
    dead_code,
    reason = "generated settings value codec (abi/generated/settings_value.rs, byte for byte)"
)]
pub(crate) mod settings_value;

#[cfg(feature = "kernel")]
pub use crate::settings::{fixed_groups_source, fixed_ids_source, fixed_settings_source};

use records_wire::{
    ActionSpec, ResourceFontExtension, ResourceFontFace, ResourceHyphenation, ScreenSpec,
    SettingExtra, SettingGroup, SettingSpec, WireEncode, records_encode_of,
};
use settings_value::{SettingValueRef, settings_encode};

mod format;
pub use format::FormatDecl;
mod reader;
pub use reader::ReaderDecl;
mod lookup;
pub use lookup::LookupProviderDecl;
mod library;
pub use library::{BookDataKind, BookFieldDecl};

mod stages;
pub use stages::{StageDecl, SubscribeDecl};

mod uix;
pub use uix::{
    HelpHintDecl, InputDecl, InputKind, StatusItemDecl, StatusLineDecl, StatusPlaceDecl,
    WidgetDecl, WidgetLayout, WidgetPlaceDecl,
};
mod registry;
pub use registry::{
    KeyBindingDecl, PresetDecl, PresetValueDecl, SetupPage, SetupStepDecl, SlotDecl, SlotOfferDecl,
};

mod callbacks;
pub use callbacks::EventKind;
mod menus;
pub use menus::{EntryPlacement, MenuEntryDecl, MenuSectionDecl};

/// Most records in one batch.
pub const BATCH_MAX_RECORDS: usize = records_wire::REGISTRATION_MAX_RECORDS;
// In-place appends replace and restore a single-byte batch-count prefix.
const _: () = assert!(BATCH_MAX_RECORDS < 128);
/// Most bytes in one encoded batch.
pub const BATCH_MAX_BYTES: usize = records_wire::REGISTRATION_MAX_BYTES;
/// Most values in a choice list (generated from `abi.def`).
pub const CHOICES_MAX_VALUES: usize = records_wire::SETTING_SPEC_CHOICES_MAX_ITEMS;
/// Longest choice value in bytes (generated from `abi.def`).
pub const CHOICE_MAX_BYTES: usize = records_wire::SETTING_SPEC_CHOICES_ITEM_MAX_BYTES;
/// Separator of the encoded choice list.
const CHOICE_SEPARATOR: char = records_wire::SETTING_SPEC_CHOICES_SEPARATOR;

/// The encoder's status for "does not fit".
const ERR_NO_SPACE: i32 = paperpaw_native::host_glue::ERR_NO_SPACE;

/// Setting flags of a `setting.spec` record.
const SPEC_SCOPED: u32 = 1;
const SPEC_GUARDED: u32 = 1 << 1;
const SPEC_KEPT_ON_RESET: u32 = 1 << 2;
const SPEC_SECRET: u32 = 1 << 3;
const SPEC_ADVANCED: u32 = 1 << 4;
/// Flags of a `setting.extra` record.
const EXTRA_SLIDER: u32 = 1;
const EXTRA_SECTION_OF_PLUGIN: u32 = 1 << 1;
const EXTRA_KEEPS_UNKNOWN: u32 = 1 << 2;

/// Why an item was not accepted. Nothing was added.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DeclareError {
    /// A field that must not be empty is (the title of a group or a setting, a
    /// setting's group, a language or a path).
    Empty(&'static str),
    /// A field is longer than the wire allows.
    TooLong(&'static str),
    /// The default does not fit the kind: its value, its range or its choices.
    Default,
    /// A range with `min` above `max`, or a number outside 32 bits.
    Range,
    /// A choice list with no value, an empty, too long or repeated one, or more
    /// than [`CHOICES_MAX_VALUES`].
    Choices,
    /// A secret setting that is also scoped (a secret has no scopes).
    SecretScoped,
    /// One item does not fit a batch by itself, or the encoder refused it with
    /// this status.
    Encode(i32),
    /// The submission sink refused a batch. This builder cannot submit again.
    Sink(i32),
    /// The builder could not allocate memory. No further submission is possible.
    NoMemory,
}

impl fmt::Display for DeclareError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DeclareError::Empty(field) => write!(f, "{field} must not be empty"),
            DeclareError::TooLong(field) => write!(f, "{field} is too long"),
            DeclareError::Default => f.write_str("the default does not fit the setting's kind"),
            DeclareError::Range => f.write_str("the range is empty or outside 32 bits"),
            DeclareError::Choices => f.write_str("the choices are not 1 to 32 distinct values"),
            DeclareError::SecretScoped => f.write_str("a secret setting cannot be scoped"),
            DeclareError::Encode(status) => write!(f, "the encoder refused it ({status})"),
            DeclareError::Sink(status) => write!(f, "the submission sink refused it ({status})"),
            DeclareError::NoMemory => f.write_str("not enough memory for declarations"),
        }
    }
}

/// The kind of a setting and what belongs to it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Kind<'a> {
    /// A switch, with its default.
    Bool(bool),
    /// A number in `min..=max`, with its default.
    Int {
        /// The default.
        default: i64,
        /// The smallest value.
        min: i32,
        /// The largest value.
        max: i32,
    },
    /// One of `choices` (translation keys), with the default one of them.
    Choice {
        /// The default, one of `choices`.
        default: &'a str,
        /// The values in their order.
        choices: &'a [&'a str],
    },
    /// Free text, with its default.
    Text(&'a str),
    /// A UI language tag, with its default (may be empty).
    Language(&'a str),
    /// Ids separated by commas, with their default.
    List(&'a str),
}

/// A `setting.group` record.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct GroupDecl<'a> {
    /// The group's id, relative to the plugin.
    pub name: &'a str,
    /// The translation key of its name; not empty.
    pub title: &'a str,
    /// By order, smallest first; 0 names none.
    pub order: i32,
    /// A screen of the plugin shown instead of the generated page, relative;
    /// empty for none.
    pub page: &'a str,
    /// The full id of the parent group; empty for none.
    pub parent: &'a str,
}

impl<'a> GroupDecl<'a> {
    /// A group `name` (relative to the plugin) called `title` (a translation key).
    pub fn new(name: &'a str, title: &'a str) -> Self {
        Self {
            name,
            title,
            order: 0,
            page: "",
            parent: "",
        }
    }
}

/// A `setting.spec` record.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SpecDecl<'a> {
    /// The setting's id, relative to the plugin.
    pub name: &'a str,
    /// Its kind and default.
    pub kind: Kind<'a>,
    /// The full id of its group; not empty.
    pub group: &'a str,
    /// The translation key of its name; not empty.
    pub title: &'a str,
    /// The translation key of its help; empty for none.
    pub help: &'a str,
    /// May differ per scope, such as per book.
    pub scoped: bool,
    /// Changed by its own plugin and the settings editor alone.
    pub guarded: bool,
    /// "Reset to defaults" leaves it as it is.
    pub kept_on_reset: bool,
    /// Kept with the secrets, not the settings.
    pub secret: bool,
    /// Shown when the user asks for advanced settings.
    pub advanced: bool,
}

impl<'a> SpecDecl<'a> {
    /// A setting `name` (relative to the plugin) of `kind`, in `group` (a full
    /// id), called `title` (a translation key); the rest as set with the fields.
    pub fn new(name: &'a str, kind: Kind<'a>, group: &'a str, title: &'a str) -> Self {
        Self {
            name,
            kind,
            group,
            title,
            help: "",
            scoped: false,
            guarded: false,
            kept_on_reset: false,
            secret: false,
            advanced: false,
        }
    }
}

/// A `setting.extra` record: what few settings have, for the setting `name`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ExtraDecl<'a> {
    /// The setting's relative name.
    pub name: &'a str,
    /// The translation key of its unit; empty for none.
    pub unit: &'a str,
    /// The translation key of its part of the group; empty for none.
    pub section: &'a str,
    /// The full id of the setting whose value it has until set; empty for none.
    pub follows: &'a str,
    /// The setting it is shown for, and the value, as written in `paperpaw.ini`.
    pub shown_if: Option<(&'a str, &'a str)>,
    /// Where it is edited.
    pub editor: Option<EditorDecl<'a>>,
    /// How much one step changes a number. 0 and 1 both mean one step and are
    /// written to the wire as 0.
    pub step: i32,
    /// Shown as a slider on touch screens.
    pub slider: bool,
    /// Its part is the name of the plugin its id ends in.
    pub section_of_plugin: bool,
    /// A choice that keeps values its options lack.
    pub keeps_unknown: bool,
}

/// Where a setting is edited instead of on the settings page.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum EditorDecl<'a> {
    /// A screen of the plugin, relative to it.
    Screen(&'a str),
    /// Whatever screen fills the slot (a full id).
    Slot(&'a str),
}

impl<'a> ExtraDecl<'a> {
    /// The extra of setting `name` (relative to the plugin), with nothing set.
    pub fn new(name: &'a str) -> Self {
        Self {
            name,
            unit: "",
            section: "",
            follows: "",
            shown_if: None,
            editor: None,
            step: 1,
            slider: false,
            section_of_plugin: false,
            keeps_unknown: false,
        }
    }
}

/// A `resource.font_face` record.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct FontFaceDecl<'a> {
    /// The face's id, relative to the plugin.
    pub name: &'a str,
    /// Its file, relative to the plugin's folder.
    pub path: &'a str,
    /// The family name; empty for a face outside any family (bound by
    /// `role`, never offered for text).
    pub family: &'a str,
    /// Bit 0 bold, bit 1 italic; others are refused by the host.
    pub style: u32,
    /// 1 to 1000, or 0 for the file's (and always 0 for a `.ppfp` pack).
    pub weight: u32,
    /// The role it is bound to (a full id); empty for none.
    pub role: &'a str,
    /// Ranks a role binding, -100 to 100; 0 without a role.
    pub priority: i32,
}

impl<'a> FontFaceDecl<'a> {
    /// A face `name` (relative to the plugin) from the file `path`; the rest as
    /// set with the fields.
    pub fn new(name: &'a str, path: &'a str) -> Self {
        Self {
            name,
            path,
            family: "",
            style: 0,
            weight: 0,
            role: "",
            priority: 0,
        }
    }
}

/// A `resource.font_extension` record: glyphs a `.ppfp` pack adds to a face.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct FontExtensionDecl<'a> {
    /// The extension's id, relative to the plugin.
    pub name: &'a str,
    /// Its `.ppfp` file.
    pub path: &'a str,
    /// The full id of the face it extends.
    pub extends: &'a str,
}

impl<'a> FontExtensionDecl<'a> {
    /// An extension `name` (relative to the plugin) from the `.ppfp` file
    /// `path`, adding glyphs to the face `extends` (a full id).
    pub fn new(name: &'a str, path: &'a str, extends: &'a str) -> Self {
        Self {
            name,
            path,
            extends,
        }
    }
}

/// How repeated opens of a native screen create or reuse instances.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Instances {
    /// Reuse the instance with equal parameters.
    #[default]
    PerParams,
    /// Reuse any open instance of the screen.
    Single,
    /// Create a new instance for every open.
    Many,
}

/// A `screen.spec` declaration with a plugin-owned callback token.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ScreenDecl<'a> {
    /// The screen's id, relative to the plugin.
    pub name: &'a str,
    /// The translation key of its title.
    pub title: &'a str,
    /// Token passed to the screen's callbacks.
    pub token: u32,
    /// Request a full refresh when the screen opens.
    pub full_refresh_on_open: bool,
    policy_flags: u32,
}

impl<'a> ScreenDecl<'a> {
    /// A screen with no refresh flag set.
    pub fn new(name: &'a str, title: &'a str, token: u32) -> Self {
        Self {
            name,
            title,
            token,
            full_refresh_on_open: false,
            policy_flags: 0,
        }
    }
}

impl ScreenDecl<'_> {
    /// Choose how ordinary opens create or reuse this screen's instances.
    #[must_use]
    pub const fn instances(mut self, instances: Instances) -> Self {
        let bits = match instances {
            Instances::PerParams => 0,
            Instances::Single => 1,
            Instances::Many => 2,
        };
        self.policy_flags = (self.policy_flags & !0xc0) | (bits << 6);
        self
    }

    /// Guard the device from this screen's opening.
    #[must_use]
    pub const fn guards(mut self) -> Self {
        self.policy_flags |= 2;
        self
    }
    /// Draw over the screen below, subject to host privacy checks.
    #[must_use]
    pub const fn overlay(mut self) -> Self {
        self.policy_flags |= 4;
        self
    }
    /// Wait for screen_ready before offering help.
    #[must_use]
    pub const fn waits_ready(mut self) -> Self {
        self.policy_flags |= 8;
        self
    }
    /// Keep this screen's contents out from under another plugin's overlay.
    #[must_use]
    pub const fn secret(mut self) -> Self {
        self.policy_flags |= 16;
        self
    }
    /// Hold edge gestures until the screen releases them.
    #[must_use]
    pub const fn holds_edges(mut self) -> Self {
        self.policy_flags |= 32;
        self
    }
    /// Show no row of button hints with this screen (the sleep screen).
    #[must_use]
    pub const fn hints_hidden(mut self) -> Self {
        self.policy_flags |= 256;
        self
    }
}

/// An `action.spec` declaration with a plugin-owned callback token.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ActionDecl<'a> {
    /// The action's id, relative to the plugin.
    pub name: &'a str,
    /// The translation key of its title.
    pub title: &'a str,
    /// Token passed to the action's event callback.
    pub token: u32,
    /// Run only while one of this plugin's screens is on top.
    pub needs_screen: bool,
}

impl<'a> ActionDecl<'a> {
    /// An action which can run without a screen.
    pub fn new(name: &'a str, title: &'a str, token: u32) -> Self {
        Self {
            name,
            title,
            token,
            needs_screen: false,
        }
    }
}

enum Item<'a> {
    Registry(&'a dyn records_wire::WireEncode),
    MenuEntry(records_wire::MenuEntry<'a>),
    Icon(records_wire::IconSpec<'a>),
    MenuNeed(records_wire::MenuNeed<'a>),
    MenuSection(records_wire::MenuSection<'a>),
    MenuHide(records_wire::MenuHide<'a>),
    MenuDescribe(records_wire::MenuDescribe<'a>),
    Registration(records_wire::Registration),
    Group(GroupDecl<'a>),
    Spec(SpecDecl<'a>, SpecParts),
    Extra(ExtraDecl<'a>),
    Hyphenation { language: &'a str, path: &'a str },
    FontFace(FontFaceDecl<'a>),
    FontExtension(FontExtensionDecl<'a>),
    Screen(ScreenDecl<'a>),
    Action(ActionDecl<'a>),
    Stage(StageDecl<'a>),
    Subscribe(SubscribeDecl<'a>),
}

/// The items a plugin declares, in the order it declares them.
#[must_use = "call submit() to send the final batch; dropping does not submit"]
pub struct Declarations<F> {
    sink: F,
    bytes: Vec<u8>,
    used: usize,
    count: usize,
    total: usize,
    failed: Option<Latched>,
    abort_sent: bool,
}

#[derive(Debug, Clone, Copy)]
enum Latched {
    Sink(i32),
    NoMemory,
    Refused(DeclareError),
}

impl Latched {
    fn error(self) -> DeclareError {
        match self {
            Self::Sink(status) => DeclareError::Sink(status),
            Self::NoMemory => DeclareError::NoMemory,
            Self::Refused(error) => error,
        }
    }
}

impl<F> Drop for Declarations<F> {
    fn drop(&mut self) {
        debug_assert!(
            self.count == 0 || self.failed.is_some(),
            "declarations dropped without submit()"
        );
    }
}

impl<F> fmt::Debug for Declarations<F> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Declarations")
            .field("pending", &self.count)
            .field("total", &self.total)
            .field("failed", &self.failed)
            .finish_non_exhaustive()
    }
}

fn within(text: &str, cap: usize, field: &'static str) -> Result<(), DeclareError> {
    if text.len() > cap {
        Err(DeclareError::TooLong(field))
    } else {
        Ok(())
    }
}

fn filled(text: &str, field: &'static str) -> Result<(), DeclareError> {
    if text.is_empty() {
        Err(DeclareError::Empty(field))
    } else {
        Ok(())
    }
}

/// The wire value of a default: one `settings_value`, header included.
fn value_bytes(value: SettingValueRef<'_>) -> Result<Vec<u8>, DeclareError> {
    let mut out = [MaybeUninit::<u8>::uninit(); records_wire::SETTING_SPEC_DEFAULT_MAX_BYTES];
    let len = settings_encode(value, &mut out).map_err(|_| DeclareError::Default)?;
    let bytes = out.get(..len).ok_or(DeclareError::TooLong("default"))?;
    let mut encoded = Vec::new();
    encoded
        .try_reserve_exact(len)
        .map_err(|_| DeclareError::NoMemory)?;
    // SAFETY: `settings_encode` initialized the first `len` bytes of `out`.
    encoded.extend(bytes.iter().map(|byte| unsafe { byte.assume_init() }));
    Ok(encoded)
}

fn choices_text(choices: &[&str], default: &str) -> Result<String, DeclareError> {
    let ok = !choices.is_empty()
        && choices.len() <= CHOICES_MAX_VALUES
        && choices
            .iter()
            .all(|v| !v.is_empty() && v.len() <= CHOICE_MAX_BYTES && !v.contains(CHOICE_SEPARATOR))
        && choices
            .iter()
            .enumerate()
            .all(|(i, v)| !choices.iter().take(i).any(|earlier| earlier == v));
    if !ok {
        return Err(DeclareError::Choices);
    }
    if !choices.contains(&default) {
        return Err(DeclareError::Default);
    }
    let length = choices.iter().map(|choice| choice.len()).sum::<usize>() + choices.len() - 1;
    if length > records_wire::SETTING_SPEC_CHOICES_MAX_BYTES {
        return Err(DeclareError::TooLong("choices"));
    }
    let mut joined = String::new();
    joined
        .try_reserve_exact(length)
        .map_err(|_| DeclareError::NoMemory)?;
    for (index, choice) in choices.iter().enumerate() {
        if index != 0 {
            joined.push(CHOICE_SEPARATOR);
        }
        joined.push_str(choice);
    }
    Ok(joined)
}

impl<'a> SpecDecl<'a> {
    /// The record's scalars and the default as wire bytes.
    fn parts(&self) -> Result<SpecParts, DeclareError> {
        let (default, min, max, choices) = match &self.kind {
            Kind::Bool(v) => (value_bytes(SettingValueRef::Bool(*v))?, 0, 0, String::new()),
            Kind::Int { default, min, max } => {
                if min > max {
                    return Err(DeclareError::Range);
                }
                if !(i64::from(*min)..=i64::from(*max)).contains(default) {
                    return Err(DeclareError::Default);
                }
                (
                    value_bytes(SettingValueRef::Int(*default))?,
                    *min,
                    *max,
                    String::new(),
                )
            }
            Kind::Choice { default, choices } => (
                value_bytes(SettingValueRef::Text(default))?,
                0,
                0,
                choices_text(choices, default)?,
            ),
            Kind::Text(v) | Kind::Language(v) | Kind::List(v) => {
                (value_bytes(SettingValueRef::Text(v))?, 0, 0, String::new())
            }
        };
        let kind = match &self.kind {
            Kind::Bool(_) => 0,
            Kind::Int { .. } => 1,
            Kind::Choice { .. } => 2,
            Kind::Text(_) => 3,
            Kind::Language(_) => 4,
            Kind::List(_) => 5,
        };
        let flags = spec_flags(self);
        Ok(SpecParts {
            kind,
            flags,
            min,
            max,
            choices,
            default,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SpecParts {
    kind: u32,
    flags: u32,
    min: i32,
    max: i32,
    choices: String,
    default: Vec<u8>,
}

impl<F: FnMut(&[u8]) -> Result<(), i32>> Declarations<F> {
    /// Starts an empty stream with one reusable 4 KiB buffer. Declaration strings
    /// are borrowed only while adding each item; no declaration is retained.
    /// Call [`Self::submit`] to send the final batch. Dropping does not submit.
    /// If allocation fails, every addition and submission returns
    /// [`DeclareError::NoMemory`]. The sink then sees only the one empty abort
    /// batch of the next [`Self::submit`], which poisons the host construction.
    pub fn new(sink: F) -> Self {
        let mut bytes = Vec::new();
        let failed = match bytes.try_reserve_exact(BATCH_MAX_BYTES) {
            Ok(()) => {
                bytes.resize(BATCH_MAX_BYTES, 0);
                None
            }
            Err(_) => Some(Latched::NoMemory),
        };
        Self {
            sink,
            bytes,
            used: 1,
            count: 0,
            total: 0,
            failed,
            abort_sent: false,
        }
    }

    /// How many items there are.
    pub fn len(&self) -> usize {
        self.total
    }

    /// Whether there are none.
    pub fn is_empty(&self) -> bool {
        self.total == 0
    }

    /// Adds a `setting.group`.
    ///
    /// # Errors
    ///
    /// An empty title, or a field over its cap.
    pub fn group(&mut self, group: GroupDecl<'_>) -> Result<&mut Self, DeclareError> {
        self.check_sink()?;
        let result = (|| {
            within(
                group.name,
                records_wire::SETTING_GROUP_NAME_MAX_BYTES,
                "group name",
            )?;
            filled(group.title, "group title")?;
            within(
                group.title,
                records_wire::SETTING_GROUP_TITLE_MAX_BYTES,
                "group title",
            )?;
            within(
                group.page,
                records_wire::SETTING_GROUP_PAGE_MAX_BYTES,
                "group page",
            )?;
            within(
                group.parent,
                records_wire::SETTING_GROUP_PARENT_MAX_BYTES,
                "group parent",
            )?;
            self.push(Item::Group(group))?;
            Ok(())
        })();
        if let Err(error) = result {
            if self.failed.is_none() {
                self.failed = Some(Latched::Refused(error));
            }
            return Err(error);
        }
        Ok(self)
    }

    /// Adds a `setting.spec`.
    ///
    /// # Errors
    ///
    /// An empty title or group, a field over its cap, a default that does not
    /// fit the kind, a bad range or choice list, or a secret that is scoped.
    pub fn spec(&mut self, spec: SpecDecl<'_>) -> Result<&mut Self, DeclareError> {
        self.check_sink()?;
        let result = (|| {
            within(
                spec.name,
                records_wire::SETTING_SPEC_NAME_MAX_BYTES,
                "setting name",
            )?;
            filled(spec.group, "setting group")?;
            within(
                spec.group,
                records_wire::SETTING_SPEC_GROUP_MAX_BYTES,
                "setting group",
            )?;
            filled(spec.title, "setting title")?;
            within(
                spec.title,
                records_wire::SETTING_SPEC_TITLE_MAX_BYTES,
                "setting title",
            )?;
            within(
                spec.help,
                records_wire::SETTING_SPEC_HELP_MAX_BYTES,
                "setting help",
            )?;
            if spec.secret && spec.scoped {
                return Err(DeclareError::SecretScoped);
            }
            let parts = spec.parts().inspect_err(|error| {
                if *error == DeclareError::NoMemory {
                    self.failed = Some(Latched::NoMemory);
                }
            })?;
            within(
                &parts.choices,
                records_wire::SETTING_SPEC_CHOICES_MAX_BYTES,
                "choices",
            )?;
            if parts.default.len() > records_wire::SETTING_SPEC_DEFAULT_MAX_BYTES {
                return Err(DeclareError::TooLong("default"));
            }
            self.push(Item::Spec(spec, parts))?;
            Ok(())
        })();
        if let Err(error) = result {
            if self.failed.is_none() {
                self.failed = Some(Latched::Refused(error));
            }
            return Err(error);
        }
        Ok(self)
    }

    /// Adds a `setting.extra`.
    ///
    /// # Errors
    ///
    /// A field over its cap, a negative step, or `shown_if` with an empty id.
    pub fn extra(&mut self, extra: ExtraDecl<'_>) -> Result<&mut Self, DeclareError> {
        self.check_sink()?;
        let result = (|| {
            within(
                extra.name,
                records_wire::SETTING_EXTRA_NAME_MAX_BYTES,
                "extra name",
            )?;
            within(
                extra.unit,
                records_wire::SETTING_EXTRA_UNIT_MAX_BYTES,
                "unit",
            )?;
            within(
                extra.section,
                records_wire::SETTING_EXTRA_SECTION_MAX_BYTES,
                "section",
            )?;
            within(
                extra.follows,
                records_wire::SETTING_EXTRA_FOLLOWS_MAX_BYTES,
                "follows",
            )?;
            if let Some((id, value)) = &extra.shown_if {
                filled(id, "shown_if id")?;
                within(
                    id,
                    records_wire::SETTING_EXTRA_SHOWN_ID_MAX_BYTES,
                    "shown_if id",
                )?;
                within(
                    value,
                    records_wire::SETTING_EXTRA_SHOWN_VALUE_MAX_BYTES,
                    "shown_if value",
                )?;
            }
            if let Some(EditorDecl::Screen(id) | EditorDecl::Slot(id)) = &extra.editor {
                filled(id, "editor")?;
                within(id, records_wire::SETTING_EXTRA_EDITOR_MAX_BYTES, "editor")?;
            }
            if extra.step < 0 {
                return Err(DeclareError::Range);
            }
            self.push(Item::Extra(extra))?;
            Ok(())
        })();
        if let Err(error) = result {
            if self.failed.is_none() {
                self.failed = Some(Latched::Refused(error));
            }
            return Err(error);
        }
        Ok(self)
    }

    /// Adds a `resource.hyphenation`: the table of `path` for `language`.
    ///
    /// # Errors
    ///
    /// An empty language or path, or one over its cap.
    pub fn hyphenation(&mut self, language: &str, path: &str) -> Result<&mut Self, DeclareError> {
        self.check_sink()?;
        let result = (|| {
            filled(language, "language")?;
            filled(path, "path")?;
            within(
                language,
                records_wire::RESOURCE_HYPHENATION_LANGUAGE_MAX_BYTES,
                "language",
            )?;
            within(
                path,
                records_wire::RESOURCE_HYPHENATION_PATH_MAX_BYTES,
                "path",
            )?;
            self.push(Item::Hyphenation { language, path })?;
            Ok(())
        })();
        if let Err(error) = result {
            if self.failed.is_none() {
                self.failed = Some(Latched::Refused(error));
            }
            return Err(error);
        }
        Ok(self)
    }

    /// Adds a `resource.font_face`.
    ///
    /// # Errors
    ///
    /// An empty name or path, or a field over its cap.
    pub fn font_face(&mut self, face: FontFaceDecl<'_>) -> Result<&mut Self, DeclareError> {
        self.check_sink()?;
        let result = (|| {
            filled(face.name, "face name")?;
            filled(face.path, "path")?;
            within(
                face.name,
                records_wire::RESOURCE_FONT_FACE_NAME_MAX_BYTES,
                "face name",
            )?;
            within(
                face.path,
                records_wire::RESOURCE_FONT_FACE_PATH_MAX_BYTES,
                "path",
            )?;
            within(
                face.family,
                records_wire::RESOURCE_FONT_FACE_FAMILY_MAX_BYTES,
                "family",
            )?;
            within(
                face.role,
                records_wire::RESOURCE_FONT_FACE_ROLE_MAX_BYTES,
                "role",
            )?;
            self.push(Item::FontFace(face))?;
            Ok(())
        })();
        if let Err(error) = result {
            if self.failed.is_none() {
                self.failed = Some(Latched::Refused(error));
            }
            return Err(error);
        }
        Ok(self)
    }

    /// Adds a `resource.font_extension`.
    ///
    /// # Errors
    ///
    /// An empty name, path or `extends`, or a field over its cap.
    pub fn font_extension(
        &mut self,
        extension: FontExtensionDecl<'_>,
    ) -> Result<&mut Self, DeclareError> {
        self.check_sink()?;
        let result = (|| {
            filled(extension.name, "extension name")?;
            filled(extension.path, "path")?;
            filled(extension.extends, "extends")?;
            within(
                extension.name,
                records_wire::RESOURCE_FONT_EXTENSION_NAME_MAX_BYTES,
                "extension name",
            )?;
            within(
                extension.path,
                records_wire::RESOURCE_FONT_EXTENSION_PATH_MAX_BYTES,
                "path",
            )?;
            within(
                extension.extends,
                records_wire::RESOURCE_FONT_EXTENSION_EXTENDS_MAX_BYTES,
                "extends",
            )?;
            self.push(Item::FontExtension(extension))?;
            Ok(())
        })();
        if let Err(error) = result {
            if self.failed.is_none() {
                self.failed = Some(Latched::Refused(error));
            }
            return Err(error);
        }
        Ok(self)
    }

    /// Adds a `screen.spec` declaration.
    ///
    /// # Errors
    /// An empty name or title, a field over its generated cap, or a sink refusal.
    pub fn screen(&mut self, screen: ScreenDecl<'_>) -> Result<&mut Self, DeclareError> {
        self.check_sink()?;
        let result = (|| {
            filled(screen.name, "screen name")?;
            filled(screen.title, "screen title")?;
            within(
                screen.name,
                records_wire::SCREEN_SPEC_NAME_MAX_BYTES,
                "screen name",
            )?;
            within(
                screen.title,
                records_wire::SCREEN_SPEC_TITLE_MAX_BYTES,
                "screen title",
            )?;
            self.push(Item::Screen(screen))
        })();
        if let Err(error) = result {
            if self.failed.is_none() {
                self.failed = Some(Latched::Refused(error));
            }
            return Err(error);
        }
        Ok(self)
    }

    /// Adds an `action.spec` declaration.
    ///
    /// # Errors
    /// An empty name or title, a field over its generated cap, or a sink refusal.
    pub fn action(&mut self, action: ActionDecl<'_>) -> Result<&mut Self, DeclareError> {
        self.check_sink()?;
        let result = (|| {
            filled(action.name, "action name")?;
            filled(action.title, "action title")?;
            within(
                action.name,
                records_wire::ACTION_SPEC_NAME_MAX_BYTES,
                "action name",
            )?;
            within(
                action.title,
                records_wire::ACTION_SPEC_TITLE_MAX_BYTES,
                "action title",
            )?;
            self.push(Item::Action(action))
        })();
        if let Err(error) = result {
            if self.failed.is_none() {
                self.failed = Some(Latched::Refused(error));
            }
            return Err(error);
        }
        Ok(self)
    }

    /// Submits the pending batch. Earlier full batches were submitted as items
    /// were added. The sink borrows each batch only for the duration of its call.
    ///
    /// # Errors
    ///
    /// A sink refusal is returned and latched: no further batch is submitted.
    /// A local refusal sends one empty abort batch and returns the original error.
    pub fn submit(&mut self) -> Result<(), DeclareError> {
        if let Some(failure) = self.failed {
            if !matches!(failure, Latched::Sink(_)) && !self.abort_sent {
                self.abort_sent = true;
                let _ = (self.sink)(&[]);
            }
            return Err(failure.error());
        }
        if self.count == 0 {
            return Ok(());
        }
        if let Err(status) = (self.sink)(&self.bytes[..self.used]) {
            self.failed = Some(Latched::Sink(status));
            return Err(DeclareError::Sink(status));
        }
        self.used = 1;
        self.count = 0;
        Ok(())
    }

    fn check_sink(&self) -> Result<(), DeclareError> {
        self.failed.map_or(Ok(()), |failure| Err(failure.error()))
    }

    fn push(&mut self, item: Item<'_>) -> Result<(), DeclareError> {
        self.check_sink()?;
        if self.count == BATCH_MAX_RECORDS {
            self.submit()?;
        }
        let previous = self.bytes[self.used - 1];
        let result = item.encode(&mut self.bytes[self.used - 1..]);
        let length = match result {
            Ok(length) => {
                self.bytes[self.used - 1] = previous;
                length
            }
            Err(ERR_NO_SPACE) if self.count > 0 => {
                self.submit()?;
                item.encode(&mut self.bytes).map_err(DeclareError::Encode)?
            }
            Err(status) => return Err(DeclareError::Encode(status)),
        };
        self.used += length - 1;
        self.count += 1;
        self.bytes[0] = self.count as u8;
        self.total += 1;
        Ok(())
    }
}

impl<'a> Item<'a> {
    /// Encodes the item as a one-record batch, by its own record type: a plugin's
    /// declarations link only the codecs of the declarations it makes.
    fn encode(&self, out: &mut [u8]) -> Result<usize, i32> {
        let one = |record: &dyn WireEncode, out: &mut [u8]| records_encode_of(&[record], out);
        match self {
            Item::Registry(record) => one(*record, out),
            Item::MenuEntry(entry) => one(entry, out),
            Item::Icon(icon) => one(icon, out),
            Item::MenuNeed(need) => one(need, out),
            Item::MenuSection(section) => one(section, out),
            Item::MenuHide(hide) => one(hide, out),
            Item::MenuDescribe(describe) => one(describe, out),
            Item::Registration(registration) => one(registration, out),
            Item::Group(g) => one(
                &SettingGroup {
                    name: g.name,
                    title: g.title,
                    order: g.order,
                    page: g.page,
                    parent: g.parent,
                },
                out,
            ),
            Item::Spec(s, parts) => one(&spec_record(s, parts), out),
            Item::Extra(e) => one(&extra_record(e), out),
            Item::Hyphenation { language, path } => {
                one(&ResourceHyphenation { language, path }, out)
            }
            Item::FontFace(f) => one(
                &ResourceFontFace {
                    name: f.name,
                    path: f.path,
                    family: f.family,
                    style: f.style,
                    weight: f.weight,
                    role: f.role,
                    priority: f.priority,
                },
                out,
            ),
            Item::FontExtension(e) => one(
                &ResourceFontExtension {
                    name: e.name,
                    path: e.path,
                    extends: e.extends,
                },
                out,
            ),
            Item::Screen(s) => one(
                &ScreenSpec {
                    name: s.name,
                    title: s.title,
                    token: s.token,
                    flags: u32::from(s.full_refresh_on_open) | s.policy_flags,
                },
                out,
            ),
            Item::Stage(stage) => one(&stage.record(), out),
            Item::Subscribe(to) => one(&to.record(), out),
            Item::Action(a) => one(
                &ActionSpec {
                    name: a.name,
                    title: a.title,
                    token: a.token,
                    flags: u32::from(a.needs_screen),
                },
                out,
            ),
        }
    }
}

fn spec_record<'a>(s: &'a SpecDecl<'a>, parts: &'a SpecParts) -> SettingSpec<'a> {
    SettingSpec {
        name: s.name,
        kind: parts.kind,
        flags: parts.flags,
        group: s.group,
        title: s.title,
        help: s.help,
        default: &parts.default,
        min: parts.min,
        max: parts.max,
        choices: &parts.choices,
    }
}

fn spec_flags(s: &SpecDecl<'_>) -> u32 {
    [
        (s.scoped, SPEC_SCOPED),
        (s.guarded, SPEC_GUARDED),
        (s.kept_on_reset, SPEC_KEPT_ON_RESET),
        (s.secret, SPEC_SECRET),
        (s.advanced, SPEC_ADVANCED),
    ]
    .iter()
    .filter(|(on, _)| *on)
    .fold(0, |flags, (_, bit)| flags | bit)
}

fn extra_record<'a>(e: &ExtraDecl<'a>) -> SettingExtra<'a> {
    let (shown_id, shown_value) = e
        .shown_if
        .as_ref()
        .map_or(("", ""), |(id, value)| (*id, *value));
    let (editor_kind, editor) = match &e.editor {
        None => (0, ""),
        Some(EditorDecl::Screen(id)) => (1, *id),
        Some(EditorDecl::Slot(id)) => (2, *id),
    };
    let flags = [
        (e.slider, EXTRA_SLIDER),
        (e.section_of_plugin, EXTRA_SECTION_OF_PLUGIN),
        (e.keeps_unknown, EXTRA_KEEPS_UNKNOWN),
    ]
    .iter()
    .filter(|(on, _)| *on)
    .fold(0, |flags, (_, bit)| flags | bit);
    SettingExtra {
        name: e.name,
        unit: e.unit,
        section: e.section,
        follows: e.follows,
        shown_id,
        shown_value,
        editor_kind,
        editor,
        // The wire writes the default step as 0.
        step: if e.step == 1 { 0 } else { e.step },
        flags,
        check: 0,
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod typed_wire_tests;

#[cfg(test)]
mod uix_tests;
