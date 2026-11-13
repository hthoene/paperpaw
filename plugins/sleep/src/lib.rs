//! The sleep screen (slot `core.sleep`): what the screen shows while the
//! device sleeps, e-ink keeping it without power.
//!
//! Setting `sleep.show`:
//!
//! - *Sleeping cat* (the default): PaperPaw's drawing of a cat asleep,
//!   in the middle of the screen on white (see [`cat`]);
//! - *Cover*: the cover of the book read last;
//! - *Picture*: the pictures (JPEG, PNG) of a folder (`sleep.folder`,
//!   `sleep` at first, as on CrossPoint), one after the other;
//! - *Message*: PaperPaw's logo and "Sleeping" (the logo as set,
//!   see [`paperpaw_sdk::logo`]);
//! - *Widgets*: widgets as on the home screen (setting `sleep.widgets`, a
//!   place of widgets above each other, edited on the slot
//!   `core.edit-widgets`), e.g. the book being read and today's reading,
//!   in the middle of the screen;
//! - *Page*: the screen stays as it is;
//! - *Page with a picture*: the page stays, a picture or a widget over
//!   it, its clear parts showing the page (see [`overlay`]);
//! - a mode of another plugin: it offers a screen of its own among the
//!   choices (`Registrar::add_choice(sleep::SHOW, "my.sleep-screen")`, the
//!   value the screen's id and the translation key of its name), which
//!   the sleep screen shows in its place, sleeping as it does.
//!
//! Without a cover or a picture the cat shows. Where the device is
//! off while it sleeps (the X4), this is also what shows as it starts.
//! Asleep, the screen takes every button but *Sleep* and *Home*. It
//! closes when the device wakes, wherever it is: what opened above it as
//! the device woke (a lock, a review of flash cards) stays. Only where PaperPaw
//! shows the sleep screen itself (the X4, not Android): elsewhere its
//! settings are hidden.

#![no_std]

extern crate alloc;

#[cfg(test)]
extern crate std;
/// Test-only: refuses chosen allocations to prove the fallible paths.
#[cfg(test)]
#[path = "../../test-support/fail_alloc.rs"]
mod fail_alloc;
#[cfg(test)]
#[global_allocator]
static ALLOC: fail_alloc::Failing = fail_alloc::Failing;

#[path = "../declarations.rs"]
#[allow(
    dead_code,
    reason = "the build script and the declaration test also use the builders"
)]
mod declarations;
mod native_entry;
/// The picker of what goes over the page, without the host.
pub mod native_pick;
mod native_picker;
/// The interface-7 plugin.
pub mod native_plugin;
mod native_screen;
pub mod native_sleep;
mod native_stack;
pub mod overlay;
#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;

/// The settings the plugin declares, made by its build script from the same
/// declarations as a table.
#[cfg(feature = "linked")]
mod native_settings {
    use paperpaw_sdk::{Id, SettingSpec};
    include!(concat!(env!("OUT_DIR"), "/sleep_settings.rs"));
}

/// The settings the plugin declares as a table kept in the program (as Beta 3
/// kept them), for a linked build to use in place of a copy on the heap of
/// each (`Kernel::register_linked`). Only a linked plugin has it.
#[cfg(feature = "linked")]
pub static NATIVE_SETTINGS: &[paperpaw_sdk::SettingSpec] = &native_settings::SETTINGS;

/// The setting groups the plugin declares, as a table kept in the program
/// like [`NATIVE_SETTINGS`].
#[cfg(feature = "linked")]
pub static NATIVE_GROUPS: &[paperpaw_sdk::SettingGroupSpec] = &native_settings::GROUPS;

/// The full ids the plugin declares, sorted, as a table kept in the program
/// like [`NATIVE_SETTINGS`] (the kernel takes them in place of copies).
#[cfg(feature = "linked")]
pub static NATIVE_IDS: &[paperpaw_sdk::Id] = &native_settings::IDS;

