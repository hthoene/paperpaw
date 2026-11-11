//! The interface-7 plugin store, so far without a screen: the settings, the
//! action that opens the store, and the fetch of the list and of a plugin's page
//! over `core.net` into the index kept on the card (`native_load`). The screens
//! (`store.list` and the rest), the downloads, the install, the queue, the
//! requests of other plugins and the first-run offer follow (`PORTING-7.md`).
//!
//! **Memory:** nothing is read or built at start (the settings are submitted,
//! the handles are opened when first needed); the list kept on the card is read
//! when a screen asks (`NativeStore::loader`), and the device and what is
//! installed only for the length of a fetch (`native_device::Facts`).
use alloc::string::String;

use paperpaw_sdk::Storage;
use paperpaw_sdk::native::{
    Error, Host, Plugin,
    commands::{CommandSender, MAX_REVOCATIONS, Revocation},
    events::{Event, Incoming, kinds},
    net::{FileRoot, Header, HttpDone, Net, REPLY, REQUEST_INPUT, Request, RequestId},
    registrations,
    reply::Reply,
    storage::DataStorage,
    ui::Open,
};
use paperpaw_ui_kit::{mem, read_event};

use crate::declarations::{HTTP_TOKEN, OPEN_TOKEN};
use crate::native_device::{self, Facts};
use crate::native_load::{Done, Loader, Wire};
use crate::native_model::Revoked;

mod image {
    include!(concat!(env!("OUT_DIR"), "/store_declarations.rs"));
}

/// The `User-Agent` of interface 6's requests.
const USER_AGENT: &str = "PaperPaw/0.1 (e-reader)";
/// The screen the action opens (declared with the screens, slice 9).
const LIST_SCREEN: &str = "list";

/// The handles of the services, opened when first needed.
#[derive(Debug, Default)]
pub struct Handles {
    net: Option<Net>,
}

/// The interface-7 plugin store.
#[derive(Debug, Default)]
pub struct NativeStore {
    /// The list and the page held; read from the card when a screen asks.
    pub loader: Loader,
    handles: Handles,
}

/// The host as the fetch uses it.
struct HostWire<'a, 'h> {
    host: &'a Host<'h>,
    handles: &'a mut Handles,
    storage: DataStorage<'a, 'h>,
}

impl<'a, 'h> HostWire<'a, 'h> {
    fn new(host: &'a Host<'h>, handles: &'a mut Handles) -> Self {
        Self {
            host,
            handles,
            storage: DataStorage::new(host),
        }
    }

    fn net(&mut self) -> Result<&Net, Error> {
        if self.handles.net.is_none() {
            self.handles.net = Some(Net::open(self.host)?);
        }
        self.handles.net.as_ref().ok_or(Error::State)
    }
}

impl Wire for HostWire<'_, '_> {
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
        let mut input = mem::buffer(REQUEST_INPUT)?;
        let mut out = mem::buffer(REPLY)?;
        let headers = [Header::new("User-Agent", USER_AGENT)?];
        let request = Request::get(token, url)?
            .headers(&headers)?
            .save_to(FileRoot::Data, file)?
            .max_file(max)?;
        let host = self.host;
        self.net()?.request(host, &request, &mut input, &mut out)
    }

    fn cancel(&mut self, token: u32) {
        let host = self.host;
        // Only the request of this token is ended: the net service cancels by id
        // (the token is the plugin's own number, kept by the host).
        if let (Ok(mut input), Ok(mut out)) = (mem::buffer(REQUEST_INPUT), mem::buffer(REPLY))
            && let Ok(net) = self.net()
        {
            net.cancel(host, token, &mut input, &mut out).ok();
        }
    }

    fn final_url(&mut self, id: RequestId) -> Option<String> {
        let mut input = mem::buffer(REQUEST_INPUT).ok()?;
        let mut out = mem::buffer(REPLY).ok()?;
        let host = self.host;
        let answer = self
            .net()
            .ok()?
            .answer(host, id, &mut input, &mut out)
            .ok()?;
        let url = answer.final_url();
        if url.is_empty() {
            None
        } else {
            paperpaw_ui_kit::text::copy(url).ok()
        }
    }

    fn small_memory(&self) -> bool {
        use paperpaw_sdk::native::device::{Device, DeviceClass};
        let (Ok(device), Ok(mut out)) = (Device::open(self.host), mem::buffer(1024)) else {
            return false;
        };
        let mut input = [0; 256];
        let small = device
            .device(self.host, &mut input, &mut out)
            .is_ok_and(|identity| identity.class() == DeviceClass::Small);
        device.release(self.host).ok();
        small
    }

    fn revoke(&mut self, revoked: &[Revoked]) -> Result<(), Error> {
        let mut scratch = mem::buffer(4096)?;
        for batch in revoked.chunks(MAX_REVOCATIONS) {
            let mut list = alloc::vec::Vec::new();
            list.try_reserve_exact(batch.len())
                .map_err(|_| Error::NoMemory)?;
            for item in batch {
                list.push(Revocation::new(&item.plugin, &item.key, &item.signature));
            }
            CommandSender::new(self.host, &mut scratch).trust_revoked(&list)?;
        }
        Ok(())
    }
}

impl Plugin for NativeStore {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::SETTINGS
            .iter()
            .chain(image::SCREENS)
            .chain(image::SETUP)
            .chain(image::NAVIGATION)
            .chain(image::EVENTS)
        {
            registrations::submit(host, batch)?;
        }
        Ok(Self::default())
    }

    fn event(
        &mut self,
        host: &Host<'_>,
        token: u32,
        incoming: Incoming<'_>,
        _reply: &mut Reply<'_>,
    ) -> Result<(), Error> {
        let Some(event) = read_event!(incoming; kinds::ActionRun, kinds::HttpDone) else {
            return Ok(());
        };
        match event {
            Event::ActionRun { .. } if token == OPEN_TOKEN => {
                let mut scratch = [0; 128];
                CommandSender::new(host, &mut scratch).open(Open::screen(LIST_SCREEN)?)
            }
            Event::HttpDone(done) if token == HTTP_TOKEN => self.ended(host, &done),
            _ => Ok(()),
        }
    }
}

impl NativeStore {
    /// A request ended: the list or the page it asked for is taken.
    fn ended(&mut self, host: &Host<'_>, done: &HttpDone) -> Result<(), Error> {
        let done = Done::new(done.id(), done.token(), done.reason(), done.status());
        let mut wire = HostWire::new(host, &mut self.handles);
        self.loader.ended(&mut wire, &done).map(|_| ())
    }
}
