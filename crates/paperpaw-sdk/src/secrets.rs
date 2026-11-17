//! Passwords and keys, kept in one place for every plugin: plugins read
//! them with `Env::secrets` and keep or remove them with
//! [`Command::SetSecret`](crate::Command::SetSecret); the settings list
//! them all under *Passwords and keys* to change or remove.
//!
//! They are kept in a file of their own (`.paperpaw/secrets.txt`), out of
//! `paperpaw.ini` and not readable at a glance. That is no encryption:
//! the devices have no safe place for a key, so whoever has the files
//! can read them.
//!
//! A plugin sees its own secrets alone, those in its namespace
//! (`kosync.key` for `kosync`): `Env::secrets` is a
//! [`Visible`] view that hands out a value only to the plugin whose code
//! runs. Every plugin may see which secrets there are, by name and label
//! ([`Visible::listed`]), as the settings' list of them shows. Code a
//! plugin hands another to run (a widget or a status item drawn on its
//! screen) runs as the plugin of that screen, so it sees that plugin's
//! secrets, not its own: such code keeps its secrets out of drawing.
//! Native code
//! can still go past this (plugins are not sandboxed); the SDK hands no
//! plugin another's secret.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// Where the secrets are kept.
pub const FILE: &str = crate::data_path!("secrets.txt");

/// Mixed into the stored values, so they are not read at a glance. It
/// keeps the old name, so the files written before stay readable.
const VEIL: &[u8] = b"QuireInk keeps this out of sight, not safe"; // not PaperPaw

/// A password or key. Printed for debugging without its value.
#[derive(Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Secret {
    /// Who keeps it and for what, e.g. `translate.deepl-key`.
    pub id: String,
    /// What it is, shown in the list: a translation key or the text.
    pub label: String,
    pub value: String,
    /// It can be typed anew in the list (not, e.g., a hash of one).
    pub changeable: bool,
}

impl core::fmt::Debug for Secret {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Secret")
            .field("id", &self.id)
            .field("label", &self.label)
            .field("value", &"…")
            .field("changeable", &self.changeable)
            .finish()
    }
}

/// Which plugin's code runs now, as the kernel knows it: what a
/// [`Visible`] view of the secrets asks.
pub trait Running {
    /// Whether the code of `plugin` (a namespace, e.g. `kosync`) runs now.
    fn runs(&self, plugin: &str) -> bool;
}

/// Any plugin runs: for tools and tests that see every secret.
#[derive(Debug, Clone, Copy, Default)]
pub struct Anyone;

impl Running for Anyone {
    fn runs(&self, _: &str) -> bool {
        true
    }
}

/// The secrets as the plugin running sees them: its own values, and of
/// every secret its name and label (see the [module](self)).
#[derive(Clone, Copy)]
pub struct Visible<'a> {
    all: &'a Secrets,
    running: &'a dyn Running,
}

impl core::fmt::Debug for Visible<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Visible").finish_non_exhaustive()
    }
}

/// A secret as everyone may see it: its name and label, not its value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Listed<'a> {
    pub id: &'a str,
    pub label: &'a str,
    /// It can be typed anew in the list.
    pub changeable: bool,
}

impl<'a> Visible<'a> {
    /// `all` as the plugin `running` says runs sees them.
    pub fn new(all: &'a Secrets, running: &'a dyn Running) -> Self {
        Self { all, running }
    }

    /// Whether secret `id` is the running plugin's: in its namespace.
    fn mine(&self, id: &str) -> bool {
        let plugin = id.split('.').next().unwrap_or_default();
        !plugin.is_empty() && self.running.runs(plugin)
    }

    /// The value of the running plugin's secret `id`, if kept; `None` for
    /// another plugin's.
    pub fn get(&self, id: &str) -> Option<&'a str> {
        if self.mine(id) {
            self.all.get(id)
        } else {
            None
        }
    }

    /// The running plugin's secrets.
    pub fn iter(&self) -> impl Iterator<Item = &'a Secret> + '_ {
        self.all.iter().filter(|s| self.mine(&s.id))
    }

    /// Whether the running plugin keeps no secret.
    pub fn is_empty(&self) -> bool {
        self.iter().next().is_none()
    }

    /// Every secret, by name and label, without its value: what the
    /// settings' list of them shows.
    pub fn listed(&self) -> impl Iterator<Item = Listed<'a>> + 'a {
        self.all.iter().map(|s| Listed {
            id: &s.id,
            label: &s.label,
            changeable: s.changeable,
        })
    }

    /// Whether secret `id` is kept, whoever keeps it.
    pub fn has(&self, id: &str) -> bool {
        self.all.get(id).is_some()
    }

    /// Whether secret `id` is on the card as it is here (see
    /// [`Secrets::saved`]): a plugin removes its own copy of a secret only
    /// once it is.
    pub fn saved(&self, id: &str) -> bool {
        self.all.saved(id)
    }
}

