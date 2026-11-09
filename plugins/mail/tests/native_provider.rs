//! The provider fill-in: a chosen provider sets its servers; an address a
//! provider is known by sets them once, where nothing is set yet.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests unwrap what they just made"
)]

use paperpaw_plugin_mail::native_model::account::{OWN, PRESETS, Values, key};
use paperpaw_plugin_mail::native_model::mem::{Error, Secret};
use paperpaw_plugin_mail::native_model::provider::{Set, after_change, parse};
use std::cell::Cell;
use std::collections::BTreeMap;

#[derive(Default)]
struct Fake {
    text: BTreeMap<String, String>,
    /// The read that is refused (1-based), and how many were made.
    refuse: Cell<usize>,
    reads: Cell<usize>,
}

impl Values for Fake {
    fn text(&self, id: &str) -> Result<String, Error> {
        let n = self.reads.get() + 1;
        self.reads.set(n);
        if n == self.refuse.get() {
            return Err(Error::NoMemory);
        }
        Ok(self.text.get(id).cloned().unwrap_or_default())
    }
    fn int(&self, _: &str) -> Result<i64, Error> {
        Ok(0)
    }
    fn secret(&self, _: &str) -> Result<Option<Secret>, Error> {
        Ok(None)
    }
}

fn set(f: &mut Fake, id: &str, value: &str) {
    f.text.insert(id.into(), value.into());
}

fn text(value: &str) -> Set {
    Set::Text(value.into())
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
    set(&mut f, "mail.1.provider", "mail.provider.gmx");
    let changes = after_change(&f, "mail.1.provider").unwrap();
    assert!(changes.contains(&("mail.1.imap-server".into(), text("imap.gmx.net"))));
    assert!(changes.contains(&("mail.1.smtp-port".into(), Set::Int(465))));
    // A server of another provider is not the one trusted before.
    assert!(changes.contains(&("mail.1.imap-pin".into(), text(""))));
    assert!(changes.contains(&("mail.1.smtp-pin".into(), text(""))));
    // The reader's own servers are left alone.
    set(&mut f, "mail.1.provider", OWN);
    assert!(after_change(&f, "mail.1.provider").unwrap().is_empty());
}

#[test]
fn every_provider_sets_all_of_its_servers() {
    for preset in &PRESETS {
        let mut f = Fake::default();
        set(&mut f, &key(3, "provider").unwrap(), preset.key);
        let changes = after_change(&f, "mail.3.provider").unwrap();
        assert_eq!(changes.len(), 8, "{}", preset.key);
        assert!(changes.contains(&("mail.3.imap-server".into(), text(preset.imap.0))));
        assert!(changes.contains(&("mail.3.smtp-server".into(), text(preset.smtp.0))));
    }
}

#[test]
fn a_known_address_sets_the_provider_once() {
    let mut f = Fake::default();
    set(&mut f, "mail.2.address", "ada@web.de");
    set(&mut f, "mail.2.provider", OWN);
    let changes = after_change(&f, "mail.2.address").unwrap();
    assert!(changes.contains(&("mail.2.provider".into(), text("mail.provider.webde"))));
    assert!(changes.contains(&(
        "mail.2.smtp-security".into(),
        text("mail.security.starttls")
    )));
    // With servers set already an address changes nothing.
    set(&mut f, "mail.2.imap-server", "imap.example.org");
    assert!(after_change(&f, "mail.2.address").unwrap().is_empty());
    // Nor does one nobody knows.
    set(&mut f, "mail.2.imap-server", "");
    set(&mut f, "mail.2.address", "ada@example.org");
    assert!(after_change(&f, "mail.2.address").unwrap().is_empty());
}

#[test]
fn other_settings_and_the_empty_key_set_nothing() {
    let f = Fake::default();
    for id in [
        "",
        "mail.fetch",
        "mail.1.name",
        "mail.1.imap-server",
        "core.x",
    ] {
        assert!(after_change(&f, id).unwrap().is_empty(), "{id}");
    }
}

#[test]
fn a_refused_read_is_an_error_not_an_empty_answer() {
    for at in 1..=3 {
        let mut f = Fake::default();
        set(&mut f, "mail.1.address", "ada@gmx.net");
        set(&mut f, "mail.1.provider", OWN);
        f.refuse.set(at);
        assert_eq!(
            after_change(&f, "mail.1.address"),
            Err(Error::NoMemory),
            "read {at}"
        );
    }
}
