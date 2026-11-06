//! Battery pixel acceptance using private kernel-backed native dispatch hooks.
//! Registry activation is separate; no exported CoreFrame and no mocked slots.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "fixture failures fail acceptance"
)]
use crate::{
    Kernel,
    native_host::{DispatchDepth, HostResources, shell},
};
use alloc::{borrow::Cow, format, vec, vec::Vec};
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
use paperpaw_sdk::{Id, Manifest, Plugin, RegisterError, Registrar, SettingSpec, Version};
extern crate std;
use std::eprintln;
use std::{
    alloc::Layout,
    fs::File,
    io::{BufReader, BufWriter},
    path::Path,
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
    metadata = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../plugins/battery/native/metadata.host.bin"
    )),
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
fn report(_: &Heap, _: &str, s: crate::native_host::TeardownStep, e: crate::native_host::Error) {
    eprintln!("native teardown {s:?}: {e:?}");
}
// Test-only catalog/settings installation, never a Battery behavior adapter.
struct Catalogs;
impl Plugin for Catalogs {
    fn manifest(&self) -> Manifest {
        Manifest::new(
            Id::new_static("battery"),
            "Battery catalogs",
            Version::new(1, 0, 8),
        )
    }
    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_setting(SettingSpec::switch(
            "battery.enabled",
            "battery.setting.enabled",
            true,
            Id::new_static("power.group"),
        ))?;
        r.add_setting(SettingSpec::switch(
            "battery.by-plugin",
            "battery.setting.by-plugin",
            true,
            paperpaw_sdk::builtin::groups::DEVELOPER,
        ))?;
        r.add_translations(Cow::Borrowed(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../plugins/battery/i18n/en.ini"
        ))))?;
        r.add_translations(Cow::Borrowed(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../plugins/battery/i18n/de.ini"
        ))))?;
        Ok(())
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
    let checked = metadata::check(
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../plugins/battery/native/metadata.host.bin"
        )),
        &host,
    )
    .unwrap();
    shell::DeclaredServices::try_from_iter(checked.services()).unwrap()
}

