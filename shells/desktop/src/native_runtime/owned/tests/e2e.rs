//! #747: a real plugin-owned library (`samples/abi7-tick-draw`) end to end
//! through the inactive owner path: preflight, make with wire registrations,
//! leased tick/draw dispatch inside a scoped environment (slots 4/5), teardown,
//! and clean refusals. Nothing here activates the production loader.
use super::super::canvas as canvas_bridge;
use super::super::environment as bridge;
use super::canvas::{gray, save_png};
use super::environment::{KEY, run};
use super::*;
use paperpaw_core::native_host::registrations::{self, Registration};
use paperpaw_core::sdk::SettingSpec;
use paperpaw_graphics::Framebuffer;
use paperpaw_platform::{Color, PixelFormat, SystemStatus};

/// Registered by the sample during make (see its README): tick token 1, draw token 2.
const TICK_INDEX: usize = 0;
const DRAW_INDEX: usize = 1;

/// Removes the directory when dropped, also when the build below fails.
struct RemoveOnDrop(std::path::PathBuf);

impl Drop for RemoveOnDrop {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn sample_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../samples/abi7-tick-draw")
        .canonicalize()
        .unwrap()
}

/// The sample's library bytes, built once per process: the first caller runs
/// the build (exactly as the sample's `verify.sh` does, into a private target
/// directory that is removed again), every test then writes its own copy into
/// its own fixture root, which its `Fixture` removes.
pub(super) fn sample_bytes() -> &'static [u8] {
    static BYTES: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
    BYTES.get_or_init(|| {
        let root = RemoveOnDrop(crate::private::dir("paperpaw-e2e747-build").unwrap());
        let target = root.0.join("target");
        let mut build = Command::new("cargo");
        // The nested build depends on nothing but the toolchain: the test run's
        // own CARGO_* settings (target, profile, flags) must not change it, so
        // the environment is cleared and only what finds the toolchain and
        // `python3` (for the sample's `build.rs`) is passed on.
        build.env_clear();
        for name in [
            "PATH",
            "HOME",
            "RUSTUP_HOME",
            "CARGO_HOME",
            "RUSTUP_TOOLCHAIN",
        ] {
            if let Some(value) = std::env::var_os(name) {
                build.env(name, value);
            }
        }
        let output = build
            .current_dir(sample_dir())
            .env("CARGO_TARGET_DIR", &target)
            .args([
                "rustc",
                "--release",
                "--lib",
                "--locked",
                "--offline",
                "-j4",
                "--",
                "--crate-type=cdylib",
                "-Cpanic=abort",
                "-Clink-arg=-nostartfiles",
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        // The override of the crate type leaves the library in `deps/`, hashed.
        let built = std::fs::read_dir(target.join("release/deps"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| {
                        name.starts_with("libpaperpaw_sample_abi7_tick_draw-")
                            && name.ends_with(".so")
                    })
            })
            .expect("the sample's cdylib");
        std::fs::read(built).unwrap()
    })
}

/// The sample in a fixture root of its own.
fn sample() -> Fixture {
    let root = crate::private::dir("paperpaw-e2e747").unwrap();
    let path = root.join("sample.so");
    std::fs::write(&path, sample_bytes()).unwrap();
    let digest = paperpaw_core::sdk::digest::sha256::digest(&std::fs::read(&path).unwrap());
    Fixture { root, path, digest }
}

/// Services for loads that must be refused before any code runs: any call
/// into them means a library executed, which is the failure under test.
struct Refused;
impl Services for Refused {
    fn alloc(&self, _: usize, _: usize) -> *mut u8 {
        panic!("a refused library must not allocate")
    }
    unsafe fn dealloc(&self, _: *mut u8, _: usize, _: usize) {
        panic!("a refused library must not free")
    }
    unsafe fn realloc(&self, _: *mut u8, _: usize, _: usize, _: usize) -> *mut u8 {
        panic!("a refused library must not reallocate")
    }
    fn micros(&self) -> u64 {
        panic!("a refused library must not read the clock")
    }
}

fn white_frame() -> Framebuffer {
    let mut frame = Framebuffer::new(160, 80, PixelFormat::Gray8).unwrap();
    frame.canvas().fill_rect(0, 0, 160, 80, Color::WHITE);
    frame
}

/// The sample's draw fills one black rectangle: x=8, y=8, 64x32 (plugin-owned
/// constants, see its README). Everything else stays white.
fn assert_sample_painting(frame: &Framebuffer) {
    for (x, y, want) in [
        (8, 8, 0),
        (71, 39, 0),
        (40, 20, 0),
        (7, 8, 255),
        (8, 7, 255),
        (72, 39, 255),
        (71, 40, 255),
        (0, 0, 255),
        (159, 79, 255),
    ] {
        assert_eq!(gray(frame, x, y), want, "pixel ({x},{y})");
    }
}

