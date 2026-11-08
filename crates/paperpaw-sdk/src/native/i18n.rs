//! The UI language selected by the host's catalogs.
use super::{
    Error, Host,
    settings::{self, SettingValue},
};

const LANGUAGE_SETTING: &str = "core.language";

/// Read the full UI language tag, such as `de` or `pt-BR`, without allocation.
/// The returned text borrows `scratch`, which must fit the complete setting
/// value including its wire header. The tag's spelling and case are preserved.
/// Plugins caching language-dependent data should subscribe to
/// `event.setting_changed` for `core.language` and read the value again.
///
/// # Errors
/// Invalid for a non-text value, an empty tag or empty/non-alphanumeric subtags;
/// NoSpace when the complete value does not fit; otherwise the host's refusal
/// or the settings decoder's error. No default language is substituted.
pub fn language<'a>(host: &Host<'_>, scratch: &'a mut [u8]) -> Result<&'a str, Error> {
    let SettingValue::Text(tag) = settings::read_setting(host, LANGUAGE_SETTING, scratch)? else {
        return Err(Error::Invalid);
    };
    if tag
        .split('-')
        .any(|part| part.is_empty() || !part.bytes().all(|b| b.is_ascii_alphanumeric()))
    {
        return Err(Error::Invalid);
    }
    Ok(tag)
}

#[cfg(test)]
#[path = "i18n_tests.rs"]
mod tests;
