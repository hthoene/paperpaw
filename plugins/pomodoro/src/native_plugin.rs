//! The interface-7 Pomodoro plugin: one timer (`state::Timer`) shared by the
//! screen and the three widgets, kept in a small file, ended on time by a work
//! timer (`core.work`) whatever screen is open, and shown once a minute.
//!
//! What the timer decides without a host is in `phase`, `state` and
//! `native_shown`; the widgets are drawn by `native_widgets` and the screen by
//! `native_screen`. This file is the glue: it reads the clock and the settings,
//! routes the host's events and callbacks, schedules the work and tells the
//! widgets and the screen when what they show has changed (`PORTING-7.md`).
use crate::{
    declarations::{
        self as decl, AUTO, BUTTON_TOKEN, GOAL, LIFECYCLE_TOKEN, LONG, OPEN_TOKEN, ROUNDS,
        SCREEN_TOKEN, SHORT, SIGNAL, SKIP_TOKEN, TIMER_TOKEN, TODAY_TOKEN, TOGGLE_TOKEN,
        WORK_TOKEN,
    },
    native_screen::{Act, Press, ScreenView},
    native_shown::Shown,
    native_widgets,
    phase::{Lengths, Phase},
    state::{FILE, FILE_MAX, Kept, Restored, Timer},
};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Handled, Rect, Storage,
    native::{
        Error, Host, Plugin,
        callbacks::{DrawFrame, DrawPurpose, FrameRect, Input},
        clock,
        commands::{CommandSender, RedrawMode},
        events::{Event, Incoming, kinds},
        formatting::Arg,
        registrations,
        reply::Reply,
        settings::{SettingValue, read_setting},
        snapshot,
        storage::DataStorage,
        ui::{Instance, Notify, WidgetInputKind, WidgetTarget, stages::WidgetCall},
        work::{SPAN_MAX_MS, Work, WorkSpec},
    },
};
use paperpaw_ui_kit::{
    Messages, Screens, paint, read_event,
    widget::{
        Cx,
        cx::zone_offset,
        settings::{Stack, base},
    },
};

mod image {
    include!(concat!(env!("OUT_DIR"), "/pomodoro_declarations.rs"));
}

/// The most bytes of a stage reply (a size and a target).
const REPLY: usize = 512;
/// The target of a widget: all of it.
const TARGET: u32 = 1;
/// The work that ends the phase, the one that draws the minutes again, and the
/// one that brings the front light back.
const DONE_KEY: &str = "pomodoro.done";
const TICK_KEY: &str = "pomodoro.tick";
const LIGHT_KEY: &str = "pomodoro.light";
/// How long the front light stays at the other end, in milliseconds: the least
/// work declared in a callback waits.
const BLINK_MS: u64 = 1_000;
/// How early the work that ends a phase may come (the clocks of the host's work
/// and of this plugin differ a little) and still end it.
const EARLY_MS: u64 = 3_000;
/// The front light's setting.
const FRONTLIGHT: &str = "core.frontlight";
/// The screen's id, for opening it.
const SCREEN: &str = "pomodoro.screen";

/// What the end signal is besides the notice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Signal {
    Notice,
    Flash,
    Light,
}

/// A widget instance the host made.
#[derive(Debug)]
struct Item {
    instance: Instance,
    token: u32,
    /// The widget's id as the host names it, with its placement.
    widget: String,
    /// What it showed when the host was told last.
    told: Option<Shown>,
}

/// The interface-7 Pomodoro plugin.
#[derive(Debug, Default)]
pub struct NativePomodoro {
    timer: Timer,
    /// Whether the kept state was read.
    loaded: bool,
    items: Vec<Item>,
    screens: Screens<ScreenView>,
    work: Option<Work>,
    /// The front light as it was before the blink of the end signal.
    light_back: Option<i64>,
}

