//! Settings in menus with the real kernel, by the buttons of an X4: the
//! reader's bar opens a setting's own editor and a number's settings page,
//! and choices of many open the settings plugin's list, or step on where
//! there is none, in the bar, the list and the panel alike.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::borrow::Cow;

use paperpaw_core::Kernel;
use paperpaw_platform::{ButtonState, InputEvent, Rotation};
use paperpaw_plugin_menu::Menu;
use paperpaw_plugin_settings::SettingsPlugin;
use paperpaw_sdk::builtin::{params, slots};
use paperpaw_sdk::{
    Canvas, Choices, Cx, DeviceInfo, DisplayInfo, Env, Handled, Id, InputCaps, Instant, Manifest,
    MemoryClass, MemoryStorage, MenuEntry, Params, PixelFormat, Plugin, RegisterError, Registrar,
    Screen, ScreenSpec, SettingGroupSpec, SettingKind, SettingSpec, SettingValue, Version,
};

/// An X4: buttons, no touch.
const X4: DeviceInfo = DeviceInfo::new(
    "test",
    "Test device",
    DisplayInfo::new(480, 800, 220, PixelFormat::Gray4, Rotation::Deg0, None),
    InputCaps::new(true, false, false, &[]),
    MemoryClass::Large,
);

/// The screen the menus open over, as a book.
const BASE: Id = Id::new_static("probe.base");
/// The own page of [`KIND`], as the lock's asks for its code.
const EDITOR: Id = Id::new_static("probe.editor");
const GROUP: Id = Id::new_static("probe.group");
/// A choice with an editor of its own, as `lock.kind`.
const KIND: Id = Id::new_static("probe.kind");
/// A choice of three.
const MANY: Id = Id::new_static("probe.many");
/// A number.
const SIZE: Id = Id::new_static("probe.size");
/// The bar's menu: [`KIND`], [`MANY`] and [`SIZE`].
const BAR_MENU: Id = Id::new_static("probe.bar");
/// [`MANY`] alone.
const CHOICES: Id = Id::new_static("probe.choices");

const MENU_SCREEN: Id = Id::new_static("menu.list");
const SETTINGS_SCREEN: Id = Id::new_static("settings.list");
const CHOOSE_SCREEN: Id = Id::new_static("settings.choose");

const OPTIONS: &[&str] = &["probe.a", "probe.b", "probe.c"];

struct Blank;

impl Screen for Blank {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }
}

/// Settings of each kind, in menus.
struct Probe;

impl Plugin for Probe {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("probe"), "Probe", Version::new(0, 1, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        for id in [BASE, EDITOR] {
            r.add_screen(ScreenSpec::new(
                id,
                "probe",
                Box::new(|_, _| Box::new(Blank)),
            ))?;
        }
        r.add_setting_group(SettingGroupSpec::new(GROUP, "probe.group"))?;
        let choice = |id: Id| {
            let mut spec = SettingSpec::new(
                id.clone(),
                "",
                SettingKind::Choice(Choices::Fixed(OPTIONS)),
                SettingValue::Text(Cow::Borrowed(OPTIONS[0])),
                GROUP,
            );
            spec.title = id.as_str().to_string().into();
            spec
        };
        static EDITED: paperpaw_sdk::SettingExtra =
            paperpaw_sdk::SettingExtra::new().editor(EDITOR);
        r.add_setting(choice(KIND).with_extra(&EDITED))?;
        r.add_setting(choice(MANY))?;
        r.add_setting(SettingSpec::new(
            SIZE,
            "probe.size",
            SettingKind::Int { min: 1, max: 9 },
            SettingValue::Int(5),
            GROUP,
        ))?;
        for (order, item) in [KIND, MANY, SIZE].into_iter().enumerate() {
            r.add_menu_entry(MenuEntry::new(BAR_MENU, item).order(order as i16))?;
        }
        r.add_menu_entry(MenuEntry::new(CHOICES, MANY))?;
        Ok(())
    }
}

struct Test {
    kernel: Kernel,
    now: u64,
}

