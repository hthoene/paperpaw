//! Building a kernel: what the shell says of its device, the plugins, the
//! configuration, then the kernel made from them ([`KernelBuilder`]).

#[allow(
    unused_imports,
    reason = "a part of the kernel, which sees all of it; not every part uses every name"
)]
use super::*;

/// Configures and creates a [`Kernel`].
///
/// What it holds lies on the heap: each call moves the builder, and a
/// shell that calls many would otherwise keep a copy of it on the stack
/// for each (the X4's start, whose stack is small).
pub struct KernelBuilder(Box<BuilderParts>);

/// What a [`KernelBuilder`] was told.
struct BuilderParts {
    device: DeviceInfo,
    plugins: Vec<Box<dyn Plugin>>,
    /// Plugins shipped switched off, until the user switches them on.
    off: Vec<String>,
    device_keymap: Option<Cow<'static, str>>,
    user_config: Option<String>,
    glyph_cache: usize,
    /// What the shell does; the device's touch, the firmware and the row
    /// of buttons come from their own settings at the end.
    caps: Capabilities,
    firmware: Option<Firmware>,
    code: Option<crate::code_plugin::CodePlugins>,
    safe_mode: Option<paperpaw_sdk::native::SafeMode>,
    /// Plugins the shell put into folders, to install before plugins load.
    staged: Vec<(Id, String)>,
    button_row: &'static [&'static str],
    frame_rows: Option<u16>,
    row_align: u16,
    storage: Option<Box<dyn Storage>>,
    card_trouble: Option<crate::CardTrouble>,
    clock: Option<fn() -> u64>,
    busy_mark: bool,
    setting_defaults: Vec<(String, String)>,
    plugin_dirs: Vec<String>,
    font_dirs: Vec<String>,
    catalog_device: Option<&'static str>,
    random: paperpaw_sdk::random::Random,
    memory: paperpaw_sdk::memory::Memory,
    started: paperpaw_sdk::power::Start,
    after_sleep: bool,
    panel: paperpaw_sdk::realtime::PanelInfo,
    /// Network settings kept in the program, by plugin.
    network_settings: &'static [crate::NetworkSettings],
    catalog_indexes: paperpaw_i18n::FixedIndexes,
    /// The most Bluetooth links the shell keeps; none without a radio.
    bluetooth_links: u8,
}

impl fmt::Debug for KernelBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("KernelBuilder")
            .field("device", &self.0.device.id)
            .field("plugins", &self.0.plugins.len())
            .finish_non_exhaustive()
    }
}

impl KernelBuilder {
    /// Adds a plugin. Plugins registered later win priority ties.
    pub fn plugin(mut self, plugin: impl Plugin + 'static) -> Self {
        self.0.plugins.push(Box::new(plugin));
        self
    }

    /// Adds a plugin that is off until the user switches it on (e.g. in
    /// the plugin manager), for features not everyone wants.
    pub fn plugin_off(mut self, plugin: impl Plugin + 'static) -> Self {
        self.0.off.push(String::from(plugin.manifest().id.as_str()));
        self.plugin(plugin)
    }

    /// Key bindings specific to the device (e.g. its physical buttons).
    pub fn device_keymap(mut self, source: impl Into<Cow<'static, str>>) -> Self {
        self.0.device_keymap = Some(source.into());
        self
    }

    /// The user's configuration file (`paperpaw.ini`). It overrides
    /// settings, slot bindings and key maps, and can disable plugins.
    /// The network settings of the networked plugins it is given, kept
    /// in the program rather than made at every start (see
    /// [`network_settings!`](crate::network_settings)); those of any
    /// other networked plugin are made as before.
    pub fn network_settings(mut self, settings: &'static [crate::NetworkSettings]) -> Self {
        self.0.network_settings = settings;
        self
    }

    /// The indexes of the translation catalogs built in, made when the
    /// program was built: such catalogs keep no index on the heap (see
    /// [`FixedIndexes`](paperpaw_i18n::FixedIndexes)).
    pub fn catalog_indexes(mut self, indexes: paperpaw_i18n::FixedIndexes) -> Self {
        self.0.catalog_indexes = indexes;
        self
    }

    pub fn user_config(mut self, source: impl Into<String>) -> Self {
        self.0.user_config = Some(source.into());
        self
    }

    /// The device's storage (books, settings, plugin data). Without it the
    /// kernel runs with an empty, read-only storage.
    pub fn storage(mut self, storage: impl Storage + 'static) -> Self {
        self.0.storage = Some(Box::new(storage));
        self
    }

    /// Like [`storage`](Self::storage), for a storage chosen at run time
    /// (e.g. the SD card, or none without one).
    pub fn storage_boxed(mut self, storage: Box<dyn Storage>) -> Self {
        self.0.storage = Some(storage);
        self
    }

    /// Shows a mark at once when an input opens or closes a screen that
    /// took long the last time (or was not opened yet), before the work:
    /// the pressed button's hint inverted, or a sign in the corner. For
    /// shells whose panel refreshes on its own while the processor works
    /// (the X4's), which handle [`Effects::proceed`]; it needs the
    /// [`clock`](KernelBuilder::clock) to know what is slow.
    pub fn busy_mark(mut self, on: bool) -> Self {
        self.0.busy_mark = on;
        self
    }

    /// A monotonic clock in microseconds, for measuring the time each
    /// plugin's code takes ([`paperpaw_sdk::usage::PluginUsage`]).
    pub fn clock(mut self, clock: fn() -> u64) -> Self {
        self.0.clock = Some(clock);
        self
    }

