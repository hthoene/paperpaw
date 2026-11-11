//! The kernel's side of what the shell does for it: HTTP, servers and
//! sockets, speech, the clipboard, Wi-Fi and firmware updates.

#[allow(
    unused_imports,
    reason = "a part of the kernel, which sees all of it; not every part uses every name"
)]
use super::*;

/// What the shell is to do for the kernel, and what it does: HTTP,
/// servers, sockets, speech, the clipboard, Wi-Fi, firmware updates.
#[derive(Debug)]
pub(crate) struct IoState {
    /// Plugins' requests ([`Io::Http`]).
    pub(crate) http: io::http::Http,
    /// Plugins' servers ([`Io::Server`]).
    pub(crate) servers: io::servers::Servers,
    /// Plugins' sockets and broadcasts ([`Io::Socket`]).
    pub(crate) sockets: io::sockets::Sockets,
    /// Text spoken ([`Io::Speech`]).
    pub(crate) speech: io::speech::Speech,
    /// The system's clipboard ([`Io::Copy`], [`Io::ReadClipboard`]).
    pub(crate) clipboard: io::clipboard::Clipboard,
    /// What the shell is to do with the radio ([`Io::Wifi`]).
    pub(crate) wifi_calls: Vec<WifiCommand>,
    /// Bluetooth, where the kernel uses a radio ([`Io::Bluetooth`]).
    pub(crate) bluetooth: Option<io::bluetooth::Radio>,
    /// What the shell is to do with the firmware ([`Io::Firmware`]).
    pub(crate) firmware_calls: Vec<FirmwareCommand>,
    /// Since when plugins' network work has waited for a network.
    pub(crate) network_waiting: Option<Instant>,
    /// Something used the network at the end of the last call.
    pub(crate) network_busy: bool,
    /// What is spoken is the kernel's own word of the focus
    /// (`Kernel::speak_focus`), which the next replaces.
    pub(crate) focus_spoken: bool,
    /// The focus last told to plugins (`events::FOCUS_CHANGED`).
    pub(crate) focus_told: Option<paperpaw_sdk::semantics::Part>,
}

impl Kernel {
    /// Wi-Fi as plugins see it (state, the networks last found, whether a
    /// scan runs, whether the network is wanted or busy), for a shell that
    /// tells its own host services of it (`core.device`, ops 8 and 9). `None`
    /// where the kernel is built without Wi-Fi.
    pub fn wifi_model(&self) -> Option<&paperpaw_sdk::wifi::Wifi> {
        self.shared.wifi.as_ref()
    }

    /// What happened with Wi-Fi: plugins hear of it, and network work that
    /// waited goes on once there is a network.
    pub fn wifi_event(&mut self, now: Instant, event: WifiEvent) -> Effects {
        let _running = self.shared.meter.enter(meter::CORE);
        self.now = now;
        // While the reader waits for the network, what Wi-Fi says is theirs.
        self.shared.reader = self.io.http.reader_waits();
        // Native plugins' join and host requests end by what the radio says.
        if let Ok(mut table) = self.radio_requests.try_borrow_mut() {
            table.on_wifi_event(&event);
        }
        let Some(wifi) = &mut self.shared.wifi else {
            return self.finish();
        };
        match event {
            WifiEvent::State(state) => {
                if state != wifi.state {
                    if !state.is_on() {
                        wifi.scanning = false;
                    }
                    wifi.state = state;
                    self.output.commands.push(events::WIFI_CHANGED.emit(()));
                    // Status lines show it (`wifi.status`).
                    self.output.commands.push(events::STATUS_CHANGED.emit(()));
                    self.native_hint(crate::native_host::registrations::EVENT_WIFI_CHANGED_TAG);
                    self.native_hint(crate::native_host::registrations::EVENT_STATUS_CHANGED_TAG);
                }
            }
            WifiEvent::Scanned(mut networks) => {
                // Strongest first, each name once (several access points
                // may carry one network).
                paperpaw_sort::sort_by_cached_key(&mut networks, |n| core::cmp::Reverse(n.signal));
                let mut seen: Vec<String> = Vec::new();
                networks.retain(|n| {
                    let new = !n.ssid.is_empty() && !seen.contains(&n.ssid);
                    if new {
                        seen.push(n.ssid.clone());
                    }
                    new
                });
                wifi.networks = networks;
                wifi.scanning = false;
                self.output.commands.push(events::WIFI_SCANNED.emit(()));
                self.native_hint(crate::native_host::registrations::EVENT_WIFI_SCANNED_TAG);
            }
            WifiEvent::Failed { ssid, reason } => {
                self.output
                    .commands
                    .push(events::WIFI_FAILED.emit(events::WifiFailed::new(ssid, reason)));
            }
        }
        self.finish()
    }

