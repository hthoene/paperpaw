//! The PaperPaw kernel.
//!
//! The kernel hosts plugins and connects them: it keeps the registries of
//! contributions, resolves slots to screens, turns raw input into gestures
//! and actions through layered key maps, applies plugin commands and decides
//! how the e-ink panel is refreshed. It contains no product UI: even its own
//! contributions are registered as the `core` plugin.
//!
//! The kernel is *sans-I/O*: shells feed it [`InputEvent`]s and time, and
//! execute the returned [`Effects`]. It never blocks, spawns threads or
//! talks to an operating system, so it runs unchanged on microcontrollers,
//! Linux, Android and in the browser.
//!
//! Drawing is separate from event handling: after a call whose effects
//! contain a refresh, the shell calls [`Kernel::render`], which draws what
//! changed and hands it over as rows of pixels: the whole region at once,
//! or band by band on devices that cannot hold a full frame in RAM
//! ([`KernelBuilder::frame_rows`]). The panel controller keeps the frame.
//!
//! ```ignore
//! let mut kernel = Kernel::builder(device).plugin(MyPlugin).build()?;
//! let mut effects = kernel.start(clock.now());
//! loop {
//!     if let Some((region, mode)) = effects.refresh() {
//!         kernel.render(&mut |top, rows| panel.write(top, rows));
//!         panel.refresh(region, mode);
//!     }
//!     arm_timer(effects.wake_at());
//!     effects = match wait_for_input_or_timer() {
//!         Input(event) => kernel.handle(clock.now(), event),
//!         Timer => kernel.tick(clock.now()),
//!     };
//! }
//! ```

#![no_std]

extern crate alloc;

mod awake;
mod bluetooth;
mod builder;
mod card;
mod chrome;
mod code_plugin;
mod commands;
mod config;
mod core_plugin;
mod data_plugin;
mod drawing;
mod effects;
mod gestures;
mod hints;
mod input;
mod io;
mod keymap;
mod meter;
mod network;
mod plugins;
mod realtime;
mod recall;
mod refresh;
mod registry;
mod room;
mod screens;
#[cfg(test)]
mod tests;
mod work;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt;
use core::mem;

use paperpaw_graphics::{Framebuffer, GraphicsError};
use paperpaw_i18n::Translator;
use paperpaw_platform::{
    ButtonState, Color, DeviceInfo, FrameView, InputEvent, Instant, NoStorage, Point, PointerEvent,
    PointerPhase, Rect, RefreshMode, Storage, StorageError, SystemStatus,
};
use paperpaw_sdk::builtin::{
    CONFIG_FILE, GLOBAL_KEYMAP_LAYER, events, keymap_presets, params as builtin_params,
    settings as core_settings, slots,
};
use paperpaw_sdk::event::EventData;
use paperpaw_sdk::native::{Native, SafeMode};
use paperpaw_sdk::setting::{Setting, SettingType};
use paperpaw_sdk::{
    Actions, Capabilities, Command, Contribution, Cx, Direction, Env, EnvParts, Fonts, Formats,
    Gesture, GestureKind, Handled, HintVisibility, Hyphenation, Id, Manifest, Menus, Metrics,
    Output, Params, Plugin, PluginInfo, PluginState, Plugins, RegisterError, RequiredFor, Screen,
    Settings, ShowsThrough, StatusItems,
};

pub use crate::builder::KernelBuilder;
pub use crate::card::CardTrouble;
pub use crate::code_plugin::{CodeFile, CodeLoader, code_to_load};
use crate::config::{mono_coverage, plugin_switches, read_config};
use crate::core_plugin::{CORE, CorePlugin};
pub use crate::effects::{Effect, Effects};
use crate::input::{Recognized, Recognizer, Thresholds};
pub use crate::io::cable::PREFIX as CABLE_PREFIX;
pub use crate::io::{
    Advert, Bond, BtCall, BtReport, Found, HttpCall, HttpResponse, Io, MAX_FILE, MAX_RESPONSE,
    PairQuestion, SOCKET_PART, ServedCharacteristic, ServedFile, ServedService, ServerCall,
    ServerEvent, SocketCall, SocketEvent, SpeechCall, WifiEvent,
};
use crate::keymap::{InputKey, Keymap, Origin, Zone};
use crate::refresh::RefreshPolicy;
use crate::registry::{Registry, Staging};
use crate::screens::uncovered;
pub use crate::work::NetworkSettings;
#[doc(hidden)]
pub use crate::work::network_specs_of;
use paperpaw_sdk::Registrar;
use paperpaw_sdk::filters::InputVerdict;
pub use paperpaw_sdk::firmware::{Firmware, FirmwareCommand, FirmwareState, InstallFailure};
pub use paperpaw_sdk::wifi::{JoinFailure, WifiCommand, WifiNetwork, WifiState};

pub use paperpaw_sdk as sdk;

/// Commands may trigger further commands; this bounds runaway loops.
const MAX_COMMAND_ROUNDS: usize = 16;
/// A frame with pictures over this share of the screen (percent) is left
/// with a full refresh: e-paper keeps ghosts of large pictures where text
/// leaves none. Covers in a list stay below it.
const PICTURES_PERCENT: u32 = 15;
/// Of the screen, in percent, pictures of any size cover at least for it
/// to be shown in grays when drawn whole
/// ([`settings::PICTURES_IN_GRAYS`](paperpaw_sdk::builtin::settings::PICTURES_IN_GRAYS)):
/// a cover or two, not an icon.
const PICTURES_GRAY_PERCENT: u32 = 2;
/// How long the row of hints shows on a screen that has it on a press.
const HINTS_MS: u64 = 4000;
/// Work that took this long last time (opening a screen, before it is
/// drawn) shows the busy mark first (see `KernelBuilder::busy_mark`):
/// about as long as the panel takes to show the mark, which it does
/// meanwhile, so the mark costs next to nothing.
const SLOW_MS: u32 = 400;
/// What was timed is remembered for this many things at most.
const TIMED: usize = 24;
/// Largest file of passwords and keys read.
const MAX_SECRETS: usize = 64 * 1024;
/// Default memory for glyphs rasterized at runtime.
const DEFAULT_GLYPH_CACHE: usize = 32 * 1024;

/// Where a warning comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum WarningSource {
    Plugin(Id),
    DeviceKeymap,
    UserConfig,
    Runtime,
}

/// A non-fatal problem. PaperPaw keeps running with sensible defaults and
/// reports what it ignored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warning {
    pub source: WarningSource,
    pub line: Option<u32>,
    pub message: String,
}

impl fmt::Display for Warning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.source {
            WarningSource::Plugin(id) => write!(f, "plugin `{id}`")?,
            WarningSource::DeviceKeymap => f.write_str("device key map")?,
            WarningSource::UserConfig => f.write_str("user configuration")?,
            WarningSource::Runtime => f.write_str("runtime")?,
        }
        if let Some(line) = self.line {
            write!(f, ", line {line}")?;
        }
        write!(f, ": {}", self.message)
    }
}

