//! Host-owned copies of declaration records staged during one construction.
//!
//! The wire records borrow the caller's bytes, so the host copies what it keeps.
//! One [`Declarations`] belongs to one `make` call. A batch is staged whole or
//! not at all: [`Declarations::stage`] checks everything without allocating,
//! then reserves every Vec and every owned copy, and only then inserts with
//! operations that cannot fail. After any `Err` the value is unchanged.
//! [`Declarations::finish`] runs the intra-plugin cross-record checks once
//! `make` has returned. Mapping onto the settings, font and hyphenation
//! registries is not done here; foreign references are left to that step.

use super::{
    declarations,
    registrations::{
        Record, RecordBatch, ResourceFontExtension, ResourceFontFace, ResourceHyphenation,
        SettingExtra, SettingGroup, SettingSpec,
    },
};
use alloc::{string::String, vec::Vec};
use core::mem::size_of;
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NO_MEMORY, ERR_NO_SPACE};

/// Most setting groups one plugin may declare.
pub const GROUPS_MAX: usize = 32;
/// Most setting specs one plugin may declare (provisional, Q7). Bounds owned
/// copies only, i.e. loads without an image (tests, `(null, 0)`); declarations
/// read from the plugin's image are bounded by [`IMAGE_DECLARATIONS_MAX`] and
/// [`IMAGE_INDEX_MAX_BYTES`] instead. Every production shell passes the image
/// (the X4's flash mapping, `.rodata` on the desktop).
pub const SPECS_MAX: usize = 128;
/// Most setting extras one plugin may declare (provisional, Q7). Bounds owned
/// copies only, as [`SPECS_MAX`].
pub const EXTRAS_MAX: usize = 128;
/// Most hyphenation resources one plugin may declare.
pub const HYPHENATIONS_MAX: usize = 8;
/// Most font faces one plugin may declare.
pub const FONT_FACES_MAX: usize = 16;
/// Most font extensions one plugin may declare.
pub const FONT_EXTENSIONS_MAX: usize = 16;
/// Byte budget for everything one plugin's staged declarations own: each entry
/// counts its struct size plus every owned string and byte copy. Provisional
/// until the X4 heap gate has measured it. Bounds owned copies only, as
/// [`SPECS_MAX`]; image-backed declarations keep offsets and are counted in
/// [`IMAGE_INDEX_MAX_BYTES`].
pub const DECLARATIONS_MAX_BYTES: usize = 32 * 1024;

/// Most declarations one plugin may borrow from its immutable image.
pub const IMAGE_DECLARATIONS_MAX: usize = 512;
/// Resident indexes for image-backed declarations, separate from owned copies.
pub const IMAGE_INDEX_MAX_BYTES: usize = 8 * 1024;

/// An owned setting group.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct StagedGroup {
    /// Name relative to the plugin.
    pub name: String,
    /// Display title.
    pub title: String,
    /// Sort order.
    pub order: i32,
    /// Relative page name, or empty.
    pub page: String,
    /// Full parent group id, or empty.
    pub parent: String,
}

/// An owned setting spec.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct StagedSpec {
    /// Name relative to the plugin.
    pub name: String,
    /// Kind: bool, int, choice, text, language, list.
    pub kind: u32,
    /// Flag bits.
    pub flags: u32,
    /// Full group id.
    pub group: String,
    /// Display title.
    pub title: String,
    /// Help text, or empty.
    pub help: String,
    /// The validated `settings_value` bytes of the default.
    pub default: Vec<u8>,
    /// Integer minimum.
    pub min: i32,
    /// Integer maximum.
    pub max: i32,
    /// Newline-separated choice values.
    pub choices: String,
}

/// An owned setting extra; its `name` is the name of a staged spec.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct StagedExtra {
    /// Name of the spec this extends.
    pub name: String,
    /// Unit label.
    pub unit: String,
    /// Section label.
    pub section: String,
    /// Full id of the setting this one follows, or empty.
    pub follows: String,
    /// Full id of a setting that gates display, or empty.
    pub shown_id: String,
    /// Value of `shown_id` that shows this setting.
    pub shown_value: String,
    /// Editor kind.
    pub editor_kind: u32,
    /// Editor name.
    pub editor: String,
    /// Step for integer editors.
    pub step: i32,
    /// Flag bits.
    pub flags: u32,
}

/// An owned hyphenation resource.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct StagedHyphenation {
    /// Language tag.
    pub language: String,
    /// Relative resource path.
    pub path: String,
}

