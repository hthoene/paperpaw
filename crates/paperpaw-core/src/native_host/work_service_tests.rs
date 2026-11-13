//! `core.work`: the ops over the scheduler, their statuses and the replies the
//! dispatcher checks.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
extern crate std;
use super::*;
use crate::abi7::services_wire;
use crate::native_host::admitted_features::AdmittedFeatures;
use crate::native_host::call_services::validate_reply;
use crate::native_host::work_sched::{Limits, Power};
use alloc::rc::Rc;
use alloc::vec::Vec;
use core::cell::Cell;
use paperpaw_i18n::Translator;
use paperpaw_native::host_glue::{ERR_DENIED, ERR_FULL, ERR_STATE};
use paperpaw_native::metadata::Feature;
use paperpaw_platform::Instant;
use paperpaw_sdk::{Env, Settings};

static WIRED: &[Feature<'static>] = &[Feature {
    key: "uses.network",
    revision: 1,
}];

struct Clock {
    now: Cell<u64>,
    allowed: Cell<bool>,
    /// Owners that subscribed to `work_due`.
    subscribed: Vec<u64>,
}
impl WorkEnv for Clock {
    fn now_ms(&self) -> u64 {
        self.now.get()
    }
    fn power(&self) -> Power {
        Power {
            level: 0,
            stretch: 1,
            radio_on: true,
        }
    }
    fn screen_hold(&self) -> bool {
        false
    }
    fn network_up(&self) -> bool {
        true
    }
    fn network_allowed(&self, _: u64) -> bool {
        self.allowed.get()
    }
    fn saving_manual(&self, _: u64) -> bool {
        false
    }
    fn subscribed(&self, owner: u64, tag: u32) -> bool {
        tag == wire::EVENT_WORK_DUE_TAG && self.subscribed.contains(&owner)
    }
}

struct View<'a> {
    env: &'a Env<'a>,
    hosts: crate::native_host::host_services::HostServices,
    clock: Rc<Clock>,
    sched: crate::native_host::shared::Shared<WorkSched>,
}
impl Frame for View<'_> {
    fn settings(&self) -> &Settings {
        self.env.settings()
    }
    fn translator(&self) -> &Translator {
        self.env.i18n()
    }
}
impl View<'_> {
    fn call(
        &self,
        owner: u64,
        features: AdmittedFeatures,
        op: u32,
        record: Record<'_>,
    ) -> Result<Vec<u8>, i32> {
        let schema = services_wire::service_schema("core.work", 1).unwrap();
        let op = schema.op(op).unwrap();
        let mut input = std::vec![0; 4096];
        let used = wire::records_encode_counted(&[record], 1, op.max_input, &mut input)?;
        self.call_raw(owner, features, op, &input[..used])
    }
    fn call_raw(
        &self,
        owner: u64,
        features: AdmittedFeatures,
        op: &ServiceOp,
        input: &[u8],
    ) -> Result<Vec<u8>, i32> {
        let caller = Caller {
            owner,
            plugin: "p",
            features,
            hosts: &self.hosts,
        };
        let mut out = std::vec![0; op.max_reply];
        let length = backend(self, &caller, op, input, &mut out)?;
        out.truncate(length);
        validate_reply(&out, op)?;
        Ok(out)
    }
}

fn with_view<R>(wire_it: bool, f: impl FnOnce(&View<'_>) -> R) -> R {
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(Instant(0));
    let clock = Rc::new(Clock {
        now: Cell::new(1_000),
        allowed: Cell::new(true),
        // Owners 1 and 2 are plugins that subscribed to `work_due`; 3 did not.
        subscribed: Vec::from([1, 2]),
    });
    let sched = crate::native_host::shared::Shared::try_new(WorkSched::new(Limits::X4)).unwrap();
    let hosts = crate::native_host::host_services::HostServices::new();
    if wire_it {
        hosts
            .wire_work(WorkServices {
                sched: sched.try_clone().unwrap(),
                env: crate::native_host::shared::Shared::try_new(alloc::boxed::Box::new(
                    clock.clone(),
                )
                    as alloc::boxed::Box<dyn crate::native_host::work_sched::WorkEnv>)
                .unwrap(),
            })
            .unwrap();
    }
    f(&View {
        env: &env,
        hosts,
        clock,
        sched,
    })
}

fn spec(key: &str) -> wire::ServiceWorkSpec<'_> {
    wire::ServiceWorkSpec {
        token: 5,
        needs: 0,
        flags: 0,
        moments: 0,
        after_ms: 0,
        within_ms: 0,
        every_ms: 0,
        at_ms: 0,
        key,
    }
}
const NONE: AdmittedFeatures = AdmittedFeatures::NONE;