impl Test {
    /// A kernel with the menus over [`BASE`], with the settings plugin
    /// or without it.
    fn new(with_settings: bool) -> Self {
        let mut builder = Kernel::builder(X4)
            .storage(MemoryStorage::new())
            .plugin(Menu)
            .plugin(Probe);
        if with_settings {
            builder = builder.plugin(SettingsPlugin);
        }
        let mut kernel = builder.build().unwrap();
        kernel.start(Instant(0));
        kernel.open_screen(Instant(0), BASE, Params::new());
        Self { kernel, now: 0 }
    }

    /// Opens `menu` in `slot`, as `mode` if given.
    fn open(&mut self, slot: Id, menu: &Id, mode: Option<&str>) {
        let mut with = Params::new().with(params::MENU, menu.as_str().to_string());
        if let Some(mode) = mode {
            with = with.with(params::MODE, mode.to_string());
        }
        self.now += 200;
        self.kernel.open_slot(Instant(self.now), slot, with);
        self.kernel.render(&mut |_, _| {});
    }

    fn press(&mut self, name: &'static str) {
        self.now += 200;
        let at = Instant(self.now);
        for (later, state) in [(0, ButtonState::Pressed), (50, ButtonState::Released)] {
            self.kernel
                .handle(at.add_millis(later), InputEvent::Button { name, state });
        }
        self.kernel.render(&mut |_, _| {});
    }

    fn active(&self) -> Option<&Id> {
        self.kernel.active_screen()
    }

    fn many(&self) -> &str {
        self.kernel.settings().text(MANY.as_str())
    }
}

#[test]
fn the_bar_opens_a_settings_own_editor() {
    let mut test = Test::new(true);
    test.open(slots::MENU, &BAR_MENU, Some("bar"));
    assert_eq!(test.active(), Some(&MENU_SCREEN));
    test.press("confirm");
    assert_eq!(test.active(), Some(&EDITOR));
}

#[test]
fn the_bar_opens_the_settings_of_a_number() {
    let mut test = Test::new(true);
    test.open(slots::MENU, &BAR_MENU, Some("bar"));
    test.press("down");
    test.press("down");
    test.press("confirm");
    assert_eq!(test.active(), Some(&SETTINGS_SCREEN));
}

/// Where a choice of many is chosen: the bar (its second entry), the
/// list and the panel.
const PLACES: [(Id, Id, Option<&str>, usize); 3] = [
    (slots::MENU, BAR_MENU, Some("bar"), 1),
    (slots::MENU, CHOICES, None, 0),
    (slots::PANEL, CHOICES, None, 0),
];

/// Opens [`MANY`] where `place` says and chooses it `times` times.
fn choose(with_settings: bool, place: &(Id, Id, Option<&str>, usize), times: usize) -> Test {
    let (slot, menu, mode, downs) = place;
    let mut test = Test::new(with_settings);
    test.open(slot.clone(), menu, *mode);
    for _ in 0..*downs {
        test.press("down");
    }
    for _ in 0..times {
        test.press("confirm");
    }
    test
}

#[test]
fn choices_open_the_settings_list_where_there_is_one() {
    for place in &PLACES {
        let test = choose(true, place, 1);
        assert_eq!(test.active(), Some(&CHOOSE_SCREEN), "{place:?}");
        assert_eq!(test.many(), OPTIONS[0], "{place:?}");
    }
}

#[test]
fn choices_step_on_without_the_settings_plugin() {
    // Every place that fails, with what it chose.
    let mut wrong = Vec::new();
    for place in &PLACES {
        let shown = choose(false, place, 0).active().cloned();
        for (times, option) in [(1, OPTIONS[1]), (2, OPTIONS[2])] {
            let test = choose(false, place, times);
            // The menu stays open, with the next value.
            if test.many() != option || test.active() != shown.as_ref() {
                wrong.push((place.0.clone(), place.2, times, test.many().to_string()));
            }
        }
    }
    assert!(wrong.is_empty(), "{wrong:?}");
}