/// An owned font face.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct StagedFontFace {
    /// Name relative to the plugin.
    pub name: String,
    /// Relative resource path.
    pub path: String,
    /// Family name.
    pub family: String,
    /// Style bits.
    pub style: u32,
    /// Weight, 0 to 1000.
    pub weight: u32,
    /// Full role id, or empty.
    pub role: String,
    /// Priority, -100 to 100.
    pub priority: i32,
}

/// An owned font extension.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct StagedFontExtension {
    /// Name relative to the plugin.
    pub name: String,
    /// Relative resource path.
    pub path: String,
    /// Full id of the font it extends.
    pub extends: String,
}

/// Declarations staged by one construction. Empty values do not allocate.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Declarations {
    groups: Vec<StagedGroup>,
    specs: Vec<StagedSpec>,
    extras: Vec<StagedExtra>,
    hyphenations: Vec<StagedHyphenation>,
    faces: Vec<StagedFontFace>,
    extensions: Vec<StagedFontExtension>,
    bytes: usize,
}

#[derive(Clone, Copy, Default)]
struct Counts {
    groups: usize,
    specs: usize,
    extras: usize,
    hyphenations: usize,
    faces: usize,
    extensions: usize,
    bytes: usize,
}

impl Counts {
    fn of(batch: &RecordBatch<'_>) -> Self {
        let mut counts = Self::default();
        for record in batch.iter() {
            let (slot, cost) = match &record {
                Record::SettingGroup(r) => (&mut counts.groups, group_cost(r)),
                Record::SettingSpec(r) => (&mut counts.specs, spec_cost(r)),
                Record::SettingExtra(r) => (&mut counts.extras, extra_cost(r)),
                Record::ResourceHyphenation(r) => (&mut counts.hyphenations, hyph_cost(r)),
                Record::ResourceFontFace(r) => (&mut counts.faces, face_cost(r)),
                Record::ResourceFontExtension(r) => (&mut counts.extensions, ext_cost(r)),
                _ => continue,
            };
            *slot += 1;
            counts.bytes += cost;
        }
        counts
    }
}

fn group_cost(r: &SettingGroup<'_>) -> usize {
    // Every field named: a text field added to the record must be counted here.
    let SettingGroup {
        name: _,
        title: _,
        order: _,
        page: _,
        parent: _,
    } = r;
    size_of::<StagedGroup>() + r.name.len() + r.title.len() + r.page.len() + r.parent.len()
}

fn spec_cost(r: &SettingSpec<'_>) -> usize {
    // Every field named: a text field added to the record must be counted here.
    let SettingSpec {
        name: _,
        kind: _,
        flags: _,
        group: _,
        title: _,
        help: _,
        default: _,
        min: _,
        max: _,
        choices: _,
    } = r;
    size_of::<StagedSpec>()
        + r.name.len()
        + r.group.len()
        + r.title.len()
        + r.help.len()
        + r.default.len()
        + r.choices.len()
}

fn extra_cost(r: &SettingExtra<'_>) -> usize {
    // Every field named: a text field added to the record must be counted here.
    let SettingExtra {
        name: _,
        unit: _,
        section: _,
        follows: _,
        shown_id: _,
        shown_value: _,
        editor_kind: _,
        editor: _,
        step: _,
        flags: _,
        check: _,
    } = r;
    size_of::<StagedExtra>()
        + r.name.len()
        + r.unit.len()
        + r.section.len()
        + r.follows.len()
        + r.shown_id.len()
        + r.shown_value.len()
        + r.editor.len()
}

fn hyph_cost(r: &ResourceHyphenation<'_>) -> usize {
    // Every field named: a text field added to the record must be counted here.
    let ResourceHyphenation {
        language: _,
        path: _,
    } = r;
    size_of::<StagedHyphenation>() + r.language.len() + r.path.len()
}

fn face_cost(r: &ResourceFontFace<'_>) -> usize {
    // Every field named: a text field added to the record must be counted here.
    let ResourceFontFace {
        name: _,
        path: _,
        family: _,
        style: _,
        weight: _,
        role: _,
        priority: _,
    } = r;
    size_of::<StagedFontFace>() + r.name.len() + r.path.len() + r.family.len() + r.role.len()
}

fn ext_cost(r: &ResourceFontExtension<'_>) -> usize {
    // Every field named: a text field added to the record must be counted here.
    let ResourceFontExtension {
        name: _,
        path: _,
        extends: _,
    } = r;
    size_of::<StagedFontExtension>() + r.name.len() + r.path.len() + r.extends.len()
}

