//! Callback-owned bytes in the same generation-checked registry as fonts.
use super::{Kind, OwnerResourceView, OwnerResources, Release, Resource, Table};
use alloc::vec::Vec;
use paperpaw_native::host_glue::{ERR_BUSY, ERR_INVALID, ERR_NO_SPACE, ERR_STATE};

#[derive(Clone, Copy)]
pub(super) struct Limits {
    pub(super) page: usize,
    live_bytes: usize,
    live_count: usize,
}

impl Limits {
    // The 384-byte payload shares the 512 charged-byte stream-chunk row.
    pub(super) const X4: Self = Self {
        page: 384,
        live_bytes: 384,
        live_count: 1,
    };

    #[cfg(test)]
    pub(super) const LARGE: Self = Self {
        page: 4096,
        live_bytes: 16 * 1024,
        live_count: 1024,
    };
}

pub(crate) struct Blob {
    pub(crate) bytes: Vec<u8>,
    pub(crate) callback: u64,
}

impl OwnerResources {
    pub(crate) fn blob_page_bytes(&self) -> usize {
        self.table.blob_limits.page
    }

    /// Family/registry capacity precedes the device-wide temporary budget.
    pub(crate) fn check_blob_capacity(&self, bytes: usize) -> Result<(), i32> {
        let handles = self.table.handles.try_borrow().map_err(|_| ERR_BUSY)?;
        handles
            .check_insert(self.owner, Kind::Blob)
            .map_err(crate::abi7::handles::Error::status)?;
        let limits = self.table.blob_limits;
        if bytes > limits.page {
            return Err(ERR_NO_SPACE);
        }
        let mut count = 0usize;
        let mut total = 0usize;
        handles.visit_kind(Kind::Blob, |value| {
            if let Resource::Blob(blob) = value {
                count += 1;
                // Charge the allocation's capacity, not just encoded bytes.
                total = total.saturating_add(blob.bytes.capacity());
            }
        });
        if count >= limits.live_count || total.saturating_add(bytes) > limits.live_bytes {
            return Err(ERR_BUSY);
        }
        Ok(())
    }

    pub(crate) fn insert_blob(&self, blob: Blob) -> Result<u32, i32> {
        self.check_blob_capacity(blob.bytes.capacity())?;
        if blob.callback == 0 {
            return Err(ERR_INVALID);
        }
        let result = {
            let mut handles = self.table.handles.try_borrow_mut().map_err(|_| ERR_BUSY)?;
            handles.insert(self.owner, Kind::Blob, Resource::Blob(blob))
        };
        // A refused payload drops after the registry borrow ends.
        result.map_err(|(error, _)| error.status())
    }

    pub(crate) fn with_blob<R>(
        &self,
        handle: u32,
        callback: u64,
        visit: impl FnOnce(&[u8]) -> Result<R, i32>,
    ) -> Result<R, i32> {
        let handles = self.table.handles.try_borrow().map_err(|_| ERR_BUSY)?;
        let value = handles
            .get(self.owner, Kind::Blob, handle)
            .map_err(crate::abi7::handles::Error::status)?;
        match value {
            Resource::Blob(blob) if callback != 0 && blob.callback == callback => {
                visit(&blob.bytes)
            }
            _ => Err(ERR_STATE),
        }
    }

    pub(crate) fn release_blob(&self, handle: u32, callback: u64) -> Result<(), i32> {
        self.with_blob(handle, callback, |_| Ok(()))?;
        crate::abi7::handles::release(&self.table.handles, self.owner, Kind::Blob, handle)
            .map_err(crate::abi7::handles::Error::status)
    }

    /// The blobs of the callback that ended expire (its context's cleanup).
    pub(crate) fn expire_blobs(&self) -> Result<(), i32> {
        expire(&self.table, self.owner)
    }
}

impl OwnerResourceView {
    /// Tests: a view's blobs expire (a hosted plugin's expire through its
    /// context's resources).
    #[cfg(test)]
    pub(crate) fn expire_blobs(&self) -> Result<(), i32> {
        expire(&self.table, self.owner)
    }
}

fn expire(table: &Table, owner: u64) -> Result<(), i32> {
    loop {
        let value = table
            .handles
            .try_borrow_mut()
            .map_err(|_| ERR_BUSY)?
            .take_owner_kind(owner, Kind::Blob);
        let Some(value) = value else {
            return Ok(());
        };
        // The registry borrow ends before releasing the payload.
        value.release();
    }
}

#[cfg(test)]
#[path = "blobs/tests.rs"]
mod tests;
