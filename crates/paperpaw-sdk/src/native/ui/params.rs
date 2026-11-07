//! Lossless handover parameters. Transport uses Kernel's service batch encoder.
use super::{Error, QuestionKind, RequestId, wire};

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
    /// Decode the host's dialog request parameter. Other keys return None.
    ///
    /// # Errors
    /// Invalid unless core.request is canonical decimal in 1..=u32::MAX.
    pub fn request(self) -> Result<Option<RequestId>, Error> {
        if self.key != "core.request" {
            return Ok(None);
        }
        if !matches!(self.value.as_bytes().first(), Some(b'1'..=b'9'))
            || !self.value.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(Error::Invalid);
        }
        let raw = self.value.parse::<u32>().map_err(|_| Error::Invalid)?;
        RequestId::new(raw).map(Some).ok_or(Error::Invalid)
    }
    /// Decode the host's dialog kind parameter. Other keys return None.
    ///
    /// # Errors
    /// Invalid unless core.kind is exactly one decimal digit from 0 to 3.
    pub fn kind(self) -> Result<Option<QuestionKind>, Error> {
        if self.key != "core.kind" {
            return Ok(None);
        }
        match self.value.as_bytes() {
            [digit @ b'0'..=b'3'] => QuestionKind::decode(u32::from(digit - b'0')).map(Some),
            _ => Err(Error::Invalid),
        }
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

pub(super) fn validate(params: &[Param<'_>], max_rows: usize) -> Result<(), Error> {
    if params.len() > max_rows {
        return Err(Error::NoSpace);
    }
    for (index, param) in params.iter().enumerate() {
        if param.key.starts_with("core.")
            || params[..index]
                .iter()
                .any(|earlier| earlier.key == param.key)
        {
            return Err(Error::Invalid);
        }
    }
    validate_content(params)
}
pub(super) fn validate_content(params: &[Param<'_>]) -> Result<(), Error> {
    let mut bytes = 0usize;
    for param in params {
        bytes = bytes
            .checked_add(param.key.len())
            .and_then(|n| n.checked_add(param.value.len()))
            .ok_or(Error::NoSpace)?;
        if bytes > 1024 {
            return Err(Error::NoSpace);
        }
    }
    Ok(())
}
