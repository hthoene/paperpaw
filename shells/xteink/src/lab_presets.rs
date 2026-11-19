//! LAB ONLY (#40, feature `turn-lab`, in no release): the presets of the
//! page-turn lab, by the panel's controller. One choice setting,
//! `x4.turn-lab` (Settings > Display), says how a page turn (the page-turn
//! refresh, `RefreshMode::Partial`) is shown; it is read before every turn,
//! so a change shows at once. The lab's screen (Settings, below the groups;
//! `paperpaw_core::turn_lab`) turns test pages and sets the same setting.
//!
//! The numbers are researched guesses to be judged by eye on each device
//! (`~/shared/reports/40-waveform-research.md`): none was run on a device.
//! A page turn that is not a fast refresh (a clean one, grays) keeps the
//! stock waveform, so a full refresh cleans every ghost.

use core::sync::atomic::{AtomicU8, Ordering};

use alloc::string::String;
use paperpaw_core::kit::action::ActionSpec;
use paperpaw_core::kit::menus::MenuEntry;
use paperpaw_core::{Ask, Staging};
use paperpaw_sdk::builtin::{groups, menus};
use paperpaw_sdk::setting::{Setting, Text};
use paperpaw_sdk::{Id, Params, RegisterError, SettingSpec};
use paperpaw_x4::display::Experiment as Ssd1677;
use paperpaw_x4::panel::Experiment;
use paperpaw_x4::probe::Controller;
use paperpaw_x4::uc8179;
use paperpaw_x4::waveform::Direct;

/// Which preset the page turns are shown with.
pub const PRESET: Setting<Text> = Setting::new("x4.turn-lab");
const OPEN: Id = Id::new_static("x4.turn-lab-open");

pub const STANDARD: &str = "x4.turn-lab.standard";
pub const CONTRAST: &str = "x4.turn-lab.contrast";
pub const BALANCED: &str = "x4.turn-lab.balanced";
pub const FAST: &str = "x4.turn-lab.fast";
pub const FASTEST: &str = "x4.turn-lab.fastest";
pub const WARM: &str = "x4.turn-lab.warm";
pub const SOFT: &str = "x4.turn-lab.soft";
#[cfg(feature = "picco")]
pub const SMALL: &str = "x4.turn-lab.small";
#[cfg(feature = "picco")]
pub const FULL: &str = "x4.turn-lab.full";

/// The choices of each controller, today's behaviour first (the reset).
const SSD1677: [&str; 6] = [STANDARD, CONTRAST, BALANCED, FAST, FASTEST, WARM];
const UC8179: [&str; 6] = [STANDARD, CONTRAST, BALANCED, FAST, FASTEST, SOFT];
const UC8253: [&str; 5] = [STANDARD, CONTRAST, BALANCED, FAST, FASTEST];
#[cfg(feature = "picco")]
const SSD2677: [&str; 3] = [STANDARD, SMALL, FULL];

static KIND: AtomicU8 = AtomicU8::new(0);

/// Tells which controller the panel has (`main`, before the kernel is
/// built): the presets are its own.
pub fn set_controller(controller: Controller) {
    KIND.store(
        match controller {
            Controller::Ssd1677 => 1,
            Controller::Uc8179 => 2,
            Controller::Uc8253 => 3,
            #[cfg(feature = "picco")]
            Controller::Ssd2677 => 4,
            _ => 0,
        },
        Ordering::Relaxed,
    );
}

/// The presets of this panel's controller; none where its waveforms cannot
/// be set (the UC8279).
pub fn choices() -> &'static [&'static str] {
    match KIND.load(Ordering::Relaxed) {
        1 => &SSD1677,
        2 => &UC8179,
        3 => &UC8253,
        #[cfg(feature = "picco")]
        4 => &SSD2677,
        _ => &[],
    }
}

/// What a preset does to a page turn.
pub struct Preset {
    /// The waveform of the turn (`None`: the panel's own, as the product).
    pub experiment: Option<Experiment>,
    /// Every this many turns a half refresh cleans every ghost (0: never,
    /// the kernel's *full refresh every N pages* alone).
    pub clean_every: u32,
}

