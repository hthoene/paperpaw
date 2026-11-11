//! The store's three views of the host, over one set of service handles: what
//! the fetch of the list and of a page needs ([`HostWire`], `native_load::Wire`),
//! what the download of a plugin's files needs ([`DownloadWire`],
//! `native_download::Wire`) and what the install sends the host
//! ([`InstallWire`], `native_install::Wire`). Each is made for the length of one
//! callback; the handles live in the plugin.
//!
//! **Memory:** `core.net` is opened when first needed; a request is written
//! from a block of [`REQUEST_INPUT`] bytes and answered into [`REPLY`] bytes,
//! both freed when the call returns; an install's batch (the install, the
//! author's record and up to six rotations) needs about a kilobyte of scratch,
//! the revocations 4 KiB, each held for one call.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::Storage;
use paperpaw_sdk::native::{
    Error, Host,
    commands::{
        CommandSender, Evidence, InstallRoot, MAX_REVOCATIONS, Revocation, Rotation as Statement,
    },
    net::{FileRoot, Header, Net, REPLY, REQUEST_INPUT, Request, RequestId},
    storage::DataStorage,
};
use paperpaw_ui_kit::{mem, text::copy};

use crate::native_device::{self, Facts};
use crate::native_install::Proof;
use crate::native_model::Revoked;

/// The `User-Agent` of interface 6's requests.
const USER_AGENT: &str = "PaperPaw/0.1 (e-reader)";
/// Scratch for the batch of one install (`install_plugin_with`: about 1 KiB).
const INSTALL_SCRATCH: usize = 1536;
/// Scratch for a batch of revocations.
const REVOKE_SCRATCH: usize = 4096;

/// The handles of the services, opened when first needed.
#[derive(Debug, Default)]
pub struct Handles {
    net: Option<Net>,
}

impl Handles {
    fn net(&mut self, host: &Host<'_>) -> Result<&Net, Error> {
        if self.net.is_none() {
            self.net = Some(Net::open(host)?);
        }
        self.net.as_ref().ok_or(Error::State)
    }

    fn get(
        &mut self,
        host: &Host<'_>,
        url: &str,
        file: &str,
        token: u32,
        max: u64,
    ) -> Result<RequestId, Error> {
        let mut input = mem::buffer(REQUEST_INPUT)?;
        let mut out = mem::buffer(REPLY)?;
        let headers = [Header::new("User-Agent", USER_AGENT)?];
        let request = Request::get(token, url)?
            .headers(&headers)?
            .save_to(FileRoot::Data, file)?
            .max_file(max)?;
        self.net(host)?
            .request(host, &request, &mut input, &mut out)
    }

    fn cancel(&mut self, host: &Host<'_>, token: u32) {
        // Only the request of this token is ended: the net service cancels by id
        // (the token is the plugin's own number, kept by the host).
        if let (Ok(mut input), Ok(mut out)) = (mem::buffer(REQUEST_INPUT), mem::buffer(REPLY))
            && let Ok(net) = self.net(host)
        {
            net.cancel(host, token, &mut input, &mut out).ok();
        }
    }
}

/// The host as the fetch of the list and of a page uses it.
pub struct HostWire<'a, 'h> {
    host: &'a Host<'h>,
    handles: &'a mut Handles,
    storage: DataStorage<'a, 'h>,
}

impl core::fmt::Debug for HostWire<'_, '_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("HostWire").finish_non_exhaustive()
    }
}

impl<'a, 'h> HostWire<'a, 'h> {
    /// The fetch's view of `host`.
    pub fn new(host: &'a Host<'h>, handles: &'a mut Handles) -> Self {
        Self {
            host,
            handles,
            storage: DataStorage::new(host),
        }
    }
}

impl crate::native_load::Wire for HostWire<'_, '_> {
    fn storage(&self) -> &dyn Storage {
        &self.storage
    }

    fn facts(&mut self) -> Result<Option<Facts>, Error> {
        let catalog = native_device::catalog_setting(self.host)?;
        Facts::read(self.host, &catalog)
    }

    fn catalog(&mut self) -> Result<(String, bool), Error> {
        Ok((
            native_device::catalog_setting(self.host)?,
            native_device::community(self.host)?,
        ))
    }

    fn get(&mut self, url: &str, file: &str, token: u32, max: u64) -> Result<RequestId, Error> {
        self.handles.get(self.host, url, file, token, max)
    }

    fn cancel(&mut self, token: u32) {
        self.handles.cancel(self.host, token);
    }

