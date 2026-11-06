//! Flat events delivered by the production kernel subscription pipeline.
#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]
use paperpaw_core::{
    Kernel,
    native_host::{
        DispatchDepth, Error,
        environment::{EnvironmentScope, snapshot_decode},
        events,
        flat::{DropReason, FlatError, FlatInstance, FlatPlugin, FlatRegistration},
        registrations::*,
    },
};
use paperpaw_native::host_glue::{ERR_INVALID, ERR_STATE, ERR_UNSUPPORTED, OK};
use paperpaw_platform::{
    Battery, DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, PixelFormat, Rotation,
    SystemStatus,
};
use paperpaw_sdk::{Id, Manifest, Params, Version, builtin::events as builtin, event::EventData};
use std::{
    cell::{Cell, RefCell},
    pin::Pin,
    rc::Rc,
};

const DEVICE: DeviceInfo = DeviceInfo::new(
    "flat-test",
    "Flat fixture",
    DisplayInfo::new(80, 100, 220, PixelFormat::Gray4, Rotation::Deg0, None),
    InputCaps::new(true, false, false, &[]),
    MemoryClass::Small,
);
const TAGS: [u32; 6] = [32, 33, 34, 35, 36, 37];

struct Observed {
    calls: RefCell<Vec<(usize, Vec<u8>)>>,
    dropped: RefCell<Vec<(u32, DropReason)>>,
    response: Cell<Result<i32, Error>>,
    environment: Pin<Rc<EnvironmentScope>>,
    scopes: Cell<u32>,
}
impl Observed {
    fn new() -> Rc<Self> {
        Rc::new(Self {
            calls: RefCell::new(Vec::new()),
            dropped: RefCell::new(Vec::new()),
            response: Cell::new(Ok(OK)),
            environment: Rc::pin(EnvironmentScope::new()),
            scopes: Cell::new(0),
        })
    }
}
struct Instance {
    registrations: Vec<(u32, usize)>,
    seen: Rc<Observed>,
}
impl FlatInstance for Instance {
    fn registrations(&self, visit: &mut dyn FnMut(FlatRegistration)) {
        for &(tag, index) in &self.registrations {
            visit(if tag == 2 {
                FlatRegistration::Other { record_tag: 2 }
            } else {
                FlatRegistration::Event {
                    event_tag: tag,
                    index,
                }
            });
        }
    }
    fn environment(&self) -> Pin<&EnvironmentScope> {
        self.seen.environment.as_ref()
    }
    fn dispatch_event(
        &self,
        index: usize,
        payload: &[u8],
        _: &DispatchDepth,
    ) -> Result<i32, Error> {
        let mut snapshot = [0; 128];
        // SAFETY: initialized local output is exclusive and disjoint from host state.
        assert_eq!(
            unsafe {
                self.seen
                    .environment
                    .copy(snapshot.as_mut_ptr(), snapshot.len())
            },
            OK
        );
        let len = u32::from_le_bytes(snapshot[4..8].try_into().unwrap()) as usize;
        assert_eq!(
            snapshot_decode(&snapshot[..len]).unwrap().monotonic_ms,
            now(payload)
        );
        self.seen.scopes.set(self.seen.scopes.get() + 1);
        self.seen.calls.borrow_mut().push((index, payload.to_vec()));
        self.seen.response.get()
    }
    fn event_dropped(&self, tag: u32, why: DropReason) {
        self.seen.dropped.borrow_mut().push((tag, why));
    }
}
fn now(bytes: &[u8]) -> u64 {
    match events::decode(bytes).unwrap() {
        Record::EventStarted(r) => r.now_ms,
        Record::EventResume(r) => r.now_ms,
        Record::EventSuspend(r) => r.now_ms,
        Record::EventStopping(r) => r.now_ms,
        Record::EventBatteryMeasured(r) => r.now_ms,
        Record::EventPageTurned(r) => r.now_ms,
        _ => panic!("not an event"),
    }
}
fn tag(bytes: &[u8]) -> u32 {
    u32::from(bytes[1])
}
fn plugin(seen: &Rc<Observed>, tags: &[u32]) -> FlatPlugin {
    FlatPlugin::new(
        Manifest::new(Id::new_static("flat"), "Flat", Version::new(1, 0, 0)),
        Box::new(Instance {
            registrations: tags
                .iter()
                .enumerate()
                .map(|(i, &tag)| (tag, i + 10))
                .collect(),
            seen: Rc::clone(seen),
        }),
        Rc::new(DispatchDepth::new()),
    )
    .unwrap()
}
fn kernel(seen: &Rc<Observed>, tags: &[u32]) -> Kernel {
    Kernel::builder(DEVICE)
        .plugin(plugin(seen, tags))
        .build()
        .unwrap()
}
fn params() -> Params {
    builtin::Reading::new("books/a.epub".into(), "A".into(), 33.33, 2, 400, "C".into())
        .page(4)
        .pages(10)
        .to_params()
}
fn emit(kernel: &mut Kernel, tag: u32, at: u64, params: Params) {
    let id = match tag {
        32 => builtin::STARTED.id().clone(),
        33 => builtin::RESUME.id().clone(),
        34 => builtin::SUSPEND.id().clone(),
        35 => builtin::STOPPING.id().clone(),
        36 => builtin::BATTERY_MEASURED.id().clone(),
        37 => builtin::PAGE_TURNED.id().clone(),
        _ => panic!("bad tag"),
    };
    kernel.send_event(Instant(at), id.clone(), params);
}