/// Errors that prevent the kernel from starting.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum KernelError {
    Graphics(GraphicsError),
}

impl fmt::Display for KernelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KernelError::Graphics(error) => write!(f, "cannot create framebuffer: {error}"),
        }
    }
}

impl core::error::Error for KernelError {}

/// A screen on the navigation stack.
struct ActiveScreen {
    id: Id,
    /// Tells this opening of the screen from every other, also once it was
    /// made anew (see `Kernel::remake_screens`): e.g. the owner of a
    /// settings scope.
    serial: u32,
    /// The slot it was opened for, if any.
    slot: Option<Id>,
    /// Made for the part above the row of hints (when the row shows), as
    /// it asked ([`HintVisibility::Shown`]).
    hinted: bool,
    /// The part of the screen it is made for and drawn into: all of it
    /// but the bars shown over it and the row of hints (see
    /// [`Env::content`]).
    area: Rect,
    /// What it was opened with, e.g. the book.
    params: Params,
    screen: Box<dyn Screen>,
    /// When it wants [`Screen::on_timer`] ([`Cx::wake_at`]); below another
    /// screen, once it is on top again.
    timer: Option<Instant>,
    /// The key map layer put in front of its slot's while it is on top
    /// ([`Command::SetKeyLayer`]).
    key_layer: Option<Id>,
    /// Opened above a screen guarding the device by that screen (or one
    /// it opened): its keyboard. Others above it (a sleep screen) get no
    /// input and are closed as the device wakes.
    by_guard: bool,
}

/// An open screen kept while its task is not shown
/// ([`Command::SwitchTask`]): its id, slot and params.
type KeptScreen = (Id, Option<Id>, Params);

/// A finger that came down at an edge (see [`Kernel::edge_lifted`]).
#[derive(Debug, Clone, Copy)]
struct EdgeTouch {
    id: u8,
    side: Direction,
    start: Point,
    tool: paperpaw_platform::PointerTool,
}

/// The PaperPaw kernel. See the [crate documentation](crate).
pub struct Kernel {
    shared: Shared,
    framebuffer: Framebuffer,
    subscriptions: crate::registry::Subscriptions,
    /// The lowest screen the last offer of input reached ([`Kernel::offer`]).
    offered_down_to: usize,
    /// The params of the action offered to the screens now, for their
    /// context ([`Cx::params`](paperpaw_sdk::Cx::params)); none else.
    action_params: Params,
    /// What sees every action run and every event emitted.
    action_observers: Vec<paperpaw_sdk::ObserverSpec>,
    event_observers: Vec<paperpaw_sdk::ObserverSpec>,
    /// The screen on top changed: it is told ([`Screen::on_show`]) once
    /// the commands of the call are done, so that one shown for a moment
    /// (e.g. below a menu that closes to open another screen) is not.
    show_pending: bool,
    /// Work plugins declared, timers among it (see [`work`]).
    works: work::Queue,
    /// The short message showing ([`Command::Notify`]) and until when.
    notice: Option<(Box<dyn Screen>, Instant)>,
    stack: Vec<ActiveScreen>,
    output: Output,
    warnings: Vec<Warning>,
    /// A restart was requested (see [`Command::Restart`]).
    restart: bool,
    /// Sleep was requested (see [`Command::Sleep`]).
    sleep: bool,
    /// The row of hints over the buttons below the screen.
    hint: hints::HintRow,
    /// See [`KernelBuilder::busy_mark`].
    busy_mark: bool,
    /// The work held back while the mark is shown ([`Kernel::proceed`]).
    withheld: Vec<Command>,
    /// Of `withheld`, the commands of a guarding screen (see `trusted`).
    withheld_trusted: Vec<usize>,
    /// Of `withheld`, those of the guard itself (see `own_trusted`).
    withheld_own: Vec<usize>,
    /// The commands in `output.commands` (by their place) that a guarding
    /// screen ([`Screen::guards`]) or a screen above it gave: those alone
    /// open screens above it.
    trusted: Vec<usize>,
    /// The command being carried out came from a guarding screen or one
    /// above it.
    from_guard: bool,
    /// The command carried out comes from a screen guarding the device
    /// itself, or one it opened (see `own_trusted`).
    from_guard_own: bool,
    /// Of the commands asked for, those of a screen guarding the device or
    /// one it opened: what they open above it is its own.
    own_trusted: Vec<usize>,
    /// A notice given while a screen guarded the device, shown once it
    /// lets go.
    held_notice: Option<String>,
    /// When the device was last used (input, a page turned), for sleeping
    /// by itself ([`settings::SLEEP_AFTER`](core_settings::SLEEP_AFTER)).
    last_activity: Instant,
    /// Since when the screen on top has been counted as shown.
    shown_since: Instant,
    /// The active settings scope: a book's own settings.
    scope: Option<config::Scope>,
    /// The screen that asked for the values shown without being kept
    /// ([`Command::PreviewSettings`]), if any (its `serial`).
    preview_owner: Option<u32>,
    /// The task shown ([`Command::SwitchTask`]); `None` the home slot's.
    task: Option<Id>,
    /// The screens of the tasks not shown, by their ids, slots and params.
    tasks: Vec<(Id, Vec<KeptScreen>)>,
    /// The `serial` of the screen opened next.
    next_serial: u32,
    /// `paperpaw.ini` is there but could not be read at start: the user
    /// is told once the first screen shows.
    config_unreadable: bool,
    /// The shell found no card it can use: the reader is told why at
    /// each start.
    card_trouble: Option<CardTrouble>,
    /// This start ends a sleep that restarted the device (see
    /// [`KernelBuilder::after_sleep`]): [`events::RESUME`] follows
    /// [`events::STARTED`].
    after_sleep: bool,
    /// The file of passwords and keys is there but could not be read at
    /// start: it is not written over.
    secrets_unreadable: bool,
    /// The number a pairing question's answer must bring back.
    pairing_nonce: Option<u64>,
    /// A screen was opened or closed since the last call ended; then the
    /// frame drawn for it is not shown yet (see [`Kernel::shown`]).
    screens_changed: bool,
    /// Raw keys taken, the way out, frames asked (see [`realtime`]).
    raw: realtime::RawKeys,
    /// Room made for the screen that needs memory (see [`room`]).
    lease: Option<room::Lease>,
    /// Plugins' holds on the device's sleep (see [`awake`]).
    holds: awake::Holds,
    /// Setting defaults from plugins, applied once all are registered:
    /// `(plugin, setting, value)`.
    plugin_defaults: Vec<(Id, Id, Cow<'static, str>)>,
    /// Values plugins offer among other plugins' choices, added once all
    /// are registered: `(plugin, setting, value)`.
    plugin_choices: Vec<(Id, Id, Cow<'static, str>)>,
    now: Instant,
    input: gestures::InputState,
    io: network::IoState,
    frame: drawing::FrameState,
    keys: plugins::KeySources,
    /// The bars along the edges of the screen plugins added
    /// ([`paperpaw_sdk::chrome`]), the outermost first.
    chromes: Vec<chrome::Chrome>,
    /// Those shown now, by their place in `chromes`, with their strips.
    chrome_strips: Vec<(usize, Rect)>,
    /// The plugin of each of `Shared::stylesheets`, to drop them with it.
    stylesheet_owners: Vec<Id>,
    timing: drawing::Timing,
}

impl fmt::Debug for Kernel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Kernel")
            .field("device", &self.shared.device.id)
            .field("plugins", &self.shared.plugins.len())
            .field(
                "stack",
                &self.stack.iter().map(|s| &s.id).collect::<Vec<_>>(),
            )
            .finish_non_exhaustive()
    }
}

