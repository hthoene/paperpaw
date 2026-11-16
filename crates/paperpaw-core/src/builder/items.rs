//! Contributions given to the builder as a closure (#244, slice D2's target
//! for tests): what a test fixture wrote as a type with `impl Plugin` it
//! writes as [`KernelBuilder::items`], and names neither `Plugin` nor
//! `Registrar`.
//!
//! [`Items`] is a type of the kernel's own over the SDK's `Registrar`. It
//! offers the adding calls that have a caller today, under the same names and
//! signatures, and nothing else: not the calls nobody makes, not the
//! contributions themselves. So a fixture cannot come to depend on what goes
//! with `Registrar`, and the compiler says so. The port is a change of names:
//! handlers still get the old `Env`, screens are still an old `Screen`, and a
//! plugin is still named by a `Manifest`.

use super::KernelBuilder;
use alloc::borrow::Cow;
use alloc::string::String;
use paperpaw_sdk::{Id, Manifest, Plugin, RegisterError, Registrar};

/// What a closure given to [`KernelBuilder::items`] adds its contributions
/// with: the adding calls of the SDK's `Registrar` that have a caller today.
/// Name this, not that.
#[derive(Debug)]
pub struct Items(Registrar);

impl Items {
    /// The id the contributions are added under.
    pub fn plugin(&self) -> &Id {
        self.0.plugin()
    }

    /// As `add_action` of the SDK's `Registrar`.
    pub fn add_action(&mut self, action: paperpaw_sdk::ActionSpec) -> Result<(), RegisterError> {
        self.0.add_action(action)
    }

    /// As `add_screen` of the SDK's `Registrar`.
    pub fn add_screen(&mut self, screen: paperpaw_sdk::ScreenSpec) -> Result<(), RegisterError> {
        self.0.add_screen(screen)
    }

    /// As `add_screens` of the SDK's `Registrar`.
    pub fn add_screens(
        &mut self,
        screens: &'static [paperpaw_sdk::FixedScreen],
    ) -> Result<(), RegisterError> {
        self.0.add_screens(screens)
    }

    /// As `add_slot` of the SDK's `Registrar`.
    pub fn add_slot(&mut self, slot: paperpaw_sdk::SlotSpec) -> Result<(), RegisterError> {
        self.0.add_slot(slot)
    }

    /// As `set_ui_look` of the SDK's `Registrar`.
    pub fn set_ui_look(
        &mut self,
        painters: &'static paperpaw_sdk::ui::look::Painters,
        priority: paperpaw_sdk::Priority,
    ) -> Result<(), RegisterError> {
        self.0.set_ui_look(painters, priority)
    }

    /// As `bind_slot` of the SDK's `Registrar`.
    pub fn bind_slot(
        &mut self,
        slot: impl Into<Id>,
        screen: impl Into<Id>,
        priority: paperpaw_sdk::Priority,
    ) -> Result<(), RegisterError> {
        self.0.bind_slot(slot, screen, priority)
    }

    /// As `add_setting` of the SDK's `Registrar`.
    pub fn add_setting(&mut self, setting: paperpaw_sdk::SettingSpec) -> Result<(), RegisterError> {
        self.0.add_setting(setting)
    }

    /// As `add_settings` of the SDK's `Registrar`.
    pub fn add_settings(
        &mut self,
        settings: &'static [paperpaw_sdk::SettingSpec],
    ) -> Result<(), RegisterError> {
        self.0.add_settings(settings)
    }

    /// As `add_keymap` of the SDK's `Registrar`.
    pub fn add_keymap(
        &mut self,
        source: impl Into<Cow<'static, str>>,
    ) -> Result<(), RegisterError> {
        self.0.add_keymap(source)
    }

    /// As `add_stylesheet` of the SDK's `Registrar`.
    pub fn add_stylesheet(
        &mut self,
        css: impl Into<Cow<'static, str>>,
    ) -> Result<(), RegisterError> {
        self.0.add_stylesheet(css)
    }

    /// As `add_book_data` of the SDK's `Registrar`.
    pub fn add_book_data(
        &mut self,
        folder: impl Into<Cow<'static, str>>,
        prefix: impl Into<Cow<'static, str>>,
    ) -> Result<(), RegisterError> {
        self.0.add_book_data(folder, prefix)
    }

