//! One emulated device in the browser: the desktop emulator's device model
//! (kernel, panel, profile, heap budget) with its files in memory, the
//! sample books and the data plugins given to it, as a card would have
//! them. Nothing here knows the browser: the exports (browser builds only)
//! pass its calls on, and the tests drive it directly.

mod commands;

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use paperpaw_core::sdk::install::{is_plugin_name, is_safe_path};
use paperpaw_core::sdk::{MemoryStorage, Storage};
use paperpaw_dev::report::json_string;
use paperpaw_dev::target::{DataPlugin, Target};
use paperpaw_emulator::device::{AddPlugins, Device};
use paperpaw_emulator::profiles::{self, Profile};
use paperpaw_emulator::{budget, http, panel, trace};
use paperpaw_platform::{
    ButtonState, InputEvent, PixelFormat, Point, PointerEvent, PointerPhase, PointerTool,
};

use crate::network::{self, Fixture, Network, Routed};

/// The books every library starts with (from `samples/library`).
const BOOKS: &[(&str, &[u8])] = &[
    (
        "books/alice-in-wonderland.epub",
        include_bytes!("../../../samples/library/books/alice-in-wonderland.epub"),
    ),
    (
        "books/die-verwandlung.epub",
        include_bytes!("../../../samples/library/books/die-verwandlung.epub"),
    ),
];

/// The devices the browser offers, as `paperpaw-dev`'s galleries show
/// them.
pub const PROFILES: [&str; 3] = ["xteink-x4", "xteink-x4-pro", "boox-go-7"];

/// PaperPaw's version, and the commit the module was built from where
/// the build said (`PAPERPAW_COMMIT`, set by `build.py`).
pub fn version() -> String {
    match option_env!("PAPERPAW_COMMIT") {
        Some(commit) if !commit.is_empty() => format!("{} ({commit})", env!("CARGO_PKG_VERSION")),
        _ => env!("CARGO_PKG_VERSION").to_string(),
    }
}

/// Wake-ups handled in one call of [`Emulator::advance`], at most.
const MAX_TICKS: usize = 100;

/// A device running the standard app with the data plugins given.
#[derive(Debug)]
pub struct Emulator {
    profile: &'static Profile,
    language: String,
    dark: bool,
    /// The device's storage: the books, the data plugins.
    files: Rc<MemoryStorage>,
    /// The ids of the data plugins given (their files are on the card
    /// alone).
    plugins: BTreeSet<String>,
    device: Option<Device>,
    /// What the panel shows, four bytes a pixel (RGBA), for a canvas.
    frame: Vec<u8>,
    /// Plugins' requests: recorded answers, the page's fetches (see
    /// [`crate::network`]).
    network: Rc<RefCell<Network>>,
}

impl Emulator {
    /// A device of `profile` (one of [`PROFILES`], or any profile of the
    /// emulator), its texts in `language`, in night mode if `dark`. Not
    /// started yet: data plugins are given first ([`Emulator::put_file`]).
    pub fn new(profile: &str, language: &str, dark: bool) -> Result<Self, String> {
        let profile =
            profiles::find(profile).ok_or_else(|| format!("no device profile `{profile}`"))?;
        if language.is_empty()
            || !language
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-')
        {
            return Err(format!("no language `{language}`"));
        }
        let files = MemoryStorage::new();
        for (path, bytes) in BOOKS {
            files
                .write(path, bytes)
                .map_err(|e| format!("{path}: {e:?}"))?;
        }
        // The first-run setup counts as done.
        files
            .write(".paperpaw/plugins/setup/done", b"")
            .map_err(|e| format!("setup: {e:?}"))?;
        Ok(Self {
            profile,
            language: language.to_string(),
            dark,
            files: Rc::new(files),
            plugins: BTreeSet::new(),
            device: None,
            frame: Vec::new(),
            network: Network::shared(),
        })
    }

