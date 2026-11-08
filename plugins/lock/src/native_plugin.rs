//! The interface-7 screen lock: a PIN, a pattern or a password asked for when
//! PaperPaw starts and when the device wakes up, before anything else can be
//! used. Off by default. The rules are `native_model`, the steps of the screen
//! `native_flow` and `native_input`, what it draws `native_screen`; this file
//! carries the effects out through the host and routes its events and inputs.
use paperpaw_sdk::{
    Handled,
    native::{
        Error, Host, Plugin,
        callbacks::{DrawFrame, DrawPurpose, Input, InputGestureKind},
        commands::{CommandSender, RedrawMode},
        events::{AnswerOutcome, Answered, Event, Incoming, kinds},
        fonts::{Font, FontRole},
        registrations,
        reply::Reply,
        settings::SettingValue,
        snapshot,
        ui::{HandoverId, Instance, Notify, Open, Param, Question, RequestId, ScreenPolicy, Ui},
    },
};
use paperpaw_ui_kit::{
    Footer, Item, Messages, Screens, Style,
    mem::{SERVICE_BUFFER, buffer, wipe},
    paint, read_event,
};

use crate::declarations::{NOW, RESUME, SCREEN, STARTED};
use crate::native_clock;
use crate::native_flow::{Context, Effect, Effects, Flow, Lockstate, Step};
use crate::native_gate::{self as gate, Device, Door};
use crate::native_input::Act;
use crate::native_model::{self as model, Code};
use crate::native_screen::{Clock, View, prepare};
use crate::native_store as store;

mod image {
    include!(concat!(env!("OUT_DIR"), "/lock_declarations.rs"));
}

/// The parameter of the lock screen: what it is for.
const MODE: &str = "mode";
const UNLOCK: &str = "unlock";
/// The longest password taken (the code is hashed whole, never cut).
const PASSWORD_MAX: usize = 4096;

/// What the screen needs of the device and the code kept: read afresh at each
/// input, so a code removed elsewhere is seen at once.
struct Read {
    stored: Option<Code>,
    active: Option<&'static str>,
    touch: bool,
}

impl Read {
    fn new(host: &Host<'_>) -> Result<Self, Error> {
        let stored = store::stored(host)?;
        // The code kept decides, not the kind setting: that can be set to
        // `off` while the code stays (`gate::active`).
        let active = gate::active(stored.as_deref());
        let touch = snapshot::capture_snapshot(host)?.has_touch();
        Ok(Self {
            stored,
            active,
            touch,
        })
    }

    fn context(&self, now_ms: u64) -> Context<'_> {
        Context {
            stored: self.stored.as_deref(),
            active: self.active,
            now_ms,
            touch: self.touch,
        }
    }
}

/// The interface-7 screen lock.
#[derive(Debug, Default)]
pub struct NativeLock {
    lock: Lockstate,
    screens: Screens<View>,
    door: Door,
}