impl Plugin for NativePomodoro {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::SETTINGS
            .iter()
            .chain(image::UI)
            .chain(image::NAVIGATION)
            .chain(image::WIDGETS)
            .chain(image::EVENTS)
        {
            registrations::submit(host, batch)?;
        }
        Ok(Self::default())
    }

    fn event(
        &mut self,
        host: &Host<'_>,
        token: u32,
        incoming: Incoming<'_>,
        reply: &mut Reply<'_>,
    ) -> Result<(), Error> {
        let Some(event) = read_event!(
            incoming;
            kinds::Started,
            kinds::Resume,
            kinds::SettingChanged,
            kinds::WorkDue,
            kinds::ActionRun,
            kinds::WidgetMade,
            kinds::WidgetDropped,
            kinds::StageCall,
            kinds::ScreenOpened,
            kinds::ScreenShown,
            kinds::ScreenClosed
        ) else {
            return Ok(());
        };
        match event {
            Event::Started { .. } if token == LIFECYCLE_TOKEN => self.started(host),
            Event::Resume { .. } if token == LIFECYCLE_TOKEN => self.resumed(host),
            Event::SettingChanged { key, .. } if token == LIFECYCLE_TOKEN => {
                self.setting_changed(host, key)
            }
            Event::WorkDue(due) if token == WORK_TOKEN => match due.key() {
                DONE_KEY => self.phase_due(host),
                TICK_KEY => self.tick(host),
                LIGHT_KEY => self.light_back(host),
                _ => Ok(()),
            },
            Event::ActionRun { .. } if token == OPEN_TOKEN => open_screen(host),
            Event::ActionRun { .. } if token == TOGGLE_TOKEN => self.toggle(host, false),
            Event::ActionRun { .. } if token == SKIP_TOKEN => self.press(host, Press::Skip),
            Event::WidgetMade {
                widget, instance, ..
            } => self.made(host, widget, instance),
            Event::WidgetDropped { instance, .. } => {
                self.items.retain(|item| item.instance != instance);
                self.schedule(host)
            }
            Event::StageCall {
                stage: "core.widget",
                payload,
                ..
            } => self.stage(host, payload, reply),
            Event::ScreenOpened { instance, .. } if token == SCREEN_TOKEN => {
                self.load(host)?;
                self.screens.open(instance, ScreenView::default())?;
                self.schedule(host)
            }
            Event::ScreenShown {
                instance: Some(instance),
                ..
            } if token == SCREEN_TOKEN => {
                self.screens.shown(token, instance)?;
                self.schedule(host)
            }
            Event::ScreenClosed { instance, .. } if token == SCREEN_TOKEN => {
                self.screens.close(instance);
                self.schedule(host)
            }
            _ => Ok(()),
        }
    }

    fn draw(&mut self, host: &Host<'_>, token: u32, mut frame: DrawFrame<'_>) -> Result<(), Error> {
        self.load(host)?;
        let shown = self.shown(host)?;
        if token == SCREEN_TOKEN {
            return self
                .screens
                .top(token)
                .ok_or(Error::State)?
                .draw(host, &mut frame, &shown);
        }
        if frame.purpose != DrawPurpose::Widget {
            return Err(Error::Unsupported);
        }
        let item = self.find(frame.id).ok_or(Error::NotFound)?;
        let mut cx = Cx::open(host, &item.widget)?;
        let area = area_of(frame.rect);
        let mut out = Vec::new();
        native_widgets::draw(&mut cx, &mut out, area, item.token, &shown.for_widget())?;
        paint::replay(&mut frame, &cx.font, &out)
    }

    fn input(&mut self, host: &Host<'_>, token: u32, input: Input<'_>) -> Result<Handled, Error> {
        if token == SCREEN_TOKEN {
            return self.screen_input(host, token, &input);
        }
        let Input::Widget {
            instance,
            target: TARGET,
            kind,
            ..
        } = input
        else {
            return Ok(Handled::No);
        };
        let widget = self.find(instance.get()).ok_or(Error::NotFound)?.token;
        // A tap starts or pauses (the count opens the screen); a long press
        // opens the screen from the timer and the count. The button is only a button.
        match (widget, kind) {
            (TIMER_TOKEN | BUTTON_TOKEN, WidgetInputKind::Tap) => self.toggle(host, false)?,
            (TODAY_TOKEN, WidgetInputKind::Tap)
            | (TIMER_TOKEN | TODAY_TOKEN, WidgetInputKind::LongPress) => open_screen(host)?,
            _ => return Ok(Handled::No),
        }
        Ok(Handled::Yes)
    }
}

impl NativePomodoro {
    fn find(&self, instance: u32) -> Option<&Item> {
        self.items
            .iter()
            .find(|item| item.instance.get() == instance)
    }

    // What the host says: the clock, the day, the settings.

