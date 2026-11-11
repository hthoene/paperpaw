//! Going online for the transfer (the offer in the offline power mode) and the
//! reader's own network (*Without a router*): the device opens a network of its
//! own, shows the code to join it, and serves the page there. The password is
//! drawn anew each time from the device's random source. The network closes
//! with the screen, or after ten minutes without a request.
use super::{
    NativeTransfer, PORT, SERVE_OUT, SERVER, View, draw_from, failed, handle, no_random, redraw,
};
use crate::{
    declarations::{SCREEN, WORK},
    native_key, native_page,
};
use alloc::{string::String, vec::Vec};
use paperpaw_qr::QrCode;
use paperpaw_sdk::native::{
    Error, Host,
    commands::{CommandSender, RedrawMode},
    device::{PowerLevel, WifiState},
    events::{WifiDone, WifiFailure},
    net::{MAX_SERVED_BODY, Server},
    work::{Work, WorkSpec},
};
use paperpaw_ui_kit::{
    Item, Messages,
    mem::{SERVICE_BUFFER, buffer, wipe_text},
};

/// How long the network of the plugin's own is asked for when the reader
/// chooses to go online (the most the host takes).
const ONLINE_MS: u32 = 600_000;
/// The request number of hosting the own network (apart from the installs').
pub(super) const OWN_REQUEST: u32 = 0x7fff_0001;
/// Without a request this long, the reader's own network closes (and the
/// screen with it): it is not left open by mistake.
const OWN_IDLE_MS: u64 = 10 * 60 * 1000;
/// The key of the idle timer.
const IDLE_KEY: &str = "transfer.own-idle";
/// Letters of the password of the own network.
const PASSWORD_LETTERS: usize = 15;

/// The reader's own network, once chosen: its name and password and the code
/// that joins it.
#[derive(Debug)]
pub(super) struct Own {
    pub ssid: String,
    pub password: String,
    pub qr: Option<QrCode>,
}

impl Own {
    /// Wipes the password.
    pub fn wipe(&mut self) {
        wipe_text(core::mem::take(&mut self.password));
    }
}

impl NativeTransfer {
    /// Whether the device is in the offline power mode, where the network is
    /// the reader's to allow.
    pub(super) fn is_offline(&mut self, host: &Host<'_>) -> Result<bool, Error> {
        let device = handle(&mut self.device, || {
            paperpaw_sdk::native::device::Device::open(host)
        })?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        match device.power(host, 0, &mut input, &mut out) {
            Ok(info) => Ok(info.level() == PowerLevel::Offline),
            Err(Error::NoMemory) => Err(Error::NoMemory),
            Err(_) => Ok(false),
        }
    }

    /// The rows under the header for `view`: the offer to go online, or the
    /// button for the reader's own network where PaperPaw runs the Wi-Fi.
    pub(super) fn rows(host: &Host<'_>, view: &View) -> Result<Vec<Item>, Error> {
        let mut messages = Messages::new()?;
        let mut items = Vec::new();
        items.try_reserve_exact(2).map_err(|_| Error::NoMemory)?;
        match view {
            View::Offline => {
                items.push(Item::row(
                    &messages.copy(host, "core.offline.go-online")?,
                    "",
                )?);
                items.push(Item::row(
                    &messages.copy(host, "core.offline.not-now")?,
                    "",
                )?);
            }
            View::NoNetwork(true) => {
                items.push(Item::row(
                    &messages.copy(host, "transfer.without-router")?,
                    "",
                )?);
            }
            _ => {}
        }
        Ok(items)
    }

    /// The reader chose a row on the offer (`index` 0 *Go online*, 1 *Not now*)
    /// or the button for the own network (`index` 0 on *No Wi-Fi*).
    pub(super) fn chosen(&mut self, host: &Host<'_>, index: usize) -> Result<(), Error> {
        let view = self.screens.top(SCREEN).map(|page| match page.view {
            View::Offline => 1,
            View::NoNetwork(true) => 2,
            _ => 0,
        });
        match (view, index) {
            (Some(1), 0) => {
                // Refused (a device that cannot): the transfer is tried all the
                // same and says that the device is offline.
                let mut scratch = [0; 64];
                let asked = CommandSender::new(host, &mut scratch).go_online(ONLINE_MS);
                if asked == Err(Error::NoMemory) {
                    return Err(Error::NoMemory);
                }
                self.show(host, SCREEN, View::Starting)?;
                self.start(host, SCREEN)
            }
            (Some(1), _) => {
                let managed = self.wifi(host)?.is_some();
                self.show(host, SCREEN, View::NoNetwork(managed))
            }
            (Some(2), 0) => self.without_router(host),
            _ => Ok(()),
        }
    }

