//! Everything of the network for the main loop, one step at a time: the
//! kernel's Wi-Fi commands to the radio and what happened back, plugins'
//! HTTP requests (`http`) and their answers, their servers (`serve`) and
//! sockets (`sockets`), and the clock set over the network (`clock`).
//!
//! What is asked over USB (`wifi …`, `http get`) goes the same way; its
//! answers are printed once they come (see `remote`).

use alloc::string::String;
use alloc::vec::Vec;
use core::net::SocketAddr;

use paperpaw_core::{
    Effects, HttpCall, Kernel, ServerCall, ServerEvent, SocketCall, SocketEvent, WifiCommand,
};
#[cfg(feature = "remote")]
use paperpaw_core::{WifiEvent, WifiState};

use crate::serve::Servers;
use crate::sockets::Sockets;
use paperpaw_platform::Instant;

use crate::clock;
use crate::http::Calls;
use crate::net::{Handle, Net, Query, Resolve};
#[cfg(feature = "remote")]
use crate::remote;
use crate::wifi::{Parts, Radio};
#[cfg(feature = "remote")]
use paperpaw_x4::remote::Ask;
use paperpaw_x4::sntp;

/// How often the clock is set again while online, and how long to wait
/// after a failed try.
const RESYNC_MS: u64 = 12 * 60 * 60 * 1000;
const RETRY_MS: u64 = 60_000;
/// How long an answer is waited for.
const ASK_MS: u64 = 5_000;

/// Setting the clock over the network (SNTP).
enum Sync {
    /// Nothing going on; the next try not before this time.
    Idle {
        not_before: u64,
    },
    Resolving {
        query: Query,
        until: u64,
    },
    Asking {
        socket: Handle,
        asked: sntp::Asked,
        until: u64,
    },
}

/// Why plugins' servers and sockets fail without a network.
const NO_NETWORK: &str = "no network";
/// The numbers of requests made over USB, apart from the kernel's.
const REMOTE_REQUESTS: u32 = 0xF000_0000;

/// What the kernel asks of the network.
pub enum NetworkIo {
    Wifi(WifiCommand),
    Http(HttpCall),
    Server(ServerCall),
    Socket(SocketCall),
}

/// Answers still to give over USB.
#[cfg(feature = "remote")]
#[derive(Debug, Default)]
struct Asked {
    scan: bool,
    /// The network asked to join, and its password, kept once joined.
    connect: Option<(String, String)>,
    next_request: u32,
    /// Requests asked: their number and when.
    requests: Vec<(u32, u64)>,
}

pub struct Network {
    radio: Radio,
    calls: Calls,
    servers: Servers,
    sockets: Sockets,
    /// The sockets of interface 7 (`core.sockets`), shared with the loader
    /// that wires core with them.
    #[cfg(feature = "ppn5")]
    host: Option<alloc::rc::Rc<crate::host_net::Bridge>>,
    /// The device behind `core.device`, told of the networks a scan found.
    #[cfg(feature = "ppn5")]
    device: Option<alloc::rc::Rc<paperpaw_x4::device_host::X4Device>>,
    /// What the kernel asked for plugins' servers and sockets, for the
    /// next step.
    server_calls: Vec<ServerCall>,
    socket_calls: Vec<SocketCall>,
    #[cfg(feature = "remote")]
    asked: Asked,
    sync: Sync,
    /// When the clock was last set over the network.
    synced: Option<u64>,
    /// A try to set the clock failed since the radio came online: secure
    /// requests no longer wait for it.
    sync_failed: bool,
    #[cfg(feature = "bluetooth")]
    bluetooth: crate::bluetooth::Bluetooth,
}

