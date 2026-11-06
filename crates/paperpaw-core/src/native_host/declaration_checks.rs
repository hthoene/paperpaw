//! Allocation-free cross-record checks over image and owned declarations.
use super::{raw_declarations::RawDeclarations, registrations::*, staged::Declarations};
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
        let mut parent = None;
        self.visit(&mut |record| {
            if let Record::SettingGroup(group) = record
                && group.name == name
            {
                parent = Some(group.parent);
            }
        })?;
        Ok(parent)
    }
    fn spec(&self, name: &str) -> Result<Option<u32>, i32> {
        let mut kind = None;
        self.visit(&mut |record| {
            if let Record::SettingSpec(spec) = record
                && spec.name == name
            {
                kind = Some(spec.kind);
            }
        })?;
        Ok(kind)
    }
    pub(crate) fn check_batch(&self, batch: &RecordBatch<'_>) -> Result<(), i32> {
        super::declarations::validate_batch(self.plugin, batch)?;
        for (index, record) in batch.iter().enumerate() {
            let Some(wanted) = key(record) else {
                continue;
            };
            let mut duplicate = batch
                .iter()
                .take(index)
                .any(|earlier| same_key(key(earlier), wanted));
            self.visit(&mut |earlier| duplicate |= same_key(key(earlier), wanted))?;
            if duplicate {
                return Err(ERR_INVALID);
            }
            if let Record::SettingExtra(extra) = record {
                let known = self.spec(extra.name)?.is_some() || batch.iter().take(index).any(|earlier| matches!(earlier, Record::SettingSpec(spec) if spec.name == extra.name));
                if !known {
                    return Err(ERR_INVALID);
                }
            }
        }
        Ok(())
    }
    pub(crate) fn finish(&self) -> Result<(), i32> {
        let mut groups = 0;
        self.visit(&mut |record| {
            if matches!(record, Record::SettingGroup(_)) {
                groups += 1;
            }
        })?;
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
fn key(record: Record<'_>) -> Option<(u8, &str)> {
    Some(match record {
        Record::SettingGroup(r) => (0, r.name),
        Record::SettingSpec(r) => (1, r.name),
        Record::SettingExtra(r) => (2, r.name),
        Record::ResourceHyphenation(r) => (3, r.language),
        Record::ResourceFontFace(r) => (4, r.name),
        Record::ResourceFontExtension(r) => (4, r.name),
        _ => return None,
    })
}
fn same_key(candidate: Option<(u8, &str)>, wanted: (u8, &str)) -> bool {
    candidate.is_some_and(|candidate| {
        candidate.0 == wanted.0
            && if wanted.0 == 3 {
                candidate.1.eq_ignore_ascii_case(wanted.1)
            } else {
                candidate.1 == wanted.1
            }
    })
}