/// The id and title of each screen and action the plugin declares, as a table
/// kept in the program like [`NATIVE_SETTINGS`].
#[cfg(feature = "linked")]
pub static NATIVE_TITLES: &[paperpaw_sdk::TitleEntry] = &native_settings::TITLES;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{actions, events, slots};
use paperpaw_sdk::document::ImageRef;
use paperpaw_sdk::library;
use paperpaw_sdk::logo::Logo;
use paperpaw_sdk::semantics::Role;
use paperpaw_sdk::setting::{Setting, Text};
use paperpaw_sdk::ui::{self, UiStyle};
use paperpaw_sdk::widgets::{PlaceKind, WidgetPlace, WidgetStack};
use paperpaw_sdk::{
    Canvas, Color, Command, Cx, Env, FixedScreen, Handled, Id, Manifest, MenuNeed, Params, Plugin,
    Priority, Rect, RefreshMode, RegisterError, Registrar, Screen, SettingSpec, ShowsThrough,
};

pub const PLUGIN: Id = Id::new_static("sleep");
/// Its settings' group (Settings, Device, Sleep screen).
pub const GROUP: Id = Id::new_static("sleep.group");
/// What the sleep screen shows: one of [`CAT`], [`COVER`], [`PICTURE`],
/// [`OVERLAY`], [`WIDGETS`], [`MESSAGE`], [`PAGE`], or a screen another
/// plugin offers ([`paperpaw_sdk::Registrar::add_choice`]).
pub const SHOW: Setting<Text> = Setting::new("sleep.show");
pub const CAT: &str = "sleep.show.cat";
pub const COVER: &str = "sleep.show.cover";
pub const PICTURE: &str = "sleep.show.picture";
pub const MESSAGE: &str = "sleep.show.message";
pub const PAGE: &str = "sleep.show.page";
pub const WIDGETS: &str = "sleep.show.widgets";
/// The page, a picture over it (see [`overlay`]).
pub const OVERLAY: &str = "sleep.show.overlay";
/// The widgets shown with [`WIDGETS`], ids separated by commas.
pub const WIDGET_LIST: Setting<Text> = Setting::new("sleep.widgets");
/// The folder of the pictures.
pub const FOLDER: Setting<Text> = Setting::new("sleep.folder");
const SCREEN: Id = Id::new_static("sleep.screen");
/// Parameter of the slot `core.sleep` (set to `1`): the device is switched
/// off after this screen, not put to sleep (the power menu's *Switch off*).
/// Modes that leave the page as it is show the message "Switched off"
/// instead, or it would look as if the device were on.
pub const OFF: &str = "off";

pub(crate) fn data(name: &str) -> String {
    format!("{}/{name}", Env::plugin_dir(PLUGIN.as_str()))
}

/// The sleep screen plugin.
#[derive(Debug, Default)]
pub struct Sleep;

/// The sleep screen's part of the display settings, its folder and its
/// widgets each chosen on a screen of their own.
static IN_SLEEP: paperpaw_sdk::SettingExtra = paperpaw_sdk::SettingExtra::new();
static FOLDER_EXTRA: paperpaw_sdk::SettingExtra =
    paperpaw_sdk::SettingExtra::new().editor_slot(paperpaw_sdk::builtin::slots::CHOOSE_FOLDER);
static WIDGETS_EXTRA: paperpaw_sdk::SettingExtra =
    paperpaw_sdk::SettingExtra::new().editor_slot(slots::EDIT_WIDGETS);
/// Where its widgets show, for editors and widgets that ask.
const PLACE: WidgetPlace = WidgetPlace::new(
    Id::new_static("sleep.widgets"),
    "sleep.title",
    PLACE_SETTINGS,
    PlaceKind::Stack,
)
.shown_if(Id::new_static(SHOW.static_str()), WIDGETS);
const PLACE_SETTINGS: &[Id] = &[Id::new_static(WIDGET_LIST.static_str())];
/// The widget shown over the page, where one is (see [`overlay`]).
const OVERLAY_PLACE: WidgetPlace = WidgetPlace::new(
    Id::new_static("sleep.overlay"),
    "sleep.title",
    OVERLAY_SETTINGS,
    PlaceKind::Stack,
)
.shown_if(Id::new_static(SHOW.static_str()), OVERLAY);
const OVERLAY_SETTINGS: &[Id] = &[Id::new_static(overlay::WIDGET.static_str())];
static PICKED_EXTRA: paperpaw_sdk::SettingExtra =
    paperpaw_sdk::SettingExtra::new().editor(overlay::PICK);
