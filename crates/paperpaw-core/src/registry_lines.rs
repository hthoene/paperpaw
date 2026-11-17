//! The registry as text lines ([`Kernel::registry_lines`]): what the plugins
//! of a build registered with the kernel, the old way and as interface-7
//! declarations, for the tests that pin it while the kernel leaves the old
//! plugin API (#244 item 3: the registry golden of the safety net).
//!
//! A line is `<part> <key> | <what>`. The lines of a part are sorted by
//! their key alone and keep, within a key, the kernel's own order: the order
//! in which screens are tried for a slot, subscribers hear an event,
//! handlers stand in an action's place, entries stand in a menu. Sorting
//! whole lines would hide just that.
//!
//! Not listed, because their stores name no entries: text, input and refresh
//! filters, decorators, page layers and lookup providers are printed as their
//! stores print themselves (ids and orders); a handler put in the place of
//! an action no plugin registered; a status item's and a menu entry's state
//! (closures). Hyphenation tables are not listed.
//!
//! Not part of any interface: the `testing` feature only.
use crate::Kernel;
use crate::abi7::context::registry_views::{Asker, RegistryView as _};
use crate::native_host::declaration_store::Kind;
use crate::native_host::registrations::{Record, record_decode_prefix};
use crate::native_host::registry_service::Sources;
use alloc::{format, string::String, vec::Vec};
use core::fmt::{Debug, Display};
use core::ops::ControlFlow;
use paperpaw_sdk::{ScreenMaker as _, keys::KeyBindings as _};

/// The longest a printed value may be before it is cut to its start, its
/// length and its hash (an icon's bitmap, a long list).
const VALUE_MAX: usize = 400;

/// The lines so far, and those of the part being made.
#[derive(Default)]
struct Lines {
    out: Vec<String>,
    part: Vec<(String, String)>,
}

impl Lines {
    fn add(&mut self, key: impl Display, what: impl Display) {
        self.part.push((format!("{key}"), cut(format!("{what}"))));
    }

    /// The part is over: its lines by key, those of one key as they came.
    fn end(&mut self, part: &str) {
        self.part.sort_by(|a, b| a.0.cmp(&b.0));
        for (key, what) in self.part.drain(..) {
            let sep = if key.is_empty() { "" } else { " " };
            self.out.push(format!("{part}{sep}{key} | {what}"));
        }
    }
}

/// `text`, or where it is long its start, its length and its hash.
fn cut(text: String) -> String {
    if text.len() <= VALUE_MAX {
        return text;
    }
    let mut end = VALUE_MAX / 2;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let hash = paperpaw_sdk::digest::fnv1a64(text.as_bytes());
    format!("{}… ({} bytes, {hash:016x})", &text[..end], text.len())
}

