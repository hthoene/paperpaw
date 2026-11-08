//! OPDS catalogs: online libraries such as Project Gutenberg, Calibre-Web,
//! Kavita or Komga, browsed and searched like folders; their books are
//! downloaded into the library.
//!
//! - *Catalogs* lists them. Tapping one browses it; holding it (with
//!   buttons: the menu button) offers to sync, edit or remove it.
//! - A catalog has an address and, if it needs them, a user name and a
//!   password. Marked for syncing, *Sync catalogs* downloads its books
//!   that are not on the device yet into its folder (see `sync`); the
//!   action can also be put into menus.
//! - Books go to the library folder when downloaded one by one, in the
//!   format PaperPaw reads best of those offered.
//!
//! Feeds are read in pieces and books written straight to storage, so
//! catalogs of any size work on small devices too.

#![no_std]

extern crate alloc;

#[path = "../declarations.rs"]
#[allow(
    dead_code,
    reason = "the build script and the declaration test also use the builders"
)]
pub mod declarations;
mod native_entry;
#[doc(hidden)]
pub mod native_model;
/// The interface-7 catalogs (slice 1; see `PORTING-7.md`).
pub mod native_plugin;

#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;

#[cfg(test)]
mod native_parity_tests;

mod browse;
mod catalogs;
mod edit;
mod feed;
mod json;
mod list;
mod sync;
mod temp;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::RefCell;

use paperpaw_sdk::builtin::{events, groups, menus, params};
use paperpaw_sdk::event::Event;
use paperpaw_sdk::replies::HttpReply;
use paperpaw_sdk::{
    ActionSpec, Command, Commands, Env, FixedAction, FixedScreen, HttpRequest, Id, Manifest,
    MenuEntry, Params, Plugin, RegisterError, Registrar, ScreenSpec, SettingSpec,
};

use crate::catalogs::Catalog;
use crate::sync::Sync;
use crate::temp::TempFiles;
use paperpaw_sdk::setting::Setting;

pub const PLUGIN: Id = Id::new_static("opds");
/// Opens the list of catalogs.
pub const OPEN: Id = Id::new_static("opds.open");
/// Syncs the catalogs marked for it.
pub const SYNC: Id = Id::new_static("opds.sync");
const LIST_SCREEN: Id = Id::new_static("opds.catalogs");
/// Declared work: a sync no reader waits for (a rule's), at a good moment.
const SYNC_DUE: Event = Event::new("opds.sync-due");
const BROWSE_SCREEN: Id = Id::new_static("opds.browse");
const EDIT_SCREEN: Id = Id::new_static("opds.edit");
/// Translation keys of the choices: this, then the option.
const OPTION_LABELS: &str = "opds.option.";

/// Parameters: an address, and which catalog (its place in the list).
const URL: &str = params::URL;
const ID: &str = params::ID;

/// Setting: covers in the lists of catalogs (on devices with the memory).
const COVERS: Setting<bool> = Setting::new("opds.covers");

/// A request to a catalog, its answer written to `save_to` (see
/// [`temp`]). The catalog's credentials go only to the catalog itself:
/// feeds may link to covers, books or further feeds anywhere, and those
/// get no password.
fn request(
    agent: &(String, String),
    catalog: &Catalog,
    url: &str,
    save_to: String,
    reply: &Event<HttpReply>,
    params: Params,
) -> HttpRequest {
    request_by("GET", agent, catalog, url, save_to, reply, params)
}

/// The same with `method`.
fn request_by(
    method: &str,
    agent: &(String, String),
    catalog: &Catalog,
    url: &str,
    save_to: String,
    reply: &Event<HttpReply>,
    params: Params,
) -> HttpRequest {
    let request = HttpRequest::new(method, url, reply)
        .with(params.with(temp::FILE, save_to.clone()))
        .save_to(save_to)
        .header(agent.0.clone(), agent.1.clone());
    match catalog.authorization() {
        Some(auth) if same_origin(url, &catalog.url) => {
            request.header(String::from("Authorization"), auth)
        }
        _ => request,
    }
}

