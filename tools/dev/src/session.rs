//! One emulated device in a variant, for the checks to drive: a fresh
//! library (the sample books, the data plugins looked at, the first-run
//! setup done), the standard app with the plugins added, simulated time.
//! The library is a folder in the output folder, or kept in memory
//! ([`Context::in_memory`], e.g. in a browser).

use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};

use paperpaw_core::sdk::builtin::actions;
use paperpaw_core::sdk::semantics::{Described, Part};
use paperpaw_core::sdk::{Id, Params};
use paperpaw_emulator::device::Device;
use paperpaw_emulator::{budget, headless, trace};
use paperpaw_platform::{Instant, Storage};

use crate::cli::Context;
use crate::image::Image;
use crate::variant::{self, Variant};

/// The books every library starts with (from `samples/library`).
const BOOKS: &[(&str, &[u8])] = &[
    (
        "books/alice-in-wonderland.txt",
        include_bytes!("../../../samples/library/books/alice-in-wonderland.txt"),
    ),
    (
        "books/alice-in-wonderland.epub",
        include_bytes!("../../../samples/library/books/alice-in-wonderland.epub"),
    ),
    (
        "books/die-verwandlung.epub",
        include_bytes!("../../../samples/library/books/die-verwandlung.epub"),
    ),
];

/// A book to open in sessions that read.
pub const BOOK: &str = "books/alice-in-wonderland.epub";

/// Numbers the libraries of one run.
static NEXT: AtomicUsize = AtomicUsize::new(0);

/// How a session starts.
#[derive(Debug, Clone, Default)]
pub struct Start {
    /// Settings besides the variant's, as `(id, value)`.
    pub settings: Vec<(String, String)>,
    /// Leave the first-run setup to show (by default it is done).
    pub setup: bool,
    /// Files to put into the library besides the books, as `(path, bytes)`.
    pub files: Vec<(String, Vec<u8>)>,
    /// What the storage says it has free, in bytes (the computer's disk
    /// where `None`); for a library kept in a folder.
    pub free_space: Option<u64>,
}

/// A device running the app in a variant.
#[derive(Debug)]
pub struct Session {
    pub device: Device,
    pub variant: Variant,
    /// The library's folder, unless it is kept in memory.
    root: Option<PathBuf>,
    keep: bool,
    /// Requests answered from the emulator's fixtures (`--network`).
    fixtures: bool,
}

impl Session {
    /// A session as most checks want it: the variant, nothing else.
    pub fn new(context: &Context, variant: &Variant) -> Result<Session, String> {
        Self::start(context, variant, &Start::default())
    }

