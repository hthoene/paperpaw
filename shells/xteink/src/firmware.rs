//! The main loop: the kernel and the X4's parts, one step at a time. Each
//! step feeds the watchdogs, moves the network on, carries out what the
//! kernel asked of the firmware (an update, a restart), passes on the
//! battery and the time, the presses that came, what came over USB
//! (`remote`), wakes the kernel when it asked to be, and rests until there
//! is something to do. Whatever the kernel gives back is shown
//! ([`Firmware::apply`]); when it says so, the X4 sleeps.

use esp_hal::delay::Delay;
use esp_hal::gpio::{Event, Output, WakeupConfig};
use log::{info, warn};
use paperpaw_core::{
    Effects, FirmwareCommand, FirmwareState, InstallFailure, Io, Kernel, SpeechCall,
};
use paperpaw_platform::{
    Battery, ButtonState, InputEvent, Instant, Point, PointerEvent, PointerPhase, PointerTool,
    Rect, RefreshMode, SystemStatus,
};
use paperpaw_x4::display::{Planes, Refresh};
#[cfg(feature = "remote")]
use paperpaw_x4::sent::Check;
use paperpaw_x4::sent::Sent;

use crate::board::Panel;
use crate::network::{Network, NetworkIo};
use crate::ota::Ota;
use crate::{clock, device, input, now, power, stack};

mod paced;
mod waveform;

/// How long the main loop rests when there is nothing to do.
const POLL_MS: u64 = 10;
/// Lines of the cable transfer read in one turn of the main loop at most.
const CABLE_LINES: usize = 32;
/// After a line of the cable transfer, the main loop reads on without
/// resting for this long.
const CABLE_QUIET_MS: u64 = 300;
/// How often the battery is read.
const STATUS_MS: u64 = 60_000;
/// On battery, the longest the processor sleeps between readings of the
/// buttons (a press lasts 80 ms and more); less with the button response
/// *Faster*, more with *Saving* (a tap of under that may be missed).
const IDLE_MS: u64 = 30;
const IDLE_FAST_MS: u64 = 10;
const IDLE_SAVING_MS: u64 = 50;
/// How often the buttons are read while awake (`input`'s timer): the
/// rest while a press settles.
const SAMPLE_MS: u64 = 5;
/// How long the kernel's time waits at most for a press or release that
/// settles (`buttons::DEBOUNCE_MS` and a reading more).
const SETTLE_MS: u64 = 25;
/// How long after the last refresh the panel's power goes off: soon
/// where powering up again costs nothing; where it makes the next
/// refresh slower, once refreshes do not follow each other (menus,
/// sliders) and reading goes on.
const PANEL_IDLE_MS: u64 = 1_000;
const PANEL_IDLE_SLOW_MS: u64 = 60_000;
/// How long going to sleep waits for work under way (a position pushed).
const SLEEP_WAIT_MS: u64 = 15_000;
/// How long after the last work what was written is written back to the
/// card (not at once: a download writes in many pieces).
const SYNC_MS: u64 = 2_000;

/// The X4's firmware: the kernel, the parts it drives, and where the main
/// loop is.
pub struct Firmware {
    pub kernel: Kernel,
    pub panel: Panel,
    pub network: Network,
    /// Updates, where the flash has room for them.
    pub ota: Option<Ota>,
    /// Holds the battery's power on (the X4 and the X4 Pro); the X3's
    /// card power (see its board): low before deep sleep. The X4
    /// Classic's parts' power, kept on through it.
    pub power_latch: Output<'static>,
    /// High while on USB power.
    pub usb_power: crate::board::UsbPower,
    pub delay: Delay,
    /// Lines from a computer on USB.
    pub usb: crate::usb_lines::UsbLines,
    #[cfg(feature = "remote")]
    pub remote: crate::remote::Remote,
    /// The X4 Pro's front light.
    #[cfg(feature = "x4-pro")]
    pub light: Option<crate::light::Light>,
    pub state: State,
}

/// Where the main loop is.
#[derive(Debug, Default)]
pub struct State {
    /// The kernel asked to sleep.
    sleep: bool,
    /// When the kernel wants to be woken.
    wake_at: Option<Instant>,
    /// When the battery was read last.
    status_read: Option<Instant>,
    /// Whether the device was on outer power at that reading.
    status_on_usb: bool,
    /// The latest time the kernel was given: presses that waited in the
    /// queue keep their own time, but never one before it.
    kernel_time: u64,
    /// A button pressed over USB, and when to let it go.
    #[cfg(feature = "remote")]
    held: Option<(&'static str, Instant)>,
    /// When a line of the cable transfer came last: while they come, the
    /// main loop reads on without resting, so the transfer is quick.
    cable_at: Option<u64>,
    /// Light sleeps on USB too until then (`idle sleep` over USB), for
    /// trying them.
    #[cfg(feature = "remote")]
    sleep_on_usb_until: Option<Instant>,
    /// The panel shows grays (a frame whose pictures are best so, or the
    /// gray test over USB): the next change is shown with the whole screen
    /// drawn again.
    gray_shown: bool,
    /// When the kernel first worked since the card was last written back,
    /// if what it wrote may not be on the card yet: written back
    /// [`SYNC_MS`] after, so nothing is lost when the X4 is cut off
    /// (reset, flashed, an empty battery).
    unsynced: Option<u64>,
    /// When the panel's power goes off, after the last refresh.
    panel_idle_at: Option<u64>,
    /// How long fast, half and full refreshes take, in milliseconds (the
    /// shortest seen, see [`Firmware::refresh`]): the processor sleeps
    /// about that long while the next one goes (on battery).
    refresh_ms: [u64; 3],
    /// An update was kept after its trial, which started without code
    /// plugins, and there are some to load: a start at a calm moment.
    restart_for_code: bool,
    /// The pace the screen on top runs at (`Effects::pace`): its frames
    /// are shown without waiting for the panel ([`paced`]).
    pace: Option<core::time::Duration>,
    /// A paced frame on its way to the panel, and what changed since,
    /// waiting for it.
    in_flight: Option<paced::InFlight>,
    frames: paperpaw_x4::paced::Frames,
    /// The panel's memories may differ after startup or a paced frame:
    /// the next one drives every pixel.
    frame_stale: bool,
}

/// The place of `refresh` in [`State::refresh_ms`].
fn kind(refresh: Refresh) -> usize {
    match refresh {
        Refresh::Fast => 0,
        Refresh::Half => 1,
        Refresh::Full => 2,
    }
}

impl Firmware {
    /// Starts the kernel, then runs the main loop.
    pub fn run(mut self) -> ! {
        crate::startup::finish(&mut self.panel);
        // Startup released its bands before loading the kernel. Drive the
        // first frame whole instead of comparing its old panel memories.
        self.state.frame_stale = true;
        // What building the kernel warned of.
        for warning in self.kernel.take_warnings() {
            warn!("{warning}");
        }
        let effects = self.kernel.start(now());
        self.apply(effects);
        self.finishing_update();
        // Its first frame is shown: its code plugins started cleanly.
        crate::safe_mode::started();
        loop {
            self.step();
        }
    }

