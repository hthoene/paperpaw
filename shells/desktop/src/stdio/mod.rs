//! `paperpaw-emu --stdio`: the emulator driven by another program (the
//! workshop, a test) over its standard input and output, one JSON object
//! a line each way. The program sends requests; the emulator answers and
//! tells what happens: a frame only when the panel changed, errors,
//! warnings, the requests plugins make.
//!
//! [`PROTOCOL`] is the protocol's version, said first (`ready`); a field
//! is only added, and a request or event whose meaning changes gets a new
//! name. Version 2 added `slot`, `set`, `setting`, `emit`, `screens`,
//! `actions`, `wait`, `until`, `expect`, `network`, `fixture` and the
//! `screen` of `described`, after the X4's remote control
//! (`tools/x4-remote.py`); what version 1 had is as it was.
//!
//! Requests (`op`):
//!
//! | `op` | Fields | Does |
//! |------|--------|------|
//! | `start` | `profile`, `library` (a folder), `settings` (INI text, optional), `trust_code` (optional), `wifi` (`online` or `off`, optional) | starts a device of that profile on the folder (setup done), with `trust_code` the code plugins built for this computer there installed (`native::trust`), with `wifi: online` online from the start as `--wifi online` (`off`: the radio off; not given: as `--wifi` said); `started` follows, then a `frame` |
//! | `button` | `name` (`confirm`, `page-forward`, …), `pressed` | a button pressed or let go |
//! | `pointer` | `phase` (`down`, `move`, `up`, `cancel`), `x`, `y` | a finger on a touch screen, at the panel's pixels |
//! | `reload` | | starts the device again on its folder: plugins changed there are read anew |
//! | `describe` | | `described`: what the screen shows, part by part |
//! | `run` | `action` | runs an action, as a button bound to it would |
//! | `open` | `screen`, `params` (an object of texts, optional) | opens a screen, as a plugin would |
//! | `shot` | `path` (optional) | `shot`: the panel as a PNG, written there or in the event |
//! | `slot` | `slot`, `params` (optional) | opens the screen bound to a slot, as a plugin would (`core.settings`) |
//! | `set` | `id`, `value` (as configuration files write it) | `set`: a setting set, as the settings screen would |
//! | `setting` | `id` | `setting`: its value as configuration files write it (`null` if there is none, `•` for a secret) |
//! | `emit` | `event`, `params` (optional) | sends an event, as a plugin would |
//! | `screens` | | `screens`: the screens open (`open`, the one shown last), `shown`, and every screen plugins added (`known`) |
//! | `actions` | `prefix` (optional) | `actions`: every action, its `id` and `title` |
//! | `wait` | `ms` (at most 600 000) | lets that much time pass, the device going on; `waited` follows with the `ms` it waited |
//! | `until` | `text` or `screen`, `timeout` (ms, 30 000 if not given, at most 600 000) | waits until the screen shows that text (as `tap-on` finds it) or is that screen; `until` with `found` and `ms` |
//! | `expect` | `text`, `not` (optional) | `expect` with `ok`: whether the screen shows that text now (or not) |
//! | `network` | `mode`: `real`, `recorded` or `offline` | where plugins' requests go: the network, the recorded answers (`fixture`; none recorded: failed), nowhere (failed); `network` follows |
//! | `fixture` | `url` (a `*` at its end matches what starts so), `method`, `status` (200), `body` (text) or `file` (its bytes: a file of at most 16 MB in the folder the device was started on, relative to it), `content_type` | records an answer for `recorded`; `clear: true` forgets them all |
//! | `quit` | | ends |
//!
//! Events (`event`): `ready` (`protocol`, `paperpaw`, `profiles`,
//! `fingerprint` and `toolchain`, which code plugins must be built for,
//! and `commit`, the commit of PaperPaw's source it is built from or
//! `null`, and `release`, whether that is the release of its version, so
//! tools can pin `paperpaw-sdk` to exactly that source),
//! `started` (`profile`, `width`, `height`, `buttons`, `touch`), `frame`
//! (`width`, `height`, `png` in base64), `described` (`screen`, `parts`:
//! `role`, `label`, `x`, `y`, `width`, `height`, `focused`, `on`), `shot`
//! (`path` or `png`), `request` (`method`, `url`, and `answer`: `network`,
//! `recorded` or `none`), `set`, `setting`, `screens`, `actions`,
//! `waited`, `until`, `expect`, `network`, `fixture`, `warning`
//! (`message`), `error` (`message`, and `op` where a request caused it).

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::time::Duration;

