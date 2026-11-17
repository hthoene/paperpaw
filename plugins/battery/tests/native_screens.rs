//! An Env-only host scope refuses Battery's Core-only data services.
//! Positive pixel acceptance uses real CoreFrame views in paperpaw-core's
//! native_battery_pixel_tests, covering all 16 unchanged Battery goldens.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "fixture failures fail acceptance"
)]
use paperpaw_core::sdk::{Env, Id, Manifest, Version, builtin};
use paperpaw_core::{
    Kernel, KernelBuilder,
    native_host::{DispatchDepth, HostResources, shell},
};
use paperpaw_native::host_glue::Services;
use paperpaw_platform::{
    DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, PixelFormat, Rotation,
};
use paperpaw_plugin_battery::native_plugin::NativeBattery;
use paperpaw_sdk::native::{
    self, Error, Host,
    callbacks::DrawFrame,
    declare::{Declarations, EventKind},
    events::{Event, Incoming},
    reply::Reply,
};
use std::{alloc::Layout, cell::RefCell, rc::Rc};

const DEVICE: DeviceInfo = DeviceInfo::new(
    "battery-native-screens",
    "Battery native screens",
    DisplayInfo::new(480, 800, 220, PixelFormat::Gray4, Rotation::Deg0, None),
    InputCaps::new(true, false, false, &["page-forward", "page-back", "back"]),
    MemoryClass::Small,
);

