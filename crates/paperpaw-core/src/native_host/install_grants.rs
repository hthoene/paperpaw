//! The trusted-installer grants of plugin jobs (`device.md` 5.2).
//!
//! A plugin installs without being asked each time only if the user granted
//! it so: the host-held setting `core.grant.install.<plugin>`, off by
//! default. Plugins cannot write it (`core.grant.*` is refused in command
//! batches) and read only their own. The kernel reads it just before the
//! driver steps a plugin job, so a grant given, revoked or lost with a
//! plugin's stop counts from the next step on, and a plugin that is not
//! running is never granted. Nothing here allocates: the owners, the key and
//! the answer live on the stack and in the driver, and any failure grants
//! nothing. A grant that already skipped a question is not asked again for
//! that job: a revocation counts for later jobs.

use super::{
    firmware_driver::{Driver, REQUESTS_MAX},
    plugin_job::InstallGrants,
};
use crate::Kernel;
use alloc::rc::Rc;
use core::cell::RefCell;
use paperpaw_sdk::SettingValue;

/// The owners that hold the grant at the moment of the reading.
#[derive(Clone, Copy)]
pub(crate) struct Granted {
    owners: [u64; REQUESTS_MAX],
    count: usize,
}

impl Granted {
    /// Nobody holds a grant.
    pub(crate) const NONE: Self = Self {
        owners: [0; REQUESTS_MAX],
        count: 0,
    };

    /// Whether `owner` holds it.
    pub(crate) fn holds(&self, owner: u64) -> bool {
        self.owners[..self.count].contains(&owner)
    }

    pub(crate) fn add(&mut self, owner: u64) {
        if let Some(slot) = self.owners.get_mut(self.count) {
            *slot = owner;
            self.count += 1;
        }
    }
}

impl InstallGrants for Granted {
    fn granted(&self, owner: u64) -> bool {
        self.holds(owner)
    }
}

const PREFIX: &str = "core.grant.install.";
/// Room for the key: the prefix and a plugin id.
const KEY_MAX: usize = 128;

/// The setting that holds the grant of the plugin `id`, in `buffer`;
/// `None` for an id that does not fit.
fn grant_key<'b>(id: &str, buffer: &'b mut [u8; KEY_MAX]) -> Option<&'b str> {
    let end = PREFIX.len().checked_add(id.len())?;
    let (head, tail) = buffer.get_mut(..end)?.split_at_mut(PREFIX.len());
    head.copy_from_slice(PREFIX.as_bytes());
    tail.copy_from_slice(id.as_bytes());
    core::str::from_utf8(&buffer[..end]).ok()
}

impl Kernel {
    /// Reads the grants of the plugins that have a plugin job and hands
    /// them to the driver, before it steps. A busy driver is left alone: it
    /// steps nothing then either.
    pub(crate) fn feed_install_grants(&self, driver: &Rc<RefCell<Driver>>) {
        let mut owners = [0; REQUESTS_MAX];
        let Ok(count) = driver
            .try_borrow()
            .map(|driver| driver.plugin_owners(&mut owners))
        else {
            return;
        };
        if count == 0 {
            return;
        }
        let mut granted = Granted::NONE;
        for &owner in &owners[..count] {
            let mut on = false;
            self.shared.native_instances.with_id(owner, &mut |id| {
                let mut buffer = [0; KEY_MAX];
                on = grant_key(id, &mut buffer).is_some_and(|key| {
                    matches!(
                        self.shared.settings.get(key),
                        Some(SettingValue::Bool(true))
                    )
                });
            });
            if on {
                granted.add(owner);
            }
        }
        if let Ok(mut driver) = driver.try_borrow_mut() {
            driver.feed_install_grants(granted);
        }
    }
}

#[cfg(test)]
#[path = "install_grants_tests.rs"]
mod tests;
