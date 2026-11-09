//! What a failed connection says: each failure picks its message of the
//! catalog, and the messages that take words get them.
#![allow(clippy::unwrap_used, reason = "tests unwrap what they just made")]

use paperpaw_plugin_mail::native_model::account::Offer;
use paperpaw_plugin_mail::native_model::session::{Failure, Stage};
use paperpaw_plugin_mail::native_model::texts::{Say, distrust, failure, pin_message, stage};
use paperpaw_sdk::native::sockets::{Reason, Untrusted};

#[test]
fn a_failure_picks_its_message() {
    let key = |f: Failure| match failure(&f) {
        Say::Key(key) | Say::One { key, .. } | Say::Two { key, .. } => key,
    };
    assert_eq!(
        key(Failure::Network(Reason::NoNetwork)),
        "mail.fail.no-network"
    );
    assert_eq!(
        key(Failure::Network(Reason::Offline)),
        "mail.fail.no-network"
    );
    assert_eq!(key(Failure::Network(Reason::Dns)), "mail.fail.dns");
    assert_eq!(key(Failure::Network(Reason::Refused)), "mail.fail.refused");
    assert_eq!(key(Failure::Network(Reason::Timeout)), "mail.fail.timeout");
    assert_eq!(key(Failure::Network(Reason::Lost)), "mail.fail.lost");
    assert_eq!(key(Failure::Secure(Reason::Name)), "mail.fail.name");
    assert_eq!(key(Failure::Secure(Reason::Expired)), "mail.fail.expired");
    assert_eq!(key(Failure::Secure(Reason::Tls)), "mail.fail.secure");
    assert_eq!(key(Failure::ClockNotSet), "mail.fail.clock");
    assert_eq!(key(Failure::Protocol), "mail.fail.protocol");
    assert_eq!(key(Failure::Storage), "mail.fail.storage");
}

#[test]
fn the_server_s_words_are_passed_on() {
    let login = Failure::Login("bad".into());
    assert_eq!(
        failure(&login),
        Say::One {
            key: "mail.fail.login",
            name: "words",
            text: "bad"
        }
    );
    let recipient = Failure::Recipient("bob@x".into(), "no".into());
    assert_eq!(
        failure(&recipient),
        Say::Two {
            key: "mail.fail.recipient",
            first: ("who", "bob@x"),
            second: ("words", "no")
        }
    );
}

#[test]
fn every_stage_and_distrust_has_a_message() {
    for (s, key) in [
        (Stage::Connecting, "mail.stage.connecting"),
        (Stage::LoggingIn, "mail.stage.logging-in"),
        (Stage::Folders, "mail.stage.folders"),
        (Stage::Headers, "mail.stage.headers"),
        (Stage::Message, "mail.stage.message"),
        (Stage::Updating, "mail.stage.updating"),
        (Stage::Sending, "mail.stage.sending"),
    ] {
        assert_eq!(stage(s), key);
    }
    assert_eq!(distrust(Untrusted::NotPinned), "mail.pin.why.not-pinned");
    assert_eq!(
        distrust(Untrusted::UnknownIssuer),
        "mail.pin.why.unknown-issuer"
    );
    assert_eq!(distrust(Untrusted::Other(9)), "mail.pin.why.unknown-issuer");
}

/// Every `"mail.…"` string in `source`.
fn keys_in(source: &str) -> Vec<String> {
    let mut found = Vec::new();
    for part in source.split('"').skip(1).step_by(2) {
        if part.starts_with("mail.")
            && part.len() > 5
            && part
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'-')
        {
            found.push(part.to_string());
        }
    }
    found
}

#[test]
fn every_key_the_native_screens_use_is_in_both_languages() {
    let en = include_str!("../i18n/en.ini");
    let de = include_str!("../i18n/de.ini");
    let has = |ini: &str, key: &str| {
        ini.lines()
            .any(|line| line.split('=').next().is_some_and(|k| k.trim() == key))
    };
    let mut checked = 0;
    for source in [
        include_str!("../src/native_model/texts.rs"),
        include_str!("../src/native_model/browser.rs"),
        include_str!("../src/native_screens.rs"),
        include_str!("../src/native_words.rs"),
        include_str!("../src/native_reader.rs"),
        include_str!("../src/native_model/reader.rs"),
        include_str!("../src/native_writer.rs"),
        include_str!("../src/native_model/writer.rs"),
    ] {
        for key in keys_in(source) {
            // Ids of screens and actions, not messages.
            if [
                "mail.boxes",
                "mail.list",
                "mail.read",
                "mail.compose",
                "mail.open",
            ]
            .contains(&key.as_str())
            {
                continue;
            }
            assert!(has(en, &key), "{key} is not in en.ini");
            assert!(has(de, &key), "{key} is not in de.ini");
            checked += 1;
        }
    }
    assert!(checked > 30, "found only {checked} keys");
}

#[test]
fn an_untrusted_server_is_said_by_what_may_be_offered() {
    // Where the list asks, a reader or the compose window says to open it; a
    // changed key or a provider's server must not be told to "trust".
    assert_eq!(pin_message(Offer::Ask), "mail.pin.open-list");
    assert_eq!(pin_message(Offer::Changed), "mail.pin.changed");
    assert_eq!(pin_message(Offer::Attack), "mail.pin.attack");
}