use paperpaw_core::sdk::json::string;
use paperpaw_platform::{ButtonState, InputEvent, Point, PointerEvent, PointerTool};

use paperpaw_core::sdk::Id;

use crate::device::{self, Device};
use crate::profiles::{self, PROFILES};

mod fixtures;
mod requests;

use fixtures::{Answers, fixture_file};
pub use fixtures::{Fixture, Network};
use requests::op_name;
pub use requests::{Request, parse};

/// The protocol's version.
pub const PROTOCOL: u32 = 2;

/// How long `until` waits where it is not told, in milliseconds.
const UNTIL_MS: u64 = 30_000;

/// The longest `wait` or `until`, in milliseconds: ten minutes. Requests
/// that come meanwhile are read (`quit` and the end of the input end the
/// wait) and done after it.
const MAX_WAIT_MS: u64 = 600_000;

/// The largest file a `fixture` answers with.
const MAX_FIXTURE: u64 = 16 << 20;

/// How long the loop waits at most between looks when the device asks
/// for nothing sooner (a clock, an animation).
const IDLE_MS: u64 = 50;
/// The longest request line read, in bytes: a longer one is said to be
/// too long and passed over, up to its end.
pub const MAX_LINE: usize = 16 << 20;

/// The next line of `input` (without its end), at most `max` bytes: `None`
/// at the input's end, `Err` for one longer (read to its end and dropped,
/// so the next one is read as it is) or not UTF-8.
pub fn read_line(
    input: &mut impl BufRead,
    max: usize,
) -> std::io::Result<Option<Result<String, String>>> {
    let mut line = Vec::new();
    let mut long = false;
    let mut any = false;
    loop {
        let buf = input.fill_buf()?;
        if buf.is_empty() {
            break;
        }
        any = true;
        let (part, end) = match buf.iter().position(|&b| b == b'\n') {
            Some(at) => (&buf[..at], Some(at + 1)),
            None => (buf, None),
        };
        if !long && line.len() + part.len() > max {
            long = true;
            line = Vec::new();
        }
        if !long {
            line.extend_from_slice(part);
        }
        let used = end.unwrap_or(buf.len());
        input.consume(used);
        if end.is_some() {
            break;
        }
    }
    if !any {
        return Ok(None);
    }
    if long {
        return Ok(Some(Err(format!("a request longer than {max} bytes"))));
    }
    if line.last() == Some(&b'\r') {
        line.pop();
    }
    Ok(Some(
        String::from_utf8(line).map_err(|_| String::from("a request not in UTF-8")),
    ))
}

/// The device a program drives, and what it was last shown.
#[derive(Default)]
pub struct Session {
    device: Option<Device>,
    /// Where plugins' requests go, and the answers recorded for them,
    /// shared with the route the device's requests take (`network`).
    network: std::rc::Rc<std::cell::RefCell<(Option<Network>, Vec<Fixture>)>>,
    /// What the device was started with, for `reload`.
    started: Option<Started>,
    shown: Vec<u8>,
}

impl Session {
    pub fn new() -> Self {
        Self::default()
    }

    /// The first event: who answers.
    pub fn ready() -> String {
        let profiles: Vec<String> = PROFILES.iter().map(|p| string(p.device.id)).collect();
        let native = crate::native::native();
        format!(
            "{{\"event\": \"ready\", \"protocol\": {PROTOCOL}, \"paperpaw\": {}, \"profiles\": [{}], \"fingerprint\": \"{:016x}\", \"toolchain\": {}, \"commit\": {}, \"release\": {}}}",
            string(env!("CARGO_PKG_VERSION")),
            profiles.join(", "),
            native.fingerprint.0,
            string(&native.toolchain),
            match env!("PAPERPAW_SOURCE_COMMIT") {
                "" => String::from("null"),
                commit => string(commit),
            },
            env!("PAPERPAW_SOURCE_RELEASE")
        )
    }