    fn now_ms(host: &Host<'_>) -> Result<u64, Error> {
        Ok(clock::micros(host)? / 1000)
    }

    /// The Unix time and today's number (days since 1970 by the local time), if
    /// the device knows the time.
    fn day(host: &Host<'_>) -> Result<(Option<i64>, Option<i32>), Error> {
        let Some(unix) = snapshot::capture_snapshot(host)?.unix_seconds() else {
            return Ok((None, None));
        };
        let local = unix.saturating_add(i64::from(zone_offset(host, unix)?));
        Ok((Some(unix), i32::try_from(local.div_euclid(86_400)).ok()))
    }

    fn int(host: &Host<'_>, key: &str, default: i64) -> Result<i64, Error> {
        let mut scratch = [0; 64];
        Ok(match read_setting(host, key, &mut scratch)? {
            SettingValue::Int(value) => value,
            _ => default,
        })
    }

    fn lengths(host: &Host<'_>) -> Result<Lengths, Error> {
        let read = |key: &str, default: i64, most: i64| -> Result<u32, Error> {
            Ok(u32::try_from(Self::int(host, key, default)?.clamp(1, most)).unwrap_or(1))
        };
        Ok(Lengths {
            focus: read(decl::FOCUS, 25, 90)?,
            short: read(SHORT, 5, 30)?,
            long: read(LONG, 15, 60)?,
            rounds: read(ROUNDS, 4, 8)?,
        })
    }

    /// The daily goal in rounds, if one is set.
    fn goal(host: &Host<'_>) -> Result<Option<u32>, Error> {
        Ok(u32::try_from(Self::int(host, GOAL, 0)?)
            .ok()
            .filter(|goal| *goal > 0))
    }

    fn flag(host: &Host<'_>, key: &str) -> Result<bool, Error> {
        let mut scratch = [0; 64];
        Ok(matches!(
            read_setting(host, key, &mut scratch)?,
            SettingValue::Bool(true)
        ))
    }

    /// What the timer shows now.
    fn shown(&self, host: &Host<'_>) -> Result<Shown, Error> {
        let (_, today) = Self::day(host)?;
        Ok(Shown::of(
            &self.timer,
            &Self::lengths(host)?,
            Self::goal(host)?,
            (Self::now_ms(host)?, today),
        ))
    }

    // The kept state.

    /// Reads the kept state once; a round that was running goes on by the clock.
    fn load(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if self.loaded {
            return Ok(());
        }
        self.loaded = true;
        let mut buffer = [0u8; FILE_MAX + 1];
        let Ok(n) = DataStorage::new(host).read_at(FILE, 0, &mut buffer) else {
            return Ok(());
        };
        let kept = buffer
            .get(..n)
            .filter(|text| text.len() <= FILE_MAX)
            .and_then(|text| core::str::from_utf8(text).ok())
            .and_then(Kept::parse);
        let Some(kept) = kept else {
            return Ok(());
        };
        let (unix, today) = Self::day(host)?;
        let lengths = Self::lengths(host)?;
        let restored = self
            .timer
            .restore(&kept, (Self::now_ms(host)?, unix, today), &lengths);
        self.after_restore(host, restored)
    }

    /// What a round that went on, or ended while the device was off, needs.
    fn after_restore(&mut self, host: &Host<'_>, restored: Restored) -> Result<(), Error> {
        match restored {
            Restored::Idle => Ok(()),
            Restored::Running(_) => self.schedule(host),
            Restored::Over => {
                // Its time ran out meanwhile: it is finished, and told.
                self.timer.running = true;
                self.finish(host)
            }
        }
    }

    fn save(&self, host: &Host<'_>) {
        let mut text = Stack::<FILE_MAX>::new();
        if self.timer.text(&mut text) {
            DataStorage::new(host)
                .write(FILE, text.as_str().as_bytes())
                .ok();
        }
    }

    // Events.

    fn started(&mut self, host: &Host<'_>) -> Result<(), Error> {
        self.load(host)?;
        self.changed(host, false)
    }

    fn resumed(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if !self.loaded {
            return self.load(host);
        }
        let (unix, _) = Self::day(host)?;
        let restored = self.timer.wake(Self::now_ms(host)?, unix);
        self.after_restore(host, restored)?;
        self.changed(host, false)
    }