    /// Plugins' network work may go to the shell: there is a network, or
    /// the system manages it.
    pub(crate) fn network_ready(&self) -> bool {
        self.shared
            .wifi
            .as_ref()
            .is_none_or(|wifi| wifi.state.is_online())
    }

    /// The device's own network is open (`WifiState::Hosting`).
    fn hosting(&self) -> bool {
        self.shared
            .wifi
            .as_ref()
            .is_some_and(|wifi| wifi.state.is_hosting())
    }

    /// Network work waiting for the shell (requests, servers, sockets).
    pub(crate) fn network_work_waiting(&self) -> bool {
        self.io.http.waiting() || self.io.servers.waiting() || self.io.sockets.waiting()
    }

    /// After every call, where PaperPaw manages Wi-Fi: tells plugins that
    /// work waits for a network ([`events::NETWORK_WANTED`]) or that
    /// nothing uses it any more ([`events::NETWORK_IDLE`]).
    pub(crate) fn check_network(&mut self) {
        // What `core.net` and `core.sockets` want (and a wish of the work
        // scheduler) counts as work that waits where no network is up.
        let (native_wants, native_live) = self.native_demand();
        let work = self.network_work_waiting() || native_wants;
        let Some(wifi) = &mut self.shared.wifi else {
            return;
        };
        let waiting = work && !wifi.state.is_online();
        let (mut wanted, mut idle) = (false, false);
        match (waiting, self.io.network_waiting) {
            (true, None) => {
                self.io.network_waiting = Some(self.now);
                self.output.commands.push(events::NETWORK_WANTED.emit(()));
                wanted = true;
            }
            (false, Some(_)) => self.io.network_waiting = None,
            _ => {}
        }
        // Also once waiting work failed (`fail_waiting_network`): nothing
        // wants the radio then.
        wifi.wanted = self.io.network_waiting.is_some();
        let busy =
            self.io.http.busy() || self.io.servers.busy() || self.io.sockets.any() || native_live;
        wifi.busy = busy;
        if self.io.network_busy && !busy {
            self.output.commands.push(events::NETWORK_IDLE.emit(()));
            idle = true;
        }
        self.io.network_busy = busy;
        // The native network manager hears the same (`device.md` section 3).
        if wanted {
            self.native_hint(crate::native_host::registrations::EVENT_NETWORK_WANTED_TAG);
        }
        if idle {
            self.native_hint(crate::native_host::registrations::EVENT_NETWORK_IDLE_TAG);
        }
    }

    /// When network work that waits fails, if it does not get one first.
    pub(crate) fn network_deadline(&self) -> Option<Instant> {
        self.io
            .network_waiting
            .map(|since| since.add_millis(paperpaw_sdk::wifi::NETWORK_WAIT_MS))
    }

