//! The radio, as the kernel tells it (`paperpaw_sdk::wifi`): on and off,
//! looking for networks, joining and leaving one, and what happened. When
//! and which network is the `wifi` plugin's to decide; this only does it.
//!
//! esp-radio (Espressif's radio firmware with a Rust driver around it,
//! whose interface is still a beta before its 1.0) is used here alone, so a
//! change of it changes this file. It needs a scheduler that switches
//! tasks (esp-rtos), started the first time the radio is and running on
//! (idle) after. Off, the driver is dropped: the radio is powered down and
//! its memory (about 47 KB) free again.
//!
//! Looking for networks and joining one take a while: they go on in the
//! background, moved on by [`Radio::poll`] from the main loop, so the X4
//! stays usable.
//!
//! For the setup with a phone it opens a network of its own (an access
//! point, `WifiCommand::Host`) at [`HOST_ADDRESS`]; any other command
//! closes it again, the radio back in a station's part.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::future::Future;
use core::pin::{Pin, pin};
use core::task::{Context, Poll, Waker};

use esp_hal::peripherals::{FROM_CPU_INTR0, WIFI};
use esp_hal::timer::timg::Timer;
use esp_radio::wifi::ap::AccessPointConfig;
use esp_radio::wifi::scan::ScanConfig;
use esp_radio::wifi::sta::{ScanMethod, StationConfig};
use esp_radio::wifi::{
    AuthenticationMethod, AuthenticationMethodConfig, Config, ConnectionError, ControllerConfig,
    DisconnectReason, PowerSaveMode, WifiController,
};
use paperpaw_core::{JoinFailure, WifiCommand, WifiEvent, WifiNetwork, WifiState};

use crate::net::Net;
use crate::power::{at, mark};

/// The longest a scan or a try to join is waited for, and to stop.
const WAIT_MS: u64 = 15_000;
const STOP_MS: u64 = 2_000;
/// Tries of one access point before the next is tried.
const JOIN_RETRIES: u8 = 2;
/// Networks a scan reports at most.
const SCAN_MAX: usize = 20;
/// The device's address in its own network.
const HOST_ADDRESS: [u8; 4] = [192, 168, 4, 1];
/// Phones in the device's own network at a time.
const HOST_CLIENTS: u16 = 2;

/// What the radio needs to start the first time.
pub struct Parts {
    pub timer: Timer<'static>,
    pub interrupt: FROM_CPU_INTR0<'static>,
    pub wifi: WIFI<'static>,
}

type Controller = &'static mut WifiController<'static>;

/// What a background task of the radio brought.
/// The networks a scan found, and the security each asks for.
type Found = (Vec<WifiNetwork>, Vec<(String, AuthenticationMethod)>);

enum Outcome {
    Scanned(Option<Found>),
    /// Joined, or why not.
    Joined(Result<(), JoinFailure>),
    Left,
    /// The device's own network named so is open, or could not be.
    Hosted {
        ssid: String,
        opened: bool,
    },
}

type Task = Pin<Box<dyn Future<Output = (Outcome, Controller)>>>;

enum Work {
    Idle(Controller),
    Busy(Task),
}

/// The radio while it is on.
struct On {
    work: Work,
    net: Net,
    /// The network joined or being joined.
    ssid: Option<String>,
    /// Joined (the link is up), waiting for an address or with one.
    linked: bool,
    /// The device's own network is open, named so.
    hosting: Option<String>,
    /// What to do once the task going on is done.
    queued: Vec<WifiCommand>,
    /// The power saving set (none yet: the driver's, off).
    saving: Option<bool>,
    /// The security each network of the last scan asks for, to join it
    /// so ([`authentication`]).
    security: Vec<(String, AuthenticationMethod)>,
}

