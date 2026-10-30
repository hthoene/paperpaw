//! The native activity: event loop, window, input and storage.

use std::ffi::CString;
use std::io::Read;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant as StdInstant};

use android_activity::input::{InputEvent as AndroidInput, KeyAction, MotionAction, ToolType};
use android_activity::{AndroidApp, InputStatus, MainEvent, PollEvent, WindowManagerFlags};
use ndk::hardware_buffer_format::HardwareBufferFormat;
use ndk::native_window::NativeWindow;
use paperpaw_core::{Effects, Io, Kernel};
use paperpaw_fs::DirStorage;
use paperpaw_graphics::Framebuffer;
use paperpaw_platform::{
    ButtonState, DeviceInfo, DirEntry, DisplayInfo, InputCaps, InputEvent, Instant, Mounted,
    PixelFormat, Point, PointerEvent, PointerPhase, PointerTool, RefreshMode, Rotation, Storage,
    StorageError,
};
use paperpaw_sdk::builtin::settings;
use paperpaw_sdk::library;

use crate::epd::Epd;
use crate::lights::Lights;
use crate::plugin::{AndroidPlugin, Frontlight};
use crate::private::Private;
use crate::{access, keys, pixels, status};

/// Where the device's shared storage appears in PaperPaw's storage.
const SHARED: &str = "storage";
/// Where SD cards appear: the first as `SD card`, more numbered.
const REMOVABLE: &str = "SD card";
/// Data plugins on the shared storage, where users can copy them.
const PLUGINS: &str = "PaperPaw/plugins";
/// Dictionaries on the shared storage.
const DICTIONARIES: &str = "PaperPaw/dictionaries";
/// Where KOReader keeps dictionaries; used if it has any.
const KOREADER_DICTIONARIES: &str = "koreader/data/dict";
/// Fonts to read with: PaperPaw's folder, KOReader's and the one Boox's
/// reader uses.
const FONTS: &str = "PaperPaw/fonts";
const KOREADER_FONTS: &str = "koreader/fonts";
const SYSTEM_FONTS: &str = "fonts";

/// The running app once the window size is known.
struct Running {
    kernel: Kernel,
    /// The whole screen, as the window shows it.
    frame: Framebuffer,
    wake_at: Option<Instant>,
    /// The kernel asked to be built again (e.g. plugins were switched).
    restart: bool,
    /// The shared storage is mounted as `storage`.
    shared_storage: bool,
    /// The strongest refresh requested since the last frame was shown.
    refresh: Option<RefreshMode>,
    /// The rows of the frame changed since it was shown last.
    changed_rows: Option<Range<u16>>,
    /// When the battery and time zone were read last.
    status_read: Instant,
    /// HTTP requests in flight.
    net: crate::net::Net,
    /// For the system's clipboard.
    app: AndroidApp,
    /// Plugins' servers.
    servers: paperpaw_serve::Servers,
    /// Plugins' sockets.
    sockets: paperpaw_serve::Sockets,
    /// Reading aloud.
    speech: crate::speech::Speech,
    /// Plugins wait for the system clipboard's text.
    clipboard_wanted: bool,
    /// The screen is kept on (while reading aloud).
    awake: bool,
    /// How the system was asked to turn the screen (see
    /// `Kernel::rotation`).
    rotation: Rotation,
}

/// How often the battery and time zone are read again.
const STATUS_EVERY_MS: u64 = 60_000;

