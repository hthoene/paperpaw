//! The flat Power plugin. It keeps no host handle across callbacks except the
//! UI service, and the one question it has asked.
use crate::declarations::{EVENTS_TOKEN, MENU_TOKEN, OPEN_TOKEN, STATUS_TOKEN};
use crate::model::{self, Battery, Caps, Level, Options, Policy, Step};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::native::{
    Error, Host, Plugin,
    commands::{CommandSender, PowerAction},
    device::{Device, DeviceCapability, PowerLevel},
    events::{AnswerOutcome, Event, Incoming, kinds},
    formatting::copy_message,
    registrations,
    reply::Reply,
    settings::{self, SettingValue},
    snapshot::capture_snapshot,
    ui::{
        Open, Param, Question, RequestId, Ui,
        stages::{StatusAnswer, StatusCall},
    },
};

mod image {
    include!(concat!(env!("OUT_DIR"), "/power_declarations.rs"));
}

const OPEN: &str = "power.open";
const MENU: &str = "power.menu";
const STATUS: &str = "power.status";
/// The touch lock's switch and action (the lock plugin's, which may be off or
/// absent: the menu then has no entry for it).
const TOUCH_LOCK: &str = "lock.touch";
const TOUCH_TOGGLE: &str = "lock.touch-toggle";
/// The sleep screen's slot, and the parameter that makes it say *off*.
const SLEEP_SLOT: &str = "core.sleep";
const SETTINGS_SLOT: &str = "core.settings";
const GROUP: &str = "power.group";
/// `core.ui` and `core.device` calls use buffers of the schema's capacity (4,096
/// bytes: the host refuses a smaller reply buffer).
const UI_BUFFER: usize = 4096;
/// A translated label or title fits this.
const LABEL: usize = 128;

/// The question asked and what its answers mean.
#[derive(Debug)]
struct Asked {
    request: RequestId,
    options: Options,
    caps: Caps,
}

/// Power's interface-7 entry state.
#[derive(Debug, Default)]
pub struct NativePower {
    ui: Option<Ui>,
    asked: Option<Asked>,
}

impl Plugin for NativePower {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::SETTINGS.iter().chain(image::UI).chain(image::EVENTS) {
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
        // Only the kinds this plugin handles are read, so only their decoders are linked.
        let Some(event) = decode(&incoming)? else {
            return Ok(());
        };
        match (token, event) {
            (EVENTS_TOKEN, Event::Started { .. } | Event::Resume { .. })
            | (EVENTS_TOKEN, Event::BatteryMeasured { .. } | Event::ChargeChanged { .. }) => {
                wear(host)
            }
            // Silently: the mode changes, only the status mark says so.
            (EVENTS_TOKEN, Event::SettingChanged { key, .. }) => {
                if key.is_empty() || key.starts_with("power.") {
                    wear(host)
                } else {
                    Ok(())
                }
            }
            (EVENTS_TOKEN, Event::Answered { answer, .. }) => self.answered(host, answer),
            (OPEN_TOKEN, Event::ActionRun { action: OPEN, .. }) => open_settings(host),
            (MENU_TOKEN, Event::ActionRun { action: MENU, .. }) => self.ask(host),
            (STATUS_TOKEN, Event::StageCall { stage, payload, .. }) => {
                status(host, stage, payload, reply)
            }
            _ => Ok(()),
        }
    }
}

impl NativePower {
    /// `core.ui` is opened by the first callback that needs it.
    fn ui(&mut self, host: &Host<'_>) -> Result<&Ui, Error> {
        if self.ui.is_none() {
            self.ui = Some(Ui::open(host)?);
        }
        self.ui.as_ref().ok_or(Error::State)
    }

    /// The power button held: a small menu of what the device can do here.
    #[inline(never)]
    fn ask(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let caps = device_caps(host)?;
        let locked = touch_locked(host);
        let options = model::menu_options(caps, locked);
        let mut title_buffer = [0; LABEL];
        let title = copy_message(host, "power.menu.title", &mut title_buffer)?;
        let mut choices = String::new();
        let mut label_buffer = [0; LABEL];
        for (index, item) in options.iter().enumerate() {
            let label = copy_message(host, item.label(), &mut label_buffer)?;
            choices
                .try_reserve(label.len() + 1)
                .map_err(|_| Error::NoMemory)?;
            if index > 0 {
                choices.push('\n');
            }
            choices.push_str(label);
        }
        let question = Question::choice(title, "", &choices)?;
        let mut input = buffer(UI_BUFFER)?;
        let mut out = buffer(UI_BUFFER)?;
        let request = self.ui(host)?.ask(host, question, &mut input, &mut out)?;
        self.asked = Some(Asked {
            request,
            options,
            caps,
        });
        Ok(())
    }

