//! Remote control over USB, for testing on the device (built with the
//! `remote` feature): the commands of [`paperpaw_x4::remote`] read from
//! the USB unit, and every `PP:` answer. What only asks (the screen, the
//! heap, a file) is answered here; what changes the X4's state (a press,
//! an action, the network) is the main loop's (`firmware`).

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write;

use esp_println::println;
use paperpaw_core::{HttpResponse, JoinFailure, Kernel, WifiNetwork, WifiState};
use paperpaw_platform::FrameView;
use paperpaw_x4::remote::{Command, hex_bytes, part_state, runs};
use paperpaw_x4::upload::Upload;

use crate::usb_lines::{TooLong, UsbLines};

/// A file is written in pieces this large.
const CHUNK: usize = 4096;
/// Without data for this long, a file is given up.
const QUIET_MS: u64 = 5000;

/// The commands from USB (the lines of [`UsbLines`] that are not the
/// cable transfer's).
#[derive(Default)]
pub struct Remote;

impl Remote {
    /// The command a line says, if it says one.
    pub fn command(&mut self, line: Result<Vec<u8>, TooLong>) -> Option<Command> {
        let Ok(line) = line else {
            error("line too long");
            return None;
        };
        let line = String::from_utf8_lossy(&line);
        let command = paperpaw_x4::remote::parse(&line);
        if command.is_none() {
            error(&line);
        }
        command
    }