#[unsafe(no_mangle)]
#[expect(
    clippy::too_many_lines,
    reason = "longer than the limit from before it was set; split it when it is next changed"
)]
fn android_main(app: AndroidApp) {
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info)
            .with_tag("PaperPaw"),
    );
    log_panics();
    let start = StdInstant::now();
    let now = move || Instant(start.elapsed().as_millis() as u64);
    let mut epd = Epd::detect(&app);
    let lights = Lights::detect(&app);
    let mut running: Option<Running> = None;
    let mut window: Option<NativeWindow> = None;
    let mut quit = false;
    // In the background (or the screen is off): timers wait, unless
    // reading aloud goes on, and nothing is shown.
    let mut paused = false;
    // The frame changed and is still to be shown.
    let mut unshown = false;

    while !quit {
        let ticking = !paused || running.as_ref().is_some_and(|r| r.kernel.speaking());
        let mut timeout = running
            .as_ref()
            .and_then(|r| r.wake_at)
            .filter(|_| ticking)
            .map(|at| Duration::from_millis(at.since(now())));
        // Speech waiting for the engine looks again soon.
        if running.as_ref().is_some_and(|r| r.speech.waiting()) {
            let poll = crate::speech::POLL;
            timeout = Some(timeout.map_or(poll, |t| t.min(poll)));
        }
        app.poll_events(timeout, |event| {
            let PollEvent::Main(event) = event else {
                return;
            };
            match event {
                MainEvent::InitWindow { .. } => {
                    window = app.native_window();
                    if let Some(window) = &window {
                        if running.is_none() {
                            running = start_kernel(&app, window, lights, now());
                        }
                        if let Some(running) = &running {
                            // A new or returning window: start clean.
                            if let Some(epd) = epd.as_mut() {
                                epd.set(RefreshMode::Full);
                            }
                            present(window, &running.frame, running.kernel.colors(), None);
                        }
                    }
                }
                MainEvent::RedrawNeeded { .. }
                | MainEvent::WindowResized { .. }
                | MainEvent::ConfigChanged { .. } => {
                    if let (Some(window), Some(running)) = (&window, running.as_mut()) {
                        // Turned: the same screens, made anew for the new size.
                        let size = (window.width(), window.height());
                        let frame = (
                            i32::from(running.frame.width()),
                            i32::from(running.frame.height()),
                        );
                        if size != frame
                            && let (Ok(width), Ok(height)) =
                                (u16::try_from(size.0), u16::try_from(size.1))
                            && let Ok(new) = Framebuffer::new(width, height, running.frame.format())
                        {
                            running.frame = new;
                            let effects = running.kernel.resize(now(), width, height);
                            apply(running, effects);
                            running.changed_rows = None;
                            if let Some(epd) = epd.as_mut() {
                                epd.set(RefreshMode::Full);
                            }
                        }
                        present(window, &running.frame, running.kernel.colors(), None);
                    }
                }
                MainEvent::Resume { .. } => {
                    paused = false;
                    // The time zone or the battery may have changed.
                    if let Some(running) = running.as_mut() {
                        running.status_read = Instant(0);
                        let effects = running.kernel.resume(now());
                        unshown |= apply(running, effects);
                    }
                    // Back from the settings page that grants storage
                    // access: restart with the shared storage.
                    if let Some(running) = running.as_mut()
                        && !running.shared_storage
                        && access::granted(&app)
                    {
                        running.restart = true;
                    }
                }
                MainEvent::Pause => {
                    paused = true;
                    // Other apps get the device's own refresh setting.
                    if let Some(epd) = epd.as_mut() {
                        epd.reset();
                    }
                    // Plugins save or send what is pending (e.g. progress).
                    if let Some(running) = running.as_mut() {
                        let effects = running.kernel.suspend(now());
                        unshown |= apply(running, effects);
                    }
                }
                MainEvent::GainedFocus => {
                    // Maybe copied something in another app (not read
                    // until pasted).
                    if let Some(running) = running.as_mut() {
                        let available = crate::clipboard::has_text(&app);
                        running.kernel.set_clipboard_available(available);
                    }
                }
                MainEvent::TerminateWindow { .. } => window = None,
                MainEvent::Destroy => quit = true,
                _ => {}
            }
        });

        let Some(state) = running.as_mut() else {
            continue;
        };
        let mut changed = core::mem::take(&mut unshown);
        if let Ok(mut events) = app.input_events_iter() {
            while events.next(|event| {
                // What PaperPaw takes is handled, also when it brings no
                // event (a held key's repeats): Android would otherwise act
                // on it too, e.g. change the volume.
                let Some(events) = input(event) else {
                    return InputStatus::Unhandled;
                };
                for event in events {
                    let effects = state.kernel.handle(now(), event);
                    changed |= apply(state, effects);
                }
                InputStatus::Handled
            }) {}
        }
        if now().since(state.status_read) >= STATUS_EVERY_MS || state.status_read == Instant(0) {
            let at = now();
            state.status_read = at;
            let effects = state.kernel.set_status(at, status::read(&app, at));
            changed |= apply(state, effects);
        }
        while let Some((id, received, total)) = state.net.next_progress() {
            let effects = state.kernel.http_progress(now(), id, received, total);
            changed |= apply(state, effects);
        }
        while let Some(response) = state.net.next() {
            let effects = state.kernel.http_response(now(), response);
            changed |= apply(state, effects);
        }
        while let Some(event) = state.servers.next() {
            let effects = state.kernel.server_event(now(), event);
            changed |= apply(state, effects);
        }
        while let Some(event) = state.sockets.take_event() {
            let effects = state.kernel.socket_event(now(), event);
            changed |= apply(state, effects);
        }
        let kernel = &state.kernel;
        state
            .servers
            .pump(|path, at, buffer| kernel.read_served(path, at, buffer).ok());
        if core::mem::take(&mut state.clipboard_wanted) {
            let text = crate::clipboard::read(&app);
            let effects = state.kernel.clipboard_text(now(), text);
            changed |= apply(state, effects);
        }
        for (id, spoken) in state.speech.poll() {
            let effects = state.kernel.speech_done(now(), id, spoken);
            changed |= apply(state, effects);
        }
        if ticking && state.wake_at.is_some_and(|at| at <= now()) {
            // What changed shows before the timer runs: it may take a
            // while (e.g. reading a folder on an SD card) and say so first.
            if changed
                && !paused
                && let Some(window) = &window
            {
                show(window, epd.as_mut(), state);
                changed = false;
            }
            let effects = state.kernel.tick(now());
            changed |= apply(state, effects);
        }
        // The screen stays on while reading aloud: a sleeping device would
        // stop after the sentence.
        let speaking = state.kernel.speaking();
        if speaking != state.awake {
            state.awake = speaking;
            let (on, none) = (
                WindowManagerFlags::KEEP_SCREEN_ON,
                WindowManagerFlags::empty(),
            );
            if speaking {
                app.set_window_flags(on, none);
            } else {
                app.set_window_flags(none, on);
            }
        }
        if changed {
            match &window {
                Some(window) if !paused => show(window, epd.as_mut(), state),
                _ => unshown = true,
            }
        }
        for warning in state.kernel.take_warnings() {
            log::warn!("{warning}");
        }
        if state.restart
            && let Some(window) = &window
        {
            log::info!("restarting");
            // The new kernel does not read aloud (yet): the screen may
            // turn off again.
            if state.awake {
                app.set_window_flags(
                    WindowManagerFlags::empty(),
                    WindowManagerFlags::KEEP_SCREEN_ON,
                );
            }
            // The old kernel goes first: plugins save what is pending, its
            // files are closed and its memory is free before the new one
            // starts.
            if let Some(running) = running.as_mut() {
                running.kernel.stop(now());
            }
            drop(running.take());
            running = start_kernel(&app, window, lights, now());
            if let Some(running) = &running {
                if let Some(epd) = epd.as_mut() {
                    epd.set(RefreshMode::Full);
                }
                present(window, &running.frame, running.kernel.colors(), None);
            }
            unshown = false;
        }
    }
}

