//! The linked interface-7 plugins on Android (Decision 172, todo 587): the
//! shared registration (`paperpaw_shell_glue`) with what this shell gives it:
//! the device behind `core.device`, the card, and `core.net` over the same
//! `HttpURLConnection` client the interface-6 requests use.
//!
//! Plugins that need `core.secrets` (a sealed key) are refused at their
//! admission until a Keystore-backed sealer is wired: nothing is stored in
//! the clear. Plugin code files are never loaded (`PluginJobs::Data`): the
//! store installs data plugins, as on the Boox emulator profile.

use std::rc::Rc;
use std::time::Duration;

use android_activity::AndroidApp;
use paperpaw_core::native_host::HostWiring;
use paperpaw_core::native_host::device_service::{DeviceFacts, DeviceHost, caps};
use paperpaw_core::native_host::shell_wiring::{NetInputs, NetLimits, NetReports};
use paperpaw_core::{HttpCall, HttpResponse, Kernel};
use paperpaw_platform::{DeviceInfo, MemoryClass, Storage};
use paperpaw_shell_glue::net::{NativeNet, Transport};
use paperpaw_shell_glue::view::DeviceView;
use paperpaw_shell_glue::{PluginJobs, Shell};

/// What the device says of itself behind `core.device`: its identity and the
/// parts the kernel was built with. Android's system manages Wi-Fi, so there
/// is no radio to show, and PaperPaw is not the firmware.
#[derive(Debug)]
struct AndroidDevice {
    caps: u64,
    class: u32,
}

impl AndroidDevice {
    fn new(device: &DeviceInfo, color: bool, frontlight: bool) -> Self {
        let bit = |on: bool, bit: u64| if on { bit } else { 0 };
        Self {
            // As the kernel's builder is set in `app::start_kernel`: network,
            // rotation, speech, the system clipboard and colours.
            caps: caps::NETWORK
                | caps::ROTATION
                | caps::SPEECH
                | caps::SYSTEM_CLIPBOARD
                | bit(device.input.touch, caps::TOUCH)
                | bit(color, caps::COLOURS)
                | bit(frontlight, caps::FRONTLIGHT),
            class: match device.memory {
                MemoryClass::Small => 0,
                _ => 2,
            },
        }
    }
}

impl DeviceHost for AndroidDevice {
    fn device(&self, visit: &mut dyn FnMut(&DeviceFacts<'_>)) -> Result<(), i32> {
        visit(&DeviceFacts {
            id: "android",
            model: "Android e-reader",
            catalog: "android",
            class: self.class,
            caps: self.caps,
            controls: 0,
            tls_sessions: u32::from(paperpaw_serve::TLS_SESSIONS),
            start: 0,
            slept_ms: None,
        });
        Ok(())
    }
}

/// The HTTP client of `core.net`: the shell's own (`crate::net`), a thread
/// and a Java connection for each request, which wakes the main loop when its
/// answer is in.
struct Client {
    net: crate::net::Net,
    in_flight: usize,
}

impl Transport for Client {
    fn start_unfollowed(&mut self, call: HttpCall) {
        self.in_flight += 1;
        // Java is told not to follow redirects (`net::connect`): the shared
        // transport follows them by the rules of `network.md` 3.1.
        self.net.start(call);
    }

    fn answer_now(&mut self, response: HttpResponse) {
        self.in_flight += 1;
        self.net.answer(response);
    }

    fn next(&mut self, _wait: Option<Duration>) -> Option<HttpResponse> {
        // The main loop is woken by each answer: nothing is waited for.
        let response = self.net.next()?;
        self.in_flight = self.in_flight.saturating_sub(1);
        Some(response)
    }

    fn in_flight(&self) -> usize {
        self.in_flight
    }
}

/// The transport of `core.net` and core's handle for its reports, for the
/// main loop.
pub struct Native {
    net: Rc<NativeNet<Client>>,
    reports: NetReports,
    view: Rc<DeviceView>,
}

impl std::fmt::Debug for Native {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Native")
            .field("net", &self.net)
            .finish_non_exhaustive()
    }
}

impl Native {
    /// Hands core the answers that have come, tells it the clock and the
    /// network (the system's: always up here), and lets it take its turn.
    pub fn pump(&self) {
        self.view
            .set(paperpaw_shell_glue::clock::micros() / 1000, true);
        self.net.pump(&self.reports, None);
    }
}

/// Registers the app's linked plugins with `kernel`, before it starts: how
/// many were registered and the loop's handle on `core.net`. `card` is a
/// second handle on what the kernel reads (`None` where it cannot be
/// opened: no `core.net` then, nor the services that serve from the card).
/// `None` if the registration could not be made at all.
pub fn register(
    kernel: &mut Kernel,
    app: &AndroidApp,
    device: &DeviceInfo,
    card: Option<Rc<dyn Storage>>,
    color: bool,
    frontlight: bool,
) -> Option<Native> {
    let view = Rc::new(DeviceView::default());
    view.set(paperpaw_shell_glue::clock::micros() / 1000, true);
    let mut wiring = HostWiring::new();
    let net = card.clone().map(|storage| {
        let net = Rc::new(NativeNet::new(
            Client {
                net: crate::net::Net::new(app),
                in_flight: 0,
            },
            storage.clone(),
            view.clone(),
        ));
        wiring = std::mem::take(&mut wiring).net(NetInputs::new(
            storage,
            net.clone(),
            net.clone(),
            // As the other shells' (`SockLimits::large`): one pool, 4 TLS
            // sessions.
            NetLimits::new(0, 8).tls_sessions(4),
        ));
        net
    });
    log::info!("native plugins: no secrets (no sealer wired yet)");
    let registered = paperpaw_shell_glue::register(
        kernel,
        Shell {
            memory: device.memory,
            widest: u32::from(device.display.width.max(device.display.height)),
            device: Rc::new(AndroidDevice::new(device, color, frontlight)),
            view: view.clone(),
            card,
            simulated: false,
            wifi: false,
            jobs: PluginJobs::Data,
            wiring,
        },
    );
    let registered = match registered {
        Ok(registered) => registered,
        Err(why) => {
            log::error!("native plugins: not registered: {why:?}");
            return None;
        }
    };
    log::info!("native plugins: {} registered", registered.count);
    let reports = registered.wired.net().cloned()?;
    Some(Native {
        net: net?,
        reports,
        view,
    })
}
