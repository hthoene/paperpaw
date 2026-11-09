//! Accounts on interface 7: what the reader sets in the settings (a provider to
//! start from, the address, the servers) and in the secrets (the password) read
//! into one value, and the providers' servers as presets. The interface-6
//! `account` made fallible: nothing here allocates without asking, the password
//! is a [`Secret`] that wipes itself, and an account that cannot be read for
//! want of memory is an error, not an account that is not there.
//!
//! Providers appear here as presets, a server and a port each, and nowhere as a
//! name of this plugin.
use super::mem::{Error, Secret, copy};
use super::session::{Endpoint, Security};
use alloc::string::String;
use alloc::vec::Vec;

/// The most accounts (`store::MAX_ACCOUNTS`).
pub const MAX: u8 = 3;

/// The id of an account's setting (or secret): `mail.2.address`.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn key(number: u8, field: &str) -> Result<String, Error> {
    super::mem::string_with(9 + field.len()).and_then(|mut out| {
        super::mem::push_str(&mut out, "mail.")?;
        super::mem::push_char(&mut out, char::from(b'0'.saturating_add(number.min(9))))?;
        super::mem::push_char(&mut out, '.')?;
        super::mem::push_str(&mut out, field)?;
        Ok(out)
    })
}

/// The name of an account's secret: the setting's id without the plugin's
/// prefix (`mail.2.password` is the secret `2.password`; docs/abi/secrets.md 4).
#[must_use]
pub fn secret_name(id: &str) -> &str {
    id.strip_prefix("mail.").unwrap_or(id)
}

/// The fields of an account's settings.
pub mod field {
    pub const PROVIDER: &str = "provider";
    pub const ADDRESS: &str = "address";
    pub const NAME: &str = "name";
    pub const USER: &str = "user";
    pub const PASSWORD: &str = "password";
    pub const IMAP_HOST: &str = "imap-server";
    pub const IMAP_PORT: &str = "imap-port";
    pub const IMAP_SECURITY: &str = "imap-security";
    pub const IMAP_PIN: &str = "imap-pin";
    pub const SMTP_HOST: &str = "smtp-server";
    pub const SMTP_PORT: &str = "smtp-port";
    pub const SMTP_SECURITY: &str = "smtp-security";
    pub const SMTP_PIN: &str = "smtp-pin";
}

/// The values of `security` settings.
pub const IMPLICIT: &str = "mail.security.implicit";
pub const STARTTLS: &str = "mail.security.starttls";
pub const SECURITIES: [&str; 2] = [IMPLICIT, STARTTLS];

/// The setting's value for `security`.
#[must_use]
pub const fn security_key(security: Security) -> &'static str {
    match security {
        Security::StartTls => STARTTLS,
        _ => IMPLICIT,
    }
}

/// The security a setting's value stands for; implicit TLS for any other
/// (never plain).
#[must_use]
pub fn security_of(key: &str) -> Security {
    if key == STARTTLS {
        Security::StartTls
    } else {
        Security::Implicit
    }
}