    /// An update on trial with code plugins to load once it is kept: it
    /// starts again then, and says so meanwhile, on a screen that takes no
    /// input (nothing begun is cut off).
    fn finishing_update(&mut self) {
        let start = now();
        let Some(left) = self.ota.as_ref().and_then(|ota| ota.trial_left(start.0)) else {
            return;
        };
        if !paperpaw_core::code_to_load(self.kernel.storage(), &crate::native::this_firmware()) {
            return;
        }
        let params = paperpaw_sdk::Params::new()
            .with("from", alloc::format!("{}", start.0))
            .with("until", alloc::format!("{}", start.0 + left));
        let effects = self.kernel.open_screen(
            start,
            paperpaw_sdk::builtin::screens::FINISHING_UPDATE,
            params,
        );
        self.apply(effects);
    }

    /// The X4 ends cleanly (sleeps or restarts): an update on trial goes
    /// on with its trial at the next start (see `ota`).
    fn pause_trial(&mut self) {
        if let Some(ota) = &mut self.ota {
            ota.pause_trial(now().0);
        }
    }

    /// Whether an update is being written: it goes on to its end, the X4
    /// awake.
    fn installing(&self) -> bool {
        self.ota.as_ref().is_some_and(Ota::installing)
    }

    /// One turn of the main loop. Its frame lies under every plugin's work,
    /// so what it calls aside from the kernel (the USB lines, updates,
    /// sleeping) keeps frames of its own (`#[inline(never)]`).
    fn step(&mut self) {
        if let Some(ota) = &mut self.ota
            && ota.check_trial(now().0)
        {
            // An update on trial starts without code plugins: those
            // installed before (an update that keeps the fingerprint
            // fetches none) and those fetched for it wait for a start once
            // it is kept, which installs and loads them (ADR 0024).
            let native = crate::native::this_firmware();
            self.state.restart_for_code =
                paperpaw_core::code_to_load(self.kernel.storage(), &native);
        }
        // At a calm moment: at once, unless a book is open, then once it
        // is left. A sleep on battery before is a start too.
        if self.state.restart_for_code && !self.reading() {
            info!("native: the update is kept; restarting to load the code plugins");
            self.restart();
        }
        let installing = self.installing();
        if self.state.sleep && !installing {
            return self.sleep();
        }
        power::feed();
        power::mark(power::at::LOOP);
        stack::check();
        self.io();
        for effects in self.network.step(&mut self.kernel, now()) {
            self.apply(effects);
        }
        self.firmware_step();
        let light = self.light();
        self.panel_idle();
        // Not while the radio is on: its tasks run in between. Nor while
        // the X4 Pro's light is on: its PWM stops in a light sleep.
        let battery = !self.network.radio_on()
            && !installing
            && !light
            && ((self.usb_power.is_low() && self.kernel.settings().read(&device::LIGHT_SLEEP))
                || self.trying_sleep());
        power::allow(battery);
        self.status();
        self.buttons();
        self.poll_frame();
        self.usb();
        self.tick();
        self.poll_frame();
        self.rest(battery, installing);
    }

    /// Whether the button response is *Faster* (see `device`).
    fn faster(&self) -> bool {
        self.kernel.settings().read(&device::BUTTON_RESPONSE) == device::RESPONSE_FASTER
    }

    /// The longest the processor sleeps between readings of the buttons, by
    /// the button response.
    fn idle_ms(&self) -> u64 {
        let response = self.kernel.settings().read(&device::BUTTON_RESPONSE);
        if response == device::RESPONSE_FASTER {
            IDLE_FAST_MS
        } else if response == device::RESPONSE_SAVING {
            IDLE_SAVING_MS
        } else {
            IDLE_MS
        }
    }

    /// How long after a refresh the panel's power goes off: never where
    /// that slows the next refresh and the response is *Faster*.
    fn panel_idle_ms(&self) -> u64 {
        match self.panel.power_up_ms() {
            0 => PANEL_IDLE_MS,
            _ if self.faster() => u64::MAX / 2,
            _ => PANEL_IDLE_SLOW_MS,
        }
    }

    /// The panel's power off once no refresh followed for a while.
    fn panel_idle(&mut self) {
        if self.state.panel_idle_at.is_some_and(|at| now().0 >= at) {
            self.state.panel_idle_at = None;
            if self.panel.powered()
                && let Err(e) = self.panel.power_down()
            {
                warn!("display: {e:?}");
            }
        }
    }

    /// Shows the front light as its settings say; whether it is on.
    #[cfg(feature = "x4-pro")]
    fn light(&mut self) -> bool {
        let settings = self.kernel.settings();
        let level = settings.read(&device::FRONTLIGHT);
        let warmth = settings.read(&device::WARMTH);
        if let Some(light) = self.light.as_mut() {
            light.show(level, warmth);
        }
        level > 0 && self.light.is_some()
    }

    /// The X4 and the X4 Classic have no light.
    #[cfg(not(feature = "x4-pro"))]
    fn light(&mut self) -> bool {
        false
    }

    /// The time for the kernel now, never before one it was given.
    fn kernel_now(&mut self) -> Instant {
        self.state.kernel_time = now().0.max(self.state.kernel_time);
        Instant(self.state.kernel_time)
    }