    /// Fails the network work that waited too long, as without a network.
    pub(crate) fn fail_waiting_network(&mut self) {
        const REASON: &str = "no network";
        let storage = &*self.shared.storage;
        let out = &mut self.output.commands;
        self.io.http.fail_waiting(REASON, storage, out);
        self.io.servers.fail_waiting(REASON, out);
        self.io.sockets.fail_waiting(REASON, storage, out);
        self.io.network_waiting = None;
    }

    /// Whether requests are under way that going to sleep waits for a
    /// while (ADR 0025): a position pushed on suspending. Not work waiting
    /// for a network that may never come (Offline, none in reach), nor
    /// jobs (they go on at the next start). A shell that cuts the power on
    /// sleep keeps going until they are answered or a while has passed,
    /// then sleeps anyway.
    pub fn holds_sleep(&self) -> bool {
        self.io.http.busy()
    }

    /// Whether plugins have sockets or broadcasts open, e.g. to keep
    /// polling for what they bring.
    pub fn sockets_open(&self) -> bool {
        self.io.sockets.any()
    }

    /// What happened with a socket or a broadcast.
    pub fn socket_event(&mut self, now: Instant, event: SocketEvent) -> Effects {
        let _running = self.shared.meter.enter(meter::CORE);
        self.now = now;
        // Servers and sockets are the reader's connection.
        self.shared.reader = true;
        let storage = &*self.shared.storage;
        self.io
            .sockets
            .event(event, storage, &mut self.output.commands);
        self.finish()
    }

    /// What happened with a server: started, failed, a request.
    pub fn server_event(&mut self, now: Instant, event: ServerEvent) -> Effects {
        let _running = self.shared.meter.enter(meter::CORE);
        self.now = now;
        // Servers and sockets are the reader's connection.
        self.shared.reader = true;
        let storage = &*self.shared.storage;
        self.io
            .servers
            .event(event, storage, &mut self.output.commands);
        self.finish()
    }

    /// A line from the computer at the other end of the cable (USB), as
    /// the shell read it: requests for the server on the cable (see
    /// `io::cable`, `docs/usb-transfer.md`). A line that is not the
    /// cable's is passed over. Where nothing serves the cable, the screen
    /// bound to [`slots::CABLE`] is opened first. The answers wait in
    /// [`Kernel::take_cable_lines`].
    pub fn cable_line(&mut self, now: Instant, line: &str) -> Effects {
        use io::cable::Line;
        let Some(line) = io::cable::parse(line) else {
            return Effects::default();
        };
        let _running = self.shared.meter.enter(meter::CORE);
        self.now = now;
        // The computer at the cable is the reader's.
        self.shared.reader = true;
        self.last_activity = now;
        let cable = &mut self.io.servers.cable;
        let paused = cable
            .last
            .is_none_or(|last| now.0.saturating_sub(last) >= io::cable::PAUSE_MS);
        cable.last = Some(now.0);
        match line {
            Line::Request {
                n,
                method,
                target,
                len,
            } => {
                if self.io.servers.cable_server().is_none() {
                    // Ended on the device: only a new start (after a
                    // pause) opens it again.
                    if self.io.servers.cable.ended && !paused {
                        let cable = &mut self.io.servers.cable;
                        cable.refuse(n, 403, "ended on the device");
                        return self.finish();
                    }
                    self.io.servers.cable.ended = false;
                    self.output
                        .commands
                        .push(Command::OpenSlot(slots::CABLE, Params::new()));
                    self.run_commands();
                }
                let Some(server) = self.io.servers.cable_server() else {
                    self.io
                        .servers
                        .cable
                        .refuse(n, 503, "nothing serves the cable");
                    return self.finish();
                };
                let storage = &*self.shared.storage;
                if storage.free_space().is_some_and(|free| len > free) {
                    self.io.servers.cable.refuse(n, 507, "not enough room");
                    return self.finish();
                }
                let (request, cut) = self.io.servers.cable.begin(n, len);
                // A body still coming, cut short by this request: let go,
                // and told, unless the computer gave this request its
                // number (it gave the other up): the answer is the new one's.
                match cut {
                    Some((m, cut)) if m == n => self.cable_close(cut),
                    Some((m, cut)) => self.cable_cut(m, cut, "cut short by another request"),
                    None => {}
                }
                let storage = &*self.shared.storage;
                let event = ServerEvent::Request {
                    server,
                    request,
                    method,
                    target,
                    body: len > 0,
                };
                self.io
                    .servers
                    .event(event, storage, &mut self.output.commands);
            }
            Line::Data { n, bytes } => match self.io.servers.cable.data(n, bytes.len()) {
                Some((request, last)) => {
                    let event = ServerEvent::Body {
                        request,
                        data: bytes,
                        last,
                    };
                    let storage = &*self.shared.storage;
                    self.io
                        .servers
                        .event(event, storage, &mut self.output.commands);
                }
                None => {
                    // Its request, if any, is cut short: its body goes.
                    match self.io.servers.cable.cut(n) {
                        Some(request) => self.cable_cut(n, request, "no such body coming"),
                        None => self.io.servers.cable.refuse(n, 400, "no such body coming"),
                    }
                }
            },
            Line::Bad { n: Some(n), why } => match self.io.servers.cable.cut(n) {
                Some(request) => self.cable_cut(n, request, why),
                None => self.io.servers.cable.refuse(n, 400, why),
            },
            Line::Bad { n: None, .. } => {}
        }
        self.finish()
    }