pub struct Radio {
    /// Until the scheduler started: what it takes.
    scheduler: Option<(Timer<'static>, FROM_CPU_INTR0<'static>)>,
    /// The peripheral, until the radio first starts.
    peripheral: Option<WIFI<'static>>,
    on: Option<On>,
    /// Switch off once the task going on is done.
    stopping: bool,
    /// Start anew once the task going on is done ([`Radio::restart`]).
    restarting: bool,
    /// The state last reported.
    reported: WifiState,
    /// What to report.
    events: Vec<WifiEvent>,
    /// The host name the router lists the device by, and its DHCP option
    /// (kept for the network's life: made anew only when the name
    /// changes).
    host: Option<(String, &'static [smoltcp::wire::DhcpOption<'static>])>,
}

/// `future`'s outcome, or `None` if it takes longer than `ms`.
async fn within<F: Future>(ms: u64, future: F) -> Option<F::Output> {
    let until = esp_hal::time::Instant::now() + esp_hal::time::Duration::from_millis(ms);
    let mut future = pin!(future);
    core::future::poll_fn(|cx| match future.as_mut().poll(cx) {
        Poll::Ready(value) => Poll::Ready(Some(value)),
        // Polled again and again (from the main loop): no waking needed.
        Poll::Pending if esp_hal::time::Instant::now() > until => Poll::Ready(None),
        Poll::Pending => Poll::Pending,
    })
    .await
}

/// How to join a network the last scan found with `method`, with
/// `password`: with the security it asks for, which the radio takes as
/// the least it accepts (so WPA2 also joins a WPA2/WPA3 network, and WPA3
/// where the radio has it); WPA2 where the scan did not say. One this
/// device cannot join at all is said so at once.
fn authentication(
    method: Option<AuthenticationMethod>,
    password: &str,
) -> Result<AuthenticationMethodConfig, JoinFailure> {
    use AuthenticationMethod as M;
    use AuthenticationMethodConfig as C;
    if password.is_empty() || method == Some(M::None) {
        return Ok(C::Open);
    }
    let password = || password.try_into().map_err(|_| JoinFailure::Other);
    Ok(match method {
        Some(M::Wep) => C::Wep(password()?),
        Some(M::Wpa) => C::Wpa(password()?),
        Some(M::WpaWpa2Personal) => C::WpaWpa2Personal(password()?),
        Some(M::Wpa2Enterprise | M::Wpa3EntSuiteB192Bit | M::WapiPersonal) => {
            log::info!("wifi: {method:?} is a security this device cannot join");
            return Err(JoinFailure::Other);
        }
        _ => C::Wpa2Personal(password()?),
    })
}

/// Why joining failed, as the SDK says it (the radio's own reason is in
/// the log); `method`, the security the scan found the network with. A
/// security this device cannot join, or a network turning it away, is
/// `Other`, as is one broken off while joining.
fn failure(error: &ConnectionError, method: Option<AuthenticationMethod>) -> JoinFailure {
    use DisconnectReason as R;
    match error {
        ConnectionError::Failed(info) => match info.reason {
            R::NoAccessPointFound | R::NoAccessPointFoundInRssiThreshold => JoinFailure::NotFound,
            R::NoAccessPointFoundWithCompatibleSecurity
            | R::NoAccessPointFoundInAuthmodeThreshold
            | R::GroupCipherInvalid
            | R::PairwiseCipherInvalid
            | R::AkmpInvalid
            | R::CipherSuiteRejected
            | R::BadCipherOrAkm => JoinFailure::Other,
            // WPA3 alone: a refused authentication is its security, not
            // (only) a password.
            R::AuthenticationFailed if method == Some(AuthenticationMethod::Wpa3Personal) => {
                JoinFailure::Other
            }
            R::AuthenticationFailed
            | R::FourWayHandshakeTimeout
            | R::HandshakeTimeout
            | R::MicFailure => JoinFailure::Password,
            R::AssociationFailed
            | R::ConnectionFailed
            | R::AssociationTooMany
            | R::NotAuthorizedThisLocation => JoinFailure::Other,
            R::AssociationLeave | R::AuthenticationLeave | R::StationLeaving => JoinFailure::Other,
            R::Timeout | R::BeaconTimeout => JoinFailure::Timeout,
            _ => JoinFailure::Other,
        },
        _ => JoinFailure::Other,
    }
}

