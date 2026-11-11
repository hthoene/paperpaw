//! Allocation-free cross-record checks over image and owned declarations.
use super::{
    declaration_index::{STAGE_KIND, STAGE_SUBSCRIPTION_KIND, key, same_name},
    raw_declarations::RawDeclarations,
    registrations::*,
    staged::Declarations,
};
use paperpaw_native::host_glue::{ERR_FULL, ERR_INVALID};

/// Most stage subscriptions one plugin declares.
pub(crate) const STAGE_SUBSCRIPTIONS_MAX: usize = 8;

/// Index category of `screen.spec` (`declaration_index::KIND_TABLE`).
const KIND_SCREEN: u8 = 5;
/// Index category of `action.spec`.
const KIND_ACTION: u8 = 6;
/// Index category of `menu.section`.
const KIND_MENU_SECTION: u8 = 26;
/// Index category of `format.spec`.
const KIND_FORMAT: u8 = 31;
/// Index category of `reader.spec`.
const KIND_READER: u8 = 32;
/// Index category of `book_field.spec`.
const KIND_BOOK_FIELD: u8 = 34;
/// Index category of `book_badge.spec`.
const KIND_BOOK_BADGE: u8 = 35;
/// Index category of `lookup.spec`.
const KIND_LOOKUP: u8 = 33;
const _: () = assert!(KIND_LOOKUP == super::declaration_index::KIND_TABLE[33].1);
/// Lookup providers one plugin declares at most (a ninth is `ERR_INVALID`),
/// and the device holds (`ERR_FULL`), `document.md` section 7.
pub(crate) const LOOKUPS_PER_PLUGIN: usize = 8;
pub(crate) const LOOKUPS_DEVICE: usize = 32;

/// The declaration cap a refused plugin ran into, as `reg_plugin` row says it
/// (`docs/abi/registry.md`): `problem` 14 too many formats, 15 too many
/// readers or 18 too many lookup providers, `arg` the cap (formats 8 or 32,
/// readers 4 or 16, providers 8 or 32).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Cap {
    pub(crate) problem: u32,
    pub(crate) arg: u32,
}
impl Cap {
    /// `reg_plugin.problem`: too many formats.
    pub(crate) const FORMATS: u32 = 14;
    /// `reg_plugin.problem`: too many readers.
    pub(crate) const READERS: u32 = 15;
    /// `reg_plugin.problem`: too many book fields (8 per plugin, 64 device-wide).
    pub(crate) const BOOK_FIELDS: u32 = 16;
    /// `reg_plugin.problem`: too many book badges (4 per plugin, 16 device-wide).
    pub(crate) const BOOK_BADGES: u32 = 17;
    /// `reg_plugin.problem`: too many lookup providers (16 and 17 are the
    /// book fields and badges).
    pub(crate) const LOOKUPS: u32 = 18;

    /// The cap of `problem`; 0 where it does not fit a `u32` (no cap does
    /// today: the row's 0 for "the host cannot say" has no producer yet).
    pub(crate) fn of(problem: u32, cap: usize) -> Self {
        Self {
            problem,
            arg: u32::try_from(cap).unwrap_or(0),
        }
    }
}