/// The scheme, host and port of `url` (the port filled in for `http` and
/// `https`), in lower case; `None` where any of them is missing or in
/// doubt, such as an address with a user name (`https://me@host/`).
fn origin(url: &str) -> Option<(String, String, u16)> {
    let (scheme, rest) = url.split_once("://")?;
    let authority = &rest[..rest.find(['/', '?', '#']).unwrap_or(rest.len())];
    let doubtful = |c: char| matches!(c, '@' | '\\' | '%') || c.is_whitespace() || c.is_control();
    if authority.contains(doubtful) {
        return None;
    }
    // A colon inside `[…]` belongs to an IPv6 address, not the port.
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) if !port.contains(']') => (host, Some(port)),
        _ => (authority, None),
    };
    let scheme = scheme.to_ascii_lowercase();
    let port = match (port, scheme.as_str()) {
        (Some(port), _) => port.parse().ok()?,
        (None, "http") => 80,
        (None, "https") => 443,
        (None, _) => return None,
    };
    (!host.is_empty()).then(|| (scheme, host.to_ascii_lowercase(), port))
}

/// Whether `a` and `b` are on the same server: the same scheme, host and
/// port.
fn same_origin(a: &str, b: &str) -> bool {
    matches!((origin(a), origin(b)), (Some(a), Some(b)) if a == b)
}

/// A request for a list of a catalog: for a WebDAV folder its listing
/// (`PROPFIND`), else the feed.
fn list_request(
    agent: &(String, String),
    catalog: &Catalog,
    url: &str,
    save_to: String,
    reply: &Event<HttpReply>,
    params: Params,
) -> HttpRequest {
    if !catalog.webdav {
        return request(agent, catalog, url, save_to, reply, params);
    }
    request_by("PROPFIND", agent, catalog, url, save_to, reply, params)
        .header(String::from("Depth"), String::from("1"))
        .header(
            String::from("Content-Type"),
            String::from("application/xml; charset=utf-8"),
        )
        .body(String::from(feed::PROPFIND))
}

/// Reads a list of a catalog from `path`, which came from `base`.
fn read_list(
    catalog: &Catalog,
    storage: &dyn paperpaw_sdk::Storage,
    path: &str,
    base: &str,
) -> feed::Feed {
    if catalog.webdav {
        feed::read_webdav(storage, path, base)
    } else if json::is_json(storage, path) {
        json::read(storage, path, base)
    } else {
        feed::read(storage, path, base)
    }
}

/// The catalogs marked for syncing.
fn marked(env: &Env<'_>) -> Vec<Catalog> {
    let all = catalogs::load(env).unwrap_or_default();
    all.into_iter().filter(|c| c.sync).collect()
}

/// The OPDS plugin.
#[derive(Debug, Default)]
pub struct Opds;

/// The catalogs' part of the library settings.
static IN_CATALOGS: paperpaw_sdk::SettingExtra =
    paperpaw_sdk::SettingExtra::new().section("opds.section");
static IN_CATALOGS_10: paperpaw_sdk::SettingExtra = paperpaw_sdk::SettingExtra::new()
    .section("opds.section")
    .step(10);

/// Covers in lists, and how many books a sync fetches at most.
static SETTINGS: [SettingSpec; 2] = [
    SettingSpec::switch(
        COVERS.static_str(),
        "opds.setting.covers",
        true,
        groups::LIBRARY,
    )
    .with_extra(&IN_CATALOGS)
    .description("opds.setting.covers.help"),
    SettingSpec::number(
        sync::LIMIT.static_str(),
        "opds.setting.limit",
        10,
        1000,
        50,
        groups::LIBRARY,
    )
    .with_extra(&IN_CATALOGS_10)
    .description("opds.setting.limit.help"),
];

/// How it works, by language (see `Manifest::how`).
static HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/en.txt")),
    ("de", include_str!("../how/de.txt")),
];

static ACTIONS: [FixedAction; 1] = [FixedAction::new(OPEN, "opds.title")
    .handler(|_, _, out| out.push(Command::OpenScreen(LIST_SCREEN, Params::new())))];
static ENTRIES: [MenuEntry; 1] = [MenuEntry::new(menus::TOOLS, OPEN).order(45)];
static ICONS: [(Id, &str); 2] = [(OPEN, "book-download"), (SYNC, "refresh")];

static SCREENS: [FixedScreen; 1] = [FixedScreen::new(
    EDIT_SCREEN,
    "opds.title",
    |env, params| Box::new(edit::EditScreen::new(env, params)),
)];