// Callback-only fixture: registry publication is separately owned by Core #944.
// Do not use this adapter in production or claim it proves built-in activation.
struct ScreenProbe(NativeBattery);
impl native::Plugin for ScreenProbe {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        let mut d =
            Declarations::new(|b| native::registrations::submit(host, b).map_err(i32::from));
        d.subscribe_event(EventKind::Started, 10)
            .map_err(|_| Error::Invalid)?
            .register_draw(10)
            .map_err(|_| Error::Invalid)?
            .submit()
            .map_err(|_| Error::Invalid)?;
        Ok(Self(NativeBattery::default()))
    }
    fn event(
        &mut self,
        h: &Host<'_>,
        t: u32,
        e: Incoming<'_>,
        r: &mut Reply<'_>,
    ) -> Result<(), Error> {
        // Test-only trigger: screen routing is Core #944, not this pixel fixture.
        let Event::Started { now_ms, .. } = e.decode()? else {
            return Err(Error::Invalid);
        };
        assert!(native::snapshot::capture_snapshot(h).is_ok());
        assert!(matches!(
            native::services::device_usage(h),
            Err(Error::State)
        ));
        let font = native::fonts::Font::acquire_role(h, native::fonts::FontRole::Ui)?;
        assert!(font.read_line_metrics(16.0).is_ok());
        assert!(matches!(font.ink(16.0, "H"), Err(Error::State)));
        let mut bytes = [0; 128];
        let len = paperpaw_core::native_host::events::encode(
            &paperpaw_core::native_host::registrations::Record::EventScreenShown(
                paperpaw_core::native_host::registrations::EventScreenShown {
                    last_input: 0,
                    now_ms,
                    screen: "battery.screen",
                    width: 480,
                    height: 800,
                    instance: None,
                },
            ),
            &mut bytes,
        )
        .map_err(|_| Error::Invalid)?;
        native::Plugin::event(&mut self.0, h, t, Incoming::new(&bytes[..len])?, r)
    }
    fn draw(&mut self, h: &Host<'_>, t: u32, f: DrawFrame<'_>) -> Result<(), Error> {
        native::Plugin::draw(&mut self.0, h, t, f)
    }
}
paperpaw_sdk::native_plugin!(
    ScreenProbe,
    metadata = include_bytes!("../native/metadata.host.bin"),
    linked = SCREEN_ENTRY
);
struct Heap;
impl Services for Heap {
    fn alloc(&self, s: usize, a: usize) -> *mut u8 {
        // SAFETY: host validates the nonzero layout before forwarding.
        unsafe { std::alloc::alloc(Layout::from_size_align(s, a).unwrap()) }
    }
    unsafe fn dealloc(&self, p: *mut u8, s: usize, a: usize) {
        // SAFETY: this allocation was returned by this heap with the same layout.
        unsafe { std::alloc::dealloc(p, Layout::from_size_align(s, a).unwrap()) }
    }
    unsafe fn realloc(&self, p: *mut u8, s: usize, a: usize, n: usize) -> *mut u8 {
        // SAFETY: the allocation belongs to this heap and n is checked by the host.
        unsafe { std::alloc::realloc(p, Layout::from_size_align(s, a).unwrap(), n) }
    }
    fn micros(&self) -> u64 {
        0
    }
}
fn report(
    _: &Heap,
    _: &str,
    s: paperpaw_core::native_host::TeardownStep,
    e: paperpaw_core::native_host::Error,
) {
    eprintln!("native teardown {s:?}: {e:?}");
}
type Run = Rc<dyn Fn(&Env<'_>)>;
std::thread_local! {
    /// What runs with the live `Env` once the kernel has started, in the
    /// test running (the handler is a plain function and holds nothing).
    static CAPTURE: std::cell::RefCell<Option<Run>> = const { std::cell::RefCell::new(None) };
}
/// Runs `run` with the live `Env` when the kernel has started.
fn capture(builder: KernelBuilder, run: Run) -> KernelBuilder {
    let manifest = Manifest::new(
        Id::new_static("native-screen-capture"),
        "Native screen capture",
        Version::new(1, 0, 0),
    );
    CAPTURE.with(|capture| *capture.borrow_mut() = Some(run));
    builder.items(manifest, |items| {
        items.subscribe(builtin::events::STARTED, |k, _| {
            if let Some(run) = CAPTURE.with(|capture| capture.borrow().clone()) {
                k.with_env(|env, _| run(env));
            }
        })
    })
}
fn declared_services() -> shell::DeclaredServices {
    use paperpaw_native::metadata::{self, Host as AdmissionHost, Profile, Service};
    let services = [
        "core.plugin",
        "core.plugin-usage",
        "core.usage",
        "core.work-note",
    ]
    .map(|key| Service {
        key,
        major: 1,
        semantic_revision: 0,
        min_table_bytes: 0,
    });
    let host = AdmissionHost {
        abi_major: 7,
        minor: 1,
        profile: Profile {
            target: "linux-x86_64-sysv64-le",
            entry_layout: 1,
            entry_bytes: 32,
        },
        services: &services,
        features: &[],
    };
    let checked = metadata::check(include_bytes!("../native/metadata.host.bin"), &host).unwrap();
    shell::DeclaredServices::try_from_iter(checked.services()).unwrap()
}

fn dispatch_without_core_views(env: &Env<'_>) -> i32 {
    // SAFETY: statically linked code and metadata live for the whole test; no image range.
    let hosted = unsafe {
        shell::Hosted::new(
            Heap,
            (),
            SCREEN_ENTRY.entry().make.unwrap(),
            "battery",
            (core::ptr::null(), 0),
            report,
            HostResources::try_new().unwrap(),
            declared_services(),
        )
    }
    .unwrap();
    let batch = hosted
        .construct_registered(hosted.begin().unwrap())
        .unwrap();
    let mut bytes = [0; 256];
    let n = paperpaw_core::native_host::events::encode_started(0, &mut bytes).unwrap();
    let depth = DispatchDepth::new();
    let status = shell::dispatch_with_env(
        &batch,
        0,
        &depth,
        shell::Invocation::Event(&bytes[..n]),
        env,
    )
    .unwrap();
    drop(batch);
    hosted.stop();
    hosted.owner().finish().unwrap();
    status
}
#[test]
fn env_only_dispatch_refuses_core_only_services() {
    for language in ["en", "de"] {
        let result = Rc::new(RefCell::new(None));
        let captured = Rc::clone(&result);
        let run = Rc::new(move |env: &Env<'_>| {
            *captured.borrow_mut() = Some(dispatch_without_core_views(env));
        });
        let builder = Kernel::builder(DEVICE)
            .user_config(format!(
                "[core]\nlanguage = {language}\n[battery]\nenabled = off\n"
            ))
            .built_in_fonts(&paperpaw_plugin_fonts_default::DEFAULT_FONT_PACKS);
        let mut kernel = capture(builder, run).build().unwrap();
        kernel.start(Instant(0));
        let status = result.borrow_mut().take().expect("capture ran");
        assert_eq!(status, paperpaw_native::host_glue::ERR_STATE);
    }
}
