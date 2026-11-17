//! The Android shell's own plugin: what only the platform can do, offered
//! through the same SDK as every other plugin.

use std::borrow::Cow;

use android_activity::AndroidApp;
use paperpaw_core::Items;
use paperpaw_sdk::builtin::{events, groups, menus, params};
use paperpaw_sdk::document::Document;
use paperpaw_sdk::{
    ActionSpec, Canvas, Command, Cx, Env, FormatSpec, Handled, Id, Manifest, MenuEntry, MenuNeed,
    Rect, RegisterError, SettingSpec, Version,
};

use crate::lights::Lights;
use paperpaw_sdk::setting::{Setting, Text};

pub const PLUGIN: Id = Id::new_static("android");
/// Asks for access to the shared storage, where the books are.
pub const ALLOW_STORAGE: Id = Id::new_static("android.allow-storage");
/// PDF books, rendered by the system (see `pdf`).
pub const PDF: Id = Id::new_static("android.pdf");
/// The voice that reads aloud: opens the system's speech settings.
pub const VOICE: Setting<Text> = Setting::new("android.voice");
const VOICE_SCREEN: Id = Id::new_static("android.voice-screen");
/// The settings for reading aloud, every reader's.
const ALOUD_SECTION: &str = paperpaw_sdk::builtin::group_sections::ALOUD;
/// Frontlight brightness (0 switches it off).
pub const FRONTLIGHT: Setting<i64> = Setting::new("android.frontlight");
/// Frontlight colour temperature (higher is warmer).
pub const WARMTH: Setting<i64> = Setting::new("android.warmth");
/// The home screen's shortcuts (see the `home` plugin).
const HOME_ACTIONS: Id = Id::new_static("home.actions");
/// The reader's panel of font and layout, every reader's.
const READER_PANEL: Id = paperpaw_sdk::builtin::menus::READER_PANEL;

/// The device's frontlight and its levels when PaperPaw started.
#[derive(Clone, Copy)]
pub struct Frontlight {
    pub lights: Lights,
    pub brightness: i32,
    pub warmth: i32,
}

/// What the plugin's handlers reach of the platform: they are plain
/// functions and hold nothing (#293). The kernel runs on one thread.
#[derive(Default)]
struct Platform {
    app: Option<AndroidApp>,
    lights: Option<Lights>,
    /// The light follows a level shown, not kept.
    following: bool,
}

std::thread_local! {
    static PLATFORM: std::cell::RefCell<Platform> = std::cell::RefCell::new(Platform::default());
}

/// The app, once registered.
fn platform_app() -> Option<AndroidApp> {
    PLATFORM.with(|platform| platform.borrow().app.clone())
}

/// The frontlight, where there is one.
fn platform_lights() -> Option<Lights> {
    PLATFORM.with(|platform| platform.borrow().lights)
}

/// The Android plugin.
pub struct AndroidPlugin {
    pub app: AndroidApp,
    /// PaperPaw can already read the shared storage.
    pub shared_storage: bool,
    pub frontlight: Option<Frontlight>,
    /// The screen's size in pixels, which PDF pages are rendered to fit.
    pub screen: (u32, u32),
}

impl AndroidPlugin {
    /// What [`AndroidPlugin::register`] registers under.
    pub fn manifest() -> Manifest {
        Manifest::new(PLUGIN, "Android", Version::new(0, 1, 0))
            .description("Connects PaperPaw to Android, e.g. to your book folders.")
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    /// Its actions, settings, format and texts (`KernelBuilder::items` with
    /// [`AndroidPlugin::manifest`]).
    pub fn register(&self, r: &mut Items) -> Result<(), RegisterError> {
        PLATFORM.with(|platform| {
            platform.borrow_mut().app = Some(self.app.clone());
        });
        r.add_action(
            ActionSpec::new(ALLOW_STORAGE, "android.allow-storage").handler(|_, _, _| {
                if let Some(app) = platform_app() {
                    crate::access::request(&app);
                }
            }),
        )?;
        if !self.shared_storage {
            // First on the home screen until access is granted.
            r.add_menu_entry(MenuEntry::new(HOME_ACTIONS, ALLOW_STORAGE))?;
        }
        if let Some(frontlight) = self.frontlight {
            self.register_frontlight(r, frontlight)?;
        }
        self.register_voice(r)?;
        let (app, screen) = (self.app.clone(), self.screen);
        r.add_format(FormatSpec::new(
            PDF,
            "android.pdf",
            vec![Cow::Borrowed("pdf")],
            Box::new(move |storage, path, _| {
                crate::pdf::PdfDocument::open(&app, storage, path, screen)
                    .map(|d| Box::new(d) as Box<dyn Document>)
            }),
        ))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
        Ok(())
    }
}

/// Opens the system's speech settings and closes at once: the editor of
/// [`VOICE`].
struct VoiceSettings {
    app: AndroidApp,
}

impl paperpaw_core::Layer for VoiceSettings {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}

    fn region(&self, _: &Env<'_>) -> Option<Rect> {
        Some(Rect::new(0, 0, 0, 0))
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        if !crate::speech::open_settings(&self.app) {
            let text = cx.env().i18n().tr("android.voice.unavailable").to_string();
            cx.command(Command::Notify(text));
        }
        cx.command(Command::Back);
    }

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }
}