    /// Answers a command that only asks (the screen, a screenshot, what it
    /// shows, the heap) or brings a file; the others it gives back for the
    /// main loop.
    pub fn answer(
        &mut self,
        kernel: &mut Kernel,
        lines: &mut UsbLines,
        command: Command,
    ) -> Option<Command> {
        match command {
            Command::Shot => shot(kernel),
            Command::Describe => describe(kernel),
            Command::Screen => {
                let id = kernel.active_screen().map_or("", |id| id.as_str());
                println!("PP:SCREEN {id}");
            }
            Command::Heap => {
                let (left, size) = crate::stack::left();
                println!(
                    "PP:HEAP {} {} {left} {size} {} {}",
                    esp_alloc::HEAP.used(),
                    esp_alloc::HEAP.free(),
                    crate::largest_block(),
                    crate::peak::peak()
                );
            }
            Command::Put { size, path } => self.receive(kernel, lines, size, &path),
            Command::Get(path) => send(kernel, &path),
            Command::Hang(seconds) => {
                println!("PP:HANGING");
                let until = esp_hal::time::Instant::now()
                    + esp_hal::time::Duration::from_secs(seconds.min(60));
                while esp_hal::time::Instant::now() < until {}
                ok();
            }
            Command::Nap(seconds) => {
                println!("PP:NAPPING");
                crate::power::try_long_nap(seconds.min(60));
            }
            Command::Boot => {
                let (place, naps, reason) = crate::power::last_run();
                println!("PP:BOOT {reason:?} {place} {naps}");
                println!("PP:NAPS {}", crate::power::naps());
                if let Some(message) = critical_section::with(|cs| PANIC.borrow_ref(cs).clone()) {
                    println!("PP:PANIC {message}");
                }
            }
            Command::Profile(true) => {
                crate::input::profile(true);
                ok();
            }
            Command::Profile(false) => {
                let places = crate::input::profile(false).unwrap_or_default();
                let mut line = String::new();
                for chunk in places.chunks(24) {
                    line.clear();
                    for pc in chunk {
                        let _ = write!(line, " {pc:x}");
                    }
                    println!("PP:PC{line}");
                }
                println!("PP:END");
            }
            Command::Trace(true) => {
                crate::traced::trace(true);
                ok();
            }
            Command::Trace(false) => {
                for line in crate::traced::trace(false).unwrap_or_default() {
                    println!("PP:IO {line}");
                }
                println!("PP:END");
            }
            Command::Peek { at, words } => {
                let mut line = String::new();
                for start in (0..words).step_by(16) {
                    line.clear();
                    for i in start..(start + 16).min(words) {
                        let address = (at + i * 4) as *const u32;
                        // SAFETY: an aligned word of the ROM (`parse` lets
                        // no other through), always mapped.
                        let word = unsafe { core::ptr::read_volatile(address) };
                        let _ = write!(line, " {word:x}");
                    }
                    println!("PP:MEM {:x}{line}", at + start * 4);
                }
                println!("PP:END");
            }
            Command::Input => {
                let (samples, front, side) = crate::input::stats();
                let power = crate::input::power_down();
                let millivolts = crate::input::millivolts();
                println!("PP:INPUT {samples} {front} {side} {power} {millivolts}");
            }
            Command::Setting(id) => match kernel.settings().get(&id) {
                Some(_) if is_secret(kernel, &id) => println!("PP:SETTING •"),
                Some(value) => println!("PP:SETTING {}", value.to_config_text()),
                None => error("no such setting"),
            },
            Command::List(path) => list(kernel, &path),
            Command::Stat(path) => {
                let storage = kernel.storage();
                if let Ok(size) = storage.size(&path) {
                    println!("PP:STAT f {size}");
                } else if storage.list(&path, &mut |_| {}).is_ok() {
                    println!("PP:STAT d 0");
                } else {
                    println!("PP:STAT - 0");
                }
            }
            Command::Md5(path) => md5(kernel, &path),
            Command::Remove(path) => {
                let storage = kernel.storage();
                let removed = if storage.size(&path).is_ok() {
                    storage.remove(&path)
                } else {
                    storage.remove_dir_all(&path)
                };
                match removed.and_then(|()| storage.sync()) {
                    Ok(()) => ok(),
                    Err(e) => error(&alloc::format!("{path}: {e:?}")),
                }
            }
            Command::MakeDir(path) => match kernel.storage().create_dir(&path) {
                Ok(()) => ok(),
                Err(e) => error(&alloc::format!("{path}: {e:?}")),
            },
            Command::Move { from, to } => {
                let storage = kernel.storage();
                match storage.rename(&from, &to).and_then(|()| storage.sync()) {
                    Ok(()) => ok(),
                    Err(e) => error(&alloc::format!("{from}: {e:?}")),
                }
            }
            Command::Sync => match kernel.storage().sync() {
                Ok(()) => ok(),
                Err(e) => error(&alloc::format!("{e:?}")),
            },
            Command::Version => {
                println!(
                    "PP:VERSION {} {}",
                    env!("PAPERPAW_VERSION"),
                    kernel.device().id
                );
                if let Some(list) =
                    paperpaw_sdk::firmware::Plugins::read_static(&crate::ota::PLUGINS)
                {
                    for c in list.iter() {
                        println!("PP:CRATE {} {}", c.name, c.version);
                    }
                }
                println!("PP:END");
            }
            Command::Plugins => {
                for plugin in kernel.plugins().iter() {
                    let m = &plugin.manifest;
                    let on = if plugin.enabled { "on" } else { "off" };
                    let code = match (plugin.kind, plugin.problem) {
                        (paperpaw_sdk::PluginKind::Code, Some(problem)) => {
                            alloc::format!(" code {problem:?}")
                        }
                        (paperpaw_sdk::PluginKind::Code, None) => String::from(" code"),
                        _ => String::new(),
                    };
                    println!("PP:PLUGIN {} {on} {}{code}", m.id, m.version);
                }
                println!("PP:END");
            }
            Command::Native => {
                crate::native::describe(&mut |line| println!("PP:NATIVE {line}"));
                println!("PP:END");
            }
            Command::Actions(prefix) => {
                for action in kernel.actions().iter() {
                    if action.id().as_str().starts_with(prefix.as_str()) {
                        println!("PP:ACTION {}", action.id());
                    }
                }
                println!("PP:END");
            }
            Command::Settings(prefix) => {
                let settings = kernel.settings();
                for spec in settings.specs() {
                    let id = spec.id.as_str();
                    if id.starts_with(prefix.as_str())
                        && let Some(value) = settings.get(id)
                    {
                        if spec.extra().secret {
                            println!("PP:VALUE {id} •");
                        } else {
                            println!("PP:VALUE {id} {}", value.to_config_text());
                        }
                    }
                }
                println!("PP:END");
            }
            Command::Screens => {
                let mut line = String::from("PP:SCREENS");
                for id in kernel.screens() {
                    let _ = write!(line, " {id}");
                }
                println!("{line}");
            }
            Command::Usage => {
                let u = kernel.usage();
                let r = u.refreshes;
                let (fast, partial, full) = (r.fast, r.partial, r.full);
                println!(
                    "PP:USAGE {fast} {partial} {full} {} {} {} {}",
                    u.pixels, u.inputs, u.sleeps, u.panel_ms
                );
                for p in &kernel.plugin_usage() {
                    let r = p.refreshes;
                    let (fast, partial, full) = (r.fast, r.partial, r.full);
                    println!(
                        "PP:USE {} {} {} {} {fast} {partial} {full} {} {}",
                        p.plugin, p.busy_us, p.calls, p.shown_ms, p.read, p.written
                    );
                }
                println!("PP:END");
            }
            Command::Time => {
                let now = crate::now();
                match crate::clock::unix_ms(now) {
                    Some(ms) => println!("PP:TIME {ms} {}", now.0),
                    None => println!("PP:TIME - {}", now.0),
                }
            }
            Command::Log(level) => {
                let level = level.parse().unwrap_or(log::LevelFilter::Info);
                // SAFETY: nothing else sets the level, and interrupts are
                // held meanwhile (the chip has no atomics for the safe one).
                critical_section::with(|_| unsafe { log::set_max_level_racy(level) });
                ok();
            }
            Command::Panic(message) => panic!("asked over USB: {message}"),
            command @ (Command::Press { .. }
            | Command::Run(_)
            | Command::Set { .. }
            | Command::Open(_)
            | Command::Refresh { .. }
            | Command::Gray(_)
            | Command::IdleSleep(_)
            | Command::Network(_)
            | Command::Emit { .. }
            | Command::Type { .. }
            | Command::Slot { .. }
            | Command::Show { .. }
            | Command::Touch { .. }
            | Command::Drag { .. }
            | Command::Clicks { .. }
            | Command::GrayTone(_)
            | Command::SetTime(_)
            | Command::Restart) => return Some(command),
        }
        None
    }