impl Network {
    pub fn new(parts: Parts) -> Self {
        Self {
            radio: Radio::new(parts),
            calls: Calls::default(),
            servers: Servers::default(),
            sockets: Sockets::default(),
            #[cfg(feature = "ppn5")]
            host: None,
            #[cfg(feature = "ppn5")]
            device: None,
            server_calls: Vec::new(),
            socket_calls: Vec::new(),
            #[cfg(feature = "remote")]
            asked: Asked::default(),
            sync: Sync::Idle { not_before: 0 },
            synced: None,
            sync_failed: false,
            #[cfg(feature = "bluetooth")]
            bluetooth: crate::bluetooth::Bluetooth::new(),
        }
    }

    /// Runs the interface-7 sockets of `host` from now on.
    #[cfg(feature = "ppn5")]
    pub fn attach(&mut self, host: alloc::rc::Rc<crate::host_net::Bridge>) {
        self.host = Some(host);
    }

    /// Tells `device` of the networks a scan finds, from now on.
    #[cfg(feature = "ppn5")]
    pub fn attach_device(&mut self, device: alloc::rc::Rc<paperpaw_x4::device_host::X4Device>) {
        self.device = Some(device);
    }

    /// A radio is on: its tasks need the processor (no light sleep).
    pub fn radio_on(&self) -> bool {
        #[cfg(feature = "bluetooth")]
        if self.bluetooth.is_on() {
            return true;
        }
        self.radio.is_on()
    }

    /// What the kernel asks of Bluetooth (`Io::Bluetooth`), on the
    /// scheduler Wi-Fi shares.
    #[cfg(feature = "bluetooth")]
    pub fn bluetooth(&mut self, call: paperpaw_core::BtCall) {
        let radio = &mut self.radio;
        self.bluetooth.apply(call, || radio.start_scheduler());
    }

    /// The host name the router lists the device by, from the next time
    /// a network is joined.
    pub fn set_host_name(&mut self, name: &str) {
        self.radio.set_host_name(name);
    }

    /// What the kernel asks of the network (from `Kernel::take_io`),
    /// carried out with the next step.
    pub fn ask(&mut self, io: NetworkIo) {
        match io {
            NetworkIo::Wifi(command) => self.radio.apply(command),
            NetworkIo::Http(call) => self.calls.add(call),
            NetworkIo::Server(call) => self.server_calls.push(call),
            NetworkIo::Socket(call) => self.socket_calls.push(call),
        }
    }

    /// One step: what the kernel asked, what happened, the answers. The
    /// kernel's effects for the main loop to apply. The radio's, the
    /// requests' and the clock's work keep frames of their own
    /// (`#[inline(never)]`): the kernel's, called here too, goes deep.
    pub fn step(&mut self, kernel: &mut Kernel, now: Instant) -> Vec<Effects> {
        let mut effects = Vec::new();
        // The radio sleeps between beacons unless data moves, a page is
        // served (the transfer, the setup with a phone) or a connection
        // is open (Calibre): asleep, it missed parts of what came in a
        // burst, and the connection stalled until the other side gave up.
        let busy = self.moving() || kernel.serving() || kernel.sockets_open() || self.host_open();
        // Beside Bluetooth the radio is shared: Wi-Fi never keeps it to
        // itself (its modem sleeps between beacons, Bluetooth's links run
        // meanwhile).
        #[cfg(feature = "bluetooth")]
        let busy = busy && !self.bluetooth.is_on();
        self.radio.save_power(!busy);
        #[cfg(feature = "ppn5")]
        let mut scanned = false;
        for event in self.radio.poll() {
            #[cfg(feature = "remote")]
            self.answer_asked(kernel, &event);
            #[cfg(feature = "ppn5")]
            {
                scanned |= matches!(event, paperpaw_core::WifiEvent::Scanned(_));
            }
            effects.push(kernel.wifi_event(now, event));
        }
        // The networks the scan found, as the kernel keeps them (sorted, each
        // name once), before its effects are carried out.
        #[cfg(feature = "ppn5")]
        if let (true, Some(device)) = (scanned, &self.device) {
            crate::device_host::sync_networks(device, kernel);
        }
        #[cfg(feature = "bluetooth")]
        self.bluetooth
            .set_wifi_busy(self.radio.busy() || self.moving());
        #[cfg(feature = "bluetooth")]
        for report in self.bluetooth.poll() {
            effects.push(kernel.bluetooth_event(now, report));
        }
        // Secure requests and sockets wait for the clock (their
        // certificates' dates are checked against it), unless it cannot be
        // had.
        let clock_ready = clock::unix_ms(now).is_some() || self.sync_failed;
        self.serve(kernel, now, clock_ready, &mut effects);
        #[cfg(feature = "ppn5")]
        if let Some(host) = &self.host {
            effects.extend(host.run(&mut self.radio, kernel, now, clock_ready));
        }
        self.sync_clock(now);
        if self.calls.busy() {
            for answer in self
                .calls
                .poll(self.radio.net(), kernel.storage(), clock_ready)
            {
                if answer.id >= REMOTE_REQUESTS {
                    #[cfg(feature = "remote")]
                    self.answer_request(&answer, now);
                } else {
                    effects.push(kernel.http_response(now, answer));
                }
            }
            for (id, received, total) in self.calls.progress() {
                effects.push(kernel.http_progress(now, id, received, total));
            }
            if self.calls.take_restart() {
                self.radio.restart();
            }
        }
        effects
    }