    /// What the kernel asks of the X4: the network's work goes to
    /// `network`, the firmware's is done here; speech and a system
    /// clipboard it does not have.
    fn io(&mut self) {
        for io in self.kernel.take_io() {
            match io {
                Io::Wifi(command) => {
                    // Joining: named for the router as the reader named it.
                    if matches!(command, paperpaw_sdk::wifi::WifiCommand::Join { .. }) {
                        let name = self.kernel.device_name();
                        let host = paperpaw_sdk::device_name::host_name(&name);
                        self.network.set_host_name(&host);
                    }
                    self.network.ask(NetworkIo::Wifi(command));
                }
                Io::Http(call) => self.network.ask(NetworkIo::Http(call)),
                Io::Server(call) => self.network.ask(NetworkIo::Server(call)),
                Io::Socket(call) => self.network.ask(NetworkIo::Socket(call)),
                Io::Firmware(command) => self.firmware(command),
                Io::Speech(SpeechCall::Say { id, .. }) => {
                    let effects = self.kernel.speech_done(now(), id, false);
                    self.apply(effects);
                }
                #[cfg(feature = "bluetooth")]
                Io::Bluetooth(call) => self.network.bluetooth(call),
                // Without the X4 Pro's `bluetooth` feature (ADR 0027) no
                // radio is offered, so none is asked for.
                #[cfg(not(feature = "bluetooth"))]
                Io::Bluetooth(_) => {}
                Io::Speech(SpeechCall::Stop) | Io::Copy(_) => {}
                Io::ReadClipboard => {
                    let effects = self.kernel.clipboard_text(now(), None);
                    self.apply(effects);
                }
            }
        }
    }

    /// An update or a restart the kernel asked for.
    #[inline(never)]
    fn firmware(&mut self, command: FirmwareCommand) {
        match command {
            FirmwareCommand::Install {
                path, key, older, ..
            } => {
                let state = match &mut self.ota {
                    Some(ota) => ota.begin(self.kernel.storage(), &path, key.as_ref(), older),
                    None => FirmwareState::Failed(InstallFailure::Write),
                };
                let effects = self.kernel.firmware_state(now(), state);
                self.apply(effects);
            }
            FirmwareCommand::InstallForeign { path, .. } => {
                let state = match &mut self.ota {
                    Some(ota) => ota.begin_foreign(self.kernel.storage(), &path),
                    None => FirmwareState::Failed(InstallFailure::Write),
                };
                let effects = self.kernel.firmware_state(now(), state);
                self.apply(effects);
            }
            FirmwareCommand::Restart => self.restart(),
            // A command this firmware does not know: nothing done.
            _ => {}
        }
    }

    /// Restarts the X4 once what plugins keep is saved and all written
    /// is on the card (the kernel's stop writes it back): no sleep.
    /// Whether a book is open in the reader, shown now.
    fn reading(&self) -> bool {
        // The reader plugin's screen (`paperpaw_plugin_reader::SCREEN`).
        self.kernel
            .active_screen()
            .is_some_and(|screen| screen.as_str() == "reader.screen")
    }

    fn restart(&mut self) -> ! {
        info!("restarting");
        self.settle_frame();
        crate::startup::restarting(&mut self.panel, &self.kernel);
        self.pause_trial();
        let _ = self.kernel.stop(now());
        self.network.leave();
        esp_hal::system::software_reset();
    }

    /// How an update goes.
    #[inline(never)]
    fn firmware_step(&mut self) {
        if let Some(ota) = &mut self.ota
            && let Some(state) = ota.step(self.kernel.storage())
        {
            let effects = self.kernel.firmware_state(now(), state);
            self.apply(effects);
        }
    }

    /// The battery and the time, every [`STATUS_MS`], once the clock was
    /// set and as soon as the device goes on or off outer power (looked at
    /// in every round anyway, for the light sleeps): charging shows the
    /// moment the cable goes in or out, without reading more often.
    fn status(&mut self) {
        let at = now();
        let due = self
            .state
            .status_read
            .is_none_or(|read| at.since(read) >= STATUS_MS);
        let on_usb = self.usb_power.is_high();
        let plugged = on_usb != self.state.status_on_usb;
        if !clock::take_changed() && !due && !plugged {
            return;
        }
        power::mark(power::at::STATUS);
        self.state.status_read = Some(at);
        self.state.status_on_usb = on_usb;
        let millivolts = input::millivolts();
        let status = SystemStatus {
            clock: clock::wall(at),
            battery: Some(Battery {
                percent: input::percent(),
                charging: on_usb,
                millivolts: Some(millivolts),
            }),
        };
        let effects = self.kernel.set_status(at, status);
        self.apply(effects);
    }

    /// The presses and releases that came, before any work in the
    /// background. All that waits is handled before a frame shows: what
    /// came while the last one refreshed (taps on *+*, presses of a
    /// button) shows in one frame, not a frame each after the other.
    fn buttons(&mut self) {
        let mut waiting: Option<Effects> = None;
        while let Some(change) = input::next() {
            let (event, at) = match change {
                input::Change::Button(name, down, at) => {
                    let state = if down {
                        ButtonState::Pressed
                    } else {
                        ButtonState::Released
                    };
                    (InputEvent::Button { name, state }, at)
                }
                input::Change::Touch(input::Touch::Home(down), at) => {
                    let state = if down {
                        ButtonState::Pressed
                    } else {
                        ButtonState::Released
                    };
                    let name = paperpaw_platform::buttons::HOME;
                    (InputEvent::Button { name, state }, at)
                }
                input::Change::Touch(touch, at) => (self.pointer(touch), at),
            };
            self.state.kernel_time = at.max(self.state.kernel_time);
            let started = now();
            power::mark(power::at::INPUT);
            let effects = self.kernel.handle(Instant(self.state.kernel_time), event);
            if let InputEvent::Button { name, .. } = event {
                info!("{name}: {} ms", now().since(started));
            }
            let effects = match waiting.take() {
                Some(before) => before.and(effects),
                None => effects,
            };
            // What cannot wait for the next input: a restart, sleep, the
            // busy mark's work, a frame in grays.
            if effects.restart() || effects.sleep() || effects.proceed() || effects.grays() {
                self.apply(effects);
            } else {
                waiting = Some(effects);
            }
        }
        if let Some(effects) = waiting {
            self.apply(effects);
        }
    }

