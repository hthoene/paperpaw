//! Which waveform the X4's and the X4 Pro's fast refreshes are shown with, and
//! how much of the picture the X4 Pro sends for them: from the settings.

use paperpaw_platform::RefreshMode;
use paperpaw_x4::display::Refresh;

use super::Firmware;
use crate::device;

impl Firmware {
    /// Sets how the next refresh is shown: the waveform and the transfer
    /// of a fast refresh in `mode`. Both stay on the panel until set again,
    /// so every frame sets them, also the paced ones (`paced.rs`), which
    /// would else show with what the last frame to wait for the panel left.
    pub(super) fn set_waveform(&mut self, refresh: Refresh, mode: RefreshMode) {
        let experiment = if refresh == Refresh::Fast {
            fast_waveform(mode, self.kernel.settings())
        } else {
            None
        };
        let _supported = self.panel.set_experiment(experiment);
        self.panel.set_transfer(transfer(self.kernel.settings()));
    }
}

/// The waveform of a fast refresh on the X4: a direct drive of a few frames for the modes that ask
/// for speed: `RefreshMode::Fast` (menus, focus moved, small changes; the
/// setting *Fast changes* says how, 4 to 8 frames in about 160 to 240 ms, or
/// the panel's own, about 500 ms) and `RefreshMode::Mono` (what a screen with
/// `ScreenMode::fastest` gets: games, pen strokes; 1 frame in about 100 ms by
/// default), where the panel's own fast waveform takes 500 ms. They leave
/// ghosts that the next full refresh clears. Partial refreshes (pages) keep
/// the panel's own. The numbers are kept in the order the modes have
/// (`waveform::ordered`).
fn fast_waveform(
    mode: RefreshMode,
    settings: &paperpaw_sdk::Settings,
) -> Option<paperpaw_x4::panel::Experiment> {
    menu_waveform(mode, settings)
}

/// How much of the picture the X4 Pro sends for a fast refresh (the other
/// controllers send what they must).
fn transfer(settings: &paperpaw_sdk::Settings) -> paperpaw_x4::uc8179::Transfer {
    use paperpaw_x4::uc8179::Transfer;
    match settings.read(&device::UC_TRANSFER) {
        device::UC_TRANSFER_CHANGED => Transfer::Changed,
        device::UC_TRANSFER_COPIED => Transfer::Copied,
        _ => Transfer::Whole,
    }
}

/// The waveform of the menus and the games: the SSD1677's on the X4, the
/// UC8179's of the registers on the X4 Pro (where the controller is one,
/// `device::uc8179`), else none.
#[cfg(feature = "x4")]
fn menu_waveform(
    mode: RefreshMode,
    settings: &paperpaw_sdk::Settings,
) -> Option<paperpaw_x4::panel::Experiment> {
    menu_ssd1677(mode, settings)
}

#[cfg(not(feature = "x4"))]
fn menu_waveform(
    mode: RefreshMode,
    settings: &paperpaw_sdk::Settings,
) -> Option<paperpaw_x4::panel::Experiment> {
    if device::uc8179() {
        menu_uc8179(mode, settings)
    } else {
        None
    }
}

