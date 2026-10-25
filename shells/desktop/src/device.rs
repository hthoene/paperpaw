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
use paperpaw_platform::{DirEntry, Storage, StorageError};

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
    /// A card image, mounted by PaperPaw's FAT or exFAT storage
    /// ([`crate::card_image`]).
    #[cfg(not(target_arch = "wasm32"))]
    Image(crate::card_image::ImageBlocks),
}

impl Library {
    /// The storage for a kernel.
    pub fn open(&self) -> std::io::Result<Card> {
        Ok(match self {
            Library::Dir(root) => Card::Dir(library(root)?),
            Library::Memory(files) => Card::Memory(files.clone()),
            // Mounted where its memory counts: the device's own, as on
            // the X4 (its blocks kept, the file system's state).
            #[cfg(not(target_arch = "wasm32"))]
            Library::Image(blocks) => Card::Image(Mounted(budget::track(|| {
                crate::card_image::mount(blocks.clone())
            })?)),
        })
    }
}

/// A [`Library`] opened: what the kernel reads and writes.
#[derive(Debug)]
pub enum Card {
    Dir(DirStorage),
    Memory(Rc<MemoryStorage>),
    Image(Mounted),
}

/// A card image's file system, mounted.
pub struct Mounted(Box<dyn Storage>);

impl core::fmt::Debug for Mounted {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("mounted card image")
    }
}

impl Card {
    /// `f` on the storage. The files of a card in memory are the card's,
    /// not the device's heap: what keeping them takes is not counted
    /// against the budget (a decoded picture would fill an X4's).
    fn on<R>(&self, f: impl FnOnce(&dyn Storage) -> R) -> R {
        match self {
            Card::Dir(storage) => f(storage),
            Card::Memory(files) => budget::untracked(|| f(&**files)),
            // The X4's own storage: what it keeps is counted.
            Card::Image(mounted) => f(mounted.0.as_ref()),
        }
    }
}

impl Storage for Card {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.on(|s| s.read_at(path, offset, buf))
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.on(|s| s.size(path))
    }
    fn modified(&self, path: &str) -> Option<i64> {
        self.on(|s| s.modified(path))
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.on(|s| s.write(path, data))
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.on(|s| s.append(path, data))
    }
    fn write_at(&self, path: &str, offset: u64, data: &[u8]) -> Result<(), StorageError> {
        self.on(|s| s.write_at(path, offset, data))
    }
    fn truncate(&self, path: &str, len: u64) -> Result<(), StorageError> {
        self.on(|s| s.truncate(path, len))
    }
    fn sync(&self) -> Result<(), StorageError> {
        self.on(|s| s.sync())
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        // What the visitor keeps is the kernel's: counted.
        match self {
            Card::Dir(storage) => storage.list(dir, visit),
            Card::Memory(files) => files.list(dir, visit),
            Card::Image(mounted) => mounted.0.list(dir, visit),
        }
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.on(|s| s.remove(path))
    }
    fn exists(&self, path: &str) -> bool {
        self.on(|s| s.exists(path))
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.on(|s| s.rename(from, to))
    }
    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        self.on(|s| s.create_dir(path))
    }
    fn remove_dir_all(&self, path: &str) -> Result<(), StorageError> {
        self.on(|s| s.remove_dir_all(path))
    }
    fn ignores_case(&self) -> bool {
        self.on(|s| s.ignores_case())
    }
    fn free_space(&self) -> Option<u64> {
        self.on(|s| s.free_space())
    }
    fn total_space(&self) -> Option<u64> {
        self.on(|s| s.total_space())
    }
    fn local_path(&self, path: &str, with: &mut dyn FnMut(&str)) {
        self.on(|s| s.local_path(path, with));
    }
}