/// A provider's servers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Preset {
    /// The translation key of its name, and the value of the provider
    /// setting.
    pub key: &'static str,
    pub imap: (&'static str, u16, Security),
    pub smtp: (&'static str, u16, Security),
}

const fn preset(
    key: &'static str,
    imap: (&'static str, u16, Security),
    smtp: (&'static str, u16, Security),
) -> Preset {
    Preset { key, imap, smtp }
}

use Security::{Implicit, StartTls};

/// The value of the provider setting that sets nothing.
pub const OWN: &str = "mail.provider.own";

/// The providers to start from. Their servers are public; the reader still
/// needs the password (often an app password) the provider asks for.
pub const PRESETS: [Preset; 10] = [
    preset(
        "mail.provider.gmail",
        ("imap.gmail.com", 993, Implicit),
        ("smtp.gmail.com", 465, Implicit),
    ),
    preset(
        "mail.provider.outlook",
        ("outlook.office365.com", 993, Implicit),
        ("smtp-mail.outlook.com", 587, StartTls),
    ),
    preset(
        "mail.provider.icloud",
        ("imap.mail.me.com", 993, Implicit),
        ("smtp.mail.me.com", 587, StartTls),
    ),
    preset(
        "mail.provider.yahoo",
        ("imap.mail.yahoo.com", 993, Implicit),
        ("smtp.mail.yahoo.com", 465, Implicit),
    ),
    preset(
        "mail.provider.gmx",
        ("imap.gmx.net", 993, Implicit),
        ("mail.gmx.net", 465, Implicit),
    ),
    preset(
        "mail.provider.webde",
        ("imap.web.de", 993, Implicit),
        ("smtp.web.de", 587, StartTls),
    ),
    preset(
        "mail.provider.tonline",
        ("secureimap.t-online.de", 993, Implicit),
        ("securesmtp.t-online.de", 465, Implicit),
    ),
    preset(
        "mail.provider.posteo",
        ("posteo.de", 993, Implicit),
        ("posteo.de", 465, Implicit),
    ),
    preset(
        "mail.provider.mailbox",
        ("imap.mailbox.org", 993, Implicit),
        ("smtp.mailbox.org", 465, Implicit),
    ),
    preset(
        "mail.provider.fastmail",
        ("imap.fastmail.com", 993, Implicit),
        ("smtp.fastmail.com", 465, Implicit),
    ),
];

/// The provider setting's options: the reader's own servers, then each
/// preset.
pub const PROVIDERS: [&str; 11] = [
    OWN,
    "mail.provider.gmail",
    "mail.provider.outlook",
    "mail.provider.icloud",
    "mail.provider.yahoo",
    "mail.provider.gmx",
    "mail.provider.webde",
    "mail.provider.tonline",
    "mail.provider.posteo",
    "mail.provider.mailbox",
    "mail.provider.fastmail",
];

/// The preset for a provider setting's value.
#[must_use]
pub fn preset_of(provider: &str) -> Option<&'static Preset> {
    PRESETS.iter().find(|p| p.key == provider)
}

/// The preset a guess from an address's domain suggests (`ada@gmx.net`), for a
/// reader who typed the address first.
#[must_use]
pub fn preset_for_address(address: &str) -> Option<&'static Preset> {
    let domain = address.rsplit_once('@')?.1.trim();
    let is = |name: &str| domain.eq_ignore_ascii_case(name);
    let key = if is("gmail.com") || is("googlemail.com") {
        "mail.provider.gmail"
    } else if [
        "outlook.com",
        "hotmail.com",
        "live.com",
        "msn.com",
        "outlook.de",
        "hotmail.de",
    ]
    .iter()
    .any(|d| is(d))
    {
        "mail.provider.outlook"
    } else if ["icloud.com", "me.com", "mac.com"].iter().any(|d| is(d)) {
        "mail.provider.icloud"
    } else if ["yahoo.com", "yahoo.de", "ymail.com"].iter().any(|d| is(d)) {
        "mail.provider.yahoo"
    } else if ["gmx.de", "gmx.net", "gmx.at", "gmx.ch", "gmx.com"]
        .iter()
        .any(|d| is(d))
    {
        "mail.provider.gmx"
    } else if is("web.de") {
        "mail.provider.webde"
    } else if is("t-online.de") || is("magenta.de") {
        "mail.provider.tonline"
    } else if is("posteo.de") || is("posteo.net") {
        "mail.provider.posteo"
    } else if is("mailbox.org") {
        "mail.provider.mailbox"
    } else if is("fastmail.com") || is("fastmail.fm") {
        "mail.provider.fastmail"
    } else {
        return None;
    };
    preset_of(key)
}

/// What to do about a server whose certificate no authority vouches for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Offer {
    /// An own server, trusted before by no key: ask once.
    Ask,
    /// A key was trusted and is another now: no connection, and no way to
    /// trust the new one with a tap (clear the old one in the settings).
    Changed,
    /// A provider's server (its certificate is one an authority vouches for):
    /// someone else is answering. No connection, no offer.
    Attack,
}

