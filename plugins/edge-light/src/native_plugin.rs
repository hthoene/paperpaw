//! The interface-7 Edge light plugin: the input filter that follows a finger along
//! an edge (`swipe::Tracker`), the setting of the light, the notice of the new
//! level, the action *Light on or off* and the widget.
//!
//! The filter answers *pass* to every input but two: a swipe of ours and the
//! vertical swipe the kernel makes of that finger when it lifts (*take*, so
//! that no page turns). A swipe is one *preview* after another (the host shows
//! the level the finger is at, no card write, from the swipe's first sample on,
//! which also takes the finger from the screens); when the finger lifts the
//! filter answers *run* the action `edge-light.step` (a filter may send no
//! command, the action sets the level once and says it).
use crate::{
    declarations::{
        FILTER_STAGE, FILTER_TOKEN, FN_BRIGHTNESS, FN_OFF, FN_WARMTH, LEFT, LIFECYCLE_TOKEN, RIGHT,
        STEP_ACTION, STEP_TOKEN, TOGGLE_TOKEN, WIDGET_TOKEN, WIDTH, WIDTH_DEFAULT,
    },
    native_widget,
    swipe::{self, Out, Phase, Side, Tracker, Zone},
};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Rect,
    native::{
        Error, Host, Plugin,
        callbacks::{DrawFrame, DrawPurpose, FrameRect, Input, InputGestureKind, PointerPhase},
        commands::CommandSender,
        device::{Device, DeviceCapability},
        events::{Event, Incoming, kinds},
        formatting::Arg,
        registrations,
        reply::Reply,
        settings::{SettingValue, encode_value, read_setting},
        snapshot,
        stage_schemas::core_input_filter_v1 as stage,
        ui::{Instance, Notify, WidgetInputKind, WidgetTarget, stages::WidgetCall},
    },
};
use paperpaw_ui_kit::{
    Messages, paint, read_event,
    widget::{Cx, settings::base},
};

mod image {
    include!(concat!(env!("OUT_DIR"), "/edge_light_declarations.rs"));
}

/// The front light's settings.
const BRIGHTNESS: &str = "core.frontlight";
const WARMTH: &str = "core.frontlight-warmth";
/// The most bytes of a stage reply (a size and a target).
const REPLY: usize = 512;
/// The target of the widget: all of it.
const TARGET: u32 = 1;
/// `core.device` answers need a buffer of the schema's capacity.
const DEVICE_BUFFER: usize = 4096;
/// The level a switch-on goes to when the light was never seen lit here.
const SWITCH_ON: i64 = 50;
/// The least level a switch-on goes to, whatever it was before.
const LEAST_ON: i64 = 5;
/// How much of the screen's height is the whole range of the light.
const TRAVEL_PERCENT: u32 = 60;
/// How far, in millimetres, a finger moves before it is a swipe or not one.
const SLOP_MM: u32 = 3;
/// The verdicts of `core.input-filter`.
const PASS: u32 = 0;
const TAKE: u32 = 1;
const RUN: u32 = 2;
/// A take that shows a setting's value until the finger lifts (revision 1).
const PREVIEW: u32 = 6;
/// The most bytes of a setting value in a verdict (`filter.verdict.value`).
const VALUE: usize = 64;
/// What the filter answers to one input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Answer {
    Pass,
    Take,
    /// Keep the level of the swipe that ended (the action `step`).
    Run,
    /// Show this level of `function` while the finger goes on.
    Preview {
        function: Function,
        level: i64,
    },
}

/// What the strips are, as read from the settings (until one changes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Strips {
    left: Function,
    right: Function,
    /// In percent of the screen's width.
    width: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Function {
    Brightness,
    Warmth,
    Off,
}

impl Function {
    fn of(text: &str) -> Self {
        match text {
            FN_BRIGHTNESS => Self::Brightness,
            FN_WARMTH => Self::Warmth,
            FN_OFF => Self::Off,
            _ => Self::Off,
        }
    }

