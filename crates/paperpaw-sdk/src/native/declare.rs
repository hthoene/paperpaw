//! Declaration records for native plugins (interface 7): a builder that writes
//! the records `setting.group`, `setting.spec`, `setting.extra`,
//! `resource.hyphenation`, `resource.font_face` and `resource.font_extension`
//! (tags 5-10) with the generated encoder, as the batches a plugin submits
//! through `registrations.submit` while it is made.
//!
//! A plugin fills a [`Declarations`] with typed items and calls
//! [`Declarations::batches`]: the items come out in order, split into as few
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
//! field.

use alloc::borrow::ToOwned;
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
mod records_wire;
#[rustfmt::skip]
#[allow(
    clippy::all,
    clippy::pedantic,
    missing_docs,
    dead_code,
    reason = "generated settings value codec (abi/generated/settings_value.rs, byte for byte)"
)]
mod settings_value;

use records_wire::{
    Record, ResourceFontExtension, ResourceFontFace, ResourceHyphenation, SettingExtra,
    SettingGroup, SettingSpec, records_encode,
};
use settings_value::{SETTINGS_MAX_BYTES, SettingValueRef, settings_encode};

#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
mod interface6;

/// Most records in one batch.
pub const BATCH_MAX_RECORDS: usize = records_wire::REGISTRATION_MAX_RECORDS;
/// Most bytes in one encoded batch.
pub const BATCH_MAX_BYTES: usize = records_wire::REGISTRATION_MAX_BYTES;
/// Most values in a choice list.
pub const CHOICES_MAX_VALUES: usize = 32;
/// Longest choice value in bytes.
pub const CHOICE_MAX_BYTES: usize = 96;

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
    /// A setting that cannot be written as a record: it is of a kind the wire
    /// does not know, it has a value check (a callback), or its id is not in the
    /// plugin's own namespace.
    Unsupported(&'static str),
    /// One item does not fit a batch by itself, or the encoder refused it with
    /// this status.
    Encode(i32),
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
            DeclareError::Unsupported(what) => write!(f, "cannot be declared: {what}"),
            DeclareError::Encode(status) => write!(f, "the encoder refused it ({status})"),
        }
    }
}

/// The kind of a setting and what belongs to it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Kind {
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
        default: String,
        /// The values in their order.
        choices: Vec<String>,
    },
    /// Free text, with its default.
    Text(String),
    /// A UI language tag, with its default (may be empty).
    Language(String),
    /// Ids separated by commas, with their default.
    List(String),
}

/// A `setting.group` record.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct GroupDecl {
    /// The group's id, relative to the plugin.
    pub name: String,
    /// The translation key of its name; not empty.
    pub title: String,
    /// By order, smallest first; 0 names none.
    pub order: i32,
    /// A screen of the plugin shown instead of the generated page, relative;
    /// empty for none.
    pub page: String,
    /// The full id of the parent group; empty for none.
    pub parent: String,
}

impl GroupDecl {
    /// A group `name` (relative to the plugin) called `title` (a translation key).
    pub fn new(name: &str, title: &str) -> Self {
        Self {
            name: name.to_owned(),
            title: title.to_owned(),
            order: 0,
            page: String::new(),
            parent: String::new(),
        }
    }
}

/// A `setting.spec` record.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SpecDecl {
    /// The setting's id, relative to the plugin.
    pub name: String,
    /// Its kind and default.
    pub kind: Kind,
    /// The full id of its group; not empty.
    pub group: String,
    /// The translation key of its name; not empty.
    pub title: String,
    /// The translation key of its help; empty for none.
    pub help: String,
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