    /// The cable's request `n` (the kernel's `request`) is cut short: its
    /// body is let go, and the computer told why.
    fn cable_cut(&mut self, n: u32, request: u32, why: &str) {
        self.cable_close(request);
        self.io.servers.cable.refuse(n, 400, why);
    }

    /// The kernel's request `request` of the cable ends: its body is let
    /// go, its plugin told.
    fn cable_close(&mut self, request: u32) {
        let storage = &*self.shared.storage;
        let closed = ServerEvent::Closed { request };
        self.io
            .servers
            .event(closed, storage, &mut self.output.commands);
    }

    /// The next line to send to the computer at the cable: send them all
    /// after every kernel call, as [`Kernel::take_io`] is taken. Answers
    /// go a line at a time, so a long one is never written out whole.
    pub fn next_cable_line(&mut self) -> Option<String> {
        self.io.servers.cable.next_line()
    }

    /// All the lines to send to the computer at the cable, in order (see
    /// [`Kernel::next_cable_line`]).
    pub fn take_cable_lines(&mut self) -> Vec<String> {
        core::iter::from_fn(|| self.io.servers.cable.next_line()).collect()
    }

    /// Delivers the answer to a request ([`Io::Http`]).
    pub fn http_response(&mut self, now: Instant, response: HttpResponse) -> Effects {
        let _running = self.shared.meter.enter(meter::CORE);
        self.now = now;
        // The answer to the reader's request is the reader's.
        self.shared.reader = self.io.http.of_reader(response.id);
        let storage = &*self.shared.storage;
        self.io
            .http
            .answer(response, storage, &mut self.output.commands);
        self.finish()
    }

    /// How much of the body of request `id` came so far (`received` bytes,
    /// of `total` where the answer said), for a request that asks
    /// ([`HttpCall::progress`]): shells say so as the body comes, as often
    /// as they like; the plugin hears it at most every 5 % or 2 seconds,
    /// and never after the answer.
    pub fn http_progress(
        &mut self,
        now: Instant,
        id: u32,
        received: u64,
        total: Option<u64>,
    ) -> Effects {
        let _running = self.shared.meter.enter(meter::CORE);
        self.now = now;
        self.io
            .http
            .progress(id, received, total, now, &mut self.output.commands);
        self.finish()
    }