    pub fn profile(&self) -> &'static Profile {
        self.profile
    }

    /// Puts a file of the data plugin `plugin` on the card, as
    /// `plugins/<plugin>/<path>`; seen by the app at the next start
    /// ([`Emulator::restart`]).
    pub fn put_file(&mut self, plugin: &str, path: &str, bytes: &[u8]) -> Result<(), String> {
        if !is_plugin_name(plugin) {
            return Err(format!("`{plugin}` is no plugin's name"));
        }
        if !is_safe_path(path) {
            return Err(format!("`{path}` is no path inside a plugin"));
        }
        let at = format!("plugins/{plugin}/{path}");
        self.files
            .write(&at, bytes)
            .map_err(|e| format!("{at}: {e:?}"))?;
        self.plugins.insert(plugin.to_string());
        Ok(())
    }

    /// Takes the data plugin `plugin` off the card, every file of it
    /// (also those a page put there some other way).
    pub fn remove_plugin(&mut self, plugin: &str) {
        self.plugins.remove(plugin);
        if is_plugin_name(plugin) {
            let _ = self.files.remove_dir_all(&format!("plugins/{plugin}"));
        }
    }

    /// Starts the device: the standard app with the data plugins on the
    /// card. Started already, it starts again (see [`Emulator::restart`]).
    pub fn start(&mut self) -> Result<(), String> {
        if self.device.is_some() {
            self.restart();
            return Ok(());
        }
        // Dark as asked; else as the defaults have it (a theme's too).
        let night = if self.dark { "night-mode = on\n" } else { "" };
        // Started as the first-run setup left it, and without the tips on
        // gestures a new reader sees once.
        let config = format!(
            "[core]\nlanguage = {}\n{night}[help]\nshow = false\n",
            self.language
        );
        let mut device = Device::in_memory(
            self.profile,
            Some(config),
            self.files.clone(),
            true,
            added(),
        )
        .map_err(|e| e.to_string())?;
        self.route();
        device.start();
        self.device = Some(device);
        Ok(())
    }

    /// Sends the thread's requests to this device's network, of the page's
    /// making.
    fn route(&self) {
        http::set_route(Some(Box::new(Routed(self.network.clone()))));
    }

    /// The recorded answers plugins' requests get (see
    /// [`crate::network`]); those before are let go.
    pub fn set_fixtures(&mut self, fixtures: Vec<Fixture>) {
        self.network.borrow_mut().set_fixtures(fixtures);
    }

    /// Adds a recorded answer after those there are.
    pub fn add_fixture(&mut self, fixture: Fixture) {
        self.network.borrow_mut().add_fixture(fixture);
    }

    /// Lets the page fetch what no recorded answer answers (`true`), or
    /// fails it as without a network.
    pub fn set_real_network(&mut self, real: bool) {
        self.network.borrow_mut().set_real(real);
    }

    /// The requests plugins made since asked last, as JSON (see
    /// [`network::seen_json`]).
    pub fn requests(&mut self) -> String {
        network::seen_json(&self.network.borrow_mut().take_seen())
    }

    /// The requests for the page to fetch since asked last, as JSON (see
    /// [`network::outbox_json`]).
    pub fn outbox(&mut self) -> String {
        network::outbox_json(&self.network.borrow_mut().take_outbox())
    }

    /// The page's answer to request `id` it fetched (status 0: failed,
    /// the reason in `body`), from the address `url` (after redirects;
    /// empty: the one asked), at `now_ms`: given to the device.
    pub fn fetched(
        &mut self,
        id: u32,
        status: u16,
        headers: &str,
        body: Vec<u8>,
        url: &str,
        now_ms: u64,
    ) -> Result<(), String> {
        if !self
            .network
            .borrow_mut()
            .fetched(id, status, headers, body, url)
        {
            return Err(format!("no request {id} waits for an answer"));
        }
        self.advance(now_ms);
        Ok(())
    }

    /// Starts the device again, as after a reboot: what changed on the
    /// card (a data plugin's files) is read anew. A few milliseconds.
    pub fn restart(&mut self) {
        if let Some(device) = &mut self.device {
            device.restart();
        }
    }

    /// Whether the device has a button `name`.
    pub fn has_button(&self, name: &str) -> bool {
        self.profile.buttons.contains(&name)
    }

    /// A button pressed or let go at `now_ms` (as [`Emulator::advance`]
    /// counts); `false` if the device has no such button. Time passes to
    /// then first: a press is as long as it was held, also where the
    /// device had nothing to do for a while before.
    pub fn button(&mut self, name: &str, pressed: bool, now_ms: u64) -> bool {
        let Some(&name) = self.profile.buttons.iter().find(|b| **b == name) else {
            return false;
        };
        self.advance(now_ms);
        let state = if pressed {
            ButtonState::Pressed
        } else {
            ButtonState::Released
        };
        if let Some(device) = &mut self.device {
            device.input(InputEvent::Button { name, state });
        }
        true
    }

    /// A finger put down, moved or lifted at `(x, y)` of the screen at
    /// `now_ms`, time passed to then first (as [`Emulator::button`]);
    /// nothing where the device has no touch screen.
    pub fn pointer(&mut self, phase: PointerPhase, x: i32, y: i32, now_ms: u64) {
        if !self.profile.device.input.touch {
            return;
        }
        self.advance(now_ms);
        if let Some(device) = &mut self.device {
            device.input(InputEvent::Pointer(PointerEvent::new(
                0,
                phase,
                Point::new(x, y),
                PointerTool::Finger,
                1000,
            )));
        }
    }

    /// Lets time pass to `now_ms` (since the emulator started, as the
    /// browser counts it) and does what is due; how many milliseconds
    /// until something is due next, `None` if nothing is.
    pub fn advance(&mut self, now_ms: u64) -> Option<u64> {
        // Recorded answers due by then.
        self.network.borrow_mut().tick(now_ms);
        let device = self.device.as_mut()?;
        let now = device.now().millis();
        device.advance(now_ms.saturating_sub(now));
        for _ in 0..MAX_TICKS {
            device.poll_http(None);
            match device.next_wake() {
                Some(at) if at.millis() <= device.now().millis() => device.tick_if_due(),
                _ => break,
            }
        }
        let now = device.now().millis();
        let wake = device.next_wake().map(|at| at.millis());
        // A recorded answer coming later is something due too.
        let answer = self.network.borrow().next_due();
        let next = match (wake, answer) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        next.map(|at| at.saturating_sub(now))
    }

    /// The id of the screen shown, `None` before the device started.
    pub fn screen(&self) -> Option<String> {
        let device = self.device.as_ref()?;
        device.kernel.active_screen().map(ToString::to_string)
    }

    /// The screen's size in pixels (turned as the reader turned it).
    pub fn size(&self) -> (u16, u16) {
        match &self.device {
            Some(device) => (device.panel.width(), device.panel.height()),
            None => {
                let display = self.profile.device.display;
                (display.width, display.height)
            }
        }
    }

    /// What the panel shows now, four bytes a pixel (RGBA), row by row:
    /// its grays as the panel shows them, a full refresh flashing, in the
    /// tones of e-ink on a gray panel ([`eink`]).
    pub fn frame(&mut self) -> &[u8] {
        self.frame.clear();
        let gray = self.profile.device.display.color.is_none();
        if let Some(device) = &self.device {
            for pixel in device.panel.compose() {
                let rgb = [(pixel >> 16) as u8, (pixel >> 8) as u8, pixel as u8];
                let [r, g, b] = if gray { eink(rgb) } else { rgb };
                self.frame.extend_from_slice(&[r, g, b, 255]);
            }
        }
        &self.frame
    }

    /// What the panel shows once a full refresh has flashed, four bytes a
    /// pixel (RGBA): for screenshots.
    pub fn shown(&self) -> Vec<u8> {
        let Some(device) = &self.device else {
            return Vec::new();
        };
        device
            .panel
            .rgb_bytes()
            .chunks(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect()
    }

    /// Whether the panel still changes by itself (a full refresh
    /// flashing): frames are wanted until it is done.
    pub fn animating(&self) -> bool {
        self.device.as_ref().is_some_and(|d| d.panel.is_animating())
    }

    /// The data plugins given and the plugins with code named in `code`
    /// (added with [`set_plugins`]), checked by `paperpaw-dev` on this
    /// device's profile with `commands` (none: package, lint, visual and
    /// reach; `perf` too where code is checked); its report as JSON
    /// (`report.json`).
    pub fn check(&mut self, commands: &[&str], code: &[&str]) -> Result<String, String> {
        if self.plugins.is_empty() && code.is_empty() {
            return Err("no data plugin on the card to check".to_string());
        }
        let mut target = Target::default();
        if !code.is_empty() {
            let add = added().ok_or("no plugin with code was added (`set_plugins`)")?;
            let device = self.device.as_ref().ok_or("the device has not started")?;
            for id in code {
                if !device
                    .kernel
                    .plugins()
                    .iter()
                    .any(|p| p.manifest.id.as_str() == *id)
                {
                    return Err(format!("no plugin `{id}` in the app"));
                }
                target.plugins.push((*id).to_string());
            }
            target.add = Some(add);
        }
        // Read from the card, held while checked.
        for id in &self.plugins {
            let files = files_under(&*self.files, &format!("plugins/{id}"));
            target.take_data(DataPlugin::new(id, files));
        }
        let report = paperpaw_dev::data::check(target, vec![self.profile], commands);
        // The checks measure their own devices; this one keeps its budget
        // and records no frames.
        trace::stop_recording();
        budget::end_override(self.profile.heap_budget);
        Ok(report?.json())
    }

    /// What another device (`profile`), night mode or not (`dark`), or the
    /// sleep screen (`asleep`) shows with the card as it is: made on a
    /// device of its own, so that what this one shows stays as it is.
    /// Width, height and four bytes a pixel (RGBA), settled.
    pub fn snapshot(
        &self,
        profile: Option<&str>,
        dark: Option<bool>,
        asleep: bool,
    ) -> Result<(u16, u16, Vec<u8>), String> {
        let profile = profile.unwrap_or(self.profile.device.id);
        let mut other = Emulator::new(profile, &self.language, dark.unwrap_or(self.dark))?;
        // The same recorded answers, in a network of its own: its answers
        // reach it alone, and the page fetches nothing for it.
        other.set_fixtures(self.network.borrow().fixtures());
        // The same card, copied: what the other device writes stays its.
        for (path, bytes) in files_under(&*self.files, "") {
            other
                .files
                .write(&path, &bytes)
                .map_err(|e| format!("{path}: {e:?}"))?;
        }
        other.plugins = self.plugins.clone();
        // Its memory is no part of this device's.
        budget::override_limit(None);
        let made = other.start().map(|()| {
            other.advance(5_000);
            if asleep && let Some(device) = &mut other.device {
                device.call(|kernel, now| {
                    kernel.open_slot(
                        now,
                        paperpaw_core::sdk::builtin::slots::SLEEP,
                        Default::default(),
                    )
                });
            }
            other.advance(10_000);
            let (width, height) = other.size();
            (width, height, other.shown())
        });
        drop(other);
        // This device's requests to its own network again.
        if self.device.is_some() {
            self.route();
        }
        budget::end_override(self.profile.heap_budget);
        made
    }

    /// The device as the page shows it, as JSON: its name, screen size,
    /// grays and dpi, whether it has touch, its buttons and those in a
    /// row below the screen, and its body with the keys on it.
    pub fn device_json(&self) -> String {
        let device = &self.profile.device;
        let (width, height) = self.size();
        let grays = match device.display.format {
            PixelFormat::Mono1 => 2,
            PixelFormat::Gray2 => 4,
            PixelFormat::Gray4 => 16,
            _ => 256,
        };
        let list = |names: &[&str]| {
            names
                .iter()
                .map(|n| json_string(n))
                .collect::<Vec<_>>()
                .join(",")
        };
        format!(
            "{{\"id\":{},\"name\":{},\"width\":{width},\"height\":{height},\"dpi\":{},\"grays\":{grays},\"color\":{},\"touch\":{},\"buttons\":[{}],\"row\":[{}],\"body\":{},\"version\":{}}}",
            json_string(device.id),
            json_string(device.name),
            device.display.dpi,
            device.display.color.is_some(),
            device.input.touch,
            list(self.profile.buttons),
            list(self.profile.button_row),
            self.profile.body.json(),
            json_string(&version()),
        )
    }
}

