//! The core plugin's translation catalogs.

use crate::Items;
#[cfg(any(test, feature = "testing"))]
use alloc::borrow::Cow;
use paperpaw_sdk::RegisterError;

/// Adds the catalogs. Each stays under the 15-bit static catalog-index limit
/// (32 KiB): the host prompts and the `core.setting.*` texts have catalogs
/// of their own.
///
/// Only where the kernel is tested (`cfg(test)`, the `testing` feature): a
/// program carries the kernel's texts without their keys, as tables its
/// build script makes of these same files and its shell hands to the
/// builder ([`KernelBuilder::built_messages`](crate::KernelBuilder::built_messages),
/// `paperpaw_app::CORE_MESSAGES`), not the sources too (about 30 KB of flash
/// less, #306).
#[cfg(any(test, feature = "testing"))]
pub(crate) fn add(r: &mut Items) -> Result<(), RegisterError> {
    for source in [
        include_str!("../i18n/en.ini"),
        include_str!("../i18n/de.ini"),
        include_str!("../i18n/host/en.ini"),
        include_str!("../i18n/host/de.ini"),
        include_str!("../i18n/settings/en.ini"),
        include_str!("../i18n/settings/de.ini"),
    ] {
        r.add_translations(Cow::Borrowed(source))?;
    }
    Ok(())
}

/// The sources are not carried (see above): the shell's tables are.
#[cfg(not(any(test, feature = "testing")))]
#[allow(
    clippy::unnecessary_wraps,
    reason = "the same signature as the one that adds the sources"
)]
pub(crate) fn add(_: &mut Items) -> Result<(), RegisterError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use paperpaw_i18n::{Catalog, check};

    /// Each language pair has the same keys and placeholders, the moved
    /// `core.setting.*` and the host prompts included.
    #[test]
    fn every_catalog_pair_is_complete() {
        for (en, de) in [
            (
                include_str!("../i18n/en.ini"),
                include_str!("../i18n/de.ini"),
            ),
            (
                include_str!("../i18n/host/en.ini"),
                include_str!("../i18n/host/de.ini"),
            ),
            (
                include_str!("../i18n/settings/en.ini"),
                include_str!("../i18n/settings/de.ini"),
            ),
        ] {
            let en = Catalog::parse(en).unwrap();
            let de = Catalog::parse(de).unwrap();
            let issues = check::compare(&en, &de);
            assert!(issues.is_empty(), "{issues:?}");
        }
    }
}