fn tick(now: u64) -> Vec<u8> {
    let mut bytes = [0; 64];
    let len = registrations::tick_encode(now, &mut bytes).unwrap();
    bytes[..len].to_vec()
}

static BOOL: [SettingSpec; 1] = [SettingSpec::switch(
    KEY,
    "enabled",
    false,
    paperpaw_core::sdk::builtin::groups::OTHER,
)];
static TEXT: [SettingSpec; 1] = [SettingSpec::text(
    KEY,
    "enabled",
    "",
    paperpaw_core::sdk::builtin::groups::OTHER,
)];

#[test]
fn real_sample_registers_and_dispatches_tick_and_draw_with_environment() {
    let fixture = Rc::new(sample());
    for value in ["true", "false"] {
        let fixture = fixture.clone();
        run(&BOOL, value, SystemStatus::default(), move |env| {
            let (plugin, evidence) = fixture.open();
            let batch = plugin
                .make_registered(plugin.begin::<Registration>().unwrap())
                .unwrap()
                .commit()
                .unwrap();
            assert_eq!(batch.len(), 2, "tick and draw registrations only");
            assert_eq!(
                evidence.allocations.get(),
                1,
                "the sample allocates its state once, from the host heap"
            );
            let depth = DispatchDepth::new();
            let mut frame = white_frame();
            // No frame at all: the sample cannot read slots 4/5 and paints nothing.
            assert_eq!(
                dispatch_registration(
                    &batch,
                    DRAW_INDEX,
                    &depth,
                    Invocation::Draw(&screen_frame())
                )
                .unwrap(),
                host_glue::ERR_STATE
            );
            // A query-only frame (Env, no Canvas): slots 4/5 work, slot 9 answers
            // ERR_STATE, the sample returns that status unchanged, nothing is painted.
            assert_eq!(
                bridge::dispatch(
                    &batch,
                    DRAW_INDEX,
                    &depth,
                    Invocation::Draw(&screen_frame()),
                    env
                )
                .unwrap(),
                host_glue::ERR_STATE
            );
            assert_eq!(gray(&frame, 8, 8), 255, "nothing painted without a canvas");
            for now in [41, 42] {
                assert_eq!(
                    bridge::dispatch(
                        &batch,
                        TICK_INDEX,
                        &depth,
                        Invocation::Tick(&tick(now)),
                        env
                    )
                    .unwrap(),
                    host_glue::OK
                );
                // A full draw frame: Env (slots 4/5) and Canvas (slot 9).
                assert_eq!(
                    canvas_bridge::dispatch_draw(
                        &batch,
                        DRAW_INDEX,
                        &depth,
                        env,
                        &mut frame.canvas()
                    )
                    .unwrap(),
                    host_glue::OK
                );
                assert_sample_painting(&frame);
            }
            if value == "true"
                && let Some(path) = std::env::var_os("PAPERPAW_E2E_SCREENSHOT")
            {
                save_png(&frame, Path::new(&path));
            }
            // A malformed tick is refused before the foreign call.
            let mut bad = tick(43);
            bad.pop();
            assert!(matches!(
                bridge::dispatch(&batch, TICK_INDEX, &depth, Invocation::Tick(&bad), env),
                Err(Error::Status(host_glue::ERR_INVALID))
            ));
            drop(batch);
            plugin.owner.as_ref().revoke();
            plugin.owner.as_ref().finish().unwrap();
            assert_eq!(evidence.frees.get(), 1, "state returned to the host heap");
            drop(plugin);
            assert!(!mapped(&fixture.path), "module unmapped after owner drop");
        });
    }
}

#[test]
fn real_sample_draw_fails_closed_on_oversized_setting() {
    let fixture = Rc::new(sample());
    // 52 bytes is the sample's window; a longer text must surface ERR_NO_SPACE.
    let long = "x".repeat(100);
    run(&TEXT, &long, SystemStatus::default(), move |env| {
        let (plugin, _) = fixture.open();
        let batch = plugin
            .make_registered(plugin.begin::<Registration>().unwrap())
            .unwrap()
            .commit()
            .unwrap();
        let depth = DispatchDepth::new();
        let mut frame = white_frame();
        assert_eq!(
            canvas_bridge::dispatch_draw(&batch, DRAW_INDEX, &depth, env, &mut frame.canvas())
                .unwrap(),
            host_glue::ERR_NO_SPACE
        );
        assert_eq!(
            gray(&frame, 8, 8),
            255,
            "the settings failure precedes any painting"
        );
        drop(batch);
        plugin.owner.as_ref().revoke();
        plugin.owner.as_ref().finish().unwrap();
    });
}