/// What may be offered to the reader for the account's IMAP (`smtp` false) or
/// SMTP server that was not trusted.
///
/// # Errors
/// [`Error::NoMemory`] and what the host says.
pub fn offer(values: &dyn Values, number: u8, smtp: bool) -> Result<Offer, Error> {
    // A provider's server, as the preset names it: a host the reader changed
    // to their own is theirs to trust.
    let host = trimmed(
        values,
        number,
        if smtp {
            field::SMTP_HOST
        } else {
            field::IMAP_HOST
        },
    )?;
    let provider = trimmed(values, number, field::PROVIDER)?;
    if let Some(p) = preset_of(&provider)
        && host == if smtp { p.smtp.0 } else { p.imap.0 }
    {
        return Ok(Offer::Attack);
    }
    let pin = trimmed(
        values,
        number,
        if smtp {
            field::SMTP_PIN
        } else {
            field::IMAP_PIN
        },
    )?;
    Ok(if pin.is_empty() {
        Offer::Ask
    } else {
        Offer::Changed
    })
}

/// A key pin as the setting keeps it: 64 hex digits.
#[must_use]
pub fn pin_of(text: &str) -> Option<[u8; 32]> {
    let text = text.trim();
    if text.len() != 64 || !text.is_ascii() {
        return None;
    }
    let mut out = [0u8; 32];
    for (byte, pair) in out.iter_mut().zip(text.as_bytes().chunks(2)) {
        let high = char::from(*pair.first()?).to_digit(16)?;
        let low = char::from(*pair.get(1)?).to_digit(16)?;
        *byte = u8::try_from(high * 16 + low).ok()?;
    }
    Some(out)
}

/// A key pin as the setting keeps it.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn pin_text(pin: &[u8; 32]) -> Result<String, Error> {
    let mut out = super::mem::string_with(64)?;
    for byte in pin {
        for digit in [byte >> 4, byte & 15] {
            super::mem::push_char(
                &mut out,
                char::from_digit(u32::from(digit), 16).unwrap_or('0'),
            )?;
        }
    }
    Ok(out)
}

/// One of an account's servers.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Server {
    pub host: String,
    pub port: u16,
    pub security: Security,
    /// The key the reader trusted (SHA-256 of the key; see
    /// [`super::session::Event::Untrusted`]).
    pub pin: Option<[u8; 32]>,
}

impl Server {
    /// Where to connect, as whom.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn endpoint(&self, user: &str, password: Secret) -> Result<Endpoint, Error> {
        let mut endpoint = Endpoint::new(
            copy(&self.host)?,
            self.port,
            self.security,
            copy(user)?,
            password,
        );
        endpoint.pin = self.pin;
        Ok(endpoint)
    }
}

/// An account, as set: everything but the password, which is read from the
/// secrets when a connection is made and not kept.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Account {
    pub number: u8,
    pub address: String,
    /// The name others see in From.
    pub name: String,
    /// What to log in as: the address unless set otherwise.
    pub user: String,
    pub imap: Server,
    pub smtp: Server,
}

/// Where an account's values are read from: the settings and the secrets, or a
/// test's.
pub trait Values {
    /// A text setting (empty where unset).
    ///
    /// # Errors
    /// [`Error::NoMemory`] and what the host says.
    fn text(&self, id: &str) -> Result<String, Error>;
    /// A whole-number setting (0 where unset).
    ///
    /// # Errors
    /// What the host says.
    fn int(&self, id: &str) -> Result<i64, Error>;
    /// A secret of this plugin, if there is one.
    ///
    /// # Errors
    /// [`Error::NoMemory`] and what the host says; `Unsupported` where the
    /// device keeps no secrets.
    fn secret(&self, id: &str) -> Result<Option<Secret>, Error>;
}