impl Plugin for NativeLock {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::SETTINGS
            .iter()
            .chain(image::SCREENS)
            .chain(image::NAVIGATION)
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
        _: &mut Reply<'_>,
    ) -> Result<(), Error> {
        let Some(event) = read_event!(incoming; kinds::ActionRun, kinds::Started, kinds::Resume, kinds::ScreenOpened, kinds::ScreenShown, kinds::ScreenClosed, kinds::Answered)
        else {
            return Ok(());
        };
        match event {
            Event::ActionRun { .. } if token == NOW => self.lock_now(host),
            Event::Started { now_ms } if token == STARTED => self.started(host, now_ms),
            Event::Resume { .. } if token == RESUME => {
                self.sync_door();
                gate::woke(&HostDevice(host), &mut self.lock, &mut self.door)
            }
            Event::ScreenOpened {
                instance, handover, ..
            } if token == SCREEN => self.opened(host, instance, handover),
            Event::ScreenShown {
                instance: Some(instance),
                ..
            } if token == SCREEN => self.shown(host, instance),
            Event::ScreenClosed { instance, .. } if token == SCREEN => self.closed(host, instance),
            Event::Answered { answer, .. } => self.answered(host, answer),
            _ => Ok(()),
        }
    }

    fn draw(&mut self, host: &Host<'_>, token: u32, mut frame: DrawFrame<'_>) -> Result<(), Error> {
        if !matches!(frame.purpose, DrawPurpose::Screen) {
            return Ok(());
        }
        let now_ms = snapshot::capture_snapshot(host)?.instant().millis();
        let wait_left = if self.lock.attempts.waiting(now_ms) {
            self.lock.attempts.seconds_left(now_ms)
        } else {
            0
        };
        let view = self.screens.top(token).ok_or(Error::State)?;
        let mut messages = Messages::new()?;
        if view.flow.step() == Step::Choose {
            return draw_kinds(host, &mut frame, view, &mut messages);
        }
        let font = Font::acquire_role(host, FontRole::Ui)?;
        let drawn = (|| {
            let display = snapshot::capture_snapshot(host)?;
            let style = Style::read(host, &display, &font)?;
            let clock = if view.flow.step() == Step::Unlock {
                native_clock::now(host, &mut messages).unwrap_or_default()
            } else {
                Clock::default()
            };
            let (ops, drawn) = prepare(
                host,
                &font,
                &style,
                &display,
                &mut messages,
                view,
                wait_left,
                &clock,
            )?;
            paint::replay(&mut frame, &font, ops.iter())?;
            Ok(drawn)
        })();
        let released = font.release();
        let drawn = drawn?;
        released?;
        view.drawn = Some(drawn);
        Ok(())
    }

    fn input(&mut self, host: &Host<'_>, token: u32, input: Input<'_>) -> Result<Handled, Error> {
        // Locked, an input that fails is taken all the same: nothing gets past
        // the lock because a read or an allocation was refused.
        match self.take(host, token, &input) {
            Err(_) if self.lock.locked => Ok(Handled::Yes),
            Ok(Handled::No) if self.lock.locked && self.screens.top_instance(token).is_none() => {
                Ok(Handled::Yes)
            }
            other => other,
        }
    }
}

impl NativeLock {
    fn take(&mut self, host: &Host<'_>, token: u32, input: &Input<'_>) -> Result<Handled, Error> {
        let Some(instance) = self.screens.top_instance(token) else {
            return Ok(Handled::No);
        };
        let read = Read::new(host)?;
        let ctx = read.context(now_of(input));
        let mut fx = Effects::default();
        let handled = {
            let Some(view) = self.screens.get(instance) else {
                return Ok(Handled::No);
            };
            // A wait that is over: the tries are forgotten before anything.
            view.flow.wait_over(&mut self.lock, &ctx, &mut fx);
            route(host, view, &mut self.lock, &ctx, &mut fx, input)?
        };
        self.carry_out(host, instance, &mut fx)?;
        Ok(handled)
    }
}

/// The time of an input.
fn now_of(input: &Input<'_>) -> u64 {
    match *input {
        Input::Key { now_ms, .. }
        | Input::Pointer { now_ms, .. }
        | Input::Gesture { now_ms, .. }
        | Input::Action { now_ms, .. }
        | Input::Widget { now_ms, .. } => now_ms,
        _ => 0,
    }
}

/// What an input does to the screen's steps, and whether the screen took it.
fn route(
    host: &Host<'_>,
    view: &mut View,
    lock: &mut Lockstate,
    ctx: &Context<'_>,
    fx: &mut Effects,
    input: &Input<'_>,
) -> Result<Handled, Error> {
    let act = match *input {
        Input::Action { action, .. } => view.flow.act(action, lock, ctx, fx)?,
        Input::Gesture {
            position,
            kind: InputGestureKind::Tap,
            ..
        } => {
            let Some(drawn) = view.drawn.as_ref() else {
                return Ok(Handled::Yes);
            };
            view.flow
                .tap(position, (&drawn.layout, drawn.back), lock, ctx, fx)?
        }
        // Locked, no other gesture gets past; setting up, nothing is locked.
        Input::Gesture { .. } => {
            return Ok(
                if view.flow.step() == Step::Unlock || Flow::waiting(lock, ctx.now_ms) {
                    Handled::Yes
                } else {
                    Handled::No
                },
            );
        }
        Input::Pointer {
            position,
            phase,
            contact,
            ..
        } => {
            let Some(drawn) = view.drawn.as_ref() else {
                return Ok(Handled::No);
            };
            let took =
                view.flow
                    .pointer((phase, contact, position), &drawn.layout, lock, ctx, fx)?;
            return Ok(if took { Handled::Yes } else { Handled::No });
        }
        _ => return Ok(Handled::No),
    };
    Ok(match act {
        Act::Taken => Handled::Yes,
        Act::Passed => Handled::No,
        Act::List => {
            let step = view.list.input(host, input)?;
            if let paperpaw_ui_kit::ItemStep::Chosen(index) = step {
                let touch = ctx.touch;
                if let Some(kind) = Flow::kinds(touch).nth(index) {
                    view.flow.choose_kind(kind, fx);
                }
            }
            paperpaw_ui_kit::ItemList::handled(step)
        }
    })
}

