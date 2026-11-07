//! The flat About plugin: one list screen over what the host says about
//! this device and build.
use crate::native_model::{self, Facts, Kind, MOST_PAGES, Texts};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Handled,
    native::{
        Error, Host, Plugin,
        callbacks::{DrawFrame, Input},
        commands::CommandSender,
        device::{Device, DeviceCursor},
        events::Event,
        formatting::Arg,
        registrations,
        reply::Reply,
        ui::{Instance, Open},
    },
};
use paperpaw_ui_kit::{
    Footer, ItemList, ItemStep, Messages, Screens,
    mem::{SERVICE_BUFFER, buffer, push},
    text::copy,
};

mod image {
    include!(concat!(env!("OUT_DIR"), "/about_declarations.rs"));
}

const OPEN: u32 = crate::declarations::OPEN_TOKEN;
const SCREEN: u32 = crate::declarations::SCREEN_TOKEN;

/// One open page: its rows and what each does.
#[derive(Debug, Default)]
struct View {
    kinds: Vec<Kind>,
    list: ItemList,
}

/// The interface-7 About plugin.
#[derive(Debug, Default)]
pub struct NativeAbout {
    device: Option<Device>,
    screens: Screens<View>,
}

impl Plugin for NativeAbout {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::SCREENS.iter().chain(image::NAVIGATION) {
            registrations::submit(host, batch)?;
        }
        // `core.device` is opened by the first callback: opening needs a live frame.
        Ok(Self::default())
    }

    fn event(
        &mut self,
        host: &Host<'_>,
        token: u32,
        event: Event<'_>,
        _: &mut Reply<'_>,
    ) -> Result<(), Error> {
        match event {
            Event::ActionRun {
                action: "about.open",
                ..
            } if token == OPEN => {
                let mut scratch = [0; 128];
                CommandSender::new(host, &mut scratch).open(Open::screen("about.screen")?)
            }
            Event::ScreenOpened { instance, .. } if token == SCREEN => self.opened(host, instance),
            Event::ScreenShown {
                instance: Some(instance),
                ..
            } if token == SCREEN => self.screens.shown(SCREEN, instance),
            Event::ScreenClosed { instance, .. } => {
                self.screens.close(instance);
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn draw(&mut self, host: &Host<'_>, token: u32, mut frame: DrawFrame<'_>) -> Result<(), Error> {
        let view = self.screens.top(token).ok_or(Error::State)?;
        let title = Messages::new()?.copy(host, "about.title")?;
        view.list
            .draw_with(host, &mut frame, &title, true, Footer::None)
    }

    fn input(&mut self, host: &Host<'_>, token: u32, input: Input<'_>) -> Result<Handled, Error> {
        let Some(view) = self.screens.top(token) else {
            return Ok(Handled::No);
        };
        let step = view.list.input(host, &input)?;
        if let ItemStep::Chosen(index) = step {
            return match view.kinds.get(index) {
                // BLOCKED (#228): the text of these rows opens in the reader, and
                // interface 7 has no command to open a text there yet. Refused
                // plainly rather than faked; the rest of the page works.
                Some(Kind::Licences | Kind::BuiltIn) => Err(Error::Unsupported),
                _ => Ok(Handled::Yes),
            };
        }
        Ok(ItemList::handled(step))
    }
}

impl NativeAbout {
    fn opened(&mut self, host: &Host<'_>, instance: Instance) -> Result<(), Error> {
        if self.device.is_none() {
            self.device = Some(Device::open(host)?);
        }
        let device = self.device.as_ref().ok_or(Error::State)?;
        let facts = read_facts(host, device)?;
        let mut messages = Messages::new()?;
        let texts = texts(host, &mut messages, &facts)?;
        let rows = native_model::rows(&texts, &facts)?;
        let mut kinds = Vec::new();
        let mut items = Vec::new();
        for (kind, item) in rows {
            push(&mut kinds, kind)?;
            push(&mut items, item)?;
        }
        self.screens.open(
            instance,
            View {
                kinds,
                list: ItemList::new(items),
            },
        )
    }
}

/// Reads the firmware, the plugins built in and the build code plugins need.
/// A part the device has none of (`NotFound`) is not an error.
fn read_facts(host: &Host<'_>, device: &Device) -> Result<Facts, Error> {
    let mut input = buffer(SERVICE_BUFFER)?;
    let mut out = buffer(SERVICE_BUFFER)?;
    let mut facts = Facts::default();
    match device.firmware(host, &mut input, &mut out) {
        Ok(firmware) => facts.version = Some(copy(firmware.version())?),
        Err(Error::NotFound) => {}
        Err(error) => return Err(error),
    }
    let mut cursor = DeviceCursor::start();
    for _ in 0..MOST_PAGES {
        let page = match device.builtins(host, cursor, &mut input, &mut out) {
            Ok(page) => page,
            Err(Error::NotFound) => break,
            Err(error) => return Err(error),
        };
        facts.built_in = page.end().total();
        for plugin in page.rows() {
            push(&mut facts.names, copy(plugin.id())?)?;
        }
        match page.end().next_cursor() {
            Some(next) => cursor = next,
            None => break,
        }
    }
    match device.native(host, &mut input, &mut out) {
        Ok(native) => facts.native = Some((native.abi_major(), native.abi_draft())),
        Err(Error::NotFound) => {}
        Err(error) => return Err(error),
    }
    Ok(facts)
}

/// The page's words, in the reader's language.
fn texts(host: &Host<'_>, messages: &mut Messages, facts: &Facts) -> Result<Texts, Error> {
    // Where there is no firmware (the desktop, Android) the SDK's version is PaperPaw's.
    let own = match facts.version {
        Some(_) => String::new(),
        None => {
            let mut text = String::new();
            text.try_reserve(32).map_err(|_| Error::NoMemory)?;
            core::fmt::Write::write_fmt(&mut text, format_args!("{}", paperpaw_sdk::SDK_VERSION))
                .map_err(|_| Error::NoMemory)?;
            text
        }
    };
    let version = facts.version.as_deref().unwrap_or(&own);
    let version = native_model::shown_version(version, |release, number| {
        messages.format(
            host,
            "about.version.beta",
            &[
                Arg::Text {
                    name: "version",
                    value: release,
                },
                Arg::Text {
                    name: "number",
                    value: number,
                },
            ],
        )
    })?;
    let native_value = match facts.native {
        Some((major, draft)) => messages.format(
            host,
            "about.native.value",
            &[
                Arg::Int {
                    name: "major",
                    value: i64::from(major),
                },
                Arg::Int {
                    name: "draft",
                    value: i64::from(draft),
                },
            ],
        )?,
        None => String::new(),
    };
    Ok(Texts {
        version,
        summary: messages.copy(host, "about.summary")?,
        source: messages.copy(host, "about.source")?,
        plugins: messages.copy(host, "about.plugins.title")?,
        native: messages.copy(host, "about.native.title")?,
        native_value,
        native_note: messages.copy(host, "about.native.note")?,
        licences: messages.copy(host, "about.licences")?,
        licences_help: messages.copy(host, "about.licences.help")?,
    })
}
