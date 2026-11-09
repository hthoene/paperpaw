//! What a failed connection says: each failure picks its message of the
//! catalog, and the messages that take words get them.
#![allow(clippy::unwrap_used, reason = "tests unwrap what they just made")]

use paperpaw_plugin_mail::native_model::session::{Failure, Stage};
use paperpaw_plugin_mail::native_model::texts::{Say, distrust, failure, stage};
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