    /// Changes the default of a setting for this device, e.g. the
    /// two-button key map preset on a device with only two buttons. Users
    /// can still override it.
    pub fn setting_default<T: SettingType>(self, setting: &Setting<T>, value: T::Input) -> Self {
        let text = T::write(value).to_config_text();
        self.setting_default_text(setting.as_str(), &text)
    }

    /// As [`setting_default`](Self::setting_default), for a setting known only at run
    /// time (e.g. from a device profile's file), its value written as in
    /// `paperpaw.ini`.
    pub fn setting_default_text(mut self, id: &str, value: &str) -> Self {
        self.0
            .setting_defaults
            .push((String::from(id), String::from(value)));
        self
    }

    /// A folder on the storage with font files (OpenType/TrueType) to
    /// read with; they appear as one plugin, "font folder". Only for
    /// devices that render scalable fonts.
    pub fn font_dir(mut self, dir: impl Into<String>) -> Self {
        self.0.font_dirs.push(dir.into());
        self
    }

    /// A folder on the storage with data plugins (see `data_plugin`), one
    /// subfolder each. Without any, `plugins` is used.
    pub fn plugin_dir(mut self, dir: impl Into<String>) -> Self {
        self.0.plugin_dirs.push(dir.into());
        self
    }

    /// What plugin libraries call this kind of device, e.g. `x4`, `x4-pro`
    /// or `android` for paperpaw.org's (ADR 0023): its plugin store then
    /// offers what is made for it. Without, none is offered.
    pub fn catalog_device(mut self, name: &'static str) -> Self {
        self.0.catalog_device = Some(name);
        self
    }

    /// The platform's source of random bytes, for keys and tokens
    /// ([`paperpaw_sdk::random`]): `fill` fills a buffer and says whether
    /// it could. Without, nothing that needs one works (e.g. the transfer
    /// by browser).
    pub fn random(mut self, fill: fn(&mut [u8]) -> bool) -> Self {
        self.0.random = paperpaw_sdk::random::Random::new(fill);
        self
    }

    /// The platform's probe of memory ([`paperpaw_sdk::memory`]): `read`
    /// says what is free now, or `None` where it cannot tell, for
    /// diagnostics and plugins checking their room. Without, nothing is
    /// known.
    pub fn memory(mut self, read: fn() -> Option<paperpaw_sdk::memory::MemoryUse>) -> Self {
        self.0.memory = paperpaw_sdk::memory::Memory::new(read);
        self
    }

    /// How PaperPaw started this time ([`paperpaw_sdk::power::Start`]),
    /// for plugins: a wake, the power switched on, a restart by a
    /// watchdog. Without, it is not known.
    pub fn started(mut self, start: paperpaw_sdk::power::Start) -> Self {
        self.0.started = start;
        self
    }

    /// This start ends a sleep that restarted the device (the X4's cuts
    /// its power): [`Kernel::start`] sends [`events::RESUME`] after
    /// [`events::STARTED`], so that plugins hear the device wake as they do
    /// where it sleeps on, and [`events::SUSPEND`] and [`events::RESUME`]
    /// come in pairs. Without, it is not told.
    ///
    /// [`events::RESUME`]: paperpaw_sdk::builtin::events::RESUME
    /// [`events::STARTED`]: paperpaw_sdk::builtin::events::STARTED
    /// [`events::SUSPEND`]: paperpaw_sdk::builtin::events::SUSPEND
    pub fn after_sleep(mut self, after_sleep: bool) -> Self {
        self.0.after_sleep = after_sleep;
        self
    }

    /// What the panel can do ([`paperpaw_sdk::realtime::PanelInfo`]): its
    /// grays, refresh times known beforehand, whether frames overlap.
    /// Refreshes shown add their times ([`Kernel::panel_busy`]). Without,
    /// a panel of the display's grays, its times not known.
    pub fn panel(mut self, panel: paperpaw_sdk::realtime::PanelInfo) -> Self {
        self.0.panel = panel;
        self
    }

    /// PaperPaw manages Wi-Fi itself on this device (the shell answers
    /// [`Io::Wifi`] and reports with [`Kernel::wifi_event`]);
    /// it implies [`network`](Self::network). See [`paperpaw_sdk::wifi`].
    pub fn wifi(mut self, managed: bool) -> Self {
        self.0.caps.wifi = managed;
        self.0.caps.network |= managed;
        self
    }

    /// The shell drives a Bluetooth Low Energy radio for the kernel,
    /// keeping up to `max_links` links at once ([`Io::Bluetooth`],
    /// reported with [`Kernel::bluetooth_event`]); 0, the default, for
    /// none. See [`paperpaw_sdk::bluetooth`].
    pub fn bluetooth(mut self, max_links: u8) -> Self {
        self.0.caps.bluetooth = max_links > 0;
        self.0.bluetooth_links = max_links;
        self
    }

    /// The shell sets the device's front light as the settings
    /// [`settings::FRONTLIGHT`](core_settings::FRONTLIGHT) and
    /// [`settings::FRONTLIGHT_WARMTH`](core_settings::FRONTLIGHT_WARMTH)
    /// say; without, they are not offered.
    pub fn frontlight(mut self, settable: bool) -> Self {
        self.0.caps.frontlight = settable;
        self
    }

