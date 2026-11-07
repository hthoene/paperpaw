//! Lossless handover parameters. Transport uses Kernel's service batch encoder.
use super::{Error, wire};

/// One borrowed handover parameter, checked without allocating or truncating.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Param<'a> {
    key: &'a str,
    value: &'a str,
}
impl<'a> Param<'a> {
    /// A caller-owned parameter. The core namespace belongs to the host.
    ///
    /// # Errors
    /// Invalid for key syntax or the core namespace; NoSpace above a field cap.
    pub fn new(key: &'a str, value: &'a str) -> Result<Self, Error> {
        if key.len() > wire::SERVICE_UI_PARAM_KEY_MAX_BYTES
            || value.len() > wire::SERVICE_UI_PARAM_VALUE_MAX_BYTES
        {
            return Err(Error::NoSpace);
        }
        if key.is_empty()
            || key.starts_with("core.")
            || !key.bytes().all(|byte| {
                byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || matches!(byte, b'_' | b'.' | b'-')
            })
        {
            return Err(Error::Invalid);
        }
        Ok(Self { key, value })
    }
    /// The exact parameter key.
    #[must_use]
    pub const fn key(self) -> &'a str {
        self.key
    }
    /// The complete value, including any newlines.
    #[must_use]
    pub const fn value(self) -> &'a str {
        self.value
    }
    pub(super) const fn record(self) -> wire::Record<'a> {
        wire::Record::ServiceUiParam(wire::ServiceUiParam {
            key: self.key,
            value: self.value,
        })
    }
    pub(super) fn decode(row: wire::Record<'a>) -> Result<Self, Error> {
        let wire::Record::ServiceUiParam(row) = row else {
            return Err(Error::State);
        };
        // Host-created core.request/core.kind are valid in a read reply.
        if row.key.is_empty()
            || !row.key.bytes().all(|byte| {
                byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || matches!(byte, b'_' | b'.' | b'-')
            })
        {
            return Err(Error::Invalid);
        }
        Ok(Self {
            key: row.key,
            value: row.value,
        })
    }
}

pub(super) fn validate(params: &[Param<'_>]) -> Result<(), Error> {
    if params.iter().any(|param| param.key.starts_with("core.")) {
        return Err(Error::Invalid);
    }
    validate_content(params)
}
pub(super) fn validate_content(params: &[Param<'_>]) -> Result<(), Error> {
    if params.len() > 32 {
        return Err(Error::NoSpace);
    }
    let mut bytes = 0usize;
    for (index, param) in params.iter().enumerate() {
        bytes = bytes
            .checked_add(param.key.len())
            .and_then(|n| n.checked_add(param.value.len()))
            .ok_or(Error::NoSpace)?;
        if bytes > 1024 {
            return Err(Error::NoSpace);
        }
        if params[..index]
            .iter()
            .any(|earlier| earlier.key == param.key)
        {
            return Err(Error::Invalid);
        }
    }
    Ok(())
}
