//! How the kernel's shared state builds the [`Env`] plugins are called with.

use super::{Env, EnvParts, Instant, Metrics, Shared};

impl Shared {
    pub(crate) fn env(&self, now: Instant) -> Env<'_> {
        self.env_parts(now).with_screen_maker(&self.registry)
    }

    pub(crate) fn env_parts(&self, now: Instant) -> Env<'_> {
        Env::from(EnvParts {
            device: &self.device,
            now,
            metrics: Metrics::new(self.device.display.dpi),
            settings: &self.settings,
            i18n: &self.translator,
            fonts: &self.fonts,
            formats: &self.formats,
            hyphenation: &self.hyphenation,
            menus: &self.menus,
            actions: &self.actions,
            plugins: &self.plugins,
            status: &self.status,
            usage: &self.usage,
            plugin_usage: &self.meter.table,
            status_items: &self.status_items,
            widgets: &self.widgets,
            setup: &self.setup,
            services: &self.services,
            text_filters: &self.text_filters,
            decorators: &self.decorators,
            page_layers: &self.page_layers,
            lookup: &self.lookup,
            readers: &self.readers,
            stylesheets: &self.stylesheets,
            book_data: &self.book_data,
            book_fields: &self.book_fields,
            book_badges: &self.book_badges,
            caps: self.caps,
            wifi: self.wifi.as_ref(),
            bluetooth: self.bluetooth.as_ref(),
            firmware: self.firmware.as_ref(),
            native: self.native.as_ref(),
            power: &self.power,
            work: &self.work_states,
            reader: self.reader,
            clipboard: self.system_clipboard || !self.clipboard.is_empty(),
            last_input: self.last_input,
            inputs: &self.inputs,
            content: self.content,
            stack: &self.stack,
            slots: &self.slots,
            screens: self.registry.screens.list(),
            font_dirs: &self.font_dirs,
            catalog_device: self.catalog_device,
            secrets: &self.secrets,
            running: &*self.meter,
            random: self.random,
            memory: self.memory,
            clock: self.meter.clock(),
            started: self.started,
            slept_for: self.slept_for,
            panel: self.panel,
            keys_held: &self.keys_held,
            room: self.room,
            storage: self.storage.as_ref(),
            keys: &self.keymap,
            look: self.look.0,
        })
    }
}
