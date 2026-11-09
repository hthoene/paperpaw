//! The catalogs the user added: a list in the plugin's folder, one
//! catalog a line with its fields separated by tabs. Until the user
//! changes it, the list holds a free catalog to start with. Their
//! passwords are kept with the other passwords and keys (see
//! `paperpaw_sdk::secrets`), by the catalog's address.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::secrets::SecretChange;
use paperpaw_sdk::web::{clean_name, host};
use paperpaw_sdk::{Arg, Command, Commands, Env, Storage, files};

use crate::PLUGIN;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Catalog {
    pub name: String,
    pub url: String,
    pub user: String,
    pub password: String,
    /// Where syncing puts its books; empty for a folder named after the
    /// catalog in the library.
    pub folder: String,
    /// Synced with the others by *Sync catalogs*.
    pub sync: bool,
    /// A WebDAV folder (e.g. in Nextcloud) rather than an OPDS catalog.
    pub webdav: bool,
}

impl Catalog {
    /// A new catalog at `url`, named after its host.
    pub(crate) fn new(url: &str) -> Self {
        Self {
            name: host(url).trim_start_matches("www.").to_string(),
            url: url.to_string(),
            // Nextcloud's and ownCloud's addresses say so.
            webdav: url.contains("/remote.php/") || url.contains("/webdav"),
            ..Self::default()
        }
    }

    /// The folder its books are synced to.
    pub(crate) fn sync_folder(&self, env: &Env<'_>) -> String {
        if !self.folder.trim().is_empty() {
            return self.folder.trim().trim_matches('/').to_string();
        }
        let library = paperpaw_sdk::library::folder(env);
        let name = clean_name(&self.name, 60).unwrap_or_else(|| String::from("OPDS"));
        if library.is_empty() {
            name
        } else {
            format!("{library}/{name}")
        }
    }

    /// The `Authorization` header for it, if it needs one.
    pub(crate) fn authorization(&self) -> Option<String> {
        (!self.user.is_empty()).then(|| paperpaw_sdk::web::basic_auth(&self.user, &self.password))
    }
}

fn path() -> String {
    format!("{}/catalogs.txt", Env::plugin_dir(PLUGIN.as_str()))
}

/// The prefix of the catalogs' secrets.
const SECRETS: &str = "opds.";

/// The id of the password of the catalog at `url`.
fn secret_id(url: &str) -> String {
    let digest = paperpaw_sdk::digest::md5::of(url);
    format!("{SECRETS}{}", &digest[..16])
}

/// The most the list may take; a list that has outgrown it is not read
/// (and so not saved over).
const MAX_FILE: usize = 64 * 1024;

/// The list could not be read: it is there, but the card failed, it is
/// too large or it is not UTF-8 text. Nothing may be saved over it then,
/// or the catalogs and their passwords would be lost.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Unreadable;

/// The catalogs; until the user changes them, one to start with.
pub(crate) fn load(env: &Env<'_>) -> Result<Vec<Catalog>, Unreadable> {
    let Some(mut catalogs) = read(env.storage())? else {
        return Ok(Vec::from([Catalog {
            name: String::from("Project Gutenberg"),
            url: String::from("https://www.gutenberg.org/ebooks.opds/"),
            ..Catalog::default()
        }]));
    };
    fill_passwords(env, &mut catalogs);
    Ok(catalogs)
}

/// The list as the file has it; `None` if there is none yet.
fn read(storage: &dyn Storage) -> Result<Option<Vec<Catalog>>, Unreadable> {
    let text = files::load_text_for_update(storage, &path(), MAX_FILE).map_err(|_| Unreadable)?;
    Ok(text.as_deref().map(parse))
}

/// The passwords kept with the other passwords and keys, for catalogs
/// whose line has none (earlier versions kept it there, see
/// `move_passwords`).
fn fill_passwords(env: &Env<'_>, catalogs: &mut [Catalog]) {
    for c in catalogs.iter_mut().filter(|c| c.password.is_empty()) {
        c.password = env
            .secrets()
            .get(&secret_id(&c.url))
            .unwrap_or_default()
            .to_string();
    }
}

/// Moves passwords kept in the list by earlier versions to the other
/// passwords and keys. The list lets go of them only once those hold
/// them, at a later start: where they could not be saved, the list still
/// has them.
pub(crate) fn move_passwords(env: &Env<'_>, out: &mut Commands) {
    let Ok(Some(mut catalogs)) = read(env.storage()) else {
        return;
    };
    if catalogs.iter().all(|c| c.password.is_empty()) {
        return;
    }
    let kept = |c: &Catalog| env.secrets().get(&secret_id(&c.url)) == Some(c.password.as_str());
    if !catalogs.iter().all(|c| c.password.is_empty() || kept(c)) {
        return keep_passwords(env, &catalogs, out);
    }
    // Those moved already stay.
    fill_passwords(env, &mut catalogs);
    save(env, &catalogs, out);
}

