//! H1/H2/H4: actual library submission/dispatch through the reviewed wire bridge.
use super::*;
use paperpaw_core::native_host::registrations;
use registrations::{EVENT_TICK_TAG, Registration};

const TOKEN: u32 = 17;

fn records_bytes(records: &[Registration]) -> Vec<u8> {
    let mut bytes = vec![0; registrations::REGISTRATION_MAX_BYTES];
    let len = registrations::registrations_encode(records, &mut bytes).unwrap();
    bytes.truncate(len);
    bytes
}

fn tick_bytes() -> Vec<u8> {
    let mut bytes = [0; 64];
    let len = registrations::tick_encode(41, &mut bytes).unwrap();
    bytes[..len].to_vec()
}

fn records() -> [Registration; 2] {
    [
        Registration::Event {
            event_tag: EVENT_TICK_TAG,
            token: TOKEN,
        },
        Registration::Draw { token: TOKEN },
    ]
}

fn fixture(invalid: &[u8], ignore_invalid: bool) -> Fixture {
    fixture_query(invalid, ignore_invalid, "")
}

pub(super) fn fixture_query(invalid: &[u8], ignore_invalid: bool, query: &str) -> Fixture {
    fixture_custom(
        records_bytes(&records()),
        invalid,
        ignore_invalid,
        query,
        "",
    )
}

/// The same library with its valid batch given (wire bytes) and `prelude`
/// source added at its top (e.g. an extra export).
pub(super) fn fixture_custom(
    valid: Vec<u8>,
    invalid: &[u8],
    ignore_invalid: bool,
    query: &str,
    prelude: &str,
) -> Fixture {
    let root = crate::private::dir("paperpaw-foreign726").unwrap();
    let path = root.join("fixture.so");
    let abi = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../abi/generated/paperpaw_abi.rs")
        .canonicalize()
        .unwrap();
    let metadata = crate::native_runtime::tests::metadata_bytes();
    let mut source = include_str!("../fixtures/registration.rs.in").to_owned();
    for (name, value) in [
        ("ABI_PATH", format!("{abi:?}")),
        ("METADATA_LEN", metadata.len().to_string()),
        ("METADATA_BYTES", format!("{metadata:?}")),
        ("NEEDS_MINOR", "1".to_owned()),
        ("VALID_BATCH", format!("{valid:?}")),
        ("INVALID_BATCH", format!("{invalid:?}")),
        ("EXPECTED_EVENT", format!("{:?}", tick_bytes())),
        ("TOKEN", TOKEN.to_string()),
        ("IGNORE_INVALID", ignore_invalid.to_string()),
        ("QUERY_BODY", query.to_owned()),
    ] {
        source = source.replace(&format!("@{name}@"), &value);
    }
    assert!(!source.contains('@'));
    let source = source.replacen("#![no_std]", &format!("#![no_std]\n{prelude}"), 1);
    let input = root.join("fixture.rs");
    std::fs::write(&input, source).unwrap();
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
    Fixture { root, path, digest }
}

#[test]
fn h1_real_make_submission_commits_then_dispatches_event_and_draw() {
    let fixture = fixture(&[], false);
    let (plugin, evidence) = fixture.open();
    let transaction = plugin.begin::<Registration>().unwrap();
    assert!(matches!(
        plugin.owner.as_ref().lease(),
        Err(native_host::Error::State)
    ));
    let pending = plugin.make_registered(transaction).unwrap();
    assert!(!plugin.owner.as_ref().context().registrations.is_active());
    assert!(matches!(
        plugin.owner.as_ref().lease(),
        Err(native_host::Error::State)
    ));
    let batch = pending.commit().unwrap();
    assert_eq!(batch.len(), 2);
    assert_eq!(
        evidence.callback_markers.get(),
        0,
        "nothing dispatches during make"
    );
    let depth = DispatchDepth::new();
    assert_eq!(
        dispatch_registration(&batch, 0, &depth, Invocation::Tick(&tick_bytes())).unwrap(),
        host_glue::OK
    );
    assert_eq!(
        dispatch_registration(&batch, 1, &depth, Invocation::Draw(&screen_frame())).unwrap(),
        host_glue::OK
    );
    assert_eq!(evidence.callback_markers.get(), 2);
    assert_eq!(evidence.callback_frees.get(), 2);
    let mut invalid_tick = tick_bytes();
    invalid_tick.pop();
    assert!(matches!(
        dispatch_registration(&batch, 0, &depth, Invocation::Tick(&invalid_tick)),
        Err(Error::Status(host_glue::ERR_INVALID))
    ));
    assert_eq!(
        evidence.callback_markers.get(),
        2,
        "invalid event refused before foreign call"
    );
    drop(batch);
    plugin.owner.as_ref().revoke();
    plugin.owner.as_ref().finish().unwrap();
    assert_eq!(evidence.clocks.get(), 2);
    assert_eq!(evidence.frees.get(), 1);
    drop(plugin);
    assert!(!mapped(&fixture.path));
}

