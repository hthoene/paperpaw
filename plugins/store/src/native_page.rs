//! What a plugin's page offers (slice 6, host-free): the rows of actions and of
//! more to read, from what the list and the page say of the plugin and what the
//! device has of it. The interface-6 rules of `page.rs`, `origin.rs` and
//! `catalog.rs` (`may_install`, `newer`, `local`, `unbuilt`), as functions of
//! plain inputs: nothing here reads the device, draws or sends a command, so the
//! screen of a later slice only fills [`PageInput`] and draws the rows.
//!
//! **What changed with interface 7 on purpose:** the room for a plugin and its
//! code is no longer worked out here from the card and the flash: the host's
//! check (`check_file`, `docs/abi/device.md` 5.1) says it, with reason 13. The
//! page shows *no room* only once a check said so (`PageInput::no_room`); until
//! then it offers *Install*, as it did where the room could not be told. Which
//! store version was installed by this store (`Origin`) and as which release is
//! what the store keeps itself (`Installed`), a host that installs does not say.
use crate::native_model::Entry;
use alloc::vec::Vec;
use core::cmp::Ordering;
use paperpaw_sdk::Version;
use paperpaw_sdk::firmware::compare_versions;
use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::mem::push;

/// A row of the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Row {
    Install,
    Stop,
    Update,
    Remove,
    Restart,
    /// The notice before the first code plugin: understood, install.
    Understood,
    /// Not now, after the notice or the warning.
    Cancel,
    /// Installed some other way: this store's version in its place.
    UseStore,
    /// Built in and switched off: switched on, nothing is downloaded.
    SwitchOn,
    /// Not installed: a check said there is no room for it.
    NoRoom,
    /// Its newest build is made for a newer PaperPaw than this one.
    NeedsNewer,
    /// Its author has not built it for this PaperPaw yet.
    NoBuild,
    /// Its screenshots, one after another.
    Shots,
    /// Where its source is, as a QR code.
    Source,
    /// How it works, at length, to read paged.
    How,
}

/// What kind of plugin the device has of an id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Kind {
    BuiltIn,
    Data,
    Code,
}

/// Who put the installed version in place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Origin {
    /// This store.
    Store,
    /// Another way (the Wi-Fi transfer, a card, one's own build).
    Local,
}

/// Why an installed plugin does not run, as far as the page needs it: a build
/// for another interface or compiler is *repaired* by the same version again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Trouble {
    /// Built for an older interface or another build.
    OtherBuild,
    /// Anything else.
    Other,
}

/// What the device has of the plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed<'a> {
    pub kind: Kind,
    pub enabled: bool,
    pub restart_needed: bool,
    /// The version installed (a code plugin's update waiting for the restart
    /// counts).
    pub version: &'a str,
    pub origin: Origin,
    /// The version as the store named it where it installed it (a beta of one
    /// release shares its numbers); `None` where it did not.
    pub release: Option<&'a str>,
    pub trouble: Option<Trouble>,
}

/// Whether no build of the plugin runs on this PaperPaw yet: its newest is for
/// a newer one, or its author has not made one for this.
#[must_use]
pub fn unbuilt(entry: &Entry) -> bool {
    entry.needs_newer || entry.waits_author
}

/// Whether a plugin `code` or not of the library may take the place of what the
/// device has of that id: nothing, or a plugin of the same kind installed
/// before. A built-in comes only with the firmware, and one kind does not
/// replace the other.
#[must_use]
pub fn may_install(have: Option<Kind>, code: bool) -> bool {
    match have {
        None => true,
        Some(Kind::BuiltIn) => false,
        Some(Kind::Data) => !code,
        Some(Kind::Code) => code,
    }
}

/// Whether `version` is a pre-release (`0.5.0-beta.1`).
#[must_use]
pub fn is_pre_release(version: &str) -> bool {
    version.split('+').next().is_some_and(|v| v.contains('-'))
}

/// Whether `offered` is an update over `here`, whole versions compared
/// (`0.5.0-beta.2` after `0.5.0-beta.1`, `0.5.0` after both); a beta only where
/// the device takes `betas`.
#[must_use]
pub fn offers(offered: &str, here: &str, betas: bool) -> bool {
    Version::parse(offered).is_some()
        && compare_versions(offered, here) == Ordering::Greater
        && (betas || !is_pre_release(offered))
}