    /// PaperPaw loads native code plugins (see
    /// [`paperpaw_sdk::native`]): `loader` loads the code of each switched
    /// on at the start, files fit for `native` (which plugins see as
    /// [`Env::native`](paperpaw_sdk::Env)).
    pub fn code_plugins(mut self, native: Native, loader: Box<dyn CodeLoader>) -> Self {
        self.0.code = Some(crate::code_plugin::CodePlugins {
            native,
            loader,
            safe_mode: None,
        });
        self
    }

    /// The shell found no card it can use (see [`crate::CardTrouble`]):
    /// its storage is none, and the reader is told why at each start, in
    /// place of a guide whose choices could not be kept.
    pub fn card_trouble(mut self, trouble: crate::CardTrouble) -> Self {
        self.0.card_trouble = Some(trouble);
        self
    }

    /// This start goes without code plugins (safe mode): the shell found
    /// that the last ones did not get to their first frame, or the reader
    /// held the key for it. See [`paperpaw_sdk::native::SafeMode`]; the
    /// kernel fills in what it switched off.
    pub fn safe_mode(mut self, safe_mode: SafeMode) -> Self {
        self.0.safe_mode = Some(safe_mode);
        self
    }

    /// Installs the plugin `id` the shell put into `folder` (a folder of
    /// the plugin folder PaperPaw does not load, e.g. `plugins/.hello.part`:
    /// a plugin the web flasher wrote with the firmware) before plugins
    /// are found, so it loads at this start: in place of every version of
    /// it, as [`Command::InstallPlugin`](paperpaw_sdk::Command::InstallPlugin)
    /// installs one while PaperPaw runs. One that is not what it says is
    /// refused, with a warning, and its folder removed.
    pub fn install_plugin(mut self, id: Id, folder: impl Into<String>) -> Self {
        self.0.staged.push((id, folder.into()));
        self
    }

    /// PaperPaw is the device's firmware and updates it (the shell answers
    /// [`Io::Firmware`] and reports with
    /// [`Kernel::firmware_state`]). See [`paperpaw_sdk::firmware`].
    pub fn firmware(mut self, firmware: Firmware) -> Self {
        self.0.firmware = Some(firmware);
        self
    }

    /// The shell makes HTTP requests and serves ([`Io::Http`],
    /// [`Io::Server`], [`Io::Socket`]); without, plugins' requests fail
    /// at once.
    pub fn network(mut self, available: bool) -> Self {
        self.0.caps.network = available;
        self
    }

    /// The shell reads the system's clipboard when plugins ask for its
    /// text ([`Io::ReadClipboard`]); without, only text copied
    /// in PaperPaw is pasted.
    pub fn system_clipboard(mut self, available: bool) -> Self {
        self.0.caps.system_clipboard = available;
        self
    }

    /// The shell speaks text aloud ([`Io::Speech`]); without,
    /// plugins' speech fails at once.
    pub fn speech(mut self, available: bool) -> Self {
        self.0.caps.speech = available;
        self
    }

    /// The shell sleeps after showing PaperPaw's sleep screen (see
    /// [`Effect::Sleep`]); without, the system has its own (Android).
    pub fn sleep(mut self, available: bool) -> Self {
        self.0.caps.sleep = available;
        self
    }

    /// The shell turns the screen as the setting
    /// [`settings::ROTATION`](paperpaw_sdk::builtin::settings::ROTATION)
    /// says (see [`Kernel::rotation`]); without, it is not offered.
    pub fn rotation(mut self, available: bool) -> Self {
        self.0.caps.rotation = available;
        self
    }

    /// The buttons that lie in a row below the screen, left to right, each
    /// under an equal share of its width (e.g. the X4's four): a row of
    /// hints above them shows what they do
    /// ([`settings::BUTTON_HINTS`](core_settings::BUTTON_HINTS)).
    pub fn button_row(mut self, buttons: &'static [&'static str]) -> Self {
        self.0.button_row = buttons;
        self
    }

    /// The shell shows black and white in the colours of the settings
    /// [`settings::TEXT_COLOR`](paperpaw_sdk::builtin::settings::TEXT_COLOR)
    /// and [`settings::PAPER_COLOR`](paperpaw_sdk::builtin::settings::PAPER_COLOR)
    /// (see [`Kernel::colors`]); without, they are not offered.
    pub fn colors(mut self, available: bool) -> Self {
        self.0.caps.colors = available;
        self
    }

    /// Memory for glyphs rasterized at runtime (default 32 KiB). Bitmap font
    /// packs do not use it.
    pub fn glyph_cache(mut self, bytes: usize) -> Self {
        self.0.glyph_cache = bytes;
        self
    }

    /// Draws frames in bands of `rows` rows instead of holding the whole
    /// screen in RAM: the framebuffer shrinks to `rows / height` of its
    /// size, while each band costs one more pass over the visible screens.
    /// For devices whose panel controller keeps the frame (e.g. the X4).
    pub fn frame_rows(mut self, rows: u16) -> Self {
        self.0.frame_rows = Some(rows);
        self
    }

    /// Passes frames to [`Kernel::render`] in rows starting and ending at
    /// multiples of `rows`, e.g. 8 where the panel controller takes 8 of
    /// them in a byte (the X4's, turned). Frame bands should be a multiple
    /// of it.
    pub fn row_align(mut self, rows: u16) -> Self {
        self.0.row_align = rows.max(1);
        self
    }

    pub fn build(mut self) -> Result<Kernel, KernelError> {
        let display = self.0.device.display;
        let rows = self.0.frame_rows.unwrap_or(display.height);
        let framebuffer = Framebuffer::banded(display.width, display.height, rows, display.format)
            .map_err(KernelError::Graphics)?;
        Ok(self.0.build(framebuffer))
    }
}

