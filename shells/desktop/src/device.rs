//! The emulated device: kernel + panel + clock, independent of any window.
//! Its storage is a folder ([`Library::Dir`]) or kept in memory
//! ([`Library::Memory`], e.g. in a browser).

use std::rc::Rc;
use std::time::{Duration, Instant as StdInstant};

use paperpaw_core::{Effects, Io, Kernel, KernelBuilder, KernelError, SpeechCall, Warning};
use paperpaw_graphics::Framebuffer;
use paperpaw_platform::{
    Battery, ButtonState, DeviceInfo, InputEvent, Instant, PixelFormat, RefreshMode, Rotation,
    SystemStatus, WallClock, buttons,
};

use crate::budget;
use crate::panel::Panel;
use crate::profiles::Profile;
use paperpaw_core::sdk::MemoryStorage;
use paperpaw_fs::DirStorage;
use paperpaw_platform::Storage;

mod card;
mod kernel;
mod sleep;
pub use card::{Card, Shared};
use kernel::build_kernel;

/// What the emulated device's storage says it has free (`--free-space`),
/// where set: screenshots that look the same on every computer.
static FREE_SPACE: std::sync::OnceLock<u64> = std::sync::OnceLock::new();

/// Makes the storage say it has `bytes` free, whatever the computer's
/// disk has.
pub fn set_free_space(bytes: u64) {
    let _ = FREE_SPACE.set(bytes);
}

/// Whether the emulated device loads code plugins built for this computer
/// (`native`): only where the one who starts it says so (`--trust-code`,
/// `trust_code` over `--stdio`), never because a library says it has
/// them installed (a library copied from elsewhere would run its code).
static CODE_PLUGINS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

static WIFI_ONLINE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Devices start online (`--wifi online`, before `--stdio` too), or with
/// the radio off as on a device (by default); see
/// [`Device::set_wifi_online`].
pub fn set_wifi_online(on: bool) {
    WIFI_ONLINE.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// Whether code plugins built for this computer load (`--trust-code`); read
/// by the native device host, which the web build does not have.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn code_plugins_on() -> bool {
    CODE_PLUGINS.load(std::sync::atomic::Ordering::Relaxed)
}

