//! A sample PaperPaw plugin to start from: everything a plugin usually
//! needs, small. See `docs/plugins.md` for the walk through it.
//!
//! - an **action**, *Hello*, listed in the tools menu with an icon;
//! - a **screen** it opens, usable by touch and by buttons alike;
//! - a **setting**, the name to greet, listed in the settings;
//! - an **event** it listens to: books opened, counted;
//! - **translations**, English and German.

#![no_std]

extern crate alloc;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::rc::Rc;
use core::cell::Cell;

use paperpaw_core::{Items, Plugin};
use paperpaw_sdk::builtin::{actions, events, groups, menus};
use paperpaw_sdk::setting::{Setting, Text};
use paperpaw_sdk::ui::{self, UiStyle};
use paperpaw_sdk::{
    ActionSpec, Arg, Canvas, Command, Cx, Env, Gesture, GestureKind, Handled, Id, Manifest,
    MenuEntry, Params, Rect, RegisterError, Screen, ScreenSpec, SettingSpec, Version,
};

/// Ids start with the plugin's id: `hello.…`.
pub const PLUGIN: Id = Id::new_static("hello");
pub const OPEN: Id = Id::new_static("hello.open");
pub const SCREEN: Id = Id::new_static("hello.screen");
pub const NAME: Setting<Text> = Setting::new("hello.name");

/// The plugin. State shared between its parts lives in `Rc`s it hands to
/// them when it registers.
#[derive(Debug, Default)]
pub struct Hello;

/// How it works, by language (see `Manifest::how`).
static HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/en.txt")),
    ("de", include_str!("../how/de.txt")),
];

impl Plugin for Hello {
    fn manifest(&self) -> Manifest {
        Manifest::new(PLUGIN, "Hello", Version::new(0, 1, 0))
            .description("A sample plugin to start from.")
            .how(&HOW)
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Items) -> Result<(), RegisterError> {
        // Books opened since the start, counted by the event below and
        // shown by the screen.
        let opened = Rc::new(Cell::new(0u32));

        // The action: what a menu entry, a button or a gesture runs.
        r.add_action(ActionSpec::new(OPEN, "hello.title").handler(|_, _, out| {
            out.push(Command::OpenScreen(SCREEN, Params::new()));
        }))?;
        r.add_menu_entry(MenuEntry::new(menus::TOOLS, OPEN).order(90))?;
        r.add_icon(OPEN, Cow::Borrowed("star"))?;

        // The screen, made each time it opens.
        let count = opened.clone();
        r.add_screen(ScreenSpec::new(
            SCREEN,
            "hello.title",
            Box::new(move |env, _| Box::new(HelloScreen::new(env, count.get()))),
        ))?;

        // The setting: shown in the settings, saved by the kernel.
        r.add_setting(
            SettingSpec::text(NAME.static_str(), "hello.setting.name", "", groups::OTHER)
                .description("hello.setting.name.help"),
        )?;

        // The event: every reader emits it when a book opens.
        r.on(events::BOOK_OPENED, move |_, _reading, _| {
            opened.set(opened.get() + 1);
        })?;

        r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
        Ok(())
    }
}

/// The greeting, under a header with a back arrow.
struct HelloScreen {
    style: UiStyle,
    header: ui::Header,
    opened: u32,
}

impl HelloScreen {
    fn new(env: &Env<'_>, opened: u32) -> Self {
        let style = UiStyle::new(env);
        Self {
            header: ui::Header::new(env, &style, true),
            style,
            opened,
        }
    }

    fn greeting(&self, env: &Env<'_>) -> alloc::string::String {
        let name = match env.settings().read(&NAME).trim() {
            "" => env.i18n().tr("hello.someone"),
            name => name,
        };
        let count = i64::from(self.opened);
        env.i18n().plural(
            "hello.greeting",
            count,
            &[("name", Arg::Str(name)), ("count", Arg::Int(count))],
        )
    }
}

impl Screen for HelloScreen {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        // What the screen draws with: canvas, environment and style.
        let mut ui = ui::Ui::new(canvas, env, &self.style);
        let title = env.i18n().tr("hello.title");
        self.header.draw(&mut ui, title, None, None);
        let display = env.device().display;
        let top = self.header.area().height;
        // A header taller than a tiny screen leaves no room, not an
        // overflow.
        let area = Rect::new(0, top, display.width, display.height.saturating_sub(top));
        ui.message(area, &self.greeting(env));
    }

    /// Buttons: *confirm* or *back* close it; other actions (e.g. the
    /// menu button) go on to their global handlers.
    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        if *action == actions::CONFIRM || *action == actions::BACK {
            cx.command(Command::Back);
            return Handled::Yes;
        }
        Handled::No
    }

    /// Touch: the header's back arrow, or a tap anywhere else, closes it.
    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if gesture.kind != GestureKind::Tap {
            return Handled::No;
        }
        if let Some(action) = self.header.tap(cx.env(), &self.style, gesture.start, None) {
            cx.command(Command::run(action));
        } else {
            cx.command(Command::Back);
        }
        Handled::Yes
    }
}