#[test]
fn native_kernel_calls_deliver_all_six_with_real_environment_scopes() {
    let seen = Observed::new();
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(plugin(&seen, &TAGS))
        .after_sleep(true)
        .build()
        .unwrap();
    kernel.start(Instant(1));
    kernel.set_status(Instant(2), SystemStatus::default());
    let status = SystemStatus::default().with_battery(Some(Battery::new(50, false, Some(3700))));
    kernel.set_status(Instant(3), status);
    kernel.set_status(Instant(4), status);
    kernel.suspend(Instant(5));
    kernel.resume(Instant(6));
    kernel.stop(Instant(7));
    emit(&mut kernel, 37, 8, params());
    let calls = seen.calls.borrow();
    assert_eq!(
        calls
            .iter()
            .map(|(_, b)| (tag(b), now(b)))
            .collect::<Vec<_>>(),
        [
            (32, 1),
            (33, 1),
            (36, 3),
            (36, 4),
            (34, 5),
            (33, 6),
            (35, 7),
            (37, 8)
        ]
    );
    assert_eq!(
        calls.iter().map(|(i, _)| *i).collect::<Vec<_>>(),
        [10, 11, 14, 14, 12, 11, 13, 15]
    );
    assert_eq!(seen.scopes.get(), 8);
    let mut outside = [0; 128];
    // SAFETY: exclusive local output, no alias to context state.
    assert_eq!(
        unsafe { seen.environment.copy(outside.as_mut_ptr(), outside.len()) },
        ERR_STATE
    );
    assert!(seen.dropped.borrow().is_empty());
}

#[test]
fn only_registered_kinds_and_each_registration_index_receive_the_event() {
    let seen = Observed::new();
    let mut kernel = kernel(&seen, &[32, 32, 37]);
    for tag in TAGS {
        emit(&mut kernel, tag, u64::from(tag), params());
    }
    assert_eq!(
        seen.calls
            .borrow()
            .iter()
            .map(|(i, b)| (*i, tag(b)))
            .collect::<Vec<_>>(),
        [(10, 32), (11, 32), (12, 37)]
    );
}

#[test]
fn admission_and_plugin_refusals_are_reported_once_without_retry_or_blocking_lifecycle() {
    for (response, expected) in [
        (Err(Error::Busy), DropReason::Busy),
        (Err(Error::Depth), DropReason::Busy),
        (Err(Error::State), DropReason::State),
        (Ok(ERR_UNSUPPORTED), DropReason::Plugin(ERR_UNSUPPORTED)),
        (Ok(ERR_INVALID), DropReason::Plugin(ERR_INVALID)),
        (Ok(1), DropReason::Plugin(1)),
    ] {
        let seen = Observed::new();
        seen.response.set(response);
        let mut kernel = kernel(&seen, &TAGS);
        for tag in TAGS {
            emit(&mut kernel, tag, 10, params());
        }
        assert_eq!(seen.calls.borrow().len(), 6);
        assert_eq!(&*seen.dropped.borrow(), &TAGS.map(|tag| (tag, expected)));
        seen.response.set(Ok(OK));
        kernel.suspend(Instant(11));
        kernel.stop(Instant(12));
        assert_eq!(seen.calls.borrow().len(), 8);
        assert_eq!(seen.dropped.borrow().len(), 6);
        // Event failure is not a network refusal.
        assert_eq!(
            kernel
                .plugin_usage()
                .iter()
                .find(|p| p.plugin.as_str() == "flat")
                .unwrap()
                .refused,
            0
        );
    }
}