static PERCENT: paperpaw_sdk::SettingExtra = paperpaw_sdk::SettingExtra::new()
    .unit("core.unit.percent")
    .step(5)
    .slider();

/// What the sleep screen shows, the folder of its pictures, its widgets
/// and its picture over the page.
static SETTINGS: [SettingSpec; 9] = [
    SettingSpec::choice(
        SHOW.static_str(),
        "sleep.setting.show",
        &[CAT, COVER, PICTURE, OVERLAY, WIDGETS, MESSAGE, PAGE],
        CAT,
        GROUP,
    )
    .with_extra(&IN_SLEEP)
    .description("sleep.setting.show.help"),
    SettingSpec::text(FOLDER.static_str(), "sleep.setting.folder", "sleep", GROUP)
        .with_extra(&FOLDER_EXTRA)
        .description("sleep.setting.folder.help"),
    SettingSpec::new(
        paperpaw_sdk::Id::new_static(WIDGET_LIST.static_str()),
        "sleep.setting.widgets",
        paperpaw_sdk::SettingKind::List,
        paperpaw_sdk::SettingValue::Text(Cow::Borrowed(
            "home.date, home.continue, stats.widget.today, home.recent",
        )),
        GROUP,
    )
    .with_extra(&WIDGETS_EXTRA)
    .description("sleep.setting.widgets.help"),
    SettingSpec::text(
        overlay::PICTURE.static_str(),
        "sleep.setting.overlay-picture",
        "",
        GROUP,
    )
    .with_extra(&PICKED_EXTRA)
    .description("sleep.setting.overlay-picture.help"),
    SettingSpec::text(
        overlay::WIDGET.static_str(),
        "sleep.setting.overlay-widget",
        "",
        GROUP,
    )
    .with_extra(&PICKED_EXTRA)
    .description("sleep.setting.overlay-widget.help"),
    SettingSpec::choice(
        overlay::POSITION.static_str(),
        "sleep.setting.overlay-position",
        &overlay::POSITIONS,
        overlay::DEFAULT_POSITION,
        GROUP,
    )
    .with_extra(&IN_SLEEP)
    .description("sleep.setting.overlay-position.help"),
    SettingSpec::number(
        overlay::SIZE.static_str(),
        "sleep.setting.overlay-size",
        10,
        100,
        overlay::DEFAULT_SIZE,
        GROUP,
    )
    .with_extra(&PERCENT)
    .description("sleep.setting.overlay-size.help"),
    SettingSpec::number(
        overlay::SEE_THROUGH.static_str(),
        "sleep.setting.overlay-see-through",
        0,
        90,
        0,
        GROUP,
    )
    .with_extra(&PERCENT)
    .description("sleep.setting.overlay-see-through.help"),
    SettingSpec::choice(
        overlay::USED.static_str(),
        "sleep.setting.overlay-when",
        &overlay::WHEN,
        overlay::WHEN[0],
        GROUP,
    )
    .with_extra(&IN_SLEEP)
    .description("sleep.setting.overlay-when.help"),
];

/// How it works, by language (see `Manifest::how`).
static HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/en.txt")),
    ("de", include_str!("../how/de.txt")),
];

static SCREENS: [FixedScreen; 2] = [
    FixedScreen::new(
        overlay::PICK,
        "sleep.setting.overlay-picture",
        |env, params| Box::new(overlay::Picker::new(env, params)),
    ),
    FixedScreen::new(SCREEN, "sleep.title", |env, params| {
        Box::new(SleepScreen::new(env, params.get(OFF) == Some("1")))
    })
    .instances(paperpaw_sdk::Instances::SINGLE),
];

