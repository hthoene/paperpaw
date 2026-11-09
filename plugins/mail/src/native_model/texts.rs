//! What the screens say about a connection, as catalog keys: where it is, and
//! why it failed. The words come from the host's catalog (`i18n.format`); this
//! picks which and with what, so that it can be tested without a host.
use super::session::{Failure, Stage};
use paperpaw_sdk::native::sockets::Reason;

/// A message of the catalog, and the values it takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Say<'a> {
    /// A message that takes nothing.
    Key(&'static str),
    /// A message with one text, `{name}`.
    One {
        /// The key.
        key: &'static str,
        /// The name in the message.
        name: &'static str,
        /// The text.
        text: &'a str,
    },
    /// A message with two texts, `{first}` and `{second}`.
    Two {
        /// The key.
        key: &'static str,
        /// The first name and text.
        first: (&'static str, &'a str),
        /// The second name and text.
        second: (&'static str, &'a str),
    },
}

/// Where a connection is, in words.
#[must_use]
pub const fn stage(stage: Stage) -> &'static str {
    match stage {
        Stage::Connecting => "mail.stage.connecting",
        Stage::LoggingIn => "mail.stage.logging-in",
        Stage::Folders => "mail.stage.folders",
        Stage::Headers => "mail.stage.headers",
        Stage::Message => "mail.stage.message",
        Stage::Updating => "mail.stage.updating",
        Stage::Sending => "mail.stage.sending",
    }
}

/// Why a connection failed, kindly, and what to do.
#[must_use]
pub fn failure(failure: &Failure) -> Say<'_> {
    let key = match failure {
        Failure::Network(
            Reason::NoNetwork | Reason::Offline | Reason::Saving | Reason::NetworkOff,
        ) => "mail.fail.no-network",
        Failure::Network(Reason::Dns) => "mail.fail.dns",
        Failure::Network(Reason::Refused) => "mail.fail.refused",
        Failure::Network(Reason::Timeout) => "mail.fail.timeout",
        Failure::Network(_) => "mail.fail.lost",
        Failure::Secure(Reason::Name) => "mail.fail.name",
        Failure::Secure(Reason::Expired) => "mail.fail.expired",
        Failure::Secure(_) => "mail.fail.secure",
        Failure::NoTls => "mail.fail.no-tls",
        Failure::ClockNotSet => "mail.fail.clock",
        Failure::Busy => "mail.fail.busy",
        Failure::NoMemory => "mail.fail.memory",
        Failure::Login(words) => return words_of("mail.fail.login", words),
        Failure::Refused(words) => return words_of("mail.fail.server", words),
        Failure::Temporary(words) => return words_of("mail.fail.later", words),
        Failure::Recipient(who, words) => {
            return Say::Two {
                key: "mail.fail.recipient",
                first: ("who", who),
                second: ("words", words),
            };
        }
        Failure::Unsafe => "mail.fail.unsafe",
        Failure::Storage => "mail.fail.storage",
        _ => "mail.fail.protocol",
    };
    Say::Key(key)
}

fn words_of<'a>(key: &'static str, words: &'a str) -> Say<'a> {
    Say::One {
        key,
        name: "words",
        text: words,
    }
}

/// Why a certificate is not trusted: the line under the question.
#[must_use]
pub const fn distrust(why: paperpaw_sdk::native::sockets::Untrusted) -> &'static str {
    match why {
        paperpaw_sdk::native::sockets::Untrusted::NotPinned => "mail.pin.why.not-pinned",
        _ => "mail.pin.why.unknown-issuer",
    }
}
