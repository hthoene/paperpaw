//! Native pixel acceptance through Core's shared host table. No golden blessing.
//! The capture plugin supplies Env only; Battery's screen executes native callbacks.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "fixture failures fail acceptance"
)]
use paperpaw_core::{
    Kernel,
    native_host::{DispatchDepth, HostResources, shell},
};
use paperpaw_graphics::Framebuffer;
use paperpaw_native::host_glue::Services;
use paperpaw_platform::{
    DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, PixelFormat, Rotation,
};
use paperpaw_plugin_battery::native_plugin::NativeBattery;
use paperpaw_sdk::native::{
    self, Error, Host,
    callbacks::DrawFrame,
    declare::{Declarations, EventKind},
    events::Event,
    reply::Reply,
};
use paperpaw_sdk::{Env, Id, Manifest, Plugin, RegisterError, Registrar, Version, builtin};
use std::{
    alloc::Layout,
    cell::RefCell,
    fs::File,
    io::{BufReader, BufWriter},
    path::Path,
    rc::Rc,
};

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
        e: Event<'_>,
        r: &mut Reply<'_>,
    ) -> Result<(), Error> {
        // Test-only trigger: screen routing is Core #944, not this pixel fixture.
        let Event::Started { now_ms } = e else {
            return Err(Error::Invalid);
        };
        eprintln!(
            "native snapshot: {:?}",
            native::snapshot::capture_snapshot(h).map(|_| ())
        );
        eprintln!(
            "native device usage: {:?}",
            native::services::device_usage(h)
        );
        match native::fonts::Font::acquire_role(h, native::fonts::FontRole::Ui) {
            Ok(font) => {
                eprintln!("native text metrics: {:?}", font.read_line_metrics(16.0));
                eprintln!("native text ink: {:?}", font.ink(16.0, "H"));
            }
            Err(error) => eprintln!("native font acquisition: {error:?}"),
        }
        native::Plugin::event(
            &mut self.0,
            h,
            t,
            Event::ScreenShown {
                now_ms,
                screen: "battery.screen",
                width: 480,
                height: 800,
            },
            r,
        )
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
struct Capture(Run);
impl Plugin for Capture {
    fn manifest(&self) -> Manifest {
        Manifest::new(
            Id::new_static("native-screen-capture"),
            "Native screen capture",
            Version::new(1, 0, 0),
        )
    }
    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let run = Rc::clone(&self.0);
        r.subscribe(
            builtin::events::STARTED,
            Box::new(move |env, _, _| run(env)),
        )
    }
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

fn render(env: &Env<'_>) -> (i32, Vec<u8>) {
    // SAFETY: statically linked code and metadata live for the whole test; no image range.
    let hosted = unsafe {
        shell::Hosted::new(
            Heap,
            (),
            SCREEN_ENTRY.entry().make.unwrap(),
            "battery",
            (0, 0),
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
    let mut status = shell::dispatch_with_env(
        &batch,
        0,
        &depth,
        shell::Invocation::Event(&bytes[..n]),
        env,
    )
    .unwrap();
    let mut framebuffer = Framebuffer::new(480, 800, PixelFormat::Gray4).unwrap();
    framebuffer
        .canvas()
        .fill_rect(0, 0, 480, 800, paperpaw_sdk::Color::WHITE);
    if status == 0 {
        status = shell::dispatch_draw(&batch, 1, &depth, env, &mut framebuffer.canvas()).unwrap();
    }
    let view = framebuffer.view();
    let mut pixels = Vec::with_capacity(480 * 800);
    for y in 0..800 {
        for x in 0..480 {
            pixels.push(view.pixel(x, y).luma());
        }
    }
    drop(batch);
    hosted.stop();
    hosted.owner().finish().unwrap();
    (status, pixels)
}
fn png_write(path: &Path, pixels: &[u8]) {
    let mut encoder = png::Encoder::new(BufWriter::new(File::create(path).unwrap()), 480, 800);
    encoder.set_color(png::ColorType::Grayscale);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .unwrap()
        .write_image_data(pixels)
        .unwrap();
}
#[test]
fn native_empty_screen_matches_existing_goldens() {
    for language in ["en", "de"] {
        let result = Rc::new(RefCell::new(None));
        let captured = Rc::clone(&result);
        let run = Rc::new(move |env: &Env<'_>| {
            *captured.borrow_mut() = Some(render(env));
        });
        let mut kernel = Kernel::builder(DEVICE)
            .user_config(format!(
                "[core]\nlanguage = {language}\n[battery]\nenabled = off\n"
            ))
            .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
            .plugin(paperpaw_plugin_battery::Battery)
            .plugin(Capture(run))
            .build()
            .unwrap();
        kernel.start(Instant(0));
        let (status, pixels) = result.borrow_mut().take().expect("capture ran");
        let name = format!("empty-unknown-{language}");
        let golden =
            Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("testdata/screens/{name}.png"));
        let mut reader = png::Decoder::new(BufReader::new(File::open(golden).unwrap()))
            .read_info()
            .unwrap();
        let mut expected = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut expected).unwrap();
        expected.truncate(info.buffer_size());
        let out = std::env::temp_dir().join(format!("battery-native-{name}"));
        png_write(&out.with_extension("actual.png"), &pixels);
        let diff: Vec<_> = pixels
            .iter()
            .zip(&expected)
            .map(|(a, b)| a.abs_diff(*b))
            .collect();
        png_write(&out.with_extension("diff.png"), &diff);
        assert_eq!(
            status,
            0,
            "native callback refused before pixel comparison; blank actual is a BLOCKED render, not layout drift; diff: {}",
            out.with_extension("diff.png").display()
        );
        assert_eq!(
            pixels,
            expected,
            "native pixel mismatch: {}",
            out.with_extension("diff.png").display()
        );
    }
}