    fn final_url(&mut self, id: RequestId) -> Option<String> {
        let mut input = mem::buffer(REQUEST_INPUT).ok()?;
        let mut out = mem::buffer(REPLY).ok()?;
        let host = self.host;
        let answer = self
            .handles
            .net(host)
            .ok()?
            .answer(host, id, &mut input, &mut out)
            .ok()?;
        let url = answer.final_url();
        if url.is_empty() { None } else { copy(url).ok() }
    }

    fn small_memory(&self) -> bool {
        small_memory(self.host)
    }

    fn revoke(&mut self, revoked: &[Revoked]) -> Result<(), Error> {
        revoke(self.host, revoked)
    }
}

/// The host as the download of a plugin's files uses it.
pub struct DownloadWire<'a, 'h> {
    host: &'a Host<'h>,
    handles: &'a mut Handles,
    storage: DataStorage<'a, 'h>,
}

impl core::fmt::Debug for DownloadWire<'_, '_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("DownloadWire").finish_non_exhaustive()
    }
}

impl<'a, 'h> DownloadWire<'a, 'h> {
    /// The download's view of `host`.
    pub fn new(host: &'a Host<'h>, handles: &'a mut Handles) -> Self {
        Self {
            host,
            handles,
            storage: DataStorage::new(host),
        }
    }
}

impl crate::native_download::Wire for DownloadWire<'_, '_> {
    fn storage(&self) -> &dyn Storage {
        &self.storage
    }

    fn get(
        &mut self,
        url: &str,
        file: &str,
        token: u32,
        max_bytes: u64,
    ) -> Result<RequestId, Error> {
        self.handles.get(self.host, url, file, token, max_bytes)
    }

    fn cancel(&mut self, token: u32) {
        self.handles.cancel(self.host, token);
    }
}

/// What the install sends the host.
pub struct InstallWire<'a, 'h> {
    host: &'a Host<'h>,
    storage: DataStorage<'a, 'h>,
}

impl core::fmt::Debug for InstallWire<'_, '_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("InstallWire").finish_non_exhaustive()
    }
}

impl<'a, 'h> InstallWire<'a, 'h> {
    /// The install's view of `host`.
    #[must_use]
    pub fn new(host: &'a Host<'h>) -> Self {
        Self {
            host,
            storage: DataStorage::new(host),
        }
    }
}

impl crate::native_install::Wire for InstallWire<'_, '_> {
    fn storage(&self) -> &dyn Storage {
        &self.storage
    }

    fn install(
        &mut self,
        request: u32,
        folder: &str,
        update: bool,
        allow_older: bool,
        proof: &Proof,
    ) -> Result<(), Error> {
        let mut steps: Vec<Statement<'_>> = Vec::new();
        steps
            .try_reserve_exact(proof.rotations.len())
            .map_err(|_| Error::NoMemory)?;
        for rotation in &proof.rotations {
            steps.push(Statement::new(&rotation.to, &rotation.signature));
        }
        let mut evidence = Evidence::new().rotations(&steps);
        if let Some(author) = &proof.author {
            evidence = evidence.author(author);
        }
        if let Some(recovery) = &proof.recovery {
            evidence = evidence.recovery(recovery);
        }
        let mut scratch = mem::buffer(INSTALL_SCRATCH)?;
        CommandSender::new(self.host, &mut scratch).install_plugin_with(
            request,
            InstallRoot::Data,
            folder,
            update,
            allow_older,
            &evidence,
        )
    }

    fn revoke(&mut self, revoked: &[Revoked]) -> Result<(), Error> {
        revoke(self.host, revoked)
    }
}

/// Whether the device has little memory (the X4): fewer pictures are kept.
fn small_memory(host: &Host<'_>) -> bool {
    use paperpaw_sdk::native::device::{Device, DeviceClass};
    let (Ok(device), Ok(mut out)) = (Device::open(host), mem::buffer(mem::SERVICE_BUFFER)) else {
        return false;
    };
    let mut input = [0; 256];
    let small = device
        .device(host, &mut input, &mut out)
        .is_ok_and(|identity| identity.class() == DeviceClass::Small);
    device.release(host).ok();
    small
}

/// Hands the revocations of the library to the host, in the batches the SDK
/// takes.
fn revoke(host: &Host<'_>, revoked: &[Revoked]) -> Result<(), Error> {
    let mut scratch = mem::buffer(REVOKE_SCRATCH)?;
    for batch in revoked.chunks(MAX_REVOCATIONS) {
        let mut list = Vec::new();
        list.try_reserve_exact(batch.len())
            .map_err(|_| Error::NoMemory)?;
        for item in batch {
            list.push(Revocation::new(&item.plugin, &item.key, &item.signature));
        }
        CommandSender::new(host, &mut scratch).trust_revoked(&list)?;
    }
    Ok(())
}