/// Writes a panic's message, place and stack to the log: release builds
/// abort at once (`panic = "abort"`), and what goes to stderr is lost, so
/// a crash would otherwise leave only the system's signal report.
fn log_panics() {
    std::panic::set_hook(Box::new(|info| {
        let payload = info.payload();
        let message = payload
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
            .unwrap_or("(no message)");
        let place = info
            .location()
            .map(|at| format!("{}:{}", at.file(), at.line()))
            .unwrap_or_default();
        let stack = std::backtrace::Backtrace::force_capture();
        log::error!("panic at {place}: {message}\n{stack}");
    }));
}

/// Builds the kernel for the window's size.
#[expect(
    clippy::too_many_lines,
    reason = "longer than the limit from before it was set; split it when it is next changed"
)]
fn start_kernel(
    app: &AndroidApp,
    window: &NativeWindow,
    lights: Option<Lights>,
    now: Instant,
) -> Option<Running> {
    let (width, height) = (window.width(), window.height());
    let dpi = app.config().density().unwrap_or(160);
    // Gray for monochrome e-ink, colour (for pictures) elsewhere.
    let screen = crate::access::with_activity(app, |env, _| {
        let mut build = |field: &str| -> jni::errors::Result<String> {
            let value = env
                .get_static_field("android/os/Build", field, "Ljava/lang/String;")?
                .l()?;
            Ok(env.get_string(&value.into())?.into())
        };
        let (manufacturer, model) = (build("MANUFACTURER")?, build("MODEL")?);
        log::info!("{manufacturer} {model}");
        Ok(pixels::screen(&manufacturer, &model))
    });
    let (format, color) = match screen {
        Some(pixels::Screen::Color(panel)) => (PixelFormat::Rgb888, panel),
        _ => (PixelFormat::Gray8, None),
    };
    let device = DeviceInfo::new(
        "android",
        "Android e-reader",
        DisplayInfo::new(
            u16::try_from(width).ok()?,
            u16::try_from(height).ok()?,
            u16::try_from(dpi).unwrap_or(160),
            format,
            Rotation::Deg0,
            color,
        ),
        InputCaps::new(
            true,
            true,
            false,
            // The page buttons (or volume keys) and back.
            &[
                paperpaw_platform::buttons::PAGE_FORWARD,
                paperpaw_platform::buttons::PAGE_BACK,
                paperpaw_platform::buttons::BACK,
            ],
        ),
        paperpaw_platform::MemoryClass::Large,
    );
    let internal = app.internal_data_path();
    let root = app
        .external_data_path()
        .or_else(|| internal.clone())
        .unwrap_or_else(|| PathBuf::from("."));
    install_samples(app, &root);
    let storage = match DirStorage::new(&root) {
        Ok(storage) => storage,
        Err(e) => {
            log::error!("storage {}: {e}", root.display());
            return None;
        }
    };
    // The passwords where only PaperPaw reads them (see `private`).
    let storage = Volumes(
        match internal
            .filter(|internal| *internal != root)
            .map(DirStorage::new)
        {
            Some(Ok(private)) => Box::new(Private::new(storage, private)),
            Some(Err(e)) => {
                log::warn!("private folder: {e}");
                Box::new(storage)
            }
            None => Box::new(storage),
        },
    );
    // The shared storage as `storage`, if the user allowed access; the
    // library is then its `Books` folder unless the user chose another.
    let shared = access::granted(app)
        .then(|| access::storage_dir(app))
        .flatten()
        .and_then(|dir| {
            // Data plugins and dictionaries are copied there.
            for folder in [PLUGINS, DICTIONARIES, FONTS] {
                if let Err(e) = std::fs::create_dir_all(dir.join(folder)) {
                    log::warn!("{}: {e}", dir.join(folder).display());
                }
            }
            match DirStorage::new(&dir) {
                Ok(shared) => Some(shared),
                Err(e) => {
                    log::warn!("{}: {e}", dir.display());
                    None
                }
            }
        });
    // KOReader's dictionaries if there are any, else PaperPaw's folder.
    let dictionaries = access::storage_dir(app)
        .filter(|dir| shared.is_some() && dir.join(KOREADER_DICTIONARIES).is_dir())
        .map_or(DICTIONARIES, |_| KOREADER_DICTIONARIES);
    let shared_storage = shared.is_some();
    // SD cards next to the shared storage, with the same access.
    let removable: Vec<(&'static str, DirStorage)> = if shared_storage {
        access::removable_dirs(app)
            .into_iter()
            .filter_map(|dir| match DirStorage::new(&dir) {
                Ok(volume) => Some(volume),
                Err(e) => {
                    log::warn!("{}: {e}", dir.display());
                    None
                }
            })
            .enumerate()
            .map(|(n, volume)| {
                let name = match n {
                    0 => REMOVABLE.to_string(),
                    n => format!("{REMOVABLE} {}", n + 1),
                };
                // Once per start: the mount points live as long as the app.
                (&*Box::leak(name.into_boxed_str()), volume)
            })
            .collect()
    } else {
        Vec::new()
    };
    // The plugins the store offers as files are built in here (an app
    // from Play may load no code from files), switched off for a new
    // reader until wanted; for one who ran PaperPaw before, as they were.
    let ran_before = paperpaw_app::ran_before(&storage);
    let mut builder = Kernel::builder(device)
        .clock(micros)
        .random(random)
        .plugin_dir("plugins")
        .network(true)
        .catalog_device("android")
        .system_clipboard(true)
        .speech(true)
        .rotation(true)
        .colors(true);
    builder = match shared {
        Some(shared) => builder
            .storage(removable.into_iter().fold(
                Volumes(Box::new(Mounted::new(storage, SHARED, shared))),
                |all, (name, volume)| Volumes(Box::new(Mounted::new(all, name, volume))),
            ))
            .plugin_dir(format!("{SHARED}/{PLUGINS}"))
            .font_dir(format!("{SHARED}/{FONTS}"))
            .font_dir(format!("{SHARED}/{KOREADER_FONTS}"))
            .font_dir(format!("{SHARED}/{SYSTEM_FONTS}"))
            .setting_default(&library::FOLDER, format!("{SHARED}/Books").into())
            .setting_default(
                &paperpaw_plugin_dictionary::FOLDER,
                format!("{SHARED}/{dictionaries}").into(),
            ),
        None => builder.storage(storage),
    };
    // The system language, unless the user chose another one.
    if let Some(language) = app.config().language() {
        builder = builder.setting_default(&settings::LANGUAGE, language.into());
    }
    let builder = paperpaw_app::install_with(
        builder,
        paperpaw_app::Fonts::Outline,
        paperpaw_app::StorePlugins::Off(&|_| ran_before),
    )
    .plugin(AndroidPlugin {
        app: app.clone(),
        shared_storage,
        screen: (
            u32::from(device.display.width),
            u32::from(device.display.height),
        ),
        frontlight: lights.map(|lights| Frontlight {
            lights,
            brightness: lights.brightness(app),
            warmth: lights.warmth(app),
        }),
    });
    let mut kernel = match builder.build() {
        Ok(kernel) => kernel,
        Err(e) => {
            log::error!("cannot start: {e}");
            return None;
        }
    };
    let frame = Framebuffer::new(device.display.width, device.display.height, format).ok()?;
    let _ = kernel.set_status(now, status::read(app, now));
    let effects = kernel.start(now);
    let mut running = Running {
        kernel,
        frame,
        wake_at: None,
        restart: false,
        shared_storage,
        refresh: None,
        changed_rows: None,
        status_read: now,
        net: crate::net::Net::new(app),
        app: app.clone(),
        servers: {
            let waker = app.create_waker();
            paperpaw_serve::Servers::new(move || waker.wake())
        },
        sockets: {
            let waker = app.create_waker();
            paperpaw_serve::Sockets::new(move || waker.wake())
        },
        speech: crate::speech::Speech::new(app),
        clipboard_wanted: false,
        awake: false,
        rotation: Rotation::Deg0,
    };
    apply(&mut running, effects);
    Some(running)
}