    fn setting(self) -> Option<&'static str> {
        match self {
            Self::Brightness => Some(BRIGHTNESS),
            Self::Warmth => Some(WARMTH),
            Self::Off => None,
        }
    }

    const fn notice(self) -> &'static str {
        match self {
            Self::Warmth => "edge-light.level.warmth",
            _ => "edge-light.level.brightness",
        }
    }
}

/// A swipe going on: what it sets, where it began and what it has set.
#[derive(Debug, Clone, Copy)]
struct Active {
    function: Function,
    start: i64,
    last: i64,
}

/// A level the filter asked for and the action *step* has yet to set.
#[derive(Debug, Clone, Copy)]
struct Pending {
    function: Function,
    level: i64,
}

/// A widget instance the host made.
#[derive(Debug)]
struct Item {
    instance: Instance,
    /// The widget's id as the host names it, with its placement.
    widget: String,
}

/// The interface-7 Edge light plugin.
#[derive(Debug, Default)]
pub struct NativeEdgeLight {
    tracker: Tracker,
    /// Whether the device has a touch screen and a front light; read once.
    usable: Option<bool>,
    strips: Option<Strips>,
    /// The screen as the strips need it, read at every finger down.
    zone: Option<(Zone, u16)>,
    active: Option<Active>,
    pending: Option<Pending>,
    /// The brightness before the light was switched off here.
    on_level: Option<i64>,
    items: Vec<Item>,
}