impl Kernel {
    /// How the screen should be turned, by the setting
    /// [`settings::ROTATION`](paperpaw_sdk::builtin::settings::ROTATION):
    /// quarter turns from upright. Shells that turn it (see
    /// [`KernelBuilder::rotation`]) check it after changes and build the
    /// kernel anew for the turned size.
    pub fn rotation(&self) -> paperpaw_platform::Rotation {
        use paperpaw_platform::Rotation;
        use paperpaw_sdk::builtin::settings;
        let value = self.shared.settings.read(&settings::ROTATION);
        match settings::ROTATIONS.iter().position(|r| *r == value) {
            Some(1) => Rotation::Deg90,
            Some(2) => Rotation::Deg180,
            Some(3) => Rotation::Deg270,
            _ => Rotation::Deg0,
        }
    }

    /// The colours black and white are shown in, by the settings
    /// [`settings::TEXT_COLOR`](paperpaw_sdk::builtin::settings::TEXT_COLOR)
    /// and [`settings::PAPER_COLOR`](paperpaw_sdk::builtin::settings::PAPER_COLOR),
    /// as red, green and blue; the grays between lie between them. Shells
    /// that tint the screen (see [`KernelBuilder::colors`]) show each gray
    /// of the frame so; the frame does not change when they do.
    pub fn colors(&self) -> [[u8; 3]; 2] {
        use paperpaw_sdk::builtin::settings;
        use paperpaw_sdk::setting::{Setting, Text};
        let text = |setting: &Setting<Text>| self.shared.settings.text(setting.as_str());
        // A preset, or one's own colour code; black and white otherwise.
        let rgb = |id: &Setting<Text>, own: &str, code: &Setting<Text>, fallback: [u8; 3]| {
            let value = text(id);
            if value == own {
                settings::parse_rgb(text(code))
            } else {
                settings::rgb(value)
            }
            .unwrap_or(fallback)
        };
        [
            rgb(
                &settings::TEXT_COLOR,
                settings::OWN_TEXT,
                &settings::TEXT_COLOR_OWN,
                [0, 0, 0],
            ),
            rgb(
                &settings::PAPER_COLOR,
                settings::OWN_PAPER,
                &settings::PAPER_COLOR_OWN,
                [255, 255, 255],
            ),
        ]
    }

    /// Opens the home slot and emits [`events::STARTED`]. Call once after
    /// building.
    pub fn start(&mut self, now: Instant) -> Effects {
        // The kernel's work, but for the plugins' it calls.
        let _running = self.shared.meter.enter(meter::CORE);
        self.now = now;
        self.last_activity = now;
        // Replace anything the shell showed before startup, even where
        // the home screen only draws an overlay.
        self.invalidate(self.framebuffer.bounds());
        self.output.commands.push(Command::open_slot(slots::HOME));
        self.output.commands.push(events::STARTED.emit(()));
        // The device slept and restarted: it wakes as it would elsewhere.
        if mem::take(&mut self.after_sleep) {
            self.output.commands.push(events::RESUME.emit(()));
        }
        if let Some(radio) = &mut self.io.bluetooth {
            radio.start();
        }
        // The bonds read with the secrets, for every plugin to see.
        self.sync_bluetooth();
        if mem::take(&mut self.config_unreadable) {
            let text = self.config_notice();
            self.output.commands.push(Command::Notify(text));
        }
        if let Some(trouble) = self.card_trouble {
            let text = String::from(self.shared.translator.tr(trouble.key()));
            self.output.commands.push(Command::Notify(text));
        }
        self.finish()
    }

    /// Handles an input event.
    pub fn handle(&mut self, now: Instant, event: InputEvent) -> Effects {
        let _running = self.shared.meter.enter(meter::CORE);
        self.shared.reader = true;
        if self.stale(now, &event) {
            return self.finish();
        }
        self.hint.pressed = None;
        self.now = now;
        self.last_activity = now;
        // Not counted while a screen guards the device: the count would
        // tell how long a code typed into it is.
        if self.guard().is_none() {
            self.shared.usage.inputs = self.shared.usage.inputs.saturating_add(1);
        }
        // A screen above a guard that the guard did not open (a sleep
        // screen) gets no input: what is typed into the guard is its own.
        // A press or a finger put down closes it, so that the guard is
        // reached again also where the shell never resumes
        // ([`Kernel::resume`]); the release of the press that opened it
        // does not.
        if self.top_shut_out() {
            let begins = matches!(
                event,
                InputEvent::Button {
                    state: ButtonState::Pressed,
                    ..
                } | InputEvent::Pointer(PointerEvent {
                    phase: PointerPhase::Down,
                    ..
                })
            );
            if begins {
                self.close_shut_out();
            }
            return self.finish();
        }
        let event = self.page_buttons_swapped(event);
        match event {
            InputEvent::Button { name, state } => self.note_held(name, state),
            InputEvent::Pointer(pointer) => self.note_corner(&pointer, now),
            _ => {}
        }
        // Plugins' input filters come first; while a screen guards the
        // device, or a PIN or a password is entered, only those of its
        // plugin.
        let (verdict, delivered) = {
            let asking = self.secret_owner();
            let env = self.shared.env(now);
            self.shared
                .input_filters
                .filter_of(&env, &event, asking.as_deref())
        };
        let event = match verdict {
            InputVerdict::Take => {
                // Taken from the recognizers that follow the finger too.
                if let InputEvent::Pointer(p) = delivered {
                    self.recognizers_lose(&p);
                }
                return self.finish();
            }
            InputVerdict::Run(action) => {
                self.dispatch(action);
                self.run_commands();
                return self.finish();
            }
            InputVerdict::Cancel => {
                self.cancel_input(&delivered);
                self.run_commands();
                return self.finish();
            }
            InputVerdict::Replace(other) => other,
            _ => event,
        };
        self.shared.last_input = match event {
            InputEvent::Button { .. } => paperpaw_sdk::keys::InputMode::Buttons,
            InputEvent::Pointer(p)
                if matches!(
                    p.tool,
                    paperpaw_platform::PointerTool::Pen | paperpaw_platform::PointerTool::Eraser
                ) =>
            {
                paperpaw_sdk::keys::InputMode::Pen
            }
            InputEvent::Pointer(_) => paperpaw_sdk::keys::InputMode::Touch,
            _ => self.shared.last_input,
        };
        match event {
            InputEvent::Button { name, state } if self.raw_key(name, state, now) => {}
            InputEvent::Button { name, state } => {
                if state == ButtonState::Pressed {
                    self.input.pressed_on = self.stack.last().map(|top| top.serial);
                }
                // Without a long press to tell apart, at once.
                let long = InputKey::LongButton(Cow::Borrowed(name));
                let at_once = self.resolve(&long).is_none() && !self.takes_buttons();
                if let Some(recognized) = self.input.recognizer.button(name, state, now, at_once) {
                    self.recognized(recognized);
                }
            }
            InputEvent::Pointer(pointer) => self.pointer(pointer),
            _ => {}
        }
        if let Some(effects) = self.mark_if_slow() {
            return effects;
        }
        // The screens as the input left them, to say where the focus is.
        self.run_commands();
        self.speak_focus();
        self.finish()
    }