impl Plugin for Sleep {
    fn manifest(&self) -> Manifest {
        Manifest::new(PLUGIN, "Sleep screen", paperpaw_sdk::package_version!())
            .description(
                "Shows a sleeping cat, the book's cover, your pictures or a message while the device sleeps.",
            )
            .how(&HOW)
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        // Its settings on a page of their own (Settings, Device).
        r.add_setting_group({
            static SPEC: paperpaw_sdk::SettingGroupSpec =
                paperpaw_sdk::SettingGroupSpec::new(GROUP, "sleep.title").order(62);
            &SPEC
        })?;
        r.add_icon(GROUP, Cow::Borrowed("moon"))?;
        r.set_menu_need(GROUP, MenuNeed::Sleep)?;
        r.add_settings(&SETTINGS)?;
        let items: [Id; 9] = [
            SHOW.into(),
            FOLDER.into(),
            WIDGET_LIST.into(),
            overlay::PICTURE.into(),
            overlay::WIDGET.into(),
            overlay::POSITION.into(),
            overlay::SIZE.into(),
            overlay::SEE_THROUGH.into(),
            overlay::USED.into(),
        ];
        for item in items {
            r.set_menu_need(item, MenuNeed::Sleep)?;
        }
        r.add_screens(&SCREENS)?;
        // Asleep, the buttons do nothing but wake the device.
        r.set_screen_hints(SCREEN, paperpaw_sdk::HintVisibility::Hidden)?;
        r.bind_slot(slots::SLEEP, SCREEN, Priority::DEFAULT)?;
        r.add_widget_place(PLACE)?;
        r.add_widget_place(OVERLAY_PLACE)?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
        Ok(())
    }
}

/// The cover of the book read last, as the library knows it: the book is
/// opened for it only once (memory is short while a book is open).
fn cover(env: &Env<'_>) -> Option<ImageRef> {
    let path = library::opened_books(env.storage()).into_iter().next()?;
    library::indexed(env, &path)
        .cover
        .filter(|image| env.storage().exists(&image.path))
}

/// PaperPaw's sleeping cat: made for PaperPaw (see `NOTICE`), shipped
/// in the plugin, decoded once for the screen's size into the plugin's
/// folder (a few rows at a time, never whole in memory) and drawn from
/// there. As wide as the screen leaves inside the interface's margins,
/// never larger than it is drawn.
pub mod cat {
    use alloc::format;

    use paperpaw_sdk::Env;
    use paperpaw_sdk::document::ImageRef;
    use paperpaw_sdk::image::{Fit, ImageFile};
    use paperpaw_sdk::ui::UiStyle;

    /// The drawing: 760 × 501 pixels, 16 grays stored in 4 bits each
    /// (#258), cropped to the cat.
    pub const PICTURE: &[u8] = include_bytes!("../pictures/sleeping-cat.png");

    /// The room around the cat on each side, in pixels.
    pub fn margin(env: &Env<'_>) -> u16 {
        UiStyle::new(env).margin()
    }

    /// The cat decoded for this screen.
    pub fn image(env: &Env<'_>) -> Option<ImageRef> {
        let display = env.device().display;
        let room = 2 * margin(env);
        let side = display
            .width
            .saturating_sub(room)
            .min(display.height.saturating_sub(room))
            .max(1);
        // A file for each size: decoded once, again only where the screen
        // turns or its margins change.
        let file = super::data(&format!("cat-{side}.gray"));
        if let Some(kept) = ImageFile::open(env.storage(), &file) {
            return Some(ImageRef {
                path: kept.path,
                width: kept.width,
                height: kept.height,
            });
        }
        let fit = Fit::side(u32::from(side));
        paperpaw_sdk::image::picture_from_bytes(env.storage(), PICTURE, &file, fit, false)
    }
}

/// The next picture of the folder, decoded to fit the screen.
fn picture(env: &Env<'_>) -> Option<ImageRef> {
    let path = next_from_folder(env)?;
    let display = env.device().display;
    let fit = paperpaw_sdk::image::Fit::screen(display.width.into(), display.height.into());
    // In colour on colour screens (ADR 0015).
    paperpaw_sdk::image::picture_from_file(
        env.storage(),
        &path,
        &data("picture.gray"),
        fit,
        env.device().display.format.is_color(),
    )
}

