//! Choosing a provider sets its servers; so does typing an address a provider
//! is known by, where nothing is set yet (the interface-6 `provider`, with
//! every allocation fallible).
//!
//! The host-free part: from the id of the setting that changed and the values
//! as they are now, what to set. The plugin stages the result as settings
//! commands (`native_plugin`).
use super::account::{OWN, Preset, Values, field, key, preset_for_address, preset_of};
use super::mem::{Error, copy, vec_with};
use alloc::string::String;
use alloc::vec::Vec;

/// A value to set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Set {
    /// A text setting.
    Text(String),
    /// A whole-number setting.
    Int(i64),
}

/// The settings to set, by id.
pub type Changes = Vec<(String, Set)>;

/// Which account and field a setting id (`mail.2.address`) is.
#[must_use]
pub fn parse(id: &str) -> Option<(u8, &str)> {
    let rest = id.strip_prefix("mail.")?;
    let (number, field) = rest.split_once('.')?;
    let number: u8 = number
        .parse()
        .ok()
        .filter(|n| (1..=super::account::MAX).contains(n))?;
    Some((number, field))
}

fn text(out: &mut Changes, number: u8, name: &str, value: &str) -> Result<(), Error> {
    out.push((key(number, name)?, Set::Text(copy(value)?)));
    Ok(())
}

fn servers(out: &mut Changes, number: u8, preset: &Preset) -> Result<(), Error> {
    text(out, number, field::IMAP_HOST, preset.imap.0)?;
    out.push((
        key(number, field::IMAP_PORT)?,
        Set::Int(i64::from(preset.imap.1)),
    ));
    text(
        out,
        number,
        field::IMAP_SECURITY,
        super::account::security_key(preset.imap.2),
    )?;
    text(out, number, field::SMTP_HOST, preset.smtp.0)?;
    out.push((
        key(number, field::SMTP_PORT)?,
        Set::Int(i64::from(preset.smtp.1)),
    ));
    text(
        out,
        number,
        field::SMTP_SECURITY,
        super::account::security_key(preset.smtp.2),
    )?;
    // A server of another provider is not the one trusted before.
    text(out, number, field::IMAP_PIN, "")?;
    text(out, number, field::SMTP_PIN, "")
}

/// What to set after setting `id` changed (nothing for any other setting, or
/// the reader's own servers).
///
/// # Errors
/// [`Error::NoMemory`] and what reading the values says.
pub fn after_change(values: &dyn Values, id: &str) -> Result<Changes, Error> {
    // 9 servers' settings and the provider: the most this sets.
    let mut out = vec_with(9)?;
    let Some((number, changed)) = parse(id) else {
        return Ok(out);
    };
    match changed {
        field::PROVIDER => {
            if let Some(preset) = preset_of(&values.text(id)?) {
                servers(&mut out, number, preset)?;
            }
        }
        field::ADDRESS
            if values.text(&key(number, field::PROVIDER)?)? == OWN
                && values.text(&key(number, field::IMAP_HOST)?)?.is_empty() =>
        {
            if let Some(preset) = preset_for_address(&values.text(id)?) {
                servers(&mut out, number, preset)?;
                text(&mut out, number, field::PROVIDER, preset.key)?;
            }
        }
        _ => {}
    }
    Ok(out)
}
