//! Whether two setting specs are the same in every field.

use super::{SettingKind, SettingSpec};

impl SettingSpec {
    /// Whether `other` is the same setting in every field: a choice list is
    /// the same list whether it is kept in the program or made on the heap,
    /// and so is any text. The kernel takes a setting kept in the program in
    /// place of a copy made from a record only when this holds. Every field is
    /// named, so one added later fails the build here until it is compared.
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn same_as(&self, other: &SettingSpec) -> bool {
        let Self {
            id,
            title,
            kind,
            default,
            group,
            level,
            description,
            scoped,
            guarded,
            kept_on_reset,
            extra: _,
        } = self;
        let same_kind = match (kind, &other.kind) {
            (SettingKind::Choice(a), SettingKind::Choice(b)) => a.iter().eq(b.iter()),
            (a, b) => a == b,
        };
        *id == other.id
            && *title == other.title
            && same_kind
            && *default == other.default
            && *group == other.group
            && *level == other.level
            && *description == other.description
            && *scoped == other.scoped
            && *guarded == other.guarded
            && *kept_on_reset == other.kept_on_reset
            && self.extra() == other.extra()
    }
}