    fn setting_changed(&mut self, host: &Host<'_>, key: &str) -> Result<(), Error> {
        if key.is_empty() || key.starts_with("pomodoro.") {
            // The lengths, the goal, a widget's look: shown anew.
            self.changed(host, true)?;
        }
        Ok(())
    }

    fn made(&mut self, host: &Host<'_>, widget: &str, instance: Instance) -> Result<(), Error> {
        let token = match base(widget) {
            "pomodoro.timer" => TIMER_TOKEN,
            "pomodoro.button" => BUTTON_TOKEN,
            "pomodoro.today" => TODAY_TOKEN,
            _ => return Ok(()),
        };
        self.items.retain(|item| item.instance != instance);
        self.items.try_reserve(1).map_err(|_| Error::NoMemory)?;
        self.items.push(Item {
            instance,
            token,
            widget: paperpaw_ui_kit::text::copy(widget)?,
            told: None,
        });
        self.load(host)?;
        self.schedule(host)
    }

    // The timer.

    /// Starts, pauses, or goes on. `from_screen`: the reader is looking at the
    /// clock, which updates once a minute: said so when it starts.
    fn toggle(&mut self, host: &Host<'_>, from_screen: bool) -> Result<(), Error> {
        self.load(host)?;
        let now = Self::now_ms(host)?;
        let lengths = Self::lengths(host)?;
        if self.timer.running {
            self.timer.pause(now, &lengths);
        } else {
            let (unix, _) = Self::day(host)?;
            self.timer.start(now, unix, &lengths);
            if from_screen {
                notice(host, "pomodoro.notice.slow");
            }
        }
        self.save(host);
        self.schedule(host)?;
        self.changed(host, true)
    }

    fn press(&mut self, host: &Host<'_>, press: Press) -> Result<(), Error> {
        self.load(host)?;
        match press {
            Press::Toggle => return self.toggle(host, true),
            Press::Skip => {
                let (_, today) = Self::day(host)?;
                self.timer.advance(false, today, &Self::lengths(host)?);
            }
            Press::Reset => self.timer.reset(),
        }
        self.save(host);
        self.schedule(host)?;
        self.changed(host, true)
    }

    /// The work that ends the phase came due.
    fn phase_due(&mut self, host: &Host<'_>) -> Result<(), Error> {
        self.load(host)?;
        let left = self.timer.remaining(
            Self::now_ms(host)?,
            Self::lengths(host)?.ms(self.timer.phase),
        );
        // A timer paused, skipped or started again since: not this one's end.
        if !self.timer.running || left > EARLY_MS {
            return self.schedule(host);
        }
        self.finish(host)
    }

    /// The phase is over: counted, told, signalled, and the next one stands
    /// ready (or runs, where it starts by itself).
    fn finish(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let was = self.timer.phase;
        let (unix, today) = Self::day(host)?;
        let lengths = Self::lengths(host)?;
        self.timer.advance(true, today, &lengths);
        let reached = was == Phase::Focus && Self::goal(host)? == Some(self.timer.log.today());
        let told = match (was, reached) {
            (Phase::Focus, true) => {
                let rounds = i64::from(self.timer.log.today());
                notice_with(host, "pomodoro.notice.goal", &[Arg::Count(rounds)]);
                None
            }
            (Phase::Focus, false) => Some("pomodoro.notice.break"),
            _ => Some("pomodoro.notice.focus"),
        };
        if let Some(key) = told {
            notice(host, key);
        }
        // A signal that cannot be given (no front light, a refusal) is no fault of the round's end.
        self.signal(host).ok();
        // The next phase starts by itself only when asked.
        if Self::flag(host, AUTO)? {
            self.timer.start(Self::now_ms(host)?, unix, &lengths);
        }
        self.save(host);
        self.schedule(host)?;
        self.changed(host, true)
    }