/// The rows `core.registry` gives for `op` in `scope`, each as its record,
/// in the order given. A row that cannot be framed or read is left out.
fn rows_of(
    sources: &Sources<'_>,
    op: u32,
    scope: &str,
    flags: u32,
    each: &mut dyn FnMut(Record<'_>),
) {
    let asker = Asker {
        owner: 0,
        plugin: "",
    };
    let _ = sources.rows(op, scope, flags, &asker, &mut |row| {
        let mut bytes = alloc::vec![0; row.framed_len().unwrap_or(0)];
        if row.encode(&mut bytes).is_ok()
            && let Ok((record, _)) = record_decode_prefix(&bytes)
        {
            each(record);
        }
        ControlFlow::Continue(())
    });
}

fn debug(value: &impl Debug) -> String {
    format!("{value:?}")
}

impl Kernel {
    /// Everything registered with the kernel, as text lines (see the
    /// module). Not part of any interface: the `testing` feature only.
    #[must_use]
    pub fn registry_lines(&self) -> Vec<String> {
        let mut lines = Lines::default();
        self.lines_of_plugins(&mut lines);
        self.lines_of_settings(&mut lines);
        self.lines_of_actions(&mut lines);
        self.lines_of_screens(&mut lines);
        self.lines_of_menus(&mut lines);
        self.lines_of_merged_menus(&mut lines);
        self.lines_of_status(&mut lines);
        self.lines_of_widgets(&mut lines);
        self.lines_of_reading(&mut lines);
        self.lines_of_keys(&mut lines);
        self.lines_of_events(&mut lines);
        self.lines_of_declarations(&mut lines);
        lines.out
    }

    fn lines_of_plugins(&self, lines: &mut Lines) {
        for info in self.shared.plugins.iter() {
            let manifest = &*info.manifest;
            lines.add(
                "",
                format_args!(
                    "{} {} {:?} enabled={} {:?}",
                    manifest.id, manifest.version, info.state, info.enabled, info.kind
                ),
            );
        }
        lines.end("plugin");
        for (id, key, _) in self.shared.work_states.iter() {
            lines.add(id, key);
        }
        lines.end("work");
        for id in self.shared.services.ids() {
            lines.add("", id);
        }
        lines.end("service");
        for point in self.shared.services.points() {
            lines.add("", debug(point));
        }
        lines.end("service-point");
        for event in self.shared.services.events() {
            lines.add("", debug(event));
        }
        lines.end("service-event");
        let services = &self.shared.services;
        let points: Vec<_> = services.contributed_points().collect();
        for point in points {
            for (plugin, what) in services.contributions(point.as_str()) {
                lines.add(point, format_args!("{plugin} {what}"));
            }
        }
        lines.end("service-contribution");
    }

    fn lines_of_settings(&self, lines: &mut Lines) {
        let settings = &self.shared.settings;
        for group in settings.groups() {
            lines.add("", debug(group));
        }
        lines.end("setting-group");
        for spec in settings.specs() {
            lines.add(
                &spec.group,
                format_args!(
                    "{} {:?} default={:?} {:?} scoped={} guarded={} secret={} kept={} title={}",
                    spec.id,
                    spec.kind,
                    spec.default,
                    spec.level,
                    spec.scoped,
                    spec.guarded,
                    spec.extra().secret,
                    spec.is_kept_on_reset(),
                    spec.title,
                ),
            );
        }
        lines.end("setting");
        for preset in settings.presets().iter() {
            lines.add(
                &preset.kind,
                format_args!(
                    "{} title={} values={:?}",
                    preset.id, preset.title, preset.values
                ),
            );
        }
        lines.end("preset");
    }

    fn lines_of_actions(&self, lines: &mut Lines) {
        let actions = &self.shared.actions;
        for action in actions.iter() {
            lines.add(
                action.id(),
                format_args!(
                    "title={} sealed={} runs-itself={} native={:?}",
                    action.title_key(),
                    action.is_sealed(),
                    action.runs_itself(),
                    action.native().map(|(_, token)| token),
                ),
            );
        }
        lines.end("action");
    }

    fn lines_of_screens(&self, lines: &mut Lines) {
        let registry = &self.shared.registry;
        for (id, title) in registry.screens.list().iter() {
            lines.add(id, format_args!("title={title}"));
        }
        lines.end("screen");
        for (id, spec) in registry.slots.iter() {
            lines.add(
                id,
                format_args!(
                    "title={} bound={:?}",
                    spec.title,
                    registry
                        .resolve_slot(id.as_str())
                        .map(paperpaw_sdk::Id::as_str)
                ),
            );
        }
        lines.end("slot");
        // The offers as made, with their priority, then the order tried.
        for (slot, screen, priority) in registry.offers_made() {
            lines.add(slot, format_args!("{screen} {priority:?}"));
        }
        lines.end("slot-offer");
        for (id, _) in registry.slots.iter() {
            registry.each_slot_screen(id.as_str(), &mut |screen| {
                lines.add(id, screen);
                true
            });
        }
        lines.end("slot-order");
        // The same order as a list (`ScreenMaker::slot_screens`): code of
        // its own, which a view of the slot's screens asks.
        for (id, _) in registry.slots.iter() {
            for screen in paperpaw_sdk::ScreenMaker::slot_screens(registry, id.as_str()) {
                lines.add(id, screen);
            }
        }
        lines.end("slot-list");
        for (slot, screen) in registry.user_bindings() {
            lines.add(slot, screen);
        }
        lines.end("slot-user");
    }

    fn lines_of_menus(&self, lines: &mut Lines) {
        let menus = &self.shared.menus;
        for menu in menus.menu_ids() {
            for item in menus.contributed_ids(menu.as_str()) {
                lines.add(
                    menu,
                    format_args!(
                        "{item} order={:?} section={} hidden-by={:?}",
                        menus.order_of(menu.as_str(), item.as_str()),
                        menus.is_section(item.as_str()),
                        menus
                            .hidden_by(menu.as_str(), item.as_str())
                            .map(paperpaw_sdk::Id::as_str),
                    ),
                );
            }
        }
        lines.end("menu");
        for step in self.shared.setup.iter() {
            lines.add(
                "",
                format_args!(
                    "{} title={} order={} page={:?}",
                    step.id, step.title, step.order, step.page
                ),
            );
        }
        lines.end("setup-step");
    }

    fn lines_of_status(&self, lines: &mut Lines) {
        let status = &self.shared.status_items;
        for (id, title) in status.iter() {
            lines.add(
                "",
                format_args!(
                    "{id} title={title} priority={}",
                    status.priority(id.as_str())
                ),
            );
        }
        lines.end("status-item");
        for (id, title) in status.lines() {
            lines.add("", format_args!("{id} title={title}"));
        }
        lines.end("status-line");
        for place in status.places() {
            lines.add("", debug(place));
        }
        lines.end("status-place");
    }

    fn lines_of_widgets(&self, lines: &mut Lines) {
        let widgets = &self.shared.widgets;
        // The registered ones and those declared by native plugins: what the
        // gallery offers.
        for (id, title, kind) in widgets.everywhere() {
            lines.add(
                &id,
                format_args!(
                    "title={title} kind={kind} category={:?} cells={:?}",
                    widgets.category(id.as_str()),
                    widgets.cells(id.as_str()),
                ),
            );
        }
        lines.end("widget");
    }

    fn lines_of_reading(&self, lines: &mut Lines) {
        let shared = &self.shared;
        for format in shared.formats.iter() {
            lines.add(
                "",
                format_args!(
                    "{} title={} extensions={:?}",
                    format.id, format.title, format.extensions
                ),
            );
        }
        lines.end("format");
        for id in shared.fonts.ids() {
            lines.add("", id);
        }
        lines.end("font");
        for family in shared.fonts.families() {
            lines.add("", family);
        }
        lines.end("font-family");
        lines.add("", debug(&shared.decorators));
        lines.end("decorators");
        for sheet in &shared.stylesheets {
            let hash = paperpaw_sdk::digest::fnv1a64(sheet.as_bytes());
            lines.add("", format_args!("{} bytes {hash:016x}", sheet.len()));
        }
        lines.end("stylesheet");
        for data in &shared.book_data {
            lines.add("", debug(data));
        }
        lines.end("book-data");
    }

    /// Every menu as `core.registry` lists it with all its rows (ops 11 and
    /// 12): the entries plugins contributed and the rows natively declared
    /// plugins put into it, in the one order a view of the menu gets.
    fn lines_of_merged_menus(&self, lines: &mut Lines) {
        use crate::abi7::services_wire::core_registry_v1 as schema;
        // Op 12's flag for every row: those the user removed and the
        // hidden ones too.
        const ALL: u32 = 1;
        let env = self.shared.env(crate::Instant(0));
        let sources = crate::native_host::registry_service::Sources {
            env: &env,
            directory: self.shared.registry.native_declarations.as_deref(),
            generation: &self.shared.registry_generation,
        };
        let mut menus = Vec::new();
        rows_of(&sources, schema::OP_MENUS, "", 0, &mut |record| {
            if let Record::ServiceRegItem(menu) = record {
                menus.push(String::from(menu.id));
            }
        });
        for menu in &menus {
            rows_of(&sources, schema::OP_MENU, menu, ALL, &mut |record| {
                if let Record::ServiceRegMenuRow(row) = record {
                    lines.add(
                        menu,
                        format_args!(
                            "{} by={} order={} kind={} flags={:#x}",
                            row.item, row.by, row.order, row.kind, row.flags
                        ),
                    );
                }
            });
        }
        lines.end("menu-merged");
    }

    fn lines_of_keys(&self, lines: &mut Lines) {
        let keymap = &self.shared.keymap;
        let mut layers = Vec::new();
        let _ = keymap.each_layer(&mut |layer| {
            layers.push(String::from(layer));
            ControlFlow::Continue(())
        });
        // The layers as the kernel walks them: the sort of `key` loses it.
        for layer in &layers {
            lines.add("", layer);
        }
        lines.end("key-layer");
        for layer in &layers {
            for (input, action, origin) in keymap.bindings(layer) {
                lines.add(
                    layer,
                    format_args!(
                        "{input} -> {} {origin:?}",
                        action.as_ref().map_or("none", paperpaw_sdk::Id::as_str)
                    ),
                );
            }
        }
        lines.end("key");
        for (id, _) in &self.keys.presets {
            lines.add("", id);
        }
        lines.end("key-preset");
    }

    fn lines_of_events(&self, lines: &mut Lines) {
        // The subscribers of an event, in the order they hear it.
        for (event, plugin, _) in self.subscriptions.iter() {
            lines.add(event, plugin);
        }
        lines.end("subscription");
    }

    /// The declarations of interface-7 plugins, every record of every kind
    /// as the store holds it: by kind and plugin, a plugin's own in the
    /// order it declared them.
    fn lines_of_declarations(&self, lines: &mut Lines) {
        let Some(directory) = self.shared.registry.native_declarations.as_ref() else {
            return;
        };
        let Ok(store) = directory.store.try_borrow() else {
            lines.add("", "the declarations are in use");
            lines.end("native");
            return;
        };
        for kind in Kind::ALL {
            let mut keys = Vec::new();
            store.keys(kind, &mut |key| keys.push(key));
            for key in keys {
                let plugin = String::from(store.plugin_of(key).unwrap_or("?"));
                store.with_record(key, &mut |record| {
                    lines.add(format_args!("{kind:?} {plugin}"), debug(&record));
                });
            }
            lines.end("native");
        }
    }
}

#[cfg(test)]
#[path = "registry_lines_tests.rs"]
mod tests;