/// The path of the next picture of the pictures folder, one after the
/// other from where the last sleep left off.
pub(crate) fn next_from_folder(env: &Env<'_>) -> Option<String> {
    let folder = env
        .settings()
        .text(FOLDER.as_str())
        .trim_matches('/')
        .to_string();
    let mut pictures = Vec::new();
    let _ = env.storage().list(&folder, &mut |entry| {
        let name = entry.name.to_lowercase();
        if !entry.is_dir
            && !entry.name.starts_with('.')
            && [".jpg", ".jpeg", ".png", ".gif"]
                .iter()
                .any(|e| name.ends_with(e))
        {
            pictures.push(entry.name.to_string());
        }
    });
    if pictures.is_empty() {
        return None;
    }
    paperpaw_sdk::sort::sort(&mut pictures);
    // One after the other, from where the last sleep left off.
    let next = next_picture(env.storage());
    let name = &pictures[next % pictures.len()];
    let _ = env.storage().write(
        &data("next.txt"),
        format!("{}", next.wrapping_add(1)).as_bytes(),
    );
    Some(format!("{folder}/{name}"))
}

/// What the screen shows while the device sleeps.
struct SleepScreen {
    show: String,
    image: Option<ImageRef>,
    /// The image is the cat: on white, not on black as a cover.
    cat: bool,
    /// What goes over the page, where, how opaque.
    over_page: Option<(overlay::Over, Rect, u8)>,
    /// Another plugin's screen shown in its place (its mode).
    shown: Option<Box<dyn Screen>>,
    /// The widgets shown, where they go.
    widgets: WidgetStack,
    /// The logo above the message, when the message shows.
    logo: Option<Logo>,
    /// Whether the device woke since the screen opened.
    woke: bool,
    /// The device is switched off, not asleep (see [`OFF`]).
    off: bool,
}

impl SleepScreen {
    fn new(env: &Env<'_>, off: bool) -> Self {
        let mut show = env.settings().read(&SHOW).to_string();
        // Off, the page staying would look like a device that is on.
        if off && matches!(show.as_str(), PAGE | OVERLAY) {
            show = MESSAGE.to_string();
        }
        // What goes over the page where it is used now, and there is
        // something; else the cover. Why not is kept for the picker.
        let mut over_page = None;
        if show == OVERLAY {
            if overlay::used(env) {
                match overlay::over(env) {
                    Ok(mut over) => {
                        let area = overlay::place(env, &mut over);
                        over_page = Some((over, area, overlay::opacity(env)));
                        overlay::remember_failure(env, None);
                    }
                    Err(why) => overlay::remember_failure(env, Some(&why)),
                }
            }
            if over_page.is_none() {
                show = COVER.to_string();
            }
        }
        // A mode of another plugin: its screen.
        let shown = (!BUILT_IN.contains(&show.as_str()))
            .then(|| Id::parse(&show).ok())
            .flatten()
            .and_then(|id| env.make_screen(&id, &Params::new()));
        let image = match show.as_str() {
            _ if shown.is_some() => None,
            OVERLAY => None,
            PICTURE => picture(env),
            MESSAGE | PAGE | WIDGETS => None,
            CAT => None,
            _ => cover(env),
        };
        // The cat, chosen, or where there is no cover or no picture (also
        // for a mode no plugin offers any more, which shows the cover).
        let wants_cat = shown.is_none() && !matches!(show.as_str(), MESSAGE | PAGE | WIDGETS);
        let (image, cat) = match image {
            None if wants_cat => (cat::image(env), true),
            image => (image, false),
        };
        let display = env.device().display;
        let screen = Rect::new(0, 0, display.width, display.height);
        let list = if show == WIDGETS {
            env.settings().read(&WIDGET_LIST)
        } else {
            ""
        };
        let widgets = WidgetStack::new(env, list, screen);
        let message = image.is_none()
            && widgets.is_empty()
            && show != PAGE
            && over_page.is_none()
            && shown.is_none();
        let logo = message.then(|| {
            let side = logo_side(env);
            Logo::read(env, side, side)
        });
        Self {
            show,
            image,
            cat,
            over_page,
            shown,
            widgets,
            logo,
            woke: false,
            off,
        }
    }
}

