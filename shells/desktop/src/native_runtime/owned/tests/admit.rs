//! H1b: a real library that declares a screen is admitted by the kernel through
//! the production loader (`HostLoader::load_native`) and its screen opens and
//! draws through the library's own callback; one whose declarations the kernel
//! refuses is not loaded and leaves nothing mapped.
use paperpaw_core::native_host::registrations::{
    self as wire, Record, Registration, ScreenSpec, SettingGroup, SettingSpec,
};
use paperpaw_core::sdk::{Id, Params, PluginKind, PluginProblem, PluginState};

const ID: &str = "declscreen";
const TOKEN: u32 = 17;
/// What the library's draw callback does: a black bar on the canvas.
const DRAW: &str = "let _ = unsafe { (h.canvas_fill_rect)(s.ctx, 8, 8, 64, 32, 0x00000000) };";

struct Switches;
impl Drop for Switches {
    fn drop(&mut self) {
        crate::device::set_code_plugins(false);
        crate::native_runtime::set_enabled(false);
    }
}

/// A card with the library installed as plugin `ID` and trusted.
fn card(records: &[Record<'_>]) -> super::Fixture {
    let mut declarations = vec![0; wire::REGISTRATION_MAX_BYTES];
    let length = wire::records_encode(records, &mut declarations).unwrap();
    declarations.truncate(length);
    let mut callbacks = [0; 64];
    let length =
        wire::registrations_encode(&[Registration::Draw { token: TOKEN }], &mut callbacks).unwrap();
    let fixture = super::foreign::fixture_two(declarations, &callbacks[..length], DRAW);
    let install = fixture.root.join("plugins").join(ID);
    std::fs::create_dir_all(&install).unwrap();
    std::fs::copy(&fixture.path, install.join(crate::native::file_name())).unwrap();
    std::fs::write(
        install.join("plugin.ini"),
        format!("[plugin]\nid = {ID}\nname = Decl screen\nversion = 0.1.0\n"),
    )
    .unwrap();
    assert_eq!(crate::native::trust(&fixture.root).unwrap(), [ID]);
    fixture
}

fn mapped(id: &str) -> bool {
    std::fs::read_to_string("/proc/self/maps")
        .unwrap()
        .lines()
        .any(|line| line.contains("paperpaw-abi7") && line.contains(&format!("/{id}-")))
}

fn device(root: &std::path::Path) -> crate::device::Device {
    crate::device::set_code_plugins(true);
    crate::native_runtime::set_enabled(true);
    let profile = crate::profiles::find("boox-go-7").unwrap();
    let storage = crate::device::library(root).unwrap();
    crate::device::Device::new(profile, None, storage, true).unwrap()
}

#[test]
fn a_declared_screen_is_admitted_opens_and_draws_through_the_library() {
    let _one = crate::one_device_at_a_time();
    let fixture = card(&[Record::ScreenSpec(ScreenSpec {
        name: "screen",
        title: "declscreen.title",
        token: TOKEN,
        flags: 0,
    })]);
    let _switches = Switches;
    let mut device = device(&fixture.root);
    let plugin = device.kernel.plugins().get(ID).unwrap();
    assert_eq!(
        (plugin.state, plugin.kind, plugin.problem),
        (PluginState::Running, PluginKind::Code, None),
        "admitted, not the events-only path"
    );
    let screen = Id::parse("declscreen.screen").unwrap();
    assert!(device.kernel.known_screens().any(|id| *id == screen));
    // The panel before the screen is drawn: no black bar where the library's
    // draw callback puts it.
    let before = device.panel.rgb_bytes();
    device.open_screen(screen, Params::new());
    let after = device.panel.rgb_bytes();
    assert_ne!(before, after, "the library's draw callback ran");
    let width = usize::from(device.panel.width());
    let at = (8 * width + 8) * 3;
    // Black is not white (the panel's darkest gray is not zero here).
    assert!(
        after[at] < 64 && before[at] > after[at],
        "its bar is on the panel"
    );
    assert!(device.kernel.take_warnings().is_empty());
    assert!(mapped(ID));
    drop(device);
    assert!(!mapped(ID), "unmapped at the end");
}

#[test]
fn a_library_the_kernel_refuses_is_not_loaded_and_nothing_stays_mapped() {
    let _one = crate::one_device_at_a_time();
    // A secret setting passes the declaration checks and the host does not map
    // it yet: the whole batch is refused.
    let fixture = card(&[
        Record::SettingGroup(SettingGroup {
            name: "g",
            title: "declscreen.group",
            order: 0,
            parent: "",
            page: "",
        }),
        Record::SettingSpec(SettingSpec {
            name: "secret",
            kind: 0,
            flags: 1 << 3,
            group: "declscreen.g",
            title: "declscreen.secret",
            help: "",
            default: &[1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1],
            min: 0,
            max: 0,
            choices: "",
        }),
        Record::ScreenSpec(ScreenSpec {
            name: "screen",
            title: "declscreen.title",
            token: TOKEN,
            flags: 0,
        }),
    ]);
    let _switches = Switches;
    let mut device = device(&fixture.root);
    let plugin = device.kernel.plugins().get(ID).unwrap();
    assert_eq!(plugin.state, PluginState::Failed);
    assert_eq!(plugin.problem, Some(PluginProblem::NeedsNewerPaperPaw));
    let screen = Id::parse("declscreen.screen").unwrap();
    assert!(
        !device.kernel.known_screens().any(|id| *id == screen),
        "nothing of it is registered"
    );
    assert!(!mapped(ID), "its library is released");
    device.open_screen(screen, Params::new());
}

#[test]
fn a_library_whose_check_fails_leaves_no_private_copy_on_disk() {
    let _one = crate::one_device_at_a_time();
    // It needs a service no host offers: the metadata check refuses it before
    // any code runs, after its private copy was written.
    let metadata = crate::native_runtime::tests::service_metadata("core.no-such-service", 1, 0);
    let mut callbacks = [0; 64];
    let length =
        wire::registrations_encode(&[Registration::Draw { token: TOKEN }], &mut callbacks).unwrap();
    let fixture =
        super::foreign::fixture_service(vec![0], &callbacks[..length], DRAW, &metadata, &[]);
    let bytes = std::fs::read(&fixture.path).unwrap();
    let resources = paperpaw_core::native_host::HostResources::try_new().unwrap();
    let depth = paperpaw_core::native_host::flat::FlatDepth::try_new().unwrap();
    let result = crate::native_runtime::admission(ID, &bytes, resources, depth, None);
    assert!(
        matches!(
            result,
            Err(crate::native_runtime::Refusal::Runtime(
                crate::native_runtime::RuntimeError::Metadata
            ))
        ),
        "the metadata check refuses it"
    );
    let copies = super::super::dev::copies_dir().unwrap();
    let left: Vec<_> = std::fs::read_dir(copies)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(&format!("{ID}-"))
        })
        .collect();
    assert!(
        left.is_empty(),
        "a refused library leaves its copy: {left:?}"
    );
}

