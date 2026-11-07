//! Call-service handles retain no callback binding.
use super::{Error, Host, status};
use crate::native::wire::services_wire::{ServiceSchema, service_schema};
use paperpaw_native::host_glue::{PpBuffer, PpBytes};

/// A schema-checked service handle owned by this plugin across callbacks.
/// Each call uses the current callback's host. Call [`Self::release`] when done;
/// there is no Drop release because no callback host is retained. Owner teardown
/// releases outstanding handles.
#[derive(Debug)]
#[must_use = "release the service handle when it is no longer needed"]
pub struct Service {
    handle: u32,
    schema: &'static ServiceSchema,
    minor: u16,
}

impl Service {
    /// Open a call service declared in this plugin's signed metadata.
    ///
    /// # Errors
    /// Unsupported for an unknown schema/version, or a test-only schema outside
    /// SDK unit tests. Otherwise returns the host's permission or resource error.
    pub fn open(host: &Host<'_>, key: &str, major: u16, minor: u16) -> Result<Self, Error> {
        let schema = service_schema(key, major).ok_or(Error::Unsupported)?;
        if minor > schema.revision || schema.test_only && !cfg!(test) {
            return Err(Error::Unsupported);
        }
        let mut handle = 0;
        // SAFETY: the callback host and key live for this call; the output
        // handle is local, aligned and disjoint from the input.
        status(unsafe {
            (host.table.service_open)(
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
        Ok(Self {
            handle,
            schema,
            minor,
        })
    }

    /// Invoke an operation using the current callback's host and caller buffers.
    /// The stage or service family decodes the returned payload.
    ///
    /// # Errors
    /// Unsupported for an unknown operation or one newer than the opened minor;
    /// NoSpace if input exceeds its cap or output cannot hold its largest reply.
    /// Returns host refusals, or Invalid for a returned count beyond capacity.
    pub fn call(
        &self,
        host: &Host<'_>,
        op: u32,
        input: &[u8],
        out: &mut [u8],
    ) -> Result<usize, Error> {
        let operation = self
            .schema
            .op(op)
            .filter(|operation| operation.since <= self.minor)
            .ok_or(Error::Unsupported)?;
        if input.len() > operation.max_input || out.len() < operation.max_reply {
            return Err(Error::NoSpace);
        }
        // SAFETY: this callback's host and input are live; output is exclusive
        // and disjoint from input for the whole call.
        let count = unsafe {
            (host.table.service_call)(
                host.ctx,
                self.handle,
                op,
                PpBytes {
                    ptr: input.as_ptr(),
                    len: input.len(),
                },
                PpBuffer {
                    ptr: out.as_mut_ptr(),
                    cap: out.len(),
                },
            )
        };
        let count = usize::try_from(count).map_err(|_| Error::from_status(count))?;
        if count > out.len() {
            return Err(Error::Invalid);
        }
        Ok(count)
    }

    /// Largest reply for an operation supported by the opened minor version.
    /// Use this to size the caller-owned output buffer.
    pub fn max_reply(&self, op: u32) -> Option<usize> {
        self.schema
            .op(op)
            .filter(|operation| operation.since <= self.minor)
            .map(|operation| operation.max_reply)
    }

    /// Release the handle using the current callback's host, consuming it even
    /// when release fails. Teardown also reclaims outstanding handles.
    ///
    /// # Errors
    /// Returns the host refusal, including State for stale or foreign handles.
    pub fn release(self, host: &Host<'_>) -> Result<(), Error> {
        // SAFETY: the current host is live; this owned handle is consumed once.
        status(unsafe { (host.table.handle_release)(host.ctx, self.handle) })
    }
}

#[cfg(test)]
mod tests;