impl Plugin for Opds {
    fn manifest(&self) -> Manifest {
        Manifest::new(PLUGIN, "OPDS catalogs", paperpaw_sdk::package_version!())
            .description(
                "Browses OPDS catalogs, downloads their books and keeps folders in sync with them.",
            )
            .how(&HOW)
            .uses(paperpaw_sdk::Uses::NETWORK)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let temp = TempFiles::default();
        let state = Rc::new(RefCell::new(Sync::new(temp.clone())));
        let shared = state.clone();
        r.add_screen(
            ScreenSpec::new(
                LIST_SCREEN,
                "opds.title",
                Box::new(move |env, _| Box::new(list::ListScreen::new(env, shared.clone()))),
            )
            .instances(paperpaw_sdk::Instances::SINGLE),
        )?;
        r.add_screen(ScreenSpec::new(
            BROWSE_SCREEN,
            "opds.title",
            Box::new(move |env, params| {
                Box::new(browse::BrowseScreen::new(env, params, temp.clone()))
            }),
        ))?;
        r.add_screens(&SCREENS)?;
        r.add_actions(&ACTIONS)?;
        let shared = state.clone();
        r.add_action(ActionSpec::new(SYNC, "opds.sync-all").handler(
            move |env: &Env<'_>, _, out: &mut Commands| {
                if env.reader() {
                    shared.borrow_mut().start(env, marked(env), out);
                } else {
                    let due = SYNC_DUE.id().clone();
                    out.push(Command::work(paperpaw_sdk::work::Work::when_cheap(
                        due,
                        core::time::Duration::from_secs(3600),
                    )));
                }
            },
        ))?;
        let shared = state.clone();
        r.on(SYNC_DUE, move |env, (), out| {
            shared.borrow_mut().start(env, marked(env), out);
        })?;
        r.add_menu_entries(&ENTRIES)?;
        r.add_icons(&ICONS)?;
        r.on(events::STARTED, |env, _, out| {
            temp::clean(env.storage());
            catalogs::move_passwords(env, out);
        })?;
        let shared = state.clone();
        r.on(sync::FEED_LOADED, move |env, reply, out| {
            shared.borrow_mut().feed_loaded(env, reply, out);
        })?;
        let shared = state.clone();
        r.on(sync::BOOK_LOADED, move |env, reply, out| {
            shared.borrow_mut().book_loaded(env, reply, out);
        })?;
        r.add_settings(&SETTINGS)?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use paperpaw_i18n::{Catalog, check};

    use super::*;

    #[test]
    fn origins_are_scheme_host_and_port() {
        let catalog = "https://Books.example.org/opds";
        assert!(same_origin(catalog, "https://books.example.org:443/b.epub"));
        assert!(same_origin("http://[::1]:8080/a", "http://[::1]:8080/b"));
        for other in [
            "http://books.example.org/b.epub",
            "https://books.example.org:8443/b.epub",
            "https://covers.example.org/c.png",
            "https://books.example.org.evil.example/b.epub",
            "https://books.example.org@evil.example/b.epub",
            "https://evil.example\\@books.example.org/b.epub",
            "http://[::1]:8081/b",
            "ftp://books.example.org/b.epub",
            "/b.epub",
        ] {
            assert!(!same_origin(catalog, other), "{other}");
        }
    }

    #[test]
    fn credentials_go_only_to_the_catalog() {
        let catalog = catalogs::Catalog {
            user: String::from("me"),
            password: String::from("s3cret"),
            ..catalogs::Catalog::new("https://books.example.org/opds")
        };
        let reply = Event::<HttpReply>::new("opds.test");
        let auth = |url: &str| {
            let agent = (String::from("User-Agent"), String::from("PaperPaw"));
            let request = request(
                &agent,
                &catalog,
                url,
                String::from("a.tmp"),
                &reply,
                Params::new(),
            );
            request
                .headers()
                .iter()
                .any(|(name, _)| name == "Authorization")
        };
        assert!(auth("https://books.example.org/opds/new"));
        assert!(!auth("http://books.example.org/opds/new"), "not in clear");
        assert!(!auth("https://tracker.example/x.png"));
    }

    #[test]
    fn translations_are_complete() {
        let en = Catalog::parse(include_str!("../i18n/en.ini")).unwrap();
        let de = Catalog::parse(include_str!("../i18n/de.ini")).unwrap();
        let issues = check::compare(&en, &de);
        assert!(issues.is_empty(), "{issues:?}");
    }
}