    /// A touch on the upright screen, on the screen PaperPaw draws (it may
    /// be turned).
    fn pointer(&self, touch: input::Touch) -> InputEvent {
        let (phase, x, y) = match touch {
            input::Touch::Down(x, y, raw) => {
                info!("touch at {x},{y} (controller {},{})", raw.0, raw.1);
                (PointerPhase::Down, x, y)
            }
            input::Touch::Move(x, y) => (PointerPhase::Move, x, y),
            input::Touch::Up(x, y) => (PointerPhase::Up, x, y),
            // A button, taken apart before.
            input::Touch::Home(_) => (PointerPhase::Cancel, 0, 0),
        };
        let (x, y) = paperpaw_x4::uc8179::turned(self.panel.rotation(), x, y);
        InputEvent::Pointer(PointerEvent {
            id: 0,
            phase,
            point: Point::new(i32::from(x), i32::from(y)),
            tool: PointerTool::Finger,
            pressure: 1000,
        })
    }

    /// Wakes the kernel when it asked to be.
    fn tick(&mut self) {
        if !self.state.wake_at.is_some_and(|due| due <= now()) {
            return;
        }
        // The presses and releases that came meanwhile (while a frame
        // refreshed) first, also those that come while their frames
        // refresh, and one settling (a little while at most): a release
        // still waiting would leave its button held at this time, a click
        // taken for a long press.
        let settled_by = now().0 + SETTLE_MS;
        loop {
            if input::waiting() {
                self.buttons();
            } else if input::settling() && now().0 < settled_by {
                self.delay.delay_millis(SAMPLE_MS as u32);
            } else {
                break;
            }
        }
        power::mark(power::at::TICK);
        let started = now();
        self.state.kernel_time = started.0;
        let effects = self.kernel.tick(started);
        let took = now().since(started);
        if took > 50 {
            info!("tick: {took} ms");
        }
        self.apply(effects);
    }

    /// On battery the processor sleeps until there is something to do or
    /// the buttons are to be read again; on USB it stays in reach of the
    /// computer (see `power`).
    fn rest(&mut self, battery: bool, installing: bool) {
        #[cfg(feature = "remote")]
        if self
            .state
            .sleep_on_usb_until
            .is_some_and(|until| until <= now())
        {
            // Started again: whatever the light sleeps did to the USB link,
            // the computer finds the X4 again.
            power::mark(power::at::TRIED);
            esp_hal::system::software_reset();
        }
        // A press under way: read again soon, awake (the readings stop in
        // a light sleep), so that it counts at once.
        if input::settling() {
            self.delay.delay_millis(SAMPLE_MS as u32);
            return;
        }
        let idle = self.idle_ms();
        let mut until = now().0
            + if battery {
                idle
            } else if self.network.moving() || installing || self.cable_moving() {
                // Data on the way, or an update: on at once.
                0
            } else {
                POLL_MS
            };
        if let Some(at) = self.state.wake_at {
            until = until.min(at.0);
        }
        if let Some(worked) = self.state.unsynced {
            let due = worked + SYNC_MS;
            if now().0 >= due {
                self.state.unsynced = None;
                if self.kernel.storage().sync().is_err() {
                    warn!("card: not all written back");
                }
            } else {
                until = until.min(due);
            }
        }
        if let Some(read) = self.state.status_read {
            until = until.min(read.0 + STATUS_MS);
        }
        if let Some(at) = self.state.panel_idle_at {
            until = until.min(at);
        }
        if let Some(due) = self.frame_due() {
            until = until.min(due);
        }
        if !power::nap_until(until) {
            self.delay
                .delay_millis(until.saturating_sub(now().0) as u32);
        }
    }

    /// Whether light sleeps are tried on USB (`idle sleep` over USB).
    fn trying_sleep(&self) -> bool {
        #[cfg(feature = "remote")]
        return self.state.sleep_on_usb_until.is_some();
        #[cfg(not(feature = "remote"))]
        false
    }

    /// Plugins save what is pending, the screen stays as it is, and the X4
    /// sleeps: its power is cut where possible, otherwise the processor
    /// enters deep sleep. The power button starts it again where it was.
    /// USB or a charging indication must not keep a sleeping reader awake.
    #[inline(never)]
    fn sleep(&mut self) {
        self.settle_frame();
        // A press over USB that put it to sleep is let go first.
        #[cfg(feature = "remote")]
        self.release_held();
        self.pause_trial();
        let effects = self.kernel.suspend(now());
        self.apply(effects);
        // Bluetooth down now: the main loop does not come by once the
        // pending work is finished and the processor sleeps.
        #[cfg(feature = "bluetooth")]
        self.network
            .bluetooth(paperpaw_core::BtCall::Radio { on: false });
        if !self.finish_work() {
            // A press meanwhile: awake after all.
            info!("sleep: a press, awake");
            self.state.sleep = false;
            if let Some(ota) = &mut self.ota {
                ota.resume_trial(now().0);
            }
            let effects = self.kernel.resume(now());
            return self.apply(effects);
        }
        // A real sleep first (the processor lightly asleep, the power button
        // waking it at once), switched off only after a while (#51).
        #[cfg(feature = "esp32c3")]
        if self.sleep_lightly() {
            return;
        }
        self.power_off()
    }

