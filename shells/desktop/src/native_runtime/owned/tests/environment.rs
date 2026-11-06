//! Actual foreign callbacks read the current Env through the pinned owner.
use super::super::environment as bridge;
use super::*;
use paperpaw_core::native_host::environment::{SNAPSHOT_BYTES, SettingValueRef, settings_decode};
use paperpaw_core::sdk::{Env, Manifest, Plugin, RegisterError, Registrar, SettingSpec, builtin};
use paperpaw_native::host_glue::PpBuffer;
use paperpaw_platform::{
    Battery, DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, PixelFormat, Rotation,
    SystemStatus, WallClock,
};
use std::mem::MaybeUninit;

#[allow(
    dead_code,
    clippy::all,
    clippy::pedantic,
    reason = "schema-generated test codec"
)]
mod wire {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../abi/generated/snapshot_wire.rs"
    ));
}

const KEY: &str = "abi7-tick-draw.enabled";
const DEVICE: DeviceInfo = DeviceInfo::new(
    "scoped-probe",
    "Scoped probe",
    DisplayInfo::new(
        480,
        800,
        220,
        PixelFormat::Other { code: 91, bits: 12 },
        Rotation::Deg0,
        None,
    ),
    InputCaps::new(true, false, false, &[]),
    MemoryClass::Small,
);
static BOOL: [SettingSpec; 1] = [SettingSpec::switch(
    KEY,
    "enabled",
    false,
    builtin::groups::OTHER,
)];
static INT: [SettingSpec; 1] = [SettingSpec::number(
    KEY,
    "enabled",
    -1,
    1,
    0,
    builtin::groups::OTHER,
)];
static TEXT: [SettingSpec; 1] = [SettingSpec::text(
    KEY,
    "enabled",
    "",
    builtin::groups::OTHER,
)];