    /// The end signal besides the notice: a flash of the screen (one full
    /// refresh, where this plugin's screen is on top), or a blink of the front
    /// light (where the device has one: else a flash).
    fn signal(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let mut scratch = [0; 64];
        let kind = match read_setting(host, SIGNAL, &mut scratch)? {
            SettingValue::Text(decl::SIGNAL_FLASH) => Signal::Flash,
            SettingValue::Text(decl::SIGNAL_LIGHT) => Signal::Light,
            _ => Signal::Notice,
        };
        if kind == Signal::Notice {
            return Ok(());
        }
        if kind == Signal::Light {
            if self.light_back.is_some() {
                return Ok(());
            }
            let mut scratch = [0; 64];
            if let SettingValue::Int(now) = read_setting(host, FRONTLIGHT, &mut scratch)? {
                let other = if now >= 50 { 0 } else { 100 };
                let mut scratch = [0; 128];
                let set = CommandSender::new(host, &mut scratch)
                    .set_setting(FRONTLIGHT, SettingValue::Int(other));
                if set.is_ok() {
                    self.light_back = Some(now);
                    // No timer to bring it back: it goes back at once.
                    if self.declare(host, LIGHT_KEY, BLINK_MS).is_err() {
                        self.light_back(host)?;
                    }
                    return Ok(());
                }
            }
        }
        // A flash, or a blink on a device without a front light. A refusal
        // (this plugin's screen is not on top) is not an error.
        let mut scratch = [0; 64];
        CommandSender::new(host, &mut scratch)
            .queue_redraw(RedrawMode::Full)
            .ok();
        Ok(())
    }

    fn light_back(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if let Some(now) = self.light_back.take() {
            let mut scratch = [0; 128];
            CommandSender::new(host, &mut scratch)
                .set_setting(FRONTLIGHT, SettingValue::Int(now))?;
        }
        Ok(())
    }

    // Work.

    fn declare(&mut self, host: &Host<'_>, key: &str, after_ms: u64) -> Result<(), Error> {
        if self.work.is_none() {
            self.work = Some(Work::open(host)?);
        }
        let work = self.work.as_ref().ok_or(Error::State)?;
        let after = u32::try_from(after_ms.min(u64::from(SPAN_MAX_MS))).unwrap_or(SPAN_MAX_MS);
        let spec = WorkSpec::new(key, WORK_TOKEN)?.after_ms(after);
        let (mut input, mut out) = ([0u8; 256], [0u8; 64]);
        work.declare(host, &spec, &mut input, &mut out)
    }

    fn cancel(&mut self, host: &Host<'_>, key: &str) -> Result<(), Error> {
        let Some(work) = self.work.as_ref() else {
            return Ok(());
        };
        let (mut input, mut out) = ([0u8; 256], [0u8; 64]);
        work.cancel(host, key, &mut input, &mut out)
    }

    /// Declares what the timer's state needs: the end of the phase, and the tick
    /// that draws the minutes again while a widget or the screen shows them.
    fn schedule(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if !self.timer.running {
            self.cancel(host, DONE_KEY)?;
            return self.cancel(host, TICK_KEY);
        }
        let shown = self.shown(host)?;
        let left = self.timer.remaining(
            Self::now_ms(host)?,
            Self::lengths(host)?.ms(self.timer.phase),
        );
        self.declare(host, DONE_KEY, left)?;
        let watched = self.screens_open() || !self.items.is_empty();
        if !watched {
            return self.cancel(host, TICK_KEY);
        }
        // Once a minute on the minute of what is left; every ten seconds in the
        // last one, on the full screen only (the widgets show minutes).
        let wait = if self.screens_open() {
            shown.tick_ms
        } else {
            let into = left % 60_000;
            if into == 0 { 60_000 } else { into }.max(1_000)
        };
        self.declare(host, TICK_KEY, wait)
    }

    fn screens_open(&self) -> bool {
        self.screens.top_instance(SCREEN_TOKEN).is_some()
    }

    /// The tick: what the widgets and the screen show of the minutes is drawn
    /// again, and the next tick declared.
    fn tick(&mut self, host: &Host<'_>) -> Result<(), Error> {
        self.load(host)?;
        if !self.timer.running {
            return Ok(());
        }
        self.changed(host, false)?;
        self.schedule(host)
    }

    // What is drawn again.

    /// Tells every widget whose picture changed, and draws the screen again:
    /// the whole screen where `whole` (a button, a setting), else only the
    /// block of the phase, the clock and the bar.
    fn changed(&mut self, host: &Host<'_>, whole: bool) -> Result<(), Error> {
        let shown = self.shown(host)?.for_widget();
        let mut scratch = [0; 64];
        let mut commands = CommandSender::new(host, &mut scratch);
        for item in &mut self.items {
            // Settings may have changed a look the picture does not show: the
            // widget is told then, as well.
            if whole || item.told != Some(shown) {
                commands.widget_changed(item.instance).ok();
                item.told = Some(shown);
            }
        }
        let Some(view) = self.screens.top(SCREEN_TOKEN) else {
            return Ok(());
        };
        let block = view.block();
        let mut scratch = [0; 64];
        let mut commands = CommandSender::new(host, &mut scratch);
        // A refusal (the screen is not on top) is not an error of what changed it.
        match (whole, block) {
            (false, Some(block)) => commands
                .queue_redraw_area(RedrawMode::Partial, paint::frame_rect(block))
                .ok(),
            _ => commands.queue_redraw(RedrawMode::Partial).ok(),
        };
        Ok(())
    }