impl BuilderParts {
    /// The kernel, built where it is returned: it is large, and each copy
    /// would take room on the stack while it is built (the X4's is small).
    fn build(&mut self, framebuffer: Framebuffer) -> Kernel {
        let mut kernel = self.new_kernel(framebuffer);
        kernel.card_trouble = self.card_trouble;
        kernel.after_sleep = self.after_sleep;
        paperpaw_sdk::data::take_over_old_name(kernel.shared.storage.as_ref());
        kernel.read_secrets();
        paperpaw_sdk::library::take_over_old(kernel.shared.storage.as_ref());
        // The configuration file on the device, then the shell's overrides.
        let stored = kernel.read_stored_config();
        let user_config = self.user_config.take();
        let configs: Vec<&str> = [stored.as_deref(), user_config.as_deref()]
            .into_iter()
            .flatten()
            .collect();
        let dirs = if self.plugin_dirs.is_empty() {
            alloc::vec![String::from(paperpaw_sdk::builtin::PLUGIN_DIR)]
        } else {
            mem::take(&mut self.plugin_dirs)
        };
        let built_in: Vec<Id> = core::iter::once(CORE)
            .chain(self.plugins.iter().map(|plugin| plugin.manifest().id))
            .collect();
        // Code plugins fetched for this firmware before it was installed
        // take their places now (ADR 0024), unless this start goes
        // without code plugins.
        if let Some(code) = &self.code
            && self.safe_mode.is_none()
        {
            let storage = kernel.shared.storage.clone();
            self.staged
                .extend(crate::code_plugin::fetched(storage.as_ref(), &code.native));
        }
        kernel.install_staged(mem::take(&mut self.staged), &dirs, &built_in);
        kernel.add_plugins(
            mem::take(&mut self.plugins),
            mem::take(&mut self.off),
            dirs,
            mem::take(&mut self.font_dirs),
            &configs,
            self.code
                .take()
                .map(|code| crate::code_plugin::CodePlugins {
                    safe_mode: self.safe_mode.take(),
                    ..code
                }),
        );
        // Every plugin's network, before the values kept for it are read.
        kernel.define_network_settings(self.network_settings);
        if let Some(source) = self.device_keymap.take() {
            kernel.load_device_keymap(source);
        }
        kernel.offer_choices();
        kernel.apply_defaults(&self.setting_defaults);
        if let Some(source) = &stored {
            kernel.apply_user_config(source);
            kernel.keys.user.clone_from(source);
        }
        if let Some(source) = &user_config {
            kernel.apply_user_config(source);
            kernel.keys.shell.clone_from(source);
        }
        // The power mode worn before, over the reader's settings.
        kernel.restore_power();
        kernel.apply_reading_font();
        kernel.apply_status_font();
        kernel.validate();
        kernel.rebuild_keymap();
        // Memory given back before the language is read in: its catalogs
        // are the largest allocations of a start, and a registry that
        // shrinks needs the new and the old room at once, so each is
        // done while the other is not there.
        kernel.shrink_to_fit();
        kernel.apply_display_settings();
        kernel.settle();
        kernel
    }

