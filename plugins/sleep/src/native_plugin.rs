//! The interface-7 sleep screen: one overlay screen (`sleep.screen`, the slot
//! `core.sleep`) over `core.ui`, with the widgets it composes
//! (`native_screen`, `native_stack`) and what it decides (`native_sleep`).
//!
//! The device waking closes it (`event.resume`); a screen still there when the
//! device is awake goes at the next press, so that it never keeps an awake
//! device to itself. While asleep every press but *sleep* and *home* is taken.
//!
//! The picker of the picture or the widget over the page (`sleep.pick-overlay`,
//! `native_picker`) is the plugin's second screen; the editor of a list of
//! widgets (`sleep.edit-widgets` on the slot `core.edit-widgets`,
//! `native_widgets_edit`) its third.
use crate::{
    declarations::{
        EDIT, EVENTS, IMAGE_READY, LIBRARY_CHANGED, PICK, PLUGINS_CHANGED, SCREEN, SETTING_CHANGED,
        STARTED, SUSPEND,
    },
    native_picker::Picker,
    native_screen::{Sleeping, read_text},
    native_sleep,
    native_widgets_edit::Editor,
};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Handled,
    native::{
        Error, Host, Plugin,
        callbacks::{DrawFrame, Input},
        commands::CommandSender,
        events::{Event, Incoming, kinds},
        image::ImageReady,
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
    /// The pickers open; each goes with its screen.
    pickers: Screens<Picker>,
    /// The editors of lists of widgets open; each goes with its screen.
    editors: Screens<Editor>,
    /// Opened for the slot offers the first time a foreign mode needs them,
    /// then kept for the plugin's lifetime (as `ui`).
    registry: Option<Registry>,
    /// The cookie of the last picture asked for (counted on, never 0).
    cookie: u32,
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
            kinds::Suspend,
            kinds::Started,
            kinds::PluginsChanged,
            kinds::SettingChanged,
            kinds::ScreenOpened,
            kinds::ScreenShown,
            kinds::ScreenClosed,
            kinds::ImageReady,
            kinds::LibraryChanged,
            kinds::Answered
        ) else {
            return Ok(());
        };
        match event {
            Event::ScreenOpened {
                instance, handover, ..
            } if token == SCREEN => self.opened(host, instance, handover),
            Event::ScreenOpened { instance, .. } if token == PICK => {
                self.pick_opened(host, instance)
            }
            Event::ScreenOpened {
                instance, handover, ..
            } if token == EDIT => {
                self.ensure_ui(host)?;
                let registry = match self.registry(host) {
                    Ok(_) => self.registry.as_ref(),
                    Err(Error::NoMemory) => return Err(Error::NoMemory),
                    Err(_) => None,
                };
                let ui = self.ui.as_ref().ok_or(Error::State)?;
                let editor = Editor::open(ui, host, registry, handover)?;
                self.editors.open(instance, editor)
            }
            Event::ScreenShown {
                instance: Some(instance),
                ..
            } if token == EDIT => {
                self.editors.shown(EDIT, instance)?;
                match self.editors.get(instance) {
                    Some(editor) => editor.shown(host),
                    None => Ok(()),
                }
            }
            Event::ScreenClosed { instance, .. } if token == EDIT => {
                self.editors.close(instance);
                Ok(())
            }
            Event::Answered { answer, .. } => {
                let Some(ui) = self.ui.as_ref() else {
                    return Ok(());
                };
                for editor in self.editors.all() {
                    if editor.answered(ui, host, &answer)? {
                        break;
                    }
                }
                Ok(())
            }
            Event::ScreenShown {
                instance: Some(instance),
                ..
            } if token == SCREEN => self.shown(host, instance),
            Event::ScreenShown {
                instance: Some(instance),
                ..
            } if token == PICK => self.pickers.shown(PICK, instance),
            Event::ScreenClosed { instance, .. } if token == SCREEN => {
                // The pictures go back to the host with the screen, and why
                // the picture over the page did not show is kept.
                let mut kept = Ok(());
                if let Some(view) = self.screens.get(instance) {
                    view.release(host);
                    kept = view.note_over(host);
                }
                self.screens.close(instance);
                kept
            }
            Event::ScreenClosed { instance, .. } if token == PICK => {
                self.pickers.close(instance);
                Ok(())
            }
            Event::ImageReady(ready) if token == IMAGE_READY => self.image_ready(host, &ready),
            Event::LibraryChanged { what, .. } if token == LIBRARY_CHANGED && what.cover() => {
                self.cover_ready(host)
            }
            Event::Resume { .. } if token == EVENTS => self.woke(host),
            Event::Suspend { .. } if token == SUSPEND => self.suspended(host),
            // At start and when the plugins changed: a stored foreign mode
            // binds the slot where the reader has not bound it, never unbinds.
            Event::Started { .. } if token == STARTED => self.bind_all_stored(host),
            Event::PluginsChanged { .. } if token == PLUGINS_CHANGED => self.bind_all_stored(host),
            // A change of a setting that chooses a slot's screen
            // (`sleep.show`, `sleep.off-show`, `sleep.start-show`,
            // `sleep.restart-show`), or a bulk reset or import (the empty
            // key): bind an offered foreign mode, unbind only a sleep mode.
            Event::SettingChanged { key, .. } if token == SETTING_CHANGED => PLACES
                .into_iter()
                .filter(|place| key.is_empty() || place.setting == key)
                .try_for_each(|place| self.bind_mode(host, place)),
            _ => Ok(()),
        }
    }

    fn draw(&mut self, host: &Host<'_>, token: u32, mut frame: DrawFrame<'_>) -> Result<(), Error> {
        if token == EDIT {
            let editor = self.editors.top(token).ok_or(Error::State)?;
            return editor.draw(host, &mut frame);
        }
        if token == PICK {
            let picker = self.pickers.top(token).ok_or(Error::State)?;
            return picker.draw(host, &mut frame);
        }
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
        if token == EDIT {
            self.ensure_ui(host)?;
            let (Some(ui), Some(editor)) = (self.ui.as_ref(), self.editors.top(token)) else {
                return Ok(Handled::No);
            };
            return editor.input(ui, host, self.registry.as_ref(), &input);
        }
        if token == PICK {
            return match self.pickers.top(token) {
                Some(picker) => picker.input(host, &input),
                None => Ok(Handled::No),
            };
        }
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

impl NativeSleep {
    fn ensure_ui(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if self.ui.is_none() {
            self.ui = Some(Ui::open(host)?);
        }
        Ok(())
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

/// A slot and the setting that chooses what it shows: the sleep screen
/// (`core.sleep`, `sleep.show`) and the off screen (`core.off`,
/// `sleep.off-show`).
#[derive(Debug, Clone, Copy)]
struct Place {
    setting: &'static str,
    slot: &'static str,
}

const SLEEP: Place = Place {
    setting: native_sleep::SHOW,
    slot: native_sleep::SLOT,
};
const OFF: Place = Place {
    setting: native_sleep::OFF_SHOW,
    slot: native_sleep::OFF_SLOT,
};
/// The start and the restart screen (#219): another plugin's screen, or the
/// slot unbound (the device's own).
const PLACES: [Place; 4] = [
    SLEEP,
    OFF,
    Place {
        setting: native_sleep::START_SHOW,
        slot: native_sleep::START_SLOT,
    },
    Place {
        setting: native_sleep::RESTART_SHOW,
        slot: native_sleep::RESTART_SLOT,
    },
];

impl NativeSleep {
    /// The start or a change of the plugins, for every slot.
    fn bind_all_stored(&mut self, host: &Host<'_>) -> Result<(), Error> {
        PLACES
            .into_iter()
            .try_for_each(|place| self.bind_stored(host, place))
    }

    /// Binds the slot of `place` for its stored setting: to the screen
    /// of another plugin's mode while that plugin offers it, else back to the
    /// default (the plugin's own screen). A refusal (a screen not offered, no
    /// consent, a host without the command) binds nothing and is not the
    /// plugin's failure; only a memory shortage is reported.
    fn bind_mode(&mut self, host: &Host<'_>, place: Place) -> Result<(), Error> {
        match self.try_bind_mode(host, place) {
            Err(Error::NoMemory) => Err(Error::NoMemory),
            Ok(()) | Err(_) => Ok(()),
        }
    }

    fn try_bind_mode(&mut self, host: &Host<'_>, place: Place) -> Result<(), Error> {
        let value = read_text(host, place.setting)?;
        let (mut offered, mut current, mut choices) = (Vec::new(), None, Vec::new());
        if native_sleep::is_foreign(&value) {
            offered = self.candidates(host, place.slot)?;
        } else {
            current = self.bound(host, place.slot)?.map(|(screen, _)| screen);
            choices = self.foreign_choices(host, place.setting)?;
        }
        let screen = match native_sleep::plan(&value, &offered, current.as_deref(), &choices) {
            native_sleep::Bind::Screen(screen) => Some(screen),
            native_sleep::Bind::Default => None,
            _ => return Ok(()),
        };
        let mut scratch = [0; BIND_SCRATCH];
        match CommandSender::new(host, &mut scratch).bind_slot(place.slot, screen) {
            Ok(()) | Err(Error::Invalid | Error::NotFound | Error::Denied | Error::Unsupported) => {
                Ok(())
            }
            Err(error) => Err(error),
        }
    }

    /// The start or a change of the plugins: binds a stored foreign mode that is
    /// offered, unless the reader has bound the slot. Never unbinds. The same
    /// rules for errors as [`Self::bind_mode`].
    fn bind_stored(&mut self, host: &Host<'_>, place: Place) -> Result<(), Error> {
        match self.try_bind_stored(host, place) {
            Err(Error::NoMemory) => Err(Error::NoMemory),
            Ok(()) | Err(_) => Ok(()),
        }
    }

    fn try_bind_stored(&mut self, host: &Host<'_>, place: Place) -> Result<(), Error> {
        let value = read_text(host, place.setting)?;
        if !native_sleep::is_foreign(&value) {
            return Ok(());
        }
        let user_bound = self.bound(host, place.slot)?.is_some_and(|(_, user)| user);
        let offered = if user_bound {
            Vec::new()
        } else {
            self.candidates(host, place.slot)?
        };
        let native_sleep::Bind::Screen(screen) =
            native_sleep::plan_stored(&value, &offered, user_bound)
        else {
            return Ok(());
        };
        let mut scratch = [0; BIND_SCRATCH];
        match CommandSender::new(host, &mut scratch).bind_slot(place.slot, Some(screen)) {
            Ok(()) | Err(Error::Invalid | Error::NotFound | Error::Denied | Error::Unsupported) => {
                Ok(())
            }
            Err(error) => Err(error),
        }
    }

    /// The screen `slot` is bound to now, and whether the reader bound it
    /// (`reg_slot.flags` bit 0).
    fn bound(&mut self, host: &Host<'_>, slot: &str) -> Result<Option<(String, bool)>, Error> {
        let registry = self.registry(host)?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let mut cursor = PageCursor::start();
        for _ in 0..MOST_PAGES {
            let page = registry.slots(host, cursor, &mut input, &mut out)?;
            for row in page.rows() {
                if row.id() == slot {
                    return Ok(Some((copy(row.screen())?, row.flags() & 1 != 0)));
                }
            }
            match page.end().next_cursor() {
                Some(next) => cursor = next,
                None => break,
            }
        }
        Ok(None)
    }

    /// The choices of `setting` that are another plugin's modes.
    fn foreign_choices(&mut self, host: &Host<'_>, setting: &str) -> Result<Vec<String>, Error> {
        let registry = self.registry(host)?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let mut found = Vec::new();
        let mut cursor = PageCursor::start();
        for _ in 0..MOST_PAGES {
            let page = registry.choices(host, setting, cursor, &mut input, &mut out)?;
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

    /// The screens now offered for `slot`, highest priority first.
    fn candidates(&mut self, host: &Host<'_>, slot: &str) -> Result<Vec<String>, Error> {
        let registry = self.registry(host)?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let mut offered = Vec::new();
        let mut cursor = PageCursor::start();
        for _ in 0..MOST_PAGES {
            let page = registry.slot_offers(host, slot, cursor, &mut input, &mut out)?;
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
            Ok(mut view) => {
                // The pictures it shows are asked for now: a draw cannot.
                view.begin(host, &mut self.cookie)?;
                view
            }
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => Sleeping::plain(false),
        };
        // One opened again lets its pictures go first.
        if let Some(old) = self.screens.get(instance) {
            old.release(host);
        }
        self.screens.open(instance, view)
    }

    /// A cover the host was making is ready: the screens that waited for it ask
    /// again.
    fn cover_ready(&mut self, host: &Host<'_>) -> Result<(), Error> {
        for view in self.screens.all() {
            view.cover_ready(host, &mut self.cookie)?;
        }
        Ok(())
    }

    /// A picture the host decoded: the screen it was asked for takes it; one
    /// nobody waits for (the screen is gone) is given back.
    fn image_ready(&mut self, host: &Host<'_>, ready: &ImageReady) -> Result<(), Error> {
        for view in self.screens.all() {
            if view.answer(host, ready, &mut self.cookie)? {
                return Ok(());
            }
        }
        if let Some(image) = ready.image() {
            let mut scratch = [0; 64];
            CommandSender::new(host, &mut scratch)
                .image_release(image)
                .ok();
        }
        Ok(())
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

    /// The picker of the picture over the page opened.
    fn pick_opened(&mut self, host: &Host<'_>, instance: Instance) -> Result<(), Error> {
        // Without the registry the pictures folder is still there.
        let registry = match self.registry(host) {
            Ok(registry) => Some(registry),
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => None,
        };
        let picker = Picker::read(host, registry)?;
        self.pickers.open(instance, picker)
    }

    /// The device goes to sleep: a sleep screen that shows what changes with
    /// the time asks to be drawn again every `sleep.refresh-every` minutes, the
    /// first time at the next whole mark of the clock. A device that cannot
    /// refresh asleep says so (`Unsupported`), and nothing is asked twice: not
    /// a fault.
    fn suspended(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let Some(view) = self.screens.top(SCREEN) else {
            return Ok(());
        };
        let Some((wait, minutes)) = view.refresh_plan(host)? else {
            return Ok(());
        };
        let mut scratch = [0; 64];
        match CommandSender::new(host, &mut scratch).show_again(wait, minutes * 60_000) {
            Err(Error::Unsupported | Error::State | Error::Invalid) => Ok(()),
            other => other,
        }
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