    /// Answers a request made over USB.
    #[cfg(feature = "remote")]
    fn answer_request(&mut self, answer: &paperpaw_core::HttpResponse, now: Instant) {
        let asked = self
            .asked
            .requests
            .iter()
            .position(|(id, _)| *id == answer.id);
        let since = asked.map(|at| self.asked.requests.remove(at).1);
        remote::http_answer(answer, since.map_or(0, |at| now.0.saturating_sub(at)));
    }

    /// Moves the TCP/IP stack and the Bluetooth host on and nothing else:
    /// what the radio brought is taken out of its queue (five frames, which
    /// drops what comes beyond) and acknowledged; a remote's keys and the
    /// links' packets are taken. For the long waits of the main loop (a
    /// refresh of the panel, up to 3.5 s) where [`Network::step`] cannot be
    /// called; the requests' and servers' work waits for the next step, the
    /// window of each connection keeping the other side from sending more
    /// than the connection takes, and what Bluetooth reports waits for it
    /// too.
    pub fn keep_served(&mut self) {
        if let Some(net) = self.radio.net() {
            net.poll();
        }
        #[cfg(feature = "bluetooth")]
        self.bluetooth.keep_polled();
    }

    /// Leaves the network before the device starts again or sleeps
    /// ([`Radio::leave`]).
    pub fn leave(&mut self) {
        self.radio.leave();
    }

    /// Whether data is moving for plugins' requests, servers or sockets:
    /// the main loop comes by again at once.
    pub fn moving(&self) -> bool {
        self.calls.busy() || self.servers.moving() || self.sockets.moving() || self.host_moving()
    }

    /// Whether the interface-7 sockets have something to do now.
    fn host_moving(&self) -> bool {
        #[cfg(feature = "ppn5")]
        return self.host.as_ref().is_some_and(|host| host.moving());
        #[cfg(not(feature = "ppn5"))]
        false
    }

    /// Whether an interface-7 socket is open (the radio does not doze).
    fn host_open(&self) -> bool {
        #[cfg(feature = "ppn5")]
        return self.host.as_ref().is_some_and(|host| host.open());
        #[cfg(not(feature = "ppn5"))]
        false
    }