/// Loads code plugins built for this computer, or none (by default).
pub fn set_code_plugins(on: bool) {
    CODE_PLUGINS.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// The emulated device's storage in the folder `root`.
pub fn library(root: &std::path::Path) -> std::io::Result<DirStorage> {
    let storage = DirStorage::new(root)?;
    Ok(match FREE_SPACE.get() {
        Some(&bytes) => storage.with_free_space(bytes),
        None => storage,
    })
}

/// Where the emulated device keeps its files, as a card would: opened
/// anew for each kernel (at start and at every restart).
#[derive(Debug, Clone)]
pub enum Library {
    /// A folder of the computer.
    Dir(std::path::PathBuf),
    /// Files in memory, kept across restarts.
    Memory(Rc<MemoryStorage>),
    /// Any other storage of the caller's, kept across restarts.
    Shared(card::Shared),
}

impl Library {
    /// The storage for a kernel.
    pub fn open(&self) -> std::io::Result<Card> {
        Ok(match self {
            Library::Dir(root) => Card::Dir(library(root)?),
            Library::Memory(files) => Card::Memory(files.clone()),
            Library::Shared(files) => Card::Shared(files.clone()),
        })
    }
}

/// How long closing the window waits for answers to what plugins sent as
/// they saved (e.g. reading progress).
const SHUT_DOWN_WAIT: Duration = Duration::from_secs(5);

/// How long each kind of request is looked at in turn while both are in
/// flight ([`Device::poll_http`]).
const MIXED_SLICE: Duration = Duration::from_millis(20);

/// Adds plugins to the standard app, after its own: e.g. a plugin being
/// written, for `paperpaw-dev` (see [`Device::with_plugins`]). Called
/// again at each restart, as a device builds its kernel at each start.
pub type AddPlugins = fn(KernelBuilder) -> KernelBuilder;

/// A running device. All kernel calls go through here, so they are
/// counted against the profile's heap budget.
#[derive(Debug)]
pub struct Device {
    pub profile: &'static Profile,
    pub kernel: Kernel,
    /// What a restart needs to build the kernel again.
    user_config: Option<String>,
    library: Library,
    /// Plugins added to the standard app.
    add: Option<AddPlugins>,
    /// The panel controller's frame memory: the kernel writes changed rows
    /// into it, possibly band by band. Not part of the heap budget, as on
    /// the device.
    controller: Framebuffer,
    pub panel: Panel,
    /// The panel shows a frame in grays (see `Effects::grays`): the next
    /// change shows the whole screen again in black and white, as on the
    /// X4.
    gray_shown: bool,
    /// When the kernel wants to be ticked.
    pub wake_at: Option<Instant>,
    /// When the shell tells the kernel the time and the battery next: about
    /// once a minute while awake, as the X4's firmware does (`STATUS_MS`).
    /// It is also what delivers the minute tick where nothing else wakes
    /// the kernel.
    status_at: Option<Instant>,
    clock: Clock,
    /// The wall clock set (`set_time`): at this time it was so many
    /// milliseconds since 1970, in the time zone it had.
    wall: Option<(Instant, i64, Option<i32>)>,
    /// HTTP requests in flight.
    pub http: crate::http::Http,
    /// The requests plugins made (method and address), kept where someone
    /// asks (`--stdio`): none by default.
    pub requests: Option<Vec<(String, String)>>,
    /// Plugins' servers.
    pub servers: paperpaw_serve::Servers,
    pub sockets: paperpaw_serve::Sockets,
    /// The pretend radio, where the profile's device manages Wi-Fi.
    wifi: crate::wifi::Wifi,
    bluetooth: crate::bluetooth::Bluetooth,
    /// Sleeping (see `Effects::sleep`): only the power button wakes it.
    asleep: bool,
    /// Switched off (`Effects::power_off`): the power button starts it anew.
    powered_off: bool,
    /// The card was taken out while asleep: the wake tells the kernel
    /// (`sleep`).
    card_left_asleep: bool,
    /// How the screen is turned (see `Kernel::rotation`).
    rotation: Rotation,
    /// The texts being spoken (emulated), in order: number and when each
    /// ends.
    speech: std::collections::VecDeque<(u32, Instant)>,
    /// The kernel's warnings since start (the last `KEPT_WARNINGS`),
    /// besides printed: for tools that check there are none.
    pub warnings: Vec<Warning>,
    /// The last `KEPT_FRAMES` frames shown (region, mode), for tests of a start.
    pub shown: Vec<(paperpaw_platform::Rect, RefreshMode)>,
    /// How many frames were shown since start (scripts' `--repl` counts
    /// them, as the X4's log lines do).
    pub frames_shown: u64,
    /// Requests are answered from the emulator's fixtures
    /// ([`Device::use_fixtures`]).
    fixtures: bool,
    /// Online from each start, on USB ([`Device::set_wifi_online`]).
    wifi_online: bool,
    /// Interface-7 plugins the developer switch loaded from the card
    /// (`--abi7-dev-loader`): none by default. Kept, not applied (S2).
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    abi7: crate::native_runtime::DevPlugins,
    /// What the linked native plugins are registered with (the card's second
    /// handle, the clock and network `core.work` reads, written by
    /// [`Device::now`]): kept for every restart.
    linked: LinkedInputs,
}

/// What the registration of the linked native plugins is given besides the
/// profile (`linked`): none without the feature.
#[cfg(all(feature = "native-linked", target_os = "linux", target_arch = "x86_64"))]
type LinkedInputs = crate::linked::Inputs;
#[cfg(not(all(feature = "native-linked", target_os = "linux", target_arch = "x86_64")))]
type LinkedInputs = ();

/// The card's second handle for the secrets file and the clock `core.work`
/// reads.
#[cfg(all(feature = "native-linked", target_os = "linux", target_arch = "x86_64"))]
fn linked_inputs(library: &Library, simulated: bool) -> LinkedInputs {
    // A run on simulated time keeps its secrets in memory, so it cannot touch
    // the secrets of a card a real run shares; a real run whose card cannot
    // be opened has no secrets.
    let storage: Option<Rc<dyn Storage>> = if simulated {
        Some(Rc::new(MemoryStorage::new()))
    } else {
        library
            .open()
            .ok()
            .map(|card| Rc::new(card) as Rc<dyn Storage>)
    };
    crate::linked::Inputs {
        storage,
        view: Rc::default(),
        simulated,
        net: Rc::default(),
        card: library
            .open()
            .ok()
            .map(|card| Rc::new(card) as Rc<dyn Storage>),
        walk: paperpaw_core::LibraryWalk::default(),
    }
}
#[cfg(not(all(feature = "native-linked", target_os = "linux", target_arch = "x86_64")))]
fn linked_inputs(_: &Library, _: bool) -> LinkedInputs {}

/// How many frames [`Device::shown`] keeps.
/// How often the clock and battery are told to the kernel (the X4's `STATUS_MS`).
const STATUS_MS: u64 = 60_000;
const KEPT_FRAMES: usize = 64;

/// How many of the kernel's warnings [`Device::warnings`] keeps.
const KEPT_WARNINGS: usize = 1000;

/// Either real time (interactive) or simulated time (headless, tests).
#[derive(Debug)]
enum Clock {
    Real(StdInstant),
    Simulated(u64),
}

impl Device {
    pub fn new(
        profile: &'static Profile,
        user_config: Option<String>,
        storage: DirStorage,
        simulated_time: bool,
    ) -> Result<Self, KernelError> {
        Self::with_plugins(profile, user_config, storage, simulated_time, None)
    }

    /// A device whose app has the plugins `add` adds besides the standard
    /// app's.
    pub fn with_plugins(
        profile: &'static Profile,
        user_config: Option<String>,
        storage: DirStorage,
        simulated_time: bool,
        add: Option<AddPlugins>,
    ) -> Result<Self, KernelError> {
        let library = Library::Dir(storage.root().to_path_buf());
        let card = Card::Dir(storage);
        Self::build(profile, user_config, library, card, simulated_time, add)
    }

    /// A device whose files are kept in memory, `files` (e.g. in a
    /// browser), its app with the plugins `add` adds.
    pub fn in_memory(
        profile: &'static Profile,
        user_config: Option<String>,
        files: Rc<MemoryStorage>,
        simulated_time: bool,
        add: Option<AddPlugins>,
    ) -> Result<Self, KernelError> {
        let card = Card::Memory(files.clone());
        let library = Library::Memory(files);
        Self::build(profile, user_config, library, card, simulated_time, add)
    }

    /// A device whose card is `files`: a storage of the caller's (a FAT
    /// volume on a RAM disk that counts the commands it is sent, say).
    pub fn on_storage(
        profile: &'static Profile,
        files: Rc<dyn Storage>,
        simulated_time: bool,
    ) -> Result<Self, KernelError> {
        let shared = card::Shared(files);
        let card = Card::Shared(shared.clone());
        Self::build(
            profile,
            None,
            Library::Shared(shared),
            card,
            simulated_time,
            None,
        )
    }

    fn build(
        profile: &'static Profile,
        user_config: Option<String>,
        library: Library,
        card: Card,
        simulated_time: bool,
        add: Option<AddPlugins>,
    ) -> Result<Self, KernelError> {
        budget::set_limit(profile.heap_budget);
        paperpaw_core::native_host::answer_text_ink_as_the_x4(profile.ink_unwired);
        card::insert();
        let config = user_config.as_deref();
        #[cfg_attr(
            not(all(feature = "native-linked", target_os = "linux", target_arch = "x86_64")),
            allow(
                clippy::let_unit_value,
                reason = "nothing is linked without the feature"
            )
        )]
        let linked = linked_inputs(&library, simulated_time);
        let mut kernel = build_kernel(profile, config, card, Rotation::Deg0, add, &linked)?;
        // Turned by the settings: built again for the turned size.
        let rotation = kernel.rotation();
        if rotation != Rotation::Deg0
            && let Ok(card) = library.open()
        {
            drop(kernel);
            kernel = build_kernel(
                profile,
                user_config.as_deref(),
                card,
                rotation,
                add,
                &linked,
            )?;
        }
        let display = turned(profile.device, rotation).display;
        let controller = Framebuffer::new(display.width, display.height, display.format)
            .map_err(KernelError::Graphics)?;
        let mut device = Self {
            profile,
            kernel,
            user_config,
            library,
            add,
            controller,
            panel: Panel::new(display.width, display.height, display.color),
            gray_shown: false,
            wake_at: None,
            status_at: None,
            http: crate::http::Http::new(),
            requests: None,
            servers: servers(),
            sockets: paperpaw_serve::Sockets::new(|| {}),
            wifi: crate::wifi::Wifi::default(),
            bluetooth: crate::bluetooth::Bluetooth::default(),
            speech: std::collections::VecDeque::new(),
            warnings: Vec::new(),
            shown: Vec::new(),
            frames_shown: 0,
            fixtures: false,
            wifi_online: WIFI_ONLINE.load(std::sync::atomic::Ordering::Relaxed),
            #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
            abi7: crate::native_runtime::DevPlugins::default(),
            linked,
            asleep: false,
            powered_off: false,
            card_left_asleep: false,
            rotation,
            wall: None,
            clock: if simulated_time {
                Clock::Simulated(0)
            } else {
                Clock::Real(StdInstant::now())
            },
        };
        device.load_abi7();
        device.report_warnings();
        Ok(device)
    }

    /// Loads the interface-7 plugins of the card's folder, if the developer
    /// switch is on (none otherwise, and none for a card in memory).
    fn load_abi7(&mut self) {
        // With code plugins trusted the kernel's code loader takes the
        // interface-7 libraries (as flat plugins): not loaded twice.
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        if !CODE_PLUGINS.load(std::sync::atomic::Ordering::Relaxed)
            && let Library::Dir(root) = &self.library
        {
            self.abi7 = crate::native_runtime::DevPlugins::load_folder(root);
        }
    }

    /// The interface-7 plugins the developer switch loaded.
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    #[must_use]
    pub fn abi7_plugins(&self) -> &crate::native_runtime::DevPlugins {
        &self.abi7
    }

    /// Where the device keeps its files.
    pub fn library(&self) -> &Library {
        &self.library
    }

    pub fn now(&self) -> Instant {
        let now = match self.clock {
            Clock::Real(start) => Instant(start.elapsed().as_millis() as u64),
            Clock::Simulated(ms) => Instant(ms),
        };
        self.sync_view(now);
        now
    }

    /// What the native plugins' services read of the device (`DeviceView`):
    /// the time, and the network as the pretend radio reported it last.
    fn sync_view(&self, now: Instant) {
        #[cfg(all(feature = "native-linked", target_os = "linux", target_arch = "x86_64"))]
        {
            let view = &self.linked.view;
            view.set(now.0, !self.profile.wifi || self.wifi.network_up());
            view.set_hosting(self.wifi.is_hosting());
        }
        #[cfg(not(all(feature = "native-linked", target_os = "linux", target_arch = "x86_64")))]
        let _ = now;
    }

    /// Advances simulated time; no-op for real time.
    pub fn advance(&mut self, millis: u64) {
        if let Clock::Simulated(ms) = &mut self.clock {
            *ms += millis;
        }
    }

    pub fn start(&mut self) {
        let now = self.now();
        self.send_status(now);
        let effects = budget::track(|| self.kernel.start(now));
        self.execute(effects);
        if self.fixtures && self.profile.wifi {
            // Known as if joined before: joined when a plugin wants the
            // network, as a device of the reader's would.
            let keep = paperpaw_core::sdk::wifi::keep(crate::wifi::TEST_NETWORK, "fixtures");
            self.kernel.set_secret(keep);
            self.share_secrets_with_native();
        }
        if self.wifi_online && self.profile.wifi {
            self.go_online(now);
        }
    }

    /// Starts online from now on (`--wifi online`, the workshop's): where
    /// PaperPaw manages Wi-Fi, the device is on USB, its radio on and in
    /// the pretend radio's `Workshop Wi-Fi`, known as joined before, from
    /// each start; a plugin's request goes out at once. The `wifi`
    /// plugin keeps the radio on while charging (`wifi.while-charging`),
    /// and switching it off and on in the settings works as on a device.
    /// Elsewhere (a phone, a computer) the network is always there.
    pub fn set_wifi_online(&mut self, online: bool) {
        self.wifi_online = online;
    }

    /// A network saved as joined before, for the native Wi-Fi plugin (simulated time only).
    fn share_secrets_with_native(&self) {
        #[cfg(all(feature = "native-linked", target_os = "linux", target_arch = "x86_64"))]
        self.linked.share_secrets(self.kernel.storage());
    }

    fn go_online(&mut self, now: Instant) {
        let keep = paperpaw_core::sdk::wifi::keep(
            crate::wifi::WORKSHOP_NETWORK,
            crate::wifi::WORKSHOP_PASSWORD,
        );
        self.kernel.set_secret(keep);
        self.share_secrets_with_native();
        self.wifi.online(now);
        // On USB: what keeps the radio on, as on a device.
        self.send_status(now);
        self.tick_if_due();
    }

    /// Whether network work is under way that the emulated time brings to
    /// an end: requests not answered yet (waiting for the network or in
    /// flight), or the pretend radio starting, scanning or joining. With
    /// the fixtures (answered at once), a tool lets time pass until it
    /// ends, as a reader would wait.
    pub fn network_settling(&self) -> bool {
        self.http.in_flight > 0
            || self.native_in_flight() > 0
            || self.kernel.holds_sleep()
            || self.wifi.next_due().is_some()
            || self.bluetooth.settling()
    }

    /// Answers plugins' requests from the emulator's fixtures from now on
    /// ([`crate::offline`], on this thread; a route set already, such as
    /// a tool's network that fails on purpose, stays), and, where
    /// PaperPaw manages Wi-Fi, knows the pretend radio's test network from
    /// each start, so that what waits for the network gets it, offline.
    pub fn use_fixtures(&mut self) {
        if !crate::http::routed() {
            crate::http::set_route(Some(Box::new(crate::offline::Offline)));
        }
        self.fixtures = true;
    }

    /// Tells the kernel the clock and the battery now, and starts the minute
    /// to the next time it is told.
    fn send_status(&mut self, now: Instant) {
        let status = self.status(now);
        self.status_at = Some(now.add_millis(STATUS_MS));
        let effects = budget::track(|| self.kernel.set_status(now, status));
        self.execute(effects);
    }

    /// The emulated clock and battery: the computer's time and time zone
    /// (with simulated time a fixed one, so screenshots repeat) and a
    /// battery at 80 %, charging where it starts online.
    fn status(&self, now: Instant) -> SystemStatus {
        // The web build has no real clock to read at `now`.
        let _ = now;
        let clock = match (self.wall, &self.clock) {
            (Some((at, unix_millis, utc_offset_minutes)), _) => {
                WallClock::new(at, unix_millis, utc_offset_minutes)
            }
            #[cfg(not(target_arch = "wasm32"))]
            (None, Clock::Real(_)) => {
                let local = chrono::Local::now();
                WallClock::new(
                    now,
                    local.timestamp_millis(),
                    Some(local.offset().local_minus_utc() / 60),
                )
            }
            // 14 October 2026, 10:41 in Berlin at the start of the emulated
            // time (also in a browser, which drives the clock itself). One
            // pair for every status, so that the time goes on with the
            // emulated time and a status a minute later is no jump.
            _ => WallClock::new(Instant(0), 1_791_967_260_000, Some(120)),
        };
        SystemStatus::new(
            Some(clock),
            Some(Battery::new(
                80,
                self.wifi_online && self.profile.wifi,
                None,
            )),
        )
    }

    /// Sets the wall clock to `unix_millis` milliseconds since 1970 (UTC)
    /// now (the remote's `time set`); kept over restarts.
    pub fn set_time(&mut self, unix_millis: i64) {
        let now = self.now();
        // The time zone stays as it was.
        let offset = self.status(now).clock.and_then(|c| c.utc_offset_minutes);
        self.wall = Some((now, unix_millis, offset));
        self.send_status(now);
    }

    /// The wall clock now, in milliseconds since 1970 (UTC).
    pub fn unix_millis(&self) -> Option<i64> {
        let now = self.now();
        self.status(now).clock.map(|c| c.unix_millis_at(now))
    }

    /// Runs an action as a button bound to it would (scripts' `run:`).
    pub fn run_action(&mut self, action: paperpaw_core::sdk::Id) {
        let now = self.now();
        let effects = budget::track(|| self.kernel.run_action(now, action));
        self.execute(effects);
    }

    /// Opens a screen as a plugin would (scripts' `open:`).
    pub fn open_screen(
        &mut self,
        screen: paperpaw_core::sdk::Id,
        params: paperpaw_core::sdk::Params,
    ) {
        let now = self.now();
        let effects = budget::track(|| {
            // The keyboard asks a question (a native one cannot open without
            // it): the step's `title` and `secret` are the question's.
            if self.kernel.keyboard_screen() == Some(&screen) {
                let title = params.get("title").unwrap_or_default();
                let secret = params.get("secret").is_some_and(|value| value == "true");
                if let Ok(effects) = self.kernel.stage_text_question(now, title, secret) {
                    return effects;
                }
            }
            self.kernel.open_screen(now, screen, params)
        });
        self.execute(effects);
    }

    /// Makes a call of the kernel's at the emulated time, counted against
    /// the heap budget, and does what it asks (draws, ticks, requests):
    /// for tools, e.g. `|kernel, now| kernel.set_setting(now, id, value)`.
    pub fn call(&mut self, call: impl FnOnce(&mut Kernel, Instant) -> Effects) {
        let now = self.now();
        let kernel = &mut self.kernel;
        let effects = budget::track(|| call(kernel, now));
        self.execute(effects);
    }

    /// A line from the computer at the cable (`--cable`): the cable
    /// transfer's requests (see `paperpaw_core::Kernel::cable_line`); the
    /// answers wait in [`Device::take_cable_lines`].
    pub fn cable_line(&mut self, line: &str) {
        let now = self.now();
        let kernel = &mut self.kernel;
        let effects = budget::track(|| kernel.cable_line(now, line));
        self.execute(effects);
    }

    /// The cable transfer's lines for the computer, in order.
    pub fn take_cable_lines(&mut self) -> Vec<String> {
        self.kernel.take_cable_lines()
    }

    /// Types `text` into the text field shown, then confirms it with
    /// `done` (scripts' `type`, the remote's): whether one was shown.
    pub fn type_text(&mut self, text: &str, done: bool) -> bool {
        let now = self.now();
        let kernel = &mut self.kernel;
        let Some(effects) = budget::track(|| kernel.type_text(now, text, done)) else {
            return false;
        };
        self.execute(effects);
        true
    }

    /// A phone joins the device's own network, where it has one open
    /// (scripts' `phone-joins`): the pretend radio reports it at once.
    pub fn phone_joins(&mut self) {
        let now = self.now();
        self.wifi.phone_joins(now);
        self.tick_if_due();
    }

    /// The pretend Bluetooth remote's key `name` pressed and let go, where
    /// its keys go to the key maps (scripts' `ble-key`).
    pub fn ble_key(&mut self, name: &str) -> bool {
        let now = self.now();
        let pressed = self.bluetooth.press(name, now);
        self.tick_if_due();
        pressed
    }

    /// Draws the whole screen again, nothing on it changed (scripts'
    /// `redraw`).
    pub fn draw_again(&mut self) {
        let now = self.now();
        let effects = budget::track(|| self.kernel.draw_again(now));
        self.execute(effects);
    }

    pub fn input(&mut self, event: InputEvent) {
        let now = self.now();
        if self.asleep {
            // Asleep, only the power button wakes the device.
            if let InputEvent::Button {
                name: buttons::POWER,
                state: ButtonState::Pressed,
            } = event
            {
                self.wake();
            }
            return;
        }
        let effects = budget::track(|| self.kernel.handle(now, event));
        self.execute(effects);
    }

    /// Delivers HTTP answers that have arrived; with `wait`, waits that
    /// long for the requests still in flight.
    pub fn poll_http(&mut self, wait: Option<std::time::Duration>) {
        // One deadline for both kinds of request: where both are in flight,
        // each is looked at in turn for a short slice, so that neither holds
        // the other's answers up for the whole wait.
        let end = wait.map(|wait| StdInstant::now() + wait);
        loop {
            let left = end.map(|end| end.saturating_duration_since(StdInstant::now()));
            let mixed = self.native_in_flight() > 0 && self.http.in_flight > 0;
            let slice = left.map(|left| if mixed { left.min(MIXED_SLICE) } else { left });
            let before = self.native_in_flight();
            self.poll_native_net(if mixed { None } else { slice });
            let mut delivered = self.native_in_flight() < before;
            while let Some((id, received, total)) = self.http.next_progress() {
                let now = self.now();
                let effects = budget::track(|| self.kernel.http_progress(now, id, received, total));
                self.execute(effects);
            }
            while self.http.in_flight > 0 {
                let Some(response) = self.http.next(slice) else {
                    break;
                };
                delivered = true;
                let now = self.now();
                let effects = budget::track(|| self.kernel.http_response(now, response));
                self.execute(effects);
            }
            if !mixed || delivered || left.is_none_or(|left| left <= MIXED_SLICE) {
                break;
            }
        }
    }

    /// Hands the native plugins' HTTP answers to core (`core.net`), and lets
    /// it take its turn.
    #[cfg(all(feature = "native-linked", target_os = "linux", target_arch = "x86_64"))]
    fn poll_native_net(&mut self, wait: Option<std::time::Duration>) {
        // A request that waits for a network does not bring the emulated
        // radio up: the linked Wi-Fi plugin hears of it
        // (`event.network_wanted`) and decides, as on the X4.
        let told = {
            let net = self.linked.net.borrow();
            net.as_ref().is_some_and(|net| {
                let up = !self.profile.wifi || self.wifi.is_online();
                let told = net.transport.pump(&net.reports, wait);
                let sockets = net
                    .sockets
                    .as_ref()
                    .is_some_and(|(sockets, reports)| sockets.pump(reports, up));
                told | sockets
            })
        };
        // What core was told reaches the plugins in a kernel pass (its
        // service pump): one now, as the X4's shell does after its reports.
        // Without it an answer waits for the kernel's next wake, which a
        // screen that only waits for that answer never asks for (#2795).
        if told {
            let now = self.now();
            let effects = budget::track(|| self.kernel.tick(now));
            self.execute(effects);
        }
    }

    #[cfg(not(all(feature = "native-linked", target_os = "linux", target_arch = "x86_64")))]
    fn poll_native_net(&mut self, _wait: Option<std::time::Duration>) {}

    /// Whether the native plugins' servers run (the loop looks again soon).
    #[must_use]
    pub fn native_serving(&self) -> bool {
        #[cfg(all(feature = "native-linked", target_os = "linux", target_arch = "x86_64"))]
        return self
            .linked
            .net
            .borrow()
            .as_ref()
            .is_some_and(|net| net.transport.serving());
        #[cfg(not(all(feature = "native-linked", target_os = "linux", target_arch = "x86_64")))]
        false
    }

    /// The native plugins' requests started and not answered yet.
    #[cfg(all(feature = "native-linked", target_os = "linux", target_arch = "x86_64"))]
    fn native_in_flight(&self) -> usize {
        self.linked
            .net
            .borrow()
            .as_ref()
            .map_or(0, |net| net.transport.in_flight())
    }

    #[cfg(not(all(feature = "native-linked", target_os = "linux", target_arch = "x86_64")))]
    fn native_in_flight(&self) -> usize {
        0
    }

    /// Whether a native plugin has a socket open (`core.sockets`): its
    /// threads work in real time, so a script's `wait` gives them real time.
    #[cfg(all(feature = "native-linked", target_os = "linux", target_arch = "x86_64"))]
    pub fn native_sockets_open(&self) -> bool {
        self.linked.net.borrow().as_ref().is_some_and(|net| {
            net.sockets
                .as_ref()
                .is_some_and(|(sockets, _)| sockets.open() > 0)
        })
    }

    #[cfg(not(all(feature = "native-linked", target_os = "linux", target_arch = "x86_64")))]
    pub fn native_sockets_open(&self) -> bool {
        false
    }

    /// Passes requests to plugins' servers on, and what their sockets
    /// brought.
    pub fn poll_servers(&mut self) {
        while let Some(mut event) = self.servers.next() {
            // In the device's own network at the device's address there.
            if let paperpaw_core::ServerEvent::Started { address, .. } = &mut event {
                *address = self.wifi.served_at(address);
            }
            let now = self.now();
            let effects = budget::track(|| self.kernel.server_event(now, event));
            self.execute(effects);
        }
        while let Some(event) = self.sockets.take_event() {
            let now = self.now();
            let effects = budget::track(|| self.kernel.socket_event(now, event));
            self.execute(effects);
        }
        let kernel = &self.kernel;
        self.servers
            .pump(|path, at, buffer| budget::track(|| kernel.read_served(path, at, buffer).ok()));
    }

    /// When something is due next: a wake-up the kernel asked for, the end
    /// of speech, the pretend radio. Nothing while asleep: only the power
    /// button wakes the device then.
    pub fn next_wake(&self) -> Option<Instant> {
        if self.powered_off {
            return None;
        }
        if self.asleep {
            // The sleep screen shown again (`command.show_again`), as the X4's
            // light sleep wakes for it; nothing else wakes.
            return self.kernel.show_again_at();
        }
        let speech = self.speech.front().map(|&(_, end)| end);
        [
            self.wake_at,
            self.status_at,
            speech,
            self.wifi.next_due(),
            self.bluetooth.next_due(),
        ]
        .into_iter()
        .flatten()
        .min()
    }

    /// Ticks the kernel if a requested wake-up is due.
    pub fn tick_if_due(&mut self) {
        if self.powered_off {
            return;
        }
        if self.asleep {
            let now = self.now();
            if let Some(effects) = budget::track(|| self.kernel.show_again_now(now)) {
                eprintln!("{}: the sleep screen again", self.profile.device.id);
                self.execute(effects);
            }
            return;
        }
        self.poll_http(None);
        self.poll_servers();
        let now = self.now();
        while let Some(event) = self.wifi.take_due(now) {
            // The network as this event leaves it, before the kernel hears
            // it: its plugins answer at once (a server for the device's own
            // network), as the X4's bridge takes the radio's state first
            // (`Bridge::track`, #467).
            self.sync_view(now);
            // The networks found, for `core.device` op 9, before the kernel
            // tells the plugins of the scan: they read them when they hear it.
            #[cfg(all(feature = "native-linked", target_os = "linux", target_arch = "x86_64"))]
            if let paperpaw_core::WifiEvent::Scanned(found) = &event {
                let rows = found.iter();
                let rows = rows.map(|n| (n.ssid.as_str(), i32::from(n.signal), n.secured));
                self.linked.view.networks.set(rows);
            }
            let effects = budget::track(|| self.kernel.wifi_event(now, event));
            self.execute(effects);
        }
        while let Some(report) = self.bluetooth.take_due(now) {
            let effects = budget::track(|| self.kernel.bluetooth_event(now, report));
            self.execute(effects);
        }
        while let Some(&(id, end)) = self.speech.front()
            && end <= now
        {
            self.speech.pop_front();
            let effects = budget::track(|| self.kernel.speech_done(now, id, true));
            self.execute(effects);
        }
        if self.status_at.is_some_and(|at| at <= now) {
            self.send_status(now);
        }
        if self.wake_at.is_some_and(|at| at <= now) {
            let effects = budget::track(|| self.kernel.tick(now));
            self.execute(effects);
        }
    }

    /// Before the window closes: plugins save and send what is pending, as
    /// when the device goes to sleep, and answers to what they sent are
    /// waited for a little.
    pub fn shut_down(&mut self) {
        if !self.asleep {
            let now = self.now();
            let effects = budget::track(|| self.kernel.suspend(now));
            self.execute(effects);
        }
        let end = StdInstant::now() + SHUT_DOWN_WAIT;
        // Both kinds of answer, for as long as they come, to one deadline.
        while (self.native_in_flight() > 0 || self.http.in_flight > 0) && StdInstant::now() < end {
            self.poll_http(Some(end.saturating_duration_since(StdInstant::now())));
        }
    }

    /// Shows the whole screen in four grays, as the X4 does (see
    /// `Effects::grays`), and tells the kernel the frame for `area` was
    /// shown in grays (as the firmware's `show_gray`); where there is no
    /// memory for it, as it was.
    fn show_gray(&mut self, area: paperpaw_platform::Rect) {
        let started = crate::host::micros();
        let screen = self.controller.bounds();
        let Ok(mut gray) = Framebuffer::new(screen.width, screen.height, PixelFormat::Gray2) else {
            return;
        };
        let kernel = &mut self.kernel;
        let mut frame = crate::trace::Frame::start();
        let drawn = budget::track(|| {
            kernel.render_gray(&mut |top, rows| {
                gray.write_rows(top, rows);
                if let Some(frame) = &mut frame {
                    frame.band();
                }
            })
        });
        if let Some(frame) = frame {
            let screen = self.kernel.active_screen().map(ToString::to_string);
            frame.done(screen.as_deref().unwrap_or_default());
        }
        if !drawn {
            return;
        }
        self.panel
            .refresh(gray.view(), screen, RefreshMode::Full, None);
        self.gray_shown = true;
        self.note_shown(area, RefreshMode::Gray, started);
    }

    /// Whether `name` is a button of this device.
    pub fn has_button(&self, name: &str) -> bool {
        self.profile.buttons.contains(&name)
    }

    /// Whether the device sleeps: only the power button wakes it.
    pub fn asleep(&self) -> bool {
        self.asleep
    }

    /// Draws what changed and shows it, in `region` with `mode`; the
    /// frame's times go to the kernel.
    fn show_frame(&mut self, region: paperpaw_platform::Rect, mode: RefreshMode) {
        let started = crate::host::micros();
        let (kernel, controller) = (&mut self.kernel, &mut self.controller);
        let mut frame = crate::trace::Frame::start();
        budget::track(|| {
            kernel.render(&mut |top, rows| {
                controller.write_rows(top, rows);
                if let Some(frame) = &mut frame {
                    frame.band();
                }
            });
        });
        if let Some(frame) = frame {
            let screen = self.kernel.active_screen().map(ToString::to_string);
            frame.done(screen.as_deref().unwrap_or_default());
        }
        let colors = self.profile.colors.then(|| self.kernel.colors());
        self.panel
            .refresh(self.controller.view(), region, mode, colors);
        self.note_shown(region, mode, started);
    }

    /// A frame drawn since `started` was shown in `region` with `mode`:
    /// kept for the tests, counted, and its times told to the kernel.
    fn note_shown(&mut self, region: paperpaw_platform::Rect, mode: RefreshMode, started: u64) {
        if self.shown.len() >= KEPT_FRAMES {
            self.shown.remove(0);
        }
        self.shown.push((region, mode));
        self.frames_shown += 1;
        // Shown as soon as drawn: plugins that pace themselves by the
        // panel go as fast as they can here.
        let drawn = core::time::Duration::from_micros(crate::host::micros() - started);
        let now = self.now();
        self.kernel
            .frame_shown(region, mode, drawn, core::time::Duration::ZERO, now);
    }

    fn execute(&mut self, effects: Effects) {
        if effects.restart() {
            return self.restart();
        }
        // Pictures in grays, as the X4 shows them: the whole screen, small
        // changes over it waiting; after them, the whole screen again in
        // black and white.
        let grays = effects.grays() && self.controller.format() == PixelFormat::Mono1;
        if let Some((area, _)) = effects.refresh().filter(|_| grays) {
            let screen = self.controller.bounds();
            let small = u32::from(area.width) * u32::from(area.height) * 4
                < u32::from(screen.width) * u32::from(screen.height);
            if !(self.gray_shown && small) {
                self.show_gray(area);
            }
        } else if self.gray_shown && effects.refresh().is_some() {
            self.gray_shown = false;
            let now = self.now();
            let again = budget::track(|| self.kernel.draw_again(now));
            return self.execute(effects.and(again));
        } else if let Some((region, mode)) = effects.refresh() {
            self.show_frame(region, mode);
        }
        // Slow work ahead: its mark shows first, as on the X4 (whose panel
        // refreshes it while the kernel works on), then what the work
        // brought.
        if effects.refresh().is_some() && effects.proceed() {
            let now = self.now();
            let effects = budget::track(|| self.kernel.proceed(now));
            return self.execute(effects);
        }
        self.wake_at = effects.wake_at();
        if (effects.sleep() || effects.power_off()) && !self.asleep {
            self.fall_asleep(effects.power_off());
        }
        // The card shown to a computer: the emulator has no USB, as a device
        // with no cable in, which says so at once (`firmware/usb_drive.rs`).
        if effects.usb_drive() {
            let now = self.now();
            let effects = budget::track(|| self.kernel.usb_drive_refused(now));
            self.execute(effects);
        }
        self.run_io();
        self.report_warnings();
        // Turned: the same screens, made anew for the turned size.
        if self.kernel.rotation() != self.rotation {
            self.rotation = self.kernel.rotation();
            let display = turned(self.profile.device, self.rotation).display;
            if let Ok(controller) = Framebuffer::new(display.width, display.height, display.format)
            {
                self.controller = controller;
                self.panel = Panel::new(display.width, display.height, display.color);
                let now = self.now();
                let effects =
                    budget::track(|| self.kernel.resize(now, display.width, display.height));
                self.execute(effects);
            }
        }
    }

    /// Does the kernel's input and output: requests, servers, sockets,
    /// speech, the radios, the clipboard.
    fn run_io(&mut self) {
        for io in self.kernel.take_io() {
            match io {
                Io::Http(mut call) => {
                    if let Some(requests) = &mut self.requests {
                        requests.push((call.method.clone(), call.url.clone()));
                    }
                    if let Err(e) = call.load_body(self.kernel.storage()) {
                        let now = self.now();
                        let error = paperpaw_core::sdk::HttpError::Io;
                        let failed =
                            paperpaw_core::HttpResponse::failed(call.id, error, &e.to_string());
                        let effects = budget::track(|| self.kernel.http_response(now, failed));
                        self.execute(effects);
                        continue;
                    }
                    self.http.start(call);
                }
                Io::Server(call) => self.servers.apply(call),
                Io::Socket(call) => self.sockets.apply(call),
                Io::Speech(call) => self.speak(call),
                Io::Wifi(call) => {
                    let now = self.now();
                    self.wifi.apply(call, now);
                }
                Io::Bluetooth(call) => {
                    let now = self.now();
                    self.bluetooth.apply(call, now);
                }
                // The emulator's clipboard is the kernel's own: nothing to
                // copy to, and it is not asked to read one.
                Io::Copy(_) => {}
                Io::ReadClipboard => {
                    let now = self.now();
                    let effects = budget::track(|| self.kernel.clipboard_text(now, None));
                    self.execute(effects);
                }
                // The emulator is no device's firmware: it is not asked.
                Io::Firmware(_) => {}
            }
        }
    }

    /// Speech, emulated: the text is logged and takes as long as it would
    /// to speak, at about 160 words a minute.
    fn speak(&mut self, call: SpeechCall) {
        match call {
            SpeechCall::Say {
                id,
                text,
                language,
                rate,
                queue,
            } => {
                if !queue {
                    self.speech.clear();
                }
                let words = text.split_whitespace().count() as u64;
                let millis = words * 375 * 100 / u64::from(rate.max(1));
                let start: String = text.chars().take(60).collect();
                let queued = if queue { ", queued" } else { "" };
                eprintln!("speech ({language}, {rate} %, {millis} ms{queued}): {start}");
                let after = self.speech.back().map_or(self.now(), |&(_, end)| end);
                self.speech
                    .push_back((id, after.max(self.now()).add_millis(millis)));
            }
            SpeechCall::Stop => {
                eprintln!("speech stopped");
                self.speech.clear();
            }
        }
    }

    /// Builds a new kernel, as the device would after a reboot: also
    /// after its files changed, e.g. a data plugin's.
    pub fn restart(&mut self) {
        self.start_again(true);
    }

    /// Builds the kernel anew, as at a start of the device: after the old
    /// one's `stop` (`stop`, a restart), or after a switch-off (its plugins
    /// heard `core.suspend` then).
    fn start_again(&mut self, stop: bool) {
        let storage = match self.library.open() {
            Ok(storage) => storage,
            Err(e) => return eprintln!("restart: {:?}: {e}", self.library),
        };
        eprintln!("{}: restarting", self.profile.device.id);
        // Plugins save what is pending, as on the device before it restarts.
        if stop {
            let now = self.now();
            budget::track(|| self.kernel.stop(now));
        }
        // Interface-7 plugins of the switch are released with the old kernel
        // and loaded again below (a changed library is read again).
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        self.abi7.unload();
        // As after a reboot, what the old kernel asked for is gone: its
        // requests' answers would otherwise reach the new kernel's (they
        // count from 1 again).
        self.http = crate::http::Http::new();
        self.servers = servers();
        self.sockets = paperpaw_serve::Sockets::new(|| {});
        self.wifi = crate::wifi::Wifi::default();
        self.bluetooth = crate::bluetooth::Bluetooth::default();
        self.speech.clear();
        // The old kernel goes first, as on the device: a minimal one (one
        // frame row, no plugins) holds its place meanwhile.
        let profile = self.profile;
        let kernel = &mut self.kernel;
        let swapped = budget::track(|| {
            let placeholder = Kernel::builder(profile.device).frame_rows(1).build()?;
            drop(std::mem::replace(kernel, placeholder));
            Ok::<(), KernelError>(())
        });
        if let Err(e) = swapped {
            return eprintln!("restart: {e}");
        }
        match build_kernel(
            self.profile,
            self.user_config.as_deref(),
            storage,
            self.rotation,
            self.add,
            &self.linked,
        ) {
            Ok(new) => {
                let kernel = &mut self.kernel;
                budget::track(|| drop(std::mem::replace(kernel, new)));
                let display = turned(self.profile.device, self.rotation).display;
                if (display.width, display.height) != (self.panel.width(), self.panel.height()) {
                    if let Ok(controller) =
                        Framebuffer::new(display.width, display.height, display.format)
                    {
                        self.controller = controller;
                    }
                    self.panel = Panel::new(display.width, display.height, display.color);
                }
                self.load_abi7();
                self.report_warnings();
                self.start();
            }
            Err(e) => eprintln!("restart: {e}"),
        }
    }

    fn report_warnings(&mut self) {
        let kernel = &mut self.kernel;
        let warnings = budget::track(|| kernel.take_warnings());
        for warning in warnings {
            eprintln!("warning: {warning}");
            if self.warnings.len() == KEPT_WARNINGS {
                self.warnings.remove(0);
            }
            // Copied outside the kernel's budget, which the kernel's own
            // copy leaves as it is dropped.
            self.warnings.push(warning.clone());
        }
    }
}