/// The kinds as a list, the one in use marked.
fn draw_kinds(
    host: &Host<'_>,
    frame: &mut DrawFrame<'_>,
    view: &mut View,
    messages: &mut Messages,
) -> Result<(), Error> {
    let read = Read::new(host)?;
    let current = read.active.unwrap_or("off");
    let mut items = alloc::vec::Vec::new();
    for kind in Flow::kinds(read.touch) {
        let key = paperpaw_ui_kit::text::concat(&[model::KIND_PREFIX, kind])?;
        let name = messages.copy(host, &key)?;
        let mark = if kind == current { "✓" } else { "" };
        paperpaw_ui_kit::mem::push(&mut items, Item::row(&name, mark)?)?;
    }
    view.list.replace(items);
    let title = messages.copy(host, view.flow.title_key())?;
    view.list
        .draw_with(host, frame, &title, view.flow.shown, Footer::None)
}

/// The host as the gate's device.
struct HostDevice<'a, 'h>(&'a Host<'h>);

impl Device for HostDevice<'_, '_> {
    fn stored(&self) -> Result<Option<Code>, Error> {
        store::stored(self.0)
    }

    /// The kind goes with the code kept: changed without it (a plain list of
    /// choices, *Reset all settings*), it goes back. Interface 6 did this when
    /// the setting changed; the host does not take that subscription yet, so
    /// it is done at the start and when the device wakes, after the lock is
    /// asked for (the command takes effect when the callback ends).
    fn repair_kind(&self, stored: Option<&str>) -> Result<(), Error> {
        let host = self.0;
        let Some(kept) = model::kept_kind(stored) else {
            return Ok(());
        };
        let value = paperpaw_ui_kit::text::concat(&[model::KIND_PREFIX, kept])?;
        if store::kind_setting(host)? == value {
            return Ok(());
        }
        let mut scratch = [0; 256];
        CommandSender::new(host, &mut scratch)
            .set_setting(store::KIND, SettingValue::Text(&value))?;
        notify(host, "lock.on-its-page")
    }

    fn tries(&self) -> Result<u8, Error> {
        store::tries(self.0)
    }

    fn on_wake(&self) -> Result<bool, Error> {
        store::on_wake(self.0)
    }

    fn open_unlock(&self) -> Result<(), Error> {
        let host = self.0;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let ui = Ui::open(host)?;
        let opened = (|| {
            let params = [Param::new(MODE, UNLOCK)?];
            let handover = ui.handover(host, &params, &mut input, &mut out)?;
            let mut scratch = [0; 256];
            CommandSender::new(host, &mut scratch)
                .open(Open::screen("lock.screen")?.handover(handover))
        })();
        // Releasing the handle is of no matter once the screen is staged, and an
        // error returned now would drop the staged opening.
        let _ = ui.release(host);
        opened
    }
}

/// A notice with the text of `key`.
fn notify(host: &Host<'_>, key: &str) -> Result<(), Error> {
    let text = Messages::new()?.copy(host, key)?;
    let mut scratch = buffer(SERVICE_BUFFER)?;
    CommandSender::new(host, &mut scratch).notify(Notify::new(&text)?)
}

impl NativeLock {
    /// Whether an unlock screen is open: what the gate needs to know.
    fn sync_door(&mut self) {
        self.door.shown = self
            .screens
            .all()
            .any(|view| view.flow.step() == Step::Unlock);
    }

