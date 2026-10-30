//! Plugins registered: their contributions merged into the kernel, the
//! plugins switched off or required, menus, key maps and fonts.

#[allow(
    unused_imports,
    reason = "a part of the kernel, which sees all of it; not every part uses every name"
)]
use super::*;

/// The sizes of book text offered, in points, where the reading font has
/// every size; with a font pack, those it has (see
/// `Kernel::fit_font_size`).
pub(crate) const FONT_SIZES: (i64, i64) = (6, 40);

/// Sections of `paperpaw.ini` that are no plugin's settings: no plugin
/// may take their name as its id (its settings would be read as these).
pub(crate) const RESERVED_IDS: [&str; 5] = ["plugins", "fonts", "menus", KNOWN, "slots"];

/// After a menu's id in `[menus]`: the entries plugins contributed when
/// the user redefined it.
/// The config section with what plugins contributed to menus the user
/// redefined, and the user left out (see [`Menus::set_known`]).
///
/// [`Menus::set_known`]: paperpaw_sdk::Menus::set_known
pub(crate) const KNOWN: &str = "menus-known";

/// The key maps the kernel's is built from.
#[derive(Debug, Default)]
pub(crate) struct KeySources {
    /// The plugins' key maps, by plugin, to rebuild the key map when the
    /// preset changes or a plugin goes.
    pub(crate) plugins: Vec<(Id, Cow<'static, str>)>,
    /// The device's own key map, under all others.
    pub(crate) device: Cow<'static, str>,
    pub(crate) presets: Vec<(Id, Cow<'static, str>)>,
    /// Plugins' key maps that apply while a switch (the id) is on.
    pub(crate) when: Vec<(Id, Cow<'static, str>)>,
    /// The user's bindings: `paperpaw.ini` as read at start and changed
    /// since ([`Command::Bind`]), then the shell's overrides (see
    /// [`KernelBuilder::user_config`]), which win.
    pub(crate) user: String,
    pub(crate) shell: String,
}

impl Kernel {
    /// Registers `plugin`, or only lists it if the user switched it off.
    pub(crate) fn add_plugin(&mut self, plugin: &dyn Plugin, disabled: &[String]) {
        let manifest = plugin.manifest();
        if manifest.id != CORE && disabled.iter().any(|id| id == manifest.id.as_str()) {
            self.register_off(plugin, manifest);
        } else {
            self.register_plugin(plugin, manifest);
        }
    }

    pub(crate) fn register_plugin(&mut self, plugin: &dyn Plugin, manifest: Manifest) {
        let id = manifest.id.clone();
        let source = || WarningSource::Plugin(id.clone());
        if id.as_str() != id.namespace() {
            return self.warn(source(), None, "plugin ids must not contain '.'".into());
        }
        if self.shared.plugins.get(id.as_str()).is_some() {
            return self.warn(
                source(),
                None,
                "a plugin with this id is already loaded".into(),
            );
        }
        if RESERVED_IDS.contains(&id.as_str()) {
            let message = format!("`{id}` names a section of {CONFIG_FILE}; not loaded");
            self.warn(source(), None, message);
            return self.add_plugin_info(manifest, PluginState::Failed, true);
        }
        let mut staging = Staging::default();
        let mut registrar = Registrar::new(id.clone());
        let result = plugin
            .register(&mut registrar)
            .and_then(|()| registry::stage(&id, registrar.into_contributions(), &mut staging))
            .and_then(|()| self.merge(&id, staging));
        match result {
            Ok(()) => self.add_plugin_info(manifest, PluginState::Running, true),
            Err(error) => {
                let line = match error {
                    RegisterError::Parse { line, .. } => Some(line),
                    _ => None,
                };
                self.warn(source(), line, format!("not loaded: {error}"));
                self.add_plugin_info(manifest, PluginState::Failed, true);
            }
        }
    }

    /// Lists a plugin the user switched off. Only its translations are
    /// loaded, so plugin managers can show its name and description in the
    /// user's language.
    pub(crate) fn register_off(&mut self, plugin: &dyn Plugin, manifest: Manifest) {
        let id = manifest.id.clone();
        if id.as_str() != id.namespace() || self.shared.plugins.get(id.as_str()).is_some() {
            return;
        }
        if RESERVED_IDS.contains(&id.as_str()) {
            return self.register_plugin(plugin, manifest);
        }
        let mut registrar = Registrar::new(id);
        if plugin.register(&mut registrar).is_ok() {
            for contribution in registrar.into_contributions() {
                if let Contribution::Translations(source) = contribution
                    && let Ok(catalog) = registry::catalog(source)
                {
                    self.shared.translator.add(catalog);
                }
            }
        }
        self.add_plugin_info(manifest, PluginState::Off, false);
    }

    pub(crate) fn add_plugin_info(
        &mut self,
        manifest: Manifest,
        state: PluginState,
        enabled: bool,
    ) {
        self.shared.plugins.push(PluginInfo::new(
            manifest,
            state,
            enabled,
            RequiredFor::default(),
            paperpaw_sdk::PluginKind::BuiltIn,
            None,
            None,
        ));
    }

    /// Marks plugins that alone fill a standard slot or provide all fonts:
    /// switching one of them off would leave the user without e.g.
    /// settings, a menu or text, and no way back.
    pub(crate) fn find_required_plugins(&mut self) {
        for slot in [
            slots::HOME,
            slots::READER,
            slots::LIBRARY,
            slots::MENU,
            slots::SETTINGS,
        ] {
            let sole = {
                let mut providers = self
                    .shared
                    .registry
                    .slot_screens(&slot)
                    .map(Id::namespace)
                    .filter(|plugin| *plugin != CORE.as_str());
                match providers.next() {
                    Some(first) if providers.all(|plugin| plugin == first) => first.to_string(),
                    _ => continue,
                }
            };
            if let Some(info) = self.shared.plugins.get_mut(&sole) {
                info.required_for.insert(&slot);
            }
        }
        // Any font can stand in for any role, but without fonts no text
        // is shown at all.
        let mut owners = self.shared.fonts.ids().map(Id::namespace);
        let sole = match owners.next() {
            Some(first) if owners.all(|plugin| plugin == first) => first.to_string(),
            _ => return,
        };
        if let Some(info) = self.shared.plugins.get_mut(&sole) {
            info.required_for.insert(&paperpaw_sdk::builtin::fonts::UI);
        }
    }

    /// Merges a plugin's contributions: all of them or none.
    #[expect(
        clippy::too_many_lines,
        reason = "longer than the limit from before it was set; split it when it is next changed"
    )]
    pub(crate) fn merge(&mut self, plugin: &Id, mut staging: Staging) -> Result<(), RegisterError> {
        for setting in staging.settings.iter().map(registry::Staged::spec) {
            if self.shared.settings.get(setting.id.as_str()).is_some() {
                return Err(RegisterError::Duplicate {
                    id: setting.id.clone(),
                });
            }
            if !setting.kind.accepts(&setting.default) {
                return Err(RegisterError::Invalid {
                    message: Cow::Owned(format!("invalid default for `{}`", setting.id)),
                });
            }
        }
        if let Some(font) = staging
            .fonts
            .iter()
            .find(|f| self.shared.fonts.contains(f.id.as_str()))
        {
            return Err(RegisterError::Duplicate {
                id: font.id.clone(),
            });
        }
        if let Some(format) = staging
            .formats
            .iter()
            .find(|f| self.shared.formats.contains(f.id.as_str()))
        {
            return Err(RegisterError::Duplicate {
                id: format.id.clone(),
            });
        }
        if let Some(spec) = staging
            .hyphenation
            .iter()
            .find(|h| self.shared.hyphenation.contains(h.id.as_str()))
        {
            return Err(RegisterError::Duplicate {
                id: spec.id.clone(),
            });
        }
        staging.merge_into(&mut self.shared.registry, &mut self.shared.actions)?;
        // A sealed action that came after handlers in its place: they go.
        for action in self.shared.actions.drop_sealed_overrides() {
            self.warn_runtime(format!("`{action}` is sealed; not overridden"));
        }
        for setting in staging.settings.drain(..) {
            // Cannot fail: checked above.
            let _ = match setting {
                registry::Staged::Owned(spec) => self.shared.settings.define(*spec),
                registry::Staged::Fixed(list, at) => self.shared.settings.define_fixed(list, at),
            };
        }
        for source in staging.keymaps.drain(..) {
            // Validated by the registrar.
            self.shared.keymap.load(&source, Origin::Plugin, true);
            self.keys.plugins.push((plugin.clone(), source));
        }
        self.keys.presets.append(&mut staging.keymap_presets);
        // Loaded once the settings are known (`rebuild_keymap`).
        self.keys.when.append(&mut staging.keymaps_when);
        for catalog in staging.catalogs.drain(..) {
            self.shared.translator.add(catalog);
        }
        for font in staging.fonts.drain(..) {
            // Cannot fail: checked above.
            let _ = self.shared.fonts.add(font);
        }
        for (id, path) in staging.font_files.drain(..) {
            self.add_font_file(plugin, id, path);
        }
        for (id, path, extends) in staging.font_extensions.drain(..) {
            if let Err(error) = self.shared.fonts.add_extension(id, path, extends) {
                self.warn(
                    WarningSource::Plugin(plugin.clone()),
                    None,
                    format!("{error}"),
                );
            }
        }
        for face in staging.font_faces.drain(..) {
            self.add_font_face(plugin, face);
        }
        for (role, font, priority) in staging.font_roles.drain(..) {
            self.shared.fonts.propose(role, font, priority);
        }
        // The highest priority, of equal ones the later.
        for (painters, priority) in staging.ui_looks.drain(..) {
            if self.shared.look.1.is_none_or(|p| priority >= p) {
                self.shared.look = (paperpaw_sdk::ui::look::Look::of(painters), Some(priority));
            }
        }
        for format in staging.formats.drain(..) {
            // Cannot fail: checked above.
            let _ = self.shared.formats.add(format);
        }
        for group in staging.setting_groups.drain(..) {
            if let Err(error) = self.shared.settings.add_group(group) {
                self.warn_runtime(format!("{error}"));
            }
        }
        for spec in staging.hyphenation.drain(..) {
            // Cannot fail: checked above and by the registrar.
            let _ = self.shared.hyphenation.add(spec);
        }
        self.subscriptions.append(&mut staging.subscriptions);
        for (action, priority, handler) in staging.overrides.drain(..) {
            // Not in place of a sealed action (one known by now; one that
            // comes later is never overridden either).
            if self
                .shared
                .actions
                .get(action.as_str())
                .is_some_and(paperpaw_sdk::Action::is_sealed)
            {
                self.warn_runtime(format!("{plugin}: `{action}` is sealed; not overridden"));
                continue;
            }
            self.shared
                .actions
                .add_override(action, priority, plugin.clone(), handler);
        }
        self.action_observers.append(&mut staging.action_observers);
        self.event_observers.append(&mut staging.event_observers);
        self.plugin_defaults.extend(
            staging
                .setting_defaults
                .drain(..)
                .map(|(id, value)| (plugin.clone(), id, value)),
        );
        self.plugin_choices.extend(
            staging
                .choices
                .drain(..)
                .map(|(id, value)| (plugin.clone(), id, value)),
        );
        for entry in staging.menu_entries.drain(..) {
            self.shared.menus.add(entry);
        }
        for list in staging.fixed_menu_entries.drain(..) {
            self.shared.menus.add_fixed(list);
        }
        for list in staging.fixed_icons.drain(..) {
            self.shared.menus.add_fixed_icons(list);
        }
        for section in staging.menu_sections.drain(..) {
            self.shared.menus.add_section(section);
        }
        for (item, icon) in staging.icons.drain(..) {
            self.shared.menus.set_icon(item, icon);
        }
        for (item, need) in staging.menu_needs.drain(..) {
            self.shared.menus.set_need(item, need);
        }
        for (menu, item, by) in staging.hidden_entries.drain(..) {
            self.shared.menus.hide(menu, item, by);
        }
        for (item, state) in staging.item_states.drain(..) {
            self.shared.menus.set_state(item, state);
        }
        for (item, key) in staging.item_descriptions.drain(..) {
            self.shared.menus.set_description(item, key);
        }
        for (screen, hints) in staging.screen_hints.drain(..) {
            self.shared.registry.hints.insert(screen, hints);
        }
        for item in staging.status_items.drain(..) {
            self.shared.status_items.add(item);
        }
        for line in staging.status_lines.drain(..) {
            self.shared.status_items.add_line(line);
        }
        for place in staging.status_places.drain(..) {
            self.shared.status_items.add_place(place);
        }
        for place in staging.widget_places.drain(..) {
            self.shared.widgets.add_place(place);
        }
        for category in staging.widget_categories.drain(..) {
            self.shared.widgets.add_category(category);
        }
        for step in staging.setup_steps.drain(..) {
            self.shared.setup.add(step);
        }
        for list in staging.fixed_presets.drain(..) {
            self.shared.settings.presets_mut().add_fixed(list);
        }
        for preset in staging.presets.drain(..) {
            self.shared.settings.presets_mut().add(preset);
        }
        for widget in staging.widgets.drain(..) {
            self.shared.widgets.add(widget);
        }
        self.stylesheet_owners
            .extend(staging.stylesheets.iter().map(|_| plugin.clone()));
        self.shared.stylesheets.append(&mut staging.stylesheets);
        self.shared.book_data.append(&mut staging.book_data);
        self.shared.book_fields.append(&mut staging.book_fields);
        self.shared.book_badges.append(&mut staging.book_badges);
        for (id, service) in staging.services.drain(..) {
            self.shared.services.insert(id, service);
        }
        for point in staging.points.drain(..) {
            self.shared.services.declare_point(point);
        }
        for event in staging.events.drain(..) {
            self.shared.services.declare_event(event);
        }
        for (point, contribution) in staging.contributions.drain(..) {
            self.shared
                .services
                .contribute(point, plugin.clone(), contribution);
        }
        for filter in staging.text_filters.drain(..) {
            self.shared.text_filters.add(filter);
        }
        for decorator in staging.decorators.drain(..) {
            self.shared.decorators.add(decorator);
        }
        for layer in staging.page_layers.drain(..) {
            self.shared.page_layers.add(layer);
        }
        for frame in staging.page_frames.drain(..) {
            self.shared.page_layers.add_frame(frame);
        }
        for provider in staging.lookup_providers.drain(..) {
            self.shared.lookup.add(provider);
        }
        for voice in staging.voices.drain(..) {
            self.shared.voices.add(voice);
        }
        for reader in staging.readers.drain(..) {
            self.shared.readers.add(reader);
        }
        for filter in staging.input_filters.drain(..) {
            self.shared.input_filters.add(filter);
        }
        for service in staging.gatt_services.drain(..) {
            if let Some(radio) = &mut self.io.bluetooth {
                radio.add_service(&service);
            }
        }
        for filter in staging.refresh_filters.drain(..) {
            self.shared.refresh_filters.add(filter);
        }
        for chrome in staging.chromes.drain(..) {
            self.add_chrome(chrome);
        }
        let caps = self.shared.caps;
        let inputs = staging.inputs.drain(..);
        let made = inputs.filter(|input| input.need.is_none_or(|need| need.met(caps)));
        self.shared.inputs.extend(made);
        // Recognizers are made once, their makers dropped; listed among
        // the inputs, to bind.
        for spec in staging.gestures.drain(..) {
            let recognizer = (spec.factory)();
            let mut listed = paperpaw_sdk::keys::InputSpec::new(spec.id.clone(), spec.title);
            listed.kind = paperpaw_sdk::keys::InputKind::Gesture;
            if caps.touch {
                self.shared.inputs.push(listed);
            }
            let id = spec.id;
            let fingers = Vec::new();
            let made = gestures::PluginRecognizer {
                id,
                recognizer,
                fingers,
            };
            self.input.recognizers.push(made);
        }
        for spec in staging.works.drain(..) {
            self.works.add_spec(spec);
        }
        Ok(())
    }

    /// Actions bound by any key map preset or key map for a switch, for
    /// validation.
    pub(crate) fn preset_actions(&self) -> Vec<Id> {
        let mut all = Keymap::default();
        for (_, source) in self.keys.presets.iter().chain(&self.keys.when) {
            all.load(source, Origin::Preset, true);
        }
        all.actions().cloned().collect()
    }

    /// Builds the key map from all sources and the selected preset.
    /// Problems were reported when the sources were first loaded.
    pub(crate) fn rebuild_keymap(&mut self) {
        let mut keymap = Keymap::default();
        let preset = self
            .shared
            .settings
            .text(core_settings::KEYMAP_PRESET.as_str());
        if let Some((_, source)) = self
            .keys
            .presets
            .iter()
            .find(|(id, _)| id.as_str() == preset)
        {
            keymap.load(source, Origin::Preset, true);
        }
        for (_, source) in &self.keys.plugins {
            keymap.load(source, Origin::Plugin, true);
        }
        keymap.load(&self.keys.device, Origin::Device, true);
        for source in [&self.keys.user, &self.keys.shell] {
            keymap.load(source, Origin::User, false);
        }
        // After the plugins' other bindings, which they then override.
        for (setting, source) in &self.keys.when {
            if self.shared.settings.bool(setting.as_str()) {
                keymap.load(source, Origin::Plugin, true);
            }
        }
        keymap.shrink_to_fit();
        self.shared.keymap = keymap;
    }

    /// Redefines or resets a menu (see [`Command::SetMenu`]) and saves it,
    /// with the entries plugins contribute to it now that the user left
    /// out: those added later show in the user's list too (`[menus-known]`).
    pub(crate) fn set_menu(&mut self, menu: Id, entries: Option<Vec<Id>>) {
        let join = |ids: &[Id]| ids.iter().map(Id::as_str).collect::<Vec<_>>().join(", ");
        let value = entries.as_deref().map(join);
        let listed = entries.as_deref().unwrap_or_default();
        let known: Vec<Id> = self
            .shared
            .menus
            .contributed_ids(menu.as_str())
            .into_iter()
            .filter(|id| !listed.contains(id))
            .cloned()
            .collect();
        let known_value = join(&known);
        match entries {
            Some(ids) => {
                self.shared.menus.customize(menu.clone(), ids);
                self.shared.menus.set_known(menu.clone(), known);
            }
            None => self.shared.menus.reset(menu.as_str()),
        }
        self.update_config(&format_args!("menu `{menu}`"), |source| match &value {
            Some(value) => {
                let source = paperpaw_config::set(source, "menus", menu.as_str(), value);
                paperpaw_config::set(&source, KNOWN, menu.as_str(), &known_value)
            }
            None => {
                let source = paperpaw_config::remove(source, "menus", menu.as_str());
                paperpaw_config::remove(&source, KNOWN, menu.as_str())
            }
        });
        self.output
            .commands
            .push(events::MENU_CHANGED.emit(events::MenuChanged::new(menu.clone())));
    }

    /// Adds a font file of `plugin`, described by its header.
    pub(crate) fn add_font_file(&mut self, plugin: &Id, id: Id, path: String) {
        let source = WarningSource::Plugin(plugin.clone());
        if !paperpaw_text_outlines() {
            return self.warn(
                source,
                None,
                format!("{path}: this device only reads fonts made for it (.ppfp)"),
            );
        }
        let storage = self.shared.storage.as_ref();
        let info = paperpaw_sdk::text::sfnt::read_info(&mut |offset, buf| {
            storage.read_at(&path, offset, buf).unwrap_or(0)
        });
        match info {
            Some(info) => {
                if let Err(error) = self.shared.fonts.add_file(id, path, info) {
                    self.warn(source, None, format!("{error}"));
                }
            }
            None => self.warn(source, None, format!("{path}: not a font")),
        }
    }

    /// Adds a face of a family `plugin` names in a file: packs everywhere,
    /// scalable fonts where they are drawn.
    pub(crate) fn add_font_face(&mut self, plugin: &Id, face: paperpaw_sdk::FontFace) {
        let source = WarningSource::Plugin(plugin.clone());
        if !face.is_pack() && !paperpaw_text_outlines() {
            let message = format!(
                "{}: this device only reads fonts made for it (.ppfp)",
                face.path
            );
            return self.warn(source, None, message);
        }
        if let Err(error) = self.shared.fonts.add_face(face) {
            self.warn(source, None, format!("{error}"));
        }
    }

    /// Chooses the reading font family from `core.reading-font` and loads
    /// its files.
    pub(crate) fn apply_reading_font(&mut self) {
        let chosen = self
            .shared
            .settings
            .text(core_settings::READING_FONT.as_str())
            .to_string();
        let family = (chosen != core_settings::DEFAULT_FONT
            && self.shared.fonts.families().contains(&chosen.as_str()))
        .then_some(chosen.as_str());
        self.shared.fonts.set_family(family);
        for path in self.shared.fonts.load(&self.shared.storage) {
            self.warn_runtime(format!("{path}: font could not be loaded"));
        }
        self.fit_font_size();
    }

    /// The sizes of book text offered as far as the reading font has them:
    /// a font pack's largest (built into the firmware, kept small) bounds
    /// them, a scalable font has every size. A size chosen past it gives
    /// way to the largest.
    pub(crate) fn fit_font_size(&mut self) {
        let (min, max) = FONT_SIZES;
        let dpi = f32::from(self.shared.device.display.dpi.max(1));
        let largest = self
            .shared
            .fonts
            .role(paperpaw_sdk::builtin::fonts::READING.as_str())
            .and_then(|font| font.largest_size());
        // No `round` without std.
        let most = largest.map_or(max, |px| ((px * 72.0 / dpi + 0.5) as i64).clamp(min, max));
        let id = core_settings::FONT_SIZE;
        self.shared.settings.set_range(id.as_str(), min, most);
        self.font_size_changed();
    }

    /// A font size past the largest the reading font has (set before, or
    /// a book's own) gives way to the largest; at the largest of a font
    /// pack, the help says where larger ones are found.
    pub(crate) fn font_size_changed(&mut self) {
        let id = core_settings::FONT_SIZE.static_str();
        let settings = &mut self.shared.settings;
        let most = match settings.spec(id).map(|spec| &spec.kind) {
            Some(paperpaw_sdk::SettingKind::Int { max, .. }) => *max,
            _ => return,
        };
        let size = settings.int(id);
        let largest = most < FONT_SIZES.1 && size >= most;
        let key = if largest {
            "core.setting.font-size.largest"
        } else {
            "core.setting.font-size.help"
        };
        settings.set_description(id, key);
        if size > most {
            let value = paperpaw_sdk::SettingValue::Int(most);
            let id = core_settings::FONT_SIZE.id().clone();
            self.output.commands.push(Command::SetSetting(id, value));
        }
    }

    /// Chooses what the status lines are in from `core.status-font` and
    /// loads its files.
    pub(crate) fn apply_status_font(&mut self) {
        let chosen = self
            .shared
            .settings
            .text(core_settings::STATUS_FONT.as_str())
            .to_string();
        self.shared.fonts.set_status(&chosen);
        for path in self.shared.fonts.load(&self.shared.storage) {
            self.warn_runtime(format!("{path}: font could not be loaded"));
        }
    }

    /// Loads the data plugin in `folder` while running (see
    /// [`Command::LoadPlugin`](paperpaw_sdk::Command::LoadPlugin)).
    pub(crate) fn load_plugin(&mut self, folder: &str) {
        if self.register_folder(folder) {
            self.plugins_changed();
        }
    }

    /// Registers the data plugin in `folder` (switched off where the
    /// configuration says so) and applies its defaults; `false`, with a
    /// warning, if it could not be.
    fn register_folder(&mut self, folder: &str) -> bool {
        let storage = self.shared.storage.clone();
        let plugin = match crate::data_plugin::load_folder(&storage, folder) {
            Ok(plugin) => plugin,
            Err(problem) => {
                self.warn_runtime(problem);
                return false;
            }
        };
        for problem in plugin.take_problems() {
            self.warn_runtime(problem);
        }
        let manifest = plugin.manifest();
        let id = manifest.id.clone();
        if self.shared.plugins.get(id.as_str()).is_some() {
            let problem = "a plugin with this id is already loaded".into();
            self.warn(WarningSource::Plugin(id), None, problem);
            return false;
        }
        let off = read_config(storage.as_ref())
            .ok()
            .flatten()
            .is_some_and(|c| crate::config::plugin_switches(&c, false).contains(&id.to_string()));
        if plugin.has_code() {
            self.add_code_plugin_later(&plugin, off);
        } else if off {
            plugin.keep_listing_only();
            self.register_off(&plugin, manifest);
        } else {
            self.register_plugin(&plugin, manifest);
            if let Some(info) = self.shared.plugins.get_mut(id.as_str()) {
                info.kind = paperpaw_sdk::PluginKind::Data;
            }
        }
        self.shared
            .data_folders
            .push((id, plugin.folder().to_string()));
        self.apply_defaults(&[]);
        true
    }

    /// Removes a data plugin while running (see
    /// [`Command::RemovePlugin`](paperpaw_sdk::Command::RemovePlugin)).
    pub(crate) fn remove_plugin(&mut self, id: &Id) {
        let Some(folder) = self.data_folder(id) else {
            return self.warn_runtime(format!("`{id}` is no data plugin; not removed"));
        };
        if let Err(error) = self.shared.storage.remove_dir_all(&folder) {
            return self.warn_runtime(format!("{folder}: not removed ({error:?})"));
        }
        self.unload_plugin(id);
        self.plugins_changed();
    }

    /// The folder of the data plugin `id` loaded (running or switched
    /// off).
    fn data_folder(&self, id: &Id) -> Option<String> {
        self.shared
            .data_folders
            .iter()
            .find(|(p, _)| p == id)
            .map(|(_, folder)| folder.clone())
    }

    /// Forgets the data plugin `id`: its folder, fonts and presets, and
    /// it in the list of plugins. What was chosen from it stays chosen
    /// until [`plugins_changed`](Self::plugins_changed) finds it no longer
    /// offered.
    fn unload_plugin(&mut self, id: &Id) {
        self.shared.data_folders.retain(|(p, _)| p != id);
        self.shared.fonts.remove_plugin(id.as_str());
        // Its key maps and stylesheets; the key map is built anew once the
        // plugins changed (`plugins_changed`).
        self.keys.plugins.retain(|(p, _)| p != id);
        self.keys
            .presets
            .retain(|(preset, _)| preset.namespace() != id.as_str());
        let mut owners = self.stylesheet_owners.iter();
        self.shared
            .stylesheets
            .retain(|_| owners.next().is_some_and(|owner| owner != id));
        self.stylesheet_owners.retain(|owner| owner != id);
        self.shared
            .settings
            .presets_mut()
            .remove_plugin(id.as_str());
        // A code plugin's code runs until the restart: it stays listed as
        // stopping then.
        match self.shared.plugins.get_mut(id.as_str()) {
            Some(info)
                if info.kind == paperpaw_sdk::PluginKind::Code
                    && info.state == PluginState::Running =>
            {
                info.enabled = false;
            }
            _ => {
                self.shared.plugins.remove(id.as_str());
            }
        }
    }

    /// A font chosen that is no longer offered gives way to the default.
    fn keep_offered_choices(&mut self) {
        for setting in [core_settings::READING_FONT, core_settings::STATUS_FONT] {
            let chosen = String::from(self.shared.settings.text(setting.as_str()));
            let value = paperpaw_sdk::SettingValue::Text(chosen.into());
            let offered = self
                .shared
                .settings
                .spec(setting.as_str())
                .is_some_and(|spec| spec.kind.accepts(&value));
            if !offered {
                self.reset_setting(setting.id());
            }
        }
    }

    /// Installs the data plugin `id` downloaded into `folder` (see
    /// [`Command::InstallPlugin`](paperpaw_sdk::Command::InstallPlugin)):
    /// the folder takes the place of every version of it on the storage,
    /// loaded or not, and is loaded at once.
    pub(crate) fn install_plugin(&mut self, id: &Id, folder: &str) {
        let dirs = self.shared.plugin_dirs.clone();
        // Loaded without a folder: built in, not to be replaced.
        let built_in =
            self.shared.plugins.get(id.as_str()).is_some() && self.data_folder(id).is_none();
        let Some(placed) = self.place_plugin(id, folder, &dirs, built_in) else {
            return;
        };
        // The new version in place: the old ones go, what was chosen from
        // them stays where the new one offers it too; what its installer
        // keeps for it (its code named, where it came from) is kept now.
        placed.clear(self.shared.storage.as_ref());
        paperpaw_sdk::install::marks::settle(
            self.shared.storage.as_ref(),
            id.as_str(),
            &placed.target,
        );
        // A code plugin's code runs until the restart: the new version
        // takes over then.
        if let Some(info) = self.shared.plugins.get_mut(id.as_str())
            && info.kind == paperpaw_sdk::PluginKind::Code
            && info.state == PluginState::Running
        {
            info.update = Some(placed.version);
            info.enabled = true;
            self.shared.data_folders.retain(|(p, _)| p != id);
            self.shared.data_folders.push((id.clone(), placed.target));
            return self.plugins_changed();
        }
        if self.shared.plugins.get(id.as_str()).is_some() {
            self.unload_plugin(id);
        }
        // Changed even where the new one does not load: the old ones are
        // gone.
        self.register_folder(&placed.target);
        self.plugins_changed();
    }

    /// Installs the plugins the shell put into folders before the start
    /// (see [`KernelBuilder::install_plugin`](crate::KernelBuilder::install_plugin)),
    /// before the plugins in `dirs` are found; not over the plugins built
    /// in (`built_in`).
    pub(crate) fn install_staged(
        &mut self,
        staged: Vec<(Id, String)>,
        dirs: &[String],
        built_in: &[Id],
    ) {
        for (id, folder) in staged {
            let ours = built_in.contains(&id);
            if let Some(placed) = self.place_plugin(&id, &folder, dirs, ours) {
                placed.clear(self.shared.storage.as_ref());
                let storage = self.shared.storage.as_ref();
                paperpaw_sdk::install::marks::settle(storage, id.as_str(), &placed.target);
            }
        }
    }

    /// Puts the plugin `id` in `folder` in the place of every version of
    /// it in `dirs` (and loaded); `None` when it is refused, said and its
    /// folder removed. A plugin `built_in` is not replaced, and one that
    /// would not register is refused before anything changes, so the
    /// version before stays.
    fn place_plugin(
        &mut self,
        id: &Id,
        folder: &str,
        dirs: &[String],
        built_in: bool,
    ) -> Option<Placed> {
        let storage = self.shared.storage.clone();
        let folder = folder.trim_end_matches('/');
        // The folder it is installed in: nothing to put in its place, and
        // nothing to remove.
        let dir = paperpaw_sdk::files::parent(folder);
        let target = paperpaw_sdk::files::join(dir, id.as_str());
        if paperpaw_sdk::files::same_path(storage.as_ref(), folder, &target) {
            let problem = format!("{folder}: is where `{id}` is installed; nothing to install");
            self.warn(WarningSource::Plugin(id.clone()), None, problem);
            return None;
        }
        let refuse = |kernel: &mut Kernel, problem: String| {
            let _ = storage.remove_dir_all(folder);
            kernel.warn(WarningSource::Plugin(id.clone()), None, problem);
            None
        };
        if built_in {
            return refuse(self, format!("`{id}` is built in; {folder} not installed"));
        }
        // What it says it is, before anything changes.
        let found = data_plugin::folder_plugin_id(storage.as_ref(), folder, id.as_str());
        if found.as_deref() != Some(id.as_str()) {
            return refuse(
                self,
                format!("{folder}: not the plugin `{id}`; not installed"),
            );
        }
        let plugin = match data_plugin::load_folder_as(&storage, folder, id.as_str()) {
            Ok(plugin) => plugin,
            Err(problem) => return refuse(self, problem),
        };
        for problem in plugin.take_problems() {
            self.warn_runtime(problem);
        }
        let version = plugin.manifest().version;
        // Registered aside first (its translations, key map, settings): one
        // that would fail leaves the version before in place.
        let mut registrar = Registrar::new(id.clone());
        let staged = plugin.register(&mut registrar).and_then(|()| {
            registry::stage(id, registrar.into_contributions(), &mut Staging::default())
        });
        drop(plugin);
        if let Err(error) = staged {
            return refuse(self, format!("{folder}: not installed: {error}"));
        }
        // Every version of it there is: loaded, switched off, or refused at
        // start.
        let mut old: Vec<String> = self.data_folder(id).into_iter().collect();
        for dir in dirs {
            for path in data_plugin::folders_of(storage.as_ref(), dir, id.as_str()) {
                if !old.contains(&path) {
                    old.push(path);
                }
            }
        }
        // Its own folder, named as it, in the plugin folder it was
        // downloaded into; one there of another plugin is left alone.
        let taken =
            storage.exists(&target) || paperpaw_sdk::files::is_dir(storage.as_ref(), &target);
        if taken && !old.contains(&target) {
            return refuse(
                self,
                format!("{target}: holds another plugin; `{id}` not installed"),
            );
        }
        // The version before steps aside until the new one is in place.
        let aside = paperpaw_sdk::files::join(dir, &format!(".{id}.old"));
        let _ = storage.remove_dir_all(&aside);
        let moved = old.contains(&target) && storage.rename(&target, &aside).is_ok();
        if old.contains(&target) && !moved {
            return refuse(self, format!("{target}: could not be replaced"));
        }
        if let Err(error) = storage.rename(folder, &target) {
            if moved {
                let _ = storage.rename(&aside, &target);
            }
            return refuse(self, format!("{target}: not installed ({error:?})"));
        }
        old.retain(|path| *path != target);
        Some(Placed {
            target,
            old,
            aside,
            version,
        })
    }

    /// After a plugin was loaded or removed while running: the choices of
    /// fonts and key map presets (a font chosen that is no longer offered
    /// giving way), menu labels, fonts and keys anew.
    fn plugins_changed(&mut self) {
        self.offer_choices();
        self.keep_offered_choices();
        self.apply_reading_font();
        self.apply_status_font();
        self.rebuild_keymap();
        self.output.commands.push(events::PLUGINS_CHANGED.emit(()));
    }

    /// Releases memory reserved while plugins registered: on small devices
    /// every kilobyte counts, and registries are not modified afterwards.
    pub(crate) fn shrink_to_fit(&mut self) {
        self.shared.registry.shrink_to_fit();
        self.keys.plugins.shrink_to_fit();
        self.stylesheet_owners.shrink_to_fit();
        self.keys.presets.shrink_to_fit();
        self.keys.when.shrink_to_fit();
        self.keys.user.shrink_to_fit();
        self.keys.shell.shrink_to_fit();
        self.subscriptions.shrink_to_fit();
        self.action_observers.shrink_to_fit();
        self.event_observers.shrink_to_fit();
        self.shared.plugins.shrink_to_fit();
        self.warnings.shrink_to_fit();
        let shared = &mut self.shared;
        shared.settings.shrink_to_fit();
        shared.menus.shrink_to_fit();
        shared.actions.shrink_to_fit();
        shared.fonts.shrink_to_fit();
        shared.formats.shrink_to_fit();
        shared.hyphenation.shrink_to_fit();
        shared.status_items.shrink_to_fit();
        shared.widgets.shrink_to_fit();
        shared.setup.shrink_to_fit();
        shared.services.shrink_to_fit();
        shared.text_filters.shrink_to_fit();
        shared.decorators.shrink_to_fit();
        shared.page_layers.shrink_to_fit();
        shared.lookup.shrink_to_fit();
        shared.voices.shrink_to_fit();
        shared.readers.shrink_to_fit();
        shared.input_filters.shrink_to_fit();
        shared.inputs.shrink_to_fit();
        shared.refresh_filters.shrink_to_fit();
        shared.stylesheets.shrink_to_fit();
        shared.translator.shrink_to_fit();
    }
}

/// A plugin put in its place, and what is left of the versions before.
struct Placed {
    /// Its folder.
    target: String,
    /// The folders of other versions of it.
    old: Vec<String>,
    /// Where the version before stepped aside.
    aside: String,
    /// Its version.
    version: paperpaw_sdk::Version,
}

impl Placed {
    /// Removes the versions before.
    fn clear(&self, storage: &dyn Storage) {
        for path in &self.old {
            let _ = storage.remove_dir_all(path);
        }
        let _ = storage.remove_dir_all(&self.aside);
    }
}