impl Radio {
    pub fn new(parts: Parts) -> Self {
        Self {
            scheduler: Some((parts.timer, parts.interrupt)),
            peripheral: Some(parts.wifi),
            on: None,
            stopping: false,
            restarting: false,
            reported: WifiState::Off,
            events: Vec::new(),
            host: None,
        }
    }

    /// The state last reported.
    #[cfg(feature = "remote")]
    pub fn state(&self) -> &WifiState {
        &self.reported
    }

    /// The radio is on (light sleep would stop its tasks).
    pub fn is_on(&self) -> bool {
        self.on.is_some()
    }

    /// A task of the radio's goes on: joining, a scan, hosting.
    #[cfg(feature = "bluetooth")]
    pub fn busy(&self) -> bool {
        matches!(self.on.as_ref().map(|on| &on.work), Some(Work::Busy(_)))
    }

    /// The network while the radio is on, with an address or without one
    /// (not yet, or no longer), so that what was opened on it can be let
    /// go once the address is gone.
    pub fn net(&mut self) -> Option<&mut Net> {
        self.on.as_mut().map(|on| &mut on.net)
    }

    /// The network, once there is an address.
    pub fn online(&mut self) -> Option<&mut Net> {
        self.net().filter(|net| net.address().is_some())
    }

    /// The host name `name` (letters, digits and hyphens) for the router,
    /// from the next time a network is joined.
    pub fn set_host_name(&mut self, name: &str) {
        if self.host.as_ref().is_some_and(|(kept, _)| kept == name) {
            return;
        }
        let data: &'static [u8] = Box::leak(name.as_bytes().to_vec().into_boxed_slice());
        let option: &'static [smoltcp::wire::DhcpOption<'static>] =
            Box::leak(Box::new([smoltcp::wire::DhcpOption { kind: 12, data }]));
        self.host = Some((String::from(name), option));
        if let Some(on) = &mut self.on {
            on.net.set_host_name(option);
        }
    }

    /// The DHCP option naming the device, if it has a name yet.
    fn host_option(&self) -> Option<&'static [smoltcp::wire::DhcpOption<'static>]> {
        self.host.as_ref().map(|(_, option)| *option)
    }

    /// A station's network, named as the device is (`host`).
    fn station(host: Option<&'static [smoltcp::wire::DhcpOption<'static>]>) -> Net {
        let mut net = Net::new(esp_radio::wifi::Interface::station());
        if let Some(option) = host {
            net.set_host_name(option);
        }
        net
    }

    /// Does what the kernel asked (now, or once the task going on is
    /// done).
    pub fn apply(&mut self, command: WifiCommand) {
        match (&mut self.on, command) {
            (None, WifiCommand::On) => self.start(),
            (None, _) => {}
            (Some(_), WifiCommand::On) => {}
            (Some(on), WifiCommand::Off) => {
                on.queued.clear();
                self.stopping = true;
            }
            (Some(on), command) => {
                on.queued.push(command);
            }
        }
    }

    /// The scheduler the radio's tasks run on, started once (for Wi-Fi,
    /// or for Bluetooth with the feature `bluetooth`).
    pub fn start_scheduler(&mut self) {
        if let Some((timer, interrupt)) = self.scheduler.take() {
            esp_rtos::start(timer, interrupt);
        }
    }

    fn start(&mut self) {
        mark(at::RADIO_SCHEDULER);
        self.start_scheduler();
        mark(at::RADIO_DRIVER);
        let peripheral = self.peripheral.take().unwrap_or_else(|| {
            // SAFETY: the radio was on before and is off: its controller,
            // which held the peripheral, was dropped, so nothing else uses
            // it.
            unsafe { WIFI::steal() }
        });
        let mut controller = match WifiController::new(peripheral, controller_config()) {
            Ok(controller) => controller,
            Err(e) => {
                log::warn!("wifi: driver: {e:?}");
                return;
            }
        };
        mark(at::RADIO_STATION);
        if let Err(e) = controller.set_config(&Config::Station(StationConfig::default())) {
            log::warn!("wifi: station: {e:?}");
            return;
        }
        mark(at::RADIO_STARTED);
        log::info!("wifi: on");
        let net = Self::station(self.host_option());
        crate::entropy::wifi(true);
        self.on = Some(On {
            work: Work::Idle(Box::leak(Box::new(controller))),
            net,
            ssid: None,
            linked: false,
            hosting: None,
            queued: Vec::new(),
            saving: None,
            security: Vec::new(),
        });
        self.stopping = false;
    }

    /// Moves the radio on: tasks, the network, what to report.
    // A frame of its own: not under the kernel's work in `Network::step`.
    #[inline(never)]
    pub fn poll(&mut self) -> Vec<WifiEvent> {
        if let Some(on) = &mut self.on {
            on.net.poll();
            if let Work::Busy(task) = &mut on.work {
                let mut cx = Context::from_waker(Waker::noop());
                if let Poll::Ready((outcome, controller)) = task.as_mut().poll(&mut cx) {
                    on.work = Work::Idle(controller);
                    Self::finished(on, outcome, &mut self.events);
                }
            }
            if let Work::Idle(controller) = &on.work
                && on.linked
                && !controller.is_connected()
            {
                // The network went (out of reach, the router restarted).
                log::info!("wifi: link lost");
                on.linked = false;
                on.ssid = None;
                on.net.link_lost();
            }
        }
        let idle = matches!(self.on.as_ref().map(|on| &on.work), Some(Work::Idle(_)));
        if self.stopping && idle {
            self.stop();
        } else if self.restarting && idle {
            self.restarting = false;
            log::info!("wifi: starting anew");
            self.stop();
            self.start();
        }
        self.next_task();
        self.report();
        core::mem::take(&mut self.events)
    }

    /// What a task brought.
    fn finished(on: &mut On, outcome: Outcome, events: &mut Vec<WifiEvent>) {
        match outcome {
            Outcome::Scanned(found) => {
                let (networks, security) = found.unwrap_or_default();
                on.security = security;
                events.push(WifiEvent::Scanned(networks));
            }
            Outcome::Joined(Ok(())) => {
                on.linked = true;
                on.net.joined();
            }
            Outcome::Joined(Err(reason)) => {
                let ssid = on.ssid.take().unwrap_or_default();
                log::info!("wifi: not joined: {reason:?}");
                events.push(WifiEvent::Failed { ssid, reason });
            }
            Outcome::Left => {
                on.linked = false;
                on.ssid = None;
            }
            Outcome::Hosted { ssid, opened } => {
                let net = opened
                    .then(|| Net::hosted(esp_radio::wifi::Interface::access_point(), HOST_ADDRESS))
                    .flatten();
                match net {
                    Some(net) => {
                        log::info!("wifi: hosting {ssid}");
                        on.net = net;
                        on.hosting = Some(ssid);
                    }
                    None => {
                        // Not left open without a network on it: a
                        // station again, and said.
                        log::warn!("wifi: the own network could not open");
                        if let Work::Idle(controller) = &mut on.work
                            && let Err(e) =
                                controller.set_config(&Config::Station(StationConfig::default()))
                        {
                            log::warn!("wifi: station: {e:?}");
                        }
                        events.push(WifiEvent::Failed {
                            ssid,
                            reason: JoinFailure::Other,
                        });
                    }
                }
            }
        }
    }

    /// Starts the next command queued, if nothing goes on.
    fn next_task(&mut self) {
        let host = self.host_option();
        let Some(on) = &mut self.on else {
            return;
        };
        if self.stopping || !matches!(on.work, Work::Idle(_)) || on.queued.is_empty() {
            return;
        }
        let command = on.queued.remove(0);
        let Work::Idle(controller) =
            core::mem::replace(&mut on.work, Work::Busy(Box::pin(core::future::pending())))
        else {
            return;
        };
        // Anything but hosting again closes the own network: a station's
        // part and network again (what was opened on it is gone).
        let closing = on.hosting.is_some() && !matches!(command, WifiCommand::Host { .. });
        if closing {
            log::info!("wifi: own network closed");
            on.hosting = None;
            on.net = Self::station(host);
            if !matches!(command, WifiCommand::Join { .. })
                && let Err(e) = controller.set_config(&Config::Station(StationConfig::default()))
            {
                log::warn!("wifi: station: {e:?}");
            }
        }
        on.work = Work::Busy(match command {
            WifiCommand::Scan => {
                mark(at::RADIO_SCAN);
                Box::pin(async move {
                    let config = ScanConfig::default().with_max(SCAN_MAX);
                    let found = within(WAIT_MS, controller.scan_async(&config)).await;
                    let found = found.and_then(Result::ok).map(|found| {
                        let security = found
                            .iter()
                            .filter_map(|ap| {
                                Some((String::from(ap.ssid.as_str()), ap.auth_method?))
                            })
                            .collect();
                        let networks = found
                            .into_iter()
                            .map(|ap| {
                                WifiNetwork::new(
                                    String::from(ap.ssid.as_str()),
                                    ap.signal_strength,
                                    ap.auth_method
                                        .is_some_and(|m| m != AuthenticationMethod::None),
                                )
                            })
                            .collect();
                        (networks, security)
                    });
                    (Outcome::Scanned(found), controller)
                })
            }
            WifiCommand::Join { ssid, password } => {
                mark(at::RADIO_CONNECT);
                let method = on
                    .security
                    .iter()
                    .find(|(name, _)| *name == ssid)
                    .map(|(_, method)| *method);
                log::info!("wifi: joining {ssid} ({method:?})");
                let authentication = authentication(method, &password);
                let config = authentication.and_then(|authentication| {
                    // All channels, then the strongest access point: in a
                    // network of several (a mesh), the first one heard may
                    // be a far one that never lets the X4 in. Another one
                    // is tried if it fails.
                    Ok(StationConfig::default()
                        .with_ssid(ssid.as_str().try_into().map_err(|_| JoinFailure::Other)?)
                        .with_authentication(authentication)
                        .with_scan_method(ScanMethod::AllChannels)
                        .with_failure_retry_cnt(JOIN_RETRIES))
                });
                let target = ssid.clone();
                on.ssid = Some(ssid);
                on.linked = false;
                Box::pin(async move {
                    let config = match config {
                        Ok(config) => config,
                        Err(failure) => return (Outcome::Joined(Err(failure)), controller),
                    };
                    // Left first: joining another network while still in
                    // one ended at once, when the one left said so.
                    if controller.is_connected() {
                        log::info!("wifi: leaving first");
                        let _ = within(STOP_MS, controller.disconnect_async()).await;
                    }
                    if controller.set_config(&Config::Station(config)).is_err() {
                        return (Outcome::Joined(Err(JoinFailure::Other)), controller);
                    }
                    // Bounded: once, after flashing, no answer ever came.
                    let started = esp_hal::time::Instant::now();
                    let mut answer = within(WAIT_MS, controller.connect_async()).await;
                    // Another network's leaving (late) is not this one's
                    // answer: asked once more.
                    if let Some(Err(ConnectionError::Failed(info))) = &answer
                        && info.ssid.as_str() != target.as_str()
                    {
                        log::info!("wifi: {} left meanwhile, asked again", info.ssid.as_str());
                        answer = within(WAIT_MS, controller.connect_async()).await;
                    }
                    let joined = match answer {
                        Some(Ok(_)) => Ok(()),
                        Some(Err(e)) => {
                            log::info!("wifi: join refused: {e:?}");
                            Err(failure(&e, method))
                        }
                        None => {
                            log::info!("wifi: join gave no answer");
                            let _ = within(STOP_MS, controller.disconnect_async()).await;
                            Err(JoinFailure::Timeout)
                        }
                    };
                    log::info!("wifi: join took {} ms", started.elapsed().as_millis());
                    (Outcome::Joined(joined), controller)
                })
            }
            WifiCommand::Leave => Box::pin(async move {
                if controller.is_connected() {
                    let _ = within(STOP_MS, controller.disconnect_async()).await;
                }
                (Outcome::Left, controller)
            }),
            WifiCommand::Host { ssid, password } => {
                on.ssid = None;
                on.linked = false;
                let config = password
                    .as_str()
                    .try_into()
                    .map(AuthenticationMethodConfig::Wpa2Personal)
                    .and_then(|authentication| {
                        Ok(AccessPointConfig::default()
                            .with_ssid(ssid.as_str().try_into()?)
                            .with_authentication(authentication)
                            .with_max_connections(HOST_CLIENTS))
                    });
                Box::pin(async move {
                    let opened = config.ok().is_some_and(|config| {
                        controller.set_config(&Config::AccessPoint(config)).is_ok()
                    });
                    (Outcome::Hosted { ssid, opened }, controller)
                })
            }
            // On and Off, and a command to come: nothing to do here.
            _ => Box::pin(async move { (Outcome::Left, controller) }),
        });
    }

    /// Starts the radio anew, once the task going on is done: the driver
    /// dropped and made again, with what was stuck in it (a connection
    /// that got no answer at all, `paperpaw_x4::stall`). Reported as on and
    /// not joined, the network is joined again as after a link lost.
    pub fn restart(&mut self) {
        if self.on.is_some() && !self.stopping {
            self.restarting = true;
        }
    }

    /// Leaves the network and switches the radio off at once, before the
    /// device starts again or sleeps: switched off, the driver takes leave
    /// of the router (`esp_wifi_stop`), which a reset or the power cut in
    /// sleep do not; a FRITZ!Box then refused the X4's joins for a while
    /// (seen after updates and flashing). A task going on is waited for
    /// [`STOP_MS`] at the longest, and the radio left on if it goes on.
    pub fn leave(&mut self) {
        if self.on.is_none() {
            return;
        }
        let until = esp_hal::time::Instant::now() + esp_hal::time::Duration::from_millis(STOP_MS);
        paperpaw_x4::bounded::wait(
            || {
                let busy = matches!(self.on.as_ref().map(|on| &on.work), Some(Work::Busy(_)));
                if busy {
                    let _ = self.poll();
                }
                !busy
            },
            || esp_hal::time::Instant::now() > until,
        );
        if self
            .on
            .as_ref()
            .is_some_and(|on| matches!(on.work, Work::Idle(_)))
        {
            log::info!("wifi: leaving the network");
            self.stop();
        }
    }

    /// Switches the radio off: the driver dropped, the radio powered down.
    fn stop(&mut self) {
        let Some(on) = self.on.take() else {
            return;
        };
        // From now the generator's numbers are no longer random.
        crate::entropy::wifi(false);
        drop(on.net);
        if let Work::Idle(controller) = on.work {
            // SAFETY: made by `Box::leak` in `start` and back from its last
            // task: nothing else holds it.
            drop(unsafe { Box::from_raw(core::ptr::from_mut(controller)) });
        }
        self.stopping = false;
        log::info!("wifi: off");
    }

    /// The radio's power saving: between the network's beacons it sleeps
    /// (ADR 0025), unless data moves (a download, a server's answer), which
    /// it would slow by as long as it sleeps.
    pub fn save_power(&mut self, saving: bool) {
        let Some(on) = &mut self.on else {
            return;
        };
        if on.saving == Some(saving) {
            return;
        }
        let Work::Idle(controller) = &mut on.work else {
            return;
        };
        let mode = if saving {
            PowerSaveMode::Minimum
        } else {
            PowerSaveMode::None
        };
        match controller.set_power_saving(mode) {
            Ok(()) => on.saving = Some(saving),
            Err(e) => log::warn!("wifi: power saving: {e:?}"),
        }
    }

    /// Reports the state when it changed.
    fn report(&mut self) {
        let state = match &mut self.on {
            None => WifiState::Off,
            Some(On {
                hosting: Some(ssid),
                net,
                ..
            }) => WifiState::Hosting {
                ssid: ssid.clone(),
                address: net
                    .address()
                    .map(|a| alloc::format!("{}", a.address()))
                    .unwrap_or_default(),
                // A phone that took an address: reported as it joins.
                guests: net.guests(),
            },
            Some(on) => match (&on.ssid, on.linked, on.net.address()) {
                (Some(ssid), true, Some(address)) => WifiState::Online {
                    ssid: ssid.clone(),
                    address: alloc::format!("{}", address.address()),
                    name: on.net.local_name(),
                },
                (Some(ssid), _, _) => WifiState::Joining { ssid: ssid.clone() },
                (None, ..) => WifiState::On,
            },
        };
        if state != self.reported {
            log::info!("wifi: {state:?}");
            self.reported = state.clone();
            self.events.push(WifiEvent::State(state));
        }
    }
}

