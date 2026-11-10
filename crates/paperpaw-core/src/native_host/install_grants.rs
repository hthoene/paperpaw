//! The trusted-installer grants of plugin jobs (`device.md` 5.2).
//!
//! A plugin installs without being asked each time only if the user granted
//! it so: the host-held setting `core.grant.install.<plugin>`, off by
//! default. Plugins cannot write it (`core.grant.*` is refused in command
//! batches) and read only their own. The kernel reads it just before the
//! driver steps a plugin job, so a grant given, revoked or lost with a
//! plugin's stop counts from the next step on, and a plugin that is not
//! running is never granted.

use super::{firmware_driver::Driver, plugin_job::InstallGrants};
use crate::Kernel;
use alloc::{format, rc::Rc, vec::Vec};
use core::cell::RefCell;
use paperpaw_sdk::SettingValue;

/// The owners that hold the grant at the moment of the reading.
pub(crate) struct Granted(pub(crate) Vec<u64>);

impl InstallGrants for Granted {
    fn granted(&self, owner: u64) -> bool {
        self.0.contains(&owner)
    }
}

/// The setting that holds the grant of the plugin `id`.
pub(crate) fn grant_key(id: &str) -> alloc::string::String {
    format!("core.grant.install.{id}")
}

impl Kernel {
    /// Reads the grants of the plugins that have a plugin job and hands
    /// them to the driver, before it steps. A busy driver is left alone: it
    /// steps nothing then either.
    pub(crate) fn feed_install_grants(&self, driver: &Rc<RefCell<Driver>>) {
        let Ok(owners) = driver.try_borrow().map(|driver| driver.plugin_owners()) else {
            return;
        };
        if owners.is_empty() {
            return;
        }
        let granted = owners
            .into_iter()
            .filter(|owner| {
                self.shared
                    .native_instances
                    .id_of(*owner)
                    .is_some_and(|id| {
                        matches!(
                            self.shared.settings.get(&grant_key(&id)),
                            Some(SettingValue::Bool(true))
                        )
                    })
            })
            .collect();
        if let Ok(mut driver) = driver.try_borrow_mut() {
            driver.feed_install_grants(granted);
        }
    }
}

#[cfg(test)]
#[path = "install_grants_tests.rs"]
mod tests;