/// Copies the sample books from the APK on first start, so there is
/// something to read. Once the folder is there, it is the user's: sample
/// books deleted from it stay deleted.
fn install_samples(app: &AndroidApp, root: &Path) {
    let books = root.join("books");
    if books.exists() {
        return;
    }
    if let Err(e) = std::fs::create_dir_all(&books) {
        log::warn!("{}: {e}", books.display());
        return;
    }
    let assets = app.asset_manager();
    let Some(dir) = assets.open_dir(c"books") else {
        return;
    };
    for name in dir {
        let Ok(text) = name.to_str() else {
            continue;
        };
        let Ok(path) = CString::new(format!("books/{text}")) else {
            continue;
        };
        let Some(mut asset) = assets.open(&path) else {
            continue;
        };
        let mut data = Vec::new();
        if asset.read_to_end(&mut data).is_ok()
            && let Err(e) = std::fs::write(books.join(text), data)
        {
            log::warn!("{text}: {e}");
        }
    }
}

/// Executes the effects of a kernel call; `true` if the screen changed.
fn apply(state: &mut Running, effects: Effects) -> bool {
    state.wake_at = effects.wake_at();
    for io in state.kernel.take_io() {
        match io {
            Io::Http(mut call) => {
                if let Err(e) = call.load_body(state.kernel.storage()) {
                    let error = paperpaw_core::sdk::HttpError::Io;
                    let failed =
                        paperpaw_core::HttpResponse::failed(call.id, error, &e.to_string());
                    // Answered on the next round, like the network's answers.
                    state.net.answer(failed);
                    continue;
                }
                state.net.start(call);
            }
            Io::Copy(text) => crate::clipboard::write(&state.app, &text),
            Io::ReadClipboard => state.clipboard_wanted = true,
            Io::Server(call) => state.servers.apply(call),
            Io::Socket(call) => state.sockets.apply(call),
            Io::Speech(call) => state.speech.apply(call),
            // The system manages Wi-Fi and the app: the kernel is built
            // without either, so neither is asked.
            Io::Wifi(_) | Io::Firmware(_) | Io::Bluetooth(_) => {}
        }
    }
    state.restart |= effects.restart();
    // The system turns the window; its new size comes as an event.
    let rotation = state.kernel.rotation();
    if rotation != state.rotation {
        state.rotation = rotation;
        turn(&state.app, rotation);
    }
    let Some((region, mode)) = effects.refresh() else {
        return false;
    };
    state.refresh = Some(state.refresh.map_or(mode, |m| m.max(mode)));
    // What is to be refreshed is shown anew, with the rows drawn.
    let bottom = u16::try_from(region.bottom()).unwrap_or(u16::MAX);
    let mut rows = state.changed_rows.take().map_or(region.y..bottom, |r| {
        r.start.min(region.y)..r.end.max(bottom)
    });
    let frame = &mut state.frame;
    state.kernel.render(&mut |top, band| {
        let end = top.saturating_add(band.height);
        rows = rows.start.min(top)..rows.end.max(end);
        frame.write_rows(top, band);
    });
    state.changed_rows = Some(rows);
    true
}

