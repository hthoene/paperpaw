//! The touch lock on the host: the input filter that takes every finger and
//! the pen while it is on, its actions (lock, toggle, say how to unlock) and its
//! status item. The way out is `native_touch`; the switch is the setting
//! `lock.touch`, read at every touch (no copy kept, so a change made on the
//! settings page counts at once). Buttons are not touched: the filter answers
//! pass for them, and the host does not call it while the screen lock asks for
//! its code.
use alloc::string::String;

use paperpaw_sdk::native::{
    Error, Host,
    callbacks::{Input, PointerPhase},
    commands::CommandSender,
    device::{Device, DeviceCapability},
    reply::Reply,
    settings::{self, SettingValue},
    stage_schemas::core_input_filter_v1 as filter,
    ui::{
        Notify,
        stages::{StatusAnswer, StatusCall},
    },
};
use paperpaw_ui_kit::{
    Messages,
    mem::{SERVICE_BUFFER, buffer},
};

use crate::declarations::{TOUCH_HINT, TOUCH_LOCK, TOUCH_STATUS, TOUCH_TOGGLE};
use crate::native_touch::{Escape, Phase, Touch, Verdict};

/// The switch, and the toggle the way out runs.
const SWITCH: &str = "lock.touch";
const TOGGLE: &str = "lock.touch-toggle";
const HINT: &str = "lock.touch-hint";
const STATUS: &str = "lock.touch-status";
/// `filter.verdict.kind`: pass (no reply says it too), take, run.
pub const TAKE: u32 = 1;
pub const RUN: u32 = 2;
/// What a verdict of run names.
pub const TOGGLE_ACTION: &str = TOGGLE;

/// Where the touch lock is in its way out.
#[derive(Debug, Default)]
pub struct TouchLock {
    escape: Escape,
    hinted: Option<u64>,
    /// A hint is due when the contact that was taken ends (a run acts on the
    /// rest of a captured contact; on its start it would not hold it).
    hint_due: bool,
}

/// Whether touch is locked now.
fn locked(host: &Host<'_>) -> bool {
    let mut scratch = [0; 32];
    matches!(
        settings::read_setting(host, SWITCH, &mut scratch),
        Ok(SettingValue::Bool(true))
    )
}

/// How to unlock: by the power button where PaperPaw is the firmware.
fn how_to_unlock(host: &Host<'_>) -> &'static str {
    let buttons = Device::open(host).is_ok_and(|device| {
        let found = (|| {
            let (mut input, mut out) = ([0; 64], buffer(SERVICE_BUFFER)?);
            let identity = device.device(host, &mut input, &mut out)?;
            Ok::<_, Error>(identity.supports(DeviceCapability::PowerOff))
        })();
        let _ = device.release(host);
        found.unwrap_or(false)
    });
    if buttons {
        "lock.touch.locked.buttons"
    } else {
        "lock.touch.locked"
    }
}

fn notify(host: &Host<'_>, key: &str) -> Result<(), Error> {
    let text: String = Messages::new()?.copy(host, key)?;
    let mut scratch = buffer(SERVICE_BUFFER)?;
    CommandSender::new(host, &mut scratch).notify(Notify::new(&text)?)
}

/// A reply of one verdict.
fn verdict(reply: &mut Reply<'_>, kind: u32, action: &str) -> Result<(), Error> {
    let mut out = [0; 256];
    let rows = [filter::Record::FilterVerdict(filter::FilterVerdict::new(
        kind,
        action,
        &[],
        "",
    ))];
    let length = filter::reply_encode(&rows, &mut out).map_err(Error::Other)?;
    reply.write(out.get(..length).ok_or(Error::NoSpace)?)
}

impl TouchLock {
    /// The input filter: takes every touch while locked. Anything else, and
    /// every touch while unlocked, is passed (no reply).
    #[inline(never)]
    pub fn filter(
        &mut self,
        host: &Host<'_>,
        payload: &[u8],
        reply: &mut Reply<'_>,
    ) -> Result<(), Error> {
        match self.answer(|| locked(host), payload)? {
            Some((kind, action)) => verdict(reply, kind, action),
            None => Ok(()),
        }
    }

    /// What the filter answers to the stage call `payload`: `None` for pass,
    /// else the verdict's kind and action. `locked` says whether the switch is
    /// on; it is asked only for a touch.
    ///
    /// # Errors
    /// A payload that is no `filter.input`, or whose input is none.
    pub fn answer(
        &mut self,
        locked: impl FnOnce() -> bool,
        payload: &[u8],
    ) -> Result<Option<(u32, &'static str)>, Error> {
        let mut rows = filter::input_decode(payload).map_err(Error::Other)?;
        let Some(Ok(filter::Record::FilterInput(row))) = rows.next() else {
            return Ok(None);
        };
        let Input::Pointer {
            now_ms,
            position,
            phase,
            contact,
        } = Input::decode(row.input)?
        else {
            return Ok(None);
        };
        if !locked() {
            *self = Self::default();
            return Ok(None);
        }
        let touch = Touch {
            id: contact,
            phase: match phase {
                PointerPhase::Down => Phase::Down,
                PointerPhase::Move => Phase::Move,
                PointerPhase::Up => Phase::Up,
                _ => Phase::Cancel,
            },
            point: position,
        };
        let ends = matches!(touch.phase, Phase::Up | Phase::Cancel);
        Ok(Some(
            match self.escape.locked(now_ms, &touch, &mut self.hinted) {
                Verdict::Unlock => {
                    self.hint_due = false;
                    (RUN, TOGGLE)
                }
                Verdict::Hint if ends => (RUN, HINT),
                Verdict::Hint => {
                    self.hint_due = true;
                    (TAKE, "")
                }
                _ if ends && self.hint_due => {
                    self.hint_due = false;
                    (RUN, HINT)
                }
                _ => (TAKE, ""),
            },
        ))
    }

    /// What one of the touch lock's actions does.
    #[inline(never)]
    pub fn action(host: &Host<'_>, token: u32) -> Result<(), Error> {
        let mut scratch = [0; 128];
        match token {
            TOUCH_HINT => notify(host, how_to_unlock(host)),
            TOUCH_LOCK | TOUCH_TOGGLE => {
                let lock = token == TOUCH_LOCK || !locked(host);
                CommandSender::new(host, &mut scratch)
                    .set_setting(SWITCH, SettingValue::Bool(lock))?;
                if lock {
                    notify(host, how_to_unlock(host))
                } else {
                    notify(host, "lock.touch.unlocked")
                }
            }
            _ => Ok(()),
        }
    }

    /// The lock in the status lines while touch is locked: its words, else
    /// nothing.
    #[inline(never)]
    pub fn status(host: &Host<'_>, payload: &[u8], reply: &mut Reply<'_>) -> Result<(), Error> {
        let call = StatusCall::decode(payload)?;
        let mut label = [0; 64];
        let text = if call.items().any(|item| item == STATUS) {
            if locked(host) {
                paperpaw_sdk::native::formatting::copy_message(host, STATUS, &mut label)?
            } else {
                ""
            }
        } else {
            return Ok(());
        };
        let answer = StatusAnswer::text(STATUS, text)?;
        let mut out = [0; 256];
        let length = call.reply(&[answer], &mut out)?;
        reply.write(out.get(..length).ok_or(Error::NoSpace)?)
    }
}

/// The token of the status stage.
pub const fn is_status(token: u32) -> bool {
    token == TOUCH_STATUS
}