fn trimmed(values: &dyn Values, number: u8, name: &str) -> Result<String, Error> {
    let text = values.text(&key(number, name)?)?;
    copy(text.trim())
}

impl Account {
    /// Account `number` if it has what it needs to log in: an address, a
    /// password (its presence is checked, not read out) and the servers.
    ///
    /// # Errors
    /// [`Error::NoMemory`] and what the host says; none for an account that is
    /// merely not set up.
    pub fn read(values: &dyn Values, number: u8) -> Result<Option<Self>, Error> {
        let address = trimmed(values, number, field::ADDRESS)?;
        let server =
            |host: &str, port: &str, security: &str, pin: &str| -> Result<Option<Server>, Error> {
                let port = u16::try_from(values.int(&key(number, port)?)?)
                    .ok()
                    .filter(|p| *p != 0);
                let host = trimmed(values, number, host)?;
                let (Some(port), false) = (port, host.is_empty()) else {
                    return Ok(None);
                };
                Ok(Some(Server {
                    host,
                    port,
                    security: security_of(&trimmed(values, number, security)?),
                    pin: pin_of(&trimmed(values, number, pin)?),
                }))
            };
        let Some(imap) = server(
            field::IMAP_HOST,
            field::IMAP_PORT,
            field::IMAP_SECURITY,
            field::IMAP_PIN,
        )?
        else {
            return Ok(None);
        };
        let Some(smtp) = server(
            field::SMTP_HOST,
            field::SMTP_PORT,
            field::SMTP_SECURITY,
            field::SMTP_PIN,
        )?
        else {
            return Ok(None);
        };
        let has_password = values
            .secret(&key(number, field::PASSWORD)?)?
            .is_some_and(|p| !p.as_str().is_empty());
        if address.is_empty() || !has_password {
            return Ok(None);
        }
        let user = trimmed(values, number, field::USER)?;
        let user = if user.is_empty() {
            copy(&address)?
        } else {
            user
        };
        Ok(Some(Self {
            number,
            name: trimmed(values, number, field::NAME)?,
            address,
            user,
            imap,
            smtp,
        }))
    }

    /// The password, read for a connection (and wiped when it is dropped).
    ///
    /// # Errors
    /// [`Error::NotFound`] where there is none; what the host says.
    pub fn password(&self, values: &dyn Values) -> Result<Secret, Error> {
        values
            .secret(&key(self.number, field::PASSWORD)?)?
            .filter(|p| !p.as_str().is_empty())
            .ok_or(Error::NotFound)
    }

    /// The numbers of the accounts with an address, password or not: what a
    /// widget needs to know.
    ///
    /// # Errors
    /// [`Error::NoMemory`] and what the host says.
    pub fn numbers(values: &dyn Values) -> Result<Vec<u8>, Error> {
        let mut out = Vec::new();
        out.try_reserve_exact(usize::from(MAX))
            .map_err(|_| Error::NoMemory)?;
        for number in 1..=MAX {
            if !trimmed(values, number, field::ADDRESS)?.is_empty() {
                out.push(number);
            }
        }
        Ok(out)
    }

    /// Every account that is set up, in order.
    ///
    /// # Errors
    /// As [`Account::read`].
    pub fn all(values: &dyn Values) -> Result<Vec<Self>, Error> {
        let mut out = Vec::new();
        out.try_reserve_exact(usize::from(MAX))
            .map_err(|_| Error::NoMemory)?;
        for number in 1..=MAX {
            if let Some(account) = Self::read(values, number)? {
                out.push(account);
            }
        }
        Ok(out)
    }

    /// The part of the address before the `@`, as a short name.
    #[must_use]
    pub fn short(&self) -> &str {
        self.address.split('@').next().unwrap_or(&self.address)
    }
}