fn own(text: &str) -> Result<String, i32> {
    let mut out = String::new();
    out.try_reserve_exact(text.len())
        .map_err(|_| ERR_NO_MEMORY)?;
    out.push_str(text);
    Ok(out)
}

fn own_bytes(bytes: &[u8]) -> Result<Vec<u8>, i32> {
    let mut out = Vec::new();
    out.try_reserve_exact(bytes.len())
        .map_err(|_| ERR_NO_MEMORY)?;
    out.extend_from_slice(bytes);
    Ok(out)
}

fn reserve<T>(vec: &mut Vec<T>, additional: usize) -> Result<(), i32> {
    vec.try_reserve_exact(additional).map_err(|_| ERR_NO_MEMORY)
}

/// Room for `additional` more entries in a vector that grows over several
/// batches: amortized, so that not every batch reallocates it and leaves old
/// and new buffers alive at once on the X4 heap.
fn grow<T>(vec: &mut Vec<T>, additional: usize) -> Result<(), i32> {
    vec.try_reserve(additional).map_err(|_| ERR_NO_MEMORY)
}

/// Entries built from one batch, every buffer already allocated.
struct Pending {
    groups: Vec<StagedGroup>,
    specs: Vec<StagedSpec>,
    extras: Vec<StagedExtra>,
    hyphenations: Vec<StagedHyphenation>,
    faces: Vec<StagedFontFace>,
    extensions: Vec<StagedFontExtension>,
}

impl Pending {
    fn build(batch: &RecordBatch<'_>, counts: &Counts) -> Result<Self, i32> {
        let mut pending = Self {
            groups: Vec::new(),
            specs: Vec::new(),
            extras: Vec::new(),
            hyphenations: Vec::new(),
            faces: Vec::new(),
            extensions: Vec::new(),
        };
        reserve(&mut pending.groups, counts.groups)?;
        reserve(&mut pending.specs, counts.specs)?;
        reserve(&mut pending.extras, counts.extras)?;
        reserve(&mut pending.hyphenations, counts.hyphenations)?;
        reserve(&mut pending.faces, counts.faces)?;
        reserve(&mut pending.extensions, counts.extensions)?;
        for record in batch.iter() {
            pending.copy(&record)?;
        }
        Ok(pending)
    }

    /// Capacity was reserved from the batch's own counts, so no push grows.
    /// Every field is named, so a field added to a record fails to compile
    /// here until it is staged.
    fn copy(&mut self, record: &Record<'_>) -> Result<(), i32> {
        match record {
            Record::SettingGroup(SettingGroup {
                name,
                title,
                order,
                page,
                parent,
            }) => self.groups.push(StagedGroup {
                name: own(name)?,
                title: own(title)?,
                order: *order,
                page: own(page)?,
                parent: own(parent)?,
            }),
            Record::SettingSpec(SettingSpec {
                name,
                kind,
                flags,
                group,
                title,
                help,
                default,
                min,
                max,
                choices,
            }) => self.specs.push(StagedSpec {
                name: own(name)?,
                kind: *kind,
                flags: *flags,
                group: own(group)?,
                title: own(title)?,
                help: own(help)?,
                default: own_bytes(default)?,
                min: *min,
                max: *max,
                choices: own(choices)?,
            }),
            Record::SettingExtra(SettingExtra {
                name,
                unit,
                section,
                follows,
                shown_id,
                shown_value,
                editor_kind,
                editor,
                step,
                flags,
                check: _,
            }) => self.extras.push(StagedExtra {
                name: own(name)?,
                unit: own(unit)?,
                section: own(section)?,
                follows: own(follows)?,
                shown_id: own(shown_id)?,
                shown_value: own(shown_value)?,
                editor_kind: *editor_kind,
                editor: own(editor)?,
                step: *step,
                flags: *flags,
            }),
            Record::ResourceHyphenation(ResourceHyphenation { language, path }) => {
                self.hyphenations.push(StagedHyphenation {
                    language: own(language)?,
                    path: own(path)?,
                })
            }
            Record::ResourceFontFace(ResourceFontFace {
                name,
                path,
                family,
                style,
                weight,
                role,
                priority,
            }) => self.faces.push(StagedFontFace {
                name: own(name)?,
                path: own(path)?,
                family: own(family)?,
                style: *style,
                weight: *weight,
                role: own(role)?,
                priority: *priority,
            }),
            Record::ResourceFontExtension(ResourceFontExtension {
                name,
                path,
                extends,
            }) => self.extensions.push(StagedFontExtension {
                name: own(name)?,
                path: own(path)?,
                extends: own(extends)?,
            }),
            _ => {}
        }
        Ok(())
    }
}

