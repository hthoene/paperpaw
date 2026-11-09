//! The shells' one way to give the host the views it serves services from.
//!
//! `HostServices` and its `wire_*` methods stay inside the crate; a shell (the X4, the desktop emulator,
//! Android) describes what it has in a [`HostWiring`] and hands it to
//! [`HostResources::wire`] once, where the device's table is made. Every
//! family asked for is checked unwired before the first is set, so a refusal
//! leaves nothing half wired.
//!
//! Which families a shell can name grows with the adapters: `text.hyphenate`
//! (the card's tables), `core.device` (a host that already is public) and
//! `core.ui` (the store; the stack's view comes from each call's frame) today;
//! the other families' hosts are core's adapters over kernel objects and join
//! as their adapters land, one method each.
use super::{
    device_service::DeviceHost,
    hyphen_card::CardTables,
    resources::HostResources,
    shared::Shared,
    storage_files::StorageFiles,
    ui_service::UiServices,
    ui_store::{Limits, UiStore},
};
use alloc::{rc::Rc, vec::Vec};
use paperpaw_native::host_glue::{ERR_NO_MEMORY, ERR_STATE};
use paperpaw_platform::Storage;

/// What a shell wires into the device's host views. Nothing is wired for a
/// family the shell does not name.
#[derive(Default)]
#[non_exhaustive]
pub struct HostWiring {
    hyphenation: Option<Rc<dyn Storage>>,
    files: Option<Rc<dyn Storage>>,
    device: Option<Rc<dyn DeviceHost>>,
    /// `core.ui`: `Some(x4)`.
    ui: Option<bool>,
}

impl core::fmt::Debug for HostWiring {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("HostWiring")
            .field("hyphenation", &self.hyphenation.is_some())
            .field("files", &self.files.is_some())
            .field("device", &self.device.is_some())
            .field("ui", &self.ui.is_some())
            .finish()
    }
}

impl HostWiring {
    /// Nothing named yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `text.hyphenate`: the data plugins' hyphenation tables are files on
    /// this storage (the card). The shell then declares which languages the
    /// installed plugins offer ([`HyphenTables::declare`]).
    #[must_use]
    pub fn hyphenation(mut self, storage: Rc<dyn Storage>) -> Self {
        self.hyphenation = Some(storage);
        self
    }

    /// `core.files`: the plugins' file service over this storage (the card).
    /// The shell gives only the storage; core composes the rest: the host's
    /// own folders by identity (an 8.3 alias of `.paperpaw` is refused; the
    /// check over-refuses until the storage can answer "same entry"), what is
    /// in use (downloads, staged installs, served bodies, the library's move
    /// job: `ERR_BUSY`) and the move itself.
    #[must_use]
    pub fn files(mut self, storage: Rc<dyn Storage>) -> Self {
        self.files = Some(storage);
        self
    }

    /// `core.device`: the shell's view of its power, radio and firmware.
    #[must_use]
    pub fn device(mut self, host: Rc<dyn DeviceHost>) -> Self {
        self.device = Some(host);
        self
    }

    /// `core.ui`: the screen-stack service (questions, hand-overs, texts). The
    /// host makes its store, sized for the device named by `device_id` (the
    /// X4 gets the small limits of its 256 KiB heap; the same test as the
    /// admission rules use, so the two cannot disagree); nothing else is passed
    /// in, and nothing is allocated until a plugin uses it.
    #[must_use]
    pub fn ui(mut self, device_id: &str) -> Self {
        self.ui = Some(super::ui_store::is_x4(device_id));
        self
    }
}

/// The declared hyphenation languages and the card's changes, for the shell to
/// keep current: plugins installed or removed, a card inserted or written.
#[derive(Clone)]
pub struct HyphenTables(Rc<CardTables>);

impl core::fmt::Debug for HyphenTables {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("HyphenTables")
    }
}

impl HyphenTables {
    pub(crate) fn over(tables: Rc<CardTables>) -> Self {
        Self(tables)
    }

    #[cfg(test)]
    pub(crate) fn card(&self) -> &CardTables {
        &self.0
    }

