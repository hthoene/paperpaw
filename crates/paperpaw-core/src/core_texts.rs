//! The core plugin's translation catalogs.

use alloc::borrow::Cow;
use paperpaw_sdk::{RegisterError, Registrar};

/// Adds the catalogs. Each stays under the 15-bit static catalog-index limit
/// (32 KiB): the host prompts and the `core.setting.*` texts have catalogs
/// of their own.
pub(crate) fn add(r: &mut Registrar) -> Result<(), RegisterError> {
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