/// Whether the entry's build runs on this PaperPaw while the plugin installed
/// does not: its code was built for an older interface or another build,
/// whatever its version says (#148).
#[must_use]
pub fn repairs(entry: &Entry, installed: &Installed<'_>) -> bool {
    entry.code && !unbuilt(entry) && installed.trouble == Some(Trouble::OtherBuild)
}

/// Whether a newer version than the one installed is offered: only for a plugin
/// this store installed, never over one put in place otherwise.
#[must_use]
pub fn newer(entry: &Entry, installed: &Installed<'_>, takes_betas: bool) -> bool {
    // Its newer version is made for a newer PaperPaw: what is installed stays.
    if unbuilt(entry) {
        return false;
    }
    // The same version, built for this PaperPaw: what is installed was built for
    // another, and is offered as the update it is, whatever put it there.
    if repairs(entry, installed) {
        return true;
    }
    if installed.origin != Origin::Store {
        return false;
    }
    let here = installed.release.unwrap_or(installed.version);
    offers(&entry.version, here, takes_betas)
}

/// Whether the plugin is installed, but not by this store: its version here may
/// be used in its place, when the reader says so, never as an update.
#[must_use]
pub fn local(entry: &Entry, installed: &Installed<'_>) -> bool {
    !unbuilt(entry) && installed.origin == Origin::Local
}

/// What the page knows when it works out its rows.
#[derive(Debug, Clone, Copy)]
pub struct PageInput<'a> {
    pub entry: &'a Entry,
    /// What the device has of the plugin, if anything.
    pub installed: Option<&'a Installed<'a>>,
    /// This plugin's download (or the job waiting for the reader's word) goes on.
    pub own_job: bool,
    /// The notice before the first code plugin is shown.
    pub notice: bool,
    /// A check said there is no room for it.
    pub no_room: bool,
    /// The device takes betas as updates.
    pub takes_betas: bool,
    /// What the plugin's own page at the library has to read: its description,
    /// pictures and source (all false while the page is not here).
    pub has_description: bool,
    pub has_shots: bool,
    pub has_source: bool,
}

fn unbuilt_row(entry: &Entry) -> Row {
    if entry.waits_author {
        Row::NoBuild
    } else {
        Row::NeedsNewer
    }
}

/// The rows of the page, actions first, then what there is to read.
///
/// # Errors
/// `NoMemory`.
pub fn rows(page: &PageInput<'_>) -> Result<Vec<Row>, Error> {
    let mut rows = actions(page)?;
    if !page.notice {
        for (has, row) in [
            (page.has_description, Row::How),
            (page.has_shots, Row::Shots),
            (page.has_source, Row::Source),
        ] {
            if has {
                push(&mut rows, row)?;
            }
        }
    }
    Ok(rows)
}

/// What can be done with the plugin.
///
/// # Errors
/// `NoMemory`.
pub fn actions(page: &PageInput<'_>) -> Result<Vec<Row>, Error> {
    let entry = page.entry;
    let mut rows = Vec::new();
    if page.notice {
        push(&mut rows, Row::Understood)?;
        push(&mut rows, Row::Cancel)?;
        return Ok(rows);
    }
    if page.own_job {
        push(&mut rows, Row::Stop)?;
        return Ok(rows);
    }
    let Some(installed) = page.installed else {
        if unbuilt(entry) {
            push(&mut rows, unbuilt_row(entry))?;
        } else if page.no_room {
            push(&mut rows, Row::NoRoom)?;
        } else {
            push(&mut rows, Row::Install)?;
        }
        return Ok(rows);
    };
    // Built in: switched on if it is off, nothing else (no download).
    if installed.kind == Kind::BuiltIn {
        if installed.restart_needed {
            push(&mut rows, Row::Restart)?;
        }
        if !installed.enabled {
            push(&mut rows, Row::SwitchOn)?;
        }
        return Ok(rows);
    }
    // Not this one to install over, update or remove.
    if !may_install(Some(installed.kind), entry.code) {
        return Ok(rows);
    }
    // A restart first where one is waited for; a code plugin removed runs on
    // until then, and offers nothing else.
    if installed.restart_needed {
        push(&mut rows, Row::Restart)?;
    }
    if installed.enabled {
        if unbuilt(entry) {
            push(&mut rows, unbuilt_row(entry))?;
        } else if newer(entry, installed, page.takes_betas) {
            push(&mut rows, Row::Update)?;
        } else if local(entry, installed) {
            push(&mut rows, Row::UseStore)?;
        }
        push(&mut rows, Row::Remove)?;
    }
    Ok(rows)
}