fn render(kernel: &Kernel) -> (i32, Vec<u8>) {
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
    let n = crate::native_host::events::encode_started(200, &mut bytes).unwrap();
    let depth = DispatchDepth::new();
    let mut status = kernel
        .dispatch_native_for_test(
            &batch,
            0,
            &depth,
            shell::Invocation::Event(&bytes[..n]),
            Instant(200),
        )
        .unwrap();
    let mut framebuffer = Framebuffer::new(480, 800, PixelFormat::Gray4).unwrap();
    framebuffer
        .canvas()
        .fill_rect(0, 0, 480, 800, paperpaw_sdk::Color::WHITE);
    if status == 0 {
        status = kernel
            .draw_native_for_test(&batch, 1, &depth, Instant(200), &mut framebuffer.canvas())
            .unwrap();
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

use alloc::string::String;
use core::fmt::Write as _;
use paperpaw_platform::{Battery as Charge, SystemStatus};
use paperpaw_sdk::MemoryStorage;
const HEADER: &str =
    "kind,ms,millivolts,percent,charging,fast,partial,full,kilopixels,inputs,panel_ms,pages,unix\n";
fn few() -> String {
    format!(
        "{HEADER}start,0,4100,90,0,0,0,0,0,0,0,0,\n\
         end,3600000,4000,85,0,0,0,0,0,0,0,1,\n\
         start,4000000,4000,85,0,0,0,0,0,0,0,0,\n\
         end,7600000,3900,80,0,0,0,0,0,0,0,2,\n"
    )
}

/// Seventy completed times awake of different lengths, so that the list
/// takes several pages.
fn many() -> String {
    let mut log = String::from(HEADER);
    for i in 0..70_u64 {
        let start = i * 4 * 3_600_000;
        let hours = 1 + i % 5;
        let from = 100 - (i % 40) as u8;
        let to = from.saturating_sub(hours as u8 * 3);
        writeln!(log, "start,{start},4100,{from},0,0,0,0,0,0,0,0,").unwrap();
        writeln!(
            log,
            "end,{},3900,{to},0,0,0,0,0,0,0,{},",
            start + hours * 3_600_000,
            hours * 40
        )
        .unwrap();
    }
    log
}

struct Case {
    name: &'static str,
    language: &'static str,
    developer: bool,
    history: Option<String>,
    charge: Option<Charge>,
}
fn cases() -> Vec<Case> {
    let known = Some(Charge::new(73, false, Some(4012)));
    let mut cases = vec![];
    for (language, unknown, present, history) in [
        ("en", "empty-unknown-en", "empty-known-en", "few-en"),
        ("de", "empty-unknown-de", "empty-known-de", "few-de"),
    ] {
        for (name, history, charge) in [
            (unknown, None, None),
            (present, None, known),
            (history, Some(few()), known),
        ] {
            cases.push(Case {
                name,
                language,
                history,
                charge,
                developer: false,
            });
        }
    }
    for (name, language, developer, history, charge) in [
        (
            "charging",
            "en",
            false,
            few(),
            Charge::new(73, true, Some(4012)),
        ),
        (
            "charging-no-voltage",
            "en",
            false,
            few(),
            Charge::new(41, true, None),
        ),
        ("low", "en", false, few(), Charge::new(5, false, Some(3300))),
        ("empty", "en", false, few(), Charge::new(0, false, None)),
        (
            "charging-de",
            "de",
            false,
            few(),
            Charge::new(73, true, Some(4012)),
        ),
        (
            "many-page-1",
            "en",
            false,
            many(),
            Charge::new(64, false, None),
        ),
        (
            "developer-footer",
            "en",
            true,
            few(),
            Charge::new(73, false, Some(4012)),
        ),
    ] {
        cases.push(Case {
            name,
            language,
            developer,
            history: Some(history),
            charge: Some(charge),
        });
    }
    cases
}
fn matches_golden(name: &str, pixels: &[u8]) -> bool {
    let golden = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../../plugins/battery/testdata/screens/{name}.png"));
    let mut reader = png::Decoder::new(BufReader::new(File::open(golden).unwrap()))
        .read_info()
        .unwrap();
    let mut expected = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut expected).unwrap();
    expected.truncate(info.buffer_size());
    let out = std::env::temp_dir().join(format!("battery-native-{name}"));
    png_write(&out.with_extension("actual.png"), pixels);
    let diff: Vec<_> = pixels
        .iter()
        .zip(&expected)
        .map(|(a, b)| a.abs_diff(*b))
        .collect();
    png_write(&out.with_extension("diff.png"), &diff);
    let different = diff.iter().filter(|&&v| v != 0).count();
    eprintln!(
        "{name}: {different} changed pixels; diff {}",
        out.with_extension("diff.png").display()
    );
    pixels == expected
}
#[test]
fn native_battery_pixel_acceptance() {
    let mut failures = Vec::new();
    for case in cases() {
        let storage = case
            .history
            .as_ref()
            .map_or_else(MemoryStorage::new, |text| {
                MemoryStorage::new().with(".paperpaw/plugins/battery/log.csv", text)
            });
        let mut kernel = Kernel::builder(DEVICE)
            .storage(storage)
            .user_config(format!(
                "[core]\nlanguage = {}\ndeveloper = {}\n[battery]\nenabled = off\n",
                case.language,
                if case.developer { "on" } else { "off" }
            ))
            .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
            .plugin(Catalogs)
            .build()
            .unwrap();
        kernel.start(Instant(0));
        kernel.set_status(Instant(100), SystemStatus::new(None, case.charge));
        let (status, pixels) = render(&kernel);
        if status != 0 {
            eprintln!("{}: BLOCKED native render, status {status}", case.name);
            failures.push(case.name);
            continue;
        }
        if !matches_golden(case.name, &pixels) {
            failures.push(case.name);
        }
    }
    assert!(
        failures.is_empty(),
        "native pixel acceptance failed: {failures:?}"
    );
}