type Run = Rc<dyn Fn(&Env<'_>)>;
struct Probe {
    run: Run,
    specs: &'static [SettingSpec],
}
impl Plugin for Probe {
    fn manifest(&self) -> Manifest {
        Manifest::new(
            paperpaw_core::sdk::Id::new_static("abi7-tick-draw"),
            "Scope probe",
            paperpaw_core::sdk::Version::new(1, 0, 0),
        )
    }
    fn register(&self, registrar: &mut Registrar) -> Result<(), RegisterError> {
        registrar.add_settings(self.specs)?;
        let run = self.run.clone();
        registrar.subscribe(
            builtin::events::STARTED,
            Box::new(move |env, _, _| run(env)),
        )
    }
}

fn run(
    specs: &'static [SettingSpec],
    value: &str,
    status: SystemStatus,
    callback: impl Fn(&Env<'_>) + 'static,
) {
    let calls = Rc::new(Cell::new(0));
    let observed = calls.clone();
    let run = Rc::new(move |env: &Env<'_>| {
        callback(env);
        observed.set(observed.get() + 1);
    });
    let mut kernel = paperpaw_core::Kernel::builder(DEVICE)
        .plugin(Probe { run, specs })
        .setting_default_text(KEY, value)
        .build()
        .unwrap();
    kernel.set_status(Instant(17), status);
    kernel.start(Instant(17));
    assert_eq!(calls.get(), 1);
}

fn expected_snapshot(battery: Option<Battery>, clock: bool) -> Vec<u8> {
    let mut value = wire::Snapshot::new(
        480,
        800,
        220,
        wire::PixelFormat::Other { code: 91, bits: 12 },
    );
    value.monotonic_ms = 17;
    value.touch = true;
    value.unix_seconds = clock.then_some(0);
    value.battery = battery.map(|input| {
        let mut result = wire::Battery::new(input.percent);
        result.charging = input.charging;
        result.saver = input.saver;
        result.millivolts = input.millivolts;
        result
    });
    let mut output = [MaybeUninit::uninit(); SNAPSHOT_BYTES];
    let count = wire::snapshot_encode(&value, &mut output).unwrap();
    output[..count]
        .iter()
        .map(|b| {
            // SAFETY: the encoder initialized exactly count bytes on success.
            unsafe { b.assume_init() }
        })
        .collect()
}

fn query_fixture(snapshot: &[u8], expected: SettingValueRef<'_>) -> Fixture {
    // Generated encoders define both layouts. No slot offsets or bytes are guessed.
    let query = format!(
        r#"
    let mut snapshot = [0xa5; 64];
    let result = unsafe {{ (h.env_snapshot)(s.ctx, PpBuffer {{ ptr: snapshot.as_mut_ptr(), cap: snapshot.len() }}) }};
    if result != OK {{ return result; }}
    if snapshot[..{snapshot_len}] != {snapshot:?} {{ return ERR_INVALID; }}
    if snapshot[{snapshot_len}..].iter().any(|b| *b != 0xa5) {{ return ERR_INVALID; }}
    let key = b"abi7-tick-draw.enabled";
    let mut value = [0xa5; 64];
    let count = unsafe {{ (h.settings_read)(s.ctx, PpBytes {{ ptr: key.as_ptr(), len: key.len() }}, PpBuffer {{ ptr: value.as_mut_ptr(), cap: value.len() }}) }};
    if count < 0 {{ return count; }}
    if count as usize > value.len() {{ return ERR_NO_SPACE; }}
    if settings_decode(&value[..count as usize]) != Ok(SettingValueRef::{expected:?}) {{ return ERR_INVALID; }}
    "#,
        snapshot_len = snapshot.len()
    );
    foreign::fixture_query(&[], false, &query)
}

fn read<S>(context: Pin<&Context<S>>, key: &[u8], out: &mut [u8]) -> i32 {
    // SAFETY: exact pinned context, initialized immutable key, exclusive output;
    // all regions are live and disjoint throughout the synchronous call.
    unsafe {
        (context.table.settings_read)(
            ptr::from_ref(context.get_ref()).cast_mut().cast(),
            PpBytes {
                ptr: key.as_ptr(),
                len: key.len(),
            },
            PpBuffer {
                ptr: out.as_mut_ptr(),
                cap: out.len(),
            },
        )
    }
}

fn snapshot<S>(context: Pin<&Context<S>>, out: &mut [u8]) -> i32 {
    // SAFETY: exact pinned context and live exclusive disjoint output.
    unsafe {
        (context.table.env_snapshot)(
            ptr::from_ref(context.get_ref()).cast_mut().cast(),
            PpBuffer {
                ptr: out.as_mut_ptr(),
                cap: out.len(),
            },
        )
    }
}

fn scope<S>(context: Pin<&Context<S>>) -> Pin<&EnvironmentScope> {
    // SAFETY: the owner structurally pins this field and never moves it.
    unsafe { context.map_unchecked(|c| &c.environment) }
}

#[test]
fn foreign_callbacks_copy_snapshot_and_typed_settings_only_during_admission() {
    for (battery, clock, specs, value, expected) in [
        (None, false, &BOOL[..], "true", SettingValueRef::Bool(true)),
        (
            Some(Battery::new(0, false, None)),
            true,
            &BOOL[..],
            "false",
            SettingValueRef::Bool(false),
        ),
        (
            Some(Battery::new(0, true, Some(0)).with_saver(true)),
            true,
            &[][..],
            "",
            SettingValueRef::Absent,
        ),
        (None, false, &INT[..], "0", SettingValueRef::Int(0)),
        (None, false, &TEXT[..], "", SettingValueRef::Text("")),
    ] {
        let fixture = query_fixture(&expected_snapshot(battery, clock), expected);
        let status =
            SystemStatus::new(clock.then(|| WallClock::new(Instant(17), 0, None)), battery);
        run(specs, value, status, move |env| {
            check_foreign(&fixture, env)
        });
    }
}

fn check_foreign(fixture: &Fixture, env: &Env<'_>) {
    let (plugin, evidence) = fixture.open();
    let batch = plugin
        .make_registered(plugin.begin::<Registration>().unwrap())
        .unwrap()
        .commit()
        .unwrap();
    let context = plugin.owner.as_ref().context();
    let depth = DispatchDepth::new();
    let mut bytes = [0xa5; 64];
    assert_eq!(snapshot(context, &mut bytes), host_glue::ERR_STATE);
    assert_eq!(
        read(context, KEY.as_bytes(), &mut bytes),
        host_glue::ERR_STATE
    );
    // Direct unscoped foreign dispatch fails without a callback marker.
    assert_eq!(
        dispatch_registration(&batch, 1, &depth, Invocation::Draw).unwrap(),
        host_glue::ERR_STATE
    );
    assert_eq!(evidence.callback_markers.get(), 0);
    assert_eq!(
        bridge::dispatch(&batch, 1, &depth, Invocation::Draw, env).unwrap(),
        host_glue::OK
    );
    let mut tick = [0; 64];
    let count = registrations::tick_encode(41, &mut tick).unwrap();
    let queued = plugin.owner.as_ref().lease().unwrap();
    let event = Registration::Event {
        event_tag: EVENT_TICK_TAG,
        token: 17,
    };
    assert_eq!(
        queued
            .dispatch(&depth, |ctx| bridge::invoke(
                ctx,
                event,
                Invocation::Tick(&tick[..count]),
                env
            ))
            .unwrap()
            .unwrap(),
        host_glue::OK
    );
    assert_eq!(evidence.callback_markers.get(), 2);
    assert_eq!(snapshot(context, &mut bytes), host_glue::ERR_STATE);
    assert_eq!(
        read(context, KEY.as_bytes(), &mut bytes),
        host_glue::ERR_STATE
    );
    plugin.owner.as_ref().revoke();
    assert!(matches!(
        bridge::dispatch(&batch, 1, &depth, Invocation::Draw, env),
        Err(Error::Lifecycle(native_host::Error::State))
    ));
    assert!(matches!(
        queued.dispatch(&depth, |ctx| bridge::invoke(
            ctx,
            event,
            Invocation::Tick(&tick[..count]),
            env
        )),
        Err(native_host::Error::State)
    ));
    assert_eq!(evidence.callback_markers.get(), 2);
    assert_eq!(
        plugin.owner.as_ref().finish(),
        Err(native_host::Error::Busy)
    );
    drop(batch);
    drop(queued);
    plugin.owner.as_ref().finish().unwrap();
    plugin.owner.as_ref().finish().unwrap();
    assert_eq!(evidence.frees.get(), 1);
    assert_eq!(evidence.callback_frees.get(), 2);
    assert_eq!(snapshot(context, &mut bytes), host_glue::ERR_STATE);
    drop(plugin);
    assert!(!mapped(&fixture.path));
}

#[test]
fn scoped_queries_preserve_errors_required_lengths_and_clear_on_all_host_exits() {
    run(
        &TEXT,
        "a text longer than the caller's initial sixty four byte buffer................................................................",
        SystemStatus::default(),
        |env| {
            let fixture = foreign::fixture_query(&[], false, "");
            let (plugin, evidence) = fixture.open();
            let context = plugin.owner.as_ref().context();
            let mut bytes = [0xa5; 64];
            scope(context)
                .scope(env, || {
                    let required = read(context, KEY.as_bytes(), &mut bytes);
                    assert!(required > 64);
                    assert!(
                        settings_decode(&bytes).is_err(),
                        "prefix is not a complete setting"
                    );
                    assert_eq!(read(context, KEY.as_bytes(), &mut []), required);
                    let mut full = vec![0; usize::try_from(required).unwrap()];
                    assert_eq!(read(context, KEY.as_bytes(), &mut full), required);
                    assert!(matches!(
                        settings_decode(&full),
                        Ok(SettingValueRef::Text(_))
                    ));
                    bytes.fill(0xa5);
                    assert_eq!(read(context, &[0xff], &mut bytes), host_glue::ERR_UTF8);
                    assert_eq!(bytes, [0xa5; 64]);
                    assert_eq!(snapshot(context, &mut bytes[..1]), host_glue::ERR_NO_SPACE);
                    assert_eq!(bytes, [0xa5; 64]);
                    assert!(matches!(
                        scope(context).scope(env, || ()),
                        Err(native_host::Error::Busy)
                    ));
                })
                .unwrap();
            assert_eq!(snapshot(context, &mut bytes), host_glue::ERR_STATE);
            let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                scope(context)
                    .scope(env, || panic!("host dispatch unwind"))
                    .unwrap();
            }));
            assert!(unwind.is_err());
            assert_eq!(snapshot(context, &mut bytes), host_glue::ERR_STATE);
            assert_eq!(
                read(context, KEY.as_bytes(), &mut bytes),
                host_glue::ERR_STATE
            );
            scope(context)
                .scope(env, || {
                    assert_eq!(snapshot(context, &mut bytes), host_glue::OK)
                })
                .unwrap();
            assert_eq!(evidence.allocations.get(), 0);
            drop(plugin);
        },
    );
}