    /// The kernel with nothing registered: its `framebuffer`, its storage
    /// (metered, and what a call reads kept for the rest of it), what the
    /// shell said it does.
    #[expect(
        clippy::too_many_lines,
        reason = "longer than the limit from before it was set; split it when it is next changed"
    )]
    fn new_kernel(&mut self, framebuffer: Framebuffer) -> Kernel {
        let display = self.device.display;
        let meter = meter::Meter::new(self.clock);
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
        // What the shell said it does, and what follows from the rest.
        let mut caps = self.caps;
        caps.touch = self.device.input.touch;
        caps.black_and_white = display.format == paperpaw_platform::PixelFormat::Mono1;
        caps.firmware = self.firmware.is_some();
        caps.button_row = !self.button_row.is_empty();
        caps = with_buttons(caps, &self.device.input);
        let links = self.bluetooth_links;
        let mut translator = Translator::new();
        translator.fixed_indexes(self.catalog_indexes);
        Kernel {
            shared: Shared {
                registry: Registry::default(),
                meter,
                device: self.device,
                settings: Settings::new(),
                translator,
                fonts: Fonts::new(self.glyph_cache),
                formats: Formats::new(),
                hyphenation: Hyphenation::new(),
                menus: Menus::new(),
                actions: Actions::new(),
                plugins: Plugins::new(),
                status: SystemStatus::default(),
                usage: paperpaw_sdk::usage::Usage::default(),
                status_items: StatusItems::new(),
                widgets: paperpaw_sdk::widgets::Widgets::new(),
                setup: paperpaw_sdk::setup::SetupSteps::new(),
                services: paperpaw_sdk::services::Services::new(),
                text_filters: paperpaw_sdk::filters::TextFilters::new(),
                decorators: paperpaw_sdk::decorations::Decorators::new(),
                page_layers: paperpaw_sdk::layers::PageLayers::new(),
                lookup: paperpaw_sdk::lookup::LookupProviders::new(),
                voices: paperpaw_sdk::voices::Voices::new(),
                readers: paperpaw_sdk::readers::Readers::new(),
                input_filters: paperpaw_sdk::filters::InputFilters::new(),
                refresh_filters: paperpaw_sdk::filters::RefreshFilters::new(),
                stylesheets: Vec::new(),
                book_data: Vec::new(),
                book_fields: Vec::new(),
                book_badges: Vec::new(),
                caps,
                wifi: caps.wifi.then(paperpaw_sdk::wifi::Wifi::default),
                bluetooth: caps
                    .bluetooth
                    .then(|| paperpaw_sdk::bluetooth::Bluetooth::new(links)),
                firmware: self.firmware.take(),
                native: None,
                power: paperpaw_sdk::power::Power::default(),
                work_states: paperpaw_sdk::work::WorkStates::default(),
                reader: false,
                clipboard: String::new(),
                system_clipboard: false,
                last_input: paperpaw_sdk::keys::InputMode::at_start(&self.device.input),
                inputs: Vec::new(),
                stack: Vec::new(),
                content: Rect::new(0, 0, display.width, display.height),
                slots: Vec::new(),
                font_dirs: Vec::new(),
                data_folders: Vec::new(),
                plugin_dirs: Vec::new(),
                catalog_device: self.catalog_device,
                secrets: Default::default(),
                random: self.random,
                memory: self.memory,
                started: self.started,
                panel: self.panel,
                keys_held: Vec::new(),
                room: paperpaw_sdk::realtime::Room::default(),
                storage,
                recall,
                keymap: Keymap::default(),
                look: (paperpaw_sdk::ui::look::Look::none(), None),
            },
            frame: drawing::FrameState {
                pending: None,
                unrendered: None,
                rendered: None,
                row_align: self.row_align,
                band: self
                    .frame_rows
                    .map(|rows| u32::from(rows) * u32::from(display.width)),
                pictures_shown: false,
                again: false,
                screenshot: None,
                mark: None,
                refresh: RefreshPolicy::default(),
                deferred: None,
                defers: 0,
            },
            input: gestures::InputState {
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
                last_tap: None,
                recognizers: Vec::new(),
                pressed_on: None,
                thresholds: core::cell::Cell::new(None),
            },
            io: network::IoState {
                http: io::http::Http::for_memory(self.device.memory),
                servers: io::servers::Servers::default(),
                sockets: io::sockets::Sockets::default(),
                speech: io::speech::Speech::new(caps.speech),
                clipboard: io::clipboard::Clipboard::new(caps.system_clipboard),
                wifi_calls: Vec::new(),
                bluetooth: caps.bluetooth.then(|| io::bluetooth::Radio::new(links)),
                firmware_calls: Vec::new(),
                network_waiting: None,
                network_busy: false,
                focus_spoken: false,
                focus_told: None,
            },
            keys: plugins::KeySources::default(),
            chromes: Vec::new(),
            chrome_strips: Vec::new(),
            stylesheet_owners: Vec::new(),
            timing: drawing::Timing::default(),
            framebuffer,
            subscriptions: crate::registry::Subscriptions::default(),
            offered_down_to: 0,
            action_params: Params::new(),
            action_observers: Vec::new(),
            event_observers: Vec::new(),
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
            hint: hints::HintRow::new(self.button_row),
            busy_mark: self.busy_mark,
            withheld: Vec::new(),
            withheld_trusted: Vec::new(),
            withheld_own: Vec::new(),
            trusted: Vec::new(),
            from_guard: false,
            from_guard_own: false,
            own_trusted: Vec::new(),
            held_notice: None,
            last_activity: Instant(0),
            shown_since: Instant(0),
            scope: None,
            screens_changed: false,
            raw: crate::realtime::RawKeys::default(),
            lease: None,
            holds: crate::awake::Holds::default(),
            preview_owner: None,
            next_serial: 0,
            task: None,
            tasks: Vec::new(),
            config_unreadable: false,
            card_trouble: None,
            after_sleep: false,
            secrets_unreadable: false,
            pairing_nonce: None,
            plugin_defaults: Vec::new(),
            plugin_choices: Vec::new(),
            now: Instant::default(),
        }
    }
}

/// The steps of [`KernelBuilder::build`].
impl Kernel {
    /// Passwords and keys, as kept on the storage. A file that is there
    /// but cannot be read is said and not written over.
    fn read_secrets(&mut self) {
        use paperpaw_sdk::secrets::{FILE, Secrets};
        let storage = self.shared.storage.as_ref();
        // Damaged text keeps what can be read: values are written in hex.
        match paperpaw_sdk::files::load_for_update(storage, FILE, MAX_SECRETS) {
            Ok(data) => {
                let text = String::from_utf8_lossy(data.as_deref().unwrap_or_default());
                self.shared.secrets = Secrets::decode(&text);
                if let Some(radio) = &mut self.io.bluetooth {
                    let kept = self.shared.secrets.iter();
                    radio.load(kept.map(|s| (s.id.as_str(), s.value.as_str())));
                }
            }
            Err(error) => {
                self.secrets_unreadable = true;
                self.warn_runtime(format!(
                    "{FILE} cannot be read ({error}): passwords and keys are not used or saved"
                ));
            }
        }
    }

