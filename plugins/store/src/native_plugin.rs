//! The interface-7 plugin store: the settings, the action that opens it, the
//! screens (`native_screens`, Port D's) carried by this file, the fetch of the
//! list and of a plugin's page over `core.net` into the index kept on the card
//! (`native_load`), and the downloads and installs (`native_jobs`).
//!
//! **Memory (#248):** nothing is read or built at start (the settings and the
//! screens are submitted, the handles are opened when first needed). What is
//! installed is read when a screen opens and let go of when the last one
//! closes; the list is read from the card a page at a time by the screens; the
//! device and the facts only for the length of a fetch.
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::Handled;
use paperpaw_sdk::native::{
    Error, Host, Plugin,
    callbacks::{DrawFrame, Input},
    commands::CommandSender,
    events::{Event, Incoming, kinds},
    image::ImageStatus,
    registrations,
    reply::Reply,
    ui::{Instance, RequestId as AskId, Ui},
};
use paperpaw_ui_kit::{read_event, screens::Screens};

use crate::declarations::{
    FETCH_SCREEN, HTTP_TOKEN, INSTALL_TOKEN, LETTERS_SCREEN, LIST_SCREEN, OFFER_SCREEN, OPEN_TOKEN,
    PLUGIN_SCREEN, PLUGINS_TOKEN, SHOTS_SCREEN, SOURCE_SCREEN, STARTED_TOKEN, VIEW_SCREEN,
};
use crate::native_download::Download;
use crate::native_glue::{self, Reply as Typed};
use crate::native_install::Installer;
use crate::native_installs::{Activity, HostInstalls, Snapshot};
use crate::native_load::{Loader, Wire as _};
use crate::native_screens::{Ctx, LoaderView, Reply as ScreenReply, StoreView, ids};
use crate::native_wire::{Handles, HostWire};

mod image {
    include!(concat!(env!("OUT_DIR"), "/store_declarations.rs"));
}

/// An open screen of the store, and whether it was on top yet.
#[derive(Debug)]
struct Slot {
    view: StoreView,
    started: bool,
}

/// The question the reader is answering: the dialog, the screen that asked, and
/// the tag it gave.
#[derive(Debug)]
struct Asking {
    request: AskId,
    instance: Instance,
    tag: u8,
    text: bool,
}

/// The interface-7 plugin store.
#[derive(Debug, Default)]
pub struct NativeStore {
    /// The list and the page held; read from the card when a screen asks.
    pub loader: Loader,
    pub(crate) handles: Handles,
    pub(crate) ui: Option<Ui>,
    /// Counts the lists taken, so a screen picks its plugins from a fresh one.
    pub(crate) generation: u32,
    /// Whether the device has little memory (asked once).
    small: Option<bool>,
    screens: Screens<Slot>,
    /// The screen on top, as the host last said.
    top: Option<Instance>,
    pub(crate) download: Download,
    pub(crate) installer: Installer,
    /// What the device has installed, read while a screen is open.
    pub(crate) snapshot: Option<Snapshot>,
    pub(crate) activity: Activity,
    /// The plugin whose page is awaited to install it.
    pub(crate) want: Option<String>,
    /// Whether the page the queue waits for was asked for again.
    pub(crate) want_retried: bool,
    /// The removal the host was asked for: its request and the plugin's name.
    pub(crate) removing: Option<(u32, String)>,
    /// What the install under way needs of the page it comes from.
    pub(crate) installing: Option<crate::native_list::Page>,
    pub(crate) installing_version: Option<String>,
    /// Counts the removals asked.
    pub(crate) remove_count: u32,
    /// A value a screen handed to the one below it, which is not on top yet.
    pub(crate) returned: Option<(String, String)>,
    asking: Option<Asking>,
    /// What is kept from ask to ask of plugins other plugins want.
    pub(crate) asks: crate::native_asks::Asks,
}