    /// Does `request`; the events it makes. `None` to end.
    pub fn handle(&mut self, request: Request) -> Option<Vec<String>> {
        let op = op_name(&request);
        match self.carry_out(request) {
            Ok(Some(events)) => Some(events),
            Ok(None) => None,
            Err(message) => Some(vec![format!(
                "{{\"event\": \"error\", \"op\": {}, \"message\": {}}}",
                string(op),
                string(&message)
            )]),
        }
    }

    fn carry_out(&mut self, request: Request) -> Result<Option<Vec<String>>, String> {
        let mut events = Vec::new();
        match request {
            Request::Start {
                profile,
                library,
                settings,
                trust_code,
                wifi_online,
            } => {
                self.stop();
                let start = Started {
                    profile,
                    library,
                    settings,
                    trust_code,
                    wifi_online,
                };
                let device = start.device()?;
                events.push(started(&device));
                self.device = Some(device);
                self.started = Some(start);
            }
            Request::Reload => {
                let start = self
                    .started
                    .clone()
                    .ok_or_else(|| String::from("no device started"))?;
                self.stop();
                let device = start.device()?;
                events.push(started(&device));
                self.device = Some(device);
            }
            Request::Quit => {
                self.stop();
                return Ok(None);
            }
            Request::Network { mode } => {
                self.network.borrow_mut().0 = (mode != Network::Real).then_some(mode);
                crate::http::set_route((mode != Network::Real).then(|| {
                    Box::new(Answers(self.network.clone())) as Box<dyn crate::http::Route>
                }));
                let recorded = self.network.borrow().1.len();
                events.push(format!(
                    "{{\"event\": \"network\", \"mode\": \"{}\", \"fixtures\": {recorded}}}",
                    mode.name()
                ));
            }
            Request::FixtureFile { mut fixture, file } => {
                let started = self.started.as_ref().ok_or_else(|| {
                    String::from("a `file` is read from the device's folder: start one first")
                })?;
                fixture.body = fixture_file(&started.library, &file)?;
                return self.carry_out(Request::Fixture(Some(fixture)));
            }
            Request::Fixture(fixture) => {
                let mut network = self.network.borrow_mut();
                match fixture {
                    Some(fixture) => network.1.push(fixture),
                    None => network.1.clear(),
                }
                events.push(format!(
                    "{{\"event\": \"fixture\", \"fixtures\": {}}}",
                    network.1.len()
                ));
            }
            request => {
                let device = self
                    .device
                    .as_mut()
                    .ok_or_else(|| String::from("no device started"))?;
                if let Some(event) = drive(device, request)? {
                    events.push(event);
                }
            }
        }
        Ok(Some(events))
    }

    /// One look at the device: its ticks and answers, then what it has to
    /// tell (a frame when the panel changed, warnings, requests).
    pub fn step(&mut self) -> Vec<String> {
        let Some(device) = self.device.as_mut() else {
            return Vec::new();
        };
        device.tick_if_due();
        device.poll_http(None);
        device.poll_servers();
        let mut events = Vec::new();
        for warning in device.kernel.take_warnings() {
            events.push(format!(
                "{{\"event\": \"warning\", \"message\": {}}}",
                string(&warning.to_string())
            ));
        }
        let (mode, fixtures) = &*self.network.borrow();
        for (method, url) in device.requests.iter_mut().flat_map(std::mem::take) {
            let answer = match mode {
                None => "network",
                Some(Network::Recorded) if fixtures.iter().any(|f| f.answers(&method, &url)) => {
                    "recorded"
                }
                Some(_) => "none",
            };
            events.push(format!(
                "{{\"event\": \"request\", \"method\": {}, \"url\": {}, \"answer\": \"{answer}\"}}",
                string(&method),
                string(&url)
            ));
        }
        let rgb = device.panel.rgb_bytes();
        if rgb != self.shown {
            let (width, height) = (device.panel.width(), device.panel.height());
            match png(width, height, &rgb) {
                Ok(png) => events.push(format!(
                    "{{\"event\": \"frame\", \"width\": {width}, \"height\": {height}, \"png\": \"{}\"}}",
                    base64(&png)
                )),
                Err(message) => events.push(format!(
                    "{{\"event\": \"error\", \"message\": {}}}",
                    string(&message)
                )),
            }
            self.shown = rgb;
        }
        events
    }

