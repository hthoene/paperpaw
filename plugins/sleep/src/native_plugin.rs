//! The interface-7 sleep screen: one overlay screen (`sleep.screen`, the slot
//! `core.sleep`) over `core.ui`, with the widgets it composes
//! (`native_screen`, `native_stack`) and what it decides (`native_sleep`).
//!
//! The device waking closes it (`event.resume`); a screen still there when the
//! device is awake goes at the next press, so that it never keeps an awake
//! device to itself. While asleep every press but *sleep* and *home* is taken.
use crate::{
    declarations::{EVENTS, SCREEN, SETTING_CHANGED},
    native_screen::{Sleeping, read_text},
    native_sleep,
};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Handled,
    native::{
        Error, Host, Plugin,
        callbacks::{DrawFrame, Input},
        commands::CommandSender,
        events::{Event, Incoming, kinds},
        registrations,
        registry::{PageCursor, Registry},
        reply::Reply,
        ui::{HandoverId, Instance, Ui},
    },
};
use paperpaw_ui_kit::{
    Screens,
    mem::{SERVICE_BUFFER, buffer, push},
    read_event,
    text::copy,
};

/// Pages of the offers for the slot read at most (a page holds dozens).
const MOST_PAGES: usize = 8;
/// The room a `command.bind_slot` takes: two identifiers of 128 bytes at most.
const BIND_SCRATCH: usize = 512;

mod image {
    include!(concat!(env!("OUT_DIR"), "/sleep_declarations.rs"));
}

/// The interface-7 sleep plugin.
#[derive(Debug, Default)]
pub struct NativeSleep {
    ui: Option<Ui>,
    screens: Screens<Sleeping>,
    /// Opened for the slot offers the first time a foreign mode needs them,
    /// then kept for the plugin's lifetime (as `ui`).
    registry: Option<Registry>,
}

impl Plugin for NativeSleep {
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
        let Some(event) = read_event!(
            incoming;
            kinds::Resume,
            kinds::SettingChanged,
            kinds::ScreenOpened,
            kinds::ScreenShown,
            kinds::ScreenClosed
        ) else {
            return Ok(());
        };
        match event {
            Event::ScreenOpened {
                instance, handover, ..
            } if token == SCREEN => self.opened(host, instance, handover),
            Event::ScreenShown {
                instance: Some(instance),
                ..
            } if token == SCREEN => self.shown(host, instance),
            Event::ScreenClosed { instance, .. } if token == SCREEN => {
                self.screens.close(instance);
                Ok(())
            }
            Event::Resume { .. } if token == EVENTS => self.woke(host),
            // Only an actual change of `sleep.show` binds or unbinds: at start,
            // and for the empty key that asks every setting to be read again,
            // the slot's binding is the reader's and stays.
            Event::SettingChanged {
                key: "sleep.show", ..
            } if token == SETTING_CHANGED => self.bind_mode(host),
            _ => Ok(()),
        }
    }

    fn draw(&mut self, host: &Host<'_>, token: u32, mut frame: DrawFrame<'_>) -> Result<(), Error> {
        if token != SCREEN {
            return Err(Error::Invalid);
        }
        if self.ui.is_none() {
            self.ui = Some(Ui::open(host)?);
        }
        let ui = self.ui.as_ref().ok_or(Error::State)?;
        self.screens
            .top(token)
            .ok_or(Error::State)?
            .draw(host, ui, &mut frame)
    }

    fn input(&mut self, host: &Host<'_>, token: u32, input: Input<'_>) -> Result<Handled, Error> {
        let Some(view) = self.screens.top(token) else {
            return Ok(Handled::No);
        };
        // Awake and still here: gone at the next press.
        if view.woke {
            close(host)?;
            return Ok(Handled::Yes);
        }
        // Nothing to do while asleep but sleep again or go home.
        Ok(match input {
            Input::Action {
                action: "core.sleep" | "core.home",
                ..
            } => Handled::No,
            _ => Handled::Yes,
        })
    }
}

/// Closes the screen on top; one that is not the plugin's any more is gone.
fn close(host: &Host<'_>) -> Result<(), Error> {
    let mut scratch = [0; 64];
    match CommandSender::new(host, &mut scratch).close_screen() {
        Ok(()) | Err(Error::State) => Ok(()),
        Err(error) => Err(error),
    }
}

impl NativeSleep {
    /// Binds the slot `core.sleep` for the stored `sleep.show`: to the screen
    /// of another plugin's mode while that plugin offers it, else back to the
    /// default (the plugin's own screen). A refusal (a screen not offered, no
    /// consent, a host without the command) binds nothing and is not the
    /// plugin's failure; only a memory shortage is reported.
    fn bind_mode(&mut self, host: &Host<'_>) -> Result<(), Error> {
        match self.try_bind_mode(host) {
            Err(Error::NoMemory) => Err(Error::NoMemory),
            Ok(()) | Err(_) => Ok(()),
        }
    }