    /// Plugins' servers and sockets: what the kernel asked, then a step
    /// for each. Without a network they fail (what was open is let go).
    fn serve(
        &mut self,
        kernel: &mut Kernel,
        now: Instant,
        clock_ready: bool,
        effects: &mut Vec<Effects>,
    ) {
        let Some(net) = self.radio.online() else {
            let mut net = self.radio.net();
            effects.extend(self.servers.lost(net.as_deref_mut(), kernel, now));
            effects.extend(self.sockets.lost(net, kernel, now));
            for call in core::mem::take(&mut self.server_calls) {
                if let ServerCall::Listen { server, .. } = call {
                    let failed = ServerEvent::Failed {
                        server,
                        reason: String::from(NO_NETWORK),
                    };
                    effects.push(kernel.server_event(now, failed));
                }
            }
            for call in core::mem::take(&mut self.socket_calls) {
                let event = match call {
                    SocketCall::Connect { socket, .. } | SocketCall::StartTls { socket, .. } => {
                        SocketEvent::Failed {
                            socket,
                            error: paperpaw_core::sdk::SocketError::NoNetwork,
                            reason: String::from(NO_NETWORK),
                        }
                    }
                    SocketCall::Broadcast { broadcast, .. } => SocketEvent::Done { broadcast },
                    _ => continue,
                };
                effects.push(kernel.socket_event(now, event));
            }
            return;
        };
        for call in core::mem::take(&mut self.server_calls) {
            effects.extend(self.servers.apply(call, net, kernel, now));
        }
        for call in core::mem::take(&mut self.socket_calls) {
            if let Some(event) = self.sockets.apply(call, net, now) {
                effects.push(kernel.socket_event(now, event));
            }
        }
        effects.extend(self.servers.poll(net, kernel, now));
        effects.extend(self.sockets.poll(net, kernel, now, clock_ready));
    }

    /// Sets the clock over the network when it is not known or old, once
    /// the radio is online; a step at a time, never waiting.
    #[inline(never)]
    fn sync_clock(&mut self, now: Instant) {
        let ms = now.0;
        let Some(net) = self.radio.online() else {
            // Offline: whatever went on is let go.
            let sync = core::mem::replace(&mut self.sync, Sync::Idle { not_before: 0 });
            if let Some(net) = self.radio.net() {
                match sync {
                    Sync::Idle { .. } => {}
                    Sync::Resolving { query, .. } => net.forget(query),
                    Sync::Asking { socket, .. } => net.release_udp(socket),
                }
            }
            self.sync_failed = false;
            return;
        };
        let due = clock::unix_ms(now).is_none()
            || self
                .synced
                .is_none_or(|at| ms.saturating_sub(at) >= RESYNC_MS);
        self.sync = match core::mem::replace(&mut self.sync, Sync::Idle { not_before: 0 }) {
            Sync::Idle { not_before } if !due || ms < not_before => Sync::Idle { not_before },
            Sync::Idle { .. } => match net.resolve(sntp::SERVER) {
                Ok(Resolve::Done(address)) => ask(net, address, ms),
                Ok(Resolve::Asking(query)) => Sync::Resolving {
                    query,
                    until: ms + ASK_MS,
                },
                Err(_) => self.sync_failed(ms),
            },
            Sync::Resolving { query, until } => match net.resolved(query) {
                Some(Ok(address)) => ask(net, address, ms),
                Some(Err(_)) => self.sync_failed(ms),
                None if ms > until => {
                    net.forget(query);
                    self.sync_failed(ms)
                }
                None => Sync::Resolving { query, until },
            },
            Sync::Asking {
                socket,
                asked,
                until,
            } => {
                let udp = net.udp(socket);
                let mut time = None;
                while let Ok((data, meta)) = udp.recv() {
                    let from = SocketAddr::new(meta.endpoint.addr.into(), meta.endpoint.port);
                    time = time.or(asked.answer(data, from));
                }
                if let Some(unix_ms) = time {
                    net.release_udp(socket);
                    clock::set(now, unix_ms);
                    crate::input::keep_time(unix_ms);
                    self.synced = Some(ms);
                    log::info!("clock: set over the network");
                    Sync::Idle { not_before: 0 }
                } else if ms > until {
                    net.release_udp(socket);
                    self.sync_failed(ms)
                } else {
                    Sync::Asking {
                        socket,
                        asked,
                        until,
                    }
                }
            }
        };
    }

