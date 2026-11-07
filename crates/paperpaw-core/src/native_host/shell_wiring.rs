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
    ui_service::UiServices,
    ui_store::{Limits, UiStore},
};
use alloc::rc::Rc;
use paperpaw_native::host_glue::ERR_STATE;
use paperpaw_platform::Storage;

/// What a shell wires into the device's host views. Nothing is wired for a
/// family the shell does not name.
#[derive(Default)]
#[non_exhaustive]
pub struct HostWiring {
    hyphenation: Option<Rc<dyn Storage>>,
    device: Option<Rc<dyn DeviceHost>>,
    /// `core.ui`: `Some(x4)`.
    ui: Option<bool>,
}

impl core::fmt::Debug for HostWiring {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("HostWiring")
            .field("hyphenation", &self.hyphenation.is_some())
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

    /// `core.device`: the shell's view of its power, radio and firmware.
    #[must_use]
    pub fn device(mut self, host: Rc<dyn DeviceHost>) -> Self {
        self.device = Some(host);
        self
    }

    /// `core.ui`: the screen-stack service (questions, hand-overs, texts). The
    /// host makes its store, sized for the device (`x4`: the small limits of
    /// the X4's 256 KiB heap); nothing else is passed in, and nothing is
    /// allocated until a plugin uses it.
    #[must_use]
    pub fn ui(mut self, x4: bool) -> Self {
        self.ui = Some(x4);
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
}

impl WiredHosts {
    /// The hyphenation tables, if `text.hyphenate` was wired.
    #[must_use]
    pub fn hyphen(&self) -> Option<&HyphenTables> {
        self.hyphen.as_ref()
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
            || (wiring.device.is_some() && hosts.device().is_some())
            || (wiring.ui.is_some() && hosts.ui().is_some())
        {
            return Err(ERR_STATE);
        }
        let mut wired = WiredHosts::default();
        if let Some(storage) = wiring.hyphenation {
            let tables = Rc::new(CardTables::new(storage));
            hosts.wire_hyphen(tables.clone())?;
            wired.hyphen = Some(HyphenTables(tables));
        }
        if let Some(host) = wiring.device {
            hosts.wire_device(host)?;
        }
        if let Some(x4) = wiring.ui {
            let limits = if x4 { Limits::X4 } else { Limits::ELSEWHERE };
            hosts.wire_ui(UiServices {
                store: Rc::new(UiStore::new(limits)),
            })?;
        }
        Ok(wired)
    }
}

#[cfg(test)]
#[path = "shell_wiring_tests.rs"]
mod tests;
