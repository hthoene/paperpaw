//! Callback-scoped snapshot services. Blobs expire with their callback; dropping
//! one releases it early so the next query can use the device's bounded slot.
use super::{Error, Host, binding::status};
use core::fmt;
use paperpaw_native::host_glue::{PpBuffer, PpBytes};

mod usage;
pub use usage::{
    PageEnd, ServicePluginUsage, ServiceUsage, device_usage, for_each_plugin_usage, plugin_name,
    plugin_usage_page, work_note,
};

/// A snapshot blob borrowed from this callback's host binding. Neither the
/// binding nor this blob can be sent between threads or retained across callbacks.
///
/// ```compile_fail
/// use paperpaw_sdk::native::{Host, services::{Blob, query}};
/// fn escape<'a>(host: &'a Host<'a>) -> Blob<'static> {
///     query(host, "core.usage", 1, 0).unwrap()
/// }
/// ```
pub struct Blob<'cb> {
    host: &'cb Host<'cb>,
    handle: Option<u32>,
}
impl fmt::Debug for Blob<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Blob")
            .field("handle", &self.handle)
            .finish_non_exhaustive()
    }
}

/// Query a data service at its own major/minor version, not the ABI minor.
/// Service names and arguments follow the 128-byte ASCII key grammar. Metadata
/// permissions and version support are checked by the host after validation.
///
/// # Errors
/// Invalid keys are refused before calling the host. Returns host errors without
/// creating a blob, including Busy when the device's one blob/chunk slot is used.
pub fn query<'cb>(
    host: &'cb Host<'cb>,
    key: &str,
    major: u16,
    minor: u16,
) -> Result<Blob<'cb>, Error> {
    validate_key(key)?;
    let mut handle = 0;
    // SAFETY: callback binding/key are live; output handle storage is disjoint.
    status(unsafe {
        (host.table.services_query)(
            host.ctx,
            PpBytes {
                ptr: key.as_ptr(),
                len: key.len(),
            },
            major,
            minor,
            &mut handle,
        )
    })?;
    Ok(Blob {
        host,
        handle: Some(handle),
    })
}

impl Blob<'_> {
    /// Copy bytes from the snapshot. Short reads succeed; zero means the end.
    ///
    /// # Errors
    /// Invalid for a buffer over 4096 bytes or a returned count beyond capacity;
    /// State for expired/stale handles, or the host's other refusal.
    pub fn read(&self, offset: u64, out: &mut [u8]) -> Result<usize, Error> {
        if out.len() > 4096 {
            return Err(Error::Invalid);
        }
        let handle = self.handle.ok_or(Error::State)?;
        // SAFETY: callback binding and handle lease are live; output is exclusive.
        let n = unsafe {
            (self.host.table.blob_read)(
                self.host.ctx,
                handle,
                offset,
                PpBuffer {
                    ptr: out.as_mut_ptr(),
                    cap: out.len(),
                },
            )
        };
        let n = usize::try_from(n).map_err(|_| Error::from_status(n))?;
        if n > out.len() {
            return Err(Error::Invalid);
        }
        Ok(n)
    }

    /// Release now and report the status. This consumes the blob, including on
    /// error, so Drop never releases the same handle a second time.
    ///
    /// # Errors
    /// Returns the host refusal, including State when the callback lease expired.
    pub fn release(mut self) -> Result<(), Error> {
        self.release_inner()
    }

    fn release_inner(&mut self) -> Result<(), Error> {
        let Some(handle) = self.handle.take() else {
            return Ok(());
        };
        // SAFETY: the same callback binding created this handle, released once.
        status(unsafe { (self.host.table.blob_release)(self.host.ctx, handle) })
    }
}
impl Drop for Blob<'_> {
    fn drop(&mut self) {
        // Best-effort RAII cleanup has no error channel; callback expiry also
        // reclaims the lease. Call release explicitly when its status matters.
        let _ = self.release_inner();
    }
}

fn validate_key(key: &str) -> Result<(), Error> {
    if key.len() > 128 || !key.is_ascii() {
        return Err(Error::Invalid);
    }
    let (service, arg) = key
        .split_once('/')
        .map_or((key, None), |(s, a)| (s, Some(a)));
    let mut segments = 0;
    for s in service.split('.') {
        segments += 1;
        if s.len() > 32
            || !s.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
            || !s
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        {
            return Err(Error::Invalid);
        }
    }
    if segments < 2
        || arg.is_some_and(|a| {
            a.is_empty()
                || a.len() > 64
                || !a
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        })
    {
        return Err(Error::Invalid);
    }
    match (service, arg) {
        ("core.usage", Some(_)) | ("core.plugin" | "core.work-note", None) => Err(Error::Invalid),
        ("core.plugin-usage", Some(a)) => {
            let n = a.strip_prefix("from.").ok_or(Error::Invalid)?;
            if n.is_empty()
                || (n.len() > 1 && n.starts_with('0'))
                || !n.bytes().all(|b| b.is_ascii_digit())
                || n.parse::<u32>().is_err()
            {
                return Err(Error::Invalid);
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests;