    /// `event` with the page buttons the other way round while reading,
    /// where the user asked for it ([`core_settings::SWAP_PAGE_BUTTONS`]):
    /// a button swapped when pressed stays swapped when let go, also if
    /// another screen came up meanwhile.
    fn page_buttons_swapped(&mut self, event: InputEvent) -> InputEvent {
        use paperpaw_platform::buttons::{PAGE_BACK, PAGE_FORWARD};
        let InputEvent::Button { name, state } = event else {
            return event;
        };
        if name != PAGE_FORWARD && name != PAGE_BACK {
            return event;
        }
        let swap = match state {
            ButtonState::Pressed => {
                let reading = self
                    .stack
                    .last()
                    .and_then(|top| top.slot.as_ref())
                    .is_some_and(|slot| *slot == slots::READER);
                let swap = reading && self.shared.settings.read(&core_settings::SWAP_PAGE_BUTTONS);
                self.input.swapped.retain(|b| *b != name);
                if swap {
                    self.input.swapped.push(name);
                }
                swap
            }
            _ => {
                let held = self.input.swapped.iter().position(|b| *b == name);
                held.map(|at| self.input.swapped.remove(at)).is_some()
            }
        };
        let name = match (swap, name) {
            (false, name) => name,
            (true, PAGE_FORWARD) => PAGE_BACK,
            (true, _) => PAGE_FORWARD,
        };
        InputEvent::Button { name, state }
    }

    /// Goes on with the work an input started, once the shell began showing
    /// the busy mark ([`Effects::proceed`]).
    pub fn proceed(&mut self, now: Instant) -> Effects {
        let _running = self.shared.meter.enter(meter::CORE);
        self.now = now;
        if let Some((area, _)) = self.frame.mark.take() {
            self.invalidate(area);
        }
        let mut commands = mem::take(&mut self.withheld);
        let mut trusted = mem::take(&mut self.withheld_trusted);
        let shift = commands.len();
        trusted.extend(mem::take(&mut self.trusted).into_iter().map(|i| i + shift));
        let mut own = mem::take(&mut self.withheld_own);
        own.extend(
            mem::take(&mut self.own_trusted)
                .into_iter()
                .map(|i| i + shift),
        );
        self.own_trusted = own;
        commands.append(&mut self.output.commands);
        self.output.commands = commands;
        self.trusted = trusted;
        self.run_commands();
        self.speak_focus();
        self.finish()
    }

    /// What the commands of an input start that may be slow: a screen
    /// opened or closed, or an action run; the key its time is kept by.
    fn work_key(&self, command: &Command) -> Option<String> {
        match command {
            Command::OpenSlot(slot, _) => Some(format!("slot:{slot}")),
            Command::OpenScreen(screen, _) => Some(format!("screen:{screen}")),
            Command::Back => {
                let below = self.stack.len().checked_sub(2)?;
                Some(format!("back:{}", self.stack[below].id))
            }
            Command::RunAction(action, _) | Command::RunOriginal(action, _) => {
                Some(format!("run:{action}"))
            }
            _ => None,
        }
    }

    /// Notes how long the work timed took, once the call is done (with the
    /// mark shown, once it went on).
    fn timed(&mut self) {
        if self.frame.mark.is_some() {
            return;
        }
        let (Some((key, started)), Some(now)) =
            (self.timing.running.take(), self.shared.meter.micros())
        else {
            return;
        };
        let ms = (now.saturating_sub(started) / 1000).min(u64::from(u32::MAX)) as u32;
        self.timing.durations.retain(|(k, _)| *k != key);
        if self.timing.durations.len() >= TIMED {
            self.timing.durations.remove(0);
        }
        self.timing.durations.push((key, ms));
    }

    /// The frame last rendered is on the panel now, at `now` (on the
    /// clock the input's times are on): shells call it after each refresh.
    /// After a screen was opened or closed, presses made before it showed
    /// are dropped: pressing again while nothing seems to happen means
    /// "nothing happened", not "do it once more". On the same screen
    /// (turning pages, moving a focus quickly) a press made before the
    /// frame before the last showed, and more than a second before the
    /// last, is dropped: a click made while the device was busy counts,
    /// but pressing on while it is slow does not line up a long tail of
    /// steps. Shells that never call it keep every press.
    pub fn shown(&mut self, now: Instant) {
        self.input.shown_at = [self.input.shown_at[1], Some(now)];
        if mem::take(&mut self.input.settling) {
            self.input.stale_before = Some(now);
        }
    }

    /// The panel begins to show the frame last rendered, at `now`: shells
    /// that know call it as the refresh begins, before [`Kernel::shown`].
    /// A screen opened or closed is seen while the refresh goes on (a full
    /// one takes over a second), so presses made from here on count,
    /// those before it are dropped (see [`Kernel::shown`]).
    pub fn showing(&mut self, now: Instant) {
        if mem::take(&mut self.input.settling) {
            self.input.stale_before = Some(now);
        }
    }

    /// Whether what the screen shows is secret now: a PIN or a password
    /// being entered ([`Screen::secret`]),
    /// or the device guarded. Those who show the screen elsewhere (a
    /// remote control, tools) leave out what its parts say then.
    pub fn is_secret(&self) -> bool {
        self.guard().is_some() || self.stack.iter().any(|s| s.screen.secret())
    }

    /// The plugin of the screen that guards the device, else of the one a
    /// PIN or a password is entered on; `None` where none is.
    fn secret_owner(&self) -> Option<alloc::string::String> {
        let guard = self.guard().map(|at| &self.stack[at]);
        guard
            .or_else(|| self.stack.iter().rev().find(|s| s.screen.secret()))
            .map(|s| alloc::string::String::from(s.id.namespace()))
    }