impl Plugin for NativeEdgeLight {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::SETTINGS
            .iter()
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
            kinds::StageCall,
            kinds::SettingChanged,
            kinds::ActionRun,
            kinds::WidgetMade,
            kinds::WidgetDropped
        ) else {
            return Ok(());
        };
        match event {
            Event::StageCall {
                stage: FILTER_STAGE,
                payload,
                ..
            } if token == FILTER_TOKEN => self.filter(host, payload, reply),
            Event::StageCall {
                stage: "core.widget",
                payload,
                ..
            } => self.widget_stage(host, payload, reply),
            Event::SettingChanged { key, .. } if token == LIFECYCLE_TOKEN => {
                self.setting_changed(host, key)
            }
            Event::ActionRun { .. } if token == TOGGLE_TOKEN => self.toggle(host),
            Event::ActionRun { .. } if token == STEP_TOKEN => self.step(host),
            Event::WidgetMade {
                widget, instance, ..
            } => self.made(widget, instance),
            Event::WidgetDropped { instance, .. } => {
                self.items.retain(|item| item.instance != instance);
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn draw(&mut self, host: &Host<'_>, token: u32, mut frame: DrawFrame<'_>) -> Result<(), Error> {
        if token != WIDGET_TOKEN || frame.purpose != DrawPurpose::Widget {
            return Err(Error::Unsupported);
        }
        let usable = self.usable(host)?;
        let item = self.find(frame.id).ok_or(Error::NotFound)?;
        let mut cx = Cx::open(host, &item.widget)?;
        let level = usable.then(|| level_of(host, BRIGHTNESS).unwrap_or(0));
        let mut out = Vec::new();
        native_widget::draw(&mut cx, &mut out, area_of(frame.rect), level)?;
        paint::replay(&mut frame, &cx.font, &out)
    }

    fn input(&mut self, host: &Host<'_>, token: u32, input: Input<'_>) -> Result<Handled, Error> {
        let Input::Widget {
            instance,
            target: TARGET,
            kind: WidgetInputKind::Tap,
            ..
        } = input
        else {
            return Ok(Handled::No);
        };
        if token != WIDGET_TOKEN || self.find(instance.get()).is_none() {
            return Ok(Handled::No);
        }
        self.toggle(host)?;
        Ok(Handled::Yes)
    }
}

use paperpaw_sdk::Handled;

impl NativeEdgeLight {
    fn find(&self, instance: u32) -> Option<&Item> {
        self.items
            .iter()
            .find(|item| item.instance.get() == instance)
    }

    /// Whether the device has a touch screen and a front light.
    fn usable(&mut self, host: &Host<'_>) -> Result<bool, Error> {
        if let Some(usable) = self.usable {
            return Ok(usable);
        }
        let device = Device::open(host)?;
        let found = (|| {
            let mut input = paperpaw_ui_kit::mem::buffer(64)?;
            let mut out = paperpaw_ui_kit::mem::buffer(DEVICE_BUFFER)?;
            let identity = device.device(host, &mut input, &mut out)?;
            Ok(identity.supports(DeviceCapability::Touch)
                && identity.supports(DeviceCapability::Frontlight))
        })();
        let released = device.release(host);
        let usable = found?;
        released?;
        self.usable = Some(usable);
        Ok(usable)
    }

    fn strips(&mut self, host: &Host<'_>) -> Result<Strips, Error> {
        if let Some(strips) = self.strips {
            return Ok(strips);
        }
        let function = |key: &str, default: &str| {
            let mut scratch = [0; 64];
            match read_setting(host, key, &mut scratch) {
                Ok(SettingValue::Text(text)) => Function::of(text),
                _ => Function::of(default),
            }
        };
        let mut scratch = [0; 32];
        let width = match read_setting(host, WIDTH, &mut scratch) {
            Ok(SettingValue::Int(width)) => width,
            _ => WIDTH_DEFAULT,
        };
        let strips = Strips {
            left: function(LEFT, FN_BRIGHTNESS),
            right: function(RIGHT, FN_WARMTH),
            width,
        };
        self.strips = Some(strips);
        Ok(strips)
    }

    fn setting_changed(&mut self, host: &Host<'_>, key: &str) -> Result<(), Error> {
        if key.is_empty() || key.starts_with("edge-light.") {
            self.strips = None;
        }
        if key.is_empty() || key == BRIGHTNESS || key.starts_with("edge-light.") {
            self.widgets_changed(host);
        }
        Ok(())
    }

    fn widgets_changed(&self, host: &Host<'_>) {
        let mut scratch = [0; 64];
        let mut commands = CommandSender::new(host, &mut scratch);
        for item in &self.items {
            commands.widget_changed(item.instance).ok();
        }
    }

    fn made(&mut self, widget: &str, instance: Instance) -> Result<(), Error> {
        if base(widget) != "edge-light.light" {
            return Ok(());
        }
        self.items.retain(|item| item.instance != instance);
        self.items.try_reserve(1).map_err(|_| Error::NoMemory)?;
        self.items.push(Item {
            instance,
            widget: paperpaw_ui_kit::text::copy(widget)?,
        });
        Ok(())
    }

    // The input filter.

    /// One input: the verdict, written as the stage's one `filter.verdict`.
    fn filter(
        &mut self,
        host: &Host<'_>,
        payload: &[u8],
        reply: &mut Reply<'_>,
    ) -> Result<(), Error> {
        // What cannot be read, or fails, passes: a filter never holds an input up.
        let answer = self.verdict(host, payload).unwrap_or(Answer::Pass);
        let mut value = [0; VALUE];
        let (kind, action, key, value) = match answer {
            Answer::Pass => (PASS, "", "", &[][..]),
            Answer::Take => (TAKE, "", "", &[][..]),
            Answer::Run => (RUN, STEP_ACTION, "", &[][..]),
            Answer::Preview { function, level } => match function.setting() {
                Some(key) => {
                    let bytes = encode_value(SettingValue::Int(level), &mut value)?;
                    (PREVIEW, "", key, bytes)
                }
                None => (PASS, "", "", &[][..]),
            },
        };
        let mut out = [0; 192];
        let written = stage::reply_encode(
            &[stage::Record::FilterVerdict(stage::FilterVerdict::new(
                kind,
                action,
                &[],
                "",
                key,
                value,
            ))],
            &mut out,
        )
        .map_err(|_| Error::Invalid)?;
        reply.write(out.get(..written).ok_or(Error::NoSpace)?)
    }

    fn verdict(&mut self, host: &Host<'_>, payload: &[u8]) -> Result<Answer, Error> {
        let mut rows = stage::input_decode(payload).map_err(|_| Error::Invalid)?;
        let Some(Ok(stage::Record::FilterInput(input))) = rows.next() else {
            return Ok(Answer::Pass);
        };
        match Input::decode(input.input)? {
            Input::Pointer {
                phase,
                position,
                contact,
                ..
            } => {
                let phase = match phase {
                    PointerPhase::Down => Phase::Down,
                    PointerPhase::Move => Phase::Move,
                    PointerPhase::Up => Phase::Up,
                    PointerPhase::Cancel => Phase::Cancel,
                    _ => return Ok(Answer::Pass),
                };
                self.pointer(host, phase, contact, position.x, position.y)
            }
            Input::Gesture { kind, .. } => {
                let vertical = matches!(
                    kind,
                    InputGestureKind::SwipeUp | InputGestureKind::SwipeDown
                );
                Ok(if self.tracker.gesture(vertical) {
                    Answer::Take
                } else {
                    Answer::Pass
                })
            }
            _ => Ok(Answer::Pass),
        }
    }

    fn pointer(
        &mut self,
        host: &Host<'_>,
        phase: Phase,
        contact: u32,
        x: i32,
        y: i32,
    ) -> Result<Answer, Error> {
        if phase == Phase::Down {
            self.zone = None;
            self.pending = None;
            if !self.usable(host)? {
                return Ok(Answer::Pass);
            }
            let strips = self.strips(host)?;
            if strips.left == Function::Off && strips.right == Function::Off {
                return Ok(Answer::Pass);
            }
            self.zone = Some(zone_of(host, strips)?);
        }
        let Some((zone, travel)) = self.zone else {
            return Ok(Answer::Pass);
        };
        let strips = self.strips(host)?;
        match self.tracker.pointer(&zone, phase, contact, x, y) {
            Out::Pass => Ok(Answer::Pass),
            // Lifted: the level the finger ended at is kept, once (the action
            // *step*). Cancelled, or another finger: the preview just ends.
            Out::End => {
                self.active = None;
                if phase == Phase::Up && self.pending.is_some() {
                    return Ok(Answer::Run);
                }
                self.pending = None;
                Ok(Answer::Pass)
            }
            Out::Swipe { side, rise, first } => {
                let function = match side {
                    Side::Left => strips.left,
                    Side::Right => strips.right,
                };
                if first {
                    self.begin(host, function)?;
                }
                Ok(self.follow(first, rise, travel))
            }
        }
    }

    /// A swipe began: where the level is now.
    fn begin(&mut self, host: &Host<'_>, function: Function) -> Result<(), Error> {
        let Some(setting) = function.setting() else {
            return Ok(());
        };
        let start = level_of(host, setting)?;
        self.active = Some(Active {
            function,
            start,
            last: start,
        });
        Ok(())
    }

    /// The finger is `rise` pixels above where it began: the level to show
    /// while it goes on. The swipe's first sample answers the level it began
    /// at (that takes the finger); the others only when the level changed.
    /// What the finger ends at is kept by the action *step* when it lifts
    /// (`pending`: none while it is back where it began).
    fn follow(&mut self, first: bool, rise: i32, travel: u16) -> Answer {
        let Some(active) = &mut self.active else {
            return Answer::Pass;
        };
        if active.function.setting().is_none() {
            return Answer::Pass;
        }
        let level = swipe::level_at(active.start, rise, travel);
        if level == active.last && !first {
            return Answer::Pass;
        }
        active.last = level;
        self.pending = (level != active.start).then_some(Pending {
            function: active.function,
            level,
        });
        Answer::Preview {
            function: active.function,
            level,
        }
    }

    /// The action *step*: the level the filter asked for, set, and told.
    fn step(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let Some(Pending { function, level }) = self.pending.take() else {
            return Ok(());
        };
        let Some(setting) = function.setting() else {
            return Ok(());
        };
        set_level(host, setting, level)?;
        say(host, function.notice(), level);
        Ok(())
    }

    // The action and the widget.

    /// Switches the light off, or on again at the level it had.
    fn toggle(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let now = level_of(host, BRIGHTNESS)?;
        let next = if now > 0 {
            self.on_level = Some(now);
            0
        } else {
            self.on_level.unwrap_or(SWITCH_ON).max(LEAST_ON)
        };
        set_level(host, BRIGHTNESS, next)
    }

    fn widget_stage(
        &mut self,
        host: &Host<'_>,
        payload: &[u8],
        reply: &mut Reply<'_>,
    ) -> Result<(), Error> {
        let call = WidgetCall::decode(payload)?;
        let mut out = paperpaw_ui_kit::mem::buffer(REPLY)?;
        let written = match call {
            WidgetCall::Measure { instance, .. } => {
                let widget = paperpaw_ui_kit::text::copy(
                    &self.find(instance.get()).ok_or(Error::NotFound)?.widget,
                )?;
                let cx = Cx::open(host, &widget)?;
                let height = native_widget::height(&cx)?;
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

/// The level of the light setting `key`, in percent (0 where it holds nothing).
fn level_of(host: &Host<'_>, key: &str) -> Result<i64, Error> {
    let mut scratch = [0; 32];
    match read_setting(host, key, &mut scratch)? {
        SettingValue::Int(level) => Ok(level.clamp(0, 100)),
        _ => Ok(0),
    }
}

fn set_level(host: &Host<'_>, key: &str, level: i64) -> Result<(), Error> {
    let mut scratch = [0; 128];
    CommandSender::new(host, &mut scratch).set_setting(key, SettingValue::Int(level))
}

/// The strips' pixels and the finger's way for the whole range of the light, for
/// the screen as it is now (it may have turned).
fn zone_of(host: &Host<'_>, strips: Strips) -> Result<(Zone, u16), Error> {
    let screen = snapshot::capture_snapshot(host)?;
    let (width, height, dpi) = (
        u32::from(screen.display_width()),
        u32::from(screen.display_height()),
        u32::from(screen.display_dpi()),
    );
    let clamp = |value: u32| u16::try_from(value).unwrap_or(u16::MAX);
    let percent = u32::try_from(strips.width.clamp(0, 100)).unwrap_or(0);
    let edge = clamp(width * percent / 100);
    let strip = |function: Function| if function == Function::Off { 0 } else { edge };
    let slop = (dpi * SLOP_MM / 25).max(8);
    Ok((
        Zone {
            width: clamp(width),
            left: strip(strips.left),
            right: strip(strips.right),
            slop: clamp(slop),
        },
        clamp(height * TRAVEL_PERCENT / 100),
    ))
}

/// The small notice of the level: a courtesy, so one that cannot be shown
/// changes nothing.
fn say(host: &Host<'_>, key: &str, level: i64) {
    let shown = (|| {
        let mut messages = Messages::new()?;
        let text = messages.format(
            host,
            key,
            &[Arg::Int {
                name: "level",
                value: level,
            }],
        )?;
        let mut scratch = [0; 512];
        CommandSender::new(host, &mut scratch).notify(Notify::new(&text)?.announce())
    })();
    let _ = shown;
}

/// The rectangle of a drawing as the widget's own.
fn area_of(rect: FrameRect) -> Rect {
    let clamp = paperpaw_ui_kit::geometry::clamp_u16;
    Rect::new(
        clamp(i64::from(rect.x)),
        clamp(i64::from(rect.y)),
        clamp(i64::from(rect.width)),
        clamp(i64::from(rect.height)),
    )
}