/// The plugins with code added to the standard app ([`set_plugins`]).
static PLUGINS: std::sync::Mutex<Option<AddPlugins>> = std::sync::Mutex::new(None);

/// Adds plugins with code to the standard app of every device started
/// after: for a module built around a plugin being written, a crate of
/// its own with `paperpaw-web` as a library and a `cdylib` of its own
/// (the module the page loads, with all of this one's functions). It
/// calls this from a function `pp_plugins` it exports, which the page's
/// loader calls first:
///
/// ```ignore
/// #[unsafe(no_mangle)]
/// pub extern "C" fn pp_plugins() {
///     paperpaw_web::set_plugins(|app| app.plugin(my_plugin::MyPlugin));
/// }
/// ```
///
/// Its checks name the plugin's id ([`Emulator::check`]).
pub fn set_plugins(add: AddPlugins) {
    *PLUGINS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(add);
}

/// The plugins with code added ([`set_plugins`]).
fn added() -> Option<AddPlugins> {
    *PLUGINS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The darkest a gray e-ink panel shows: its black reflects about a
/// twelfth of the light its paper does (in linear light, against the
/// panel model's paper), where a screen's black reflects almost nothing
/// (the panel model's ink, [`panel::INK`]).
const EINK_BLACK: u8 = 0x48;

/// The panel model's colour in the tones of e-ink: each channel's range
/// from its ink to its paper ([`panel::PAPER`]) narrowed to that from
/// e-ink's black, so that text is as dark as on the device, not darker
/// (it looks bolder on a screen than on the panel otherwise). Paper stays
/// as it is, as in screenshots; what is lighter (a full refresh's white
/// flash) keeps its order.
pub fn eink(rgb: [u8; 3]) -> [u8; 3] {
    let mut out = rgb;
    for ((out, ink), paper) in out.iter_mut().zip(panel::INK).zip(panel::PAPER) {
        let (value, ink) = (u32::from((*out).max(ink)), u32::from(ink));
        let (black, paper) = (u32::from(EINK_BLACK), u32::from(paper));
        *out = (black + (value - ink) * (paper - black) / (paper - ink)) as u8;
    }
    out
}

/// Every file under `dir` of `storage` (`""` the whole card), by its
/// path from `dir`.
fn files_under(storage: &dyn Storage, dir: &str) -> BTreeMap<String, Vec<u8>> {
    let mut out = BTreeMap::new();
    let mut folders = vec![String::new()];
    let join = |a: &str, b: &str| match (a.is_empty(), b.is_empty()) {
        (true, _) => b.to_string(),
        (_, true) => a.to_string(),
        _ => format!("{a}/{b}"),
    };
    while let Some(folder) = folders.pop() {
        let at = join(dir, &folder);
        let mut entries = Vec::new();
        let _ = storage.list(&at, &mut |entry| {
            entries.push((entry.name.to_string(), entry.is_dir));
        });
        for (name, is_dir) in entries {
            let path = if folder.is_empty() {
                name
            } else {
                format!("{folder}/{name}")
            };
            if is_dir {
                folders.push(path);
            } else if let Ok(bytes) =
                paperpaw_core::sdk::read_file(storage, &join(dir, &path), usize::MAX)
            {
                out.insert(path, bytes);
            }
        }
    }
    out
}