    /// What the option chosen in the power menu does. Nothing for a cancelled
    /// menu, an answer to another request or a typing update.
    #[inline(never)]
    fn answered(
        &mut self,
        host: &Host<'_>,
        answer: paperpaw_sdk::native::events::Answered,
    ) -> Result<(), Error> {
        let Some(asked) = &self.asked else {
            return Ok(());
        };
        if asked.request != answer.request() || answer.outcome() == AnswerOutcome::Typing {
            return Ok(());
        }
        let Some(asked) = self.asked.take() else {
            return Ok(());
        };
        if answer.outcome() != AnswerOutcome::Answered {
            return Ok(());
        }
        let Some(item) = usize::try_from(answer.choice())
            .ok()
            .and_then(|index| asked.options.get(index))
        else {
            return Ok(());
        };
        run(host, model::plan(item, asked.caps))
    }
}

/// The event of one of the kinds this plugin handles, if it is one. One kind
/// is tried after the other: an array of the eight decoded would hold eight
/// events on the stack at once (over 2 KiB, the X4's band).
#[inline(never)]
fn decode<'a>(incoming: &Incoming<'a>) -> Result<Option<Event<'a>>, Error> {
    macro_rules! first {
        ($($kind:ident),+) => {
            $(
                if let Some(event) = incoming.decode_as::<kinds::$kind>()? {
                    return Ok(Some(event));
                }
            )+
        };
    }
    first!(
        Started,
        Resume,
        BatteryMeasured,
        ChargeChanged,
        SettingChanged,
        Answered,
        ActionRun,
        StageCall
    );
    Ok(None)
}

/// Opens the power settings: what a tap on the mode's status item runs.
#[inline(never)]
fn open_settings(host: &Host<'_>) -> Result<(), Error> {
    let ui = Ui::open(host)?;
    let result = (|| {
        let mut input = buffer(UI_BUFFER)?;
        let mut out = buffer(UI_BUFFER)?;
        let handover = ui.handover(
            host,
            &[Param::new(paperpaw_sdk::builtin::params::ID, GROUP)?],
            &mut input,
            &mut out,
        )?;
        let mut scratch = [0; 256];
        CommandSender::new(host, &mut scratch).open(Open::slot(SETTINGS_SLOT)?.handover(handover))
    })();
    let released = ui.release(host);
    result.and(released)
}

/// Does what a step of the power menu says.
#[inline(never)]
fn run(host: &Host<'_>, step: Step) -> Result<(), Error> {
    let mut scratch = [0; 512];
    match step {
        Step::SleepWithScreen => show_then(host, PowerAction::Sleep, false, &mut scratch),
        Step::OffWithScreen => show_then(host, PowerAction::PowerOff, true, &mut scratch),
        Step::Sleep => CommandSender::new(host, &mut scratch).sleep(),
        Step::Off => CommandSender::new(host, &mut scratch).power_off(),
        Step::Restart => CommandSender::new(host, &mut scratch).restart(),
        Step::ToggleTouchLock => CommandSender::new(host, &mut scratch).run_action(TOUCH_TOGGLE),
    }
}

/// The sleep screen first, then the power command in the same batch: the host
/// draws the frame and acts when the panel is idle, so e-ink keeps a screen
/// that shows what the device really is. Without the sleep screen (its plugin
/// is off) the command acts alone.
#[inline(never)]
fn show_then(
    host: &Host<'_>,
    action: PowerAction,
    off: bool,
    scratch: &mut [u8],
) -> Result<(), Error> {
    let shown = (|| {
        if off {
            // `off`: the sleep screen's parameter (plugin `sleep`).
            let ui = Ui::open(host)?;
            let result = (|| {
                let mut input = buffer(UI_BUFFER)?;
                let mut out = buffer(UI_BUFFER)?;
                let handover =
                    ui.handover(host, &[Param::new("off", "1")?], &mut input, &mut out)?;
                CommandSender::new(host, scratch)
                    .show_then(Open::slot(SLEEP_SLOT)?.handover(handover), action)
            })();
            let released = ui.release(host);
            result.and(released)
        } else {
            CommandSender::new(host, scratch).show_then(Open::slot(SLEEP_SLOT)?, action)
        }
    })();
    match shown {
        Err(Error::NotFound | Error::Unsupported) => {
            let mut sender = CommandSender::new(host, scratch);
            match action {
                PowerAction::Sleep => sender.sleep(),
                PowerAction::PowerOff => sender.power_off(),
                _ => sender.restart(),
            }
        }
        other => other,
    }
}