    /// Writes a file of `size` bytes to `path` from the lines `data <hex>`
    /// that follow, a piece at a time.
    fn receive(&mut self, kernel: &Kernel, lines: &mut UsbLines, size: usize, path: &str) {
        let storage = kernel.storage();
        // To a file beside the target, moved over it only when whole (#106):
        // a put that is cut off leaves the old file as it was.
        let mut upload = match Upload::begin(storage, path) {
            Ok(upload) => upload,
            Err(e) => return println!("PP:ERR {path}: {e:?}"),
        };
        println!("PP:READY");
        let millis = || {
            esp_hal::time::Instant::now()
                .duration_since_epoch()
                .as_millis()
        };
        let mut piece = Vec::with_capacity(CHUNK);
        let (mut got, mut last) = (0, millis());
        while got < size {
            crate::power::feed();
            let Some(line) = lines.next_line() else {
                if millis() - last > QUIET_MS {
                    return println!("PP:ERR {path}: {got} of {size} bytes came");
                }
                continue;
            };
            last = millis();
            let Some(bytes) = line
                .ok()
                .and_then(|line| line.strip_prefix(b"data ").and_then(hex_bytes))
            else {
                return println!("PP:ERR {path}: not data");
            };
            got += bytes.len();
            piece.extend_from_slice(&bytes);
            if piece.len() >= CHUNK || got >= size {
                if let Err(e) = upload.push(&piece) {
                    return println!("PP:ERR {path}: {e:?}");
                }
                piece.clear();
            }
        }
        // On the card, in its place, before saying so: the X4 may be reset
        // right after.
        if let Err(e) = upload.finish(size as u64) {
            return println!("PP:ERR {path}: {e:?}");
        }
        println!("PP:OK {got}");
    }
}

/// Lists the folder `path` of the storage.
fn list(kernel: &Kernel, path: &str) {
    let listed = kernel.storage().list(path, &mut |entry| {
        let kind = if entry.is_dir { "d" } else { "f" };
        println!("PP:ENTRY {kind} {} {}", entry.size, entry.name);
    });
    match listed {
        Ok(()) => println!("PP:END"),
        Err(e) => error(&alloc::format!("{path}: {e:?}")),
    }
}

/// Says the MD5 of the file `path` of the storage, and its size.
fn md5(kernel: &Kernel, path: &str) {
    let storage = kernel.storage();
    let mut digest = paperpaw_digest::md5::Md5::new();
    let mut buf = alloc::vec![0u8; CHUNK];
    let mut at = 0u64;
    loop {
        crate::power::feed();
        match storage.read_at(path, at, &mut buf) {
            Ok(0) => break,
            Ok(n) => {
                digest.update(&buf[..n]);
                at += n as u64;
            }
            Err(e) => return error(&alloc::format!("{path}: {e:?}")),
        }
    }
    println!("PP:MD5 {} {at}", digest.hex());
}

/// Sends the file `path` of the storage.
fn send(kernel: &Kernel, path: &str) {
    let storage = kernel.storage();
    let Ok(size) = storage.size(path) else {
        return println!("PP:ERR {path}: not found");
    };
    println!("PP:SIZE {size}");
    let mut buf = [0u8; 256];
    let mut line = String::with_capacity(buf.len() * 2);
    let mut at = 0u64;
    while at < size {
        crate::power::feed();
        let Ok(n) = storage.read_at(path, at, &mut buf) else {
            return println!("PP:ERR {path}: not readable");
        };
        if n == 0 {
            break;
        }
        line.clear();
        for byte in &buf[..n] {
            let _ = write!(line, "{byte:02x}");
        }
        println!("PP:DATA {line}");
        at += n as u64;
    }
    println!("PP:END");
}