/// A change: `value` `None` removes the secret. Printed for debugging
/// without the value.
#[derive(Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SecretChange {
    pub id: String,
    pub label: String,
    pub value: Option<String>,
    pub changeable: bool,
}

impl core::fmt::Debug for SecretChange {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SecretChange")
            .field("id", &self.id)
            .field("label", &self.label)
            .field("value", &self.value.as_ref().map(|_| "…"))
            .field("changeable", &self.changeable)
            .finish()
    }
}

impl SecretChange {
    /// Keeps `value` as secret `id`, shown as `label`.
    pub fn keep(id: &str, label: &str, value: &str) -> Self {
        Self {
            id: id.to_string(),
            label: label.to_string(),
            value: Some(value.to_string()),
            changeable: true,
        }
    }

    /// Whether the reader may type it anew in the list of passwords and
    /// keys (`false` for a key or a hash).
    pub fn changeable(mut self, changeable: bool) -> Self {
        self.changeable = changeable;
        self
    }

    /// Removes secret `id`.
    pub fn remove(id: &str) -> Self {
        Self {
            id: id.to_string(),
            label: String::new(),
            value: None,
            changeable: false,
        }
    }
}

/// All secrets.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Secrets {
    list: Vec<Secret>,
    /// Lines of the file this version does not read (a newer format's, or
    /// damaged): written back as they are.
    unread: Unread,
    /// Secrets changed here but not in the file: it could not be read or
    /// written.
    unsaved: Vec<String>,
}

/// Lines of the file kept as they are. Printed for debugging by number
/// alone, as they may hold values.
#[derive(Default, Clone, PartialEq, Eq)]
struct Unread(Vec<String>);

impl core::fmt::Debug for Unread {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{} lines", self.0.len())
    }
}

fn veil(data: &[u8]) -> Vec<u8> {
    data.iter()
        .enumerate()
        .map(|(i, b)| b ^ VEIL[i % VEIL.len()])
        .collect()
}

use paperpaw_digest::hex::{decode as unhex, encode as hex};

impl Secrets {
    /// The value of secret `id`, if kept.
    pub fn get(&self, id: &str) -> Option<&str> {
        self.list
            .iter()
            .find(|s| s.id == id)
            .map(|s| s.value.as_str())
    }

    pub fn iter(&self) -> impl Iterator<Item = &Secret> {
        self.list.iter()
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// Whether secret `id` is kept (or removed) on the card as it is here:
    /// a change that could not be written lasts only until the device
    /// restarts. What a plugin keeps elsewhere is removed only once it is.
    pub fn saved(&self, id: &str) -> bool {
        !self.unsaved.iter().any(|unsaved| unsaved == id)
    }

    /// Notes that the change to secret `id` could not be written.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn mark_unsaved(&mut self, id: &str) {
        if self.saved(id) {
            self.unsaved.push(id.to_string());
        }
    }

