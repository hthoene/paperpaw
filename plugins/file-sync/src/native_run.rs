//! The sync's world on the device ([`HostLink`]: the card through the files
//! service, the data folder, the network with the account's header) and the
//! run of all the pairs one after the other. The `Authorization` header, made
//! of the user and the password kept among the secrets, goes with every
//! request to the server and is wiped from the buffers it passed through.
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::native::{
    Error, Host,
    files::{Files, INPUT_BYTES, OUTPUT_BYTES, PAGE_MAX_ROWS},
    net::{FileRoot, Header, Net, REPLY, REQUEST_INPUT, Request},
    storage::{self, Root},
};
use paperpaw_ui_kit::{
    dav::{
        listing::{LISTING, PROPFIND},
        secret::Secret,
    },
    files::Volume,
    mem::{buffer, wipe},
};

use crate::native_link::{Entry, Link, TOKEN};
use crate::native_plan::Side;
use crate::native_sync::TRASH_DIR;

/// How long a listing may take.
const LIST_TIMEOUT_MS: u32 = 60_000;
/// The longest a request may be silent: a large file moves for much longer.
const FILE_TIMEOUT_MS: u32 = 120_000;

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

/// The card, the data folder and the network, with the account's header.
pub struct HostLink<'a, 'h> {
    pub host: &'a Host<'h>,
    pub net: &'a mut Option<Net>,
    pub files: &'a mut Option<Files>,
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

    fn files(&mut self) -> Result<&Files, Error> {
        if self.files.is_none() {
            *self.files = Some(Files::open(self.host)?);
        }
        self.files.as_ref().ok_or(Error::State)
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

    /// Sends `method` to `url` with the account and `extra` headers.
    fn call(
        &mut self,
        method: &str,
        url: &str,
        extra: Option<Header<'_>>,
        body: &[u8],
        file: Option<(FileRoot, &str, bool)>,
        (timeout, max): (u32, u64),
    ) -> Result<(), Error> {
        let blank = Header::new("Accept", "*/*")?;
        let mut headers = [blank; 2];
        let mut count = 0;
        if let Some(auth) = self.auth {
            headers[count] = Header::new("Authorization", auth.as_str())?;
            count += 1;
        }
        if let Some(extra) = extra {
            headers[count] = extra;
            count += 1;
        }
        let mut request = Request::new(TOKEN, method, url)?
            .headers(headers.get(..count).ok_or(Error::Invalid)?)?
            .timeout_ms(timeout)?;
        if !body.is_empty() {
            request = request.body(body)?;
        }
        match file {
            Some((root, path, true)) => request = request.save_to(root, path)?.max_file(max)?,
            Some((root, path, false)) => request = request.send_file(root, path)?,
            None => {}
        }
        self.send(&request)
    }
}

impl Link for HostLink<'_, '_> {
    fn data(&self) -> &dyn Volume {
        &self.data
    }

    fn list(&mut self, folder: &str, out: &mut Vec<Entry>, most: usize) -> Result<(), Error> {
        let host = self.host;
        let (mut input, mut reply) = (buffer(INPUT_BYTES)?, buffer(OUTPUT_BYTES)?);
        let files = self.files()?;
        let mut cursor = 0;
        // A folder is read page by page; no more pages than a folder holds.
        for _ in 0..(most / PAGE_MAX_ROWS.max(1) + 3) {
            let page = files.list(host, Root::Files, folder, cursor, &mut input, &mut reply)?;
            for entry in page.entries() {
                if entry.unaddressable() || out.len() >= most {
                    continue;
                }
                paperpaw_ui_kit::mem::push(
                    out,
                    Entry {
                        name: paperpaw_ui_kit::text::copy(entry.name())?,
                        folder: entry.is_folder(),
                        side: Side {
                            size: entry.size(),
                            stamp: entry.modified().unwrap_or(0),
                        },
                    },
                )?;
            }
            match page.next() {
                Some(next) if out.len() < most => cursor = next,
                _ => break,
            }
        }
        Ok(())
    }