    /// Says what has the focus now, if it changed and the user asked for
    /// it ([`core_settings::SPEAK_FOCUS`]): in place of what it said of
    /// the focus before, if that is still spoken; not while a plugin
    /// reads aloud.
    fn speak_focus(&mut self) {
        // Never while a PIN or a password is entered, or the device
        // guarded: the keys pressed would be told.
        let secret = self.is_secret();
        let listened = !secret
            && self
                .subscriptions
                .iter()
                .any(|(event, ..)| *event == events::FOCUS_CHANGED);
        let spoken = self.shared.caps.speech
            && self
                .shared
                .settings
                .bool(core_settings::SPEAK_FOCUS.as_str())
            && !(self.io.speech.speaking() && !self.io.focus_spoken);
        if !listened && !spoken {
            return;
        }
        // At each input: described without a glyph made, the ink of text
        // taken from its line (exact enough to tell what has the focus).
        self.shared.fonts.set_describing(true);
        let parts = self.describe();
        self.shared.fonts.set_describing(false);
        // Plugins that listen are told where it went.
        let focus = paperpaw_sdk::semantics::focused(&parts).cloned();
        if listened && focus != self.io.focus_told {
            if let Some(part) = &focus {
                let mut told = events::FocusChanged::new(
                    String::from(paperpaw_sdk::semantics::role_name(part.role)),
                    part.label.clone(),
                    part.area.x,
                    part.area.y,
                    part.area.width,
                    part.area.height,
                );
                told.on = part.on;
                self.io.focus_told = focus;
                self.output.commands.push(events::FOCUS_CHANGED.emit(told));
                self.run_commands();
            } else {
                self.io.focus_told = None;
            }
        }
        if !spoken {
            return;
        }
        let said = paperpaw_sdk::semantics::focused(&parts).map(|part| {
            let state = match part.on {
                Some(true) => self.shared.translator.tr("core.state.on"),
                Some(false) => self.shared.translator.tr("core.state.off"),
                None => "",
            };
            if state.is_empty() {
                part.label.clone()
            } else {
                alloc::format!("{}, {state}", part.label)
            }
        });
        if said == self.io.speech.focus {
            return;
        }
        if let Some(text) = &said {
            let language = self.shared.translator.language().to_string();
            let speech =
                paperpaw_sdk::Speech::new(text.clone(), Id::new_static("core.focus-spoken"))
                    .in_language(language);
            self.speak(speech);
            self.io.focus_spoken = true;
        }
        self.io.speech.focus = said;
    }

    /// Tells the kernel the time of day and the battery's charge. Shells
    /// call it at start, when the clock is set or its time zone changes,
    /// and when the battery changes (checking about once a minute is
    /// plenty). Status lines redraw when what they show changed
    /// ([`events::STATUS_CHANGED`]).
    pub fn set_status(&mut self, now: Instant, status: SystemStatus) -> Effects {
        let _running = self.shared.meter.enter(meter::CORE);
        self.now = now;
        let old = self.shared.status;
        self.shared.status = status;
        let offset = |s: &SystemStatus| s.clock.map(|c| c.utc_offset_minutes);
        // A new wall clock reading alone changes nothing visible. Compared
        // wide: readings that make no sense must not overflow.
        let jumped = match (old.clock, status.clock) {
            (Some(a), Some(b)) => {
                let base = |c: &paperpaw_platform::WallClock| {
                    i128::from(c.unix_millis) - i128::from(c.at.millis())
                };
                (base(&a) - base(&b)).abs() > 30_000
            }
            (a, b) => a.is_some() != b.is_some(),
        };
        // What status lines show: not the voltage, which moves a little at
        // every reading.
        let shown = |s: &SystemStatus| s.battery.map(|b| (b.percent, b.charging));
        if shown(&old) != shown(&status) || offset(&old) != offset(&status) || jumped {
            self.output.commands.push(events::STATUS_CHANGED.emit(()));
        }
        if status.battery.is_some() {
            self.output.commands.push(events::BATTERY_MEASURED.emit(()));
        }
        self.finish()
    }

    /// When the device is to go to sleep by itself, where it does.
    fn sleep_due(&self) -> Option<Instant> {
        let minutes = self
            .shared
            .settings
            .int(core_settings::SLEEP_AFTER.as_str());
        let set = (minutes as u64).saturating_mul(60_000);
        // Longer while a screen played by watching asks for it.
        let asked = self.top_mode().awake().map_or(0, |idle| {
            u64::try_from(idle.as_millis()).unwrap_or(u64::MAX)
        });
        let due = self.last_activity.add_millis(set.max(asked));
        // Not before plugins' holds end (ADR 0030).
        let due = self.held_until().map_or(due, |held| held.max(due));
        (self.shared.caps.sleep && minutes > 0).then_some(due)
    }

    /// Advances time, e.g. when a requested [`Effect::WakeAt`] is due.
    pub fn tick(&mut self, now: Instant) -> Effects {
        let _running = self.shared.meter.enter(meter::CORE);
        self.now = now;
        self.hide_hints();
        self.escape(now);
        self.count_awake();
        if self.sleep_due().is_some_and(|at| at <= now) {
            self.last_activity = now;
            // Busy: reading aloud, downloading, serving. Else asleep.
            if !self.io.speech.speaking() && !self.io.http.busy() && !self.io.servers.busy() {
                self.output
                    .commands
                    .push(Command::run(paperpaw_sdk::builtin::actions::SLEEP));
            }
        }
        if self.network_deadline().is_some_and(|at| at <= now) {
            self.fail_waiting_network();
        }
        if self.bluetooth_deadline().is_some_and(|at| at <= now) {
            self.bluetooth_tick();
        }
        let thresholds = self.thresholds();
        for recognized in self.input.recognizer.tick(now, &thresholds) {
            // A press held long enough: the reader's input too.
            self.shared.reader = true;
            self.recognized(recognized);
        }
        if self
            .input
            .pending
            .as_ref()
            .is_some_and(|(_, until, _)| *until <= now)
        {
            // No second press or tap came: the first is single.
            self.shared.reader = true;
            self.route_key_pending();
        }
        self.tick_recognizers();
        self.tick_chromes();
        if let Some(top) = self.stack.last_mut()
            && top.timer.is_some_and(|at| at <= now)
        {
            top.timer = None;
            self.with_top(|screen, cx| {
                screen.on_timer(cx);
                Handled::Yes
            });
        }
        if self.notice.as_ref().is_some_and(|(_, until)| *until <= now) {
            self.end_notice();
        }
        // Times and deadlines come due here; jobs step only here, never
        // while input waits.
        self.works.ticking();
        self.finish()
    }