impl Plugin for NativeStore {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::SETTINGS
            .iter()
            .chain(image::SCREENS)
            .chain(image::SETUP)
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
        _reply: &mut Reply<'_>,
    ) -> Result<(), Error> {
        let Some(event) = read_event!(incoming; kinds::ActionRun, kinds::Started, kinds::HttpDone, kinds::HttpProgress, kinds::InstallDone, kinds::PluginsChanged, kinds::ImageReady, kinds::ScreenOpened, kinds::ScreenShown, kinds::ScreenClosed, kinds::Answered, kinds::PluginsRequested)
        else {
            return Ok(());
        };
        match &event {
            Event::ActionRun { .. } if token == OPEN_TOKEN => {
                let mut scratch = [0; 128];
                CommandSender::new(host, &mut scratch)
                    .open(paperpaw_sdk::native::ui::Open::screen(ids::LIST)?)
            }
            Event::ScreenOpened {
                screen,
                instance,
                handover,
                ..
            } if is_screen(token) => self.opened(host, screen, *instance, *handover),
            Event::ScreenShown {
                instance: Some(instance),
                ..
            } if is_screen(token) => self.shown(host, *instance),
            Event::Started { .. } if token == STARTED_TOKEN => self.started(host),
            Event::ScreenClosed { instance, .. } => self.closed(host, *instance),
            Event::PluginsRequested {
                now_ms,
                request,
                by,
                plugins,
                ..
            } if token == PLUGINS_TOKEN => {
                self.plugins_requested(host, *now_ms, *request, by, *plugins)
            }
            Event::Answered { now_ms, answer, .. } if self.asks.is_dialog(answer.request()) => {
                self.dialog_answered(host, *now_ms, answer)
            }
            Event::Answered { answer, .. } => self.answered(host, answer),
            Event::HttpDone(done) if token == HTTP_TOKEN => self.http_done(host, done, &event),
            Event::InstallDone(done) if token == INSTALL_TOKEN => {
                self.install_done(host, done, &event)
            }
            other => self.forward(host, other),
        }
    }

    fn draw(&mut self, host: &Host<'_>, token: u32, mut frame: DrawFrame<'_>) -> Result<(), Error> {
        let instance = self.screens.top_instance(token).ok_or(Error::State)?;
        self.call(host, instance, |view, ctx| view.draw(ctx, &mut frame))
    }

    fn input(&mut self, host: &Host<'_>, token: u32, input: Input<'_>) -> Result<Handled, Error> {
        let Some(instance) = self.screens.top_instance(token) else {
            return Ok(Handled::No);
        };
        let (handled, next) = self.call(host, instance, |view, ctx| view.input(ctx, &input))?;
        self.carry(host, instance, next)?;
        Ok(handled)
    }
}

/// Whether `token` is one of the screens'.
const fn is_screen(token: u32) -> bool {
    matches!(
        token,
        LIST_SCREEN
            | PLUGIN_SCREEN
            | VIEW_SCREEN
            | LETTERS_SCREEN
            | SOURCE_SCREEN
            | SHOTS_SCREEN
            | FETCH_SCREEN
            | OFFER_SCREEN
    )
}

impl NativeStore {
    pub(crate) fn ui(&mut self, host: &Host<'_>) -> Result<&Ui, Error> {
        if self.ui.is_none() {
            self.ui = Some(Ui::open(host)?);
        }
        self.ui.as_ref().ok_or(Error::State)
    }

    /// What the device has installed, read when a screen needs it.
    pub(crate) fn ensure_snapshot(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if self.snapshot.is_none() {
            let storage = crate::native_folders::StoreStorage::new(host);
            let snapshot = Snapshot::read(host, &storage)?;
            self.refresh_activity(host)?;
            self.activity.read_facts(host, &storage, Some(&snapshot))?;
            self.snapshot = Some(snapshot);
        }
        Ok(())
    }

    /// What was read of the device is stale (a plugin came or went).
    pub(crate) fn forget_snapshot(&mut self) {
        self.snapshot = None;
    }

    /// Whether a plugin's page is on a screen that is open.
    pub(crate) fn has_page_screen(&mut self) -> bool {
        self.screens
            .all()
            .any(|slot| matches!(slot.view, StoreView::Plugin(_)))
    }

    pub(crate) fn screens_empty(&self) -> bool {
        self.screens.is_empty()
    }