/// The radio's firmware (libphy) locks the chip's internal I2C bus with
/// these while it calibrates; it brings empty ones, which an operating
/// system replaces. The bus is shared with the ADC, which reads the
/// buttons and the battery in an interrupt: that interrupt must not come
/// in between, so these hold all interrupts off (nested, by a count).
static mut PHY_I2C_LOCK: (u32, Option<critical_section::RestoreState>) = (0, None);

#[unsafe(no_mangle)]
extern "C" fn phy_i2c_enter_critical() {
    // SAFETY: the critical section is entered first; then nothing else
    // runs until it is left (one core).
    unsafe {
        let state = critical_section::acquire();
        let lock = &mut *core::ptr::addr_of_mut!(PHY_I2C_LOCK);
        if lock.0 == 0 {
            lock.1 = Some(state);
        }
        lock.0 += 1;
    }
}

#[unsafe(no_mangle)]
extern "C" fn phy_i2c_exit_critical() {
    // SAFETY: as in `phy_i2c_enter_critical`; still inside it.
    unsafe {
        let lock = &mut *core::ptr::addr_of_mut!(PHY_I2C_LOCK);
        if lock.0 == 0 {
            return;
        }
        lock.0 -= 1;
        if lock.0 == 0
            && let Some(state) = lock.1.take()
        {
            critical_section::release(state);
        }
    }
}

/// The radio's buffers, fewer than the driver's defaults (10 kept for
/// receiving, up to 32 more each way): each takes about 1.6 KB, and the
/// X4's memory is short while a book opens with the radio on (charging).
/// An e-reader's requests and transfers need no more.
fn controller_config() -> ControllerConfig {
    ControllerConfig::default()
        .with_static_rx_buf_num(STATIC_RX)
        .with_dynamic_rx_buf_num(DYNAMIC)
        .with_dynamic_tx_buf_num(DYNAMIC)
        // Below the receive buffers, as the driver requires.
        .with_rx_ba_win(STATIC_RX - 1)
        // Aggregated sending: what PaperPaw sends is small (requests, a
        // reading position), and its sessions take memory.
        .with_ampdu_tx_enable(false)
}

/// Receive buffers kept while the radio is on.
const STATIC_RX: u8 = 4;
/// Buffers taken as frames come and go, at most, each way: a connection's
/// window (8 KB, `http`) in frames, and some.
const DYNAMIC: u16 = 16;
