//! Every linked plugin the app lets sleep (`paperpaw_app::SLEEPS`) behaves
//! the same parked as awake (#248, Decisions 165 and 166): one case for each,
//! named, and each the same flow twice on a card of its own, once with every
//! plugin awake and once with that plugin registered to sleep. The flow takes
//! the kernel's own routes to it first (its slots, the core actions it
//! serves), then opens each of its screens from home, moves and confirms on
//! it and leaves it, and runs each of its actions. After every step the screen, what it shows
//! (the described parts), the pixels drawn, the warnings and the effects are
//! recorded; at the end every file on the card. The two records must be
//! equal, and the sleeping run must really start with the plugin parked: a
//! flagged plugin that runs awake fails, so no case passes by doing nothing.
//!
//! The plugin sleeps from its admission and the flow's first input to it
//! wakes it. In the middle of the flow the device goes to standby and back:
//! the sleeping run must really park the plugin again there (D3, Decision
//! 166), and the next input makes it again. A plugin that kept something
//! only in its own memory then differs from the awake run (the counter
//! fixture in the kernel's `parked_tests.rs` shows the comparison catches it).
#![cfg(feature = "native-linked")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

#[path = "support/card.rs"]
mod card;
mod native_activation;

use std::cell::Cell;
use std::fmt::Write as _;
use std::rc::Rc;

use card::Card;
use paperpaw_core::{Effects, Kernel};
use paperpaw_platform::{
    DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, PixelFormat, Rotation, Storage,
};
use paperpaw_sdk::builtin::{actions, slots};
use paperpaw_sdk::{Action, Id, Params};

/// An X4 as the emulator's profile has it: buttons, no touch.
const BUTTONS: DeviceInfo = DeviceInfo::new(
    "test-x4",
    "Test X4",
    DisplayInfo::new(480, 800, 220, PixelFormat::Mono1, Rotation::Deg0, None),
    InputCaps::new(
        true,
        false,
        false,
        &["back", "confirm", "left", "right", "up", "down", "power"],
    ),
    MemoryClass::Small,
);

thread_local! {
    /// The plugin this thread's case lets sleep.
    static SLEEPER: Cell<&'static str> = const { Cell::new("") };
}

fn only_the_sleeper(id: &str) -> bool {
    SLEEPER.with(|sleeper| sleeper.get() == id)
}

fn none_sleeps(_: &str) -> bool {
    false
}

fn fonts() -> paperpaw_app::Fonts {
    #[cfg(any(feature = "packs", not(feature = "outline")))]
    return paperpaw_app::Fonts::Packs;
    #[cfg(all(feature = "outline", not(feature = "packs")))]
    return paperpaw_app::Fonts::Outline;
}

/// One run of the flow and what it recorded.
struct Run {
    kernel: Kernel,
    card: Card,
    now: u64,
    steps: Vec<String>,
    /// Whether the plugin was parked right after the standby in the flow.
    reparked: Option<bool>,
}

impl Run {
    fn start(sleep: fn(&str) -> bool) -> Self {
        let card = Card::new(true);
        let builder = Kernel::builder(BUTTONS)
            .storage(card.clone())
            .user_config("[core]\nlanguage = en\n[help]\nshow = false\n");
        let mut kernel = native_activation::build_on_sleeping(
            paperpaw_app::install(builder, fonts()),
            Rc::new(card.clone()),
            sleep,
        );
        let effects = kernel.start(Instant(0));
        let mut run = Self {
            kernel,
            card,
            now: 0,
            steps: Vec::new(),
            reparked: None,
        };
        run.step("start", effects);
        run
    }

    /// Ticks until nothing is due within a second, then records the step.
    fn step(&mut self, what: &str, mut effects: Effects) {
        let mut seen = format!("{effects:?}");
        for _ in 0..10_000 {
            let Some(at) = effects.wake_at() else {
                break;
            };
            if at.0 > self.now + 1000 {
                break;
            }
            self.now = self.now.max(at.0);
            effects = self.kernel.tick(Instant(self.now));
            let _ = write!(seen, " {effects:?}");
        }
        let mut frame = Fnv::default();
        self.kernel.render(&mut |top, rows| {
            frame.bytes(&top.to_le_bytes());
            frame.bytes(rows.data);
        });
        let parts: Vec<String> = self
            .kernel
            .describe()
            .into_iter()
            .map(|part| {
                format!(
                    "{:?}|{}|{}|{:?}",
                    part.role, part.label, part.focused, part.area
                )
            })
            .collect();
        let warnings: Vec<String> = self
            .kernel
            .take_warnings()
            .into_iter()
            .map(|warning| warning.to_string())
            .collect();
        let screen = self
            .kernel
            .active_screen()
            .map(ToString::to_string)
            .unwrap_or_default();
        self.steps.push(format!(
            "{what}: screen {screen}, frame {:016x}, parts {parts:?}, warnings {warnings:?}, effects {seen}",
            frame.0
        ));
    }