    /// The framebuffer, for tests of a kernel that holds the whole screen.
    #[cfg(test)]
    pub(crate) fn frame(&self) -> FrameView<'_> {
        self.framebuffer.view()
    }

    pub fn device(&self) -> &DeviceInfo {
        &self.shared.device
    }

    /// The panel took `millis` for a refresh: shells that know say so
    /// ([`Usage::panel_ms`](paperpaw_sdk::usage::Usage::panel_ms)).
    pub fn panel_busy(&mut self, millis: u64) {
        self.shared.usage.panel_ms = self.shared.usage.panel_ms.saturating_add(millis);
    }

    /// The device's storage, e.g. for a shell that copies files onto it.
    pub fn storage(&self) -> &dyn Storage {
        self.shared.storage.as_ref()
    }

    /// The power level and mode worn (see [`paperpaw_sdk::power`]), e.g.
    /// for a shell that slows its processor down while saving.
    pub fn power(&self) -> &paperpaw_sdk::power::Power {
        &self.shared.power
    }

    pub fn settings(&self) -> &Settings {
        &self.shared.settings
    }

    /// The device's name as others see it (`Env::device_name`), e.g. for
    /// a shell that gives it to the router as its host name.
    pub fn device_name(&self) -> String {
        let value = self
            .shared
            .settings
            .text(paperpaw_sdk::device_name::SETTING.as_str());
        paperpaw_sdk::device_name::chosen(value, &self.shared.device)
    }

    /// The passwords and keys (see [`paperpaw_sdk::secrets`]), e.g. for a
    /// shell that joins networks itself.
    pub fn secrets(&self) -> &paperpaw_sdk::secrets::Secrets {
        &self.shared.secrets
    }

    /// Keeps or removes a password or key, as [`Command::SetSecret`] of a
    /// plugin does (e.g. a network's password a shell was given).
    ///
    /// The file is read again first: where it is there but cannot be
    /// read, it is left as it is (written, the others in it would be
    /// lost), the change applies until the end, and the user is told; one
    /// readable again takes the place of what could not be read at start.
    /// A change not written is not [`saved`](paperpaw_sdk::secrets::Secrets::saved).
    pub fn set_secret(&mut self, change: paperpaw_sdk::secrets::SecretChange) {
        use paperpaw_sdk::secrets::{self, Secrets};
        let storage = self.shared.storage.clone();
        let read =
            paperpaw_sdk::files::load_for_update(storage.as_ref(), secrets::FILE, MAX_SECRETS);
        if let Ok(data) = &read
            && self.secrets_unreadable
        {
            // Damaged text keeps what can be read: values are in hex.
            let text = String::from_utf8_lossy(data.as_deref().unwrap_or_default());
            self.shared.secrets = Secrets::decode(&text);
            self.secrets_unreadable = false;
        }
        let id = change.id.clone();
        self.shared.secrets.apply(change);
        if let Err(error) = read {
            self.shared.secrets.mark_unsaved(&id);
            self.secrets_unreadable = true;
            let file = secrets::FILE;
            self.warn_runtime(format!(
                "{file} cannot be read ({error}): not saved, left as it is"
            ));
            let file = paperpaw_sdk::Arg::Str(file);
            let text = self
                .shared
                .translator
                .format("core.secrets-unreadable", &[("file", file)]);
            self.output.commands.push(Command::Notify(text));
            return;
        }
        let text = self.shared.secrets.encode();
        let storage = self.shared.storage.as_ref();
        let _ = storage.create_dir(paperpaw_sdk::data::DIR);
        let saved = if self.shared.secrets.is_empty() {
            storage.remove(secrets::FILE).or(Ok(()))
        } else {
            storage.write(secrets::FILE, text.as_bytes())
        };
        match saved {
            Ok(()) => self.shared.secrets.mark_saved(),
            Err(error) => {
                self.shared.secrets.mark_unsaved(&id);
                let message = format!("{}: {error}", secrets::FILE);
                self.warn_runtime(message);
            }
        }
    }

    pub fn translator(&self) -> &Translator {
        &self.shared.translator
    }

    pub fn fonts(&self) -> &Fonts {
        &self.shared.fonts
    }

    pub fn formats(&self) -> &Formats {
        &self.shared.formats
    }

    /// The widgets every plugin added (e.g. for a test that draws each).
    pub fn widgets(&self) -> &paperpaw_sdk::widgets::Widgets {
        &self.shared.widgets
    }

    /// The menus every plugin added to, with their icons (e.g. for a tool
    /// that checks a plugin's).
    pub fn menus(&self) -> paperpaw_sdk::MenuView<'_> {
        paperpaw_sdk::MenuView::new(
            &self.shared.menus,
            &self.shared.actions,
            &self.shared.settings,
        )
    }

    /// Every plugin with its state, `core` first.
    pub fn plugins(&self) -> &Plugins {
        &self.shared.plugins
    }

    /// The id of the screen currently shown.
    pub fn active_screen(&self) -> Option<&Id> {
        self.stack.last().map(|s| &s.id)
    }

    /// The ids of the screens open, the one shown last.
    pub fn screens(&self) -> impl Iterator<Item = &Id> {
        self.stack.iter().map(|s| &s.id)
    }

    /// The ids of every screen plugins added, whether open or not.
    pub fn known_screens(&self) -> impl Iterator<Item = &Id> {
        self.shared.registry.screens.list().iter().map(|(id, _)| id)
    }

    /// What the device did since start (see [`paperpaw_sdk::usage`]).
    pub fn usage(&self) -> &paperpaw_sdk::usage::Usage {
        &self.shared.usage
    }

    /// What each plugin took since start, `core` first.
    pub fn plugin_usage(&self) -> Vec<paperpaw_sdk::usage::PluginUsage> {
        self.shared.meter.table.borrow().clone()
    }

    /// Every action plugins added.
    pub fn actions(&self) -> &paperpaw_sdk::Actions {
        &self.shared.actions
    }

    /// Sends `event` with `params` as a plugin would, to every plugin and
    /// screen that hears it, e.g. asked by a shell's remote control or a
    /// test.
    pub fn send_event(&mut self, now: Instant, event: Id, params: Params) -> Effects {
        let _running = self.shared.meter.enter(meter::CORE);
        // Asked from outside (a remote control, a test): as the reader.
        self.shared.reader = true;
        self.now = now;
        self.output.commands.push(Command::Emit(event, params));
        self.run_commands();
        self.finish()
    }

    /// Types `text` into the text field shown, as if on its keys, and
    /// confirms it with `done` ([`events::TYPE_TEXT`]): for a shell's
    /// remote control or keys of a keyboard. `None` where no text field
    /// shows (the screen bound to [`slots::KEYBOARD`] is not on top).
    pub fn type_text(&mut self, now: Instant, text: &str, done: bool) -> Option<Effects> {
        let keyboard = self
            .shared
            .registry
            .resolve_slot(slots::KEYBOARD.as_str())?;
        if self.active_screen() != Some(keyboard) {
            return None;
        }
        let typed = events::TypeText::new(text.into()).done(done);
        Some(self.send_event(now, events::TYPE_TEXT.id().clone(), typed.to_params()))
    }

    /// Returns and clears the collected warnings.
    pub fn take_warnings(&mut self) -> Vec<Warning> {
        mem::take(&mut self.warnings)
    }

    /// Opens the screen bound to `slot` with `params`, e.g. the reader with
    /// a book's path: for a shell asked to open a file (Android's "open
    /// with", a remote control).
    pub fn open_slot(&mut self, now: Instant, slot: Id, params: Params) -> Effects {
        let _running = self.shared.meter.enter(meter::CORE);
        self.shared.reader = true;
        self.now = now;
        self.last_activity = now;
        self.output.commands.push(Command::OpenSlot(slot, params));
        self.run_commands();
        self.finish()
    }

    /// Opens the book at `path` with the reading app chosen for it (see
    /// `paperpaw_sdk::readers`): for a shell asked to open a file.
    pub fn open_book(&mut self, now: Instant, path: &str) -> Effects {
        let _running = self.shared.meter.enter(meter::CORE);
        self.shared.reader = true;
        self.now = now;
        self.last_activity = now;
        self.output.commands.push(paperpaw_sdk::readers::open(path));
        self.run_commands();
        self.finish()
    }

    /// Opens the screen `screen` with `params`, as a plugin would, e.g.
    /// asked by a shell's remote control or a test.
    pub fn open_screen(&mut self, now: Instant, screen: Id, params: Params) -> Effects {
        let _running = self.shared.meter.enter(meter::CORE);
        self.shared.reader = true;
        self.now = now;
        self.last_activity = now;
        self.output
            .commands
            .push(Command::OpenScreen(screen, params));
        self.run_commands();
        self.finish()
    }

    /// Runs `action` as a button bound to it would (the screen shown has it
    /// first), e.g. asked by a shell's remote control or a test.
    pub fn run_action(&mut self, now: Instant, action: Id) -> Effects {
        let _running = self.shared.meter.enter(meter::CORE);
        self.shared.reader = true;
        self.now = now;
        self.last_activity = now;
        self.dispatch(action);
        self.finish()
    }

    /// Sets setting `id` from its text as configuration files write it
    /// (`on`, `12`, a choice's value), as the settings screen would: saved,
    /// what follows from it done, plugins told. E.g. for a shell's remote
    /// control or a test; why not, where the setting is unknown or the
    /// text no value of it.
    pub fn set_setting(
        &mut self,
        now: Instant,
        id: &str,
        text: &str,
    ) -> Result<Effects, &'static str> {
        // A placement's too (`widgets.clock@2.face`).
        let spec = self.shared.settings.spec(id).ok_or("no such setting")?;
        let value = spec.parse(text).ok_or("not a value of this setting")?;
        let id = Id::parse(id).map_err(|_| "no such setting")?;
        let _running = self.shared.meter.enter(meter::CORE);
        self.now = now;
        self.last_activity = now;
        self.output.commands.push(Command::SetSetting(id, value));
        // The shell's, on the reader's behalf: also while a screen guards
        // the device.
        self.trusted.push(self.output.commands.len() - 1);
        self.run_commands();
        Ok(self.finish())
    }

    /// The device goes to sleep or the app to the background; plugins hear
    /// [`events::SUSPEND`].
    pub fn suspend(&mut self, now: Instant) -> Effects {
        let _running = self.shared.meter.enter(meter::CORE);
        self.now = now;
        self.shared.usage.sleeps = self.shared.usage.sleeps.saturating_add(1);
        self.output.commands.push(events::SUSPEND.emit(()));
        self.bluetooth_suspend();
        let effects = self.finish();
        // What plugins saved, and what storage kept back, written.
        let _ = self.shared.storage.sync();
        effects
    }

    /// PaperPaw stops: the device restarts or the app closes for good;
    /// plugins hear [`events::STOPPING`]. Unlike [`Kernel::suspend`] it is
    /// no sleep: none is counted, and no work waits for it.
    pub fn stop(&mut self, now: Instant) -> Effects {
        let _running = self.shared.meter.enter(meter::CORE);
        self.now = now;
        self.output.commands.push(events::STOPPING.emit(()));
        let effects = self.finish();
        // What plugins saved, and what storage kept back, written.
        let _ = self.shared.storage.sync();
        effects
    }

    /// Back from [`Kernel::suspend`].
    pub fn resume(&mut self, now: Instant) -> Effects {
        let _running = self.shared.meter.enter(meter::CORE);
        self.now = now;
        self.last_activity = now;
        // The time asleep is no screen's.
        self.shown_since = now;
        self.close_shut_out();
        self.output.commands.push(events::RESUME.emit(()));
        self.finish()
    }

    /// Closes the screens above a guard that it did not open (a sleep
    /// screen): input goes to the guard again.
    fn close_shut_out(&mut self) {
        while self.top_shut_out() {
            let top = self.stack.len() - 1;
            let key = paperpaw_sdk::ScreenKey::new(self.stack[top].serial);
            let before = self.stack.len();
            self.close_screen(key);
            if self.stack.len() == before {
                break;
            }
        }
    }

    fn warn(&mut self, source: WarningSource, line: Option<u32>, message: String) {
        self.warnings.push(Warning {
            source,
            line,
            message,
        });
    }

    /// A warning about something that went wrong while running, of no
    /// file or plugin in particular.
    fn warn_runtime(&mut self, message: impl Into<String>) {
        self.warn(WarningSource::Runtime, None, message.into());
    }
}

