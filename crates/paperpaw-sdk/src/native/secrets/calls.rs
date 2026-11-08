//! Per-record codecs link only the secret request and reply types used here.
use super::paging::SecretPage;
use super::{
    Error, Host, RandomBytes, SecretList, SecretName, SecretValue, SecretWrite, Secrets, schema,
    wire,
};

impl Secrets {
    fn call(
        &self,
        host: &Host<'_>,
        op: u32,
        record: &dyn wire::WireEncode,
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<usize, Error> {
        let cap = self.service.max_reply(op).ok_or(Error::Unsupported)?;
        let n = wire::records_encode_of(&[record], input).map_err(Error::from_status)?;
        let n = self.service.call(host, op, &input[..n], out)?;
        if n > cap {
            return Err(Error::Invalid);
        }
        Ok(n)
    }
    /// Read only the calling plugin's own value. Editor access cannot read values.
    /// Output retains the value while the returned view borrows it. After use,
    /// call [`super::wipe`] on the output buffer. A malformed reply is wiped
    /// through its reported length; a call failure wipes the whole output.
    ///
    /// # Errors
    /// Returns local name/framing/cap errors or the host's refusal.
    pub fn get<'a>(
        &self,
        host: &Host<'_>,
        name: &str,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<SecretValue<'a>, Error> {
        let name = SecretName::new(name)?;
        let row = wire::ServiceSecName {
            plugin: "",
            name: name.name,
        };
        let cap = self
            .service
            .max_reply(schema::OP_GET)
            .ok_or(Error::Unsupported)?;
        let encoded = wire::records_encode_of(&[&row], input).map_err(Error::from_status)?;
        if out.len() < cap {
            return Err(Error::NoSpace);
        }
        let n = match self
            .service
            .call(host, schema::OP_GET, &input[..encoded], out)
        {
            Ok(n) => n,
            Err(error) => {
                // A failing host may have written bytes without reporting a length.
                wipe(out);
                return Err(error);
            }
        };
        let valid = (|| {
            if n > cap {
                return Err(Error::Invalid);
            }
            let row = one::<wire::ServiceSecValue<'_>>(&out[..n])?;
            super::types::label(row.label, wire::SERVICE_SEC_VALUE_LABEL_MAX_BYTES)?;
            if row.flags & !1 != 0 {
                return Err(Error::Invalid);
            }
            Ok(())
        })();
        if let Err(error) = valid {
            wipe(&mut out[..n]);
            return Err(error);
        }
        // Decode again after validation so the failure path can exclusively wipe
        // output without extending a successful reply's borrow across that path.
        one::<wire::ServiceSecValue<'a>>(&out[..n]).map(SecretValue)
    }
    /// List names and labels, never values, from the requested row cursor.
    /// Every row and terminal marker is checked before exposing a page.
    ///
    /// # Errors
    /// Invalid for malformed pages; otherwise local caps or host refusals.
    pub fn list<'a>(
        &self,
        host: &Host<'_>,
        query: SecretList,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<SecretPage<'a>, Error> {
        let row = wire::ServiceSecList {
            flags: query.flags,
            cursor: query.cursor,
        };
        let n = self.call(host, schema::OP_LIST, &row, input, out)?;
        SecretPage::decode(&out[..n], query)
    }
    /// Save an owner's value. An editor may replace only an existing changeable
    /// secret, preserving its changeable flag; the host enforces this rule.
    /// The encoded input is cleared on success and refusal before returning.
    ///
    /// # Errors
    /// Returns local encoding/cap errors, malformed replies or host refusals.
    pub fn keep(
        &self,
        host: &Host<'_>,
        value: SecretWrite<'_>,
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<(), Error> {
        let row = wire::ServiceSecKeep {
            plugin: value.name.plugin,
            name: value.name.name,
            label: value.label,
            flags: value.flags,
            value: value.value,
        };
        let result = self.call(host, schema::OP_KEEP, &row, input, out);
        wipe(input);
        if result? != 0 {
            return Err(Error::Invalid);
        }
        Ok(())
    }
    /// Remove a named secret. A missing secret succeeds; foreign owners require
    /// `uses.secrets`, even when the value was not user-changeable.
    ///
    /// # Errors
    /// Returns local caps, malformed replies or the host's refusal.
    pub fn remove(
        &self,
        host: &Host<'_>,
        name: SecretName<'_>,
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<(), Error> {
        let row = wire::ServiceSecName {
            plugin: name.plugin,
            name: name.name,
        };
        if self.call(host, schema::OP_REMOVE, &row, input, out)? != 0 {
            return Err(Error::Invalid);
        }
        Ok(())
    }
    /// Draw 1-256 bytes from the device source. Strength is reported for this
    /// draw; no weaker fallback is substituted when the host refuses it.
    /// Call [`super::wipe`] on output after consuming bytes used as key material.
    ///
    /// # Errors
    /// Invalid for a zero/over-cap length or malformed reply; otherwise buffer
    /// caps or the host refusal, including Unsupported without a source.
    pub fn random<'a>(
        &self,
        host: &Host<'_>,
        len: usize,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<RandomBytes<'a>, Error> {
        if len == 0 || len > wire::SERVICE_SEC_BYTES_DATA_MAX_BYTES {
            return Err(Error::Invalid);
        }
        let ask = wire::ServiceSecRandom {
            len: u32::try_from(len).map_err(|_| Error::Invalid)?,
        };
        let n = self.call(host, schema::OP_RANDOM, &ask, input, out)?;
        let row = one::<wire::ServiceSecBytes<'a>>(&out[..n])?;
        if row.data.len() != len || row.flags & !1 != 0 {
            return Err(Error::Invalid);
        }
        Ok(RandomBytes(row))
    }
}
fn one<'a, T: wire::WireRecord<'a>>(bytes: &'a [u8]) -> Result<T, Error> {
    let raw = wire::RawRecord::one(bytes).map_err(Error::from_status)?;
    if raw.tag() != T::TAG {
        return Err(Error::State);
    }
    raw.decode::<T>().map_err(Error::from_status)
}
/// Clear a caller-owned buffer with volatile writes and a compiler fence.
/// Call this after consuming secret values or random bytes used as key material,
/// once every borrowed view into that buffer is no longer used. This clears only
/// the supplied memory; callers must also clear any copies they made.
pub fn wipe(bytes: &mut [u8]) {
    for byte in bytes {
        // SAFETY: each byte is exclusively borrowed and remains writable.
        unsafe {
            core::ptr::write_volatile(byte, 0);
        }
    }
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
}