    fn sync_failed(&mut self, ms: u64) -> Sync {
        log::info!("clock: not set over the network");
        self.sync_failed = true;
        Sync::Idle {
            not_before: ms + RETRY_MS,
        }
    }

    /// What was asked over USB.
    #[cfg(feature = "remote")]
    pub fn ask_remote(&mut self, ask: Ask) {
        match ask {
            Ask::Scan => {
                self.asked.scan = true;
                self.radio.apply(WifiCommand::On);
                self.radio.apply(WifiCommand::Scan);
            }
            Ask::Connect { ssid, password } => {
                self.radio.apply(WifiCommand::On);
                self.radio.apply(WifiCommand::Join {
                    ssid: ssid.clone(),
                    password: password.clone(),
                });
                self.asked.connect = Some((ssid, password));
            }
            Ask::Status => remote::wifi_state(self.radio.state()),
            Ask::Off => {
                self.radio.apply(WifiCommand::Off);
                remote::ok();
            }
            Ask::Get { url, save_to } => {
                let id = REMOTE_REQUESTS + self.asked.next_request;
                self.asked.requests.push((id, crate::now().0));
                self.asked.next_request = self.asked.next_request.wrapping_add(1) % 0x0FFF_FFFF;
                self.calls.add(HttpCall {
                    id,
                    method: String::from("GET"),
                    url,
                    headers: Vec::new(),
                    body: None,
                    body_from: None,
                    max_response: if save_to.is_some() {
                        paperpaw_core::MAX_FILE
                    } else {
                        64 * 1024
                    },
                    timeout_ms: 30_000,
                    want_headers: Vec::new(),
                    save_to,
                    progress: false,
                });
            }
        }
    }

    /// Answers what was asked over USB once it is known.
    #[cfg(feature = "remote")]
    fn answer_asked(&mut self, kernel: &mut Kernel, event: &WifiEvent) {
        match event {
            WifiEvent::Scanned(networks) if self.asked.scan => {
                self.asked.scan = false;
                remote::networks(networks);
            }
            WifiEvent::State(WifiState::Online { ssid, address, .. }) => {
                if let Some((asked, password)) = self.asked.connect.take() {
                    if asked == *ssid {
                        kernel.set_secret(paperpaw_sdk::wifi::keep(&asked, &password));
                        remote::connected(ssid, address);
                    } else {
                        self.asked.connect = Some((asked, password));
                    }
                }
            }
            WifiEvent::Failed { ssid, reason }
                if self
                    .asked
                    .connect
                    .as_ref()
                    .is_some_and(|(asked, _)| asked == ssid) =>
            {
                self.asked.connect = None;
                remote::not_connected(ssid, *reason);
            }
            _ => {}
        }
    }
}

/// Sends an SNTP request to `address`, with a random number its answer
/// must give back.
fn ask(net: &mut Net, address: smoltcp::wire::IpAddress, ms: u64) -> Sync {
    let Ok(socket) = net.open_udp(2, 2 * sntp::MESSAGE) else {
        return Sync::Idle {
            not_before: ms + RETRY_MS,
        };
    };
    let rng = esp_hal::rng::Rng::new();
    let asked = sntp::Asked {
        server: SocketAddr::new(address.into(), sntp::PORT),
        nonce: u64::from(rng.random()) << 32 | u64::from(rng.random()),
    };
    if net
        .udp(socket)
        .send_slice(&asked.request(), (address, sntp::PORT))
        .is_err()
    {
        net.release_udp(socket);
        return Sync::Idle {
            not_before: ms + RETRY_MS,
        };
    }
    Sync::Asking {
        socket,
        asked,
        until: ms + ASK_MS,
    }
}
