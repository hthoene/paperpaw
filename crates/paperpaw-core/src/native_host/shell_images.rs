//! Deferred image work shared by native command callbacks and the main loop.
use super::{
    image_service::{
        commands::ImageRoots,
        prepare::{Env, SweepCursor},
    },
    resources::HostResources,
};
use alloc::string::String;
use paperpaw_native::host_glue::{ERR_DENIED, ERR_INVALID, ERR_NO_MEMORY, ERR_UNSUPPORTED};
use paperpaw_platform::Storage;
use paperpaw_sdk::Id;

pub(super) struct OwnerRoots<'a> {
    owner: u64,
    plugin: &'a Id,
    storage_root: bool,
}

impl<'a> OwnerRoots<'a> {
    pub(super) fn new(owner: u64, plugin: &'a Id, storage_root: bool) -> Self {
        Self {
            owner,
            plugin,
            storage_root,
        }
    }
}

impl ImageRoots for OwnerRoots<'_> {
    fn resolve(&self, owner: u64, root: u32, path: &str) -> Result<String, i32> {
        if owner != self.owner {
            return Err(ERR_DENIED);
        }
        if path.is_empty()
            || path.contains(['\\', '\0'])
            || path.split('/').any(|part| part.is_empty() || part == "..")
        {
            return Err(ERR_INVALID);
        }
        let prefix = match root {
            0 => ".paperpaw/plugins/",
            1 if self.storage_root => "",
            1 => return Err(ERR_DENIED),
            _ => return Err(ERR_UNSUPPORTED),
        };
        let mut joined = String::new();
        let extra = if root == 0 {
            self.plugin.as_str().len() + 1
        } else {
            0
        };
        joined
            .try_reserve_exact(prefix.len() + extra + path.len())
            .map_err(|_| ERR_NO_MEMORY)?;
        joined.push_str(prefix);
        if root == 0 {
            joined.push_str(self.plugin.as_str());
            joined.push('/');
        }
        joined.push_str(path);
        Ok(joined)
    }
}

/// Copies an encoded image-ready event into its host owner's event queue.
/// An error leaves the held answer pending. Never invoke plugin code here.
pub type ImageReadyQueue<'a> = dyn FnMut(u64, &[u8]) -> Result<(), i32> + 'a;

/// Device memory limits for one image decode turn.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct ImageProfile(Env);
impl ImageProfile {
    /// The X4 profile: 1,600-pixel side and 8 KiB source-row ceilings.
    #[must_use]
    pub const fn x4(screen_side: u32, colour: bool) -> Self {
        Self(Env::x4(screen_side, colour))
    }
    /// Desktop/Android profile with a 4,096-pixel side ceiling.
    #[must_use]
    pub const fn desktop(screen_side: u32, colour: bool) -> Self {
        Self(Env {
            screen: screen_side,
            colour,
            max_side: 4096,
            png_row_max: usize::MAX,
        })
    }
}

/// Progress of the boot cache sweep. Keep the same value until it finishes.
#[derive(Default, Debug)]
#[non_exhaustive]
pub struct ImageSweep(SweepCursor);

impl HostResources {
    /// Remove stale prepared images at boot, before admitting native plugins.
    /// Call once per idle turn with no input pending. At most eight files are
    /// removed per call; keep the cursor and repeat while this returns true.
    ///
    /// # Errors
    /// Returns the cache listing, allocation or busy error. Retry after failure.
    pub fn sweep_image_cache(
        &self,
        storage: &dyn Storage,
        sweep: &mut ImageSweep,
    ) -> Result<bool, i32> {
        self.hosts().images().sweep_cache(storage, 8, &mut sweep.0)
    }

    /// One outer main-loop turn: decode at most one image and queue one answer
    /// for its host owner. `queue` must copy the record into that owner's event
    /// queue without invoking plugin code. An error retains the answer for retry.
    /// Returns whether work or an answer remains pending.
    ///
    /// # Errors
    /// Returns a busy or queue error. Call again on a later turn.
    pub fn image_turn(
        &self,
        storage: &dyn Storage,
        profile: ImageProfile,
        now_ms: u64,
        queue: &mut ImageReadyQueue<'_>,
    ) -> Result<bool, i32> {
        let images = self.hosts().images();
        // A full decoder queue must still make progress before applying the
        // failed batch again. Every failure leaves its remaining commands held.
        let applied = self.hosts().image_work.apply(images);
        images.prepare_step(storage, profile.0)?;
        images.take_ready(&mut |ready| {
            let mut bytes = [0; 64];
            let len = super::registrations::records_encode(&[ready.record(now_ms)], &mut bytes)?;
            queue(ready.owner, &bytes[..len])
        })?;
        applied?;
        Ok(images.prepare_pending() || self.hosts().image_work.pending())
    }
}

#[cfg(test)]
#[path = "shell_images_tests.rs"]
mod tests;