impl SpecDecl {
    /// A setting `name` (relative to the plugin) of `kind`, in `group` (a full
    /// id), called `title` (a translation key); the rest as set with the fields.
    pub fn new(name: &str, kind: Kind, group: &str, title: &str) -> Self {
        Self {
            name: name.to_owned(),
            kind,
            group: group.to_owned(),
            title: title.to_owned(),
            help: String::new(),
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
pub struct ExtraDecl {
    /// The setting's relative name.
    pub name: String,
    /// The translation key of its unit; empty for none.
    pub unit: String,
    /// The translation key of its part of the group; empty for none.
    pub section: String,
    /// The full id of the setting whose value it has until set; empty for none.
    pub follows: String,
    /// The setting it is shown for, and the value, as written in `paperpaw.ini`.
    pub shown_if: Option<(String, String)>,
    /// Where it is edited.
    pub editor: Option<EditorDecl>,
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
pub enum EditorDecl {
    /// A screen of the plugin, relative to it.
    Screen(String),
    /// Whatever screen fills the slot (a full id).
    Slot(String),
}

impl ExtraDecl {
    /// The extra of setting `name` (relative to the plugin), with nothing set.
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            unit: String::new(),
            section: String::new(),
            follows: String::new(),
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
pub struct FontFaceDecl {
    /// The face's id, relative to the plugin.
    pub name: String,
    /// Its file, relative to the plugin's folder.
    pub path: String,
    /// The family name; empty to take the file's.
    pub family: String,
    /// Bit 0 italic, bit 1 oblique; others are refused by the host.
    pub style: u32,
    /// 1 to 1000, or 0 for the file's (and always 0 for a `.ppfp` pack).
    pub weight: u32,
    /// The role it is bound to (a full id); empty for none.
    pub role: String,
    /// Ranks a role binding, -100 to 100; 0 without a role.
    pub priority: i32,
}

impl FontFaceDecl {
    /// A face `name` (relative to the plugin) from the file `path`; the rest as
    /// set with the fields.
    pub fn new(name: &str, path: &str) -> Self {
        Self {
            name: name.to_owned(),
            path: path.to_owned(),
            family: String::new(),
            style: 0,
            weight: 0,
            role: String::new(),
            priority: 0,
        }
    }
}

/// A `resource.font_extension` record: glyphs a `.ppfp` pack adds to a face.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct FontExtensionDecl {
    /// The extension's id, relative to the plugin.
    pub name: String,
    /// Its `.ppfp` file.
    pub path: String,
    /// The full id of the face it extends.
    pub extends: String,
}

impl FontExtensionDecl {
    /// An extension `name` (relative to the plugin) from the `.ppfp` file
    /// `path`, adding glyphs to the face `extends` (a full id).
    pub fn new(name: &str, path: &str, extends: &str) -> Self {
        Self {
            name: name.to_owned(),
            path: path.to_owned(),
            extends: extends.to_owned(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Item {
    Group(GroupDecl),
    Spec(SpecDecl, SpecParts),
    Extra(ExtraDecl),
    Hyphenation { language: String, path: String },
    FontFace(FontFaceDecl),
    FontExtension(FontExtensionDecl),
}

/// The items a plugin declares, in the order it declares them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Declarations {
    items: Vec<Item>,
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
    let mut out = alloc::vec![MaybeUninit::<u8>::uninit(); SETTINGS_MAX_BYTES];
    let len = settings_encode(value, &mut out).map_err(|_| DeclareError::Default)?;
    let bytes = out.get(..len).ok_or(DeclareError::Default)?;
    // SAFETY: `settings_encode` initialized the first `len` bytes of `out`.
    Ok(bytes.iter().map(|b| unsafe { b.assume_init() }).collect())
}

fn choices_text(choices: &[String], default: &str) -> Result<String, DeclareError> {
    let ok = !choices.is_empty()
        && choices.len() <= CHOICES_MAX_VALUES
        && choices
            .iter()
            .all(|v| !v.is_empty() && v.len() <= CHOICE_MAX_BYTES && !v.contains('\n'))
        && choices
            .iter()
            .enumerate()
            .all(|(i, v)| !choices.iter().take(i).any(|earlier| earlier == v));
    if !ok {
        return Err(DeclareError::Choices);
    }
    if !choices.iter().any(|v| v == default) {
        return Err(DeclareError::Default);
    }
    Ok(choices.join("\n"))
}

impl SpecDecl {
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

impl Declarations {
    /// No items yet.
    pub const fn new() -> Self {
        Self { items: Vec::new() }
    }

    /// How many items there are.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether there are none.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Adds a `setting.group`.
    ///
    /// # Errors
    ///
    /// An empty title, or a field over its cap.
    pub fn group(&mut self, group: GroupDecl) -> Result<&mut Self, DeclareError> {
        within(
            &group.name,
            records_wire::SETTING_GROUP_NAME_MAX_BYTES,
            "group name",
        )?;
        filled(&group.title, "group title")?;
        within(
            &group.title,
            records_wire::SETTING_GROUP_TITLE_MAX_BYTES,
            "group title",
        )?;
        within(
            &group.page,
            records_wire::SETTING_GROUP_PAGE_MAX_BYTES,
            "group page",
        )?;
        within(
            &group.parent,
            records_wire::SETTING_GROUP_PARENT_MAX_BYTES,
            "group parent",
        )?;
        self.items.push(Item::Group(group));
        Ok(self)
    }

    /// Adds a `setting.spec`.
    ///
    /// # Errors
    ///
    /// An empty title or group, a field over its cap, a default that does not
    /// fit the kind, a bad range or choice list, or a secret that is scoped.
    pub fn spec(&mut self, spec: SpecDecl) -> Result<&mut Self, DeclareError> {
        within(
            &spec.name,
            records_wire::SETTING_SPEC_NAME_MAX_BYTES,
            "setting name",
        )?;
        filled(&spec.group, "setting group")?;
        within(
            &spec.group,
            records_wire::SETTING_SPEC_GROUP_MAX_BYTES,
            "setting group",
        )?;
        filled(&spec.title, "setting title")?;
        within(
            &spec.title,
            records_wire::SETTING_SPEC_TITLE_MAX_BYTES,
            "setting title",
        )?;
        within(
            &spec.help,
            records_wire::SETTING_SPEC_HELP_MAX_BYTES,
            "setting help",
        )?;
        if spec.secret && spec.scoped {
            return Err(DeclareError::SecretScoped);
        }
        let parts = spec.parts()?;
        within(
            &parts.choices,
            records_wire::SETTING_SPEC_CHOICES_MAX_BYTES,
            "choices",
        )?;
        if parts.default.len() > records_wire::SETTING_SPEC_DEFAULT_MAX_BYTES {
            return Err(DeclareError::TooLong("default"));
        }
        self.items.push(Item::Spec(spec, parts));
        Ok(self)
    }

    /// Adds a `setting.extra`.
    ///
    /// # Errors
    ///
    /// A field over its cap, a negative step, or `shown_if` with an empty id.
    pub fn extra(&mut self, extra: ExtraDecl) -> Result<&mut Self, DeclareError> {
        within(
            &extra.name,
            records_wire::SETTING_EXTRA_NAME_MAX_BYTES,
            "extra name",
        )?;
        within(
            &extra.unit,
            records_wire::SETTING_EXTRA_UNIT_MAX_BYTES,
            "unit",
        )?;
        within(
            &extra.section,
            records_wire::SETTING_EXTRA_SECTION_MAX_BYTES,
            "section",
        )?;
        within(
            &extra.follows,
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
        self.items.push(Item::Extra(extra));
        Ok(self)
    }

    /// Adds a `resource.hyphenation`: the table of `path` for `language`.
    ///
    /// # Errors
    ///
    /// An empty language or path, or one over its cap.
    pub fn hyphenation(&mut self, language: &str, path: &str) -> Result<&mut Self, DeclareError> {
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
        self.items.push(Item::Hyphenation {
            language: language.to_owned(),
            path: path.to_owned(),
        });
        Ok(self)
    }

    /// Adds a `resource.font_face`.
    ///
    /// # Errors
    ///
    /// An empty name or path, or a field over its cap.
    pub fn font_face(&mut self, face: FontFaceDecl) -> Result<&mut Self, DeclareError> {
        filled(&face.name, "face name")?;
        filled(&face.path, "path")?;
        within(
            &face.name,
            records_wire::RESOURCE_FONT_FACE_NAME_MAX_BYTES,
            "face name",
        )?;
        within(
            &face.path,
            records_wire::RESOURCE_FONT_FACE_PATH_MAX_BYTES,
            "path",
        )?;
        within(
            &face.family,
            records_wire::RESOURCE_FONT_FACE_FAMILY_MAX_BYTES,
            "family",
        )?;
        within(
            &face.role,
            records_wire::RESOURCE_FONT_FACE_ROLE_MAX_BYTES,
            "role",
        )?;
        self.items.push(Item::FontFace(face));
        Ok(self)
    }

    /// Adds a `resource.font_extension`.
    ///
    /// # Errors
    ///
    /// An empty name, path or `extends`, or a field over its cap.
    pub fn font_extension(
        &mut self,
        extension: FontExtensionDecl,
    ) -> Result<&mut Self, DeclareError> {
        filled(&extension.name, "extension name")?;
        filled(&extension.path, "path")?;
        filled(&extension.extends, "extends")?;
        within(
            &extension.name,
            records_wire::RESOURCE_FONT_EXTENSION_NAME_MAX_BYTES,
            "extension name",
        )?;
        within(
            &extension.path,
            records_wire::RESOURCE_FONT_EXTENSION_PATH_MAX_BYTES,
            "path",
        )?;
        within(
            &extension.extends,
            records_wire::RESOURCE_FONT_EXTENSION_EXTENDS_MAX_BYTES,
            "extends",
        )?;
        self.items.push(Item::FontExtension(extension));
        Ok(self)
    }

    /// The items as encoded batches, in order: each holds at most
    /// [`BATCH_MAX_RECORDS`] records and [`BATCH_MAX_BYTES`] bytes, and a
    /// `setting.extra` always follows its setting (a later batch at the latest).
    ///
    /// # Errors
    ///
    /// [`DeclareError::Encode`] when one item alone does not fit a batch.
    pub fn batches(&self) -> Result<Vec<Vec<u8>>, DeclareError> {
        let mut batches = Vec::new();
        let mut start = 0;
        while start < self.items.len() {
            let (bytes, taken) = self.fill_batch(start)?;
            batches.push(bytes);
            start += taken;
        }
        Ok(batches)
    }

    /// Encodes as many items from `start` as one batch takes (at least one).
    fn fill_batch(&self, start: usize) -> Result<(Vec<u8>, usize), DeclareError> {
        let rest = self.items.get(start..).unwrap_or_default();
        let mut taken = rest.len().min(BATCH_MAX_RECORDS);
        // A refused encode leaves the output unchanged, so one buffer serves every try.
        let mut out = alloc::vec![0_u8; BATCH_MAX_BYTES];
        loop {
            let records: Vec<Record<'_>> = rest[..taken].iter().map(Item::record).collect();
            match records_encode(&records, &mut out) {
                Ok(len) => {
                    out.truncate(len);
                    return Ok((out, taken));
                }
                // Too many bytes for this many records: try one fewer.
                Err(ERR_NO_SPACE) if taken > 1 => taken -= 1,
                Err(status) => return Err(DeclareError::Encode(status)),
            }
        }
    }
}

impl Item {
    fn record(&self) -> Record<'_> {
        match self {
            Item::Group(g) => Record::SettingGroup(SettingGroup {
                name: &g.name,
                title: &g.title,
                order: g.order,
                page: &g.page,
                parent: &g.parent,
            }),
            Item::Spec(s, parts) => Record::SettingSpec(spec_record(s, parts)),
            Item::Extra(e) => Record::SettingExtra(extra_record(e)),
            Item::Hyphenation { language, path } => {
                Record::ResourceHyphenation(ResourceHyphenation { language, path })
            }
            Item::FontFace(f) => Record::ResourceFontFace(ResourceFontFace {
                name: &f.name,
                path: &f.path,
                family: &f.family,
                style: f.style,
                weight: f.weight,
                role: &f.role,
                priority: f.priority,
            }),
            Item::FontExtension(e) => Record::ResourceFontExtension(ResourceFontExtension {
                name: &e.name,
                path: &e.path,
                extends: &e.extends,
            }),
        }
    }
}

fn spec_record<'a>(s: &'a SpecDecl, parts: &'a SpecParts) -> SettingSpec<'a> {
    SettingSpec {
        name: &s.name,
        kind: parts.kind,
        flags: parts.flags,
        group: &s.group,
        title: &s.title,
        help: &s.help,
        default: &parts.default,
        min: parts.min,
        max: parts.max,
        choices: &parts.choices,
    }
}

fn spec_flags(s: &SpecDecl) -> u32 {
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

fn extra_record(e: &ExtraDecl) -> SettingExtra<'_> {
    let (shown_id, shown_value) = e
        .shown_if
        .as_ref()
        .map_or(("", ""), |(id, value)| (id.as_str(), value.as_str()));
    let (editor_kind, editor) = match &e.editor {
        None => (0, ""),
        Some(EditorDecl::Screen(id)) => (1, id.as_str()),
        Some(EditorDecl::Slot(id)) => (2, id.as_str()),
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
        name: &e.name,
        unit: &e.unit,
        section: &e.section,
        follows: &e.follows,
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