    /// As `add_book_cache` of the SDK's `Registrar`.
    pub fn add_book_cache(
        &mut self,
        folder: impl Into<Cow<'static, str>>,
        prefix: impl Into<Cow<'static, str>>,
    ) -> Result<(), RegisterError> {
        self.0.add_book_cache(folder, prefix)
    }

    /// As `add_book_field` of the SDK's `Registrar`.
    pub fn add_book_field(
        &mut self,
        field: paperpaw_sdk::library::BookField,
    ) -> Result<(), RegisterError> {
        self.0.add_book_field(field)
    }

    /// As `add_keymap_preset` of the SDK's `Registrar`.
    pub fn add_keymap_preset(
        &mut self,
        id: impl Into<Id>,
        source: impl Into<Cow<'static, str>>,
    ) -> Result<(), RegisterError> {
        self.0.add_keymap_preset(id, source)
    }

    /// As `add_font` of the SDK's `Registrar`.
    pub fn add_font(&mut self, font: paperpaw_sdk::FontSpec) -> Result<(), RegisterError> {
        self.0.add_font(font)
    }

    /// As `add_font_file` of the SDK's `Registrar`.
    pub fn add_font_file(
        &mut self,
        id: impl Into<Id>,
        path: impl Into<String>,
    ) -> Result<(), RegisterError> {
        self.0.add_font_file(id, path)
    }

    /// As `add_font_extension` of the SDK's `Registrar`.
    pub fn add_font_extension(
        &mut self,
        id: impl Into<Id>,
        path: impl Into<String>,
        extends: impl Into<Id>,
    ) -> Result<(), RegisterError> {
        self.0.add_font_extension(id, path, extends)
    }

    /// As `add_font_face` of the SDK's `Registrar`.
    pub fn add_font_face(&mut self, face: paperpaw_sdk::FontFace) -> Result<(), RegisterError> {
        self.0.add_font_face(face)
    }

    /// As `bind_font` of the SDK's `Registrar`.
    pub fn bind_font(
        &mut self,
        role: impl Into<Id>,
        font: impl Into<Id>,
        priority: paperpaw_sdk::Priority,
    ) -> Result<(), RegisterError> {
        self.0.bind_font(role, font, priority)
    }

    /// As `add_actions` of the SDK's `Registrar`.
    pub fn add_actions(
        &mut self,
        actions: &'static [paperpaw_sdk::FixedAction],
    ) -> Result<(), RegisterError> {
        self.0.add_actions(actions)
    }

    /// As `add_menu_entry` of the SDK's `Registrar`.
    pub fn add_menu_entry(&mut self, entry: paperpaw_sdk::MenuEntry) -> Result<(), RegisterError> {
        self.0.add_menu_entry(entry)
    }

    /// As `add_menu_entries` of the SDK's `Registrar`.
    pub fn add_menu_entries(
        &mut self,
        entries: &'static [paperpaw_sdk::MenuEntry],
    ) -> Result<(), RegisterError> {
        self.0.add_menu_entries(entries)
    }

    /// As `place_menu_entry` of the SDK's `Registrar`.
    pub fn place_menu_entry(
        &mut self,
        menu: impl Into<Id>,
        item: impl Into<Id>,
        place: paperpaw_sdk::MenuPlace,
    ) -> Result<(), RegisterError> {
        self.0.place_menu_entry(menu, item, place)
    }

    /// As `hide_menu_entry` of the SDK's `Registrar`.
    pub fn hide_menu_entry(
        &mut self,
        menu: impl Into<Id>,
        item: impl Into<Id>,
    ) -> Result<(), RegisterError> {
        self.0.hide_menu_entry(menu, item)
    }