    fn try_bind_mode(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let value = read_text(host, "sleep.show")?;
        let (mut offered, mut current, mut choices) = (Vec::new(), None, Vec::new());
        if native_sleep::is_foreign(&value) {
            offered = self.candidates(host)?;
        } else {
            current = self.bound(host)?;
            choices = self.foreign_choices(host)?;
        }
        let screen = match native_sleep::plan(&value, &offered, current.as_deref(), &choices) {
            native_sleep::Bind::Screen(screen) => Some(screen),
            native_sleep::Bind::Default => None,
            _ => return Ok(()),
        };
        let mut scratch = [0; BIND_SCRATCH];
        match CommandSender::new(host, &mut scratch).bind_slot(native_sleep::SLOT, screen) {
            Ok(()) | Err(Error::Invalid | Error::NotFound | Error::Denied | Error::Unsupported) => {
                Ok(())
            }
            Err(error) => Err(error),
        }
    }

    /// The screen `core.sleep` is bound to now, by the reader or by default.
    fn bound(&mut self, host: &Host<'_>) -> Result<Option<String>, Error> {
        let registry = self.registry(host)?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let mut cursor = PageCursor::start();
        for _ in 0..MOST_PAGES {
            let page = registry.slots(host, cursor, &mut input, &mut out)?;
            for row in page.rows() {
                if row.id() == native_sleep::SLOT {
                    return Ok(Some(copy(row.screen())?));
                }
            }
            match page.end().next_cursor() {
                Some(next) => cursor = next,
                None => break,
            }
        }
        Ok(None)
    }

    /// The choices of `sleep.show` that are another plugin's modes.
    fn foreign_choices(&mut self, host: &Host<'_>) -> Result<Vec<String>, Error> {
        let registry = self.registry(host)?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let mut found = Vec::new();
        let mut cursor = PageCursor::start();
        for _ in 0..MOST_PAGES {
            let page = registry.choices(host, "sleep.show", cursor, &mut input, &mut out)?;
            for row in page.rows() {
                if native_sleep::is_foreign(row.value()) {
                    push(&mut found, copy(row.value())?)?;
                }
            }
            match page.end().next_cursor() {
                Some(next) => cursor = next,
                None => break,
            }
        }
        Ok(found)
    }

    /// The registry, opened the first time it is needed.
    fn registry(&mut self, host: &Host<'_>) -> Result<&Registry, Error> {
        if self.registry.is_none() {
            self.registry = Some(Registry::open(host)?);
        }
        self.registry.as_ref().ok_or(Error::State)
    }

    /// The screens now offered for the slot `core.sleep`, highest priority first.
    fn candidates(&mut self, host: &Host<'_>) -> Result<Vec<String>, Error> {
        let registry = self.registry(host)?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let mut offered = Vec::new();
        let mut cursor = PageCursor::start();
        for _ in 0..MOST_PAGES {
            let page =
                registry.slot_offers(host, native_sleep::SLOT, cursor, &mut input, &mut out)?;
            for item in page.rows() {
                push(&mut offered, copy(item.id())?)?;
            }
            match page.end().next_cursor() {
                Some(next) => cursor = next,
                None => break,
            }
        }
        Ok(offered)
    }

    fn opened(
        &mut self,
        host: &Host<'_>,
        instance: Instance,
        handover: Option<HandoverId>,
    ) -> Result<(), Error> {
        if self.ui.is_none() {
            self.ui = Some(Ui::open(host)?);
        }
        let ui = self.ui.as_ref().ok_or(Error::State)?;
        // What cannot be read shows the message: a device that cannot show what
        // was asked still shows that it sleeps.
        let view = match Sleeping::open(host, ui, handover) {
            Ok(view) => view,
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => Sleeping::plain(false),
        };
        self.screens.open(instance, view)
    }

    fn shown(&mut self, host: &Host<'_>, instance: Instance) -> Result<(), Error> {
        self.screens.shown(SCREEN, instance)?;
        let Some(view) = self.screens.top(SCREEN) else {
            return Ok(());
        };
        // Shown again after waking, once what opened above it closed: it goes
        // too.
        if view.woke {
            return close(host);
        }
        let mut scratch = [0; 64];
        view.refresh(&mut CommandSender::new(host, &mut scratch))
    }

    /// The device woke: every open sleep screen goes.
    fn woke(&mut self, host: &Host<'_>) -> Result<(), Error> {
        for view in self.screens.all() {
            view.woke = true;
        }
        // Only closed, as in interface 6: no redraw of what is going.
        close(host)
    }
}