    /// `paperpaw.ini` on the storage, if there is one; one that is there
    /// but cannot be read is said, at once and to the user once the first
    /// screen shows, and is not written over.
    fn read_stored_config(&mut self) -> Option<String> {
        match read_config(self.shared.storage.as_ref()) {
            Ok(source) => source,
            Err(error) => {
                self.config_unreadable = true;
                let message = format!(
                    "{CONFIG_FILE} cannot be read ({error}): started with the defaults, the file left as it is"
                );
                self.warn(WarningSource::UserConfig, None, message);
                None
            }
        }
    }

    /// The kernel's own plugin, the shell's, those of data on the storage
    /// and of fonts in its folders, but those switched off in `configs`
    /// or shipped off (`off`) and not switched on.
    fn add_plugins(
        &mut self,
        plugins: Vec<Box<dyn Plugin>>,
        off: Vec<String>,
        plugin_dirs: Vec<String>,
        font_dirs: Vec<String>,
        configs: &[&str],
        code: Option<crate::code_plugin::CodePlugins>,
    ) {
        self.shared.menus.set_capabilities(self.shared.caps);
        // Colour screens open documents with colour pictures (ADR 0015).
        let display = self.shared.device.display;
        self.shared.formats.set_color(display.format.is_color());
        // Pictures no larger than the screen shows them.
        let fit = paperpaw_sdk::image::Fit::screen(display.width.into(), display.height.into());
        self.shared.formats.set_image_fit(fit);
        let mut disabled: Vec<String> = configs
            .iter()
            .flat_map(|c| plugin_switches(c, false))
            .collect();
        // Shipped off, unless switched on.
        let on: Vec<String> = configs
            .iter()
            .flat_map(|c| plugin_switches(c, true))
            .collect();
        disabled.extend(off.into_iter().filter(|id| !on.contains(id)));
        // The device's name the default of its setting.
        let name = paperpaw_sdk::device_name::default_name(&self.shared.device);
        let core: Box<dyn Plugin> = Box::new(CorePlugin(name));
        for plugin in core::iter::once(core).chain(plugins) {
            self.add_plugin(plugin.as_ref(), &disabled);
        }
        // Data plugins from the storage, after the built-in ones.
        let dirs = if plugin_dirs.is_empty() {
            alloc::vec![String::from(paperpaw_sdk::builtin::PLUGIN_DIR)]
        } else {
            plugin_dirs
        };
        let storage = self.shared.storage.clone();
        let mut with_code = Vec::new();
        for dir in &dirs {
            let mut problems = Vec::new();
            let folders = data_plugin::folders(storage.as_ref(), dir, &mut |problem| {
                problems.push(problem);
            });
            for problem in problems {
                self.warn_runtime(problem);
            }
            // One at a time: loaded, registered, its files let go.
            for folder in folders {
                let plugin = match data_plugin::load_folder(&storage, &folder) {
                    Ok(plugin) => plugin,
                    Err(problem) => {
                        self.warn_runtime(problem);
                        continue;
                    }
                };
                // An install cut short after the plugin was in place: what
                // its installer keeps for it is kept now.
                let id = plugin.manifest().id.clone();
                paperpaw_sdk::install::marks::settle(storage.as_ref(), id.as_str(), &folder);
                for problem in plugin.take_problems() {
                    self.warn_runtime(problem);
                }
                if disabled
                    .iter()
                    .any(|id| id == plugin.manifest().id.as_str())
                {
                    plugin.keep_listing_only();
                }
                self.shared
                    .data_folders
                    .push((plugin.manifest().id, plugin.folder().to_string()));
                if plugin.has_code() {
                    with_code.push(plugin);
                    continue;
                }
                self.add_plugin(&plugin, &disabled);
                if let Some(info) = self.shared.plugins.get_mut(plugin.manifest().id.as_str()) {
                    info.kind = paperpaw_sdk::PluginKind::Data;
                }
            }
        }
        self.add_code_plugins(with_code, &disabled, code);
        self.shared.plugin_dirs = dirs;
        if !font_dirs.is_empty()
            && let Some(plugin) = data_plugin::font_folder(self.shared.storage.as_ref(), &font_dirs)
        {
            self.add_plugin(&plugin, &disabled);
        }
        self.shared.font_dirs = font_dirs;
        self.find_required_plugins();
    }

    /// The device's own key map, under all others.
    fn load_device_keymap(&mut self, source: Cow<'static, str>) {
        for problem in self.shared.keymap.load(&source, Origin::Device, true) {
            self.warn(
                WarningSource::DeviceKeymap,
                Some(problem.line),
                problem.message.into_owned(),
            );
        }
        self.keys.device = source;
    }