    /// Whether the screen shows `text` (as `tap-on` finds it) or is
    /// `screen`, where given.
    fn shows(&mut self, text: Option<&str>, screen: Option<&str>) -> bool {
        let Some(device) = self.device.as_mut() else {
            return false;
        };
        let is_screen =
            screen.is_none_or(|id| device.kernel.active_screen().map(Id::as_str) == Some(id));
        let has_text = text.is_none_or(|text| {
            let parts = device.kernel.describe();
            paperpaw_core::sdk::semantics::find(&parts, text).is_some()
        });
        is_screen && has_text
    }

    /// Goes on (the device's ticks, what it tells told, the requests that
    /// come read) until the screen shows `text` or is `screen`, at most
    /// `timeout` milliseconds, or until the input ends (`quit`, or no
    /// more): whether it found it, and the milliseconds it took. With
    /// neither, `timeout` passes.
    fn until(
        &mut self,
        text: Option<&str>,
        screen: Option<&str>,
        timeout: u64,
        input: &mut Input<'_>,
        tell: &mut dyn FnMut(&str) -> std::io::Result<()>,
    ) -> std::io::Result<(bool, u64)> {
        let start = std::time::Instant::now();
        let looking = text.is_some() || screen.is_some();
        loop {
            self.step_into(tell)?;
            let ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
            if looking && self.shows(text, screen) {
                return Ok((true, ms));
            }
            if ms >= timeout || input.ended {
                return Ok((false, ms));
            }
            input.listen(Duration::from_millis(self.wait().min(timeout - ms).max(1)));
        }
    }

    /// One look at the device ([`Session::step`]), what it tells told.
    fn step_into(
        &mut self,
        tell: &mut dyn FnMut(&str) -> std::io::Result<()>,
    ) -> std::io::Result<()> {
        for event in self.step() {
            tell(&event)?;
        }
        Ok(())
    }

    /// How long until the device wants a look.
    fn wait(&self) -> u64 {
        self.device
            .as_ref()
            .and_then(|d| d.next_wake().map(|at| at.since(d.now())))
            .map_or(IDLE_MS, |ms| ms.clamp(1, IDLE_MS))
    }

    fn stop(&mut self) {
        if let Some(mut device) = self.device.take() {
            device.shut_down();
        }
        self.shown.clear();
    }
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("started", &self.started)
            .finish_non_exhaustive()
    }
}

/// What a device was started with (`start`), to start it so again.
#[derive(Debug, Clone)]
struct Started {
    profile: String,
    library: PathBuf,
    settings: Option<String>,
    trust_code: bool,
    wifi_online: Option<bool>,
}

impl Started {
    /// The device, started: the code plugins built for this computer
    /// installed first where asked (also at every `reload`, so a new
    /// build runs).
    fn device(&self) -> Result<Device, String> {
        // Code plugins only where the caller says so, for this device.
        crate::device::set_code_plugins(self.trust_code);
        if self.trust_code {
            crate::native::trust(&self.library)
                .map_err(|e| format!("{}: {e}", self.library.display()))?;
        }
        start(
            &self.profile,
            &self.library,
            self.settings.as_deref(),
            self.wifi_online,
        )
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // Requests to the network again, as without the protocol.
        if self.network.borrow().0.is_some() {
            crate::http::set_route(None);
        }
        self.stop();
    }
}