    fn stat(&mut self, path: &str) -> Result<Option<Side>, Error> {
        let host = self.host;
        let (mut input, mut reply) = (buffer(INPUT_BYTES)?, buffer(OUTPUT_BYTES)?);
        match self
            .files()?
            .stat(host, Root::Files, path, &mut input, &mut reply)
        {
            Ok(entry) => Ok(Some(Side {
                size: entry.size(),
                stamp: entry.modified().unwrap_or(0),
            })),
            Err(Error::NotFound) => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn make_dir(&mut self, path: &str) -> Result<(), Error> {
        let host = self.host;
        let (mut input, mut reply) = (buffer(INPUT_BYTES)?, buffer(OUTPUT_BYTES)?);
        self.files()?
            .make_dir(host, Root::Files, path, &mut input, &mut reply)
    }

    fn rename(&mut self, from: &str, to: &str) -> Result<(), Error> {
        let host = self.host;
        let (mut input, mut reply) = (buffer(INPUT_BYTES)?, buffer(OUTPUT_BYTES)?);
        self.files()?.move_entry(
            host,
            Root::Files,
            from,
            Root::Files,
            to,
            false,
            &mut input,
            &mut reply,
        )
    }

    fn trash(&mut self, from: &str, to: &str) -> Result<(), Error> {
        let host = self.host;
        let (mut input, mut reply) = (buffer(INPUT_BYTES)?, buffer(OUTPUT_BYTES)?);
        let files = self.files()?;
        files.make_dir(host, Root::Data, TRASH_DIR, &mut input, &mut reply)?;
        files.move_entry(
            host,
            Root::Files,
            from,
            Root::Data,
            to,
            false,
            &mut input,
            &mut reply,
        )
    }

    fn remove(&mut self, path: &str) -> Result<(), Error> {
        let host = self.host;
        let (mut input, mut reply) = (buffer(INPUT_BYTES)?, buffer(OUTPUT_BYTES)?);
        self.files()?
            .remove_tree(host, Root::Files, path, &mut input, &mut reply)
            .map(|_| ())
    }

    fn propfind(&mut self, url: &str) -> Result<(), Error> {
        let depth = Header::new("Depth", "1")?;
        self.call(
            "PROPFIND",
            url,
            Some(depth),
            PROPFIND.as_bytes(),
            Some((FileRoot::Data, LISTING, true)),
            (LIST_TIMEOUT_MS, 1 << 20),
        )
    }

    fn get(&mut self, url: &str, path: &str, max: u64) -> Result<(), Error> {
        self.call(
            "GET",
            url,
            None,
            &[],
            Some((FileRoot::Files, path, true)),
            (FILE_TIMEOUT_MS, max),
        )
    }

    fn put(&mut self, url: &str, path: &str) -> Result<(), Error> {
        let kind = Header::new("Content-Type", "application/octet-stream")?;
        self.call(
            "PUT",
            url,
            Some(kind),
            &[],
            Some((FileRoot::Files, path, false)),
            (FILE_TIMEOUT_MS, 0),
        )
    }

    fn delete(&mut self, url: &str) -> Result<(), Error> {
        self.call("DELETE", url, None, &[], None, (LIST_TIMEOUT_MS, 0))
    }

    fn mkcol(&mut self, url: &str) -> Result<(), Error> {
        self.call("MKCOL", url, None, &[], None, (LIST_TIMEOUT_MS, 0))
    }

    fn cancel(&mut self) {
        let host = self.host;
        if let (Ok(mut input), Ok(mut out)) = (buffer(REQUEST_INPUT), buffer(REPLY))
            && let Ok(net) = self.net()
        {
            net.cancel(host, TOKEN, &mut input, &mut out).ok();
        }
    }
}

/// `YYYY-MM-DD HH:MM` of a time in Unix seconds shifted to the local time.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn stamp(local: i64) -> Result<String, Error> {
    let date = crate::native_names::date(local.div_euclid(86_400))?;
    let secs = local.rem_euclid(86_400);
    paperpaw_ui_kit::text::format(format_args!(
        "{date} {:02}:{:02}",
        secs / 3600,
        secs % 3600 / 60
    ))
}