    /// Opens the reader's own network and serves the page there: for a
    /// transfer where there is no router.
    fn without_router(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let password =
            native_key::letters(&mut draw_from(&mut self.secrets, host), PASSWORD_LETTERS);
        let password = match password {
            Ok(Some(password)) => password,
            Ok(None) | Err(Error::Unsupported | Error::NotFound | Error::Denied) => {
                return self.show(host, SCREEN, View::Failed(no_random(host)?));
            }
            Err(error) => return Err(error),
        };
        // Named as the device is, for a phone to find it.
        let ssid = paperpaw_sdk::device_name::ssid(&native_page::device_name(host)?);
        let qr = QrCode::new(paperpaw_sdk::wifi::join_code(&ssid, &password).as_bytes());
        let mut scratch = [0; 256];
        let mut sender = CommandSender::new(host, &mut scratch);
        // The radio on, then the network: honoured only now, with the input
        // that chose it (`docs/abi/device.md` 3).
        let sent = sender
            .wifi_on()
            .and_then(|()| sender.wifi_host(OWN_REQUEST, &ssid, &password));
        if let Err(error) = sent {
            wipe_text(password);
            return match error {
                Error::NoMemory => Err(error),
                _ => {
                    let words = failed(host, "transfer.failed", super::word(error))?;
                    self.show(host, SCREEN, View::Failed(words))
                }
            };
        }
        if let Some(page) = self.screens.top(SCREEN) {
            page.guard = None;
            page.own = Some(Own { ssid, password, qr });
        }
        self.own_open = true;
        // The page served on the router's network stops; it starts again on this one.
        self.stop(host, SERVER)?;
        self.show(host, SCREEN, View::Starting)
    }

    /// The host ended hosting the own network: the page is served there, or
    /// the transfer says why not.
    pub(super) fn wifi_done(&mut self, host: &Host<'_>, done: &WifiDone<'_>) -> Result<(), Error> {
        if done.request().map(|request| request.get()) != Some(OWN_REQUEST) {
            return Ok(());
        }
        if done.status().is_ok() {
            self.touched(host, done.now_ms())?;
            let server = Server::new(SERVER, PORT)?.max_body(MAX_SERVED_BODY)?;
            let (mut input, mut out) = (buffer(64)?, buffer(SERVE_OUT)?);
            let net = handle(&mut self.net, || paperpaw_sdk::native::net::Net::open(host))?;
            return net.serve(host, &server, &mut input, &mut out);
        }
        self.drop_own();
        let why = match done.failure() {
            Some(WifiFailure::NeedsUser) => "not allowed",
            Some(WifiFailure::Unsupported) => "not supported",
            Some(WifiFailure::Timeout) => "timeout",
            _ => "failed",
        };
        let words = failed(host, "transfer.failed", why)?;
        self.show(host, SCREEN, View::Failed(words))
    }

    /// A request came (or the own network opened): the idle time counts from
    /// `now_ms`.
    pub(super) fn touched(&mut self, host: &Host<'_>, now_ms: u64) -> Result<(), Error> {
        if !self.own_open {
            return Ok(());
        }
        let Some(work) = self.work(host)? else {
            return Ok(());
        };
        let spec = WorkSpec::at(IDLE_KEY, WORK, now_ms.saturating_add(OWN_IDLE_MS).max(1))?;
        let (mut input, mut out) = (buffer(256)?, buffer(64)?);
        match work.declare(host, &spec, &mut input, &mut out) {
            Err(Error::NoMemory) => Err(Error::NoMemory),
            // Without the timer the network closes with the screen.
            _ => Ok(()),
        }
    }

    pub(super) fn work(&mut self, host: &Host<'_>) -> Result<Option<&Work>, Error> {
        if self.work.is_none() {
            match Work::open(host) {
                Ok(work) => self.work = Some(work),
                Err(Error::NoMemory) => return Err(Error::NoMemory),
                Err(_) => return Ok(None),
            }
        }
        Ok(self.work.as_ref())
    }

    /// The idle timer came due: the screen closes, and the network with it.
    pub(super) fn idle_due(&mut self, host: &Host<'_>, key: &str) -> Result<(), Error> {
        if key != IDLE_KEY || !self.own_open {
            return Ok(());
        }
        let mut scratch = [0; 64];
        CommandSender::new(host, &mut scratch).close_screen()
    }

    /// The own network is not the plugin's any more (it failed, or it was left).
    fn drop_own(&mut self) {
        self.own_open = false;
        if let Some(page) = self.screens.top(SCREEN)
            && let Some(mut own) = page.own.take()
        {
            own.wipe();
        }
    }

    /// The screen closed: the own network is left where it is open, and its
    /// timer is cancelled.
    pub(super) fn leave_own(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if !self.own_open {
            return Ok(());
        }
        self.own_open = false;
        // Left only where it is the own network that is up: never the router's.
        if matches!(self.wifi(host)?, Some((WifiState::Hosting, ..))) {
            let mut scratch = [0; 64];
            CommandSender::new(host, &mut scratch).wifi_leave()?;
        }
        if let Some(work) = self.work(host)? {
            let (mut input, mut out) = (buffer(256)?, buffer(64)?);
            if let Err(Error::NoMemory) = work.cancel(host, IDLE_KEY, &mut input, &mut out) {
                return Err(Error::NoMemory);
            }
        }
        redraw(host, RedrawMode::Partial)
    }
}
