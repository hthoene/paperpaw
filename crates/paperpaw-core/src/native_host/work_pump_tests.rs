//! The work pump: what comes due reaches the plugin that declared it under
//! its run's guard; refusals, the moments, the hint 105, runs that reach
//! their longest time, the per-pass cap, and nothing inside a callback.
#![allow(clippy::unwrap_used, reason = "fixture errors fail tests")]
extern crate std;
use super::super::{
    native_instances::{NativeInstance, NativeLease},
    native_screen::Dispatch,
    registrations as wire,
    work_sched::{Limits, Power, RUN_MAX_MS, Spec, WorkEnv, WorkSched, why},
    work_service::WorkServices,
};
use alloc::{rc::Rc, string::String, vec::Vec};
use core::cell::{Cell, RefCell};
use paperpaw_native::host_glue::{ERR_INVALID, OK};
use paperpaw_platform::{Battery, Instant, SystemStatus};
use paperpaw_sdk::Id;

/// What a plugin was told: the event tag, the work's token, `why` (due) or
/// `reason` (refused), the key, and whether the callback was a run's.
#[derive(Debug, PartialEq, Eq)]
struct Told {
    tag: u32,
    token: u32,
    code: u32,
    key: String,
    quiet: bool,
}

struct Fake {
    now: Cell<u64>,
    up: Cell<bool>,
    level: Cell<u32>,
    denied: Cell<bool>,
}

impl WorkEnv for Fake {
    fn now_ms(&self) -> u64 {
        self.now.get()
    }
    fn power(&self) -> Power {
        Power {
            level: self.level.get(),
            stretch: 1,
            radio_on: true,
        }
    }
    fn screen_hold(&self) -> bool {
        false
    }
    fn network_up(&self) -> bool {
        self.up.get()
    }
    fn network_allowed(&self, _: u64) -> bool {
        !self.denied.get()
    }
    fn saving_manual(&self, _: u64) -> bool {
        false
    }
}

struct Probe {
    told: Rc<RefCell<Vec<Told>>>,
    sched: Rc<WorkSched>,
    owner: Rc<Cell<u64>>,
    tags: Vec<u32>,
    running: Cell<bool>,
    answer: Rc<Cell<Result<i32, i32>>>,
    /// Opens a resource of the run in the due callback (the run stays open).
    hold_run: Rc<Cell<bool>>,
    /// What the plugin does in its due callback (given its owner key).
    on_due: Rc<RefCell<Option<Rc<dyn Fn(u64)>>>>,
}

impl NativeInstance for Probe {
    fn running(&self) -> bool {
        self.running.get()
    }
    fn stop(&self) {
        self.running.set(false);
    }
    fn subscribed(&self, tag: u32) -> bool {
        self.tags.contains(&tag)
    }
    fn service_event(&self, bytes: &[u8], _: &paperpaw_sdk::Env<'_>) -> Result<i32, i32> {
        let quiet = self.sched.quiet(self.owner.get());
        let told = match wire::records_decode_single(bytes)? {
            wire::Record::EventWorkDue(e) => Told {
                tag: 103,
                token: e.token,
                code: e.why,
                key: e.key.into(),
                quiet,
            },
            wire::Record::EventWorkRefused(e) => Told {
                tag: 104,
                token: e.token,
                code: e.reason,
                key: e.key.into(),
                quiet,
            },
            wire::Record::EventWorkAllowed(_) => Told {
                tag: 105,
                token: 0,
                code: 0,
                key: String::new(),
                quiet,
            },
            _ => return Err(ERR_INVALID),
        };
        if self.hold_run.get()
            && told.tag == 103
            && let Some(run) = self.sched.current_run(self.owner.get())
        {
            self.sched.resource_opened(run as u32).unwrap();
        }
        let hook = self.on_due.borrow().clone();
        if let (Some(hook), 103) = (hook, told.tag) {
            hook(self.owner.get());
        }
        self.told.borrow_mut().push(told);
        self.answer.get()
    }
}