    fn run(&mut self, action: &Id) {
        self.now += 100;
        let effects = self.kernel.run_action(Instant(self.now), action.clone());
        self.step(&format!("run {action}"), effects);
    }

    /// Standby and back (D3): a plugin that may sleep and runs idle is
    /// parked again on standby, and made again by the next input.
    fn standby(&mut self, plugin: &str) {
        self.now += 100;
        let effects = self.kernel.suspend(Instant(self.now));
        self.reparked = self.kernel.native_is_parked(plugin);
        self.step("suspend", effects);
        self.now += 100;
        let effects = self.kernel.resume(Instant(self.now));
        self.step("resume", effects);
    }

    fn open_slot(&mut self, slot: &Id) {
        self.now += 100;
        let effects = self
            .kernel
            .open_slot(Instant(self.now), slot.clone(), Params::new());
        self.step(&format!("open slot {slot}"), effects);
    }

    fn open(&mut self, screen: &Id) {
        self.now += 100;
        let effects = self
            .kernel
            .open_screen(Instant(self.now), screen.clone(), Params::new());
        self.step(&format!("open {screen}"), effects);
    }

    /// Every file on the card with its bytes, as one step.
    fn card_files(&mut self) {
        let mut files = Vec::new();
        list_files(&self.card, "", &mut files);
        files.sort();
        let mut text = String::from("card:");
        for path in files {
            let mut hash = Fnv::default();
            hash.bytes(&self.card.0.get(&path).unwrap_or_default());
            let _ = write!(text, " {path}={:016x}", hash.0);
        }
        self.steps.push(text);
    }
}

fn list_files(card: &Card, dir: &str, out: &mut Vec<String>) {
    let mut entries = Vec::new();
    let _ = card.list(dir, &mut |entry| {
        entries.push((entry.name.to_string(), entry.is_dir));
    });
    for (name, is_dir) in entries {
        let path = if dir.is_empty() {
            name
        } else {
            format!("{dir}/{name}")
        };
        if is_dir {
            list_files(card, &path, out);
        } else {
            out.push(path);
        }
    }
}

/// FNV-1a, 64 bits.
struct Fnv(u64);
impl Default for Fnv {
    fn default() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
}
impl Fnv {
    fn bytes(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 ^= u64::from(*byte);
            self.0 = self.0.wrapping_mul(0x0100_0000_01b3);
        }
    }
}

/// The kernel's own routes that reach `plugin` (screens bound to it, core
/// actions it serves): taken first in the flow, while the sleeping plugin is
/// still parked, so that a route which fails to wake it differs from the
/// awake run.
fn kernel_routes(plugin: &str) -> (&'static [Id], &'static [Id]) {
    static MENU_SLOT: [Id; 1] = [slots::MENU];
    static MENU_ACTION: [Id; 1] = [actions::MENU];
    static KEYBOARD_SLOT: [Id; 1] = [slots::KEYBOARD];
    static SETTINGS_SLOT: [Id; 1] = [slots::SETTINGS];
    static SETTINGS_ACTION: [Id; 1] = [actions::SETTINGS];
    match plugin {
        "menu" => (&MENU_SLOT, &MENU_ACTION),
        "keyboard" => (&KEYBOARD_SLOT, &[]),
        "settings" => (&SETTINGS_SLOT, &SETTINGS_ACTION),
        _ => (&[], &[]),
    }
}