    /// The power goes: only the power button starts the reader again, and
    /// that is a start (a light sleep ended by the button is not).
    #[expect(
        clippy::expect_used,
        reason = "the power button is taken back only here; without it the X4 could not wake"
    )]
    #[inline(never)]
    fn power_off(&mut self) -> ! {
        // The next start ends this sleep: the mark says so, for plugins to
        // hear the device wake (`core.resume`).
        paperpaw_x4::slept::mark(self.kernel.storage());
        let _ = self.panel.sleep();
        #[cfg(feature = "x4-pro")]
        if let Some(light) = self.light.as_mut() {
            light.off();
        }
        info!(
            "sleeping: hardware shutdown (USB {})",
            self.usb_power.is_high()
        );
        // Released first: held, the button would wake it at once.
        self.wait_for_power_release();
        // The power goes, the radio's with it: the network left first.
        self.network.leave();
        let mut power = input::stop().expect("the power button");
        // The X4 Classic keeps its power on and holds its lines, as its
        // stock firmware sleeps (see its board).
        #[cfg(feature = "x4-classic")]
        crate::board::hold_for_sleep(&mut self.power_latch);
        #[cfg(not(feature = "x4-classic"))]
        {
            // The X4 Pro holds its latch through resets: let go first.
            #[cfg(feature = "esp32s3")]
            self.power_latch.set_pad_hold(false);
            self.power_latch.set_low();
        }
        power.listen(Event::LowLevel);
        let _ = power.apply_wakeup_config(&WakeupConfig::default().with_low_power_path(true));
        power::deep();
    }

    /// Sleeps lightly until the power button wakes the reader (`true`:
    /// awake again, where it was), or until it is time to switch it off
    /// (`false`: the caller does). The panel keeps its picture, the radios
    /// are checked off, the readings rest and the watchdogs are the sleep's;
    /// every [`paperpaw_x4::sleeping::CHECK_MS`] it wakes for a moment to
    /// look at the battery. The order of it is
    /// [`paperpaw_x4::sleeping::run`], tested on a computer.
    #[cfg(feature = "esp32c3")]
    #[inline(never)]
    fn sleep_lightly(&mut self) -> bool {
        use paperpaw_x4::sleeping::{Ended, Off};
        // The developer switch back to how the X4 slept before.
        if !self.kernel.settings().read(&device::LIGHT_SLEEP) {
            return false;
        }
        let choice = self.kernel.settings().read(&device::POWER_OFF_AFTER);
        let off = Off::of(choice);
        info!(
            "sleeping: light sleep, off {off:?}, USB {}",
            self.usb_power.is_high()
        );
        let started = now().0;
        let ended = paperpaw_x4::sleeping::run(off, &mut Light { firmware: self });
        info!(
            "sleeping: {ended:?} after {} s",
            now().0.saturating_sub(started) / 1000
        );
        if ended == Ended::Woken {
            self.wake_lightly();
        }
        ended == Ended::Woken
    }

    /// Awake again after a light sleep: plugins hear it (`core.resume`),
    /// the screen is drawn as it was before the sleep screen.
    #[cfg(feature = "esp32c3")]
    fn wake_lightly(&mut self) {
        self.state.sleep = false;
        // The clock and the battery shown anew soon.
        self.state.status_read = None;
        if let Some(ota) = &mut self.ota {
            ota.resume_trial(now().0);
        }
        let effects = self.kernel.resume(now());
        self.apply(effects);
    }

    /// Before sleeping: requests under way (a position pushed) go on until
    /// they are answered, [`SLEEP_WAIT_MS`] at the longest; the radio goes
    /// with the power then. `false` if a press came meanwhile (it waits
    /// for the main loop): no sleep then.
    fn finish_work(&mut self) -> bool {
        let until = now().0 + SLEEP_WAIT_MS;
        if self.kernel.holds_sleep() {
            info!("sleep: finishing work first");
        }
        while self.kernel.holds_sleep() && now().0 < until {
            power::feed();
            if input::waiting() {
                return false;
            }
            self.io();
            for effects in self.network.step(&mut self.kernel, now()) {
                self.apply(effects);
            }
            self.tick();
            self.delay.delay_millis(POLL_MS as u32);
        }
        true
    }

    fn wait_for_power_release(&self) {
        while input::power_down() {
            power::feed();
            self.delay.delay_millis(10);
        }
    }

    /// Carries out what the kernel asks: shows what changed, notes when to
    /// wake it, restarts, notes whether to sleep.
    pub fn apply(&mut self, mut effects: Effects) {
        // From the first work not written back: work coming again and
        // again does not put it off.
        self.state.unsynced.get_or_insert(now().0);
        // The light as set (or shown while a slider moves) at once, not
        // after the frame that shows the setting.
        self.light();
        self.state.pace = effects.pace();
        loop {
            // A redraw or deferred work can replace these effects. Keep
            // the one-shot sleep request before taking either path.
            self.state.sleep |= effects.sleep();
            // Anything but a paced frame waits for the one on its way.
            let paced = effects.refresh().is_some_and(|(_, mode)| self.paced(mode))
                && !effects.restart()
                && !effects.proceed()
                && !effects.grays()
                && self.kernel.rotation() == self.panel.rotation();
            if !paced {
                self.settle_frame();
            }
            if effects.restart() {
                self.restart();
            }
            // Turned (the setting): the same screens, made anew for the
            // turned size and drawn whole, instead of what was asked.
            let rotation = self.kernel.rotation();
            if rotation != self.panel.rotation() {
                self.panel.set_rotation(rotation);
                let (width, height) = self.panel.size();
                info!("turned {rotation:?}: {width}x{height}");
                effects = self.kernel.resize(now(), width, height);
                continue;
            }
            // Slow work ahead: its mark is shown first, the panel
            // refreshing it while the kernel works on, then the frame the
            // work drew. After grays a refresh drives the whole screen
            // and takes long: the work is done without a mark then.
            if effects.proceed()
                && let Some((area, _)) = effects.refresh()
            {
                effects = if self.state.gray_shown || self.state.frame_stale {
                    self.kernel.proceed(now())
                } else {
                    self.show_busy(area)
                };
                continue;
            }
            // Pictures in grays: the whole screen, with the gray waveform;
            // small changes over them (the time, the battery) wait for the
            // next page rather than refresh it all.
            if effects.grays()
                && let Some((area, mode)) = effects.refresh()
            {
                let screen = self.kernel.device().display;
                let small = u32::from(area.width) * u32::from(area.height) * 4
                    < u32::from(screen.width) * u32::from(screen.height);
                if !(self.state.gray_shown && small) {
                    self.show_gray(area, mode.cleans() && self.on_sleep_screen());
                    self.state.frame_stale = false;
                }
            } else if self.state.gray_shown && effects.refresh().is_some() {
                // After grays, the whole screen, not only what changed.
                self.state.gray_shown = false;
                effects = self.kernel.draw_again(now());
                continue;
            } else if let Some((area, mode)) = effects.refresh() {
                if paced {
                    self.queue_frame(area, mode);
                } else {
                    self.show(area, mode);
                }
            }
            for warning in self.kernel.take_warnings() {
                warn!("{warning}");
            }
            self.state.wake_at = effects.wake_at();
            // The cable transfer's answers, as soon as they are there.
            while let Some(line) = self.kernel.next_cable_line() {
                esp_println::println!("{line}");
            }
            return;
        }
    }

    /// What came over USB: the cable transfer's lines for the kernel (all
    /// that is there, a few at most), the test remote control's commands
    /// (`remote` builds, one at a time).
    #[inline(never)]
    fn usb(&mut self) {
        for _ in 0..CABLE_LINES {
            let Some(line) = self.usb.next_line() else {
                break;
            };
            match line {
                Ok(line) if line.starts_with(paperpaw_core::CABLE_PREFIX.as_bytes()) => {
                    self.state.cable_at = Some(now().0);
                    let line = alloc::string::String::from_utf8_lossy(&line);
                    let at = self.kernel_now();
                    let effects = self.kernel.cable_line(at, &line);
                    self.apply(effects);
                }
                #[cfg(feature = "remote")]
                line => {
                    self.remote(line);
                    break;
                }
                #[cfg(not(feature = "remote"))]
                _ => {}
            }
        }
        #[cfg(feature = "remote")]
        if self.state.held.is_some_and(|(_, at)| at <= now()) {
            self.release_held();
        }
    }

    /// Whether the cable transfer's lines are coming (one came a moment
    /// ago, or one is half there).
    fn cable_moving(&self) -> bool {
        self.usb.in_line()
            || self
                .state
                .cable_at
                .is_some_and(|at| now().0 < at + CABLE_QUIET_MS)
    }

    /// Refreshes the panel: on battery the processor sleeps lightly for
    /// as long as the last refresh of the kind took, instead of watching
    /// the panel's busy line all along; then waits for the rest. The
    /// panel's power goes off a while after.
    fn refresh(&mut self, refresh: Refresh) -> Result<(), paperpaw_x4::display::Error> {
        let kind = kind(refresh);
        let started = now();
        self.panel.take_busy_ms();
        self.panel.start_refresh(refresh, false)?;
        let start_busy = self.panel.take_busy_ms();
        // What the start took: the setup and the new frame sent (the rest
        // of the time is the waveform and the old frame sent after it).
        let start_ms = now().since(started);
        // A little early: the rest is watched. In naps no longer than the
        // idle ones, the buttons read after each: the readings stop in a
        // light sleep, and a click within a refresh would be missed, a
        // release seen only after it (a press that looks long). None
        // while a press or a release settles.
        let expected = self.state.refresh_ms[kind].saturating_sub(20);
        let idle = self.idle_ms();
        let until = started.0 + expected;
        while expected > 0 && !input::settling() && power::nap_until(until.min(now().0 + idle)) {}
        let woke = now();
        let waited = self.panel.wait();
        let took = now().since(started);
        let busy = self.panel.take_busy_ms();
        info!(
            "refresh {refresh:?}: started in {start_ms} ms (waited {start_busy}), took {took} ms in all (waited {busy} more)"
        );
        let slot = &mut self.state.refresh_ms[kind];
        if now().since(woke) > 1 || expected == 0 {
            // Still busy after the nap: what it took is its time. The
            // shortest one counts (one after the panel's power was off,
            // or the first after a start, takes longer).
            *slot = if *slot == 0 { took } else { (*slot).min(took) };
        } else {
            // Done before the nap ended: it took less, by how much is not
            // known; sleep less next time.
            *slot = slot.saturating_sub(50);
        }
        self.state.panel_idle_at = Some(now().0 + self.panel_idle_ms());
        waited
    }

    /// Shows the busy mark in `area` and lets the kernel work while the
    /// panel refreshes; what the work brought.
    fn show_busy(&mut self, area: Rect) -> Effects {
        power::mark(power::at::SHOWING);
        let send_again = !self.panel.keeps_frame();
        let mut sent = Sent::new(heap_free);
        let panel = &mut self.panel;
        self.kernel.render(&mut |top, rows| {
            if let Err(e) = panel.write_rows(top, rows, Planes::NEW) {
                warn!("display: {e:?}");
            }
            if send_again {
                sent.keep(top, &rows);
            }
        });
        let started = now();
        let refreshing = self.panel.start_refresh(Refresh::Fast, false).is_ok();
        let next = self.kernel.proceed(now());
        let worked = now();
        if refreshing && let Err(e) = self.panel.wait() {
            warn!("display: {e:?}");
        }
        self.state.panel_idle_at = Some(now().0 + self.panel_idle_ms());
        // As after every refresh, both memories hold what is shown.
        let panel = &mut self.panel;
        let kept = !send_again
            || sent.replay(&mut |top, rows| {
                if let Err(e) = panel.write_rows(top, rows, Planes::BOTH) {
                    warn!("display: {e:?}");
                }
            });
        if !kept {
            warn!("busy mark not kept");
        }
        info!(
            "mark {},{} {}x{}: work took {} ms, waited {} ms more",
            area.x,
            area.y,
            area.width,
            area.height,
            worked.since(started),
            now().since(worked)
        );
        next
    }

    /// Shows the whole screen in four grays, its pictures best so (see
    /// `Effects::grays`); the next change draws it whole again. Where
    /// there is no memory for it, as a full refresh of `area`.
    /// Whether the sleep screen is on top: the one picture screen that is
    /// shown to be cleaned (a page of a book with pictures is not: its
    /// periodic full refresh keeps the quicker first stage).
    fn on_sleep_screen(&self) -> bool {
        self.kernel
            .active_screen()
            .is_some_and(|id| id.as_str() == "sleep.screen")
    }

    fn show_gray(&mut self, area: Rect, clean: bool) {
        power::mark(power::at::SHOWING);
        let started = now();
        let panel = &mut self.panel;
        let mut failed = None;
        let drawn = self.kernel.render_gray(&mut |top, rows| {
            if let Err(e) = panel.write_gray_rows(top, rows) {
                failed = Some(e);
            }
        });
        if !drawn {
            warn!("no memory for grays");
            return self.show(area, RefreshMode::Full);
        }
        let written = now();
        self.kernel.showing(written);
        // The sleep screen gets every pixel driven first, where the panel
        // needs it (the X4 Pro); else a fast refresh.
        let refreshed = if clean {
            self.panel.refresh_gray_clean()
        } else {
            self.panel.refresh_gray()
        };
        if let Err(e) = refreshed {
            failed = Some(e);
        }
        self.state.panel_idle_at = Some(now().0 + self.panel_idle_ms());
        let shown = now();
        self.kernel.panel_busy(shown.since(written));
        self.kernel.shown(shown);
        let ms = core::time::Duration::from_millis;
        self.kernel.frame_shown(
            area,
            RefreshMode::Gray,
            ms(written.since(started)),
            ms(shown.since(written)),
            shown,
        );
        self.state.gray_shown = true;
        if let Some(e) = failed {
            warn!("display: {e:?}");
        }
        info!(
            "frame in grays: drawn in {} ms, shown in {} ms",
            written.since(started),
            shown.since(written)
        );
    }

    /// Shows the frame changed in `area`.
    fn show(&mut self, area: Rect, mode: RefreshMode) {
        power::mark(power::at::SHOWING);
        let stale = core::mem::take(&mut self.state.frame_stale);
        let refresh = match mode {
            _ if stale => Refresh::Half,
            // Every pixel driven with a warm panel's waveform (1.6 s), as
            // the stock firmware cleans the panel every few pages.
            RefreshMode::Full | RefreshMode::Clear => Refresh::Half,
            // Partial and fast, and modes to come.
            _ => Refresh::Fast,
        };
        let planes = if refresh == Refresh::Fast {
            Planes::NEW
        } else {
            Planes::BOTH
        };
        let mut failed = None;
        let started = now();
        // What is sent is kept for sending again after the refresh, unless
        // the panel's driver keeps the frame itself.
        let send_again = !self.panel.keeps_frame();
        let mut sent = Sent::new(heap_free);
        // What went into each memory of the controller, to check that the
        // frame shown is the frame sent (test builds).
        #[cfg(feature = "remote")]
        let (mut written, mut again_written) = (Check::default(), Check::default());
        let panel = &mut self.panel;
        // How much of drawing is the bands going into the panel (the rest
        // is the screens drawing).
        let mut writing = 0;
        self.kernel.render(&mut |top, rows| {
            let before = now();
            if let Err(e) = panel.write_rows(top, rows, planes) {
                failed = Some(e);
            }
            writing += now().since(before);
            if send_again {
                #[cfg(feature = "remote")]
                written.add(top, &rows);
                sent.keep(top, &rows);
            }
        });
        let drawn = now();
        self.kernel.showing(drawn);
        self.set_waveform(refresh, mode);
        if let Err(e) = self.refresh(refresh) {
            failed = Some(e);
        }
        let shown = now();
        self.kernel.panel_busy(shown.since(drawn));
        self.kernel.shown(shown);
        let ms = core::time::Duration::from_millis;
        self.kernel.frame_shown(
            area,
            mode,
            ms(drawn.since(started)),
            ms(shown.since(drawn)),
            shown,
        );
        // After every refresh, both memories again hold the frame now
        // shown, as the stock firmware does: the refresh may leave the
        // memory of the new frame changed, and the next fast refresh
        // compares the two (sent again as kept, or drawn anew). A driver
        // that keeps the frame sends it again by itself.
        if send_again {
            let panel = &mut self.panel;
            let mut again = |top: u16, rows: paperpaw_platform::FrameView<'_>| {
                if let Err(e) = panel.write_rows(top, rows, Planes::BOTH) {
                    failed = Some(e);
                }
                #[cfg(feature = "remote")]
                again_written.add(top, &rows);
            };
            let kept = sent.replay(&mut again);
            if !kept {
                self.kernel.rerender(&mut again);
            }
            #[cfg(feature = "remote")]
            if again_written != written {
                warn!(
                    "frame shown differs from frame sent ({}): {written:?} then {again_written:?}",
                    if kept { "kept" } else { "drawn anew" }
                );
            }
        }
        if let Some(e) = failed {
            warn!("display: {e:?}");
        }
        info!(
            "frame {mode:?} {},{} {}x{}: drawn in {} ms (writing {writing} of them), shown in {} ms, sent again in {} ms",
            area.x,
            area.y,
            area.width,
            area.height,
            drawn.since(started),
            shown.since(drawn),
            now().since(shown)
        );
        // Where the heap stands, for finding what takes it (test builds).
        #[cfg(feature = "remote")]
        info!(
            "heap {} used, peak {}",
            esp_alloc::HEAP.used(),
            crate::peak::peak()
        );
    }
}