/// The revision of the sleep screen ([`Screen::revision`]) where it shows
/// `shown` in its place: no higher than that one's, so that it is asked
/// only what it can pass on; this SDK's where it shows its own.
fn revision_of(shown: Option<&dyn Screen>) -> u16 {
    shown.map_or(
        paperpaw_sdk::SCREEN_REVISION,
        paperpaw_sdk::revision_holding,
    )
}

impl Screen for SleepScreen {
    fn revision(&self) -> u16 {
        revision_of(self.shown.as_deref())
    }

    fn region(&self, env: &Env<'_>) -> Option<Rect> {
        if let Some(shown) = &self.shown {
            return shown.region(env);
        }
        // The page stays: nothing is drawn over it, or the picture over
        // all of it, the page drawn below (`shows_through`).
        let display = env.device().display;
        match self.show.as_str() {
            PAGE => Some(Rect::new(0, 0, 0, 0)),
            OVERLAY => Some(Rect::new(0, 0, display.width, display.height)),
            _ => None,
        }
    }

    fn shows_through(&self) -> ShowsThrough {
        if let Some(shown) = &self.shown {
            return shown.shows_through();
        }
        // The page, not a menu left open over it.
        if self.over_page.is_some() {
            ShowsThrough::FullScreen
        } else {
            ShowsThrough::Nothing
        }
    }

    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        if let Some(shown) = &mut self.shown {
            return shown.draw(canvas, env);
        }
        if self.show == PAGE {
            return;
        }
        if let Some((over, area, opacity)) = &self.over_page {
            overlay::draw(canvas, env, over, *area, *opacity);
            return;
        }
        let display = env.device().display;
        let screen = Rect::new(0, 0, display.width, display.height);
        if !self.widgets.is_empty() {
            self.widgets.draw(canvas, env);
            return;
        }
        match &self.image {
            Some(image) if self.cat => {
                // In the middle, on white: as the drawing was made.
                canvas.fill(screen, Color::WHITE);
                let room = 2 * cat::margin(env);
                let (w, h) = ui::fit_image(
                    image,
                    display.width.saturating_sub(room),
                    display.height.saturating_sub(room),
                );
                let area = Rect::new((display.width - w) / 2, (display.height - h) / 2, w, h);
                ui::draw_image(canvas, env.storage(), image, area);
                let what = env.i18n().tr("sleep.show.cat");
                canvas.describe(Role::Image, what, area);
            }
            Some(image) => {
                // As large as fits, in the middle, on black.
                canvas.fill(screen, Color::BLACK);
                let (w, h) = ui::fit_image(image, display.width, display.height);
                let area = Rect::new((display.width - w) / 2, (display.height - h) / 2, w, h);
                ui::draw_image(canvas, env.storage(), image, area);
            }
            None => {
                let style = UiStyle::new(env);
                let mut ui = ui::Ui::new(canvas, env, &style);
                let text = env.i18n().tr(if self.off {
                    "sleep.off"
                } else {
                    "sleep.sleeping"
                });
                let Some(logo) = &self.logo else {
                    ui.message(screen, text);
                    return;
                };
                // The logo a little above the middle, the message below it.
                let side = logo_side(env);
                let (_, h) = logo.fit(side, side);
                let line = paperpaw_sdk::widgets::line_height(env, style.text());
                let gap = style.gap() * 2;
                // Off, a second line tells how to switch on.
                let lines = if self.off { 2 * line + gap / 2 } else { line };
                let top = display.height.saturating_sub(h + gap + lines) / 2;
                let area = Rect::new(0, top, display.width, h);
                logo.draw(ui.canvas(), env.storage(), area, Color::BLACK);
                let below = Rect::new(0, top + h + gap, display.width, line);
                ui.message(below, text);
                if self.off {
                    let hint = Rect::new(0, below.y + line + gap / 2, display.width, line);
                    ui.message(hint, env.i18n().tr("sleep.off-hint"));
                }
            }
        }
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        // Shown again after waking, once what opened above it closed: it
        // goes too, so that it never keeps an awake device to itself.
        if self.woke {
            cx.command(Command::Back);
            return;
        }
        if self.show != PAGE {
            cx.request_refresh(RefreshMode::Full);
        }
        if let Some(shown) = &mut self.shown {
            shown.on_show(cx);
        }
    }

    fn on_hide(&mut self) {
        if let Some(shown) = &mut self.shown {
            shown.on_hide();
        }
    }

    fn on_timer(&mut self, cx: &mut Cx<'_, '_>) {
        if let Some(shown) = &mut self.shown {
            shown.on_timer(cx);
        }
    }

    /// A cover or a picture (also over the page): in grays where the
    /// panel can.
    fn grays(&self) -> bool {
        if let Some(shown) = &self.shown {
            return shown.grays();
        }
        let picture_over = matches!(self.over_page, Some((overlay::Over::Picture(_), ..)));
        picture_over || (self.show != PAGE && self.image.is_some() && self.widgets.is_empty())
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        // Awake and still here (see `on_show`): gone at the next press.
        if self.woke {
            cx.command(Command::Back);
            return Handled::Yes;
        }
        // Nothing to do while asleep but sleep again or go home.
        if *action == actions::SLEEP || *action == actions::HOME {
            Handled::No
        } else {
            Handled::Yes
        }
    }

    fn on_event(&mut self, event: &Id, params: &Params, cx: &mut Cx<'_, '_>) {
        if let Some(shown) = &mut self.shown {
            shown.on_event(event, params, cx);
        }
        if *event == events::RESUME {
            // Awake: gone, wherever it is. What opened above it as the
            // device woke (a lock, a review) stays where it is.
            self.woke = true;
            cx.close();
            if self.show != PAGE {
                cx.request_refresh(RefreshMode::Full);
            }
        }
    }
}