/// A device of `profile` on the folder `library`, its setup done (it
/// opens on the home screen, not the first-start questions), started.
fn start(
    profile: &str,
    library: &Path,
    settings: Option<&str>,
    wifi_online: Option<bool>,
) -> Result<Device, String> {
    let profile = profiles::find(profile).ok_or_else(|| format!("no profile `{profile}`"))?;
    let at = |e: std::io::Error| format!("{}: {e}", library.display());
    let setup = library.join(".paperpaw/plugins/setup");
    std::fs::create_dir_all(&setup).map_err(at)?;
    if !setup.join("done").exists() {
        std::fs::write(setup.join("done"), b"").map_err(at)?;
    }
    std::fs::create_dir_all(library.join("books")).map_err(at)?;
    let storage = device::library(library).map_err(at)?;
    let config = settings.map(String::from);
    let mut device = Device::new(profile, config, storage, false)
        .map_err(|e| format!("{}: {e:?}", profile.device.id))?;
    // Frames as they end up, not the panel's flashing on the way.
    device.panel.animate = false;
    device.requests = Some(Vec::new());
    if let Some(online) = wifi_online {
        device.set_wifi_online(online);
    }
    device.start();
    device.tick_if_due();
    Ok(device)
}

fn started(device: &Device) -> String {
    let profile = device.profile;
    let buttons: Vec<String> = profile.buttons.iter().map(|b| string(b)).collect();
    format!(
        "{{\"event\": \"started\", \"profile\": {}, \"width\": {}, \"height\": {}, \"buttons\": [{}], \"touch\": {}}}",
        string(profile.device.id),
        device.panel.width(),
        device.panel.height(),
        buttons.join(", "),
        profile.device.input.touch
    )
}

/// Does what drives the device; an event where the request has one.
fn drive(device: &mut Device, request: Request) -> Result<Option<String>, String> {
    match request {
        Request::Button { name, pressed } => {
            // The device's own name for it: the kernel takes `&'static str`.
            let name = *device
                .profile
                .buttons
                .iter()
                .find(|b| **b == name)
                .ok_or_else(|| format!("`{}` has no button `{name}`", device.profile.device.id))?;
            let state = if pressed {
                ButtonState::Pressed
            } else {
                ButtonState::Released
            };
            device.input(InputEvent::Button { name, state });
        }
        Request::Pointer { phase, x, y } => {
            if !device.profile.device.input.touch {
                return Err(format!(
                    "`{}` has no touch screen",
                    device.profile.device.id
                ));
            }
            device.input(InputEvent::Pointer(PointerEvent::new(
                0,
                phase,
                Point::new(x, y),
                PointerTool::Finger,
                1000,
            )));
        }
        Request::Describe => return Ok(Some(described(device))),
        Request::Run { action } => {
            device.run_action(paperpaw_core::sdk::Id::parse(&action).map_err(|e| e.to_string())?);
        }
        Request::Open { screen, params } => {
            let screen = Id::parse(&screen).map_err(|e| e.to_string())?;
            device.open_screen(screen, given(params));
        }
        Request::Shot { path } => {
            let rgb = device.panel.rgb_bytes();
            let png = png(device.panel.width(), device.panel.height(), &rgb)?;
            return Ok(Some(match path {
                Some(path) => {
                    if let Some(parent) = path.parent() {
                        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                    }
                    std::fs::write(&path, png).map_err(|e| format!("{}: {e}", path.display()))?;
                    format!(
                        "{{\"event\": \"shot\", \"path\": {}}}",
                        string(&path.display().to_string())
                    )
                }
                None => format!("{{\"event\": \"shot\", \"png\": \"{}\"}}", base64(&png)),
            }));
        }
        request => return ask(device, request),
    }
    Ok(None)
}