/// The heap's free bytes, for keeping what was sent.
fn heap_free() -> usize {
    esp_alloc::HEAP.free()
}

/// The board as [`paperpaw_x4::sleeping::run`] sees it: the firmware for
/// the length of one sleep.
#[cfg(feature = "esp32c3")]
struct Light<'a> {
    firmware: &'a mut Firmware,
}

#[cfg(feature = "esp32c3")]
impl paperpaw_x4::sleeping::Board for Light<'_> {
    fn now_ms(&mut self) -> u64 {
        now().0
    }

    fn look(&mut self) -> paperpaw_x4::sleeping::Look {
        paperpaw_x4::sleeping::Look {
            now_ms: now().0,
            usb: self.firmware.usb_power.is_high(),
            percent: input::percent_now(),
        }
    }

    fn panel_down(&mut self) {
        let _ = self.firmware.panel.power_down();
    }

    fn button_released(&mut self) -> bool {
        let until = now().0 + paperpaw_x4::sleeping::HELD_MS;
        // Released first: held, the button would wake it at once.
        while input::power_down() {
            if now().0 > until {
                return false;
            }
            power::feed();
            self.firmware.delay.delay_millis(10);
        }
        true
    }

    fn leave_radios(&mut self) {
        self.firmware.network.leave();
    }

    fn radios_off(&mut self) -> bool {
        !self.firmware.network.radio_on()
    }

    fn pause_inputs(&mut self) {
        input::pause();
    }

    fn resume_inputs(&mut self) {
        input::resume();
    }

    fn watchdog_for_sleep(&mut self) {
        power::watchdogs_for_sleep(paperpaw_x4::sleeping::SLEEP_WATCHDOG_MS);
    }

    fn watchdog_awake(&mut self) {
        power::watchdogs_awake();
    }

    fn feed(&mut self) {
        power::feed();
    }

    fn nap(&mut self, until_ms: u64) -> paperpaw_x4::sleeping::Napped {
        use paperpaw_x4::sleeping::Napped;
        if !power::sleep_asleep(until_ms) {
            return Napped::Ended;
        }
        // A press, not a glitch: still down after a moment.
        self.firmware.delay.delay_millis(20);
        if input::power_pin_low() {
            Napped::Pressed
        } else {
            Napped::Glitch
        }
    }

    fn say(&mut self, what: &str) {
        info!("sleeping: {what}");
    }
}

