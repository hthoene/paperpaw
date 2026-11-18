//! The sync's world on the device: the plugin's data folder and the network
//! service (opened by the first request). The `Authorization` header, made of
//! the user and the password kept among the secrets, goes with every request
//! to the server and is wiped from the buffers it passed through.
use alloc::string::String;

use paperpaw_sdk::native::net::FileRoot;
use paperpaw_sdk::native::{
    Error, Host,
    net::{Header, Net, REPLY, REQUEST_INPUT, Request},
    storage::{self, Root},
};
use paperpaw_ui_kit::{
    files::Volume,
    mem::{buffer, wipe},
};

use crate::native_dav::{LISTING, PROPFIND};
use crate::native_keep::Secret;
use crate::native_sync::{GET_TOKEN, LIST_TOKEN, Link};

/// How long a listing may take, and a photo without a byte moving.
const LIST_TIMEOUT_MS: u32 = 30_000;

/// The storage calls on the plugin's data folder.
pub struct Data<'a, 'h>(pub &'a Host<'h>);

impl Volume for Data<'_, '_> {
    fn size(&self, name: &str) -> Result<u64, Error> {
        storage::size(self.0, Root::Data, name)
    }
    fn read_at(&self, name: &str, at: u64, out: &mut [u8]) -> Result<usize, Error> {
        storage::read_at(self.0, Root::Data, name, at, out)
    }
    fn write(&self, name: &str, data: &[u8]) -> Result<(), Error> {
        storage::write(self.0, Root::Data, name, data)
    }
    fn append(&self, name: &str, data: &[u8]) -> Result<(), Error> {
        storage::append(self.0, Root::Data, name, data)
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), Error> {
        storage::rename(self.0, Root::Data, from, to)
    }
    fn remove(&self, name: &str) -> Result<(), Error> {
        storage::remove(self.0, Root::Data, name)
    }
    fn sync(&self) -> Result<(), Error> {
        storage::sync(self.0)
    }
}

/// The data folder and the network, with the account's header.
pub struct HostLink<'a, 'h> {
    pub host: &'a Host<'h>,
    pub net: &'a mut Option<Net>,
    pub data: Data<'a, 'h>,
    /// `Basic ...` where the server wants an account.
    pub auth: Option<&'a Secret>,
}

impl HostLink<'_, '_> {
    fn net(&mut self) -> Result<&Net, Error> {
        if self.net.is_none() {
            *self.net = Some(Net::open(self.host)?);
        }
        self.net.as_ref().ok_or(Error::State)
    }

    fn send(&mut self, request: &Request<'_>) -> Result<(), Error> {
        let (mut input, mut out) = (buffer(REQUEST_INPUT)?, buffer(REPLY)?);
        let host = self.host;
        let sent = self
            .net()?
            .request(host, request, &mut input, &mut out)
            .map(|_| ());
        // The input held the account's header.
        wipe(&mut input);
        wipe(&mut out);
        sent
    }
}

impl Link for HostLink<'_, '_> {
    fn volume(&self) -> &dyn Volume {
        &self.data
    }

    fn propfind(&mut self, url: &str) -> Result<(), Error> {
        let depth = Header::new("Depth", "1")?;
        let kind = Header::new("Content-Type", "application/xml")?;
        let account = self
            .auth
            .map(|auth| Header::new("Authorization", auth.as_str()));
        let mut headers = [depth, kind, depth];
        let mut count = 2;
        if let Some(account) = account {
            headers[2] = account?;
            count = 3;
        }
        let request = Request::new(LIST_TOKEN, "PROPFIND", url)?
            .headers(headers.get(..count).ok_or(Error::Invalid)?)?
            .body(PROPFIND.as_bytes())?
            .save_to(FileRoot::Data, LISTING)?
            .max_file(1 << 20)?
            .timeout_ms(LIST_TIMEOUT_MS)?;
        self.send(&request)
    }

    fn get(&mut self, url: &str, save: &str, max: u64) -> Result<(), Error> {
        let headers = match self.auth {
            Some(auth) => [Header::new("Authorization", auth.as_str())?],
            None => [Header::new("Accept", "image/jpeg, image/png")?],
        };
        let request = Request::new(GET_TOKEN, "GET", url)?
            .headers(&headers)?
            .save_to(FileRoot::Data, save)?
            .max_file(max)?;
        self.send(&request)
    }

    fn cancel(&mut self) {
        let host = self.host;
        if let (Ok(mut input), Ok(mut out)) = (buffer(REQUEST_INPUT), buffer(REPLY))
            && let Ok(net) = self.net()
        {
            for token in [LIST_TOKEN, GET_TOKEN] {
                net.cancel(host, token, &mut input, &mut out).ok();
            }
        }
    }
}

/// The text of an address with a `/` at its end (a folder's), `None` for no
/// address or one that does not start with `http://` or `https://`.
#[must_use]
pub fn folder_url(setting: &str) -> Option<String> {
    let url = setting.trim();
    let lower_ok = ["http://", "https://"].iter().any(|scheme| {
        url.len() > scheme.len()
            && url
                .get(..scheme.len())
                .is_some_and(|s| s.eq_ignore_ascii_case(scheme))
    });
    if !lower_ok || url.len() > 800 {
        return None;
    }
    let mut out = String::new();
    out.try_reserve_exact(url.len() + 1).ok()?;
    out.push_str(url);
    if !out.ends_with('/') {
        out.push('/');
    }
    Some(out)
}