    /// As `set_item_state` of the SDK's `Registrar`.
    pub fn set_item_state(
        &mut self,
        item: impl Into<Id>,
        state: impl Fn(&paperpaw_sdk::Env<'_>) -> paperpaw_sdk::ItemState + 'static,
    ) -> Result<(), RegisterError> {
        self.0.set_item_state(item, state)
    }

    /// As `describe_item` of the SDK's `Registrar`.
    pub fn describe_item(
        &mut self,
        item: impl Into<Id>,
        key: impl Into<Cow<'static, str>>,
    ) -> Result<(), RegisterError> {
        self.0.describe_item(item, key)
    }

    /// As `add_menu_section` of the SDK's `Registrar`.
    pub fn add_menu_section(
        &mut self,
        section: paperpaw_sdk::MenuSection,
    ) -> Result<(), RegisterError> {
        self.0.add_menu_section(section)
    }

    /// As `add_icon` of the SDK's `Registrar`.
    pub fn add_icon(
        &mut self,
        item: impl Into<Id>,
        icon: impl Into<Cow<'static, str>>,
    ) -> Result<(), RegisterError> {
        self.0.add_icon(item, icon)
    }

    /// As `add_icons` of the SDK's `Registrar`.
    pub fn add_icons(&mut self, icons: &'static [(Id, &'static str)]) -> Result<(), RegisterError> {
        self.0.add_icons(icons)
    }

    /// As `set_menu_need` of the SDK's `Registrar`.
    pub fn set_menu_need(
        &mut self,
        item: impl Into<Id>,
        need: paperpaw_sdk::MenuNeed,
    ) -> Result<(), RegisterError> {
        self.0.set_menu_need(item, need)
    }

    /// As `set_screen_hints` of the SDK's `Registrar`.
    pub fn set_screen_hints(
        &mut self,
        screen: impl Into<Id>,
        hints: paperpaw_sdk::HintVisibility,
    ) -> Result<(), RegisterError> {
        self.0.set_screen_hints(screen, hints)
    }

    /// As `add_setting_group` of the SDK's `Registrar`.
    pub fn add_setting_group(
        &mut self,
        group: impl Into<paperpaw_sdk::SettingGroup>,
    ) -> Result<(), RegisterError> {
        self.0.add_setting_group(group)
    }

    /// As `add_format` of the SDK's `Registrar`.
    pub fn add_format(&mut self, format: paperpaw_sdk::FormatSpec) -> Result<(), RegisterError> {
        self.0.add_format(format)
    }

    /// As `subscribe` of the SDK's `Registrar`.
    pub fn subscribe(
        &mut self,
        event: impl Into<Id>,
        handler: paperpaw_sdk::EventHandler,
    ) -> Result<(), RegisterError> {
        self.0.subscribe(event, handler)
    }

    /// As `add_subscriptions` of the SDK's `Registrar`.
    pub fn add_subscriptions(
        &mut self,
        subscriptions: &'static [paperpaw_sdk::event::FixedSubscription],
    ) -> Result<(), RegisterError> {
        self.0.add_subscriptions(subscriptions)
    }

    /// As `on` of the SDK's `Registrar`.
    pub fn on<P: paperpaw_sdk::event::EventData>(
        &mut self,
        event: paperpaw_sdk::event::Event<P>,
        handler: impl Fn(&paperpaw_sdk::Env<'_>, &P, &mut paperpaw_sdk::Commands) + 'static,
    ) -> Result<(), RegisterError> {
        self.0.on(event, handler)
    }

    /// As `add_status_item` of the SDK's `Registrar`.
    pub fn add_status_item(
        &mut self,
        item: paperpaw_sdk::StatusItemSpec,
    ) -> Result<(), RegisterError> {
        self.0.add_status_item(item)
    }

    /// As `add_text_filter` of the SDK's `Registrar`.
    pub fn add_text_filter(
        &mut self,
        filter: paperpaw_sdk::filters::TextFilterSpec,
    ) -> Result<(), RegisterError> {
        self.0.add_text_filter(filter)
    }

    /// As `add_decorator` of the SDK's `Registrar`.
    pub fn add_decorator(
        &mut self,
        decorator: paperpaw_sdk::decorations::DecoratorSpec,
    ) -> Result<(), RegisterError> {
        self.0.add_decorator(decorator)
    }

    /// As `add_page_layer` of the SDK's `Registrar`.
    pub fn add_page_layer(
        &mut self,
        layer: paperpaw_sdk::layers::PageLayerSpec,
    ) -> Result<(), RegisterError> {
        self.0.add_page_layer(layer)
    }

    /// As `add_page_frame` of the SDK's `Registrar`.
    pub fn add_page_frame(
        &mut self,
        frame: paperpaw_sdk::layers::PageFrameSpec,
    ) -> Result<(), RegisterError> {
        self.0.add_page_frame(frame)
    }

    /// As `add_voice` of the SDK's `Registrar`.
    pub fn add_voice(
        &mut self,
        voice: paperpaw_sdk::voices::VoiceSpec,
    ) -> Result<(), RegisterError> {
        self.0.add_voice(voice)
    }

    /// As `add_input_filter` of the SDK's `Registrar`.
    pub fn add_input_filter(
        &mut self,
        filter: paperpaw_sdk::filters::InputFilterSpec,
    ) -> Result<(), RegisterError> {
        self.0.add_input_filter(filter)
    }

    /// As `add_refresh_filter` of the SDK's `Registrar`.
    pub fn add_refresh_filter(
        &mut self,
        filter: paperpaw_sdk::filters::RefreshFilterSpec,
    ) -> Result<(), RegisterError> {
        self.0.add_refresh_filter(filter)
    }

    /// As `override_action` of the SDK's `Registrar`.
    pub fn override_action(
        &mut self,
        action: impl Into<Id>,
        handler: impl Fn(&paperpaw_sdk::Env<'_>, &paperpaw_sdk::Params, &mut paperpaw_sdk::Commands)
        + 'static,
        priority: paperpaw_sdk::Priority,
    ) -> Result<(), RegisterError> {
        self.0.override_action(action, handler, priority)
    }

    /// As `observe_actions` of the SDK's `Registrar`.
    pub fn observe_actions(
        &mut self,
        observer: paperpaw_sdk::ObserverSpec,
    ) -> Result<(), RegisterError> {
        self.0.observe_actions(observer)
    }

    /// As `observe_events` of the SDK's `Registrar`.
    pub fn observe_events(
        &mut self,
        observer: paperpaw_sdk::ObserverSpec,
    ) -> Result<(), RegisterError> {
        self.0.observe_events(observer)
    }

    /// As `add_input` of the SDK's `Registrar`.
    pub fn add_input(&mut self, input: paperpaw_sdk::keys::InputSpec) -> Result<(), RegisterError> {
        self.0.add_input(input)
    }

    /// As `add_gesture` of the SDK's `Registrar`.
    pub fn add_gesture(
        &mut self,
        gesture: paperpaw_sdk::keys::GestureSpec,
    ) -> Result<(), RegisterError> {
        self.0.add_gesture(gesture)
    }

    /// As `add_chrome` of the SDK's `Registrar`.
    pub fn add_chrome(
        &mut self,
        chrome: paperpaw_sdk::chrome::ChromeSpec,
    ) -> Result<(), RegisterError> {
        self.0.add_chrome(chrome)
    }

    /// As `add_work` of the SDK's `Registrar`.
    pub fn add_work(&mut self, work: paperpaw_sdk::work::WorkSpec) -> Result<(), RegisterError> {
        self.0.add_work(work)
    }

    /// As `add_widget` of the SDK's `Registrar`.
    pub fn add_widget(
        &mut self,
        widget: paperpaw_sdk::widgets::WidgetSpec,
    ) -> Result<(), RegisterError> {
        self.0.add_widget(widget)
    }

    /// As `add_setup_step` of the SDK's `Registrar`.
    pub fn add_setup_step(
        &mut self,
        step: paperpaw_sdk::setup::SetupStepSpec,
    ) -> Result<(), RegisterError> {
        self.0.add_setup_step(step)
    }

    /// As `add_preset` of the SDK's `Registrar`.
    pub fn add_preset(
        &mut self,
        preset: paperpaw_sdk::presets::Preset,
    ) -> Result<(), RegisterError> {
        self.0.add_preset(preset)
    }

    /// As `add_presets` of the SDK's `Registrar`.
    pub fn add_presets(
        &mut self,
        presets: &'static [paperpaw_sdk::presets::Preset],
    ) -> Result<(), RegisterError> {
        self.0.add_presets(presets)
    }

    /// As `add_status_line` of the SDK's `Registrar`.
    pub fn add_status_line(
        &mut self,
        line: paperpaw_sdk::StatusLineSpec,
    ) -> Result<(), RegisterError> {
        self.0.add_status_line(line)
    }

    /// As `add_widget_place` of the SDK's `Registrar`.
    pub fn add_widget_place(
        &mut self,
        place: paperpaw_sdk::widgets::WidgetPlace,
    ) -> Result<(), RegisterError> {
        self.0.add_widget_place(place)
    }

    /// As `provide` of the SDK's `Registrar`.
    pub fn provide<T: paperpaw_sdk::services::ServiceType>(
        &mut self,
        id: impl Into<Id>,
        service: T,
        version: paperpaw_sdk::Version,
    ) -> Result<(), RegisterError> {
        self.0.provide(id, service, version)
    }

    /// As `add_point` of the SDK's `Registrar`.
    pub fn add_point(
        &mut self,
        point: paperpaw_sdk::services::PointSpec,
    ) -> Result<(), RegisterError> {
        self.0.add_point(point)
    }

    /// As `declare_event` of the SDK's `Registrar`.
    pub fn declare_event(
        &mut self,
        event: paperpaw_sdk::services::EventSpec,
    ) -> Result<(), RegisterError> {
        self.0.declare_event(event)
    }

    /// As `contribute` of the SDK's `Registrar`.
    pub fn contribute(
        &mut self,
        point: impl Into<Id>,
        contribution: impl Into<Cow<'static, str>>,
    ) -> Result<(), RegisterError> {
        self.0.contribute(point, contribution)
    }

    /// As `add_translations` of the SDK's `Registrar`.
    pub fn add_translations(
        &mut self,
        source: impl Into<Cow<'static, str>>,
    ) -> Result<(), RegisterError> {
        self.0.add_translations(source)
    }

    /// As `add_translation_file` of the SDK's `Registrar`.
    pub fn add_translation_file(&mut self, path: impl Into<String>) -> Result<(), RegisterError> {
        self.0.add_translation_file(path)
    }

    /// As `set_setting_default` of the SDK's `Registrar`.
    pub fn set_setting_default(
        &mut self,
        setting: impl Into<Id>,
        value: impl Into<Cow<'static, str>>,
    ) -> Result<(), RegisterError> {
        self.0.set_setting_default(setting, value)
    }

    /// As `add_choice` of the SDK's `Registrar`.
    pub fn add_choice(
        &mut self,
        setting: impl Into<Id>,
        value: impl Into<Cow<'static, str>>,
    ) -> Result<(), RegisterError> {
        self.0.add_choice(setting, value)
    }
}

/// The closure of [`KernelBuilder::items`] as the plugin the kernel still
/// keeps its contributions by.
struct Given<F> {
    manifest: Manifest,
    register: F,
}

impl<F: Fn(&mut Items) -> Result<(), RegisterError>> Plugin for Given<F> {
    fn manifest(&self) -> Manifest {
        self.manifest.clone()
    }

    fn register(&self, registrar: &mut Registrar) -> Result<(), RegisterError> {
        let empty = Registrar::new(registrar.plugin().clone());
        let mut items = Items(core::mem::replace(registrar, empty));
        let result = (self.register)(&mut items);
        *registrar = items.0;
        result
    }
}

impl KernelBuilder {
    /// Adds what `register` contributes, under `manifest` (its id is the
    /// namespace). It is a plugin in every way [`KernelBuilder::plugin`]'s
    /// is: listed, switched off and on, registered again when the plugins
    /// change (so `register` may run more than once), and one that fails
    /// leaves nothing behind.
    pub fn items(
        self,
        manifest: Manifest,
        register: impl Fn(&mut Items) -> Result<(), RegisterError> + 'static,
    ) -> Self {
        self.plugin(Given { manifest, register })
    }

    /// As [`KernelBuilder::items`], off until the user switches it on
    /// ([`KernelBuilder::plugin_off`]).
    pub fn items_off(
        self,
        manifest: Manifest,
        register: impl Fn(&mut Items) -> Result<(), RegisterError> + 'static,
    ) -> Self {
        self.plugin_off(Given { manifest, register })
    }
}