/// Everything plugins can read through [`Env`].
struct Shared {
    /// The screens and slots plugins added (in here so that a screen can
    /// make another to host it: [`Env::make_screen`]).
    registry: Registry,
    device: DeviceInfo,
    settings: Settings,
    translator: Translator,
    fonts: Fonts,
    formats: Formats,
    hyphenation: Hyphenation,
    menus: Menus,
    actions: Actions,
    plugins: Plugins,
    status: SystemStatus,
    /// What the device did since start (see [`paperpaw_sdk::usage`]).
    usage: paperpaw_sdk::usage::Usage,
    status_items: StatusItems,
    widgets: paperpaw_sdk::widgets::Widgets,
    setup: paperpaw_sdk::setup::SetupSteps,
    services: paperpaw_sdk::services::Services,
    text_filters: paperpaw_sdk::filters::TextFilters,
    /// Marks on the words of book pages, from every plugin.
    decorators: paperpaw_sdk::decorations::Decorators,
    /// Layers on book pages and frames of their text, from every plugin.
    page_layers: paperpaw_sdk::layers::PageLayers,
    /// Where words are looked up, from every plugin.
    lookup: paperpaw_sdk::lookup::LookupProviders,
    /// Plugins' voices (the kernel's alone: plugins speak with
    /// `Command::Speak`).
    voices: paperpaw_sdk::voices::Voices,
    /// The reading apps, from every plugin.
    readers: paperpaw_sdk::readers::Readers,
    /// Filters of every input and of every refresh, from every plugin.
    input_filters: paperpaw_sdk::filters::InputFilters,
    refresh_filters: paperpaw_sdk::filters::RefreshFilters,
    /// Plugins' stylesheets for books.
    stylesheets: Vec<Cow<'static, str>>,
    /// Folders of what plugins keep for each book.
    book_data: Vec<paperpaw_sdk::library::BookData>,
    /// Fields kept with books, badges on their rows, from every plugin.
    book_fields: Vec<paperpaw_sdk::library::BookField>,
    book_badges: Vec<paperpaw_sdk::library::BookBadge>,
    /// What the device and its shell can do.
    caps: Capabilities,
    /// Wi-Fi, where PaperPaw manages it (see [`KernelBuilder::wifi`]).
    wifi: Option<paperpaw_sdk::wifi::Wifi>,
    /// Bluetooth, where the kernel uses a radio (see
    /// [`KernelBuilder::bluetooth`]).
    bluetooth: Option<paperpaw_sdk::bluetooth::Bluetooth>,
    /// Firmware updates, where PaperPaw is the firmware (see
    /// [`KernelBuilder::firmware`]).
    firmware: Option<Firmware>,
    /// How code plugins are loaded, where they are (see
    /// [`KernelBuilder::code_plugins`]).
    native: Option<paperpaw_sdk::native::Native>,
    /// The power level and mode in use (see [`paperpaw_sdk::power`]).
    power: paperpaw_sdk::power::Power,
    /// Where declared work stands, and plugins' notes (see [`work`]).
    work_states: paperpaw_sdk::work::WorkStates,
    /// The call comes from the reader (input, what the shell opens or
    /// runs for them, the answer to a request made so): what plugins say
    /// shows, and they may act at once (`Env::reader`). Otherwise it is
    /// background work, whose notices become notes (see [`work`]).
    reader: bool,
    /// Text copied here.
    clipboard: String,
    /// The system's clipboard has text (see
    /// [`Kernel::set_clipboard_available`]).
    system_clipboard: bool,
    /// What the reader used last ([`Env::last_input`]).
    last_input: paperpaw_sdk::keys::InputMode,
    /// The inputs plugins added, those the device can make
    /// ([`Env::inputs`]).
    inputs: Vec<paperpaw_sdk::keys::InputSpec>,
    /// The open screens as plugins see them ([`Env::stack`]), kept in
    /// step with the kernel's stack.
    stack: Vec<paperpaw_sdk::StackEntry>,
    /// The part of the screen the screen on top has ([`Env::content`]).
    content: Rect,
    /// Slots some screen is bound to.
    slots: Vec<Id>,
    /// Folders fonts are read from.
    font_dirs: Vec<String>,
    /// The folders of data plugins, by plugin, for removing them.
    data_folders: Vec<(Id, String)>,
    /// The folders data plugins are found in.
    plugin_dirs: Vec<String>,
    /// What plugin libraries call this kind of device (see
    /// [`KernelBuilder::catalog_device`]).
    catalog_device: Option<&'static str>,
    /// Passwords and keys (see [`paperpaw_sdk::secrets`]).
    secrets: paperpaw_sdk::secrets::Secrets,
    /// Random bytes (see [`KernelBuilder::random`]).
    random: paperpaw_sdk::random::Random,
    /// What memory is free (see [`KernelBuilder::memory`]).
    memory: paperpaw_sdk::memory::Memory,
    /// How PaperPaw started (see [`KernelBuilder::started`]).
    started: paperpaw_sdk::power::Start,
    /// What the panel can do and how the last frame went (see
    /// [`KernelBuilder::panel`]).
    panel: paperpaw_sdk::realtime::PanelInfo,
    /// The buttons held down now, in the order they went down.
    keys_held: Vec<&'static str>,
    /// The room the screen on top asked for and got.
    room: paperpaw_sdk::realtime::Room,
    /// Shared with font packs read where they are (see `Fonts::load`).
    storage: alloc::rc::Rc<dyn Storage>,
    /// What the storage read during the call going on (see `recall`).
    recall: alloc::rc::Rc<recall::Recall>,
    keymap: Keymap,
    /// The look of the parts of screens a plugin set, and its priority
    /// (see [`Registrar::set_ui_look`](paperpaw_sdk::Registrar::set_ui_look)).
    look: (paperpaw_sdk::ui::look::Look, Option<paperpaw_sdk::Priority>),
    /// What each plugin takes.
    meter: alloc::rc::Rc<meter::Meter>,
}

