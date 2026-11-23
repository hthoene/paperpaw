//! Going online for the transfer (the offer in the offline power mode) and the
//! reader's own network (*Without a router*): the device opens a network of its
//! own, shows the code to join it, and serves the page there. The password is
//! drawn anew each time from the device's random source. The network closes
//! with the screen, or after ten minutes without a request; the screen then
//! says so until Back leaves it (a timer cannot leave a screen).
use super::{
    NativeTransfer, PORT, SERVE_OUT, SERVER, View, draw_from, error_key, failed, handle, no_random,
    redraw,
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
    formatting::{Arg, FormatBuffers, format_message},
    net::{MAX_SERVED_BODY, Server},
    ui::ButtonHint,
    work::{Work, WorkSpec},
};
use paperpaw_ui_kit::{
    Item, Messages,
    mem::{SERVICE_BUFFER, buffer, wipe, wipe_text},
    text::copy,
};

/// The actions whose hints follow the view ([`NativeTransfer::hints`]).
const CONFIRM: &str = "core.confirm";
const LEFT: &str = "core.left";
const RIGHT: &str = "core.right";

/// Sets one button hint, best effort.
fn hint(host: &Host<'_>, hint: Result<ButtonHint<'_>, Error>) {
    if let Ok(hint) = hint {
        let _ = CommandSender::new(host, &mut [0; 256]).set_hint(hint);
    }
}

/// How long the network of the plugin's own is asked for when the reader
/// chooses to go online (the most the host takes).
const ONLINE_MS: u32 = 600_000;
/// The request number of hosting the own network (apart from the installs').
pub(super) const OWN_REQUEST: u32 = 0x7fff_0001;
/// Without a request this long, the reader's own network closes: it is not
/// left open by mistake.
const OWN_IDLE_MS: u64 = 10 * 60 * 1000;
/// The key of the idle timer.
const IDLE_KEY: &str = "transfer.own-idle";
/// Letters of the password of the own network.
const PASSWORD_LETTERS: usize = 15;
/// The room for the note of the own network: its arguments, and the text.
const NOTE_ARGUMENTS: usize = 256;
const NOTE_TEXT: usize = 512;

/// Where the reader's own network stands, as the plugin asked for it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum OwnNetwork {
    /// Not asked for.
    #[default]
    None,
    /// Asked for; the host has not said yet whether it opened.
    Asked,
    /// Open, the page served there.
    Open,
    /// Asked for and no longer wanted (the screen closed before it opened):
    /// left once the host says it opened.
    Unwanted,
}

/// The reader's own network, once chosen, as the screen shows it: its name
/// and password to type, and the code that joins it. Both hold the password.
#[derive(Debug)]
pub(super) struct Own {
    pub note: String,
    pub qr: Option<QrCode>,
}

impl Own {
    /// Wipes the password, in the note and in the code.
    pub fn wipe(&mut self) {
        wipe_text(core::mem::take(&mut self.note));
        if let Some(code) = &mut self.qr {
            code.wipe();
        }
        self.qr = None;
    }
}

/// The name and the password of the own network, to type where the code
/// cannot be scanned: made in buffers that are wiped, as the password is in
/// them.
fn own_note(host: &Host<'_>, ssid: &str, password: &str) -> Result<String, Error> {
    let grouped = paperpaw_sdk::random::grouped(password);
    let (mut scratch, mut out) = (buffer(NOTE_ARGUMENTS)?, buffer(NOTE_TEXT)?);
    let arguments = [
        Arg::Text {
            name: "ssid",
            value: ssid,
        },
        Arg::Text {
            name: "password",
            value: &grouped,
        },
    ];
    let note = format_message(
        host,
        "transfer.own.network",
        &arguments,
        &mut FormatBuffers::new(&mut scratch, &mut out),
    )
    .and_then(|length| {
        let text = out.get(..length).ok_or(Error::NoSpace)?;
        copy(core::str::from_utf8(text).map_err(|_| Error::Utf8)?)
    });
    wipe(&mut scratch);
    wipe(&mut out);
    wipe_text(grouped);
    note
}