/// The voice's part and editor: the system's speech settings.
static VOICE_EXTRA: paperpaw_sdk::SettingExtra = paperpaw_sdk::SettingExtra::new()
    .section(ALOUD_SECTION)
    .editor(VOICE_SCREEN);
/// Set with a slider.
static SLIDER: paperpaw_sdk::SettingExtra = paperpaw_sdk::SettingExtra::new().slider();

impl AndroidPlugin {
    /// The voice for reading aloud, with its other settings; it is
    /// chosen in the system's speech settings.
    fn register_voice(&self, r: &mut Items) -> Result<(), RegisterError> {
        r.add_setting(
            SettingSpec::text(VOICE.static_str(), "android.voice", "", groups::READING)
                .with_extra(&VOICE_EXTRA)
                .description("android.voice.help"),
        )?;
        r.set_menu_need(VOICE, MenuNeed::Speech)?;
        let app = self.app.clone();
        r.add_layer(paperpaw_core::LayerSpec::new(
            VOICE_SCREEN,
            "android.voice",
            Box::new(move |_, _| Box::new(VoiceSettings { app: app.clone() })),
        ))
    }

    /// Brightness and warmth as settings, also in the reader's panel and
    /// the quick panel. They follow the device: its levels at start, and every
    /// change goes to the device.
    fn register_frontlight(
        &self,
        r: &mut Items,
        frontlight: Frontlight,
    ) -> Result<(), RegisterError> {
        let lights = frontlight.lights;
        r.add_setting(
            SettingSpec::number(
                FRONTLIGHT.static_str(),
                "android.frontlight",
                0,
                i64::from(lights.max_brightness),
                i64::from(frontlight.brightness),
                groups::DISPLAY,
            )
            .description("android.frontlight.help")
            .with_extra(&SLIDER),
        )?;
        let mut items = vec![FRONTLIGHT];
        if lights.has_warmth() {
            r.add_setting(
                SettingSpec::number(
                    WARMTH.static_str(),
                    "android.warmth",
                    0,
                    i64::from(lights.max_warmth),
                    i64::from(frontlight.warmth),
                    groups::DISPLAY,
                )
                .description("android.warmth.help")
                .with_extra(&SLIDER),
            )?;
            items.push(WARMTH);
        }
        // In the reader's quick panel, first, and in the quick panel.
        for (n, item) in items.into_iter().enumerate() {
            for (menu, order) in [(READER_PANEL, -10), (menus::QUICK, 20)] {
                r.add_menu_entry(
                    MenuEntry::new(menu, item.clone().into()).order(order + n as i16),
                )?;
            }
        }
        r.add_icon(FRONTLIGHT, Cow::Borrowed("bulb"))?;
        r.add_icon(WARMTH, Cow::Borrowed("temperature"))?;
        PLATFORM.with(|platform| platform.borrow_mut().lights = Some(lights));
        r.subscribe(events::SETTING_CHANGED, |k, params| {
            let (Some(app), Some(lights)) = (platform_app(), platform_lights()) else {
                return;
            };
            let id = params.get(params::ID).unwrap_or_default();
            let level = k.settings().int(id) as i32;
            // Only real changes reach the device: taking over its
            // own levels on start never writes back.
            if id == FRONTLIGHT.as_str() && lights.brightness(&app) != level {
                lights.set_brightness(&app, level);
            } else if id == WARMTH.as_str() && lights.warmth(&app) != level {
                lights.set_warmth(&app, level);
            }
        })?;
        // A level shown before it is kept, as a slider moves: the light
        // follows it at once, and goes back to the level kept if it is not
        // kept. Other values shown (the text size) leave it alone, so as
        // not to undo what the system's own controls set meanwhile.
        r.subscribe(events::PREVIEW_CHANGED, |k, _| {
            let (Some(app), Some(lights)) = (platform_app(), platform_lights()) else {
                return;
            };
            let settings = k.settings();
            let shown =
                |id: &Setting<i64>| settings.get(id.as_str()) != settings.global(id.as_str());
            let light = shown(&FRONTLIGHT) || shown(&WARMTH);
            let following = PLATFORM
                .with(|platform| core::mem::replace(&mut platform.borrow_mut().following, light));
            if !following && !light {
                return;
            }
            let level = settings.read(&FRONTLIGHT) as i32;
            if lights.brightness(&app) != level {
                lights.set_brightness(&app, level);
            }
            let warmth = settings.read(&WARMTH) as i32;
            if lights.has_warmth() && lights.warmth(&app) != warmth {
                lights.set_warmth(&app, warmth);
            }
        })?;
        // The device's levels are the truth (they also change through
        // the system's own controls): take them over on start.
        r.subscribe(events::STARTED, |k, _| {
            let (Some(app), Some(lights)) = (platform_app(), platform_lights()) else {
                return;
            };
            let levels = [
                (FRONTLIGHT, lights.brightness(&app)),
                (WARMTH, lights.warmth(&app)),
            ];
            k.with_env(|env, out| {
                for (id, level) in levels {
                    let level = i64::from(level);
                    let known = env.settings().get(id.as_str()).is_some();
                    if known && env.settings().read(&id) != level {
                        out.push(Command::set(&id, level));
                    }
                }
            });
        })?;
        Ok(())
    }
}