/// The mode to wear now: worn if another is worn.
#[inline(never)]
fn wear(host: &Host<'_>) -> Result<(), Error> {
    let mut scratch = [0; 64];
    let mode = match settings::read_setting(host, "power.mode", &mut scratch)? {
        SettingValue::Text(mode) => alloc_text(mode)?,
        _ => String::new(),
    };
    let saving_below = number(host, "power.saving-below", 20);
    let offline_below = number(host, "power.offline-below", 10);
    let normal_while_charging = flag(host, "power.normal-while-charging", true);
    let snapshot = capture_snapshot(host)?;
    let battery = snapshot.battery_percent().map(|percent| Battery {
        percent,
        charging: snapshot.is_charging(),
        saver: snapshot.is_power_saving(),
    });
    let device = Device::open(host)?;
    let worn = (|| {
        let (mut input, mut out) = ([0; 64], buffer(UI_BUFFER)?);
        let power = device.power(host, 0, &mut input, &mut out)?;
        let level = match power.level() {
            PowerLevel::Saving => Level::Saving,
            PowerLevel::Offline => Level::Offline,
            _ => Level::Normal,
        };
        Ok((level, alloc_text(power.mode())?, power.automatic()))
    })();
    let released = device.release(host);
    let (level, worn_mode, worn_automatic) = worn?;
    released?;
    let policy = Policy {
        mode: &mode,
        saving_below,
        offline_below,
        normal_while_charging,
    };
    let choice = model::choose(&policy, battery, level);
    if !model::must_wear(choice, &worn_mode, worn_automatic) {
        return Ok(());
    }
    let mut commands = [0; 256];
    CommandSender::new(host, &mut commands).wear(choice.level.preset(), choice.automatic)
}

/// What the device can do, as far as the power menu cares.
fn device_caps(host: &Host<'_>) -> Result<Caps, Error> {
    let device = Device::open(host)?;
    let caps = (|| {
        let (mut input, mut out) = ([0; 64], buffer(UI_BUFFER)?);
        let identity = device.device(host, &mut input, &mut out)?;
        Ok(Caps {
            sleep: identity.supports(DeviceCapability::Sleep),
            power_off: identity.supports(DeviceCapability::PowerOff),
            touch: identity.supports(DeviceCapability::Touch),
        })
    })();
    let released = device.release(host);
    let caps = caps?;
    released?;
    Ok(caps)
}

/// Whether touch is locked, `None` where the lock plugin has no such setting.
fn touch_locked(host: &Host<'_>) -> Option<bool> {
    let mut scratch = [0; 32];
    match settings::read_setting(host, TOUCH_LOCK, &mut scratch) {
        Ok(SettingValue::Bool(locked)) => Some(locked),
        _ => None,
    }
}

fn number(host: &Host<'_>, key: &str, default: i64) -> i64 {
    let mut scratch = [0; 32];
    match settings::read_setting(host, key, &mut scratch) {
        Ok(SettingValue::Int(value)) => value,
        _ => default,
    }
}

fn flag(host: &Host<'_>, key: &str, default: bool) -> bool {
    let mut scratch = [0; 32];
    match settings::read_setting(host, key, &mut scratch) {
        Ok(SettingValue::Bool(value)) => value,
        _ => default,
    }
}

/// The status item: nothing in Normal, so the lines stay calm; the mode's
/// name while saving or offline.
#[inline(never)]
fn status(
    host: &Host<'_>,
    stage: &str,
    payload: &[u8],
    reply: &mut Reply<'_>,
) -> Result<(), Error> {
    if stage != "core.status" {
        return Err(Error::Unsupported);
    }
    let call = StatusCall::decode(payload)?;
    let key = match worn_level(host)? {
        Level::Normal => None,
        Level::Saving => Some("power.mode.saving"),
        Level::Offline => Some("power.mode.offline"),
    };
    let mut label = [0; LABEL];
    let text = match key {
        Some(key) => copy_message(host, key, &mut label)?,
        None => "",
    };
    let mut answers = [None; 1];
    for item in call.items() {
        if item == STATUS && answers[0].is_none() {
            answers[0] = Some(StatusAnswer::text(item, text)?);
        }
    }
    let [Some(answer)] = answers else {
        return Ok(());
    };
    let mut out = [0; 512];
    let length = call.reply(&[answer], &mut out)?;
    reply.write(&out[..length])
}

/// The level worn at present.
fn worn_level(host: &Host<'_>) -> Result<Level, Error> {
    let device = Device::open(host)?;
    let level = (|| {
        let (mut input, mut out) = ([0; 64], buffer(UI_BUFFER)?);
        Ok(match device.power(host, 0, &mut input, &mut out)?.level() {
            PowerLevel::Saving => Level::Saving,
            PowerLevel::Offline => Level::Offline,
            _ => Level::Normal,
        })
    })();
    let released = device.release(host);
    let level = level?;
    released?;
    Ok(level)
}

fn alloc_text(text: &str) -> Result<String, Error> {
    let mut owned = String::new();
    owned
        .try_reserve_exact(text.len())
        .map_err(|_| Error::NoMemory)?;
    owned.push_str(text);
    Ok(owned)
}

fn buffer(len: usize) -> Result<Vec<u8>, Error> {
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(len).map_err(|_| Error::NoMemory)?;
    bytes.resize(len, 0);
    Ok(bytes)
}