pub(crate) struct View<'a> {
    pub(crate) raw: &'a RawDeclarations,
    pub(crate) owned: &'a Declarations,
    pub(crate) image: Option<&'a [u8]>,
    pub(crate) plugin: &'a str,
    /// The cap `finish` refused the batch at, if it was one.
    pub(crate) refused: core::cell::Cell<Option<Cap>>,
}
impl<'a> View<'a> {
    pub(crate) fn visit(&self, visit: &mut dyn FnMut(Record<'a>)) -> Result<(), i32> {
        self.raw.visit(self.plugin, self.image, visit)?;
        // Decoded host submissions have no raw batch.
        for r in self.owned.groups() {
            visit(Record::SettingGroup(SettingGroup {
                name: &r.name,
                title: &r.title,
                order: r.order,
                page: &r.page,
                parent: &r.parent,
            }));
        }
        for r in self.owned.specs() {
            visit(Record::SettingSpec(SettingSpec {
                name: &r.name,
                kind: r.kind,
                flags: r.flags,
                group: &r.group,
                title: &r.title,
                help: &r.help,
                default: &r.default,
                min: r.min,
                max: r.max,
                choices: &r.choices,
            }));
        }
        for r in self.owned.extras() {
            visit(Record::SettingExtra(SettingExtra {
                name: &r.name,
                unit: &r.unit,
                section: &r.section,
                follows: &r.follows,
                shown_id: &r.shown_id,
                shown_value: &r.shown_value,
                editor_kind: r.editor_kind,
                editor: &r.editor,
                step: r.step,
                flags: r.flags,
                check: 0,
            }));
        }
        for r in self.owned.hyphenations() {
            visit(Record::ResourceHyphenation(ResourceHyphenation {
                language: &r.language,
                path: &r.path,
            }));
        }
        for r in self.owned.font_faces() {
            visit(Record::ResourceFontFace(ResourceFontFace {
                name: &r.name,
                path: &r.path,
                family: &r.family,
                style: r.style,
                weight: r.weight,
                role: &r.role,
                priority: r.priority,
            }));
        }
        for r in self.owned.font_extensions() {
            visit(Record::ResourceFontExtension(ResourceFontExtension {
                name: &r.name,
                path: &r.path,
                extends: &r.extends,
            }));
        }
        Ok(())
    }
    fn group(&self, name: &str) -> Result<Option<&'a str>, i32> {
        if let Some(Record::SettingGroup(group)) = self.raw.find(self.image, 0, name)? {
            return Ok(Some(group.parent));
        }
        Ok(self
            .owned
            .groups()
            .iter()
            .find(|group| group.name == name)
            .map(|group| group.parent.as_str()))
    }
    fn spec(&self, name: &str) -> Result<Option<u32>, i32> {
        if let Some(Record::SettingSpec(spec)) = self.raw.find(self.image, 1, name)? {
            return Ok(Some(spec.kind));
        }
        Ok(self
            .owned
            .specs()
            .iter()
            .find(|spec| spec.name == name)
            .map(|spec| spec.kind))
    }
    fn owned_has(&self, kind: u8, name: &str) -> bool {
        match kind {
            0 => self.owned.groups().iter().any(|r| r.name == name),
            1 => self.owned.specs().iter().any(|r| r.name == name),
            2 => self.owned.extras().iter().any(|r| r.name == name),
            3 => self
                .owned
                .hyphenations()
                .iter()
                .any(|r| same_name(kind, &r.language, name)),
            4 => {
                self.owned.font_faces().iter().any(|r| r.name == name)
                    || self.owned.font_extensions().iter().any(|r| r.name == name)
            }
            _ => false,
        }
    }
    pub(crate) fn finish(&self) -> Result<(), i32> {
        self.raw.check_duplicates(self.image)?;
        self.check_owned_names()?;
        // Decoded host submissions must be distinct from every raw entry.
        for entry in self.raw.index.iter().filter(|_| !self.owned.is_empty()) {
            let (kind, name) = key(self.raw.record(self.image, *entry)?).ok_or(ERR_INVALID)?;
            if self.owned_has(kind, name) {
                return Err(ERR_INVALID);
            }
        }
        let groups = self
            .raw
            .index
            .iter()
            .filter(|entry| entry.kind() == 0)
            .count()
            + self.owned.groups().len();
        // Stage subscriptions are the plugin's own declarations, so the
        // plugin can fix a ninth (`stages-services.md` section 1.1).
        let subscriptions = self
            .raw
            .index
            .iter()
            .filter(|entry| entry.kind() == STAGE_SUBSCRIPTION_KIND)
            .count();
        if subscriptions > STAGE_SUBSCRIPTIONS_MAX {
            return Err(ERR_FULL);
        }
        // At most 8 formats per plugin (`document.md` section 1); the 32 of
        // the device are the store's, where plugins meet.
        let formats = self
            .raw
            .index
            .iter()
            .filter(|entry| entry.kind() == KIND_FORMAT)
            .count();
        if formats > super::format_index::PER_PLUGIN {
            self.refused
                .set(Some(Cap::of(Cap::FORMATS, super::format_index::PER_PLUGIN)));
            return Err(ERR_FULL);
        }
        // At most 4 reading apps per plugin: the fifth refuses the batch
        // (`ERR_INVALID`); the 16 of the device are the store's.
        let readers = self
            .raw
            .index
            .iter()
            .filter(|entry| entry.kind() == KIND_READER)
            .count();
        if readers > super::format_index::READERS_PER_PLUGIN {
            self.refused.set(Some(Cap::of(
                Cap::READERS,
                super::format_index::READERS_PER_PLUGIN,
            )));
            return Err(ERR_INVALID);
        }
        // At most 8 book fields, 4 book badges and 8 lookup providers per
        // plugin (`library.md` section 1, `document.md` section 7): the one over
        // refuses the batch (`ERR_INVALID`); the device's caps are the store's.
        for (kind, cap, problem) in [
            (
                KIND_BOOK_FIELD,
                super::declaration_store::FIELDS_PER_PLUGIN,
                Cap::BOOK_FIELDS,
            ),
            (
                KIND_BOOK_BADGE,
                super::declaration_store::BADGES_PER_PLUGIN,
                Cap::BOOK_BADGES,
            ),
            (KIND_LOOKUP, LOOKUPS_PER_PLUGIN, Cap::LOOKUPS),
        ] {
            let count = self
                .raw
                .index
                .iter()
                .filter(|entry| entry.kind() == kind)
                .count();
            if count > cap {
                self.refused.set(Some(Cap::of(problem, cap)));
                return Err(ERR_INVALID);
            }
        }
        let mut result = Ok(());
        self.visit(&mut |record| {
            if result.is_err() {
                return;
            }
            result = match record {
                Record::SettingSpec(spec) => self.require_group(spec.group),
                Record::SettingGroup(group) => self.ancestry(group.parent, groups),
                Record::SettingExtra(extra) => self.follow_kind(&extra),
                Record::RegistrationStage(registration) => self.not_own_stage(&registration),
                Record::SetupStep(step) => self.step_name_is_free(step.name),
                // An item of the plugin's own must exist; another plugin's may
                // come later (`screens-input.md` section 1).
                Record::MenuEntry(entry) => self.require_item(entry.item),
                Record::IconSpec(icon) => self.require_item(icon.item),
                Record::MenuNeed(need) => self.require_item(need.item),
                Record::MenuHide(hide) => self.require_item(hide.item),
                Record::MenuDescribe(describe) => self.require_own(describe.item),
                // `registry.md` section 6: an offer names a declared screen of
                // the declaring plugin. Everything else REG declares refers to
                // other plugins' items too, so it resolves where it is read.
                Record::SlotOffer(offer) => self.require_screen(offer.screen),
                // A format's extensions are its own within the plugin: one
                // claim, one row (`document.md` section 1).
                Record::FormatSpec(format) => self.extensions_are_free(&format),
                // A reading app shows one of the plugin's own screens.
                Record::ReaderSpec(reader) => self.require_screen(reader.screen),
                _ => Ok(()),
            };
        })?;
        result
    }
    /// No other format of the plugin claims an extension of `format`.
    fn extensions_are_free(&self, format: &FormatSpec<'_>) -> Result<(), i32> {
        let mut result = Ok(());
        self.visit(&mut |record| {
            let Record::FormatSpec(other) = record else {
                return;
            };
            if other.name == format.name || result.is_err() {
                return;
            }
            let clash = format
                .extensions
                .split('\n')
                .filter(|e| !e.is_empty())
                .any(|extension| other.extensions.split('\n').any(|e| e == extension));
            if clash {
                result = Err(ERR_INVALID);
            }
        })?;
        result
    }
    fn check_owned_names(&self) -> Result<(), i32> {
        unique_names(self.owned.groups().iter().map(|r| r.name.as_str()), false)?;
        unique_names(self.owned.specs().iter().map(|r| r.name.as_str()), false)?;
        unique_names(self.owned.extras().iter().map(|r| r.name.as_str()), false)?;
        unique_names(
            self.owned
                .hyphenations()
                .iter()
                .map(|r| r.language.as_str()),
            true,
        )?;
        unique_names(
            self.owned
                .font_faces()
                .iter()
                .map(|r| r.name.as_str())
                .chain(self.owned.font_extensions().iter().map(|r| r.name.as_str())),
            false,
        )
    }