/// What came over USB.
#[cfg(feature = "remote")]
impl Firmware {
    #[inline(never)]
    fn remote(&mut self, line: Result<alloc::vec::Vec<u8>, crate::usb_lines::TooLong>) {
        self.settle_frame();
        if let Some(command) = self.remote.command(line)
            && let Some(command) = self.remote.answer(&mut self.kernel, &mut self.usb, command)
        {
            self.remote_command(command);
        }
    }

    /// Lets go of the button pressed over USB, if one is held, and says
    /// so.
    fn release_held(&mut self) {
        let Some((name, _)) = self.state.held.take() else {
            return;
        };
        let started = now();
        let at = self.kernel_now();
        let state = ButtonState::Released;
        let effects = self.kernel.handle(at, InputEvent::Button { name, state });
        info!("{name}: {} ms", now().since(started));
        self.apply(effects);
        crate::remote::ok();
    }

    /// Opens a slot (`slot`) or a screen by its id, with parameters, over
    /// USB.
    fn open_remote(
        &mut self,
        id: &str,
        params: alloc::vec::Vec<(alloc::string::String, alloc::string::String)>,
        slot: bool,
    ) {
        let Ok(id) = paperpaw_sdk::Id::parse(id) else {
            return crate::remote::error(id);
        };
        let mut with = paperpaw_sdk::Params::new();
        for (key, value) in params {
            with.set(key, value);
        }
        let at = self.kernel_now();
        let effects = if slot {
            self.kernel.open_slot(at, id, with)
        } else {
            self.kernel.open_screen(at, id, with)
        };
        self.apply(effects);
        crate::remote::ok();
    }