#[test]
fn h2_ignored_decode_unsupported_and_capacity_refusals_never_publish() {
    let mut malformed = records_bytes(&records());
    malformed.pop(); // Truncate a generated valid batch, not hand-coded framing.
    let unsupported = records_bytes(&[Registration::Event {
        event_tag: EVENT_TICK_TAG + 1,
        token: TOKEN,
    }]);
    let capacity = records_bytes(&[Registration::Draw { token: TOKEN }; 7]);
    for (invalid, expected) in [
        (malformed, native_host::Error::Invalid),
        (unsupported, native_host::Error::Invalid),
        (capacity, native_host::Error::Capacity),
    ] {
        let fixture = fixture(&invalid, true);
        let (plugin, evidence) = fixture.open();
        let transaction = plugin.begin::<Registration>().unwrap();
        let pending = plugin.make_registered(transaction).unwrap();
        assert!(!plugin.owner.as_ref().context().registrations.is_active());
        assert!(matches!(pending.commit(), Err(Error::Lifecycle(actual)) if actual == expected));
        assert_eq!(evidence.callback_markers.get(), 0);
        assert_eq!(evidence.allocations.get(), 1);
        assert_eq!(evidence.frees.get(), 1);
        assert_eq!(evidence.clocks.get(), 2);
        assert!(plugin.owner.as_ref().context().instance.get().is_none());
        assert!(matches!(
            plugin.owner.as_ref().lease(),
            Err(native_host::Error::State)
        ));
        // A fresh construction borrow proves the failed transaction was drained.
        drop(plugin.begin::<Registration>().unwrap());
        assert!(evidence.live.get());
        assert!(mapped(&fixture.path));
        drop(plugin);
        assert!(!mapped(&fixture.path));
        assert_eq!(evidence.clocks.get(), 2);
    }
}

#[test]
fn h4_real_callback_and_queued_lease_revoke_before_final_destruction() {
    let fixture = fixture(&[], false);
    let (plugin, evidence) = fixture.open();
    let transaction = plugin.begin::<Registration>().unwrap();
    let batch = plugin
        .make_registered(transaction)
        .unwrap()
        .commit()
        .unwrap();
    let queued = plugin.owner.as_ref().lease().unwrap();
    let event = records()[0];
    let bytes = tick_bytes();
    let depth = DispatchDepth::new();
    assert_eq!(
        queued
            .dispatch(&depth, |context| invoke_registration(
                context,
                event,
                Invocation::Tick(&bytes)
            ))
            .unwrap()
            .unwrap(),
        host_glue::OK
    );
    assert_eq!(
        dispatch_registration(&batch, 1, &depth, Invocation::Draw(&screen_frame())).unwrap(),
        host_glue::OK
    );
    assert_eq!(evidence.callback_markers.get(), 2);
    plugin.owner.as_ref().revoke();
    assert!(matches!(
        dispatch_registration(&batch, 0, &depth, Invocation::Tick(&bytes)),
        Err(Error::Lifecycle(native_host::Error::State))
    ));
    assert!(matches!(
        queued.dispatch(&depth, |context| invoke_registration(
            context,
            event,
            Invocation::Tick(&bytes)
        )),
        Err(native_host::Error::State)
    ));
    assert_eq!(evidence.callback_markers.get(), 2);
    assert_eq!(
        plugin.owner.as_ref().finish(),
        Err(native_host::Error::Busy)
    );
    drop(batch);
    assert_eq!(
        plugin.owner.as_ref().finish(),
        Err(native_host::Error::Busy)
    );
    assert_eq!(evidence.frees.get(), 0);
    drop(queued);
    plugin.owner.as_ref().finish().unwrap();
    plugin.owner.as_ref().finish().unwrap();
    assert_eq!(evidence.clocks.get(), 2);
    assert_eq!(evidence.frees.get(), 1);
    assert!(mapped(&fixture.path));
    drop(plugin);
    assert!(!mapped(&fixture.path));
    assert_eq!(evidence.callback_frees.get(), 2);
}

#[test]
fn registration_bridge_outside_make_refuses_without_poisoning_later_scope() {
    let fixture = fixture(&[], false);
    let (plugin, evidence) = fixture.open();
    let context = plugin.owner.as_ref().context();
    let bytes = records_bytes(&[]);
    let ctx = ptr::from_ref(context.get_ref()).cast_mut().cast();
    // SAFETY: exact pinned typed context; generated bytes live for the call.
    assert_eq!(
        unsafe {
            (context.table.registrations_submit)(
                ctx,
                PpBytes {
                    ptr: bytes.as_ptr(),
                    len: bytes.len(),
                },
            )
        },
        host_glue::ERR_STATE
    );
    let transaction = plugin.begin::<Registration>().unwrap();
    let batch = plugin
        .make_registered(transaction)
        .unwrap()
        .commit()
        .unwrap();
    assert!(!context.registrations.is_active());
    assert_eq!(batch.len(), 2);
    // SAFETY: same owned context and stable bytes, now after successful make.
    assert_eq!(
        unsafe {
            (context.table.registrations_submit)(
                ctx,
                PpBytes {
                    ptr: bytes.as_ptr(),
                    len: bytes.len(),
                },
            )
        },
        host_glue::ERR_STATE
    );
    assert_eq!(evidence.callback_markers.get(), 0);
    drop(batch);
    drop(plugin);
}
