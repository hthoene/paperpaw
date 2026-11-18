//! The kernel made with nothing registered, its parts built in place.

#[allow(
    unused_imports,
    reason = "a part of the builder, which sees all of the kernel; not every part uses every name"
)]
use super::*;

impl BuilderParts {
    /// The kernel with nothing registered: its `framebuffer`, its storage
    /// (metered, and what a call reads kept for the rest of it), what the
    /// shell said it does.
    pub(super) fn new_kernel(&mut self, framebuffer: Framebuffer) -> Kernel {
        let caps = self.device_caps();
        Kernel {
            shared: self.new_shared(caps),
            frame: self.new_frame_state(),
            input: Self::new_input_state(),
            io: self.new_io_state(caps),
            keys: plugins::KeySources::default(),
            stylesheet_owners: Vec::new(),
            timing: crate::timing::Timing::new(self.busy_mark_ms),
            framebuffer,
            subscriptions: crate::registry::Subscriptions::default(),
            core: crate::core_plugin::State::default(),
            data_actions: Vec::new(),
            offered_down_to: 0,
            action_params: Params::new(),
            show_pending: false,
            notice: None,
            works: work::Queue::new(match self.device.memory {
                paperpaw_platform::MemoryClass::Large => 32,
                _ => 8,
            }),
            stack: Vec::new(),
            output: Output::default(),
            warnings: Vec::new(),
            restart: false,
            sleep: false,
            power_off: false,
            usb_drive: false,
            developer: crate::developer_link::Link::default(),
            suspended_at: None,
            free_count: crate::free_count::FreeCount::default(),
            hint: hints::HintRow::new(self.button_row),
            busy_mark: self.busy_mark,
            withheld: Vec::new(),
            withheld_trusted: Vec::new(),
            withheld_own: Vec::new(),
            trusted: Vec::new(),
            from_guard: false,
            from_guard_own: false,
            own_trusted: Vec::new(),
            waiting_notices: crate::notices::Waiting::new(self.device.memory),
            last_activity: Instant(0),
            native_tick_at: None,
            last_user_input: None,
            radio_requests: alloc::rc::Rc::default(),
            firmware: self.firmware_host.take().map(|host| {
                // The driver is made at the first firmware or plugin command.
                let cell = crate::native_host::firmware_cell::FirmwareCell::new(host);
                if let Some(plugins) = self.plugin_host.take() {
                    cell.set_plugin_host(plugins);
                }
                alloc::rc::Rc::new(cell)
            }),
            shown_since: Instant(0),
            scope: None,
            screens_changed: false,
            raw: crate::realtime::RawKeys::default(),
            lease: None,
            holds: crate::awake::Holds::default(),
            preview_owner: None,
            next_serial: 0,
            read_top: None,
            task: None,
            tasks: Vec::new(),
            config_unreadable: false,
            card_trouble: None,
            after_sleep: false,
            storage_kept: false,
            secrets_unreadable: false,
            secrets_opened: false,
            secrets_wired: None,
            secrets_left: false,
            #[cfg(feature = "bluetooth")]
            pairing_nonce: None,
            plugin_defaults: Vec::new(),
            late_device_defaults: Vec::new(),
            late_config: Vec::new(),
            native_registering: true,
            native_catalogs_added: false,
            native_batch: None,
            native_clock: None,
            native_times: crate::native_host::NativeTimes::default(),
            plugin_choices: Vec::new(),
        }
    }

    /// What the shell said it does, and what follows from the device.
    fn device_caps(&self) -> Capabilities {
        let display = self.device.display;
        let mut caps = self.caps;
        caps.touch = self.device.input.touch;
        caps.black_and_white = display.format == paperpaw_platform::PixelFormat::Mono1;
        caps.firmware = self.firmware.is_some();
        caps.button_row = !self.button_row.is_empty();
        with_buttons(caps, &self.device.input)
    }