/// How long closing the window waits for answers to what plugins sent as
/// they saved (e.g. reading progress).
const SHUT_DOWN_WAIT: Duration = Duration::from_secs(5);

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
    /// How the screen is turned (see `Kernel::rotation`).
    rotation: Rotation,
    /// The texts being spoken (emulated), in order: number and when each
    /// ends.
    speech: std::collections::VecDeque<(u32, Instant)>,
    /// The kernel's warnings since start (the last `KEPT_WARNINGS`),
    /// besides printed: for tools that check there are none.
    pub warnings: Vec<Warning>,
    /// Requests are answered from the emulator's fixtures
    /// ([`Device::use_fixtures`]).
    fixtures: bool,
    /// Online from each start, on USB ([`Device::set_wifi_online`]).
    wifi_online: bool,
}

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

    /// A device whose card is the card image `blocks` (a file, or bytes
    /// in memory for tests), mounted by PaperPaw's FAT or exFAT storage
    /// as the X4 mounts its card, its app with the plugins `add` adds.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn on_card_image(
        profile: &'static Profile,
        user_config: Option<String>,
        blocks: crate::card_image::ImageBlocks,
        simulated_time: bool,
        add: Option<AddPlugins>,
    ) -> Result<Self, String> {
        let library = Library::Image(blocks);
        let card = library.open().map_err(|e| format!("card image: {e}"))?;
        Self::build(profile, user_config, library, card, simulated_time, add)
            .map_err(|e| format!("{}: {e}", profile.device.id))
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

    fn build(
        profile: &'static Profile,
        user_config: Option<String>,
        library: Library,
        card: Card,
        simulated_time: bool,
        add: Option<AddPlugins>,
    ) -> Result<Self, KernelError> {
        budget::set_limit(profile.heap_budget);
        let config = user_config.as_deref();
        let mut kernel = build_kernel(profile, config, card, Rotation::Deg0, add)?;
        // Turned by the settings: built again for the turned size.
        let rotation = kernel.rotation();
        if rotation != Rotation::Deg0
            && let Ok(card) = library.open()
        {
            drop(kernel);
            kernel = build_kernel(profile, user_config.as_deref(), card, rotation, add)?;
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
            http: crate::http::Http::new(),
            requests: None,
            servers: servers(),
            sockets: paperpaw_serve::Sockets::new(|| {}),
            wifi: crate::wifi::Wifi::default(),
            bluetooth: crate::bluetooth::Bluetooth::default(),
            speech: std::collections::VecDeque::new(),
            warnings: Vec::new(),
            fixtures: false,
            wifi_online: WIFI_ONLINE.load(std::sync::atomic::Ordering::Relaxed),
            asleep: false,
            rotation,
            wall: None,
            clock: if simulated_time {
                Clock::Simulated(0)
            } else {
                Clock::Real(StdInstant::now())
            },
        };
        device.report_warnings();
        Ok(device)
    }

    /// Where the device keeps its files.
    pub fn library(&self) -> &Library {
        &self.library
    }

    pub fn now(&self) -> Instant {
        match self.clock {
            Clock::Real(start) => Instant(start.elapsed().as_millis() as u64),
            Clock::Simulated(ms) => Instant(ms),
        }
    }

    /// Advances simulated time; no-op for real time.
    pub fn advance(&mut self, millis: u64) {
        if let Clock::Simulated(ms) = &mut self.clock {
            *ms += millis;
        }
    }

    pub fn start(&mut self) {
        let now = self.now();
        let status = self.status(now);
        let effects = budget::track(|| self.kernel.set_status(now, status));
        self.execute(effects);
        let effects = budget::track(|| self.kernel.start(now));
        self.execute(effects);
        if self.fixtures && self.profile.wifi {
            // Known as if joined before: joined when a plugin wants the
            // network, as a device of the reader's would.
            let keep = paperpaw_core::sdk::wifi::keep(crate::wifi::TEST_NETWORK, "fixtures");
            self.kernel.set_secret(keep);
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

    fn go_online(&mut self, now: Instant) {
        let keep = paperpaw_core::sdk::wifi::keep(
            crate::wifi::WORKSHOP_NETWORK,
            crate::wifi::WORKSHOP_PASSWORD,
        );
        self.kernel.set_secret(keep);
        self.wifi.online(now);
        // On USB: what keeps the radio on, as on a device.
        let status = self.status(now);
        let effects = budget::track(|| self.kernel.set_status(now, status));
        self.execute(effects);
        self.tick_if_due();
    }

    /// Whether network work is under way that the emulated time brings to
    /// an end: requests not answered yet (waiting for the network or in
    /// flight), or the pretend radio starting, scanning or joining. With
    /// the fixtures (answered at once), a tool lets time pass until it
    /// ends, as a reader would wait.
    pub fn network_settling(&self) -> bool {
        self.http.in_flight > 0
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

    /// The emulated clock and battery: the computer's time and time zone
    /// (with simulated time a fixed one, so screenshots repeat) and a
    /// battery at 80 %, charging where it starts online.
    fn status(&self, now: Instant) -> SystemStatus {
        let clock = match (self.wall, &self.clock) {
            (Some((at, unix_millis, utc_offset_minutes)), _) => WallClock {
                at,
                unix_millis,
                utc_offset_minutes,
            },
            #[cfg(not(target_arch = "wasm32"))]
            (None, Clock::Real(_)) => {
                let local = chrono::Local::now();
                WallClock {
                    at: now,
                    unix_millis: local.timestamp_millis(),
                    utc_offset_minutes: Some(local.offset().local_minus_utc() / 60),
                }
            }
            // 14 October 2026, 10:41 in Berlin (also in a browser, which
            // drives the clock itself).
            _ => WallClock {
                at: now,
                unix_millis: 1_791_967_260_000,
                utc_offset_minutes: Some(120),
            },
        };
        SystemStatus {
            clock: Some(clock),
            battery: Some(Battery {
                percent: 80,
                charging: self.wifi_online && self.profile.wifi,
                millivolts: None,
            }),
        }
    }

    /// Sets the wall clock to `unix_millis` milliseconds since 1970 (UTC)
    /// now (the remote's `time set`); kept over restarts.
    pub fn set_time(&mut self, unix_millis: i64) {
        let now = self.now();
        // The time zone stays as it was.
        let offset = self.status(now).clock.and_then(|c| c.utc_offset_minutes);
        self.wall = Some((now, unix_millis, offset));
        let status = self.status(now);
        let effects = budget::track(|| self.kernel.set_status(now, status));
        self.execute(effects);
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
        let effects = budget::track(|| self.kernel.open_screen(now, screen, params));
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
                self.asleep = false;
                eprintln!("{}: awake", self.profile.device.id);
                let effects = budget::track(|| self.kernel.resume(now));
                self.execute(effects);
            }
            return;
        }
        let effects = budget::track(|| self.kernel.handle(now, event));
        self.execute(effects);
    }

    /// Delivers HTTP answers that have arrived; with `wait`, waits that
    /// long for the requests still in flight.
    pub fn poll_http(&mut self, wait: Option<std::time::Duration>) {
        while let Some((id, received, total)) = self.http.next_progress() {
            let now = self.now();
            let effects = budget::track(|| self.kernel.http_progress(now, id, received, total));
            self.execute(effects);
        }
        while self.http.in_flight > 0 {
            let Some(response) = self.http.next(wait) else {
                break;
            };
            let now = self.now();
            let effects = budget::track(|| self.kernel.http_response(now, response));
            self.execute(effects);
        }
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
        if self.asleep {
            return None;
        }
        let speech = self.speech.front().map(|&(_, end)| end);
        [
            self.wake_at,
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
        if self.asleep {
            return;
        }
        self.poll_http(None);
        self.poll_servers();
        let now = self.now();
        while let Some(event) = self.wifi.take_due(now) {
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
        while self.http.in_flight > 0 {
            let left = end.saturating_duration_since(StdInstant::now());
            let Some(response) = self.http.next(Some(left)).filter(|_| !left.is_zero()) else {
                break;
            };
            let now = self.now();
            let effects = budget::track(|| self.kernel.http_response(now, response));
            self.execute(effects);
        }
    }

    /// Shows the whole screen in four grays, as the X4 does (see
    /// `Effects::grays`); where there is no memory for it, as it was.
    fn show_gray(&mut self) {
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
        if drawn {
            self.panel
                .refresh(gray.view(), screen, RefreshMode::Full, None);
            self.gray_shown = true;
        }
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
                self.show_gray();
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
        if effects.sleep() && !self.asleep {
            // The sleep screen is shown: plugins save, the device sleeps.
            self.asleep = true;
            eprintln!("{}: asleep", self.profile.device.id);
            let now = self.now();
            let effects = budget::track(|| self.kernel.suspend(now));
            self.execute(effects);
        }
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
        let storage = match self.library.open() {
            Ok(storage) => storage,
            Err(e) => return eprintln!("restart: {:?}: {e}", self.library),
        };
        // Plugins save what is pending, as on the device before it restarts.
        let now = self.now();
        budget::track(|| self.kernel.stop(now));
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

/// The standard app for `profile` (with the plugins `add` adds), counted
/// against its heap budget.
fn build_kernel(
    profile: &'static Profile,
    user_config: Option<&str>,
    storage: Card,
    rotation: Rotation,
    add: Option<AddPlugins>,
) -> Result<Kernel, KernelError> {
    budget::track(|| {
        let fonts = if profile.font_packs {
            paperpaw_app::Fonts::Packs
        } else {
            paperpaw_app::Fonts::Outline
        };
        let mut device = turned(profile.device, rotation);
        device.input.keys = profile.buttons;
        let mut builder = Kernel::builder(device)
            .storage(crate::trace::Traced(storage))
            .clock(crate::host::micros)
            .random(crate::host::random)
            .memory(budget::memory_use)
            .glyph_cache(profile.glyph_cache)
            .busy_mark(profile.busy_mark)
            // Its grays; frames show at once here (see `execute`).
            .panel(paperpaw_core::sdk::realtime::PanelInfo::new(
                device
                    .display
                    .format
                    .gray_levels()
                    .map_or(255, |levels| levels.min(255) as u8),
            ))
            .colors(profile.colors)
            .network(true)
            .wifi(profile.wifi)
            .bluetooth(profile.bluetooth)
            .speech(profile.speech)
            .sleep(profile.sleep)
            .button_row(profile.button_row)
            .rotation(true);
        // Scalable fonts only where the emulated device renders them.
        if !profile.font_packs {
            builder = builder.font_dir("fonts");
        }
        // Every plugin, those the store offers as files too switched on,
        // as if the reader had installed them (the emulator loads no
        // plugin files): `--set plugins.<id>=off` shows a device without.
        let mut builder = paperpaw_app::install(builder, fonts);
        #[cfg(feature = "hello")]
        {
            builder = builder.plugin(paperpaw_sample_hello::Hello);
        }
        #[cfg(feature = "frame-test")]
        {
            builder = builder.plugin(paperpaw_sample_frame_test::FrameTest);
        }
        if let Some(light) = profile.light {
            builder = builder.plugin(crate::light::Emulated(light));
        }
        // Code plugins built for this computer, from the card.
        #[cfg(not(target_arch = "wasm32"))]
        if CODE_PLUGINS.load(std::sync::atomic::Ordering::Relaxed) {
            builder =
                builder.code_plugins(crate::native::native(), Box::new(crate::native::HostLoader));
        }
        if let Some(add) = add {
            builder = add(builder);
        }
        if let Some(rows) = profile.frame_rows {
            builder = builder.frame_rows(rows);
        }
        builder = builder.row_align(profile.row_align);
        if let Some(name) = profile.catalog_device {
            builder = builder.catalog_device(name);
        }
        for (id, value) in profile.settings {
            builder = builder.setting_default_text(id, value);
        }
        if let Some(config) = user_config {
            builder = builder.user_config(config);
        }
        builder.build()
    })
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

    /// The heap budget counts every thread's kernel: one device at a time,
    /// and none while the budget's own tests read its counters.
    use crate::budget::accounting_tests::LOCK as ONE_AT_A_TIME;

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

    #[test]
    fn asleep_nothing_is_due() {
        let _one = ONE_AT_A_TIME
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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
        let _one = ONE_AT_A_TIME
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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
}