    /// The choices of settings that plugins add to: key map presets,
    /// reading fonts, page frames and voices; speech where the system or
    /// a plugin speaks.
    pub(crate) fn offer_choices(&mut self) {
        self.add_plugin_choices();
        let mut presets: Vec<Cow<'static, str>> =
            alloc::vec![Cow::Borrowed(keymap_presets::STANDARD_NAME)];
        presets.extend(
            self.keys
                .presets
                .iter()
                .map(|(id, _)| Cow::Owned(id.as_str().to_string())),
        );
        self.shared
            .settings
            .set_choices(core_settings::KEYMAP_PRESET.as_str(), presets);
        let mut families: Vec<Cow<'static, str>> =
            alloc::vec![Cow::Borrowed(core_settings::DEFAULT_FONT)];
        families.extend(
            self.shared
                .fonts
                .families()
                .into_iter()
                .map(|f| Cow::Owned(f.to_string())),
        );
        let mut status: Vec<Cow<'static, str>> = alloc::vec![
            Cow::Borrowed(core_settings::STATUS_FONT_UI),
            Cow::Borrowed(core_settings::STATUS_FONT_BOOK),
        ];
        status.extend(families.iter().skip(1).cloned());
        self.shared
            .settings
            .set_choices(core_settings::READING_FONT.as_str(), families);
        self.shared
            .settings
            .set_choices(core_settings::STATUS_FONT.as_str(), status);
        let mut frames: Vec<Cow<'static, str>> =
            alloc::vec![Cow::Borrowed(core_settings::PAGE_FRAME_SINGLE)];
        frames.extend(
            self.shared
                .page_layers
                .frames()
                .map(|id| Cow::Owned(id.as_str().to_string())),
        );
        self.shared
            .settings
            .set_choices(core_settings::PAGE_FRAME.as_str(), frames);
        let mut voices: Vec<Cow<'static, str>> =
            alloc::vec![Cow::Borrowed(core_settings::VOICE_SYSTEM)];
        voices.extend(
            self.shared
                .voices
                .ids()
                .map(|id| Cow::Owned(id.as_str().to_string())),
        );
        self.shared
            .settings
            .set_choices(core_settings::VOICE.as_str(), voices);
        self.offer_reading_apps();
        self.shared.caps.speech = self.io.speech.system || !self.shared.voices.is_empty();
        self.shared.menus.set_capabilities(self.shared.caps);
    }

    /// The values plugins offer among other plugins' choices
    /// ([`paperpaw_sdk::Registrar::add_choice`]), after the setting's own.
    pub(crate) fn add_plugin_choices(&mut self) {
        for (plugin, id, value) in mem::take(&mut self.plugin_choices) {
            let refused = match self.shared.settings.spec(id.as_str()) {
                None => Some("no such setting"),
                Some(spec) if spec.guarded => Some("it keeps the device safe"),
                Some(spec) => match &spec.kind {
                    paperpaw_sdk::SettingKind::Choice(choices) if choices.contains(&value) => {
                        Some("it has that choice already")
                    }
                    paperpaw_sdk::SettingKind::Choice(_)
                        if Id::parse(&value).is_ok_and(|v| v.namespace() == plugin.as_str()) =>
                    {
                        None
                    }
                    paperpaw_sdk::SettingKind::Choice(_) => {
                        Some("the value is not in the plugin's namespace")
                    }
                    _ => Some("it is no choice"),
                },
            };
            if let Some(why) = refused {
                self.warn(
                    WarningSource::Plugin(plugin),
                    None,
                    format!("choice `{value}` for `{id}`: {why}; not added"),
                );
                continue;
            }
            let Some(paperpaw_sdk::SettingKind::Choice(choices)) =
                self.shared.settings.spec(id.as_str()).map(|s| &s.kind)
            else {
                continue;
            };
            let mut values: Vec<Cow<'static, str>> =
                choices.iter().map(|v| Cow::Owned(v.to_string())).collect();
            values.push(value);
            self.shared.settings.set_choices(id.as_str(), values);
        }
    }

    /// The reading apps to choose: the default one, and one for each
    /// format (a setting each, in the reading settings' part for them),
    /// each offering the readers that read its books. Where only one
    /// does, there is nothing to choose and the setting is not listed.
    fn offer_reading_apps(&mut self) {
        let readers: Vec<Cow<'static, str>> = self
            .shared
            .readers
            .iter()
            .map(|r| Cow::Owned(r.id.as_str().to_string()))
            .collect();
        let mut all = alloc::vec![Cow::Borrowed(core_settings::READING_APP_DEFAULT)];
        if readers.len() > 1 {
            all.extend(readers);
        }
        self.shared
            .settings
            .set_choices(core_settings::READING_APP.as_str(), all);
        let formats: Vec<(Id, Cow<'static, str>)> = self
            .shared
            .formats
            .iter()
            .map(|f| (f.id.clone(), f.title.clone()))
            .collect();
        for (format, title) in formats {
            let mut choices = alloc::vec![Cow::Borrowed(core_settings::READING_APP_DEFAULT)];
            let reading: Vec<Cow<'static, str>> = self
                .shared
                .readers
                .reading(Some(&format))
                .map(|r| Cow::Owned(r.id.as_str().to_string()))
                .collect();
            let several = reading.len() > 1;
            if several {
                choices.extend(reading);
            }
            let id = paperpaw_sdk::readers::format_setting(&format);
            if self.shared.settings.spec(&id).is_some() {
                self.shared.settings.set_choices(&id, choices);
                continue;
            }
            // Nothing to choose where one reader reads it: no setting
            // (memory on small devices), until another comes.
            if !several {
                continue;
            }
            let Ok(setting) = Id::parse(&id) else {
                continue;
            };
            /// The reading apps' part, keeping apps not installed now.
            static READING_APPS: paperpaw_sdk::SettingExtra = paperpaw_sdk::SettingExtra::new()
                .section("core.section.reading-apps")
                .keeps_unknown();
            let mut spec = paperpaw_sdk::SettingSpec::new(
                setting,
                "",
                paperpaw_sdk::SettingKind::Choice(choices.into()),
                paperpaw_sdk::SettingValue::Text(Cow::Borrowed(core_settings::READING_APP_DEFAULT)),
                paperpaw_sdk::builtin::groups::READING_BEHAVIOUR,
            )
            .description("core.setting.reading-app.format.help")
            .with_extra(&READING_APPS);
            spec.title = title;
            let _ = self.shared.settings.define(spec);
        }
    }

    /// The device's defaults, then the plugins' (e.g. themes), which win:
    /// a theme installed for larger print enlarges it on a device whose
    /// own default is small too.
    pub(crate) fn apply_defaults(&mut self, device: &[(String, String)]) {
        // The kernel's own for this panel first, the device's after.
        let format = self.shared.device.display.format;
        if let Some(value) =
            crate::core_plugin::text_look::smooth_text_default(format.gray_levels())
        {
            let id = core_settings::SMOOTH_TEXT.static_str();
            if let Err(error) = self.shared.settings.set_default_from_text(id, value) {
                self.warn_runtime(format!("default `{id}`: {error}"));
            }
        }
        for (id, value) in device {
            if let Err(error) = self.shared.settings.set_default_from_text(id, value) {
                self.warn_runtime(format!("device default `{id}`: {error}"));
            }
        }
        for (plugin, id, value) in mem::take(&mut self.plugin_defaults) {
            // A setting that keeps the device safe takes no other plugin's
            // default.
            let guarded = self
                .shared
                .settings
                .spec(id.as_str())
                .is_some_and(|spec| spec.guarded);
            if guarded && id.namespace() != plugin.as_str() {
                self.warn(
                    WarningSource::Plugin(plugin),
                    None,
                    format!("default for `{id}`: it keeps the device safe; not taken"),
                );
                continue;
            }
            if let Err(error) = self
                .shared
                .settings
                .set_default_from_text(id.as_str(), &value)
            {
                self.warn(
                    WarningSource::Plugin(plugin),
                    None,
                    format!("default for `{id}`: {error}"),
                );
            }
        }
    }

    /// The language, night mode and colours the settings say.
    fn apply_display_settings(&mut self) {
        let shared = &mut self.shared;
        shared
            .translator
            .set_language(shared.settings.read(&core_settings::LANGUAGE));
        let settings = &self.shared.settings;
        self.framebuffer
            .set_inverted(settings.bool(core_settings::NIGHT_MODE.as_str()));
        self.framebuffer
            .set_pictures_kept(!settings.bool(core_settings::NIGHT_PICTURES.as_str()));
        self.framebuffer.set_mono_coverage(mono_coverage(settings));
        if let Some(panel) = self.shared.device.display.color {
            self.framebuffer.set_saturation(panel.boost_percent());
        }
    }

    /// Done registering: memory given back, the slots and screens known,
    /// the row of hints as tall as the first screen is made for.
    fn settle(&mut self) {
        self.shrink_to_fit();
        self.shared.slots = self.shared.registry.bound_slots();
        self.hint.height = self.wanted_hint_height();
    }
}