#[test]
fn malformed_page_producers_drop_without_invoking_the_plugin() {
    let seen = Observed::new();
    let mut kernel = kernel(&seen, &[37]);
    for p in [
        Params::new(),
        params().with("section", "bad"),
        params().with("page", "0"),
        params().with("pages", "0"),
        params().with("page", "11"),
        params().with("pages", "bad"),
        params().with("progress", "bad"),
    ] {
        emit(&mut kernel, 37, 1, p);
    }
    assert!(seen.calls.borrow().is_empty());
    assert_eq!(&*seen.dropped.borrow(), &[(37, DropReason::Producer); 7]);
    emit(&mut kernel, 37, 2, params());
    assert_eq!(seen.calls.borrow().len(), 1);
}

#[test]
fn page_fields_are_borrowed_clipped_and_counters_remain_optional() {
    let seen = Observed::new();
    let mut kernel = kernel(&seen, &[37]);
    let reading = builtin::Reading::new(
        "é".repeat(300),
        "é".repeat(200),
        33.33,
        2,
        400,
        "é".repeat(200),
    );
    // Exercise both Params and an already-carried Reading without cloning it.
    for p in [reading.to_params(), Params::new().carrying(reading)] {
        emit(&mut kernel, 37, 42, p);
    }
    let calls = seen.calls.borrow();
    assert_eq!(calls[0].1, calls[1].1);
    let Record::EventPageTurned(r) = events::decode(&calls[0].1).unwrap() else {
        panic!()
    };
    assert_eq!((r.section, r.offset, r.progress_ppm), (2, 400, 333_300));
    assert_eq!(
        (r.path.len(), r.title.len(), r.chapter.len()),
        (512, 256, 256)
    );
    assert_eq!((r.page, r.pages), (None, None));
    assert!(calls[0].1.len() < 1100);
}

#[test]
fn progress_conversion_clamps_rounds_and_handles_nonfinite_values() {
    let seen = Observed::new();
    let mut kernel = kernel(&seen, &[37]);
    for (percent, expected) in [
        (0.0, 0),
        (100.0, 1_000_000),
        (33.33, 333_300),
        (f32::NAN, 0),
        (f32::NEG_INFINITY, 0),
        (f32::INFINITY, 1_000_000),
        (-1.0, 0),
        (101.0, 1_000_000),
        (0.00006, 1),
    ] {
        emit(
            &mut kernel,
            37,
            1,
            params().with("progress", percent.to_string()),
        );
        let calls = seen.calls.borrow();
        let Record::EventPageTurned(r) = events::decode(&calls.last().unwrap().1).unwrap() else {
            panic!()
        };
        assert_eq!(r.progress_ppm, expected);
    }
    assert!(seen.dropped.borrow().is_empty());
}

#[test]
fn later_listeners_run_after_an_earlier_plugin_refuses() {
    let failed = Observed::new();
    failed.response.set(Ok(ERR_STATE));
    let good = Observed::new();
    let mut later = plugin(&good, &[32]);
    // The fixture shares a type, but the kernel must see different plugin ids.
    let mut manifest = paperpaw_sdk::Plugin::manifest(&later);
    manifest.id = Id::new_static("later");
    later = FlatPlugin::new(
        manifest,
        Box::new(Instance {
            registrations: vec![(32, 99)],
            seen: Rc::clone(&good),
        }),
        Rc::new(DispatchDepth::new()),
    )
    .unwrap();
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(plugin(&failed, &[32]))
        .plugin(later)
        .build()
        .unwrap();
    kernel.start(Instant(1));
    assert_eq!(failed.calls.borrow().len(), 1);
    assert_eq!(good.calls.borrow().len(), 1);
    assert_eq!(
        &*failed.dropped.borrow(),
        &[(32, DropReason::Plugin(ERR_STATE))]
    );
}