    /// Replaces the declared tables: `(language tag, card path, plugin id)`
    /// for each installed plugin's `resource.hyphenation` entry, in any order
    /// (the greatest plugin id wins a tag). All or nothing: on an error the
    /// old ones stay.
    ///
    /// # Errors
    /// `ERR_INVALID` (an empty or overlong tag or path), `ERR_NO_MEMORY`.
    pub fn declare<'a>(
        &self,
        declared: impl Iterator<Item = (&'a str, &'a str, u64)>,
    ) -> Result<(), i32> {
        self.0.replace(declared)
    }

    /// As [`declare`](Self::declare), with the plugin named by its id: the
    /// plugin whose id wins ([`paperpaw_sdk::plugin_wins`], D4) gets a tag
    /// several declare, as the kernel's own tables decide it. The
    /// iterator is walked twice.
    ///
    /// # Errors
    /// As [`declare`](Self::declare).
    pub fn declare_plugins<'a>(
        &self,
        declared: impl Iterator<Item = (&'a str, &'a str, &'a str)> + Clone,
    ) -> Result<(), i32> {
        // The SDK's own rule (D4), so the host and the SDK cannot diverge.
        let order = |a: &str, b: &str| {
            if paperpaw_sdk::plugin_wins(a, b) {
                core::cmp::Ordering::Greater
            } else if paperpaw_sdk::plugin_wins(b, a) {
                core::cmp::Ordering::Less
            } else {
                core::cmp::Ordering::Equal
            }
        };
        let mut plugins: Vec<&str> = Vec::new();
        for (_, _, plugin) in declared.clone() {
            plugins.try_reserve(1).map_err(|_| ERR_NO_MEMORY)?;
            plugins.push(plugin);
        }
        plugins.sort_unstable_by(|a, b| order(a, b));
        plugins.dedup_by(|a, b| order(a, b).is_eq());
        let rank = |plugin: &str| {
            let at = plugins
                .binary_search_by(|probe| order(probe, plugin))
                .unwrap_or(0);
            at as u64 + 1
        };
        self.0
            .replace(declared.map(|(tag, path, plugin)| (tag, path, rank(plugin))))
    }

    /// The card was inserted, removed or written: open tables are checked
    /// again before they are used.
    pub fn card_changed(&self) {
        self.0.card_changed();
    }
}

/// The handles of what [`HostResources::wire`] wired, for the shell to keep.
#[derive(Debug, Default)]
#[non_exhaustive]
pub struct WiredHosts {
    hyphen: Option<HyphenTables>,
    ui_skipped: bool,
}

impl WiredHosts {
    /// The hyphenation tables, if `text.hyphenate` was wired.
    #[must_use]
    pub fn hyphen(&self) -> Option<&HyphenTables> {
        self.hyphen.as_ref()
    }

    /// Whether `core.ui` was asked for and not wired because its store could
    /// not be allocated (the other families are wired): the shell says so.
    #[must_use]
    pub fn ui_skipped(&self) -> bool {
        self.ui_skipped
    }
}

impl HostResources {
    /// Wires the families `wiring` names, once for the device.
    ///
    /// # Errors
    /// `ERR_STATE` when one of them is wired already (nothing is wired then).
    pub fn wire(&self, wiring: HostWiring) -> Result<WiredHosts, i32> {
        let hosts = self.hosts();
        if (wiring.hyphenation.is_some() && hosts.hyphen().is_some())
            || (wiring.files.is_some() && hosts.files().is_some())
            || (wiring.device.is_some() && hosts.device().is_some())
            || (wiring.ui.is_some() && hosts.ui().is_some())
        {
            return Err(ERR_STATE);
        }
        let mut wired = WiredHosts::default();
        if let Some(storage) = wiring.hyphenation {
            let tables = Rc::new(CardTables::new(storage));
            hosts.wire_hyphen(tables.clone())?;
            wired.hyphen = Some(HyphenTables::over(tables));
        }
        if let Some(storage) = wiring.files {
            hosts.wire_files(Rc::new(StorageFiles::new(
                storage,
                hosts.busy_sources().clone(),
            )))?;
        }
        if let Some(host) = wiring.device {
            hosts.wire_device(host)?;
        }
        if let Some(x4) = wiring.ui {
            let limits = if x4 { Limits::X4 } else { Limits::ELSEWHERE };
            // A store that cannot be allocated degrades: no `core.ui` (a plugin
            // asks and is told the service is not there), the other families
            // wired; the shell is told ([`WiredHosts::ui_skipped`]).
            match Shared::try_new(UiStore::new(limits)) {
                Ok(store) => hosts.wire_ui(UiServices::new(store, x4))?,
                Err(_) => wired.ui_skipped = true,
            }
        }
        Ok(wired)
    }
}

#[cfg(test)]
#[path = "shell_wiring_tests.rs"]
mod tests;