impl Kernel {
    pub fn builder(device: DeviceInfo) -> KernelBuilder {
        KernelBuilder(Box::new(BuilderParts {
            device,
            plugins: Vec::new(),
            off: Vec::new(),
            device_keymap: None,
            user_config: None,
            glyph_cache: DEFAULT_GLYPH_CACHE,
            caps: Capabilities::default(),
            firmware: None,
            code: None,
            safe_mode: None,
            staged: Vec::new(),
            button_row: &[],
            frame_rows: None,
            row_align: 1,
            storage: None,
            card_trouble: None,
            clock: None,
            busy_mark: false,
            setting_defaults: Vec::new(),
            plugin_dirs: Vec::new(),
            font_dirs: Vec::new(),
            catalog_device: None,
            random: paperpaw_sdk::random::Random::NONE,
            memory: paperpaw_sdk::memory::Memory::NONE,
            started: paperpaw_sdk::power::Start::Unknown,
            after_sleep: false,
            panel: paperpaw_sdk::realtime::PanelInfo::new(
                device
                    .display
                    .format
                    .gray_levels()
                    .map_or(255, |l| l.min(255) as u8),
            ),
            network_settings: &[],
            catalog_indexes: &[],
            bluetooth_links: 0,
        }))
    }
}

/// What the device's buttons are good for ([`Capability::PAGE_KEYS`],
/// [`Capability::BUTTONS`]), of the buttons the shell lists; a shell that
/// lists none but has buttons has a full set.
///
/// [`Capability::PAGE_KEYS`]: paperpaw_sdk::Capability::PAGE_KEYS
/// [`Capability::BUTTONS`]: paperpaw_sdk::Capability::BUTTONS
fn with_buttons(
    caps: paperpaw_sdk::Capabilities,
    input: &paperpaw_platform::InputCaps,
) -> paperpaw_sdk::Capabilities {
    use paperpaw_platform::buttons as b;
    use paperpaw_sdk::Capability;
    if !input.buttons {
        return caps;
    }
    if input.keys.is_empty() {
        return caps.with(Capability::PAGE_KEYS).with(Capability::BUTTONS);
    }
    let has = |name: &str| input.keys.contains(&name);
    let sideways = has(b::LEFT) && has(b::RIGHT);
    let pages = (has(b::PAGE_FORWARD) && has(b::PAGE_BACK)) || sideways;
    let moves = (has(b::UP) && has(b::DOWN)) || sideways;
    let mut caps = caps;
    if pages {
        caps = caps.with(Capability::PAGE_KEYS);
    }
    if has(b::BACK) && has(b::CONFIRM) && moves {
        caps = caps.with(Capability::BUTTONS);
    }
    caps
}