/// Asks the system to turn the screen.
fn turn(app: &AndroidApp, rotation: Rotation) {
    // `ActivityInfo.SCREEN_ORIENTATION_*`.
    let orientation = match rotation {
        Rotation::Deg0 => 1,
        Rotation::Deg90 => 0,
        Rotation::Deg180 => 9,
        Rotation::Deg270 => 8,
    };
    let turned = access::with_activity(app, |env, activity| {
        env.call_method(
            activity,
            "setRequestedOrientation",
            "(I)V",
            &[jni::objects::JValue::Int(orientation)],
        )?;
        Ok(())
    });
    if turned.is_none() {
        log::warn!("cannot turn the screen");
    }
}

/// Shows the frame, with the e-ink refresh the kernel asked for.
fn show(window: &NativeWindow, epd: Option<&mut Epd>, state: &mut Running) {
    if let (Some(epd), Some(mode)) = (epd, state.refresh.take()) {
        epd.set(mode);
    }
    let rows = state.changed_rows.take();
    present(window, &state.frame, state.kernel.colors(), rows);
}

/// Copies the frame into the window, in the colours of the settings: the
/// `rows` that changed, or all of them. The window keeps the others (it
/// may ask for more, e.g. for a new buffer).
fn present(
    window: &NativeWindow,
    frame: &Framebuffer,
    colors: [[u8; 3]; 2],
    rows: Option<Range<u16>>,
) {
    let (width, height) = (i32::from(frame.width()), i32::from(frame.height()));
    if let Err(e) =
        window.set_buffers_geometry(width, height, Some(HardwareBufferFormat::R8G8B8A8_UNORM))
    {
        log::warn!("window geometry: {e}");
        return;
    }
    let rows = rows.unwrap_or(0..frame.height());
    let mut bounds = ndk::native_window::Rect {
        left: 0,
        top: i32::from(rows.start),
        right: width,
        bottom: i32::from(rows.end),
    };
    let Ok(mut buffer) = window.lock(Some(&mut bounds)) else {
        return;
    };
    // The rows the window asks for.
    let rows =
        usize::try_from(bounds.top).unwrap_or(0)..usize::try_from(bounds.bottom).unwrap_or(0);
    let stride = buffer.stride();
    let Some(bytes) = buffer.bytes() else {
        return;
    };
    let tint = pixels::tint(colors);
    pixels::to_rgba(&frame.view(), rows, stride, &tint, |at, rgba| {
        if let Some(pixel) = bytes.get_mut(at..at + 4) {
            for (slot, value) in pixel.iter_mut().zip(rgba) {
                slot.write(value);
            }
        }
    });
}