/// The text of why the own network did not open.
pub(super) const fn failure_key(failure: WifiFailure) -> &'static str {
    match failure {
        WifiFailure::NeedsUser => "transfer.reason.not-allowed",
        WifiFailure::Unsupported => "transfer.reason.not-supported",
        WifiFailure::Timeout => "transfer.reason.timeout",
        _ => "transfer.reason.failed",
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
        if let View::Offline = view {
            items.push(Item::row(
                &messages.copy(host, "core.offline.go-online")?,
                "",
            )?);
            items.push(Item::row(
                &messages.copy(host, "core.offline.not-now")?,
                "",
            )?);
        }
        Ok(items)
    }

    /// The row of button hints for `view`, as in Beta 3: Select and the
    /// arrows keep their names on the offer's rows, Select says *Without a
    /// router* where it opens the own network, and elsewhere (the code, the
    /// phone's page, a failure) only Back does something, so only Back is
    /// named. A refused hint leaves the row as it was.
    pub(super) fn hints(host: &Host<'_>, view: &View) -> Result<(), Error> {
        match view {
            View::Offline => {
                for action in [CONFIRM, LEFT, RIGHT] {
                    hint(host, ButtonHint::usual(action));
                }
            }
            View::NoNetwork(true) => {
                let words = Messages::new()?.copy(host, "transfer.without-router")?;
                hint(host, ButtonHint::words(CONFIRM, &words));
                for action in [LEFT, RIGHT] {
                    hint(host, ButtonHint::hidden(action));
                }
            }
            _ => Self::only_back(host),
        }
        Ok(())
    }

    /// The row of button hints of a screen where only Back does something:
    /// only Back is named (as Beta 3's `Hint::only_back`).
    pub(super) fn only_back(host: &Host<'_>) {
        for action in [CONFIRM, LEFT, RIGHT] {
            hint(host, ButtonHint::hidden(action));
        }
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
        let ssid = match native_page::device_name(host) {
            Ok(name) => paperpaw_sdk::device_name::ssid(&name),
            Err(error) => {
                wipe_text(password);
                return Err(error);
            }
        };
        let mut own = Own {
            note: match own_note(host, &ssid, &password) {
                Ok(note) => note,
                Err(error) => {
                    wipe_text(password);
                    return Err(error);
                }
            },
            qr: None,
        };
        let code = paperpaw_sdk::wifi::join_code(&ssid, &password);
        own.qr = QrCode::new(code.as_bytes());
        wipe_text(code);
        let mut scratch = [0; 256];
        let mut sender = CommandSender::new(host, &mut scratch);
        // The radio on, then the network: honoured only now, with the input
        // that chose it (`docs/abi/device.md` 3).
        let sent = sender
            .wifi_on()
            .and_then(|()| sender.wifi_host(OWN_REQUEST, &ssid, &password));
        // The command buffer held the password, as did the text.
        wipe(&mut scratch);
        wipe_text(password);
        if let Err(error) = sent {
            own.wipe();
            return match error {
                Error::NoMemory => Err(error),
                _ => {
                    let words = failed(host, "transfer.failed", error_key(error))?;
                    self.show(host, SCREEN, View::Failed(words))
                }
            };
        }
        if let Some(page) = self.screens.top(SCREEN) {
            page.guard = None;
            page.own = Some(own);
        } else {
            own.wipe();
        }
        self.own_network = OwnNetwork::Asked;
        // The page served on the router's network stops; it starts again on this one.
        self.stop(host, SERVER)?;
        self.show(host, SCREEN, View::Starting)
    }

    /// The host ended hosting the own network: the page is served there, or
    /// the transfer says why not. One no longer wanted is left at once.
    pub(super) fn wifi_done(&mut self, host: &Host<'_>, done: &WifiDone<'_>) -> Result<(), Error> {
        if done.request().map(|request| request.get()) != Some(OWN_REQUEST) {
            return Ok(());
        }
        match (self.own_network, done.failure()) {
            (OwnNetwork::Unwanted, failure) => {
                self.own_network = OwnNetwork::None;
                if failure.is_none() {
                    let mut scratch = [0; 64];
                    CommandSender::new(host, &mut scratch).wifi_leave()?;
                }
                Ok(())
            }
            (OwnNetwork::Asked, None) => {
                self.own_network = OwnNetwork::Open;
                self.touched(host, done.now_ms())?;
                let server = Server::new(SERVER, PORT)?.max_body(MAX_SERVED_BODY)?;
                let (mut input, mut out) = (buffer(64)?, buffer(SERVE_OUT)?);
                let net = handle(&mut self.net, || paperpaw_sdk::native::net::Net::open(host))?;
                net.serve(host, &server, &mut input, &mut out)
            }
            (OwnNetwork::Asked, Some(failure)) => {
                self.own_network = OwnNetwork::None;
                self.wipe_own();
                let words = failed(host, "transfer.failed", failure_key(failure))?;
                self.show(host, SCREEN, View::Failed(words))
            }
            _ => Ok(()),
        }
    }

    /// A request came (or the own network opened): the idle time counts from
    /// `now_ms`.
    pub(super) fn touched(&mut self, host: &Host<'_>, now_ms: u64) -> Result<(), Error> {
        if self.own_network != OwnNetwork::Open {
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

    /// The idle timer came due: the page stops and the own network closes,
    /// and the screen says so until Back leaves it (a timer cannot leave a
    /// screen). Its key bar names Back alone already, as on the code.
    pub(super) fn idle_due(&mut self, host: &Host<'_>, key: &str) -> Result<(), Error> {
        if key != IDLE_KEY || self.own_network != OwnNetwork::Open {
            return Ok(());
        }
        // What can fail first: a failed callback loses the leaving with it.
        let words = Messages::new()?.copy(host, "transfer.own.closed")?;
        // The page before the network, so that its end says nothing of its
        // own; best effort, as the host ends it with the network anyway.
        let _ = self.stop(host, SERVER);
        if let Some(page) = self.screens.top(SCREEN) {
            page.guard = None;
            page.view = View::Failed(words);
        }
        self.leave_own(host)
    }

    /// Wipes what shows the own network (its password is in it).
    fn wipe_own(&mut self) {
        if let Some(page) = self.screens.top(SCREEN)
            && let Some(mut own) = page.own.take()
        {
            own.wipe();
        }
    }

    /// The own network is no longer wanted (the screen closed, or it was left
    /// alone): left where it is open, and also where it was asked for and is
    /// not open yet, so that it does not open for nobody (one that opens all
    /// the same is left then, [`Self::wifi_done`]). Its timer is cancelled and
    /// what shows it wiped.
    pub(super) fn leave_own(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let asked = match self.own_network {
            OwnNetwork::None | OwnNetwork::Unwanted => return Ok(()),
            OwnNetwork::Asked => true,
            OwnNetwork::Open => false,
        };
        // Left only where it is the own network: never the router's.
        let hosting = matches!(self.wifi(host)?, Some((WifiState::Hosting, ..)));
        if let Some(work) = self.work(host)? {
            let (mut input, mut out) = (buffer(256)?, buffer(64)?);
            if let Err(Error::NoMemory) = work.cancel(host, IDLE_KEY, &mut input, &mut out) {
                return Err(Error::NoMemory);
            }
        }
        self.own_network = if asked {
            OwnNetwork::Unwanted
        } else {
            OwnNetwork::None
        };
        self.wipe_own();
        if asked || hosting {
            let mut scratch = [0; 64];
            CommandSender::new(host, &mut scratch).wifi_leave()?;
        }
        redraw(host, RedrawMode::Partial)
    }
}