    /// The shared part of a new kernel, its storage too. Inlined into
    /// [`Self::new_kernel`] like the parts below: the kernel is built in
    /// place, not in parts on the stack.
    #[inline(always)]
    fn new_shared(&mut self, caps: Capabilities) -> Shared {
        let display = self.device.display;
        let links = self.bluetooth_links;
        let meter = meter::Meter::new(self.clock);
        let (storage, recall) = self.new_storage(&meter);
        // Data plugins' hyphenation tables are read from the card.
        let mut hyphenation = Hyphenation::new();
        hyphenation.set_storage(storage.clone());
        let translator = self.new_translator();
        let font_removals =
            alloc::rc::Rc::new(crate::native_host::font_removals::FontRemovals::new());
        Shared {
            now: Instant::default(),
            native_events: None,
            native_instances: crate::native_host::native_instances::NativeInstances::default(),
            native_input: crate::native_host::ui_input::InputFilters::new(),
            native_filters: None,
            native_input_refused: false,
            native_depth: None,
            registry: Registry::default(),
            meter,
            device: self.device,
            settings: crate::native_host::host_memos::Watched::new(
                Settings::new(),
                alloc::rc::Rc::clone(&font_removals),
                crate::native_host::host_memos::forget_places,
            ),
            translator,
            fonts: crate::native_host::host_memos::Watched::new(
                Fonts::new(self.glyph_cache)
                    .with_pack_limits(paperpaw_sdk::PackLimits::for_memory(self.device.memory)),
                alloc::rc::Rc::clone(&font_removals),
                crate::native_host::host_memos::forget_roles,
            ),
            stack_rows: alloc::rc::Rc::new(Default::default()),
            key_guard_warned: Default::default(),
            font_removals,
            registry_generation: alloc::rc::Rc::new(
                crate::native_host::registry_generation::RegistryGeneration::new(),
            ),
            formats: Formats::new(),
            hyphenation,
            hyphen_feed: crate::hyphen_feed::HyphenFeed::new(),
            menus: Menus::new(),
            actions: Actions::new(),
            plugins: Plugins::new(),
            status: SystemStatus::default(),
            usage: paperpaw_sdk::usage::Usage::default(),
            status_items: StatusItems::new(),
            widgets: crate::kit::widgets::Widgets::new(),
            setup: crate::kit::setup::SetupSteps::new(),
            services: paperpaw_sdk::services::Services::new(),
            decorators: crate::kit::decorations::Decorators::new(),
            stylesheets: Vec::new(),
            book_data: Vec::new(),
            caps,
            wifi: caps.wifi.then(paperpaw_sdk::wifi::Wifi::default),
            bluetooth: caps
                .bluetooth
                .then(|| paperpaw_sdk::bluetooth::Bluetooth::new(links)),
            firmware: self.firmware.take(),
            native: None,
            native_safe_mode: self.safe_mode.clone(),
            after_trial: self.after_trial.take(),
            power: paperpaw_sdk::power::Power::default(),
            work_states: paperpaw_sdk::work::WorkStates::default(),
            reader: false,
            clipboard: alloc::rc::Rc::default(),
            system_clipboard: false,
            last_input: paperpaw_sdk::keys::InputMode::at_start(&self.device.input),
            stack: Vec::new(),
            content: Rect::new(0, 0, display.width, display.height),
            slots: Vec::new(),
            font_dirs: Vec::new(),
            data_folders: Vec::new(),
            plugin_dirs: Vec::new(),
            language_seed: self.language_seed,
            catalog_device: self.catalog_device,
            secrets: Default::default(),
            random: self.random,
            device_log: crate::native_host::device_log::DeviceLog::new(self.device_log),
            memory: self.memory,
            started: self.started,
            slept_for: self.slept_for,
            panel: self.panel,
            keys_held: Vec::new(),
            room: paperpaw_sdk::realtime::Room::default(),
            storage,
            recall,
            keymap: Keymap::default(),
            look: (crate::kit::ui::look::Look::none(), None),
        }
    }

    /// The storage the kernel reads through: metered, and what a call
    /// reads is kept for the rest of it.
    #[inline(always)]
    fn new_storage(
        &mut self,
        meter: &alloc::rc::Rc<meter::Meter>,
    ) -> (alloc::rc::Rc<dyn Storage>, alloc::rc::Rc<recall::Recall>) {
        // What a call reads, kept for the rest of it (more where memory is
        // plentiful).
        let inner = self.storage.take().unwrap_or_else(|| Box::new(NoStorage));
        let budget = match self.device.memory {
            paperpaw_platform::MemoryClass::Large => 256 * 1024,
            // Small, and any class to come: the frugal side.
            _ => 16 * 1024,
        };
        let recall = alloc::rc::Rc::new(recall::Recall::new(budget, inner.ignores_case()));
        let storage: alloc::rc::Rc<dyn Storage> = alloc::rc::Rc::new(meter::Metered {
            inner: Box::new(recall::Recalled {
                inner,
                recall: recall.clone(),
            }),
            meter: meter.clone(),
        });
        (storage, recall)
    }

    /// The translator with the catalogs built into the firmware.
    #[inline(always)]
    fn new_translator(&self) -> Translator {
        let mut translator = Translator::new();
        translator.fixed_indexes(self.catalog_indexes);
        for table in self.built_catalogs {
            translator.built_language(table);
        }
        for tables in &self.built_messages {
            for table in *tables {
                translator.built_messages(table);
            }
        }
        if let Some(listing) = self.language_seed.and_then(|seed| seed.listing()) {
            translator.add(listing);
        }
        translator
    }

    #[inline(always)]
    fn new_frame_state(&self) -> drawing::FrameState {
        let display = self.device.display;
        drawing::FrameState {
            pending: None,
            pending_weight: 0,
            unrendered: None,
            rendered: None,
            stop: None,
            stopped: false,
            row_align: self.row_align,
            band: self
                .frame_rows
                .map(|rows| u32::from(rows) * u32::from(display.width)),
            pictures_shown: false,
            pictures_on: None,
            refreshed_on: None,
            again: false,
            screenshot: None,
            mark: None,
            refresh: RefreshPolicy::after_start(),
            followup: None,
            followup_frame: None,
            gray_later: crate::gray_later::GrayLater::default(),
        }
    }

    #[inline(always)]
    fn new_input_state() -> gestures::InputState {
        gestures::InputState {
            recognizer: Recognizer::default(),
            captured: Vec::new(),
            edge_touch: None,
            edge_pending: None,
            settling: false,
            stale_before: None,
            shown_at: [None; 2],
            dropped_buttons: Vec::new(),
            dropped_pointers: Vec::new(),
            swapped: Vec::new(),
            pending: None,
            pressed_on: None,
            thresholds: core::cell::Cell::new(None),
        }
    }

    #[inline(always)]
    fn new_io_state(&self, caps: Capabilities) -> network::IoState {
        #[cfg(feature = "bluetooth")]
        let links = self.bluetooth_links;
        network::IoState {
            http: io::http::Http::for_memory(self.device.memory),
            servers: io::servers::Servers::default(),
            sockets: io::sockets::Sockets::new(caps.tls_sessions),
            speech: io::speech::Speech::new(caps.speech),
            clipboard: io::clipboard::Clipboard::new(caps.system_clipboard),
            wifi_calls: Vec::new(),
            #[cfg(feature = "bluetooth")]
            bluetooth: caps.bluetooth.then(|| io::bluetooth::Radio::new(links)),
            firmware_calls: Vec::new(),
            network_waiting: None,
            network_busy: false,
            focus_spoken: false,
            focus_told: None,
        }
    }
}