/// What version 2 asks of a device (`slot`, `set`, `setting`, `emit`,
/// `screens`, `actions`); an event where the request has one.
fn ask(device: &mut Device, request: Request) -> Result<Option<String>, String> {
    match request {
        Request::Slot { slot, params } => {
            let slot = Id::parse(&slot).map_err(|e| e.to_string())?;
            let params = given(params);
            device.call(|kernel, now| kernel.open_slot(now, slot, params));
        }
        Request::Set { id, value } => {
            let mut why = None;
            device.call(|kernel, now| match kernel.set_setting(now, &id, &value) {
                Ok(effects) => effects,
                Err(e) => {
                    why = Some(e);
                    paperpaw_core::Effects::default()
                }
            });
            if let Some(why) = why {
                return Err(format!("{id}: {why}"));
            }
            return Ok(Some(format!(
                "{{\"event\": \"set\", \"id\": {}, \"value\": {}}}",
                string(&id),
                string(&value)
            )));
        }
        Request::Setting { id } => {
            let settings = device.kernel.settings();
            let secret = settings.spec(&id).is_some_and(|spec| spec.extra().secret);
            let value = match settings.get(&id) {
                Some(_) if secret => string("\u{2022}"),
                Some(value) => string(&value.to_config_text()),
                None => String::from("null"),
            };
            return Ok(Some(format!(
                "{{\"event\": \"setting\", \"id\": {}, \"value\": {value}}}",
                string(&id)
            )));
        }
        Request::Emit { event, params } => {
            let event = Id::parse(&event).map_err(|e| e.to_string())?;
            let params = given(params);
            device.call(|kernel, now| kernel.send_event(now, event, params));
        }
        Request::Screens => {
            let ids = |ids: &mut dyn Iterator<Item = &Id>| {
                ids.map(|id| string(id.as_str()))
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            let kernel = &device.kernel;
            let shown = kernel
                .active_screen()
                .map_or_else(|| String::from("null"), |id| string(id.as_str()));
            return Ok(Some(format!(
                "{{\"event\": \"screens\", \"open\": [{}], \"shown\": {shown}, \"known\": [{}]}}",
                ids(&mut kernel.screens()),
                ids(&mut kernel.known_screens())
            )));
        }
        Request::Actions { prefix } => {
            let kernel = &device.kernel;
            let mut actions: Vec<_> = kernel
                .actions()
                .iter()
                .filter(|a| a.id().as_str().starts_with(prefix.as_str()))
                .map(|a| {
                    format!(
                        "{{\"id\": {}, \"title\": {}}}",
                        string(a.id().as_str()),
                        string(kernel.translator().tr(a.title()))
                    )
                })
                .collect();
            actions.sort();
            return Ok(Some(format!(
                "{{\"event\": \"actions\", \"actions\": [{}]}}",
                actions.join(", ")
            )));
        }
        _ => {}
    }
    Ok(None)
}

/// `params` as a request's parameters.
fn given(params: Vec<(String, String)>) -> paperpaw_core::sdk::Params {
    let mut given = paperpaw_core::sdk::Params::new();
    for (key, value) in params {
        given = given.with(key, value);
    }
    given
}

/// What the screen shows, part by part.
fn described(device: &mut Device) -> String {
    let parts: Vec<String> = device
        .kernel
        .describe()
        .into_iter()
        .map(|part| {
            let a = part.area;
            let on = part.on.map_or_else(|| String::from("null"), |on| on.to_string());
            format!(
                "{{\"role\": \"{:?}\", \"label\": {}, \"x\": {}, \"y\": {}, \"width\": {}, \"height\": {}, \"focused\": {}, \"on\": {on}}}",
                part.role,
                string(&part.label),
                a.x,
                a.y,
                a.width,
                a.height,
                part.focused
            )
        })
        .collect();
    let screen = device
        .kernel
        .active_screen()
        .map_or_else(|| String::from("null"), |id| string(id.as_str()));
    format!(
        "{{\"event\": \"described\", \"screen\": {screen}, \"parts\": [{}]}}",
        parts.join(", ")
    )
}

/// `rgb` (8 bits a channel) as a PNG.
fn png(width: u16, height: u16, rgb: &[u8]) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, u32::from(width), u32::from(height));
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(rgb).map_err(|e| e.to_string())?;
    writer.finish().map_err(|e| e.to_string())?;
    Ok(bytes)
}