    /// What the shell is to do: requests, servers, sockets, speech, the
    /// clipboard, the radio, the firmware (see [`Io`]). Check after every
    /// kernel call. Network work waits here until there is a network,
    /// where PaperPaw manages Wi-Fi.
    pub fn take_io(&mut self) -> Vec<Io> {
        let mut io: Vec<Io> = self.io.wifi_calls.drain(..).map(Io::Wifi).collect();
        io.extend(self.io.firmware_calls.drain(..).map(Io::Firmware));
        if let Some(radio) = &mut self.io.bluetooth {
            io.extend(radio.take().into_iter().map(Io::Bluetooth));
        }
        io.extend(self.io.speech.take().into_iter().map(Io::Speech));
        io.extend(self.io.clipboard.take_copied().map(Io::Copy));
        if self.io.clipboard.take_read() {
            io.push(Io::ReadClipboard);
        }
        if self.network_ready() {
            io.extend(self.io.http.take().into_iter().map(Io::Http));
            io.extend(self.io.servers.take().into_iter().map(Io::Server));
            io.extend(self.io.sockets.take().into_iter().map(Io::Socket));
        } else if self.hosting() {
            // The device's own network: its servers serve on it.
            io.extend(self.io.servers.take().into_iter().map(Io::Server));
        }
        io
    }

    /// Reads part of a file being sent as an answer
    /// ([`ServerCall::Respond`] with a `file`), from `at` into `buffer`;
    /// returns how much was read, 0 at the end.
    pub fn read_served(
        &self,
        path: &str,
        at: u64,
        buffer: &mut [u8],
    ) -> Result<usize, paperpaw_platform::StorageError> {
        self.shared.storage.read_at(path, at, buffer)
    }

    /// Answers a [`Command::ReadClipboard`] with the text known here.
    pub(crate) fn reply_clipboard(&mut self, reply: Id, params: Params) {
        let typed = paperpaw_sdk::Typed::new(self.shared.clipboard.get(), params);
        self.output
            .commands
            .push(Command::Emit(reply, typed.to_params()));
    }

    /// Whether plugins serve, e.g. to keep polling for requests.
    pub fn serving(&self) -> bool {
        self.io.servers.busy()
    }

    /// How an install goes: plugins hear of it.
    pub fn firmware_state(&mut self, now: Instant, state: FirmwareState) -> Effects {
        let _running = self.shared.meter.enter(meter::CORE);
        self.now = now;
        if let Some(firmware) = &mut self.shared.firmware
            && firmware.state != state
        {
            firmware.state = state;
            self.output.commands.push(events::FIRMWARE_CHANGED.emit(()));
        }
        self.finish()
    }

    /// Text `id` of a [`SpeechCall::Say`] was spoken (`spoken`), or could
    /// not be.
    pub fn speech_done(&mut self, now: Instant, id: u32, spoken: bool) -> Effects {
        let _running = self.shared.meter.enter(meter::CORE);
        self.now = now;
        self.io.speech.done(id, spoken, &mut self.output.commands);
        self.finish()
    }

    /// Whether a text is being spoken, e.g. to keep watching for its end.
    pub fn speaking(&self) -> bool {
        self.io.speech.speaking()
    }

    /// The system clipboard's text, for the plugins that asked for it
    /// ([`Io::ReadClipboard`]); `None` if it could not be read (the text
    /// copied here is used).
    pub fn clipboard_text(&mut self, now: Instant, text: Option<String>) -> Effects {
        let _running = self.shared.meter.enter(meter::CORE);
        self.now = now;
        if let Some(text) = text {
            self.shared.clipboard.set(crate::commands::clipped(&text));
        }
        for (reply, params) in self.io.clipboard.answered() {
            self.reply_clipboard(reply, params);
        }
        self.finish()
    }

    /// Tells the kernel whether the system's clipboard has text, e.g. when
    /// the app comes back from another one (without reading it, which some
    /// systems announce to the user).
    pub fn set_clipboard_available(&mut self, available: bool) {
        self.shared.system_clipboard = available;
    }
}