/// The flow for `plugin`: the kernel's routes to it first (each recording
/// whether the plugin is parked after it); then each of its screens opened
/// from home, moved on, confirmed and left; each of its actions run from
/// home; the card at the end.
fn flow(run: &mut Run, plugin: &str) {
    let (route_slots, route_actions) = kernel_routes(plugin);
    for screen in route_slots {
        run.run(&actions::HOME);
        run.open_slot(screen);
        let parked = run.kernel.native_is_parked(plugin);
        run.steps
            .push(format!("{plugin} parked after {screen}: {parked:?}"));
        run.run(&actions::DOWN);
        run.run(&actions::CONFIRM);
        run.run(&actions::BACK);
    }
    for action in route_actions {
        run.run(&actions::HOME);
        run.run(action);
        let parked = run.kernel.native_is_parked(plugin);
        run.steps
            .push(format!("{plugin} parked after {action}: {parked:?}"));
        run.run(&actions::BACK);
    }
    // Mid-flow: home, standby, and the next input wakes it (D3).
    run.run(&actions::HOME);
    run.standby(plugin);
    let mut screens: Vec<Id> = run
        .kernel
        .known_screens()
        .filter(|screen| screen.namespace() == plugin)
        .cloned()
        .collect();
    screens.sort();
    let mut own: Vec<Id> = run
        .kernel
        .actions()
        .iter()
        .filter(|action| matches!(action, Action::Native(_)))
        .map(Action::id)
        .filter(|id| id.namespace() == plugin)
        .cloned()
        .collect();
    own.sort();
    own.dedup();
    for screen in &screens {
        run.run(&actions::HOME);
        run.open(screen);
        run.run(&actions::DOWN);
        run.run(&actions::CONFIRM);
        run.run(&actions::BACK);
    }
    for action in &own {
        run.run(&actions::HOME);
        run.run(action);
        run.run(&actions::BACK);
    }
    run.run(&actions::HOME);
    run.card_files();
    run.steps.push(format!(
        "{plugin}: {} screens, {} actions",
        screens.len(),
        own.len()
    ));
}

/// The case for `plugin`: the flow awake and asleep, the same.
fn same_awake_and_asleep(plugin: &'static str) {
    assert!(
        paperpaw_app::sleeps(plugin),
        "{plugin} is not in paperpaw_app::SLEEPS"
    );
    let mut awake = Run::start(none_sleeps);
    assert_eq!(
        awake.kernel.native_is_parked(plugin),
        Some(false),
        "{plugin}: registered and awake in the awake run"
    );
    SLEEPER.with(|sleeper| sleeper.set(plugin));
    let mut asleep = Run::start(only_the_sleeper);
    assert_eq!(
        asleep.kernel.native_is_parked(plugin),
        Some(true),
        "{plugin} is flagged to sleep but runs awake at the start: the case would test nothing"
    );
    flow(&mut awake, plugin);
    flow(&mut asleep, plugin);
    assert!(
        awake.steps.len() > 2,
        "{plugin}: the flow did nothing: {:?}",
        awake.steps
    );
    for (index, (a, b)) in awake.steps.iter().zip(&asleep.steps).enumerate() {
        assert_eq!(a, b, "{plugin} differs asleep at step {index}");
    }
    assert_eq!(
        awake.steps.len(),
        asleep.steps.len(),
        "{plugin}: the runs took a different number of steps"
    );
    assert_eq!(
        asleep.reparked,
        Some(true),
        "{plugin} did not sleep again on standby: the re-park was not tested"
    );
}

/// Every flagged plugin has a case here (and no case is left without one).
#[test]
fn every_plugin_that_may_sleep_has_a_case() {
    let cases = [
        "about",
        "file-manager",
        "files",
        "keyboard",
        "menu",
        "opds",
        "settings",
    ];
    assert_eq!(paperpaw_app::SLEEPS, cases);
}

#[test]
fn about_is_the_same_asleep() {
    same_awake_and_asleep("about");
}

#[test]
fn file_manager_is_the_same_asleep() {
    same_awake_and_asleep("file-manager");
}

#[test]
fn files_is_the_same_asleep() {
    same_awake_and_asleep("files");
}

#[test]
fn keyboard_is_the_same_asleep() {
    same_awake_and_asleep("keyboard");
}

#[test]
fn menu_is_the_same_asleep() {
    same_awake_and_asleep("menu");
}

#[test]
fn opds_is_the_same_asleep() {
    same_awake_and_asleep("opds");
}

#[test]
fn settings_is_the_same_asleep() {
    same_awake_and_asleep("settings");
}