    /// A screen, action, setting or setting group of this plugin, by its
    /// relative name.
    fn own_item(&self, name: &str) -> Result<bool, i32> {
        Ok(self.raw.find(self.image, 5, name)?.is_some()
            || self.raw.find(self.image, 6, name)?.is_some()
            || self.spec(name)?.is_some()
            || self.group(name)?.is_some())
    }
    fn require_item(&self, id: &str) -> Result<(), i32> {
        match own_name(self.plugin, id) {
            Some(name) if !self.own_item(name)? => Err(ERR_INVALID),
            _ => Ok(()),
        }
    }
    fn require_own(&self, name: &str) -> Result<(), i32> {
        if self.own_item(name)? {
            Ok(())
        } else {
            Err(ERR_INVALID)
        }
    }
    fn require_group(&self, id: &str) -> Result<(), i32> {
        if let Some(name) = own_name(self.plugin, id)
            && self.group(name)?.is_none()
        {
            return Err(ERR_INVALID);
        }
        Ok(())
    }
    fn require_screen(&self, id: &str) -> Result<(), i32> {
        let name = own_name(self.plugin, id).ok_or(ERR_INVALID)?;
        // Screens are kept only as raw declarations.
        match self.raw.find(self.image, KIND_SCREEN, name)? {
            Some(_) => Ok(()),
            None => Err(ERR_INVALID),
        }
    }
    fn ancestry(&self, mut parent: &'a str, groups: usize) -> Result<(), i32> {
        let mut steps = 0;
        while let Some(name) = own_name(self.plugin, parent) {
            parent = self.group(name)?.ok_or(ERR_INVALID)?;
            steps += 1;
            if steps > groups {
                return Err(ERR_INVALID);
            }
        }
        Ok(())
    }
    /// The owner is inside its own callback whenever its stage runs, so a
    /// subscription to one of its own stages could only answer `ERR_BUSY`.
    fn not_own_stage(&self, registration: &RegistrationStage<'_>) -> Result<(), i32> {
        if let Some(name) = own_name(self.plugin, registration.stage)
            && self.raw.find(self.image, STAGE_KIND, name)?.is_some()
        {
            return Err(ERR_INVALID);
        }
        Ok(())
    }
    /// One item-state push names one thing: a step's name is no action,
    /// screen, setting or menu section of the same plugin (`registry.md`
    /// section 6).
    fn step_name_is_free(&self, name: &str) -> Result<(), i32> {
        if self.raw.find(self.image, KIND_SCREEN, name)?.is_some()
            || self.raw.find(self.image, KIND_ACTION, name)?.is_some()
            || self
                .raw
                .find(self.image, KIND_MENU_SECTION, name)?
                .is_some()
            || self.spec(name)?.is_some()
        {
            return Err(ERR_INVALID);
        }
        Ok(())
    }
    fn follow_kind(&self, extra: &SettingExtra<'_>) -> Result<(), i32> {
        if self.spec(extra.name)?.is_none() {
            return Err(ERR_INVALID);
        }
        if let Some(name) = own_name(self.plugin, extra.follows)
            && let (Some(followed), Some(own)) = (self.spec(name)?, self.spec(extra.name)?)
            && followed != own
        {
            return Err(ERR_INVALID);
        }
        Ok(())
    }
}
fn own_name<'a>(plugin: &str, id: &'a str) -> Option<&'a str> {
    id.strip_prefix(plugin)?.strip_prefix('.')
}

fn unique_names<'a>(
    names: impl Iterator<Item = &'a str> + Clone,
    language: bool,
) -> Result<(), i32> {
    for (at, name) in names.clone().enumerate() {
        if names.clone().take(at).any(|earlier| {
            if language {
                earlier.eq_ignore_ascii_case(name)
            } else {
                earlier == name
            }
        }) {
            return Err(ERR_INVALID);
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "declaration_stage_checks_tests.rs"]
mod stage_tests;

#[cfg(test)]
#[path = "declaration_doc_checks_tests.rs"]
mod doc_tests;

#[cfg(test)]
#[path = "declaration_setup_checks_tests.rs"]
mod setup_tests;