/// Translates an Android input event; `None` leaves it to Android.
fn input(event: &AndroidInput<'_>) -> Option<Vec<InputEvent>> {
    match event {
        AndroidInput::KeyEvent(key) => {
            let name = keys::button(u32::from(key.key_code()))?;
            let state = match key.action() {
                KeyAction::Down if key.repeat_count() == 0 => ButtonState::Pressed,
                // Held keys repeat; the kernel measures long presses itself.
                KeyAction::Down => return Some(Vec::new()),
                KeyAction::Up => ButtonState::Released,
                _ => return Some(Vec::new()),
            };
            Some(vec![InputEvent::Button { name, state }])
        }
        AndroidInput::MotionEvent(motion) => {
            let phase = match motion.action() {
                MotionAction::Down | MotionAction::PointerDown => PointerPhase::Down,
                MotionAction::Up | MotionAction::PointerUp => PointerPhase::Up,
                MotionAction::Move => PointerPhase::Move,
                MotionAction::Cancel => PointerPhase::Cancel,
                _ => return Some(Vec::new()),
            };
            // Down and up concern one pointer; moves report all of them.
            let pointers: Vec<_> = match phase {
                PointerPhase::Move => motion.pointers().collect(),
                _ => vec![motion.pointer_at_index(motion.pointer_index())],
            };
            Some(
                pointers
                    .into_iter()
                    .map(|pointer| {
                        InputEvent::Pointer(PointerEvent::new(
                            pointer.pointer_id().clamp(0, 255) as u8,
                            phase,
                            Point::new(pointer.x() as i32, pointer.y() as i32),
                            match pointer.tool_type() {
                                ToolType::Stylus => PointerTool::Pen,
                                ToolType::Eraser => PointerTool::Eraser,
                                ToolType::Mouse => PointerTool::Mouse,
                                _ => PointerTool::Finger,
                            },
                            (pointer.pressure().clamp(0.0, 1.0) * 1000.0) as u16,
                        ))
                    })
                    .collect(),
            )
        }
        _ => None,
    }
}