/// The UC8179's: the step of the setting *Fast changes* (or the numbers of
/// *Own values*) for fast changes, the numbers of `x4.uc8179.mono-*` (12
/// frames at 200 Hz, no hold: a guess, to be tuned) for the fastest refresh. The panel's
/// own waveform is 19 frames at 50 Hz; these are the frames judged right
/// on an X4 Pro, at the voltages of that waveform (the OTP's: set at start).
/// A hold of 3 frames keeps the ghosts of the pixels that stay down.
#[cfg_attr(
    feature = "x4",
    allow(dead_code, reason = "the X4 Pro's menus; the X4 builds it to check it")
)]
fn menu_uc8179(
    mode: RefreshMode,
    settings: &paperpaw_sdk::Settings,
) -> Option<paperpaw_x4::panel::Experiment> {
    use paperpaw_x4::uc8179::{self, FRAME_RATES, OWN_RATE};
    use paperpaw_x4::waveform::Direct;
    if !matches!(mode, RefreshMode::Fast | RefreshMode::Mono) {
        return None;
    }
    let number = |setting: &paperpaw_sdk::setting::Setting<i64>| {
        u8::try_from(settings.read(setting).clamp(0, 255)).unwrap_or(u8::MAX)
    };
    // The frames, the held frames, the kick, the rate in Hz.
    let (frames, hold, kick, hz) = match settings.read(&device::UC_REFRESH) {
        // The fastest refresh has numbers of its own, whatever the menus use.
        _ if mode == RefreshMode::Mono => (
            number(&device::UC_MONO_FRAMES),
            number(&device::UC_MONO_HOLD),
            0,
            settings.read(&device::UC_MONO_RATE),
        ),
        device::UC_CLEAN => (UC_CLEAN_FRAMES, UC_HOLD, 0, 50),
        device::UC_BALANCED => (UC_BALANCED_FRAMES, UC_HOLD, 0, 50),
        device::UC_QUICK => (UC_QUICK_FRAMES, UC_HOLD, 0, 50),
        device::UC_CUSTOM => (
            number(&device::UC_FRAMES),
            number(&device::UC_HOLD),
            number(&device::UC_KICK),
            settings.read(&device::UC_RATE),
        ),
        _ => return None,
    };
    let rate = FRAME_RATES
        .iter()
        .position(|&(_, rate)| i64::from(rate) >= hz)
        .unwrap_or(OWN_RATE);
    Some(paperpaw_x4::panel::Experiment::Uc8179(uc8179::Experiment {
        drive: Direct::new(frames, kick),
        swapped: false,
        repeat: 1,
        hold,
        rate,
        own_at: None,
    }))
}

/// The SSD1677's (the X4's): see [`menu_waveform`].
#[cfg(feature = "x4")]
fn menu_ssd1677(
    mode: RefreshMode,
    settings: &paperpaw_sdk::Settings,
) -> Option<paperpaw_x4::panel::Experiment> {
    use paperpaw_x4::waveform::{Direct, ordered};
    if !matches!(mode, RefreshMode::Fast | RefreshMode::Mono) {
        return None;
    }
    let number = |setting: &paperpaw_sdk::setting::Setting<i64>| {
        u8::try_from(settings.read(setting).clamp(0, 255)).unwrap_or(u8::MAX)
    };
    let fast = match settings.read(&device::FAST_REFRESH) {
        device::FAST_CLEAN => Some(Direct::new(CLEAN_FRAMES, 0)),
        device::FAST_BALANCED => Some(Direct::new(BALANCED_FRAMES, 0)),
        device::FAST_QUICK => Some(Direct::new(QUICK_FRAMES, 0)),
        device::FAST_CUSTOM => Some(Direct::new(
            number(&device::FAST_FRAMES),
            number(&device::FAST_KICK),
        )),
        _ => None,
    };
    let mono = Direct::new(number(&device::MONO_FRAMES), number(&device::MONO_KICK));
    let (fast, mono) = ordered(fast, mono);
    let drive = if mode == RefreshMode::Mono {
        Some(mono)
    } else {
        fast
    };
    drive.map(|drive| {
        paperpaw_x4::panel::Experiment::Ssd1677(paperpaw_x4::display::Experiment::Direct(drive))
    })
}

/// Frames the UC8179's fast changes drive a changed pixel by the step of
/// `x4.uc8179.fast-refresh`, 3 of them held (50 Hz, repeat 1: about 360, 380
/// and 440 ms; the panel's own 625 ms).
const UC_QUICK_FRAMES: u8 = 6;
const UC_BALANCED_FRAMES: u8 = 7;
const UC_CLEAN_FRAMES: u8 = 10;
const UC_HOLD: u8 = 3;

/// Frames a fast refresh drives a changed pixel, no kick (tried in the waveform
/// lab), by the steps of the setting `x4.fast-refresh`: more frames leave
/// less ghosting.
#[cfg(feature = "x4")]
const CLEAN_FRAMES: u8 = 8;
#[cfg(feature = "x4")]
const BALANCED_FRAMES: u8 = 6;
#[cfg(feature = "x4")]
const QUICK_FRAMES: u8 = 4;