    /// PaperPaw started: the device locks first, then the rest.
    fn started(&mut self, host: &Host<'_>, now_ms: u64) -> Result<(), Error> {
        self.sync_door();
        gate::started(&HostDevice(host), &mut self.lock, &mut self.door, now_ms)
    }

    /// Locks, if a lock is set and the device is not locked yet; locked
    /// without a screen to unlock, the screen is opened again.
    fn lock_now(&mut self, host: &Host<'_>) -> Result<(), Error> {
        self.sync_door();
        gate::lock_now(&HostDevice(host), &mut self.lock, &mut self.door)
    }

    /// The guard policy of an unlock screen: nothing above it but its own
    /// (the keyboard of a password); others wait below until it is unlocked.
    fn guard(&mut self, host: &Host<'_>, instance: Instance) -> Result<(), Error> {
        let mut scratch = [0; 64];
        CommandSender::new(host, &mut scratch).screen_policy(
            Some(instance),
            ScreenPolicy::new().guards().secret().holds_edges(),
        )?;
        if let Some(view) = self.screens.get(instance) {
            view.guarded = true;
        }
        Ok(())
    }

    /// Whether the handover says the screen is to unlock.
    fn unlocks(host: &Host<'_>, handover: Option<HandoverId>) -> Result<bool, Error> {
        let Some(handover) = handover else {
            return Ok(false);
        };
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let ui = Ui::open(host)?;
        let found = (|| {
            let mut unlock = false;
            for param in ui.params(host, handover, &mut input, &mut out)? {
                let param = param?;
                unlock |= param.key() == MODE && param.value() == UNLOCK;
            }
            Ok(unlock)
        })();
        let released = ui.release(host);
        let unlock = found?;
        released?;
        Ok(unlock)
    }

    fn opened(
        &mut self,
        host: &Host<'_>,
        instance: Instance,
        handover: Option<HandoverId>,
    ) -> Result<(), Error> {
        // A handover that cannot be read on a locked device is for unlocking.
        let unlock = Self::unlocks(host, handover).unwrap_or(self.lock.locked);
        let read = Read::new(host)?;
        let flow = Flow::new(unlock, read.active, !read.touch);
        self.screens.open(instance, View::new(flow))?;
        if unlock {
            self.door.opening = false;
            // Refused here, it is set again when the screen shows.
            self.guard(host, instance)?;
        }
        Ok(())
    }

    fn shown(&mut self, host: &Host<'_>, instance: Instance) -> Result<(), Error> {
        self.screens.shown(SCREEN, instance)?;
        let unguarded = self
            .screens
            .get(instance)
            .is_some_and(|view| view.flow.step() == Step::Unlock && !view.guarded);
        if unguarded {
            self.guard(host, instance)?;
        }
        let read = Read::new(host)?;
        let now_ms = snapshot::capture_snapshot(host)?.instant().millis();
        let ctx = read.context(now_ms);
        let mut fx = Effects::default();
        {
            let Some(view) = self.screens.get(instance) else {
                return Ok(());
            };
            // Unlocked while something was above it (its keyboard): it goes.
            if view.flow.step() == Step::Unlock && !self.lock.locked {
                fx.push(Effect::Back);
            } else if !view.flow.released(&mut self.lock, &ctx, &mut fx) {
                // A wait begun before this screen was made ends on time here
                // too: the next input sees it over (`WakeAt` waits for the
                // work service, see PORTING-7.md).
                view.flow.shown_screen(&mut fx);
            }
        }
        self.carry_out(host, instance, &mut fx)
    }

    /// Something closed the lock screen while locked: it opens again.
    fn closed(&mut self, host: &Host<'_>, instance: Instance) -> Result<(), Error> {
        self.screens.close(instance);
        self.door.opening = false;
        // Whatever closed, a locked device with no unlock screen gets one (also
        // where the closed screen never got its view).
        if self.lock.locked {
            // Refused, the device stays locked and the next occasion (waking,
            // *Lock now*) opens it again.
            self.sync_door();
            gate::reopen(&HostDevice(host), &self.lock, &mut self.door)?;
        }
        Ok(())
    }

