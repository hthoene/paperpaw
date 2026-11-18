//! An account as the native plugin reads it: from the settings and the secrets,
//! with the password a secret that is read when a connection is made, the
//! providers' servers as presets, the pins as the settings keep them, and every
//! refused allocation.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests unwrap what they just made"
)]

use paperpaw_plugin_mail::native_model::account::{
    Account, IMPLICIT, MAX, OWN, Offer, PRESETS, PROVIDERS, STARTTLS, Values, field, key, offer,
    pin_of, pin_text, preset_for_address, preset_of, secret_name, security_key, security_of,
};
use paperpaw_plugin_mail::native_model::mem::{Error, Secret};
use paperpaw_plugin_mail::native_model::session::Security;
use std::collections::BTreeMap;

#[derive(Default)]
struct Settings {
    text: BTreeMap<String, String>,
    int: BTreeMap<String, i64>,
    secret: BTreeMap<String, String>,
    unsupported: bool,
}

impl Values for Settings {
    fn text(&self, id: &str) -> Result<String, Error> {
        Ok(self.text.get(id).cloned().unwrap_or_default())
    }
    fn int(&self, id: &str) -> Result<i64, Error> {
        Ok(self.int.get(id).copied().unwrap_or(0))
    }
    fn secret(&self, id: &str) -> Result<Option<Secret>, Error> {
        if self.unsupported {
            return Err(Error::Unsupported);
        }
        self.secret
            .get(id)
            .map(|s| {
                let mut out = Secret::with_capacity(s.len())?;
                out.push_str(s)?;
                Ok(out)
            })
            .transpose()
    }
}

fn complete() -> Settings {
    let mut f = Settings::default();
    for (name, value) in [
        ("address", " ada@example.org "),
        ("name", "Ada"),
        ("imap-server", "imap.example.org"),
        ("imap-security", IMPLICIT),
        ("smtp-server", "smtp.example.org"),
        ("smtp-security", STARTTLS),
    ] {
        f.text.insert(key(1, name).unwrap(), value.into());
    }
    f.int.insert(key(1, "imap-port").unwrap(), 993);
    f.int.insert(key(1, "smtp-port").unwrap(), 587);
    f.secret
        .insert(key(1, "password").unwrap(), "secret".into());
    f
}

#[test]
fn a_secret_is_named_by_the_setting_without_the_plugin_prefix() {
    assert_eq!(secret_name(&key(1, field::PASSWORD).unwrap()), "1.password");
    assert_eq!(secret_name("2.password"), "2.password");
}

#[test]
fn the_keys_are_the_settings_ids() {
    assert_eq!(key(2, field::ADDRESS).unwrap(), "mail.2.address");
    assert_eq!(key(1, field::IMAP_PIN).unwrap(), "mail.1.imap-pin");
}

#[test]
fn providers_and_presets_agree() {
    assert_eq!(PROVIDERS[0], OWN);
    assert_eq!(PROVIDERS.len(), PRESETS.len() + 1);
    for (provider, preset) in PROVIDERS[1..].iter().zip(PRESETS) {
        assert_eq!(*provider, preset.key);
        // Never plain: all go through TLS.
        assert!(preset.imap.1 == 993 || preset.imap.2 == Security::StartTls);
    }
    assert!(preset_of(OWN).is_none());
}

#[test]
fn an_account_needs_address_password_and_servers() {
    let a = Account::read(&complete(), 1).unwrap().unwrap();
    assert_eq!(a.address, "ada@example.org");
    assert_eq!(a.user, "ada@example.org");
    assert_eq!(a.name, "Ada");
    assert_eq!((a.imap.port, a.imap.security), (993, Security::Implicit));
    assert_eq!((a.smtp.port, a.smtp.security), (587, Security::StartTls));
    assert_eq!(a.short(), "ada");
    assert!(Account::read(&complete(), 2).unwrap().is_none());
    for missing in ["password", "address", "imap-server", "smtp-server"] {
        let mut f = complete();
        f.text.remove(&key(1, missing).unwrap());
        f.secret.remove(&key(1, missing).unwrap());
        assert!(Account::read(&f, 1).unwrap().is_none(), "{missing}");
    }
    let mut f = complete();
    f.int.insert(key(1, "smtp-port").unwrap(), 70_000);
    assert!(Account::read(&f, 1).unwrap().is_none());
    f.int.insert(key(1, "smtp-port").unwrap(), 0);
    assert!(Account::read(&f, 1).unwrap().is_none());
}

#[test]
fn the_password_is_read_when_a_connection_is_made_and_is_never_in_the_account() {
    let f = complete();
    let a = Account::read(&f, 1).unwrap().unwrap();
    assert!(!format!("{a:?}").contains("secret"));
    let password = a.password(&f).unwrap();
    assert_eq!(password.as_str(), "secret");
    let e = a.imap.endpoint(&a.user, password).unwrap();
    assert_eq!(
        (e.host.as_str(), e.user.as_str(), e.password.as_str()),
        ("imap.example.org", "ada@example.org", "secret")
    );
    assert_eq!(e.pin, None);
    assert!(!format!("{e:?}").contains("secret"));
    let mut none = complete();
    none.secret.clear();
    assert_eq!(a.password(&none).err(), Some(Error::NotFound));
}