/// The last run's panic, said by `boot`.
static PANIC: critical_section::Mutex<core::cell::RefCell<Option<String>>> =
    critical_section::Mutex::new(core::cell::RefCell::new(None));

/// Keeps the last run's panic for `boot`.
pub fn note_panic(message: Option<String>) {
    critical_section::with(|cs| PANIC.replace(cs, message));
}

/// Done.
pub fn ok() {
    println!("PP:OK");
}

/// Not now, e.g. a press while another is held.
pub fn busy() {
    println!("PP:BUSY");
}

/// Not understood, or not done.
pub fn error(what: &str) {
    println!("PP:ERR {what}");
}

/// The panel refreshed from what its controller holds, and how it went.
pub fn refreshed(result: impl core::fmt::Debug) {
    println!("PP:OK {result:?}");
}

/// The networks a scan asked over USB found.
pub fn networks(networks: &[WifiNetwork]) {
    for net in networks {
        println!("PP:NET {} {} {}", net.signal, net.secured, net.ssid);
    }
    println!("PP:END");
}

/// The network asked over USB was joined.
pub fn connected(ssid: &str, address: &str) {
    println!("PP:CONNECTED {ssid}");
    println!("PP:IP {address}");
}

/// The network asked over USB was not joined.
pub fn not_connected(ssid: &str, reason: JoinFailure) {
    error(&alloc::format!("wifi connect {ssid}: {}", reason.as_str()));
}

pub fn wifi_state(state: &WifiState) {
    match state {
        WifiState::Off => println!("PP:WIFI off"),
        WifiState::On => println!("PP:WIFI on"),
        WifiState::Joining { ssid } => println!("PP:WIFI joining {ssid}"),
        WifiState::Online { ssid, address, .. } => println!("PP:WIFI online {ssid} {address}"),
        WifiState::Hosting {
            ssid,
            address,
            guests,
        } => println!("PP:WIFI hosting {ssid} {address} {guests}"),
        _ => println!("PP:WIFI other"),
    }
}

/// The answer of a request asked over USB.
pub fn http_answer(answer: &HttpResponse, ms: u64) {
    let why = match answer.error {
        Some(error) => alloc::format!("{error}: {}", String::from_utf8_lossy(&answer.body)),
        None if answer.status == 0 => String::from_utf8_lossy(&answer.body).into_owned(),
        None => String::new(),
    };
    println!(
        "PP:HTTP {} {} saved {} {ms} ms heap {} {} {why}",
        answer.status,
        answer.body.len(),
        answer.saved,
        esp_alloc::HEAP.used(),
        esp_alloc::HEAP.free()
    );
}

/// Sends the whole screen, as turned (screens may have less of it), or
/// `PP:BUSY` while changes wait to be shown (ask again).
fn shot(kernel: &mut Kernel) {
    let display = kernel.device().display;
    let header = (
        display.width,
        display.height,
        display.format.bits_per_pixel(),
    );
    let mut started = false;
    let mut hex = String::new();
    let done = kernel.capture(&mut |top, rows: FrameView<'_>| {
        if !started {
            println!("PP:FRAME {} {} {}", header.0, header.1, header.2);
            started = true;
        }
        for y in 0..rows.height {
            let at = usize::from(y) * rows.stride;
            let row = rows.data.get(at..at + rows.stride).unwrap_or(&[]);
            hex.clear();
            runs(row, &mut hex);
            println!("PP:ROW {} {hex}", top + y);
        }
    });
    if done {
        println!("PP:END");
    } else {
        busy();
    }
}

/// Whether setting `id` is a secret, which is never sent.
fn is_secret(kernel: &Kernel, id: &str) -> bool {
    kernel
        .settings()
        .spec(id)
        .is_some_and(|spec| spec.extra().secret)
}

/// Sends what the screen shows, part by part.
fn describe(kernel: &mut Kernel) {
    // A PIN or a password being entered: where the parts are, not what
    // they say.
    let secret = kernel.is_secret();
    for part in kernel.describe() {
        let state = part_state(part.focused, part.on, part.disabled);
        let a = part.area;
        let label = if secret {
            String::from("•")
        } else {
            part.label.replace('\n', " ")
        };
        println!(
            "PP:PART {:?} {} {} {} {} {state} {label}",
            part.role, a.x, a.y, a.width, a.height
        );
    }
    println!("PP:END");
}