impl Shared {
    fn env(&self, now: Instant) -> Env<'_> {
        self.env_parts(now).with_screen_maker(&self.registry)
    }

    fn env_parts(&self, now: Instant) -> Env<'_> {
        Env::from(EnvParts {
            device: &self.device,
            now,
            metrics: Metrics::new(self.device.display.dpi),
            settings: &self.settings,
            i18n: &self.translator,
            fonts: &self.fonts,
            formats: &self.formats,
            hyphenation: &self.hyphenation,
            menus: &self.menus,
            actions: &self.actions,
            plugins: &self.plugins,
            status: &self.status,
            usage: &self.usage,
            plugin_usage: &self.meter.table,
            status_items: &self.status_items,
            widgets: &self.widgets,
            setup: &self.setup,
            services: &self.services,
            text_filters: &self.text_filters,
            decorators: &self.decorators,
            page_layers: &self.page_layers,
            lookup: &self.lookup,
            readers: &self.readers,
            stylesheets: &self.stylesheets,
            book_data: &self.book_data,
            book_fields: &self.book_fields,
            book_badges: &self.book_badges,
            caps: self.caps,
            wifi: self.wifi.as_ref(),
            bluetooth: self.bluetooth.as_ref(),
            firmware: self.firmware.as_ref(),
            native: self.native.as_ref(),
            power: &self.power,
            work: &self.work_states,
            reader: self.reader,
            clipboard: self.system_clipboard || !self.clipboard.is_empty(),
            last_input: self.last_input,
            inputs: &self.inputs,
            content: self.content,
            stack: &self.stack,
            slots: &self.slots,
            screens: self.registry.screens.list(),
            font_dirs: &self.font_dirs,
            catalog_device: self.catalog_device,
            secrets: &self.secrets,
            running: &*self.meter,
            random: self.random,
            memory: self.memory,
            clock: self.meter.clock(),
            started: self.started,
            panel: self.panel,
            keys_held: &self.keys_held,
            room: self.room,
            storage: self.storage.as_ref(),
            keys: &self.keymap,
            look: self.look.0,
        })
    }
}

/// `text` cut to what the clipboard holds, at a character boundary.
fn clipped(text: &str) -> String {
    let mut end = text.len().min(paperpaw_sdk::MAX_CLIPBOARD);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    String::from(&text[..end])
}

/// Whether this build renders OpenType/TrueType fonts.
fn paperpaw_text_outlines() -> bool {
    paperpaw_sdk::text::OUTLINES
}