#[test]
fn unsupported_callbacks_refuse_the_entire_instance_at_construction() {
    for tags in [&[3][..], &[2][..], &[32, 3][..], &[32, 2][..], &[38][..]] {
        let seen = Observed::new();
        let error = FlatPlugin::new(
            Manifest::new(Id::new_static("flat"), "Flat", Version::new(1, 0, 0)),
            Box::new(Instance {
                registrations: tags.iter().enumerate().map(|(i, &tag)| (tag, i)).collect(),
                seen: Rc::clone(&seen),
            }),
            Rc::new(DispatchDepth::new()),
        )
        .unwrap_err();
        assert_eq!(
            error,
            FlatError::Unrouted {
                tag: *tags.last().unwrap()
            }
        );
        assert!(seen.calls.borrow().is_empty());
    }
    let seen = Observed::new();
    let p = plugin(&seen, &TAGS);
    let mut registrar = paperpaw_sdk::Registrar::new(Id::new_static("flat"));
    assert!(paperpaw_sdk::Plugin::register(&p, &mut registrar).is_ok());
}

struct ScopeConflict {
    seen: Rc<Observed>,
    handler: Rc<paperpaw_sdk::EventHandler>,
}
impl paperpaw_sdk::Plugin for ScopeConflict {
    fn manifest(&self) -> Manifest {
        Manifest::new(
            Id::new_static("conflict"),
            "Conflict",
            Version::new(1, 0, 0),
        )
    }
    fn register(&self, r: &mut paperpaw_sdk::Registrar) -> Result<(), paperpaw_sdk::RegisterError> {
        let seen = Rc::clone(&self.seen);
        let handler = Rc::clone(&self.handler);
        r.subscribe(
            builtin::STARTED,
            Box::new(move |env, params, out| {
                seen.environment
                    .as_ref()
                    .scope(env, || handler(env, params, out))
                    .unwrap();
            }),
        )
    }
}

#[test]
fn occupied_environment_scope_refuses_before_foreign_dispatch_and_resets() {
    let seen = Observed::new();
    let flat = plugin(&seen, &[32]);
    let mut registrar = paperpaw_sdk::Registrar::new(Id::new_static("flat"));
    paperpaw_sdk::Plugin::register(&flat, &mut registrar).unwrap();
    let handler = registrar
        .into_contributions()
        .into_iter()
        .find_map(|contribution| {
            if let paperpaw_sdk::Contribution::Subscription { handler, .. } = contribution {
                Some(Rc::new(handler))
            } else {
                None
            }
        })
        .unwrap();
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(ScopeConflict {
            seen: Rc::clone(&seen),
            handler,
        })
        .build()
        .unwrap();
    kernel.start(Instant(1));
    assert!(seen.calls.borrow().is_empty());
    assert_eq!(&*seen.dropped.borrow(), &[(32, DropReason::Busy)]);
    let mut output = [0; 128];
    // SAFETY: exclusive local output disjoint from the environment scope.
    assert_eq!(
        unsafe { seen.environment.copy(output.as_mut_ptr(), output.len()) },
        ERR_STATE
    );
}

struct Loader(Rc<Observed>);
impl paperpaw_core::CodeLoader for Loader {
    fn load(
        &mut self,
        _: &dyn paperpaw_platform::Storage,
        code: &paperpaw_core::CodeFile<'_>,
    ) -> Result<Box<dyn paperpaw_sdk::Plugin>, paperpaw_sdk::PluginProblem> {
        assert_eq!(code.id.as_str(), "flat");
        assert_eq!(code.sha256, [7; 32]);
        Ok(Box::new(plugin(&self.0, &TAGS)))
    }
}

#[test]
fn code_loader_returns_flat_adapter_to_the_production_kernel_route() {
    let seen = Observed::new();
    let storage = paperpaw_sdk::MemoryStorage::new()
        .with("plugins/flat/plugin.ini", "[plugin]\nversion = 1.0.0\n")
        .with("plugins/flat/code.ppn", "PPNP");
    assert!(paperpaw_sdk::native::remember_code(
        &storage, "flat", &[7; 32]
    ));
    let mut kernel = Kernel::builder(DEVICE)
        .storage(storage)
        .code_plugins(
            paperpaw_sdk::native::Native::new("code.ppn"),
            Box::new(Loader(Rc::clone(&seen))),
        )
        .build()
        .unwrap();
    kernel.start(Instant(1));
    kernel.resume(Instant(2));
    kernel.set_status(
        Instant(3),
        SystemStatus::default().with_battery(Some(Battery::new(50, false, None))),
    );
    kernel.suspend(Instant(4));
    kernel.stop(Instant(5));
    emit(&mut kernel, 37, 6, params());
    assert_eq!(
        seen.calls
            .borrow()
            .iter()
            .map(|(_, b)| tag(b))
            .collect::<Vec<_>>(),
        [32, 33, 36, 34, 35, 37]
    );
}