/// Storage with mount points that vary (the private folder, one per SD
/// card).
struct Volumes(Box<dyn Storage>);

impl Storage for Volumes {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.0.read_at(path, offset, buf)
    }

    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.0.size(path)
    }

    fn modified(&self, path: &str) -> Option<i64> {
        self.0.modified(path)
    }

    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.0.write(path, data)
    }

    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.0.append(path, data)
    }

    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.0.list(dir, visit)
    }

    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.0.remove(path)
    }

    fn exists(&self, path: &str) -> bool {
        self.0.exists(path)
    }

    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.0.rename(from, to)
    }

    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        self.0.create_dir(path)
    }

    fn remove_dir_all(&self, path: &str) -> Result<(), StorageError> {
        self.0.remove_dir_all(path)
    }

    fn ignores_case(&self) -> bool {
        self.0.ignores_case()
    }

    fn free_space(&self) -> Option<u64> {
        self.0.free_space()
    }

    fn total_space(&self) -> Option<u64> {
        self.0.total_space()
    }

    fn local_path(&self, path: &str, with: &mut dyn FnMut(&str)) {
        self.0.local_path(path, with);
    }

    fn sync(&self) -> Result<(), StorageError> {
        self.0.sync()
    }
}

/// Random bytes from the operating system.
fn random(buf: &mut [u8]) -> bool {
    getrandom::fill(buf).is_ok()
}

/// Microseconds since start, for measuring what plugins take.
fn micros() -> u64 {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    START
        .get_or_init(std::time::Instant::now)
        .elapsed()
        .as_micros() as u64
}