/// The preset `id` of this panel's controller.
pub fn preset(id: &str) -> Preset {
    let (experiment, clean_every) = match (KIND.load(Ordering::Relaxed), id) {
        (1, CONTRAST) => (ssd1677(Ssd1677::Direct(Direct::new(8, 1))), 0),
        (1, BALANCED) => (ssd1677(Ssd1677::Direct(Direct::new(6, 0))), 0),
        (1, FAST) => (ssd1677(Ssd1677::Direct(Direct::new(4, 0))), 5),
        (1, FASTEST) => (ssd1677(Ssd1677::Direct(Direct::new(2, 0))), 3),
        (1, WARM) => (ssd1677(Ssd1677::Temperature(90)), 0),
        // The frame rates: index 3 is the controller's own 50 Hz, 8 is
        // 100 Hz, 12 is 200 Hz.
        (2, CONTRAST) => (pro(10, 1, 3, 3), 0),
        (2, BALANCED) => (pro(7, 0, 3, 3), 0),
        (2, FAST) => (pro(8, 0, 8, 3), 5),
        (2, FASTEST) => (pro(12, 0, 12, 0), 3),
        // The panel's own waveform for a cooler panel (30 C, as FreeInk's
        // full refresh asks): longer phases, cleaner.
        (2, SOFT) => (
            Some(Experiment::Uc8179(uc8179::Experiment {
                drive: Direct::new(1, 0),
                swapped: false,
                repeat: 1,
                hold: 0,
                rate: uc8179::OWN_RATE,
                own_at: Some(30),
            })),
            0,
        ),
        (3, CONTRAST) => (Some(Experiment::Uc8253(150)), 0),
        (3, BALANCED) => (Some(Experiment::Uc8253(75)), 0),
        (3, FAST) => (Some(Experiment::Uc8253(50)), 5),
        (3, FASTEST) => (Some(Experiment::Uc8253(33)), 3),
        #[cfg(feature = "picco")]
        (4, SMALL) => (
            Some(Experiment::Ssd2677(paperpaw_x4::ssd2677::Table::Small)),
            0,
        ),
        #[cfg(feature = "picco")]
        (4, FULL) => (
            Some(Experiment::Ssd2677(paperpaw_x4::ssd2677::Table::Full)),
            0,
        ),
        _ => (None, 0),
    };
    Preset {
        experiment,
        clean_every,
    }
}

fn ssd1677(experiment: Ssd1677) -> Option<Experiment> {
    Some(Experiment::Ssd1677(experiment))
}

/// A direct drive of the UC8179: `frames` after `kick` frames of the other
/// colour, a pixel that stays held `hold` frames, at the frame rate
/// `FRAME_RATES[rate]`.
fn pro(frames: u8, kick: u8, rate: usize, hold: u8) -> Option<Experiment> {
    Some(Experiment::Uc8179(uc8179::Experiment {
        drive: Direct::new(frames, kick),
        swapped: false,
        repeat: 1,
        hold,
        rate,
        own_at: None,
    }))
}

/// The setting, the screen's row in Settings and the texts.
pub fn register(r: &mut Staging) -> Result<(), RegisterError> {
    let list = choices();
    if list.is_empty() {
        return Ok(());
    }
    r.add_setting(
        SettingSpec::choice(
            PRESET.static_str(),
            "x4.turn-lab",
            list,
            STANDARD,
            groups::DISPLAY,
        )
        .description("x4.turn-lab.help"),
    )?;
    r.add_action(
        ActionSpec::new(OPEN, "x4.turn-lab.open").handler(|env, _, asks| {
            let i18n = env.i18n();
            let choices = choices();
            let mut names = String::new();
            for (n, choice) in choices.iter().enumerate() {
                if n > 0 {
                    names.push('|');
                }
                names.push_str(i18n.tr(choice));
            }
            let mut params = Params::new()
                .with(paperpaw_core::turn_lab::SETTING, PRESET.as_str())
                .with(paperpaw_core::turn_lab::CHOICES, choices.join(","))
                .with(paperpaw_core::turn_lab::NAMES, names);
            for (key, word) in paperpaw_core::turn_lab::WORDS.iter().zip(WORDS) {
                params = params.with(*key, i18n.tr(word));
            }
            asks.push(Ask::OpenScreen(
                paperpaw_core::turn_lab::SCREEN,
                params,
            ));
        }),
    )?;
    r.add_menu_entry(MenuEntry::new(menus::SETTINGS, OPEN))
}

/// The texts of the screen, in the order of `paperpaw_core::turn_lab::WORDS`.
const WORDS: [&str; 8] = [
    "x4.turn-lab",
    "x4.turn-lab.word-preset",
    "x4.turn-lab.word-turn",
    "x4.turn-lab.word-loop",
    "x4.turn-lab.word-reset",
    "x4.turn-lab.word-last",
    "x4.turn-lab.text-a",
    "x4.turn-lab.text-b",
];