    /// Carries out a command over USB that changes the X4's state.
    fn remote_command(&mut self, command: paperpaw_x4::remote::Command) {
        use crate::remote;
        use paperpaw_x4::remote::Command;
        match command {
            Command::Network(ask) => self.network.ask_remote(ask),
            Command::IdleSleep(seconds) => {
                // At most ten minutes: the link is gone meanwhile.
                self.state.sleep_on_usb_until = Some(Instant(now().0 + seconds.min(600) * 1000));
                remote::ok();
            }
            Command::Run(action) => {
                let Ok(id) = paperpaw_sdk::Id::parse(&action) else {
                    return remote::error(&action);
                };
                let at = self.kernel_now();
                let started = now();
                let effects = self.kernel.run_action(at, id);
                info!("{action}: {} ms", now().since(started));
                self.apply(effects);
                remote::ok();
            }
            Command::Set { id, value } => {
                let at = self.kernel_now();
                match self.kernel.set_setting(at, &id, &value) {
                    Ok(effects) => {
                        info!("{id} = {value}");
                        self.apply(effects);
                        remote::ok();
                    }
                    Err(why) => remote::error(why),
                }
            }
            Command::Open(path) => {
                // With the reading app chosen for it.
                let at = self.kernel_now();
                let effects = self.kernel.open_book(at, &path);
                self.apply(effects);
                remote::ok();
            }
            Command::Gray(picture) => {
                let shown = crate::gray_test::show(
                    &mut self.panel,
                    self.kernel.storage(),
                    picture.as_deref(),
                );
                if let Err(e) = shown {
                    warn!("display: {e:?}");
                }
                self.state.gray_shown = true;
                remote::ok();
            }
            Command::GrayTone(tone) => {
                self.panel.set_gray_tone(tone);
                remote::ok();
            }
            Command::Refresh { full } => {
                let refresh = if full { Refresh::Full } else { Refresh::Fast };
                remote::refreshed(self.panel.refresh(refresh, false));
                self.state.panel_idle_at = Some(now().0 + self.panel_idle_ms());
            }
            Command::Slot { id, params } => self.open_remote(&id, params, true),
            Command::Show { id, params } => self.open_remote(&id, params, false),
            Command::Emit { event, params } => {
                let Ok(id) = paperpaw_sdk::Id::parse(&event) else {
                    return remote::error(&event);
                };
                let mut with = paperpaw_sdk::Params::new();
                for (key, value) in params {
                    with.set(key, value);
                }
                let at = self.kernel_now();
                let effects = self.kernel.send_event(at, id, with);
                self.apply(effects);
                remote::ok();
            }
            Command::Type { text, done } => {
                let at = self.kernel_now();
                match self.kernel.type_text(at, &text, done) {
                    Some(effects) => {
                        self.apply(effects);
                        remote::ok();
                    }
                    None => remote::error("no text field"),
                }
            }
            Command::Drag {
                from,
                to,
                steps,
                ms,
            } => {
                input::drag(from, to, steps, ms);
                remote::ok();
            }
            Command::Clicks {
                button,
                count,
                every,
                held,
                jitter,
            } => {
                input::clicks(button, count, every, held, jitter);
                remote::ok();
            }
            Command::Touch { phase, x, y } => {
                use paperpaw_x4::remote::Touch;
                let phase = match phase {
                    Touch::Down => PointerPhase::Down,
                    Touch::Move => PointerPhase::Move,
                    Touch::Up => PointerPhase::Up,
                };
                let event = InputEvent::Pointer(PointerEvent {
                    id: 0,
                    phase,
                    point: Point::new(x, y),
                    tool: PointerTool::Finger,
                    pressure: 1000,
                });
                let at = self.kernel_now();
                let effects = self.kernel.handle(at, event);
                self.apply(effects);
                remote::ok();
            }
            Command::SetTime(unix_ms) => {
                crate::clock::set(now(), unix_ms);
                remote::ok();
            }
            Command::Restart => {
                // What plugins keep, saved and on the card first.
                self.pause_trial();
                let at = self.kernel_now();
                let _ = self.kernel.stop(at);
                self.network.leave();
                remote::ok();
                esp_hal::system::software_reset();
            }
            Command::Press { button, ms } => {
                if self.state.held.is_some() {
                    return remote::busy();
                }
                let at = self.kernel_now();
                let state = ButtonState::Pressed;
                let started = now();
                let effects = self.kernel.handle(
                    at,
                    InputEvent::Button {
                        name: button,
                        state,
                    },
                );
                info!("{button} pressed: {} ms", now().since(started));
                self.apply(effects);
                self.state.held = Some((button, Instant(now().0.saturating_add(ms))));
            }
            // Only asking: answered by `remote`.
            _ => {}
        }
    }
}