#[test]
fn a_card_plugin_is_admitted_against_what_the_pool_offers() {
    use paperpaw_native::metadata::Service;
    let _one = crate::one_device_at_a_time();
    // It needs a service the emulator's table does not have: refused with no
    // offer, and past the metadata check when the device's offer lists it
    // (what `linked::prepare` tells the pool is wired).
    let metadata = crate::native_runtime::tests::service_metadata("core.offered-here", 1, 0);
    let mut callbacks = [0; 64];
    let length =
        wire::registrations_encode(&[Registration::Draw { token: TOKEN }], &mut callbacks).unwrap();
    let fixture =
        super::foreign::fixture_service(vec![0], &callbacks[..length], DRAW, &metadata, &[]);
    let bytes = std::fs::read(&fixture.path).unwrap();
    static OFFERED: [Service<'static>; 1] = [Service {
        key: "core.offered-here",
        major: 1,
        semantic_revision: 0,
        min_table_bytes: 0,
    }];
    let offer = crate::native_runtime::Offer {
        services: &OFFERED,
        features: &[],
    };
    let admit = |offer| {
        let resources = paperpaw_core::native_host::HostResources::try_new().unwrap();
        let depth = paperpaw_core::native_host::flat::FlatDepth::try_new().unwrap();
        crate::native_runtime::admission(ID, &bytes, resources, depth, offer)
    };
    let refused = |result: &Result<_, crate::native_runtime::Refusal>| {
        matches!(
            result,
            Err(crate::native_runtime::Refusal::Runtime(
                crate::native_runtime::RuntimeError::Metadata
            ))
        )
    };
    assert!(refused(&admit(None)), "no offer: the service is unknown");
    assert!(!refused(&admit(Some(offer))), "the offer lists it");
}