/// The list as the file has it.
fn parse(text: &str) -> Vec<Catalog> {
    text.lines()
        .filter_map(|line| {
            let mut fields = line.split('\t');
            let name = fields.next()?.to_string();
            let url = fields.next().filter(|url| !url.is_empty())?.to_string();
            let mut field = || fields.next().unwrap_or("").to_string();
            Some(Catalog {
                name,
                url,
                user: field(),
                password: field(),
                folder: field(),
                sync: field() == "sync",
                webdav: field() == "webdav",
            })
        })
        .collect()
}

/// The file's text: the password's place stays, empty.
fn encode(catalogs: &[Catalog]) -> String {
    let clean = |s: &str| s.replace(['\t', '\n', '\r'], " ");
    let mut text = String::new();
    for c in catalogs {
        text.push_str(&format!(
            "{}\t{}\t{}\t\t{}\t{}\t{}\n",
            clean(&c.name),
            clean(&c.url),
            clean(&c.user),
            clean(&c.folder),
            if c.sync { "sync" } else { "" },
            if c.webdav { "webdav" } else { "" },
        ));
    }
    text
}

/// Saves the list, and the passwords with the other passwords and keys:
/// their changes go to `out`. `catalogs` must come from [`load`]: what is
/// not in it is removed.
pub(crate) fn save(env: &Env<'_>, catalogs: &[Catalog], out: &mut Commands) {
    keep_passwords(env, catalogs, out);
    // Those of catalogs removed or moved to another address go.
    let ids: Vec<String> = catalogs.iter().map(|c| secret_id(&c.url)).collect();
    for secret in env.secrets().iter() {
        if secret.id.starts_with(SECRETS) && !ids.contains(&secret.id) {
            out.push(Command::set_secret(SecretChange::remove(&secret.id)));
        }
    }
    let _ = env.storage().write(&path(), encode(catalogs).as_bytes());
}

/// The catalogs' passwords, kept with the other passwords and keys:
/// their changes go to `out`.
fn keep_passwords(env: &Env<'_>, catalogs: &[Catalog], out: &mut Commands) {
    for c in catalogs {
        let id = secret_id(&c.url);
        let kept = env.secrets().get(&id);
        if c.password.is_empty() {
            if kept.is_some() {
                out.push(Command::set_secret(SecretChange::remove(&id)));
            }
        } else if kept != Some(c.password.as_str()) {
            let label = env
                .i18n()
                .format("opds.secret", &[("name", Arg::Str(&c.name))]);
            out.push(Command::set_secret(SecretChange::keep(
                &id,
                &label,
                &c.password,
            )));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogs_are_kept_without_passwords() {
        let mut catalog = Catalog::new("https://www.example.org/opds");
        assert_eq!(catalog.name, "example.org");
        catalog.user = String::from("me");
        catalog.password = String::from("s3cret");
        catalog.folder = String::from("Books/Example");
        catalog.sync = true;
        let text = encode(&[catalog.clone()]);
        assert!(!text.contains("s3cret"));
        catalog.password.clear();
        assert_eq!(parse(&text), [catalog]);
        // Earlier versions kept the password in the list.
        let old = parse("Old\thttps://old.example\tme\tpw\t\tsync\t\n");
        assert_eq!(old[0].password, "pw");
        assert!(old[0].sync);
        assert!(secret_id("https://a").starts_with(SECRETS));
        assert_ne!(secret_id("https://a"), secret_id("https://b"));
    }

    #[test]
    fn a_list_that_cannot_be_read_is_no_empty_list() {
        let storage = paperpaw_sdk::MemoryStorage::new();
        assert_eq!(read(&storage), Ok(None), "none yet");
        storage
            .write(&path(), b"Mine\thttps://a.example\n")
            .unwrap();
        assert_eq!(read(&storage).unwrap().unwrap()[0].name, "Mine");
        // Saved on a computer in another encoding.
        storage
            .write(&path(), b"B\xfccher\thttps://a.example\n")
            .unwrap();
        assert_eq!(read(&storage), Err(Unreadable));
        let long = "Mine\thttps://a.example\n".repeat(MAX_FILE / 10);
        storage.write(&path(), long.as_bytes()).unwrap();
        assert_eq!(read(&storage), Err(Unreadable));
    }

    /// What version 1 wrote stays readable (see `docs/data-files.md`).
    #[test]
    fn catalogs_of_version_1_are_read() {
        let catalogs = super::parse(include_str!("../testdata/catalogs.txt.v1"));
        assert_eq!(catalogs.len(), 3);
        assert_eq!(catalogs[0].name, "Project Gutenberg");
        assert!(!catalogs[0].sync && !catalogs[0].webdav);
        assert_eq!(catalogs[1].user, "reader");
        assert_eq!(catalogs[1].folder, "Calibre");
        assert!(catalogs[1].sync && !catalogs[1].webdav);
        assert!(catalogs[2].sync && catalogs[2].webdav);
        assert!(catalogs.iter().all(|c| c.password.is_empty()));
    }
}

#[cfg(test)]
mod format_tests;
