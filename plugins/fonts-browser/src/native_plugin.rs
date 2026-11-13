//! The interface-7 fonts browser: the list of the library's fonts (the
//! catalog fetched with `core.net` into the data folder and kept there) and a
//! font's page, on `core.ui` list screens (`PORTING-7.md` says what is not
//! native yet: downloading and installing a font, removing it, the font's own
//! face in the list). *Your own fonts* is `native_own`.
//!
//! The font a page shows is handed over as the screen's one parameter
//! (`Font::to_param`), so that the page does not read the catalog again.
use crate::{
    declarations::{DOWNLOADED, FONT, INSTALLED, LIST, OPEN, OWN, PLUGINS, SETTING},
    native_catalog::{Catalog, Font},
    native_job::Job,
    native_sample,
    native_view::{Installed, Row, Running, Trouble, rows, should_fetch, wants_to_go_on},
};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Handled,
    native::{
        Error, Host, Plugin,
        callbacks::{DrawFrame, Input},
        commands::CommandSender,
        device::{Device, DeviceCapability, PowerLevel},
        events::{Event, Incoming, kinds},
        formatting::Arg,
        net::{FileRoot, Net, REPLY, REQUEST_INPUT, Reason, Request, RequestId},
        registrations,
        registry::{PageCursor, Registry},
        reply::Reply,
        settings::{self, SettingValue},
        snapshot,
        storage::DataStorage,
        ui::{HandoverId, Instance, Open, Param, Ui},
    },
};
use paperpaw_ui_kit::{
    Footer, Item, ItemList, ItemStep, Messages, Screens,
    mem::{SERVICE_BUFFER, buffer, push},
    read_event,
    text::{concat, copy},
};

mod image {
    include!(concat!(env!("OUT_DIR"), "/fonts_browser_declarations.rs"));
}

/// The setting the library's address is in.
const CATALOG_SETTING: &str = "fonts-browser.catalog";
/// The settings the reading font and the status lines' font are chosen in.
const READING_FONT: &str = "core.reading-font";
const STATUS_FONT: &str = "core.status-font";
/// The plugin's number of the catalog's request, in its `http_done` event.
const CATALOG_REQUEST: u32 = 1;
/// The token of the font files' requests, and the numbers of the host
/// requests (an install, a removal) in their `install_done`.
const DOWNLOADED_FILE: u32 = 2;
/// The token of a font's sample request.
const SAMPLE_REQUEST: u32 = 3;
/// The first request number of an install; each download counts up from it.
const INSTALL_REQUEST: u32 = 16;
const REMOVE_REQUEST: u32 = 2;
/// The catalog kept from the last visit, and the one coming in.
const KEPT: &str = "catalog.json";
const PART: &str = "catalog.json.part";
/// How long the network is asked for when the user goes online (the most is
/// 600,000).
const GO_ONLINE_MS: u32 = 300_000;
/// The largest list of fonts taken from a library.
const MAX_LIST_FILE: u64 = 512 * 1024;
/// The screen's parameter that carries the font.
const FONT_PARAM: &str = "font";
/// Pages of the plugins read for the fonts installed (about seventy plugins).
const MOST_PLUGIN_PAGES: usize = 16;

/// The list of fonts.
#[derive(Debug)]
struct ListView {
    fonts: Vec<Font>,
    list: ItemList,
    trouble: Trouble,
    asked: bool,
    /// The user went online for the list (the offer shows once).
    decided: bool,
}

/// A font's page.
#[derive(Debug)]
struct FontView {
    font: Option<Font>,
    /// What each item of `list` does (`None` for the lines of text).
    rows: Vec<Option<Row>>,
    list: ItemList,
    /// The offer to go online for a download (an update or not) shows in
    /// place of the rows.
    offer: Option<bool>,
    /// The user went online for this page (the offer shows once).
    decided: bool,
    /// A download is going on: the footer says it can be left.
    downloading: bool,
    /// The sample of the font in its own face: where it is kept, and whether it
    /// is there (item 0 of `list` is the sentence in the menus' face until it is).
    sample: Option<Sample>,
}

/// The sample of a font kept in the data folder.
#[derive(Debug)]
struct Sample {
    path: String,
    ready: bool,
}