/// The relative part of `id` when it lies in the plugin's own namespace.
fn own_name<'a>(plugin_id: &str, id: &'a str) -> Option<&'a str> {
    id.strip_prefix(plugin_id)?.strip_prefix('.')
}

impl Declarations {
    /// Staged groups in declaration order.
    #[must_use]
    pub fn groups(&self) -> &[StagedGroup] {
        &self.groups
    }

    /// Staged specs in declaration order.
    #[must_use]
    pub fn specs(&self) -> &[StagedSpec] {
        &self.specs
    }

    /// Staged extras in declaration order.
    #[must_use]
    pub fn extras(&self) -> &[StagedExtra] {
        &self.extras
    }

    /// Staged hyphenation resources in declaration order.
    #[must_use]
    pub fn hyphenations(&self) -> &[StagedHyphenation] {
        &self.hyphenations
    }

    /// Staged font faces in declaration order.
    #[must_use]
    pub fn font_faces(&self) -> &[StagedFontFace] {
        &self.faces
    }

    /// Staged font extensions in declaration order.
    #[must_use]
    pub fn font_extensions(&self) -> &[StagedFontExtension] {
        &self.extensions
    }

    /// Whether nothing is staged.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.bytes == 0
    }

    /// Bytes counted against [`DECLARATIONS_MAX_BYTES`].
    #[must_use]
    pub const fn bytes(&self) -> usize {
        self.bytes
    }

    /// Stage a whole batch or nothing.
    ///
    /// Runs [`declarations::validate_batch`] itself, so a caller needs no
    /// earlier validation. Callback registrations in the batch are ignored.
    ///
    /// # Errors
    /// The status of the first violated per-record rule; `ERR_NO_SPACE` when a
    /// count or the byte budget would be exceeded; `ERR_INVALID` for a duplicate
    /// name or a second extra for the same spec; `ERR_NO_MEMORY` when an allocation fails. `self` is unchanged.
    pub fn stage(&mut self, plugin_id: &str, batch: &RecordBatch<'_>) -> Result<(), i32> {
        self.stage_retaining(plugin_id, batch, 0)
    }

    pub(crate) fn stage_retaining(
        &mut self,
        plugin_id: &str,
        batch: &RecordBatch<'_>,
        retained: usize,
    ) -> Result<(), i32> {
        self.stage_owned(plugin_id, batch, retained)
    }

    fn stage_owned(
        &mut self,
        plugin_id: &str,
        batch: &RecordBatch<'_>,
        retained: usize,
    ) -> Result<(), i32> {
        let counts = self.check(plugin_id, batch)?;
        if self
            .bytes
            .checked_add(counts.bytes)
            .and_then(|n| n.checked_add(retained))
            .is_none_or(|n| n > DECLARATIONS_MAX_BYTES)
        {
            return Err(ERR_NO_SPACE);
        }
        let mut pending = Pending::build(batch, &counts)?;
        grow(&mut self.groups, counts.groups)?;
        grow(&mut self.specs, counts.specs)?;
        grow(&mut self.extras, counts.extras)?;
        grow(&mut self.hyphenations, counts.hyphenations)?;
        grow(&mut self.faces, counts.faces)?;
        grow(&mut self.extensions, counts.extensions)?;
        // Everything is allocated. Append moves within reserved capacity.
        self.groups.append(&mut pending.groups);
        self.specs.append(&mut pending.specs);
        self.extras.append(&mut pending.extras);
        self.hyphenations.append(&mut pending.hyphenations);
        self.faces.append(&mut pending.faces);
        self.extensions.append(&mut pending.extensions);
        self.bytes += counts.bytes;
        Ok(())
    }

    /// First pass: no allocation, no change.
    fn check_counts(&self, plugin_id: &str, batch: &RecordBatch<'_>) -> Result<Counts, i32> {
        declarations::validate_batch(plugin_id, batch)?;
        let counts = Counts::of(batch);
        if self.groups.len() + counts.groups > GROUPS_MAX
            || self.specs.len() + counts.specs > SPECS_MAX
            || self.extras.len() + counts.extras > EXTRAS_MAX
            || self.hyphenations.len() + counts.hyphenations > HYPHENATIONS_MAX
            || self.faces.len() + counts.faces > FONT_FACES_MAX
            || self.extensions.len() + counts.extensions > FONT_EXTENSIONS_MAX
            || self.bytes + counts.bytes > DECLARATIONS_MAX_BYTES
        {
            return Err(ERR_NO_SPACE);
        }
        Ok(counts)
    }

    fn check(&self, plugin_id: &str, batch: &RecordBatch<'_>) -> Result<Counts, i32> {
        let counts = self.check_counts(plugin_id, batch)?;
        for (index, record) in batch.iter().enumerate() {
            let earlier = batch.iter().take(index);
            if !self.is_new(&record, earlier) {
                return Err(ERR_INVALID);
            }
        }
        Ok(counts)
    }

    /// Whether `record` repeats no name of its namespace.
    /// `earlier` are the records before it in the same batch.
    fn is_new<'a>(
        &self,
        record: &Record<'_>,
        earlier: impl Iterator<Item = Record<'a>> + Clone,
    ) -> bool {
        match record {
            Record::SettingGroup(r) => {
                !self.groups.iter().any(|x| x.name == r.name)
                    && !earlier
                        .clone()
                        .any(|e| matches!(e, Record::SettingGroup(x) if x.name == r.name))
            }
            Record::SettingSpec(r) => {
                !self.specs.iter().any(|x| x.name == r.name)
                    && !earlier
                        .clone()
                        .any(|e| matches!(e, Record::SettingSpec(x) if x.name == r.name))
            }
            Record::SettingExtra(r) => {
                !self.extras.iter().any(|x| x.name == r.name)
                    && !earlier
                        .clone()
                        .any(|e| matches!(e, Record::SettingExtra(x) if x.name == r.name))
            }
            Record::ResourceHyphenation(r) => {
                !self
                    .hyphenations
                    .iter()
                    .any(|x| x.language.eq_ignore_ascii_case(r.language))
                    && !earlier.clone().any(|e| {
                        matches!(e, Record::ResourceHyphenation(x)
                            if x.language.eq_ignore_ascii_case(r.language))
                    })
            }
            Record::ResourceFontFace(r) => self.font_name_is_new(r.name, earlier),
            Record::ResourceFontExtension(r) => self.font_name_is_new(r.name, earlier),
            _ => true,
        }
    }

    /// Faces and extensions share one namespace.
    fn font_name_is_new<'a>(&self, name: &str, earlier: impl Iterator<Item = Record<'a>>) -> bool {
        !self.faces.iter().any(|x| x.name == name)
            && !self.extensions.iter().any(|x| x.name == name)
            && !{ earlier }.any(|e| match e {
                Record::ResourceFontFace(x) => x.name == name,
                Record::ResourceFontExtension(x) => x.name == name,
                _ => false,
            })
    }

    /// Cross-record checks inside one plugin, after `make` has returned.
    ///
    /// Every spec `group` and group `parent` in the plugin's own namespace
    /// (`<plugin_id>.`) must name a group staged here, and own parents must not
    /// form a cycle. Each extra must name a spec in the completed transaction,
    /// regardless of submission order. An extra whose `follows` names a spec
    /// needs the same kind as its own spec. Ids outside the namespace
    /// (`core.*`, other plugins) and an own `follows` that names no spec are
    /// left to the kernel's apply step. Does not allocate.
    ///
    /// # Errors
    /// `ERR_INVALID` for the first violation.
    pub fn finish(&self, plugin_id: &str) -> Result<(), i32> {
        let group = |name: &str| self.groups.iter().find(|g| g.name == name);
        for spec in &self.specs {
            if let Some(name) = own_name(plugin_id, &spec.group)
                && group(name).is_none()
            {
                return Err(ERR_INVALID);
            }
        }
        for start in &self.groups {
            let mut current = start;
            let mut steps = 0;
            while let Some(parent) = own_name(plugin_id, &current.parent) {
                current = group(parent).ok_or(ERR_INVALID)?;
                steps += 1;
                if steps > self.groups.len() {
                    return Err(ERR_INVALID);
                }
            }
        }
        for extra in &self.extras {
            let kind_of = |name: &str| self.specs.iter().find(|s| s.name == name).map(|s| s.kind);
            let own_kind = kind_of(&extra.name).ok_or(ERR_INVALID)?;
            let Some(name) = own_name(plugin_id, &extra.follows) else {
                continue;
            };
            if let Some(followed) = kind_of(name)
                && followed != own_kind
            {
                return Err(ERR_INVALID);
            }
        }
        Ok(())
    }
}