#[test]
fn an_unwired_family_is_unsupported() {
    with_view(false, |v| {
        assert_eq!(
            v.call(1, NONE, 1, Record::ServiceWorkSpec(spec("a"))).err(),
            Some(ERR_UNSUPPORTED)
        );
    });
}

#[test]
fn declare_needs_a_subscription_to_work_due() {
    with_view(true, |v| {
        // Owner 3 never subscribed: its work is refused, nothing is kept.
        assert_eq!(
            v.call(3, NONE, 1, Record::ServiceWorkSpec(spec("a"))).err(),
            Some(ERR_STATE)
        );
        assert_eq!(v.sched.counts(), (0, 0));
        // A form that is wrong is still the form's status first.
        let mut bad = spec("a");
        bad.needs = 8;
        assert_eq!(
            v.call(3, NONE, 1, Record::ServiceWorkSpec(bad)).err(),
            Some(ERR_UNSUPPORTED)
        );
        // Subscribed: declared.
        v.call(1, NONE, 1, Record::ServiceWorkSpec(spec("a")))
            .unwrap();
        assert_eq!(v.sched.counts(), (1, 0));
        // A plugin the host does not know is not subscribed either.
        assert_eq!(
            v.call(9, NONE, 1, Record::ServiceWorkSpec(spec("a"))).err(),
            Some(ERR_STATE)
        );
    });
}

#[test]
fn declare_adds_replaces_and_state_lists_in_order_with_a_closing_end() {
    with_view(true, |v| {
        let mut exact = spec("timer");
        exact.at_ms = 9_000;
        v.call(1, NONE, 1, Record::ServiceWorkSpec(spec("a")))
            .unwrap();
        v.call(1, NONE, 1, Record::ServiceWorkSpec(exact)).unwrap();
        v.call(2, NONE, 1, Record::ServiceWorkSpec(spec("other")))
            .unwrap();
        // A replace keeps one row.
        let mut again = spec("a");
        again.token = 77;
        v.call(1, NONE, 1, Record::ServiceWorkSpec(again)).unwrap();
        let op = services_wire::service_schema("core.work", 1)
            .unwrap()
            .op(3)
            .unwrap();
        let reply = v.call_raw(1, NONE, op, &[]).unwrap();
        let mut rows = Vec::new();
        let mut end = None;
        crate::native_host::stage_wire::visit_stream(&reply, |_, framed| {
            match wire::record_decode_one(framed).unwrap() {
                Record::ServiceWorkRow(r) => rows.push((
                    alloc::string::String::from(r.key),
                    r.token,
                    r.state,
                    r.due_ms,
                )),
                Record::ServiceEnd(e) => end = Some((e.total, e.flags, e.next, e.generation)),
                other => panic!("{other:?}"),
            }
            Ok(())
        })
        .unwrap();
        assert_eq!(
            rows,
            [("a".into(), 77, 0, 1_000), ("timer".into(), 5, 0, 9_000)]
        );
        assert_eq!(end, Some((2, 0, 0, 0)));
        // Cancel one, then all; a missing key is fine.
        v.call(
            1,
            NONE,
            2,
            Record::ServiceWorkKey(wire::ServiceWorkKey { key: "a" }),
        )
        .unwrap();
        v.call(
            1,
            NONE,
            2,
            Record::ServiceWorkKey(wire::ServiceWorkKey { key: "nope" }),
        )
        .unwrap();
        assert_eq!(v.sched.counts().0, 2);
        v.call(
            1,
            NONE,
            2,
            Record::ServiceWorkKey(wire::ServiceWorkKey { key: "" }),
        )
        .unwrap();
        assert_eq!(v.sched.counts().0, 1, "the other plugin's stays");
    });
}