#[test]
fn real_sample_refusals_are_clean_errors_not_panics() {
    let fixture = sample();
    let bytes = std::fs::read(&fixture.path).unwrap();
    let open = |path: &Path, digest: &[u8; 32]| {
        // SAFETY: private immutable inode in the fixture root, as in `Fixture::open`.
        let result = unsafe { PinnedPlugin::open(path, digest, "fixture", Refused) };
        result.err()
    };
    // Wrong digest: bytes-only refusal before any code runs.
    assert!(open(&fixture.path, &[0; 32]).is_some());
    // Truncated and garbage libraries with matching digests: preflight refuses.
    for (name, data) in [
        ("truncated.so", bytes[..bytes.len() / 2].to_vec()),
        ("header-only.so", bytes[..64].to_vec()),
        ("garbage.so", vec![0x5a; 4096]),
        ("empty.so", Vec::new()),
    ] {
        let path = fixture.root.join(name);
        std::fs::write(&path, &data).unwrap();
        let digest = paperpaw_core::sdk::digest::sha256::digest(&data);
        assert!(open(&path, &digest).is_some(), "{name} must be refused");
        assert!(!mapped(&path), "{name} was never mapped");
    }
}

/// A well-formed interface-6 (beta 3) native library: its 32-byte `Entry`
/// (fingerprint, `make() -> *mut c_void`, `needs_api`, interface 6, family
/// revisions) plus the `PAPERPAW_API` / `PAPERPAW_TABLE_n` words, and no
/// interface-7 metadata. It must be refused by its missing metadata export
/// before any code runs, never read as a `PpEntry`.
const INTERFACE_6_PLUGIN: &str = r#"#![no_std]
#[panic_handler] fn panic(_: &core::panic::PanicInfo) -> ! { loop {} }
#[repr(C)] pub struct Entry {
    fingerprint: u64,
    make: unsafe extern "C" fn() -> *mut core::ffi::c_void,
    needs_api: u16,
    interface: u16,
    families: [u16; 8],
}
unsafe impl Sync for Entry {}
unsafe extern "C" fn make() -> *mut core::ffi::c_void { core::ptr::null_mut() }
#[unsafe(no_mangle)] pub static PAPERPAW_ENTRY: Entry = Entry {
    fingerprint: 0x0006c55d0a4fa341, make, needs_api: 64, interface: 6, families: [0; 8],
};
#[unsafe(no_mangle)] pub static mut PAPERPAW_API: u32 = 0;
#[unsafe(no_mangle)] pub static mut PAPERPAW_TABLE_0: u32 = 0;
#[unsafe(no_mangle)] pub static mut PAPERPAW_TABLE_1: u32 = 0;
"#;

#[test]
fn well_formed_interface_6_library_is_refused_before_any_code_runs() {
    let root = crate::private::dir("paperpaw-e2e747-v6").unwrap();
    let input = root.join("old.rs");
    let path = root.join("old.so");
    std::fs::write(&input, INTERFACE_6_PLUGIN).unwrap();
    let result = Command::new("rustc")
        .args([
            "--edition=2024",
            "--crate-type=cdylib",
            "-Cpanic=abort",
            "-Copt-level=2",
            "-Clink-arg=-nostartfiles",
        ])
        .arg(&input)
        .arg("-o")
        .arg(&path)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let digest = paperpaw_core::sdk::digest::sha256::digest(&std::fs::read(&path).unwrap());
    // SAFETY: private immutable inode; refusal happens on bytes before any load.
    let refused = unsafe { PinnedPlugin::open(&path, &digest, "fixture", Refused) };
    assert!(
        matches!(refused, Err(Error::Runtime(RuntimeError::Export))),
        "interface 6 must be refused with the Export status"
    );
    assert!(!mapped(&path), "the library was never mapped");
    std::fs::remove_dir_all(root).unwrap();
}

/// Manual evidence case: a real interface-6 (v0.6.0-beta.3) host library, the
/// release's own `crates/paperpaw-core/tests/native-plugin` built from tag
/// 2affa8d59 with `cargo build --release` in a private target directory.
/// Run with `PAPERPAW_BETA3_PLUGIN=<.so> cargo test ... -- --ignored`.
#[test]
#[ignore = "needs a library built from the beta-3 tag (PAPERPAW_BETA3_PLUGIN)"]
fn real_beta3_host_library_is_refused_cleanly() {
    let source = std::env::var_os("PAPERPAW_BETA3_PLUGIN")
        .expect("set PAPERPAW_BETA3_PLUGIN to the beta-3 library");
    let root = crate::private::dir("paperpaw-e2e747-beta3").unwrap();
    let path = root.join("beta3.so");
    std::fs::copy(source, &path).unwrap();
    let digest = paperpaw_core::sdk::digest::sha256::digest(&std::fs::read(&path).unwrap());
    // SAFETY: private immutable inode; refusal happens on bytes before any load.
    let refused = unsafe { PinnedPlugin::open(&path, &digest, "fixture", Refused) };
    eprintln!("beta-3 library refused with: {:?}", refused.as_ref().err());
    assert!(
        matches!(refused, Err(Error::Runtime(RuntimeError::Export))),
        "a beta-3 library must be refused by its missing interface-7 metadata"
    );
    assert!(!mapped(&path), "the library was never mapped");
    std::fs::remove_dir_all(root).unwrap();
}