impl Dispatch for Probe {
    fn event(&self, _: u32, _: &[u8], _: &paperpaw_sdk::Env<'_>) -> Result<i32, i32> {
        Err(ERR_INVALID)
    }
    fn input(&self, _: u32, _: &[u8], _: &paperpaw_sdk::Env<'_>) -> Result<i32, i32> {
        Err(ERR_INVALID)
    }
    fn draw(
        &self,
        _: u32,
        _: &wire::DrawFrame<'_>,
        _: &paperpaw_sdk::Env<'_>,
        _: &mut paperpaw_graphics::Canvas<'_>,
    ) -> Result<i32, i32> {
        Err(ERR_INVALID)
    }
    fn dropped(&self, _: u32, _: i32) {}
}

struct World {
    kernel: crate::Kernel,
    sched: Rc<WorkSched>,
    env: Rc<Fake>,
    owners: Vec<u64>,
    told: Vec<Rc<RefCell<Vec<Told>>>>,
    answers: Vec<Rc<Cell<Result<i32, i32>>>>,
    holds: Vec<Rc<Cell<bool>>>,
    on_dues: Vec<Rc<RefCell<Option<Rc<dyn Fn(u64)>>>>>,
}

const NAMES: [&str; 3] = ["a", "b", "c"];
const ALL: [u32; 3] = [103, 104, 105];

fn world_with(tags: &[u32], plugins: usize) -> World {
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let sched = Rc::new(WorkSched::new(Limits::OTHER));
    let env = Rc::new(Fake {
        now: Cell::new(1_000),
        up: Cell::new(true),
        level: Cell::new(0),
        denied: Cell::new(false),
    });
    let (mut owners, mut told) = (Vec::new(), Vec::new());
    let (mut answers, mut holds) = (Vec::new(), Vec::new());
    let mut on_dues = Vec::new();
    for name in NAMES.iter().take(plugins) {
        let log = Rc::new(RefCell::new(Vec::new()));
        let answer = Rc::new(Cell::new(Ok(OK)));
        let hold = Rc::new(Cell::new(false));
        let me = Rc::new(Cell::new(0));
        let on_due: Rc<RefCell<Option<Rc<dyn Fn(u64)>>>> = Rc::new(RefCell::new(None));
        let probe = Probe {
            told: Rc::clone(&log),
            sched: Rc::clone(&sched),
            owner: Rc::clone(&me),
            tags: tags.to_vec(),
            running: Cell::new(true),
            answer: Rc::clone(&answer),
            hold_run: Rc::clone(&hold),
            on_due: Rc::clone(&on_due),
        };
        let lease = NativeLease::try_new(probe).unwrap();
        let owner = kernel
            .shared
            .native_instances
            .test_insert(Id::try_parse(name).unwrap(), Some(lease), true)
            .unwrap();
        me.set(owner);
        owners.push(owner);
        told.push(log);
        answers.push(answer);
        on_dues.push(on_due);
        holds.push(hold);
    }
    kernel
        .shared
        .native_instances
        .test_resources()
        .unwrap()
        .hosts()
        .wire_work(WorkServices {
            sched: Rc::clone(&sched),
            env: Rc::clone(&env) as Rc<dyn WorkEnv>,
        })
        .unwrap();
    World {
        kernel,
        sched,
        env,
        owners,
        told,
        answers,
        holds,
        on_dues,
    }
}

fn world() -> World {
    world_with(&ALL, 1)
}