    fn small_memory(&mut self, host: &Host<'_>) -> bool {
        if let Some(small) = self.small {
            return small;
        }
        let small = HostWire::new(host, &mut self.handles).small_memory();
        self.small = Some(small);
        small
    }

    /// Runs `f` on the screen `instance` with what it may read.
    fn call<R>(
        &mut self,
        host: &Host<'_>,
        instance: Instance,
        f: impl FnOnce(&mut StoreView, &Ctx<'_>) -> Result<R, Error>,
    ) -> Result<R, Error> {
        self.ensure_snapshot(host)?;
        let small = self.small_memory(host);
        let storage = crate::native_folders::StoreStorage::new(host);
        let loader = LoaderView {
            loader: &self.loader,
            generation: self.generation,
            small_memory: small,
        };
        let snapshot = self.snapshot.as_ref().ok_or(Error::State)?;
        let installs = HostInstalls::new(snapshot, &self.activity);
        let ctx = Ctx {
            host,
            data: &storage,
            loader: &loader,
            installs: &installs,
        };
        let slot = self.screens.get(instance).ok_or(Error::State)?;
        f(&mut slot.view, &ctx)
    }

    #[inline(never)]
    fn opened(
        &mut self,
        host: &Host<'_>,
        screen: &str,
        instance: Instance,
        handover: Option<paperpaw_sdk::native::ui::HandoverId>,
    ) -> Result<(), Error> {
        let params = {
            let ui = self.ui(host)?;
            native_glue::params(host, ui, handover)?
        };
        let mut pairs: Vec<(&str, &str)> = Vec::new();
        pairs
            .try_reserve_exact(params.len())
            .map_err(|_| Error::NoMemory)?;
        for (name, value) in &params {
            pairs.push((name.as_str(), value.as_str()));
        }
        self.ensure_snapshot(host)?;
        let small = self.small_memory(host);
        let storage = crate::native_folders::StoreStorage::new(host);
        let view = {
            let loader = LoaderView {
                loader: &self.loader,
                generation: self.generation,
                small_memory: small,
            };
            let snapshot = self.snapshot.as_ref().ok_or(Error::State)?;
            let installs = HostInstalls::new(snapshot, &self.activity);
            let ctx = Ctx {
                host,
                data: &storage,
                loader: &loader,
                installs: &installs,
            };
            StoreView::open(&ctx, screen, &pairs)?
        };
        self.screens.open(
            instance,
            Slot {
                view,
                started: false,
            },
        )
    }

    /// `instance` is on top: the one before it is covered, and this one starts
    /// (the first time) or resumes.
    #[inline(never)]
    fn shown(&mut self, host: &Host<'_>, instance: Instance) -> Result<(), Error> {
        let token = self.token_of(instance);
        if let Some(token) = token {
            self.screens.shown(token, instance)?;
        }
        if let Some(before) = self.top.filter(|before| *before != instance)
            && let Some(slot) = self.screens.get(before)
        {
            slot.view.hidden();
        }
        self.top = Some(instance);
        let first = match self.screens.get(instance) {
            Some(slot) => !core::mem::replace(&mut slot.started, true),
            None => return Ok(()),
        };
        // A screen above closed with a value for this one.
        if let Some((key, value)) = self.returned.take() {
            let next = self.call(host, instance, |view, ctx| view.returned(ctx, &key, &value))?;
            self.carry(host, instance, next)?;
        }
        let next = self.call(host, instance, |view, ctx| {
            if first {
                view.started(ctx)
            } else {
                view.resume(ctx)
            }
        })?;
        self.carry(host, instance, next)
    }

    /// The screen token of an open instance.
    fn token_of(&mut self, instance: Instance) -> Option<u32> {
        let slot = self.screens.get(instance)?;
        Some(match &slot.view {
            StoreView::List(_) => LIST_SCREEN,
            StoreView::Plugin(_) => PLUGIN_SCREEN,
            StoreView::View(_) => VIEW_SCREEN,
            StoreView::Letters(_) => LETTERS_SCREEN,
            StoreView::Source(_) => SOURCE_SCREEN,
            StoreView::Shots(_) => SHOTS_SCREEN,
            StoreView::Fetch(_) => FETCH_SCREEN,
            StoreView::Offer(_) => OFFER_SCREEN,
        })
    }

    #[inline(never)]
    fn closed(&mut self, host: &Host<'_>, instance: Instance) -> Result<(), Error> {
        let was_page = self
            .screens
            .get(instance)
            .is_some_and(|slot| matches!(slot.view, StoreView::Plugin(_)));
        let result = if self.screens.get(instance).is_some() {
            self.call(host, instance, |view, ctx| view.closed(ctx))
        } else {
            Ok(())
        };
        self.screens.close(instance);
        if was_page
            && !self
                .screens
                .all()
                .any(|slot| matches!(slot.view, StoreView::Plugin(_)))
        {
            // The page held for the screen is let go when the last page screen
            // is left.
            self.loader.forget_page();
        }
        if self.top == Some(instance) {
            self.top = None;
        }
        if self.asking.as_ref().is_some_and(|a| a.instance == instance) {
            self.asking = None;
        }
        // Nothing of the device is kept while no screen is open.
        self.release_idle();
        result
    }

    /// An event for the screen on top (an answer, a download's end, a plugin
    /// that came).
    #[inline(never)]
    pub(crate) fn forward(&mut self, host: &Host<'_>, event: &Event<'_>) -> Result<(), Error> {
        if matches!(event, Event::PluginsChanged { .. }) {
            self.forget_snapshot();
        }
        let Some(instance) = self.top.filter(|i| self.screens.get(*i).is_some()) else {
            release_unwanted(host, event);
            return Ok(());
        };
        if matches!(event, Event::ImageReady(_))
            && !self
                .screens
                .get(instance)
                .is_some_and(|slot| matches!(slot.view, StoreView::Shots(_)))
        {
            // The pictures' screen was left before the host was done.
            release_unwanted(host, event);
            return Ok(());
        }
        let next = self.call(host, instance, |view, ctx| view.event(ctx, event))?;
        self.carry(host, instance, next)
    }

    /// The reader answered a question a screen asked.
    fn answered(
        &mut self,
        host: &Host<'_>,
        answer: &paperpaw_sdk::native::events::Answered,
    ) -> Result<(), Error> {
        let Some(asking) = &self.asking else {
            return Ok(());
        };
        let (request, text, instance, tag) =
            (asking.request, asking.text, asking.instance, asking.tag);
        let typed = {
            let ui = self.ui(host)?;
            native_glue::reply(host, ui, request, text, answer)?
        };
        let reply = match &typed {
            Typed::Wait => return Ok(()),
            Typed::Cancelled => {
                self.asking = None;
                return Ok(());
            }
            Typed::Choice(n) => ScreenReply::Choice(*n),
            Typed::Text(text) => ScreenReply::Text(text.as_str()),
        };
        self.asking = None;
        let next = self.call(host, instance, |view, ctx| view.answered(ctx, tag, reply))?;
        self.carry(host, instance, next)
    }

    /// Asks the reader what screen `instance` wants to know.
    pub(crate) fn start_ask(
        &mut self,
        host: &Host<'_>,
        instance: Instance,
        tag: u8,
        ask: &crate::native_screens::Ask,
    ) -> Result<(), Error> {
        let text = matches!(ask, crate::native_screens::Ask::Text { .. });
        let request = {
            let ui = self.ui(host)?;
            native_glue::ask(host, ui, ask)?
        };
        self.asking = Some(Asking {
            request,
            instance,
            tag,
            text,
        });
        Ok(())
    }
}

/// Gives back the picture of an `image_ready` no screen waits for any more: the
/// host keeps it until the plugin lets go.
fn release_unwanted(host: &Host<'_>, event: &Event<'_>) {
    if let Event::ImageReady(ready) = event
        && matches!(ready.status(), ImageStatus::Ready)
        && let Some(id) = ready.image()
    {
        let mut scratch = [0; 64];
        let _ = CommandSender::new(host, &mut scratch).image_release(id);
    }
}
