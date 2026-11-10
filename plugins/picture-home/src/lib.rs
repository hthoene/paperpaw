//! The picture home screen: your own picture behind the widgets, pages of
//! widgets and tools placed anywhere in a grid, and a status bar that is
//! yours to configure. A store plugin: while it is on, it is the home
//! screen (`core.home`); switched off, the built-in home screen is back,
//! with its own pages.
//!
//! - **Picture**: chosen from a folder on the card; fill, whole, stretch,
//!   original size or tiled, in one of nine places, zoomed, paler; made
//!   once into a screen-sized gray file (`picture`) and drawn from it.
//! - **Widgets**: any plugin's, on plates over the picture, in the grid
//!   the built-in home screen uses (`paperpaw_sdk::widgets::grid`): drag
//!   to move, by the corner to size, tap for the options.
//! - **Bars**: the status bar (top or bottom, full or compact, on a plate
//!   of five looks or on the picture) and the row of hints below (on the
//!   X4), each always, never, or for a few seconds after a press; the press
//!   that shows them does nothing else.

#![no_std]

extern crate alloc;
#[cfg(test)]
extern crate std;

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
#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;
mod add;
mod bar;
mod dashboard;
pub mod native_add;
pub mod native_add_screen;
pub use paperpaw_ui_kit::dashboard::arrange as native_arrange;
pub mod native_backdrop;
pub mod native_backdrop_host;
pub mod native_bar;
pub mod native_dash;
pub mod native_dashboard;
pub use paperpaw_ui_kit::dashboard::focus as native_focus;
pub use paperpaw_ui_kit::dashboard::grid as native_grid;
pub mod native_measure;
pub mod native_pages;
pub mod native_pick;
pub mod native_picture;
pub mod native_plugin;
pub mod native_shown;
pub mod native_source;
pub mod native_stage;
mod orient;
mod pages;
mod pick;
mod picture;
mod settings;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;

use paperpaw_sdk::builtin::slots;
use paperpaw_sdk::{
    Env, FixedScreen, Id, Manifest, Plugin, Priority, RegisterError, Registrar, SettingGroupSpec,
    widgets,
};

pub const PLUGIN: Id = Id::new_static("picture-home");
/// The home screen.
pub const SCREEN: Id = Id::new_static("picture-home.dashboard");
/// Its settings' group (Settings, Customise, Picture home screen).
pub const GROUP: Id = Id::new_static("picture-home.group");
/// Adding a widget or tool to a page.
pub(crate) const ADD_SCREEN: Id = Id::new_static("picture-home.add");
/// Choosing the picture.
pub(crate) const PICK_SCREEN: Id = Id::new_static("picture-home.pick");

/// How it works, by language (see `Manifest::how`).
static HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/en.txt")),
    ("de", include_str!("../how/de.txt")),
];

static SCREENS: [FixedScreen; 3] = [
    FixedScreen::new(SCREEN, "picture-home.title", |env, params| {
        Box::new(dashboard::Dashboard::new(env, params))
    }),
    FixedScreen::new(ADD_SCREEN, "picture-home.add", |env, params| {
        Box::new(add::AddScreen::new(env, params))
    }),
    FixedScreen::new(PICK_SCREEN, "picture-home.setting.picture", |env, _| {
        Box::new(pick::Picker::new(env))
    }),
];

/// The pages as a place of widgets in a grid.
const PLACE: widgets::WidgetPlace = widgets::WidgetPlace::new(
    Id::new_static("picture-home.pages"),
    "picture-home.title",
    settings::PAGE_SETTINGS,
    widgets::PlaceKind::Grid,
);

/// The file a plugin keeps in its folder.
pub(crate) fn data(name: &str) -> String {
    format!("{}/{name}", Env::plugin_dir(PLUGIN.as_str()))
}

/// The picture home screen plugin.
#[derive(Debug, Default)]
pub struct PictureHome;

impl Plugin for PictureHome {
    fn manifest(&self) -> Manifest {
        Manifest::new(PLUGIN, "Picture home screen", paperpaw_sdk::package_version!())
            .description(
                "A home screen with your own picture behind the widgets, and a status bar you configure.",
            )
            .how(&HOW)
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        // Its settings on a page of their own (Settings, Customise).
        r.add_setting_group({
            static SPEC: SettingGroupSpec =
                SettingGroupSpec::new(GROUP, "picture-home.title").order(221);
            &SPEC
        })?;
        r.add_icon(GROUP, Cow::Borrowed("layout-grid"))?;
        r.add_settings(&settings::SETTINGS)?;
        r.set_menu_need(settings::FOCUS_AT_ONCE, paperpaw_sdk::MenuNeed::NoTouch)?;
        r.add_widget_place(PLACE)?;
        r.add_status_place(bar::PLACE)?;
        r.add_screens(&SCREENS)?;
        // The built-in home screen's keys and its *Edit home screen* entry
        // reach this one, which is bound to the same slot.
        r.bind_slot(slots::HOME, SCREEN, Priority::PREFERRED)?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
        Ok(())
    }
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
}