fn spec(key: &str, token: u32) -> Spec<'_> {
    Spec {
        token,
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

impl World {
    fn declare(&self, plugin: usize, spec: &Spec<'_>) {
        self.sched
            .declare(self.owners[plugin], spec, self.env.now.get())
            .unwrap();
    }
    fn pump(&mut self) -> bool {
        self.kernel.native_work_pump()
    }
    fn told(&self, plugin: usize) -> Vec<Told> {
        self.told[plugin].borrow_mut().drain(..).collect()
    }
}

fn due(token: u32, code: u32, key: &str, quiet: bool) -> Told {
    Told {
        tag: 103,
        token,
        code,
        key: key.into(),
        quiet,
    }
}

#[test]
fn due_work_reaches_its_plugin_once_under_the_runs_guard() {
    let mut w = world();
    w.declare(0, &spec("job", 7));
    assert!(!w.pump());
    assert_eq!(w.told(0), [due(7, why::TIME, "job", true)]);
    // The callback returned: the run is over and the work is gone.
    assert_eq!(w.sched.counts(), (0, 0));
    assert!(!w.sched.quiet(w.owners[0]), "the guard ended with the call");
    w.env.now.set(5_000);
    w.pump();
    assert!(w.told(0).is_empty(), "told once");
}

#[test]
fn work_waits_for_its_time_and_the_pump_names_the_wake() {
    let mut w = world();
    w.declare(
        0,
        &Spec {
            after_ms: 4_000,
            ..spec("later", 3)
        },
    );
    w.pump();
    assert!(w.told(0).is_empty());
    assert_eq!(w.kernel.work_pump_due(), Some(Instant(5_000)));
    w.env.now.set(5_000);
    w.pump();
    assert_eq!(w.told(0), [due(3, why::TIME, "later", true)]);
    assert_eq!(w.kernel.work_pump_due(), None, "nothing waits on a clock");
}

#[test]
fn a_plugin_that_did_not_subscribe_or_is_stopped_is_not_called() {
    let mut w = world_with(&[104, 105], 1);
    w.declare(0, &spec("job", 1));
    w.pump();
    assert!(w.told(0).is_empty(), "no subscription to 103");
    let mut w = world();
    w.declare(0, &spec("job", 1));
    w.kernel.shared.native_instances.stop(w.owners[0]).unwrap();
    w.pump();
    assert!(w.told(0).is_empty(), "a stopped plugin is not called");
}

#[test]
fn a_failing_callback_is_not_told_again_and_does_not_stop_the_next_plugin() {
    let mut w = world_with(&ALL, 2);
    w.declare(0, &spec("one", 1));
    w.declare(1, &spec("two", 2));
    w.answers[0].set(Err(ERR_INVALID));
    w.env.now.set(2_000);
    w.pump();
    assert_eq!(w.told(0), [due(1, why::TIME, "one", true)]);
    assert_eq!(w.told(1), [due(2, why::TIME, "two", true)]);
    assert_eq!(w.sched.counts(), (0, 0), "both runs ended with their calls");
    w.pump();
    assert!(w.told(0).is_empty() && w.told(1).is_empty());
}

#[test]
fn a_network_turned_off_refuses_the_work_with_104() {
    let mut w = world();
    w.declare(
        0,
        &Spec {
            needs: 1,
            after_ms: 60_000,
            ..spec("sync", 9)
        },
    );
    w.env.denied.set(true);
    w.pump();
    assert_eq!(
        w.told(0),
        [Told {
            tag: 104,
            token: 9,
            code: 13,
            key: "sync".into(),
            quiet: false,
        }]
    );
    w.env.denied.set(false);
    w.env.now.set(100_000);
    w.pump();
    assert!(w.told(0).is_empty(), "a refused work is gone for good");
}

#[test]
fn moments_come_from_the_device_and_the_first_pass_is_no_moment() {
    // Online: the network came up between two passes.
    let mut w = world();
    w.env.up.set(false);
    w.pump();
    w.declare(
        0,
        &Spec {
            moments: 2,
            within_ms: 600_000,
            ..spec("net", 4)
        },
    );
    w.env.now.set(2_000);
    w.pump();
    assert!(w.told(0).is_empty(), "no moment yet");
    w.env.up.set(true);
    w.env.now.set(3_000);
    w.pump();
    assert_eq!(w.told(0), [due(4, why::ONLINE, "net", true)]);
    // Wake and charging.
    let mut w = world();
    w.pump();
    w.declare(
        0,
        &Spec {
            moments: 1 | 4,
            within_ms: 600_000,
            ..spec("wake", 5)
        },
    );
    w.kernel.native_work_woke();
    w.env.now.set(2_000);
    w.pump();
    assert_eq!(w.told(0), [due(5, why::WAKE, "wake", true)]);
    w.declare(
        0,
        &Spec {
            moments: 4,
            within_ms: 600_000,
            ..spec("charge", 6)
        },
    );
    w.env.now.set(3_000);
    w.pump();
    assert!(w.told(0).is_empty());
    w.kernel.shared.status =
        SystemStatus::default().with_battery(Some(Battery::new(50, true, None)));
    w.env.now.set(4_000);
    w.pump();
    assert_eq!(w.told(0), [due(6, why::CHARGING, "charge", true)]);
}

#[test]
fn what_left_a_hold_is_a_hint_to_the_subscribed_plugins() {
    let mut w = world();
    w.declare(
        0,
        &Spec {
            needs: 1,
            ..spec("sync", 2)
        },
    );
    w.env.level.set(2);
    w.pump();
    assert!(w.told(0).is_empty(), "held in the offline mode");
    w.env.level.set(0);
    w.env.now.set(2_000);
    w.pump();
    let told = w.told(0);
    assert_eq!(told.len(), 2, "{told:?}");
    assert_eq!(told[0].tag, 105);
    assert_eq!(told[1], due(2, why::TIME, "sync", true));
}

#[test]
fn a_run_that_reaches_its_longest_time_ends() {
    let mut w = world();
    // The plugin opens a request of its run in the callback: the run stays.
    w.holds[0].set(true);
    w.declare(0, &spec("long", 8));
    w.pump();
    assert_eq!(w.told(0), [due(8, why::TIME, "long", true)]);
    assert_eq!(w.sched.counts(), (1, 1), "the run is open");
    assert_eq!(w.kernel.work_pump_due(), Some(Instant(1_000 + RUN_MAX_MS)));
    w.env.now.set(1_000 + RUN_MAX_MS - 1);
    w.pump();
    assert_eq!(w.sched.counts(), (1, 1), "not yet");
    w.env.now.set(1_000 + RUN_MAX_MS);
    w.pump();
    assert_eq!(w.sched.counts(), (0, 0), "the host ended it");
    assert_eq!(w.kernel.work_pump_due(), None);
}

#[test]
fn nothing_is_delivered_inside_a_callback() {
    let mut w = world();
    w.declare(0, &spec("job", 1));
    w.kernel.shared.native_instances.test_enter_callback();
    assert!(w.pump(), "asks for another pass");
    assert!(w.told(0).is_empty());
}

#[test]
fn one_pass_calls_a_capped_number_of_plugins_and_asks_for_another() {
    let mut w = world_with(&ALL, 2);
    for n in 0..20 {
        let key = std::format!("w{n}");
        w.declare(0, &spec(&key, n));
        w.declare(1, &spec(&key, n));
    }
    assert!(w.pump(), "work remains");
    let first = w.told(0).len() + w.told(1).len();
    assert_eq!(first, 32);
    assert!(!w.pump());
    assert_eq!(w.told(0).len() + w.told(1).len(), 8);
}

/// `core.net` wired on the world's pool, the work scheduler attached to its
/// owner directory (`wire_work`): a request a `work_due` callback starts
/// belongs to the run.
mod network {
    use super::*;
    use crate::native_host::admitted_features::AdmittedFeatures;
    use crate::native_host::{
        event_tags::EventTags,
        host_services::{Caller, HostServices},
        net_service,
        service_events::ServiceEvents,
        shell_wiring::{
            HostWiring, HttpDone, HttpJob, HttpReason, NetInputs, NetLimits, NetReports,
            ServedReply, ServerReason, ShellHttp, ShellNetwork,
        },
    };
    use paperpaw_sdk::MemoryStorage;

    #[derive(Default)]
    struct Shell {
        started: RefCell<Vec<u32>>,
    }
    impl ShellHttp for Shell {
        fn start(&self, job: &HttpJob<'_>) -> Result<(), i32> {
            self.started.borrow_mut().push(job.id);
            Ok(())
        }
        fn abort(&self, _: u32) {}
        fn listen(&self, _: u32, _: u32, _: &mut [u8]) -> Result<usize, ServerReason> {
            Err(ServerReason::Refused)
        }
        fn unlisten(&self, _: u32) {}
        fn reply(&self, _: u32, _: &ServedReply<'_>) -> Result<(), i32> {
            Ok(())
        }
        fn close_served(&self, _: u32, _: u32) {}
    }
    impl ShellNetwork for Shell {
        fn now_ms(&self) -> u64 {
            0
        }
        fn network_up(&self) -> bool {
            true
        }
        fn network_demand(&self, _: bool) {}
    }

    /// A world whose plugin starts one `core.net` request in its `work_due`
    /// callback; `(world, the request's id, the shell's reports)`.
    fn with_request() -> (World, Rc<Cell<u32>>, Rc<NetReports>) {
        let mut w = world();
        let shell = Rc::new(Shell::default());
        let (reports, host) = {
            let resources = w.kernel.shared.native_instances.test_resources().unwrap();
            let wired = resources
                .wire(HostWiring::new().net(NetInputs::new(
                    Rc::new(MemoryStorage::new()),
                    shell.clone(),
                    shell,
                    NetLimits::new(49_152, 2),
                )))
                .unwrap();
            let reports = Rc::new(wired.net().unwrap().clone());
            let host = Rc::clone(&resources.hosts().net().unwrap().0);
            // The plugin subscribed to `http_done` (a request needs it).
            let owners = resources.hosts().owners().unwrap();
            let _ = owners.admit(w.owners[0], "a");
            owners.subscribe(
                w.owners[0],
                EventTags::collect(|tag| tag == wire::EVENT_HTTP_DONE_TAG),
            );
            (reports, host)
        };
        // The user's network settings, read into the policy as at start.
        w.kernel.sync_net_policy();
        let id = Rc::new(Cell::new(0));
        let started = Rc::clone(&id);
        *w.on_dues[0].borrow_mut() = Some(Rc::new(move |owner| {
            let hosts = HostServices::new();
            let who = Caller {
                owner,
                plugin: "a",
                features: AdmittedFeatures::NONE,
                hosts: &hosts,
            };
            let mut input = [0u8; 4_096];
            let used = wire::records_encode_counted(
                &[wire::Record::ServiceNetRequest(wire::ServiceNetRequest {
                    token: 3,
                    method: "GET",
                    url: "https://example.org/a",
                    flags: 0,
                    timeout_ms: 0,
                    max_body: 0,
                    max_file: 0,
                })],
                20,
                4_096,
                &mut input,
            )
            .unwrap();
            let input = &input[..used];
            started.set(net_service::request_for_tests(&*host, &who, input).unwrap());
        }));
        w.declare(0, &spec("fetch", 1));
        (w, id, reports)
    }

    #[test]
    fn a_request_the_callback_starts_keeps_the_run_open_until_it_ends() {
        let (mut w, id, reports) = with_request();
        w.pump();
        assert_eq!(w.told(0), [due(1, why::TIME, "fetch", true)]);
        assert_ne!(id.get(), 0, "the request was started");
        // The callback returned; the request goes on, so does the run.
        assert_eq!(w.sched.counts(), (1, 1));
        assert_eq!(w.kernel.work_pump_due(), Some(Instant(1_000 + RUN_MAX_MS)));
        reports
            .finished(id.get(), HttpDone::new(200, HttpReason::Ok))
            .unwrap();
        assert_eq!(w.sched.counts(), (0, 0), "the run ended with its request");
    }

    #[test]
    fn the_longest_time_ends_the_runs_request_with_work_ended() {
        let (mut w, id, _reports) = with_request();
        w.pump();
        assert_eq!(w.sched.counts(), (1, 1));
        w.env.now.set(1_000 + RUN_MAX_MS);
        w.pump();
        assert_eq!(w.sched.counts(), (0, 0), "the run is over");
        // What the plugin is told of its request: ended with reason 14.
        let instances = &w.kernel.shared.native_instances;
        let net = instances.test_resources().unwrap().hosts().net().unwrap();
        let mut buffer = [0u8; 1_280];
        let owner = w.owners[0];
        let len = net.next_event(owner, &mut buffer).unwrap().unwrap();
        // A one-record batch, as `service_event` and the SDK read it.
        let wire::Record::EventHttpDone(done) =
            wire::records_decode_single(&buffer[..len]).unwrap()
        else {
            panic!("an http_done event");
        };
        assert_eq!((done.id, done.reason), (id.get(), 14));
    }
}