#[test]
fn the_statuses_of_declare() {
    with_view(true, |v| {
        let declare =
            |sp: wire::ServiceWorkSpec<'_>, f| v.call(1, f, 1, Record::ServiceWorkSpec(sp)).err();
        assert_eq!(declare(spec(""), NONE), Some(ERR_INVALID));
        let mut bits = spec("b");
        bits.moments = 8;
        assert_eq!(declare(bits, NONE), Some(ERR_UNSUPPORTED));
        let mut exact = spec("e");
        exact.at_ms = 5;
        exact.after_ms = 5;
        assert_eq!(declare(exact, NONE), Some(ERR_INVALID));
        // A network work needs uses.network (not asked: denied) ...
        let mut network = spec("n");
        network.needs = 1;
        assert_eq!(
            declare(network, AdmittedFeatures::test(0, WIRED)),
            Some(ERR_DENIED)
        );
        // ... a host that does not wire it says unsupported ...
        assert_eq!(declare(network, NONE), Some(ERR_UNSUPPORTED));
        // ... and the user's network switch counts.
        v.clock.allowed.set(false);
        assert_eq!(
            declare(network, AdmittedFeatures::test(1, WIRED)),
            Some(ERR_DENIED)
        );
        v.clock.allowed.set(true);
        assert_eq!(declare(network, AdmittedFeatures::test(1, WIRED)), None);
        // Eight per plugin on the X4, the ninth is full; replacing is not.
        for n in 0..7 {
            declare(spec(&std::format!("k{n}")), NONE);
        }
        assert_eq!(declare(spec("ninth"), NONE), Some(ERR_FULL));
        assert_eq!(declare(spec("k0"), NONE), None);
    });
}

#[test]
fn malformed_input_is_refused_and_leaves_the_table_alone() {
    with_view(true, |v| {
        let declare = services_wire::service_schema("core.work", 1)
            .unwrap()
            .op(1)
            .unwrap();
        let cancel = services_wire::service_schema("core.work", 1)
            .unwrap()
            .op(2)
            .unwrap();
        let state = services_wire::service_schema("core.work", 1)
            .unwrap()
            .op(3)
            .unwrap();
        // Empty, truncated, a record of another op, trailing bytes.
        for input in [&[][..], &[1][..], &[1, 0xff, 0xff][..]] {
            assert!(v.call_raw(1, NONE, declare, input).is_err());
            assert!(v.call_raw(1, NONE, cancel, input).is_err());
        }
        let mut good = std::vec![0; 4096];
        let used = wire::records_encode_counted(
            &[Record::ServiceWorkKey(wire::ServiceWorkKey { key: "a" })],
            1,
            256,
            &mut good,
        )
        .unwrap();
        assert!(
            v.call_raw(1, NONE, declare, &good[..used]).is_err(),
            "a key is no spec"
        );
        let used =
            wire::records_encode_counted(&[Record::ServiceWorkSpec(spec("a"))], 1, 256, &mut good)
                .unwrap();
        assert!(
            v.call_raw(1, NONE, cancel, &good[..used]).is_err(),
            "a spec is no key"
        );
        // `state` takes no input.
        assert_eq!(
            v.call_raw(1, NONE, state, &good[..used]).err(),
            Some(ERR_INVALID)
        );
        assert_eq!(v.sched.counts().0, 0);
    });
}

#[test]
fn a_table_cannot_overflow_the_state_reply() {
    // 32 rows of the longest key and a closing end fit the 4,096-byte reply.
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(Instant(0));
    let clock = Rc::new(Clock {
        now: Cell::new(1_000),
        allowed: Cell::new(true),
        subscribed: Vec::from([1]),
    });
    let sched = crate::native_host::shared::Shared::try_new(WorkSched::new(Limits::OTHER)).unwrap();
    let hosts = crate::native_host::host_services::HostServices::new();
    hosts
        .wire_work(WorkServices {
            sched: sched.try_clone().unwrap(),
            env: crate::native_host::shared::Shared::try_new(alloc::boxed::Box::new(clock.clone())
                as alloc::boxed::Box<dyn crate::native_host::work_sched::WorkEnv>)
            .unwrap(),
        })
        .unwrap();
    let v = View {
        env: &env,
        hosts,
        clock,
        sched,
    };
    for n in 0..32 {
        let key = std::format!("{n:0>64}");
        let mut sp = spec(&key);
        sp.at_ms = u64::MAX;
        v.call(1, NONE, 1, Record::ServiceWorkSpec(sp)).unwrap();
    }
    let op = services_wire::service_schema("core.work", 1)
        .unwrap()
        .op(3)
        .unwrap();
    let reply = v.call_raw(1, NONE, op, &[]).unwrap();
    assert!(reply.len() <= 4096);
    // Too small an output is refused, not cut.
    let caller = Caller {
        owner: 1,
        plugin: "p",
        features: NONE,
        hosts: &v.hosts,
    };
    let mut small = std::vec![0; 200];
    assert!(backend(&v, &caller, op, &[], &mut small).is_err());
}