    /// Notes that every secret here is written.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn mark_saved(&mut self) {
        self.unsaved.clear();
    }

    /// Applies `change`.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn apply(&mut self, change: SecretChange) {
        // An unread line of the same secret gives way to the change.
        self.unread
            .0
            .retain(|line| unescape(line.split('\t').next().unwrap_or_default()) != change.id);
        let at = self.list.iter().position(|s| s.id == change.id);
        match (change.value, at) {
            (Some(value), Some(at)) => {
                let secret = &mut self.list[at];
                secret.value = value;
                if !change.label.is_empty() {
                    secret.label = change.label;
                }
                secret.changeable = change.changeable;
            }
            (Some(value), None) => self.list.push(Secret {
                id: change.id,
                label: change.label,
                value,
                changeable: change.changeable,
            }),
            (None, Some(at)) => {
                self.list.remove(at);
            }
            (None, None) => {}
        }
    }

    /// The file's text: `id<TAB>label<TAB>flags<TAB>value` lines, the
    /// value veiled, tabs, line breaks and backslashes in the id and the
    /// label written as `\t`, `\n`, `\r` and `\\` (a network's name may
    /// hold any of them).
    /// Lines this version does not read are written back as they are.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn encode(&self) -> String {
        let mut text: String = self
            .list
            .iter()
            .map(|s| {
                alloc::format!(
                    "{}\t{}\t{}\t{}\n",
                    escape(&s.id),
                    escape(&s.label),
                    if s.changeable { "c" } else { "" },
                    hex(&veil(s.value.as_bytes()))
                )
            })
            .collect();
        for line in &self.unread.0 {
            text.push_str(line);
            text.push('\n');
        }
        text
    }

    /// Lines it cannot read (more fields or flags than it knows, or
    /// damaged) are kept for [`Secrets::encode`], not dropped.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn decode(text: &str) -> Self {
        let mut secrets = Self::default();
        for line in text.lines() {
            let line = line.trim_end_matches('\r');
            if line.trim().is_empty() {
                continue;
            }
            match Self::decode_line(line) {
                Some(secret) => secrets.list.push(secret),
                None => secrets.unread.0.push(line.to_string()),
            }
        }
        secrets
    }

    fn decode_line(line: &str) -> Option<Secret> {
        let mut f = line.splitn(4, '\t');
        let id = unescape(f.next()?);
        let label = unescape(f.next()?);
        let changeable = match f.next()? {
            "c" => true,
            "" => false,
            _ => return None,
        };
        let value = String::from_utf8(veil(&unhex(f.next()?)?)).ok()?;
        (!id.is_empty()).then_some(Secret {
            id,
            label,
            value,
            changeable,
        })
    }
}

/// `text` on one line of the file (see [`Secrets::encode`]).
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c => out.push(c),
        }
    }
    out
}

/// The other way round from [`escape`]; any other `\` stays, as files
/// from before kept it.
fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        let escaped = match (c, chars.peek()) {
            ('\\', Some('\\')) => Some('\\'),
            ('\\', Some('t')) => Some('\t'),
            ('\\', Some('n')) => Some('\n'),
            ('\\', Some('r')) => Some('\r'),
            _ => None,
        };
        match escaped {
            Some(e) => {
                chars.next();
                out.push(e);
            }
            None => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_with_tabs_and_breaks_stay_as_they_are() {
        let mut secrets = Secrets::default();
        for id in [
            "wifi.Home\tNet",
            "wifi.Two\nlines\r",
            "wifi.back\\slash\\t",
            "x\\y",
        ] {
            secrets.apply(SecretChange::keep(id, "Label\tlabel", "pw"));
        }
        let text = secrets.encode();
        assert_eq!(text.lines().count(), 4);
        assert_eq!(Secrets::decode(&text), secrets);
        // Written before, a lone backslash stays.
        assert_eq!(unescape("a\\b"), "a\\b");
        // Values are not printed.
        let printed = alloc::format!("{secrets:?} {:?}", SecretChange::keep("a", "b", "s3cr3t"));
        assert!(
            !printed.contains("pw") && !printed.contains("s3cr3t"),
            "{printed}"
        );
    }

    #[test]
    fn kept_changed_removed_and_veiled() {
        let mut secrets = Secrets::default();
        secrets.apply(SecretChange::keep("a.key", "a.label", "s3cr\u{e9}t:fx"));
        secrets.apply(SecretChange::keep("b.key", "Catalog: B", "pw"));
        let text = secrets.encode();
        assert!(!text.contains("s3cr"));
        let read = Secrets::decode(&text);
        assert_eq!(read.get("a.key"), Some("s3cr\u{e9}t:fx"));
        assert_eq!(read, secrets);
        secrets.apply(SecretChange::keep("a.key", "", "new"));
        assert_eq!(secrets.get("a.key"), Some("new"));
        assert_eq!(
            secrets.iter().next().map(|s| s.label.as_str()),
            Some("a.label")
        );
        secrets.apply(SecretChange::remove("a.key"));
        assert_eq!(secrets.get("a.key"), None);
        assert_eq!(secrets.iter().count(), 1);
    }
}
