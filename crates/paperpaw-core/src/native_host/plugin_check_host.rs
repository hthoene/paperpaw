//! The check of a code file built for the computer the kernel runs on
//! (`code-host.so`, feature `host-code`, #278): the library with the
//! author's signature trailer after it. It has no header: the signature
//! covers the library, the digest is that of the whole file (what the
//! loader holds it to), and the metadata record is the one the library
//! exports ([`HostCode::record`]). The steps then go on at the trust.

use alloc::vec::Vec;
use paperpaw_native::{
    metadata,
    signature::{SIGNATURE_LEN, Signature},
};
use paperpaw_sdk::digest::sha256::Sha256;
use paperpaw_sdk::native::CODE_FILES;

use super::{Phase, PluginCheck, read_exact, signature_holds};
use crate::native_host::plugin_job::{JobEnv, JobStep, reason};
use crate::native_host::shared::try_box;

/// The largest library read (the emulator's own limit).
pub(super) const MAX_FILE: u64 = 256 << 20;

/// What the checker needs of the signature trailer of `size` bytes' file.
fn trailer_at(size: u64) -> Option<u64> {
    size.checked_sub(SIGNATURE_LEN as u64)
}

impl PluginCheck {
    /// Whether the code file at `index` of `CODE_FILES` is the one this
    /// device runs.
    pub(super) fn is_host_code(env: &JobEnv<'_>, index: usize) -> bool {
        env.host
            .host_code
            .is_some_and(|host| CODE_FILES.get(index).copied() == Some(host.file))
    }

    pub(super) fn host_header(&mut self, env: &JobEnv<'_>) -> JobStep {
        let Some(path) = self.code_path() else {
            return self.end(reason::OTHER);
        };
        self.code = path;
        let storage = env.storage;
        let size = match storage.size(&self.code) {
            Ok(size) => size,
            Err(why) => return self.end(super::card_reason(why)),
        };
        let Some(at) = trailer_at(size).filter(|_| size <= MAX_FILE) else {
            return self.end(reason::DAMAGED);
        };
        let mut trailer = [0_u8; SIGNATURE_LEN];
        if let Err(why) = read_exact(storage, &self.code, at, &mut trailer) {
            return self.end(why);
        }
        // A library with no trailer is code with no author's signature.
        let Some(signature) = Signature::read(&trailer) else {
            return self.end(reason::UNSIGNED);
        };
        if u64::from(signature.signed_len) != at {
            return self.end(reason::DAMAGED);
        }
        let Ok(check) = signature.check() else {
            return self.end(reason::DAMAGED);
        };
        let Some(check) = try_box(check) else {
            return self.end(reason::OTHER);
        };
        self.check = Some(check);
        self.signature = Some(signature);
        self.sha = Some(Sha256::new());
        self.size = size;
        self.at = 0;
        self.phase = Phase::HostBody;
        JobStep::More
    }

    pub(super) fn host_body(&mut self, env: &JobEnv<'_>) -> JobStep {
        let len = (self.size - self.at).min(self.buf.len() as u64) as usize;
        if let Err(why) = read_exact(env.storage, &self.code, self.at, &mut self.buf[..len]) {
            return self.end(why);
        }
        if let Some(sha) = self.sha.as_mut() {
            sha.update(&self.buf[..len]);
        }
        let signed = (self.size - SIGNATURE_LEN as u64).saturating_sub(self.at);
        let signed = signed.min(len as u64) as usize;
        if let Some(check) = self.check.as_mut() {
            check.absorb(&self.buf[..signed]);
        }
        self.at += len as u64;
        if self.at >= self.size {
            self.phase = Phase::HostVerify;
        }
        JobStep::More
    }

    pub(super) fn host_verify(&mut self) -> JobStep {
        let (Some(sha), Some(check)) = (self.sha.take(), self.check.take()) else {
            return self.end(reason::OTHER);
        };
        self.digest = Some(sha.finish());
        if !signature_holds(check) {
            return self.end(reason::DAMAGED);
        }
        self.phase = Phase::HostRecord;
        JobStep::More
    }

    pub(super) fn host_record(&mut self, env: &JobEnv<'_>) -> JobStep {
        let Some(host) = env.host.host_code else {
            return self.end(reason::OTHER);
        };
        let library = match read_library(env, &self.code, self.size) {
            Ok(library) => library,
            Err(why) => return self.end(why),
        };
        let Some(record) = (host.record)(&library) else {
            return self.end(reason::DAMAGED);
        };
        match metadata::check(record, &env.host.offered) {
            Ok(_) => {}
            Err(metadata::Refusal::Malformed) => return self.end(reason::DAMAGED),
            Err(metadata::Refusal::Incompatible) => return self.end(reason::OTHER_CHIP),
            Err(_) => return self.end(reason::NEEDS_NEWER),
        }
        self.phase = Phase::Trust;
        JobStep::More
    }
}

/// The library of the file at `path` (`size` bytes with its trailer).
pub(crate) fn read_library(env: &JobEnv<'_>, path: &str, size: u64) -> Result<Vec<u8>, u8> {
    let len =
        usize::try_from(size.saturating_sub(SIGNATURE_LEN as u64)).map_err(|_| reason::OTHER)?;
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(len).map_err(|_| reason::OTHER)?;
    bytes.resize(len, 0);
    read_exact(env.storage, path, 0, &mut bytes)?;
    Ok(bytes)
}

#[cfg(test)]
#[path = "plugin_check_host_tests.rs"]
mod tests;
