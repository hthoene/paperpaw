//! A pair of folders: the card's, the server's, the user and the mode, read
//! from the settings (`file-sync.folder-1`, `server-1`, `user-1`, `mode-1`
//! and so on); the password is a secret (`password-1`).
use alloc::string::String;

use paperpaw_sdk::native::{
    Error, Host,
    settings::{self, SettingValue},
};
use paperpaw_ui_kit::{
    dav::url::folder_url,
    text::{copy, format},
};

use crate::declarations::{MODE, PAIRS};
use crate::native_plan::Mode;

/// A pair as the settings say it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pair {
    /// Its number, 1 to [`PAIRS`].
    pub n: usize,
    /// The card's folder without a `/` at either end; empty where not set.
    pub folder: String,
    /// The server's folder with a `/` at its end; empty where not set.
    pub url: String,
    pub user: String,
    pub mode: Mode,
}

impl Pair {
    /// Whether both sides are named.
    #[must_use]
    pub fn ready(&self) -> bool {
        !self.folder.is_empty() && !self.url.is_empty()
    }
}

/// The key of the setting `stem` of pair `n`: `file-sync.folder-1`.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn key(stem: &str, n: usize) -> Result<String, Error> {
    format(format_args!("file-sync.{stem}-{n}"))
}

/// The name of the pair's password among the secrets.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn secret_name(n: usize) -> Result<String, Error> {
    format(format_args!("password-{n}"))
}

/// What a typed password keeps: the password as typed, spaces too (the server
/// compares them); spaces only take the kept one away (empty).
#[must_use]
pub fn password_of(typed: &str) -> &str {
    if typed.trim().is_empty() { "" } else { typed }
}

/// Whether the address `new` names the server of `old` (its host and port):
/// a password typed for one server is never sent to another.
#[must_use]
pub fn same_server(old: &str, new: &str) -> bool {
    paperpaw_sdk::web::host(old).eq_ignore_ascii_case(paperpaw_sdk::web::host(new))
}

/// The card's folder a setting names: no `/` at either end, empty for none
/// and for a folder a sync must not touch (the card's root, the plugins and
/// the reader's own data, a path that leaves the folder).
///
/// # Errors
/// [`Error::NoMemory`].
pub fn folder_of(setting: &str) -> Result<String, Error> {
    let folder = setting.trim().trim_matches('/');
    let first = folder.split('/').next().unwrap_or("");
    let bad = folder.is_empty()
        || folder.len() > 400
        || folder.split('/').any(|p| matches!(p, "" | "." | ".."))
        || first.starts_with('.')
        || first.eq_ignore_ascii_case("plugins")
        || folder.chars().any(char::is_control);
    if bad { Ok(String::new()) } else { copy(folder) }
}

fn text(host: &Host<'_>, key: &str) -> Result<String, Error> {
    let mut scratch = [0; 1100];
    match settings::read_setting(host, key, &mut scratch)? {
        SettingValue::Text(text) => copy(text),
        _ => Ok(String::new()),
    }
}

/// Pair `n` as the settings say it.
///
/// # Errors
/// What reading a setting says; [`Error::NoMemory`].
pub fn read(host: &Host<'_>, n: usize) -> Result<Pair, Error> {
    let folder = folder_of(&text(host, &key("folder", n)?)?)?;
    let url = folder_url(&text(host, &key("server", n)?)?).unwrap_or_default();
    let user = text(host, &key("user", n)?)?;
    let mode = if text(host, &key("mode", n)?)? == MODE[1] {
        Mode::DownloadOnly
    } else {
        Mode::TwoWay
    };
    Ok(Pair {
        n,
        folder,
        url,
        user,
        mode,
    })
}

/// The pairs that name both sides.
///
/// # Errors
/// What reading a setting says; [`Error::NoMemory`].
pub fn ready(host: &Host<'_>) -> Result<alloc::vec::Vec<Pair>, Error> {
    let mut pairs = alloc::vec::Vec::new();
    for n in 1..=PAIRS {
        let pair = read(host, n)?;
        if pair.ready() {
            paperpaw_ui_kit::mem::push(&mut pairs, pair)?;
        }
    }
    Ok(pairs)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "test assertions")]
    use super::*;

    #[test]
    fn a_folder_of_the_card_is_one_a_sync_may_touch() {
        assert_eq!(folder_of(" /Notes/ ").unwrap(), "Notes");
        assert_eq!(folder_of("Books/Sci Fi").unwrap(), "Books/Sci Fi");
        for bad in ["", "/", ".paperpaw", "plugins/x", "A/../B", "a//b", ".."] {
            assert_eq!(folder_of(bad).unwrap(), "", "{bad}");
        }
    }

    #[test]
    fn a_password_keeps_its_spaces_and_spaces_alone_take_it_away() {
        assert_eq!(password_of(" pass word "), " pass word ");
        assert_eq!(password_of("  "), "");
        assert_eq!(password_of(""), "");
    }

    #[test]
    fn another_host_or_port_is_another_server() {
        let old = "https://cloud.example.org/remote.php/dav/files/me/";
        assert!(same_server(old, "https://Cloud.Example.org/dav/other/"));
        assert!(same_server(old, "http://cloud.example.org/x/"));
        assert!(!same_server(old, "https://evil.example.net/dav/"));
        assert!(!same_server(old, "https://cloud.example.org:8443/dav/"));
    }

    #[test]
    fn the_keys_name_the_pair() {
        assert_eq!(key("folder", 2).unwrap(), "file-sync.folder-2");
        assert_eq!(secret_name(3).unwrap(), "password-3");
    }
}