#[derive(Debug)]
#[allow(
    clippy::large_enum_variant,
    reason = "one view per open screen; boxing it would be an allocation that can fail"
)]
enum View {
    List(ListView),
    Font(FontView),
}

/// The interface-7 fonts browser.
#[derive(Debug, Default)]
pub struct NativeFontsBrowser {
    ui: Option<Ui>,
    net: Option<Net>,
    registry: Option<Registry>,
    device: Option<Device>,
    screens: Screens<View>,
    /// The catalog's request in flight.
    fetching: Option<RequestId>,
    /// The font being downloaded, and the request of its file in flight.
    job: Option<Job>,
    file_request: Option<RequestId>,
    /// The request of the sample of the font on show.
    sampling: Option<RequestId>,
    /// Downloads started so far: each has its own install request number.
    started: u32,
}

#[path = "native_install.rs"]
mod install;

impl Plugin for NativeFontsBrowser {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::SCREENS
            .iter()
            .chain(image::SETTINGS)
            .chain(image::NAVIGATION)
            .chain(image::EVENTS)
            .chain(image::INSTALLS)
        {
            registrations::submit(host, batch)?;
        }
        // Nothing is downloading yet, so whatever a cut-off run left is stale.
        paperpaw_sdk::native::storage::remove(
            host,
            paperpaw_sdk::native::storage::Root::Data,
            crate::native_plan::STAGING,
        )
        .ok();
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
            kinds::ActionRun,
            kinds::HttpDone,
            kinds::InstallDone,
            kinds::SettingChanged,
            kinds::PluginsChanged,
            kinds::ScreenOpened,
            kinds::ScreenShown,
            kinds::ScreenClosed
        ) else {
            return Ok(());
        };
        match event {
            Event::ActionRun {
                action: "fonts-browser.open",
                ..
            } if token == OPEN => {
                let mut scratch = [0; 128];
                CommandSender::new(host, &mut scratch).open(Open::screen("fonts-browser.list")?)
            }
            Event::ActionRun {
                action: "fonts-browser.own",
                ..
            } if token == OWN => crate::native_own::show(host, self.ui(host)?),
            Event::HttpDone(done) if token == DOWNLOADED => {
                if done.token() == CATALOG_REQUEST && self.fetching == Some(done.id()) {
                    self.fetching = None;
                    self.catalog_done(host, &done)?;
                } else if done.token() == DOWNLOADED_FILE {
                    self.file_done(host, &done)?;
                } else if done.token() == SAMPLE_REQUEST && self.sampling == Some(done.id()) {
                    self.sampling = None;
                    self.sample_done(host, &done)?;
                }
                Ok(())
            }
            Event::InstallDone(done) if token == INSTALLED => self.install_done(host, &done),
            // A font chosen, installed or removed shows on the pages.
            Event::SettingChanged { key, .. } if token == SETTING => {
                if key == READING_FONT || key == STATUS_FONT {
                    self.refresh(host)?;
                }
                Ok(())
            }
            Event::PluginsChanged { .. } if token == PLUGINS => self.refresh(host),
            Event::ScreenOpened { instance, .. } if token == LIST => self.open_list(host, instance),
            Event::ScreenOpened {
                instance, handover, ..
            } if token == FONT => self.open_font(host, instance, handover),
            Event::ScreenShown {
                instance: Some(instance),
                ..
            } if token == LIST || token == FONT => {
                self.screens.shown(token, instance)?;
                if token == LIST {
                    self.fetch_once(host)?;
                }
                Ok(())
            }
            Event::ScreenClosed { instance, .. } => {
                // A sample still on its way is not waited for by a screen that is gone.
                if token == FONT {
                    self.sampling = None;
                }
                self.screens.close(instance);
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn draw(&mut self, host: &Host<'_>, token: u32, mut frame: DrawFrame<'_>) -> Result<(), Error> {
        let mut messages = Messages::new()?;
        match self.screens.top(token).ok_or(Error::State)? {
            View::List(view) => {
                let title = messages.copy(host, "fonts-browser.title")?;
                if view.fonts.is_empty() {
                    let (head, text) = match view.trouble {
                        Trouble::Offer => ("core.offline.title", "fonts-browser.offline-offer"),
                        Trouble::Offline => ("fonts-browser.offline", "fonts-browser.offline.help"),
                        Trouble::Failed => (
                            "fonts-browser.unreachable",
                            "fonts-browser.unreachable.help",
                        ),
                        _ => ("fonts-browser.loading", ""),
                    };
                    let (head, text) = (
                        messages.copy(host, head)?,
                        input_text(host, &mut messages, text)?,
                    );
                    return view.list.draw_with(
                        host,
                        &mut frame,
                        &title,
                        true,
                        Footer::Empty {
                            title: &head,
                            text: &text,
                        },
                    );
                }
                let hint = input_text(host, &mut messages, "fonts-browser.hint")?;
                view.list
                    .draw_with(host, &mut frame, &title, true, Footer::Note(&hint))
            }
            View::Font(view) => {
                let title = match &view.font {
                    Some(font) => copy(&font.name)?,
                    None => messages.copy(host, "fonts-browser.title")?,
                };
                if view.offer.is_some() {
                    let head = messages.copy(host, "core.offline.title")?;
                    let text = input_text(host, &mut messages, "fonts-browser.offline-offer")?;
                    return view.list.draw_with(
                        host,
                        &mut frame,
                        &title,
                        true,
                        Footer::Empty {
                            title: &head,
                            text: &text,
                        },
                    );
                }
                if view.downloading {
                    let note = messages.copy(host, "fonts-browser.downloading")?;
                    return view.list.draw_with(
                        host,
                        &mut frame,
                        &title,
                        true,
                        Footer::Note(&note),
                    );
                }
                view.list
                    .draw_with(host, &mut frame, &title, true, Footer::None)?;
                Self::draw_sample(host, &mut frame, view)
            }
        }
    }

    fn input(&mut self, host: &Host<'_>, token: u32, input: Input<'_>) -> Result<Handled, Error> {
        let Some(view) = self.screens.top(token) else {
            return Ok(Handled::No);
        };
        match view {
            View::List(view) => {
                let step = view.list.input(host, &input)?;
                match step {
                    ItemStep::Chosen(index) => {
                        return Self::choose_font(host, &mut self.ui, view, index);
                    }
                    // A list that says why it is empty: choosing it goes online
                    // (where it offers that) or tries again.
                    ItemStep::Ignored
                        if view.fonts.is_empty()
                            && matches!(
                                view.trouble,
                                Trouble::Offer | Trouble::Offline | Trouble::Failed
                            )
                            && wants_to_go_on(&input) => {}
                    _ => return Ok(ItemList::handled(step)),
                }
                let offer = view.trouble == Trouble::Offer;
                view.asked = false;
                view.decided |= offer;
                if offer {
                    Self::go_online(host)?;
                }
                self.fetch_once(host)?;
                Ok(Handled::Yes)
            }
            View::Font(view) => {
                let step = view.list.input(host, &input)?;
                if let Some(update) = view.offer {
                    if step != ItemStep::Ignored || !wants_to_go_on(&input) {
                        return Ok(ItemList::handled(step));
                    }
                    view.offer = None;
                    view.decided = true;
                    let font = view.font.as_ref().map(Font::try_clone).transpose()?;
                    Self::go_online(host)?;
                    if let Some(font) = font {
                        self.download(host, &font, update)?;
                    }
                    return Ok(Handled::Yes);
                }
                if let ItemStep::Chosen(index) = step {
                    let picked = match (&view.font, view.rows.get(index)) {
                        (Some(font), Some(Some(row))) => Some((font.try_clone()?, *row)),
                        _ => None,
                    };
                    return match picked {
                        Some((font, row)) => self.choose_row(host, &font, row),
                        None => Ok(Handled::Yes),
                    };
                }
                Ok(ItemList::handled(step))
            }
        }
    }
}

/// The catalog's address, trimmed.
fn catalog_url(host: &Host<'_>) -> Result<String, Error> {
    let mut scratch = [0; 512];
    match settings::read_setting(host, CATALOG_SETTING, &mut scratch)? {
        SettingValue::Text(text) => copy(text.trim()),
        _ => Err(Error::Invalid),
    }
}

/// `key`'s text for the way this device is used (`<key>.buttons` where there
/// is no touch and the text has one), or nothing for no key.
fn input_text(host: &Host<'_>, messages: &mut Messages, key: &str) -> Result<String, Error> {
    if key.is_empty() {
        return Ok(String::new());
    }
    if !snapshot::capture_snapshot(host)?.has_touch() {
        let buttons = concat(&[key, ".buttons"])?;
        match messages.copy(host, &buttons) {
            Ok(text) => return Ok(text),
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => {}
        }
    }
    messages.copy(host, key)
}

impl NativeFontsBrowser {
    fn ui(&mut self, host: &Host<'_>) -> Result<&Ui, Error> {
        if self.ui.is_none() {
            self.ui = Some(Ui::open(host)?);
        }
        self.ui.as_ref().ok_or(Error::State)
    }

    fn net(&mut self, host: &Host<'_>) -> Result<&Net, Error> {
        if self.net.is_none() {
            self.net = Some(Net::open(host)?);
        }
        self.net.as_ref().ok_or(Error::State)
    }

    /// Whether this device takes font files rather than packs.
    fn outlines(&mut self, host: &Host<'_>) -> Result<bool, Error> {
        if self.device.is_none() {
            self.device = Some(Device::open(host)?);
        }
        let device = self.device.as_ref().ok_or(Error::State)?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let identity = device.device(host, &mut input, &mut out)?;
        Ok(paperpaw_sdk::text::OUTLINES
            && identity.class() == paperpaw_sdk::native::device::DeviceClass::Large)
    }

    /// Whether the device has a network at all.
    fn has_network(&mut self, host: &Host<'_>) -> Result<bool, Error> {
        if self.device.is_none() {
            self.device = Some(Device::open(host)?);
        }
        let device = self.device.as_ref().ok_or(Error::State)?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let identity = device.device(host, &mut input, &mut out)?;
        Ok(identity.supports(DeviceCapability::Network))
    }

    /// Whether the device is in Offline mode and the user has not gone online
    /// (the host ends a request then, unless the user's input is just before).
    fn in_offline_mode(&mut self, host: &Host<'_>) -> Result<bool, Error> {
        if self.device.is_none() {
            self.device = Some(Device::open(host)?);
        }
        let device = self.device.as_ref().ok_or(Error::State)?;
        let (mut input, mut out) = (buffer(64)?, buffer(SERVICE_BUFFER)?);
        let power = device.power(host, 0, &mut input, &mut out)?;
        Ok(power.level() == PowerLevel::Offline && !power.online())
    }

    /// Asks for the network for a while: the user chose to go online (the host
    /// honours it right after that input). Where it does not, the request that
    /// follows says so.
    fn go_online(host: &Host<'_>) -> Result<(), Error> {
        let mut scratch = [0; 64];
        match CommandSender::new(host, &mut scratch).go_online(GO_ONLINE_MS) {
            Err(Error::NoMemory) => Err(Error::NoMemory),
            _ => Ok(()),
        }
    }

    /// The fonts installed: every plugin whose id starts `font-`.
    fn installed(&mut self, host: &Host<'_>) -> Result<Installed, Error> {
        if self.registry.is_none() {
            self.registry = Some(Registry::open(host)?);
        }
        let registry = self.registry.as_ref().ok_or(Error::State)?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let mut installed = Installed::default();
        let mut cursor = PageCursor::start();
        for _ in 0..MOST_PLUGIN_PAGES {
            let page = registry.plugins(host, cursor, &mut input, &mut out)?;
            for plugin in page.rows() {
                installed.note(plugin.id(), plugin.version())?;
            }
            match page.end().next_cursor() {
                Some(next) => cursor = next,
                None => break,
            }
        }
        Ok(installed)
    }

    /// The catalog kept from the last visit (none if there is none).
    fn kept(&mut self, host: &Host<'_>) -> Result<Vec<Font>, Error> {
        let base = catalog_url(host)?;
        let outlines = self.outlines(host)?;
        let storage = DataStorage::new(host);
        Ok(Catalog::read_file(&base, outlines, &storage, KEPT)?.unwrap_or_default())
    }

    fn open_list(&mut self, host: &Host<'_>, instance: Instance) -> Result<(), Error> {
        let fonts = self.kept(host)?;
        let installed = self.installed(host)?;
        let running = self.running()?;
        let items = Self::font_items(host, &fonts, &installed, running.as_ref())?;
        self.screens.open(
            instance,
            View::List(ListView {
                fonts,
                list: ItemList::new(items),
                trouble: Trouble::None,
                asked: false,
                decided: false,
            }),
        )
    }

    /// The rows of the list: the name, what it is, and where it stands.
    fn font_items(
        host: &Host<'_>,
        fonts: &[Font],
        installed: &Installed,
        running: Option<&Running>,
    ) -> Result<Vec<Item>, Error> {
        let mut messages = Messages::new()?;
        let mut items = Vec::new();
        for font in fonts {
            let state = Self::state(host, &mut messages, font, installed, running)?;
            let note = Self::describe(host, &mut messages, font)?;
            push(&mut items, Item::noted(&font.name, &state, &note)?)?;
        }
        Ok(items)
    }

    /// What the row of `font` says at its end.
    fn state(
        host: &Host<'_>,
        messages: &mut Messages,
        font: &Font,
        installed: &Installed,
        running: Option<&Running>,
    ) -> Result<String, Error> {
        // The font being downloaded says how far it is.
        if let (Some(run), Ok(own)) = (running, font.plugin())
            && run.plugin == own
        {
            return messages.digits(host, &run.progress()?);
        }
        let here = font.plugin().ok().and_then(|id| installed.version(&id));
        let key = if here.is_some() && Self::chosen(host, READING_FONT, font) {
            "fonts-browser.state.reading"
        } else if font.newer(here) {
            "fonts-browser.state.update"
        } else if here.is_some() {
            "fonts-browser.state.installed"
        } else {
            return size_text(font.size());
        };
        messages.copy(host, key)
    }

    /// Whether `font` is the one chosen in `setting`.
    fn chosen(host: &Host<'_>, setting: &str, font: &Font) -> bool {
        let mut scratch = [0; 256];
        matches!(
            settings::read_setting(host, setting, &mut scratch),
            Ok(SettingValue::Text(name)) if name == font.name
        )
    }

    /// What a font is: its kind and how many styles, by whom.
    fn describe(host: &Host<'_>, messages: &mut Messages, font: &Font) -> Result<String, Error> {
        let key = concat(&["fonts-browser.category.", &font.category])?;
        let kind = match messages.copy(host, &key) {
            Ok(kind) => kind,
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => messages.copy(host, "fonts-browser.category.other")?,
        };
        let styles = messages.format(
            host,
            "fonts-browser.styles",
            &[Arg::Count(i64::from(font.styles.max(1)))],
        )?;
        if font.author.is_empty() {
            concat(&[&kind, " · ", &styles])
        } else {
            concat(&[&kind, " · ", &styles, " · ", &font.author])
        }
    }

    /// Asks for the catalog once each time the list is opened.
    fn fetch_once(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let Some(View::List(view)) = self.screens.top(LIST) else {
            return Ok(());
        };
        if !should_fetch(view.asked, self.fetching.is_some()) {
            return Ok(());
        }
        view.asked = true;
        let (empty, decided) = (view.fonts.is_empty(), view.decided);
        if !self.has_network(host)? {
            return self.trouble(Trouble::Offline);
        }
        // Offline mode: with no list yet the user is asked first, once.
        if empty && !decided && self.in_offline_mode(host)? {
            return self.trouble(Trouble::Offer);
        }
        let url = catalog_url(host)?;
        let (mut input, mut reply) = (buffer(REQUEST_INPUT)?, buffer(REPLY)?);
        let request = Request::get(CATALOG_REQUEST, &url)?
            .save_to(FileRoot::Data, PART)?
            .max_file(MAX_LIST_FILE)?;
        let id = self
            .net(host)?
            .request(host, &request, &mut input, &mut reply)?;
        self.fetching = Some(id);
        if empty {
            self.trouble(Trouble::Loading)?;
        }
        Ok(())
    }

    /// Notes why the list on top is empty.
    fn trouble(&mut self, trouble: Trouble) -> Result<(), Error> {
        if let Some(View::List(view)) = self.screens.top(LIST)
            && view.fonts.is_empty()
        {
            view.trouble = trouble;
        }
        Ok(())
    }

    /// The catalog arrived (or did not): kept if it is one.
    fn catalog_done(
        &mut self,
        host: &Host<'_>,
        done: &paperpaw_sdk::native::net::HttpDone,
    ) -> Result<(), Error> {
        let storage = DataStorage::new(host);
        let ok = done.reason() == Reason::Ok && (200..300).contains(&done.status());
        let fonts = if ok {
            let (mut input, mut reply) = (buffer(REQUEST_INPUT)?, buffer(REPLY)?);
            let answer = self
                .net(host)?
                .answer(host, done.id(), &mut input, &mut reply)?;
            let configured = catalog_url(host)?;
            let base = if answer.final_url().is_empty() {
                configured
            } else {
                copy(answer.final_url())?
            };
            let outlines = self.outlines(host)?;
            Catalog::read_file(&base, outlines, &storage, PART)?
        } else {
            None
        };
        match fonts {
            Some(fonts) => {
                paperpaw_sdk::Storage::rename(&storage, PART, KEPT).map_err(|_| Error::Io)?;
                let installed = self.installed(host)?;
                let running = self.running()?;
                let items = Self::font_items(host, &fonts, &installed, running.as_ref())?;
                if let Some(View::List(view)) = self.screens.top(LIST) {
                    view.fonts = fonts;
                    view.list = ItemList::new(items);
                    view.trouble = Trouble::None;
                }
            }
            None => {
                paperpaw_sdk::Storage::remove(&storage, PART).ok();
                // The catalog kept still serves; no answer at all is no connection.
                let trouble = if done.reason() == Reason::Offline {
                    Trouble::Offer
                } else if done.reason() == Reason::Ok && done.status() != 0 {
                    Trouble::Failed
                } else {
                    Trouble::Offline
                };
                self.trouble(trouble)?;
            }
        }
        let mut scratch = [0; 64];
        CommandSender::new(host, &mut scratch)
            .queue_redraw(paperpaw_sdk::native::commands::RedrawMode::Partial)
    }

    /// Opens the page of the font in row `index`.
    fn choose_font(
        host: &Host<'_>,
        ui: &mut Option<Ui>,
        view: &mut ListView,
        index: usize,
    ) -> Result<Handled, Error> {
        if let Some(font) = view.fonts.get(index) {
            if ui.is_none() {
                *ui = Some(Ui::open(host)?);
            }
            let param = font.to_param()?;
            let params = [Param::new(FONT_PARAM, &param)?];
            let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
            let handover = ui
                .as_ref()
                .ok_or(Error::State)?
                .handover(host, &params, &mut input, &mut out)?;
            let mut scratch = [0; 256];
            CommandSender::new(host, &mut scratch)
                .open(Open::screen("fonts-browser.font")?.handover(handover))?;
        }
        Ok(Handled::Yes)
    }

    fn open_font(
        &mut self,
        host: &Host<'_>,
        instance: Instance,
        handover: Option<HandoverId>,
    ) -> Result<(), Error> {
        let mut text = String::new();
        if let Some(handover) = handover {
            let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
            for param in self
                .ui(host)?
                .params(host, handover, &mut input, &mut out)?
            {
                let param = param?;
                if param.key() == FONT_PARAM {
                    text = copy(param.value())?;
                }
            }
        }
        let font = Font::from_param(&text)?;
        let installed = self.installed(host)?;
        let running = self.running()?;
        let mut view = Self::font_view(host, font, &installed, running.as_ref(), false)?;
        // The sample is asked for; the page shows the plain line until it is there.
        view.sample = self.want_sample(host, &view)?;
        self.screens.open(instance, View::Font(view))
    }

    /// The sample of the font of `view`: kept already (ready), or asked for while
    /// the device has a network. A library that has none, or a refusal to ask,
    /// leaves the plain line.
    fn want_sample(&mut self, host: &Host<'_>, view: &FontView) -> Result<Option<Sample>, Error> {
        let Some(font) = &view.font else {
            return Ok(None);
        };
        if font.sample.is_empty() {
            return Ok(None);
        }
        let mut scratch = [0; 32];
        let language = paperpaw_sdk::native::i18n::language(host, &mut scratch)?;
        let path = native_sample::path(&font.id, &font.version, language)?;
        let storage = DataStorage::new(host);
        if paperpaw_sdk::Storage::exists(&storage, &path) {
            return Ok(Some(Sample { path, ready: true }));
        }
        if self.sampling.is_some() || !self.has_network(host)? {
            return Ok(None);
        }
        let sentence = Messages::new()?.copy(host, "fonts-browser.sample")?;
        let url = native_sample::url(
            &font.sample,
            &native_sample::characters(&font.name, &sentence)?,
        )?;
        paperpaw_sdk::Storage::create_dir(&storage, native_sample::DIR).ok();
        let (mut input, mut reply) = (buffer(REQUEST_INPUT)?, buffer(REPLY)?);
        let request = Request::get(SAMPLE_REQUEST, &url)?
            .save_to(FileRoot::Data, &path)?
            .max_file(native_sample::MAX_FILE)?;
        match self
            .net(host)?
            .request(host, &request, &mut input, &mut reply)
        {
            Ok(id) => {
                self.sampling = Some(id);
                Ok(Some(Sample { path, ready: false }))
            }
            Err(Error::NoMemory) => Err(Error::NoMemory),
            Err(_) => Ok(None),
        }
    }

    /// The sample arrived (or did not): a file that is no sample is removed.
    fn sample_done(
        &mut self,
        host: &Host<'_>,
        done: &paperpaw_sdk::native::net::HttpDone,
    ) -> Result<(), Error> {
        let ok = done.reason() == Reason::Ok && (200..300).contains(&done.status());
        let mut gone = None;
        if let Some(View::Font(view)) = self.screens.top(FONT)
            && let Some(sample) = &mut view.sample
            && !sample.ready
        {
            if ok {
                sample.ready = true;
            } else {
                gone = view.sample.take();
            }
        }
        if let Some(gone) = gone {
            paperpaw_sdk::Storage::remove(&DataStorage::new(host), &gone.path).ok();
        }
        let mut scratch = [0; 64];
        CommandSender::new(host, &mut scratch)
            .queue_redraw(paperpaw_sdk::native::commands::RedrawMode::Partial)
    }

    /// The page of `font`: what it is, and what can be done with it.
    fn font_view(
        host: &Host<'_>,
        font: Option<Font>,
        installed: &Installed,
        running: Option<&Running>,
        offer: bool,
    ) -> Result<FontView, Error> {
        let mut messages = Messages::new()?;
        let mut items = Vec::new();
        let mut kinds: Vec<Option<Row>> = Vec::new();
        let Some(font) = font else {
            push(
                &mut items,
                Item::heading(&messages.copy(host, "fonts-browser.gone")?)?,
            )?;
            push(&mut kinds, None)?;
            push(
                &mut items,
                Item::heading(&messages.copy(host, "fonts-browser.gone.help")?)?,
            )?;
            push(&mut kinds, None)?;
            return Ok(FontView {
                font: None,
                rows: kinds,
                list: ItemList::new(items),
                offer: None,
                decided: false,
                downloading: false,
                sample: None,
            });
        };
        // The sentence in the menus' font (BLOCKED #228: the font's own face
        // needs a font loaded at run time), what it is, and the licence.
        push(
            &mut items,
            Item::heading(&messages.copy(host, "fonts-browser.sample")?)?,
        )?;
        push(&mut kinds, None)?;
        push(
            &mut items,
            Item::heading(&Self::describe(host, &mut messages, &font)?)?,
        )?;
        push(&mut kinds, None)?;
        let licence = messages.format(
            host,
            "fonts-browser.licence",
            &[Arg::Text {
                name: "licence",
                value: &font.licence,
            }],
        )?;
        push(&mut items, Item::heading(&licence)?)?;
        push(&mut kinds, None)?;
        // The offer to go online takes the place of the rows.
        if !offer {
            let here = font.plugin().ok().and_then(|id| installed.version(&id));
            let (busy, installing) = match (running, font.plugin()) {
                (Some(run), Ok(own)) if run.plugin == own => (true, run.installing),
                _ => (false, false),
            };
            for row in rows(&font, here, busy, installing)? {
                let (title, value) = Self::row_text(host, &mut messages, &font, row, running)?;
                let item = Item::row(&title, &value)?;
                push(&mut items, item)?;
                push(&mut kinds, Some(row))?;
            }
        }
        let downloading = kinds.contains(&Some(Row::Stop));
        Ok(FontView {
            font: Some(font),
            rows: kinds,
            list: ItemList::new(items),
            offer: None,
            decided: false,
            downloading,
            sample: None,
        })
    }

    fn row_text(
        host: &Host<'_>,
        messages: &mut Messages,
        font: &Font,
        row: Row,
        running: Option<&Running>,
    ) -> Result<(String, String), Error> {
        let chosen = |setting: &str| {
            if Self::chosen(host, setting, font) {
                copy("•")
            } else {
                Ok(String::new())
            }
        };
        Ok(match row {
            Row::Download => (
                messages.copy(host, "fonts-browser.download")?,
                size_text(font.size())?,
            ),
            Row::Stop => (
                messages.copy(host, "fonts-browser.stop")?,
                match running {
                    Some(run) => messages.digits(host, &run.progress()?)?,
                    None => String::new(),
                },
            ),
            Row::Read => (
                messages.format(
                    host,
                    "fonts-browser.read-with",
                    &[Arg::Text {
                        name: "name",
                        value: &font.name,
                    }],
                )?,
                chosen(READING_FONT)?,
            ),
            Row::Status => (
                messages.copy(host, "fonts-browser.status-lines")?,
                chosen(STATUS_FONT)?,
            ),
            Row::Update => (
                messages.copy(host, "fonts-browser.update")?,
                copy(&font.version)?,
            ),
            Row::Remove => (messages.copy(host, "fonts-browser.remove")?, String::new()),
        })
    }

    /// The sentence over its line in the sample's own face (item 0 of the page).
    /// A sample that cannot be drawn is let go: the plain line stays.
    fn draw_sample(
        host: &Host<'_>,
        frame: &mut DrawFrame<'_>,
        view: &mut FontView,
    ) -> Result<(), Error> {
        let Some(sample) = view.sample.as_ref().filter(|s| s.ready) else {
            return Ok(());
        };
        let Some(area) = view.list.rect_of(0) else {
            return Ok(());
        };
        let sentence = Messages::new()?.copy(host, "fonts-browser.sample")?;
        if !native_sample::draw(host, frame, area, (&sentence, &sample.path))?
            && let Some(gone) = view.sample.take()
        {
            paperpaw_sdk::Storage::remove(&DataStorage::new(host), &gone.path).ok();
        }
        Ok(())
    }

    /// What choosing a row of a font's page does.
    fn choose_row(&mut self, host: &Host<'_>, font: &Font, row: Row) -> Result<Handled, Error> {
        match row {
            Row::Read | Row::Status => {
                let setting = if row == Row::Read {
                    READING_FONT
                } else {
                    STATUS_FONT
                };
                let mut scratch = [0; 384];
                CommandSender::new(host, &mut scratch)
                    .set_setting(setting, SettingValue::Text(&font.name))?;
            }
            Row::Download => self.download(host, font, false)?,
            Row::Update => self.download(host, font, true)?,
            Row::Stop => self.stop(host)?,
            Row::Remove => self.remove(host, font)?,
        }
        Ok(Handled::Yes)
    }
}

/// A size as the list shows it: kilobytes, or megabytes from 1,000.
fn size_text(bytes: u64) -> Result<String, Error> {
    let (mut a, mut b) = ([0u8; 20], [0u8; 20]);
    if bytes >= 1_000_000 {
        let tenths = bytes / 100_000;
        let whole = paperpaw_ui_kit::text::decimal_u64(tenths / 10, &mut a);
        concat(&[
            whole,
            ".",
            paperpaw_ui_kit::text::decimal_u64(tenths % 10, &mut b),
            " MB",
        ])
    } else {
        let kilo = paperpaw_ui_kit::text::decimal_u64(bytes.div_ceil(1000).max(1), &mut a);
        concat(&[kilo, " KB"])
    }
}
