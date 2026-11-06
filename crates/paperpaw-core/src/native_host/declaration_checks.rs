//! Allocation-free cross-record checks over image and owned declarations.
use super::{
    declaration_index::{key, same_name},
    raw_declarations::RawDeclarations,
    registrations::*,
    staged::Declarations,
};
use paperpaw_native::host_glue::ERR_INVALID;

pub(crate) struct View<'a> {
    pub(crate) raw: &'a RawDeclarations,
    pub(crate) owned: &'a Declarations,
    pub(crate) image: Option<&'a [u8]>,
    pub(crate) plugin: &'a str,
}
impl<'a> View<'a> {
    fn visit(&self, visit: &mut dyn FnMut(Record<'a>)) -> Result<(), i32> {
        self.raw.visit(self.plugin, self.image, visit)?;
        // Ordinary decoded submissions have no raw batch. Owned wire entries
        // may appear twice; existence checks deliberately ignore multiplicity.
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
        // Decoded-only host submissions have no raw index. Compare them with
        // image entries without mistaking an owned wire mirror for a duplicate.
        for entry in self.raw.index.iter().filter(|_| !self.owned.is_empty()) {
            if self.raw.batches[usize::from(entry.batch)].image.is_some() {
                let (kind, name) = key(self.raw.record(self.image, *entry)?).ok_or(ERR_INVALID)?;
                if self.owned_has(kind, name) {
                    return Err(ERR_INVALID);
                }
            }
        }
        let groups = self
            .raw
            .index
            .iter()
            .filter(|entry| entry.kind() == 0)
            .count()
            + self.owned.groups().len();
        let mut result = Ok(());
        self.visit(&mut |record| {
            if result.is_err() {
                return;
            }
            result = match record {
                Record::SettingSpec(spec) => self.require_group(spec.group),
                Record::SettingGroup(group) => self.ancestry(group.parent, groups),
                Record::SettingExtra(extra) => self.follow_kind(&extra),
                _ => Ok(()),
            };
        })?;
        result
    }
    // A decoded-only host batch has no raw index. Its names must also be
    // checked against owned wire mirrors retained in the decoded collection.
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

    fn require_group(&self, id: &str) -> Result<(), i32> {
        if let Some(name) = own_name(self.plugin, id)
            && self.group(name)?.is_none()
        {
            return Err(ERR_INVALID);
        }
        Ok(())
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