/// `device` with its screen turned by `rotation`: on its side, width and
/// height trade places.
fn turned(mut device: DeviceInfo, rotation: Rotation) -> DeviceInfo {
    if matches!(rotation, Rotation::Deg90 | Rotation::Deg270) {
        let display = &mut device.display;
        (display.width, display.height) = (display.height, display.width);
    }
    device
}

/// Plugins' servers, on this computer only: the emulator is for trying
/// and testing, and so asks no firewall for the network; a browser on the
/// same computer reaches them at 127.0.0.1.
fn servers() -> paperpaw_serve::Servers {
    paperpaw_serve::Servers::new(|| {}).listening_on(std::net::Ipv4Addr::LOCALHOST.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_core::WifiCommand;
    use paperpaw_platform::StorageError;

    fn device(name: &str) -> (Device, std::path::PathBuf) {
        let root =
            std::env::temp_dir().join(format!("paperpaw-device-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let storage = DirStorage::new(&root).unwrap();
        let profile = crate::profiles::find("xteink-x4").unwrap();
        let mut device = Device::new(profile, None, storage, true).unwrap();
        device.start();
        (device, root)
    }

    /// A run on simulated time keeps its secrets file in memory: the card's
    /// own file is byte for byte what it was; a real run whose card cannot be
    /// opened has no store at all (`core.secrets` is not wired).
    #[cfg(all(feature = "native-linked", target_os = "linux", target_arch = "x86_64"))]
    #[test]
    fn a_simulated_run_leaves_the_cards_secrets_alone_and_an_unopenable_card_has_none() {
        let root =
            std::env::temp_dir().join(format!("paperpaw-secrets-card-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".paperpaw")).unwrap();
        let file = root.join(".paperpaw/secrets.txt");
        std::fs::write(&file, b"mail.password\t\t\ts1:00aa\n").unwrap();
        let before = std::fs::read(&file).unwrap();
        let library = Library::Dir(root.clone());
        let inputs = linked_inputs(&library, true);
        let storage = inputs.storage.unwrap();
        storage.write(".paperpaw/secrets.txt", b"lost").unwrap();
        storage.write(".paperpaw/secrets.new", b"temp").unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), before);
        assert!(!root.join(".paperpaw/secrets.new").exists());
        // A real run reads and writes the card's file.
        let real = linked_inputs(&library, false).storage.unwrap();
        let mut buf = [0u8; 64];
        let n = real.read_at(".paperpaw/secrets.txt", 0, &mut buf).unwrap();
        assert_eq!(&buf[..n], &before[..]);
        // A card that cannot be opened: no store.
        let missing = Library::Dir(file.clone());
        assert!(linked_inputs(&missing, false).storage.is_none());
        let _ = std::fs::remove_dir_all(root);
    }

    /// A wanted network does not bring the emulated Wi-Fi up by itself: as on
    /// the X4 the radio is the user's and the Wi-Fi plugin's, which core tells
    /// of the demand (this test used to hold the emulator's own shortcut, which
    /// joined a made-up network the device would not have).
    #[cfg(all(feature = "native-linked", target_os = "linux", target_arch = "x86_64"))]
    #[test]
    fn a_wanted_network_leaves_the_radio_to_the_user() {
        use paperpaw_core::native_host::shell_wiring::ShellNetwork as _;
        let _one = crate::one_device_at_a_time();
        let (mut device, root) = device("wifi-demand");
        let demand = |device: &Device, wanted| {
            let net = device.linked.net.borrow();
            net.as_ref().unwrap().transport.network_demand(wanted);
        };
        assert!(device.linked.net.borrow().is_some(), "core.net is wired");
        assert!(
            device
                .linked
                .net
                .borrow()
                .as_ref()
                .unwrap()
                .sockets
                .is_some(),
            "core.sockets is wired with it"
        );
        assert!(device.profile.wifi && !device.wifi.is_online());
        // Nothing wanted: the radio stays as it was.
        device.poll_http(None);
        assert!(!device.wifi.is_online());
        demand(&device, true);
        device.poll_http(None);
        device.tick_if_due();
        assert!(!device.wifi.is_online(), "the shell brought no network up");
        demand(&device, false);
        let _ = std::fs::remove_dir_all(root);
    }

    /// As on the X4: a sleep leaves the radios off, which the kernel hears
    /// once awake; a switch-off leaves the sleep's mark and ends the run, and
    /// the power button starts it anew, the new kernel taking the mark.
    #[test]
    fn a_sleep_leaves_the_radios_off_and_a_switch_off_starts_anew() {
        let _one = crate::one_device_at_a_time();
        let (mut device, root) = device("switch-off");
        let now = device.now();
        device.go_online(now);
        assert!(device.wifi.is_online());
        device.fall_asleep(false);
        device.wake();
        device.tick_if_due();
        assert!(!device.wifi.is_online(), "the sleep left the radio");
        device.fall_asleep(true);
        assert!(root.join(".paperpaw/slept").exists(), "the sleep's mark");
        assert_eq!(device.next_wake(), None, "switched off: nothing is due");
        let frames = device.frames_shown;
        device.input(InputEvent::Button {
            name: buttons::POWER,
            state: ButtonState::Pressed,
        });
        assert!(!device.asleep() && !device.powered_off);
        assert!(device.frames_shown > frames, "started anew");
        assert!(!root.join(".paperpaw/slept").exists(), "the start took it");
        drop(device);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn asleep_nothing_is_due() {
        let _one = crate::one_device_at_a_time();
        let (mut device, root) = device("asleep");
        device.wake_at = Some(Instant(0));
        device.asleep = true;
        // A wake-up the kernel asked for before would be due at once, for
        // ever: the window's loop would spin.
        assert_eq!(device.next_wake(), None);
        device.asleep = false;
        assert_eq!(device.next_wake(), Some(Instant(0)));
        drop(device);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_restart_forgets_what_the_old_kernel_asked_for() {
        let _one = crate::one_device_at_a_time();
        let (mut device, root) = device("restart");
        let now = device.now();
        device.speech.push_back((1, now.add_millis(10_000)));
        device.wifi.apply(WifiCommand::On, now);
        device.restart();
        assert!(device.speech.is_empty());
        // The new kernel's radio starts off: nothing of the old is due.
        assert_eq!(device.wifi.next_due(), None);
        drop(device);
        let _ = std::fs::remove_dir_all(root);
    }

    /// The library's moves tell a book by its identity: a card in a
    /// folder of the computer passes on its folder's (#228).
    #[cfg(unix)]
    #[test]
    fn a_card_in_a_folder_passes_on_identity() {
        let root = std::env::temp_dir().join(format!("paperpaw-card-id-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let card = Card::Dir(DirStorage::new(&root).unwrap());
        card.write("a.txt", b"x").unwrap();
        let before = card.entry_identity("a.txt").unwrap();
        assert!(before.is_some());
        card.rename("a.txt", "b.txt").unwrap();
        assert_eq!(card.entry_identity("b.txt").unwrap(), before);
        assert_eq!(card.entry_identity("a.txt"), Err(StorageError::NotFound));
        card.write("c.txt", b"y").unwrap();
        assert_ne!(card.entry_identity("c.txt").unwrap(), before);
        let _ = std::fs::remove_dir_all(root);
    }
}
