//! Choosing a provider sets its servers; so does typing an address a provider
//! is known by, where nothing is set yet. The plugin hears the change
//! (`event.setting_changed`) and writes the settings the choice implies.
use crate::native_model::account::{self, OWN, Preset, Values, field, key, security_key};
use crate::native_model::mem::Error;
use alloc::string::String;
use alloc::vec::Vec;

/// The value of a setting to write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Value {
    Text(&'static str),
    Int(i64),
}

/// A setting to write: its id and its value.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Change {
    pub(crate) id: String,
    pub(crate) value: Value,
}

/// Which account and field a setting id (`mail.2.address`) is.
pub(crate) fn parse(id: &str) -> Option<(u8, &str)> {
    let rest = id.strip_prefix("mail.")?;
    let (number, field) = rest.split_once('.')?;
    let number: u8 = number
        .parse()
        .ok()
        .filter(|n| (1..=account::MAX).contains(n))?;
    Some((number, field))
}

fn change(number: u8, name: &str, value: Value) -> Result<Change, Error> {
    Ok(Change {
        id: key(number, name)?,
        value,
    })
}

fn push(out: &mut Vec<Change>, change: Change) -> Result<(), Error> {
    out.try_reserve(1).map_err(|_| Error::NoMemory)?;
    out.push(change);
    Ok(())
}

/// The servers of `preset`, and no pinned key: a server of another provider is
/// not the one trusted before.
fn servers(number: u8, preset: &Preset, out: &mut Vec<Change>) -> Result<(), Error> {
    for (name, value) in [
        (field::IMAP_HOST, Value::Text(preset.imap.0)),
        (field::IMAP_PORT, Value::Int(i64::from(preset.imap.1))),
        (
            field::IMAP_SECURITY,
            Value::Text(security_key(preset.imap.2)),
        ),
        (field::SMTP_HOST, Value::Text(preset.smtp.0)),
        (field::SMTP_PORT, Value::Int(i64::from(preset.smtp.1))),
        (
            field::SMTP_SECURITY,
            Value::Text(security_key(preset.smtp.2)),
        ),
        (field::IMAP_PIN, Value::Text("")),
        (field::SMTP_PIN, Value::Text("")),
    ] {
        push(out, change(number, name, value)?)?;
    }
    Ok(())
}

/// What to set after setting `id` changed.
///
/// # Errors
/// `NoMemory` and what the host says of a setting that cannot be read.
pub(crate) fn after_change(values: &dyn Values, id: &str) -> Result<Vec<Change>, Error> {
    let mut out = Vec::new();
    let Some((number, changed)) = parse(id) else {
        return Ok(out);
    };
    match changed {
        field::PROVIDER => {
            if let Some(preset) = account::preset_of(&values.text(id)?) {
                servers(number, preset, &mut out)?;
            }
        }
        field::ADDRESS
            if values.text(&key(number, field::PROVIDER)?)? == OWN
                && values.text(&key(number, field::IMAP_HOST)?)?.is_empty() =>
        {
            if let Some(preset) = account::preset_for_address(&values.text(id)?) {
                servers(number, preset, &mut out)?;
                push(
                    &mut out,
                    change(number, field::PROVIDER, Value::Text(preset.key))?,
                )?;
            }
        }
        _ => {}
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::collections::BTreeMap;
    use alloc::string::ToString;

    #[derive(Default)]
    struct Fake(BTreeMap<String, String>);

    impl Values for Fake {
        fn text(&self, id: &str) -> Result<String, Error> {
            Ok(self.0.get(id).cloned().unwrap_or_default())
        }
        fn int(&self, _: &str) -> Result<i64, Error> {
            Ok(0)
        }
        fn secret(&self, _: &str) -> Result<Option<crate::native_model::mem::Secret>, Error> {
            Ok(None)
        }
    }

    fn has(set: &[Change], id: &str, value: Value) -> bool {
        set.iter().any(|c| c.id == id && c.value == value)
    }

    #[test]
    fn ids_are_parsed() {
        assert_eq!(parse("mail.2.address"), Some((2, "address")));
        assert_eq!(parse("mail.4.address"), None);
        assert_eq!(parse("mail.0.address"), None);
        assert_eq!(parse("mail.fetch"), None);
        assert_eq!(parse("core.1.x"), None);
    }

    #[test]
    fn a_chosen_provider_sets_its_servers() {
        let mut f = Fake::default();
        f.0.insert(
            "mail.1.provider".to_string(),
            "mail.provider.gmx".to_string(),
        );
        let set = after_change(&f, "mail.1.provider").unwrap();
        assert!(has(&set, "mail.1.imap-server", Value::Text("imap.gmx.net")));
        assert!(has(&set, "mail.1.smtp-port", Value::Int(465)));
        assert!(has(&set, "mail.1.imap-pin", Value::Text("")));
        assert_eq!(set.len(), 8);
        // The reader's own servers are left alone.
        f.0.insert("mail.1.provider".to_string(), OWN.to_string());
        assert!(after_change(&f, "mail.1.provider").unwrap().is_empty());
    }

    #[test]
    fn a_known_address_sets_the_provider_once() {
        let mut f = Fake::default();
        f.0.insert("mail.2.address".to_string(), "ada@web.de".to_string());
        f.0.insert("mail.2.provider".to_string(), OWN.to_string());
        let set = after_change(&f, "mail.2.address").unwrap();
        assert!(has(
            &set,
            "mail.2.provider",
            Value::Text("mail.provider.webde")
        ));
        assert!(has(
            &set,
            "mail.2.smtp-security",
            Value::Text("mail.security.starttls")
        ));
        // With servers set already an address changes nothing.
        f.0.insert(
            "mail.2.imap-server".to_string(),
            "imap.example.org".to_string(),
        );
        assert!(after_change(&f, "mail.2.address").unwrap().is_empty());
        // Nor does one nobody knows.
        f.0.insert("mail.2.imap-server".to_string(), String::new());
        f.0.insert("mail.2.address".to_string(), "ada@example.org".to_string());
        assert!(after_change(&f, "mail.2.address").unwrap().is_empty());
    }

    #[test]
    fn other_settings_set_nothing() {
        let f = Fake::default();
        for id in ["mail.fetch", "mail.1.name", "weather.x", "mail.1.imap-port"] {
            assert!(after_change(&f, id).unwrap().is_empty(), "{id}");
        }
    }
}