/// `bytes` in base64 (RFC 4648, with padding).
fn base64(bytes: &[u8]) -> String {
    const LETTERS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(LETTERS[(n >> (18 - 6 * i) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Runs the protocol over `input` and `output` until `quit` or the end of
/// the input.
pub fn run(
    mut input: impl BufRead + Send + 'static,
    mut output: impl Write,
) -> std::io::Result<()> {
    let (lines, received) = channel();
    std::thread::Builder::new()
        .name("stdin".into())
        .spawn(move || {
            while let Ok(Some(line)) = read_line(&mut input, MAX_LINE) {
                if lines.send(line).is_err() {
                    break;
                }
            }
        })?;
    let mut session = Session::new();
    writeln!(output, "{}", Session::ready())?;
    output.flush()?;
    serve(&mut session, &received, &mut |event| {
        writeln!(output, "{event}")?;
        output.flush()
    })
}

/// The requests' lines as they come, and those read while the device
/// was waited for (`wait`, `until`), to do after it.
struct Input<'a> {
    received: &'a Receiver<Result<String, String>>,
    pending: std::collections::VecDeque<Result<String, String>>,
    /// `quit` came, or the input ended: what came before is still done,
    /// without waiting.
    ended: bool,
}

impl Input<'_> {
    /// The next line, at most `wait` from now; what was read before first.
    fn next(&mut self, wait: Duration) -> Result<Result<String, String>, RecvTimeoutError> {
        if let Some(line) = self.pending.pop_front() {
            return Ok(line);
        }
        if self.ended {
            return Err(RecvTimeoutError::Disconnected);
        }
        self.received.recv_timeout(wait)
    }

    /// Reads what comes in `wait`, while the device is waited for: kept
    /// for after it, but `quit` or the end of the input end it.
    fn listen(&mut self, wait: Duration) {
        if self.ended {
            return;
        }
        match self.received.recv_timeout(wait) {
            Ok(Ok(line)) if parse(&line) == Ok(Request::Quit) => self.ended = true,
            Ok(line) => self.pending.push_back(line),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => self.ended = true,
        }
    }
}

/// The loop: requests as they come, the device's ticks when due, its
/// events as they happen.
fn serve(
    session: &mut Session,
    received: &Receiver<Result<String, String>>,
    tell: &mut dyn FnMut(&str) -> std::io::Result<()>,
) -> std::io::Result<()> {
    let mut input = Input {
        received,
        pending: std::collections::VecDeque::new(),
        ended: false,
    };
    loop {
        match input.next(Duration::from_millis(session.wait())) {
            Ok(Err(message)) => tell(&format!(
                "{{\"event\": \"error\", \"message\": {}}}",
                string(&message)
            ))?,
            Ok(Ok(line)) if line.trim().is_empty() => {}
            Ok(Ok(line)) => {
                let events = match parse(&line) {
                    Ok(Request::Wait { ms }) => {
                        let (_, ms) = session.until(None, None, ms, &mut input, tell)?;
                        Some(vec![format!("{{\"event\": \"waited\", \"ms\": {ms}}}")])
                    }
                    Ok(Request::Until {
                        text,
                        screen,
                        timeout,
                    }) => {
                        let (found, ms) = session.until(
                            text.as_deref(),
                            screen.as_deref(),
                            timeout,
                            &mut input,
                            tell,
                        )?;
                        Some(vec![format!(
                            "{{\"event\": \"until\", \"found\": {found}, \"ms\": {ms}}}"
                        )])
                    }
                    Ok(Request::Expect { text, not }) => {
                        session.step_into(tell)?;
                        let shows = session.shows(Some(&text), None);
                        Some(vec![format!(
                            "{{\"event\": \"expect\", \"text\": {}, \"ok\": {}}}",
                            string(&text),
                            shows != not
                        )])
                    }
                    Ok(request) => session.handle(request),
                    Err(message) => Some(vec![format!(
                        "{{\"event\": \"error\", \"message\": {}}}",
                        string(&message)
                    )]),
                };
                let Some(events) = events else {
                    return Ok(());
                };
                for event in events {
                    tell(&event)?;
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return Ok(()),
        }
        for event in session.step() {
            tell(&event)?;
        }
    }
}

#[cfg(test)]
mod tests;
