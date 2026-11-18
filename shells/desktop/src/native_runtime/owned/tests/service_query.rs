//! A real admitted library reads core usage through the production flat loader.
use paperpaw_core::{
    native_host::{events, registrations as wire},
    sdk::usage::Usage,
};
use paperpaw_platform::Instant;

const ID: &str = "usage-probe";
const QUERY: &str = r#"
    let key = PpBytes { ptr: b"core.usage".as_ptr(), len: 10 };
    let mut blob = 0;
    let status = unsafe { (h.services_query)(s.ctx, key, 1, 0, &mut blob) };
    if status != OK { return status; }
    let mut second = 0x9876;
    let status = unsafe { (h.services_query)(s.ctx, key, 1, 0, &mut second) };
    if status != ERR_BUSY || second != 0x9876 { return ERR_INVALID; }
    let mut bytes = [0u8; 128];
    let out = PpBuffer { ptr: bytes.as_mut_ptr(), cap: bytes.len() };
    let count = unsafe { (h.blob_read)(s.ctx, blob, 0, out) };
    if count <= 0 { return ERR_INVALID; }
    let status = unsafe { (h.blob_release)(s.ctx, blob) };
    if status != OK { return status; }
    if unsafe { (h.blob_read)(s.ctx, blob, 0, out) } != ERR_STATE { return ERR_INVALID; }
    let status = unsafe { (h.storage_write)(s.ctx, 0,
        PpBytes { ptr: b"snapshot".as_ptr(), len: 8 },
        PpBytes { ptr: bytes.as_ptr(), len: count as usize }) };
    if status != OK { return status; }
"#;

struct Switches;
impl Drop for Switches {
    fn drop(&mut self) {
        crate::device::set_code_plugins(false);
        crate::native_runtime::set_enabled(false);
    }
}

fn encoded_usage(usage: &Usage) -> Vec<u8> {
    let record = wire::Record::ServiceUsage(wire::ServiceUsage {
        fast: usage.refreshes.fast,
        partial: usage.refreshes.partial,
        full: usage.refreshes.full,
        pixels: usage.pixels,
        inputs: usage.inputs,
        sleeps: usage.sleeps,
        panel_ms: usage.panel_ms,
        bluetooth_ms: usage.bluetooth_ms,
    });
    let mut bytes = [0; 128];
    let length = wire::record_encode_one(&record, &mut bytes).unwrap();
    bytes[..length].to_vec()
}

#[test]
fn real_loaded_library_usage_query_matches_kernel_view() {
    let _one = crate::one_device_at_a_time();
    let metadata = crate::native_runtime::tests::service_metadata("core.usage", 1, 0);
    let mut event = [0; 12];
    let length = events::encode_started(0, &mut event).unwrap();
    let mut batch = [0; 32];
    let count = wire::registrations_encode(
        &[wire::Registration::Event {
            event_tag: wire::EVENT_STARTED_TAG,
            token: 17,
        }],
        &mut batch,
    )
    .unwrap();
    let fixture = super::foreign::fixture_service(
        batch[..count].to_vec(),
        &[],
        QUERY,
        &metadata,
        &event[..length],
    );
    let install = fixture.root.join("plugins").join(ID);
    std::fs::create_dir_all(&install).unwrap();
    std::fs::copy(&fixture.path, install.join(crate::native::file_name())).unwrap();
    std::fs::write(
        install.join("plugin.ini"),
        "[plugin]\nid = usage-probe\nname = Usage probe\nversion = 0.1.0\n",
    )
    .unwrap();
    let data = fixture.root.join(".paperpaw/plugins").join(ID);
    std::fs::create_dir_all(&data).unwrap();
    assert_eq!(crate::native::trust(&fixture.root).unwrap(), [ID]);
    let _switches = Switches;
    crate::device::set_code_plugins(true);
    crate::native_runtime::set_enabled(true);
    let profile = crate::profiles::find("boox-go-7").unwrap();
    let storage = crate::device::library(&fixture.root).unwrap();
    let mut device = crate::device::Device::new(profile, None, storage, true).unwrap();
    device.kernel.panel_busy(37);
    // STARTED is queued by the kernel and told by the pump between callbacks,
    // inside the start (its first passes run before its first frame): the
    // library reads this usage view through the actual loaded callback and
    // CoreFrame then, before the initial refresh is counted. The view the
    // kernel has just before the start is the one it is told.
    let expected = encoded_usage(device.kernel.usage());
    let _effects = device.kernel.start(Instant(0));
    assert_eq!(device.kernel.plugins().get(ID).unwrap().problem, None);
    let _effects = device.kernel.tick(Instant(1));
    let actual = std::fs::read(data.join("snapshot")).unwrap();
    assert_eq!(actual, expected);
}