/// The modes of this plugin; others name another plugin's screen.
const BUILT_IN: [&str; 7] = [CAT, COVER, PICTURE, OVERLAY, WIDGETS, MESSAGE, PAGE];

/// The logo's size on the sleep screen: a third of the screen across.
fn logo_side(env: &Env<'_>) -> u16 {
    let display = env.device().display;
    display.width.min(display.height) / 3
}

/// Which picture comes next, counted on from the first (`next.txt`).
fn next_picture(storage: &dyn paperpaw_sdk::Storage) -> usize {
    paperpaw_sdk::read_file(storage, &data("next.txt"), 32)
        .ok()
        .and_then(|n| String::from_utf8(n).ok()?.trim().parse::<usize>().ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use paperpaw_i18n::{Catalog, check};

    #[test]
    fn translations_are_complete() {
        let en = Catalog::parse(include_str!("../i18n/en.ini")).unwrap();
        let de = Catalog::parse(include_str!("../i18n/de.ini")).unwrap();
        let issues = check::compare(&en, &de);
        assert!(issues.is_empty(), "{issues:?}");
    }

    /// A screen of revision 1, as one built before revision 2's items.
    struct Old;

    impl paperpaw_sdk::Screen for Old {
        fn revision(&self) -> u16 {
            1
        }

        fn draw(&mut self, _: &mut paperpaw_sdk::Canvas<'_>, _: &paperpaw_sdk::Env<'_>) {}

        fn on_action(
            &mut self,
            _: &paperpaw_sdk::Id,
            _: &mut paperpaw_sdk::Cx<'_, '_>,
        ) -> paperpaw_sdk::Handled {
            paperpaw_sdk::Handled::No
        }
    }

    /// Showing another plugin's screen in its place, it is no newer than
    /// that one; showing its own, of this SDK.
    #[test]
    fn the_sleep_screen_is_no_newer_than_the_screen_it_shows() {
        assert_eq!(super::revision_of(Some(&Old)), 1);
        assert_eq!(super::revision_of(None), paperpaw_sdk::SCREEN_REVISION);
    }

    /// What version 1 wrote stays readable (see `docs/data-files.md`).
    #[test]
    fn the_next_picture_of_version_1_is_read() {
        let storage = paperpaw_sdk::MemoryStorage::new().with(
            &super::data("next.txt"),
            include_str!("../testdata/next.txt.v1"),
        );
        assert_eq!(super::next_picture(&storage), 7);
    }
}