    // The screen.

    fn screen_input(
        &mut self,
        host: &Host<'_>,
        token: u32,
        input: &Input<'_>,
    ) -> Result<Handled, Error> {
        let Some(view) = self.screens.top(token) else {
            return Ok(Handled::No);
        };
        let act = view.input(input);
        let reach = match act {
            Act::Moved(from, to) => view.reach(from, to),
            _ => None,
        };
        match act {
            Act::Ignored => return Ok(Handled::No),
            Act::Moved(..) => {
                let mut scratch = [0; 64];
                let mut commands = CommandSender::new(host, &mut scratch);
                match reach {
                    Some(area) => commands
                        .queue_redraw_area(RedrawMode::Partial, paint::frame_rect(area))
                        .ok(),
                    None => commands.queue_redraw(RedrawMode::Partial).ok(),
                };
            }
            Act::Press(press) => self.press(host, press)?,
            Act::Back => {
                let mut scratch = [0; 64];
                CommandSender::new(host, &mut scratch).back().ok();
            }
        }
        Ok(Handled::Yes)
    }

    // The widgets' stage: their height and where they sit.

    fn stage(
        &mut self,
        host: &Host<'_>,
        payload: &[u8],
        reply: &mut Reply<'_>,
    ) -> Result<(), Error> {
        let call = WidgetCall::decode(payload)?;
        let mut out = paperpaw_ui_kit::mem::buffer(REPLY)?;
        let written = match call {
            WidgetCall::Measure { instance, width } => {
                let item = self.find(instance.get()).ok_or(Error::NotFound)?;
                let (token, widget) = (item.token, paperpaw_ui_kit::text::copy(&item.widget)?);
                let cx = Cx::open(host, &widget)?;
                let width = u16::try_from(width).unwrap_or(u16::MAX);
                let height = native_widgets::height(&cx, token, width)?;
                call.reply_size(u32::from(height), u32::from(height), &mut out)?
            }
            WidgetCall::Place { instance, .. } => {
                self.find(instance.get()).ok_or(Error::NotFound)?;
                call.reply_place(&mut out)?
            }
            WidgetCall::Targets { instance, rect } => {
                self.find(instance.get()).ok_or(Error::NotFound)?;
                call.reply_targets(&[WidgetTarget::new(TARGET, rect)], &mut out)?
            }
            _ => return Err(Error::Unsupported),
        };
        reply.write(out.get(..written).ok_or(Error::NoSpace)?)
    }
}

/// The rectangle of a drawing as the widgets' own.
fn area_of(rect: FrameRect) -> Rect {
    let clamp = paperpaw_ui_kit::geometry::clamp_u16;
    Rect::new(
        clamp(i64::from(rect.x)),
        clamp(i64::from(rect.y)),
        clamp(i64::from(rect.width)),
        clamp(i64::from(rect.height)),
    )
}

/// Opens the screen.
fn open_screen(host: &Host<'_>) -> Result<(), Error> {
    let mut scratch = [0; 256];
    CommandSender::new(host, &mut scratch).open_screen(SCREEN)
}

/// The notice `key`, shown to the reader. A courtesy: one that cannot be shown
/// (no text, no room, the host refuses) changes nothing else.
fn notice(host: &Host<'_>, key: &str) {
    notice_with(host, key, &[]);
}

fn notice_with(host: &Host<'_>, key: &str, args: &[Arg<'_>]) {
    let shown = (|| {
        let mut messages = Messages::new()?;
        let text = if args.is_empty() {
            messages.copy(host, key)?
        } else {
            messages.format(host, key, args)?
        };
        let mut scratch = [0; 512];
        CommandSender::new(host, &mut scratch).notify(Notify::new(&text)?)
    })();
    let _ = shown;
}