    pub fn start(context: &Context, variant: &Variant, start: &Start) -> Result<Session, String> {
        let mut files: Vec<(String, Vec<u8>)> = BOOKS
            .iter()
            .map(|(path, bytes)| (path.to_string(), bytes.to_vec()))
            .collect();
        if !start.setup {
            files.push((".paperpaw/plugins/setup/done".into(), Vec::new()));
        }
        files.extend(start.files.iter().cloned());
        if variant.language == crate::lint::PSEUDO {
            files.extend(crate::lint::pseudo_plugin(context)?);
        }
        // The data plugins' files, written from where they are kept.
        let plugin_files = context.target.data.iter().flat_map(|plugin| {
            plugin
                .files
                .iter()
                .map(|(path, bytes)| (format!("plugins/{}/{path}", plugin.id), &**bytes))
        });
        let written: Vec<(String, &[u8])> = files
            .iter()
            .map(|(path, bytes)| (path.clone(), bytes.as_slice()))
            .chain(plugin_files)
            .collect();
        // No first-time hints over the screens looked at, unless asked.
        let mut settings: Vec<(&str, &str)> = vec![("help.show", "false")];
        settings.extend(variant.settings());
        // What the command line set (`--set`), for every command.
        settings.extend(
            context
                .settings
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_str())),
        );
        settings.extend(start.settings.iter().map(|(k, v)| (k.as_str(), v.as_str())));
        let config = variant::config(settings);
        // The budget is compared after (see `perf`), rather than making
        // the run fail where the device would run out; frames are kept
        // for the checks that look at them.
        budget::override_limit(None);
        crate::probe::set_target(context.target.add);
        trace::record();
        drop(trace::take_frames());
        let profile = variant.profile;
        let add = Some(crate::probe::add as paperpaw_emulator::device::AddPlugins);
        let (device, root) = if context.in_memory {
            let library = paperpaw_core::sdk::MemoryStorage::new();
            for (path, bytes) in &written {
                library
                    .write(path, bytes)
                    .map_err(|e| format!("{path}: {e:?}"))?;
            }
            let device = Device::in_memory(profile, Some(config), Rc::new(library), true, add);
            (device, None)
        } else {
            let n = NEXT.fetch_add(1, Ordering::Relaxed);
            let root = context
                .out
                .join("work")
                .join(format!("{n:04}-{}", variant.slug()));
            let _ = std::fs::remove_dir_all(&root);
            for (path, bytes) in &written {
                let file = root.join(path);
                if let Some(parent) = file.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| format!("{}: {e}", parent.display()))?;
                }
                std::fs::write(&file, bytes).map_err(|e| format!("{}: {e}", file.display()))?;
            }
            let storage = paperpaw_fs::DirStorage::new(&root)
                .map_err(|e| format!("library {}: {e}", root.display()))?;
            let storage = match start.free_space {
                Some(bytes) => storage.with_free_space(bytes),
                None => storage,
            };
            let device = Device::with_plugins(profile, Some(config), storage, true, add);
            (device, Some(root))
        };
        let mut device = device.map_err(|e| format!("{}: {e}", variant.name()))?;
        // A variation's own network stays (see `vary`).
        if context.fixtures {
            device.use_fixtures();
        }
        device.start();
        device.tick_if_due();
        let mut session = Session {
            device,
            variant: variant.clone(),
            root,
            keep: context.keep,
            fixtures: context.fixtures,
        };
        session.settle();
        Ok(session)
    }

    /// The device's storage, as its kernel has it.
    pub fn storage(&self) -> &dyn Storage {
        self.device.kernel.storage()
    }

    /// Lets what is due happen, up to a few seconds of emulated time
    /// (screens that open in steps, books being prepared).
    pub fn settle(&mut self) {
        let end = self.device.now().add_millis(3000);
        for _ in 0..1000 {
            let Some(at) = self.device.next_wake() else {
                return;
            };
            if at > end {
                return;
            }
            let now = self.device.now();
            self.device.advance(at.since(now).max(1));
            self.device.tick_if_due();
        }
    }

    /// Runs script steps as `paperpaw-emu --script` does (see
    /// `shells/desktop/src/headless.rs`).
    pub fn script(&mut self, steps: &str) -> Result<(), String> {
        headless::run_script(&mut self.device, steps)?;
        self.settle();
        Ok(())
    }

    /// Opens a screen with parameters, as a plugin would.
    pub fn open(&mut self, screen: &str, params: &[(&str, &str)]) -> Result<(), String> {
        let id = Id::parse(screen).map_err(|e| format!("{screen}: {e}"))?;
        let mut given = Params::new();
        for (key, value) in params {
            given = given.with(key.to_string(), *value);
        }
        self.device.open_screen(id, given);
        self.settle();
        self.settle_network();
        Ok(())
    }

    /// Whether requests are answered from the emulator's fixtures.
    pub fn fixtures(&self) -> bool {
        self.fixtures
    }

    /// With the fixtures, lets emulated time pass while network work is
    /// under way (the radio joining, requests answered), up to as long as
    /// a request waits for a network: what a screen shows once its data
    /// came is what is looked at. Without, nothing (the computer's
    /// network answers in its own time).
    pub fn settle_network(&mut self) {
        if !self.fixtures {
            return;
        }
        let end = self
            .device
            .now()
            .add_millis(paperpaw_core::sdk::wifi::NETWORK_WAIT_MS + 1000);
        while self.device.network_settling() && self.device.now() < end {
            let now = self.device.now();
            let next = self.device.next_wake().filter(|&at| at > now);
            let step = next.map_or(100, |at| at.since(now).clamp(1, 100));
            self.device.advance(step);
            self.device.tick_if_due();
        }
    }

    /// Changes a setting as the settings screen would.
    pub fn set(&mut self, id: &str, value: &str) -> Result<(), String> {
        let mut result = Ok(());
        self.device
            .call(|kernel, now| match kernel.set_setting(now, id, value) {
                Ok(effects) => effects,
                Err(e) => {
                    result = Err(format!("{id} = {value}: {e}"));
                    paperpaw_core::Effects::default()
                }
            });
        self.settle();
        result
    }

    /// Runs an action, as a button bound to it would.
    pub fn run(&mut self, action: &str) -> Result<(), String> {
        let id = Id::parse(action).map_err(|e| format!("{action}: {e}"))?;
        self.device.run_action(id);
        self.settle();
        Ok(())
    }

    /// Goes back to the home screen as one would: *Home*, else *Back* (a
    /// screen may keep *Home* to itself, as the keyboard does while a
    /// password is typed) and *Home* again; what was still open if that
    /// did not reach it.
    pub fn go_home(&mut self) -> Result<(), String> {
        self.run(actions::HOME.as_str())?;
        for _ in 0..self.stack().len() {
            if self.stack().len() == 1 {
                return Ok(());
            }
            self.run(actions::BACK.as_str())?;
            self.run(actions::HOME.as_str())?;
        }
        let stack = self.stack();
        if stack.len() == 1 {
            return Ok(());
        }
        Err(format!(
            "neither Home nor Back reached the home screen ({} open: {})",
            stack.len(),
            stack.join(", ")
        ))
    }

    /// Presses a button of the device (short).
    pub fn press(&mut self, button: &str) -> Result<(), String> {
        self.script(button)
    }

    /// The screen shown.
    pub fn screen(&self) -> Option<String> {
        self.device.kernel.active_screen().map(ToString::to_string)
    }

    /// The screens open, the one shown last.
    pub fn stack(&self) -> Vec<String> {
        self.device
            .kernel
            .screens()
            .map(ToString::to_string)
            .collect()
    }

    /// What the panel shows.
    pub fn image(&self) -> Image {
        Image::from_rgb(
            u32::from(self.device.panel.width()),
            u32::from(self.device.panel.height()),
            self.device.panel.rgb_bytes(),
        )
    }

    /// What the screen shows, part by part; while it is secret (a PIN or
    /// a password being entered) without what the parts say.
    pub fn parts(&mut self) -> Vec<Part> {
        let secret = self.device.kernel.is_secret();
        let mut parts = self.device.kernel.describe();
        if secret {
            for part in &mut parts {
                part.label = String::from("•");
            }
        }
        parts
    }

    /// Everything the drawing helpers told as they drew, with clips;
    /// while it is secret without what they say.
    pub fn described(&mut self) -> Vec<Described> {
        let secret = self.device.kernel.is_secret();
        let mut described = self.device.kernel.described();
        if secret {
            for d in &mut described {
                d.label = String::from("•");
            }
        }
        described
    }

    /// The frames drawn since the last call (time, storage calls by band).
    /// Recording starts with the first session.
    pub fn frames(&self) -> Vec<trace::FrameRecord> {
        trace::take_frames()
    }

    pub fn now(&self) -> Instant {
        self.device.now()
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if let Some(root) = &self.root
            && !self.keep
        {
            let _ = std::fs::remove_dir_all(root);
        }
    }
}