#[test]
fn a_device_that_keeps_no_secrets_has_no_account_to_log_in_with() {
    let mut f = complete();
    f.unsupported = true;
    assert_eq!(Account::read(&f, 1).err(), Some(Error::Unsupported));
}

#[test]
fn a_user_name_overrides_the_address() {
    let mut f = complete();
    f.text.insert(key(1, "user").unwrap(), "ada".into());
    let a = Account::read(&f, 1).unwrap().unwrap();
    assert_eq!(a.user, "ada");
    assert_eq!(Account::all(&f).unwrap().len(), 1);
    assert_eq!(Account::numbers(&f).unwrap(), [1]);
    assert_eq!(MAX, 3);
}

#[test]
fn the_address_suggests_a_provider() {
    assert_eq!(
        preset_for_address("a@GMX.net").unwrap().key,
        "mail.provider.gmx"
    );
    assert_eq!(
        preset_for_address("a@googlemail.com").unwrap().key,
        "mail.provider.gmail"
    );
    assert_eq!(
        preset_for_address("a@Hotmail.de").unwrap().key,
        "mail.provider.outlook"
    );
    assert!(preset_for_address("a@example.org").is_none());
    assert!(preset_for_address("no-at").is_none());
}

#[test]
fn security_keys_round_trip_and_anything_else_is_implicit_tls() {
    for security in [Security::Implicit, Security::StartTls] {
        assert_eq!(security_of(security_key(security)), security);
    }
    assert_eq!(security_of("mail.security.none"), Security::Implicit);
    assert_eq!(security_of(""), Security::Implicit);
}

#[test]
fn a_pin_is_64_hex_digits_and_nothing_else() {
    let pin = [0xab; 32];
    let text = pin_text(&pin).unwrap();
    assert_eq!(text.len(), 64);
    assert_eq!(pin_of(&text), Some(pin));
    assert_eq!(pin_of(&format!(" {} ", text.to_uppercase())), Some(pin));
    for bad in [
        "",
        "ab",
        &text[..63],
        &format!("{text}0"),
        &"g".repeat(64),
        &"é".repeat(32),
    ] {
        assert_eq!(pin_of(bad), None, "{bad}");
    }
    let mut f = complete();
    f.text.insert(key(1, "imap-pin").unwrap(), text);
    assert_eq!(Account::read(&f, 1).unwrap().unwrap().imap.pin, Some(pin));
    let e = {
        let a = Account::read(&f, 1).unwrap().unwrap();
        a.imap.endpoint(&a.user, a.password(&f).unwrap()).unwrap()
    };
    assert_eq!(e.pin, Some(pin));
}

#[test]
fn an_untrusted_server_is_offered_to_trust_only_where_that_is_safe() {
    // An own server never trusted: ask once.
    assert_eq!(offer(&complete(), 1, false).unwrap(), Offer::Ask);
    // A key trusted before and another now: no connection.
    let mut f = complete();
    f.text.insert(key(1, "imap-pin").unwrap(), "ab".repeat(32));
    assert_eq!(offer(&f, 1, false).unwrap(), Offer::Changed);
    assert_eq!(offer(&f, 1, true).unwrap(), Offer::Ask);
    // A provider's own server: someone else is answering.
    let mut g = complete();
    g.text
        .insert(key(1, "provider").unwrap(), "mail.provider.gmx".into());
    g.text
        .insert(key(1, "imap-server").unwrap(), "imap.gmx.net".into());
    assert_eq!(offer(&g, 1, false).unwrap(), Offer::Attack);
    // The reader's own host under a provider is theirs to trust.
    g.text
        .insert(key(1, "imap-server").unwrap(), "mail.my.example".into());
    assert_eq!(offer(&g, 1, false).unwrap(), Offer::Ask);
}

#[test]
fn every_refused_allocation_is_an_error_not_a_missing_account() {
    // The fixture is built first; then each allocation of a read is refused in turn.
    // (`tests/native_model_alloc_refusal.rs` holds the heap that refuses; here the
    // reads are shown to hand the error on, through a settings double that fails.)
    struct Refuses(usize, std::cell::Cell<usize>, Settings);
    impl Values for Refuses {
        fn text(&self, id: &str) -> Result<String, Error> {
            let n = self.1.get() + 1;
            self.1.set(n);
            if n == self.0 {
                Err(Error::NoMemory)
            } else {
                self.2.text(id)
            }
        }
        fn int(&self, id: &str) -> Result<i64, Error> {
            self.2.int(id)
        }
        fn secret(&self, id: &str) -> Result<Option<Secret>, Error> {
            self.2.secret(id)
        }
    }
    for at in 1..=12 {
        let values = Refuses(at, std::cell::Cell::new(0), complete());
        match Account::read(&values, 1) {
            Err(Error::NoMemory) => {}
            Ok(Some(_)) => assert!(at > 8, "at {at}"),
            other => panic!("at {at}: {other:?}"),
        }
    }
}