    /// The password was typed on the keyboard.
    fn answered(&mut self, host: &Host<'_>, answer: Answered) -> Result<(), Error> {
        if answer.outcome() == AnswerOutcome::Typing {
            return Ok(());
        }
        let request = answer.request();
        let Some((instance, view)) = self.screens_asking(request) else {
            return Ok(());
        };
        view.asked = None;
        if answer.outcome() != AnswerOutcome::Answered {
            return Ok(());
        }
        let password = read_password(host, request)?;
        let read = Read::new(host)?;
        let now_ms = snapshot::capture_snapshot(host)?.instant().millis();
        let ctx = read.context(now_ms);
        let mut fx = Effects::default();
        {
            let Some(view) = self.screens.get(instance) else {
                return Ok(());
            };
            view.flow.typed(password, &mut self.lock, &ctx, &mut fx)?;
        }
        self.carry_out(host, instance, &mut fx)
    }

    /// The screen that asked `request`.
    fn screens_asking(&mut self, request: RequestId) -> Option<(Instance, &mut View)> {
        let instance = self.screens.top_instance(SCREEN)?;
        let view = self.screens.get(instance)?;
        (view.asked == Some(request)).then_some((instance, view))
    }

    /// Asks for the password on the keyboard.
    fn ask_password(&mut self, host: &Host<'_>, instance: Instance) -> Result<(), Error> {
        let Some(view) = self.screens.get(instance) else {
            return Ok(());
        };
        let title = Messages::new()?.copy(host, view.flow.title_key())?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let ui = Ui::open(host)?;
        let asked = ui.ask(
            host,
            Question::text(&title, "")?.secret()?,
            &mut input,
            &mut out,
        );
        let released = ui.release(host);
        view.asked = Some(asked?);
        released
    }

    /// Carries out what an input meant, in order. A refused effect ends the
    /// list: the effects after it are dropped, with the codes they hold
    /// wiped.
    fn carry_out(
        &mut self,
        host: &Host<'_>,
        instance: Instance,
        fx: &mut Effects,
    ) -> Result<(), Error> {
        while let Some(effect) = fx.next_effect() {
            match effect {
                Effect::Redraw => {
                    let mut scratch = [0; 64];
                    CommandSender::new(host, &mut scratch).queue_redraw(RedrawMode::Fast)?;
                }
                Effect::Back | Effect::Unlocked => {
                    let mut scratch = [0; 64];
                    CommandSender::new(host, &mut scratch).back()?;
                }
                Effect::AskPassword => self.ask_password(host, instance)?,
                Effect::Notify(key) => notify(host, key)?,
                Effect::Forget => store::forget(host)?,
                Effect::SetKind(value) => {
                    let mut scratch = [0; 256];
                    CommandSender::new(host, &mut scratch)
                        .set_setting(store::KIND, SettingValue::Text(value))?;
                }
                Effect::Keep(kind, code) | Effect::Renew(kind, code) => {
                    store::keep(host, kind, &code)?;
                }
                // The end of a wait is seen at the next input (and drawn from
                // the clock); an exact wake needs the work service.
                Effect::WakeAt(_) => {}
                // Not kept, the count still holds until PaperPaw starts anew.
                Effect::KeepTries(tries) => {
                    let _ = store::keep_tries(host, tries);
                }
            }
        }
        Ok(())
    }
}

/// The password the keyboard answered, whole.
fn read_password(host: &Host<'_>, request: RequestId) -> Result<Code, Error> {
    let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
    let ui = Ui::open(host)?;
    let read = (|| {
        let mut text: Option<Code> = None;
        let mut offset = 0u32;
        loop {
            let part = ui.text_read(host, request.text(), offset, &mut input, &mut out)?;
            let total = usize::try_from(part.total()).map_err(|_| Error::NoSpace)?;
            if total > PASSWORD_MAX {
                return Err(Error::NoSpace);
            }
            let piece = part.data();
            let code = match &mut text {
                Some(code) => code,
                none => none.insert(Code::reserved(total)?),
            };
            code.push_str(piece)?;
            offset = offset
                .checked_add(u32::try_from(piece.len()).map_err(|_| Error::NoSpace)?)
                .ok_or(Error::NoSpace)?;
            if usize::try_from(offset).map_or(true, |read| read >= total) {
                return Ok(text.take().unwrap_or_default());
            }
        }
    })();
    wipe(&mut input);
    wipe(&mut out);
    let released = ui.release(host);
    let password = read?;
    released?;
    Ok(password)
}
