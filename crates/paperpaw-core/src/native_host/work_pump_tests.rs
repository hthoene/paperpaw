//! The work pump: what comes due reaches the plugin that declared it under
//! its run's guard; refusals, the moments, the hint 105, runs that reach
//! their longest time, the per-pass cap, and nothing inside a callback.
#![allow(clippy::unwrap_used, reason = "fixture errors fail tests")]
extern crate std;
use super::super::{
    native_instances::{NativeInstance, NativeLease},
    native_screen::Dispatch,
    registrations as wire,
    work_sched::{Limits, Power, RUN_END_GRACE_MS, RUN_MAX_MS, Spec, WorkEnv, WorkSched, why},
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
    /// The events the plugins subscribed to (all of them the same).
    tags: Vec<u32>,
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
    fn subscribed(&self, _: u64, tag: u32) -> bool {
        self.tags.contains(&tag)
    }
}

/// What a plugin does in a callback (given its owner key), once set.
type Hook = Rc<RefCell<Option<Rc<dyn Fn(u64)>>>>;

struct Probe {
    told: Rc<RefCell<Vec<Told>>>,
    sched: crate::native_host::shared::Shared<WorkSched>,
    owner: Rc<Cell<u64>>,
    tags: Vec<u32>,
    running: Cell<bool>,
    answer: Rc<Cell<Result<i32, i32>>>,
    /// Opens a resource of the run in the due callback (the run stays open).
    hold_run: Rc<Cell<bool>>,
    /// What the plugin does in its due callback (given its owner key).
    on_due: Hook,
    /// And in the callback of the end of one of its requests.
    on_done: Hook,
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
            wire::Record::EventHttpDone(e) => {
                let told = Told {
                    tag: wire::EVENT_HTTP_DONE_TAG,
                    token: e.id,
                    code: e.reason,
                    key: String::new(),
                    quiet,
                };
                let hook = self.on_done.borrow().clone();
                if let Some(hook) = hook {
                    hook(self.owner.get());
                }
                self.told.borrow_mut().push(told);
                return self.answer.get();
            }
            wire::Record::EventSocket(e) => {
                let told = Told {
                    tag: wire::EVENT_SOCKET_TAG,
                    token: e.socket,
                    code: e.reason,
                    key: String::new(),
                    quiet,
                };
                self.told.borrow_mut().push(told);
                return self.answer.get();
            }
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
    sched: crate::native_host::shared::Shared<WorkSched>,
    env: Rc<Fake>,
    owners: Vec<u64>,
    told: Vec<Rc<RefCell<Vec<Told>>>>,
    answers: Vec<Rc<Cell<Result<i32, i32>>>>,
    holds: Vec<Rc<Cell<bool>>>,
    on_dues: Vec<Hook>,
    on_dones: Vec<Hook>,
}

const NAMES: [&str; 3] = ["a", "b", "c"];
const ALL: [u32; 3] = [103, 104, 105];

fn world_with(tags: &[u32], plugins: usize) -> World {
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let sched = crate::native_host::shared::Shared::try_new(WorkSched::new(Limits::OTHER)).unwrap();
    let env = Rc::new(Fake {
        now: Cell::new(1_000),
        up: Cell::new(true),
        level: Cell::new(0),
        denied: Cell::new(false),
        tags: tags.to_vec(),
    });
    let (mut owners, mut told) = (Vec::new(), Vec::new());
    let (mut answers, mut holds) = (Vec::new(), Vec::new());
    let (mut on_dues, mut on_dones) = (Vec::new(), Vec::new());
    for name in NAMES.iter().take(plugins) {
        let log = Rc::new(RefCell::new(Vec::new()));
        let answer = Rc::new(Cell::new(Ok(OK)));
        let hold = Rc::new(Cell::new(false));
        let me = Rc::new(Cell::new(0));
        let on_due: Hook = Rc::new(RefCell::new(None));
        let on_done: Hook = Rc::new(RefCell::new(None));
        let probe = Probe {
            told: Rc::clone(&log),
            sched: sched.try_clone().unwrap(),
            owner: Rc::clone(&me),
            tags: tags.to_vec(),
            running: Cell::new(true),
            answer: Rc::clone(&answer),
            hold_run: Rc::clone(&hold),
            on_due: Rc::clone(&on_due),
            on_done: Rc::clone(&on_done),
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
        on_dones.push(on_done);
        holds.push(hold);
    }
    kernel
        .shared
        .native_instances
        .test_resources()
        .unwrap()
        .hosts()
        .wire_work(WorkServices {
            sched: sched.try_clone().unwrap(),
            env: crate::native_host::shared::Shared::try_new(
                alloc::boxed::Box::new(Rc::clone(&env))
                    as alloc::boxed::Box<dyn crate::native_host::work_sched::WorkEnv>,
            )
            .unwrap(),
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
        on_dones,
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
    assert_eq!(
        w.sched.counts(),
        (1, 1),
        "told to end; it waits for the end events of what it started"
    );
    // No service delivers them: after the grace the run is over.
    w.env.now.set(1_000 + RUN_MAX_MS + RUN_END_GRACE_MS);
    w.pump();
    assert_eq!(w.sched.counts(), (0, 0), "the host ended it");
    assert_eq!(w.kernel.work_pump_due(), None);
}

#[test]
fn the_grace_counts_from_when_the_run_was_told_to_end() {
    let mut w = world();
    w.holds[0].set(true);
    w.declare(0, &spec("long", 8));
    w.pump();
    assert_eq!(w.sched.counts(), (1, 1));
    // The first pass after the limit is late by more than the grace.
    let late = 1_000 + RUN_MAX_MS + 2 * RUN_END_GRACE_MS;
    w.env.now.set(late);
    w.pump();
    assert_eq!(w.sched.counts(), (1, 1), "the grace starts now");
    assert_eq!(
        w.kernel.work_pump_due(),
        Some(Instant(late + RUN_END_GRACE_MS))
    );
    w.env.now.set(late + RUN_END_GRACE_MS - 1);
    w.pump();
    assert_eq!(w.sched.counts(), (1, 1), "not yet");
    w.env.now.set(late + RUN_END_GRACE_MS);
    w.pump();
    assert_eq!(w.sched.counts(), (0, 0), "over after its own grace");
}

#[test]
fn the_hints_do_not_use_up_the_pass_the_due_work_needs() {
    let mut w = world_with(&ALL, 3);
    // 32 works held in the offline mode, then free; three plugins hear the
    // hint, and every work is told in the one pass.
    for n in 0..32 {
        let key = std::format!("w{n}");
        w.declare(
            0,
            &Spec {
                needs: 1,
                ..spec(&key, n)
            },
        );
    }
    w.env.level.set(2);
    w.pump();
    assert!(w.told(0).is_empty());
    w.env.level.set(0);
    w.env.now.set(2_000);
    w.pump();
    let told = w.told(0);
    assert_eq!(told.iter().filter(|t| t.tag == 105).count(), 1);
    assert_eq!(told.iter().filter(|t| t.tag == 103).count(), 32, "{told:?}");
    assert_eq!(w.sched.counts(), (0, 0));
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
        HostResources,
        event_tags::EventTags,
        host_services::{Caller, HostServices},
        net_service,
        service_events::ServiceEvents,
        shell_wiring::{
            Connect, HostWiring, HttpDone, HttpJob, HttpReason, NetInputs, NetLimits, NetReports,
            ServedReply, ServerReason, ShellHttp, ShellNetwork, SockInputs, SockLimits,
            SockTransport,
        },
        sock_service,
    };
    use paperpaw_sdk::MemoryStorage;

    const DONE: u32 = wire::EVENT_HTTP_DONE_TAG;

    #[derive(Default)]
    struct Shell;
    impl ShellHttp for Shell {
        fn start(&self, _: &HttpJob<'_>) -> Result<(), i32> {
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
    impl SockTransport for Shell {
        fn connect(&self, _: u32, _: &Connect<'_>) -> Result<(), i32> {
            Ok(())
        }
        fn send(&self, _: u32, data: &[u8]) -> Result<usize, i32> {
            Ok(data.len())
        }
        fn read(&self, _: u32, _: u32) {}
        fn close(&self, _: u32) {}
        fn broadcast(&self, _: u32, _: &[u8], _: &[u16]) -> Result<(), i32> {
            Ok(())
        }
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

    /// The ids of the requests the plugin started, in order.
    type Started = Rc<RefCell<Vec<u32>>>;

    fn caller(owner: u64, hosts: &HostServices) -> Caller<'_> {
        Caller {
            owner,
            plugin: "a",
            features: AdmittedFeatures::NONE,
            hosts,
        }
    }

    /// A one-record counted input batch.
    fn batch(record: wire::Record<'_>) -> ([u8; 4_096], usize) {
        let mut input = [0u8; 4_096];
        let used = wire::records_encode_counted(&[record], 20, 4_096, &mut input).unwrap();
        (input, used)
    }

    /// What the plugin does to start a `core.net` request.
    fn request_starter(device: &Rc<HostResources>, started: &Started) -> Rc<dyn Fn(u64)> {
        let (device, started) = (Rc::clone(device), Rc::clone(started));
        Rc::new(move |owner| {
            let (input, used) = batch(wire::Record::ServiceNetRequest(wire::ServiceNetRequest {
                token: 3,
                method: "GET",
                url: "https://example.org/a",
                flags: 0,
                timeout_ms: 0,
                max_body: 0,
                max_file: 0,
            }));
            let hosts = HostServices::new();
            // The plugin's first call makes the device (#248).
            let host = &*device.hosts().net_for_call().unwrap().0;
            let id = net_service::request_for_tests(host, &caller(owner, &hosts), &input[..used])
                .unwrap();
            started.borrow_mut().push(id);
        })
    }

    /// What the plugin does to connect a socket to `mail.example.org`.
    fn socket_starter(device: &Rc<HostResources>, started: &Started) -> Rc<dyn Fn(u64)> {
        let (device, started) = (Rc::clone(device), Rc::clone(started));
        Rc::new(move |owner| {
            let (input, used) = batch(wire::Record::ServiceSockConnect(wire::ServiceSockConnect {
                token: 5,
                port: 143,
                flags: 0,
                timeout_ms: 0,
                idle_ms: 0,
                host: "mail.example.org",
                server_name: "",
            }));
            let hosts = HostServices::new();
            let socks = &*device.hosts().sockets_for_call().unwrap().0;
            let id = sock_service::connect_for_tests(socks, &caller(owner, &hosts), &input[..used])
                .unwrap();
            started.borrow_mut().push(id);
        })
    }

    /// A world with `core.net` wired and one plugin that starts a request in
    /// its `work_due` callback and, in the callback of the end of each
    /// request, another (a retry) while `retries` lasts.
    fn with_request(retries: u32) -> (World, Started, Rc<NetReports>) {
        with_retry(retries, false)
    }

    /// The same; the retry is a socket when `socket` is set.
    fn with_retry(retries: u32, socket: bool) -> (World, Started, Rc<NetReports>) {
        let mut w = world();
        let shell = Rc::new(Shell);
        let (reports, device) = {
            let resources = w.kernel.shared.native_instances.test_resources().unwrap();
            let wired = resources
                .wire(
                    HostWiring::new()
                        .net(NetInputs::new(
                            Rc::new(MemoryStorage::new()),
                            shell.clone(),
                            shell.clone(),
                            NetLimits::new(49_152, 2),
                        ))
                        .sockets(SockInputs::new(
                            Rc::new(MemoryStorage::new()),
                            shell.clone(),
                            shell,
                            SockLimits::small(49_152),
                        )),
                )
                .unwrap();
            let reports = Rc::new(wired.net().unwrap().clone());
            let device = Rc::new(resources.try_clone().unwrap());
            // The plugin subscribed to `http_done` (a request needs it).
            let owners = resources.hosts().owners().unwrap();
            let _ = owners.admit(w.owners[0], &paperpaw_sdk::Id::new_static("a"));
            owners.subscribe(
                w.owners[0],
                EventTags::collect(|tag| tag == DONE || tag == wire::EVENT_SOCKET_TAG),
            );
            (reports, device)
        };
        // The user's network settings, read into the policy as at start.
        w.kernel.sync_net_policy();
        let started: Started = Rc::new(RefCell::new(Vec::new()));
        let start = request_starter(&device, &started);
        let connect = socket_starter(&device, &started);
        *w.on_dues[0].borrow_mut() = Some(start.clone());
        let left = Rc::new(Cell::new(retries));
        *w.on_dones[0].borrow_mut() = Some(Rc::new(move |owner| {
            if left.get() > 0 {
                left.set(left.get() - 1);
                if socket {
                    connect(owner);
                } else {
                    start(owner);
                }
            }
        }));
        w.declare(0, &spec("fetch", 1));
        (w, started, reports)
    }

    fn finished(reports: &NetReports, id: u32) {
        reports
            .finished(id, HttpDone::new(200, HttpReason::Ok))
            .unwrap();
    }

    fn done(id: u32, code: u32, quiet: bool) -> Told {
        Told {
            tag: DONE,
            token: id,
            code,
            key: String::new(),
            quiet,
        }
    }

    #[test]
    fn a_run_counts_its_requests_until_their_end_events_were_delivered() {
        let (mut w, started, reports) = with_request(1);
        w.pump();
        assert_eq!(w.told(0), [due(1, why::TIME, "fetch", true)]);
        let first = started.borrow()[0];
        // The callback returned; the request goes on, so does the run.
        assert_eq!(w.sched.counts(), (1, 1));
        assert_eq!(w.kernel.work_pump_due(), Some(Instant(1_000 + RUN_MAX_MS)));
        // The transport is done, the plugin has not heard of it: the run goes on.
        finished(&reports, first);
        assert_eq!(w.sched.counts(), (1, 1), "the end event is not delivered");
        // Delivered in the run (quiet): the callback starts a retry, which
        // belongs to the same run, so the run goes on after the callback.
        w.kernel.native_service_pump();
        assert_eq!(w.told(0), [done(first, 0, true)]);
        let second = started.borrow()[1];
        assert_ne!(first, second);
        assert_eq!(w.sched.counts(), (1, 1), "the retry holds the run");
        // The retry ends; the run ends only after its end event's callback.
        finished(&reports, second);
        assert_eq!(w.sched.counts(), (1, 1), "not yet delivered");
        w.kernel.native_service_pump();
        assert_eq!(w.told(0), [done(second, 0, true)]);
        assert_eq!(started.borrow().len(), 2, "no third request");
        assert_eq!(w.sched.counts(), (0, 0), "the run is over");
    }

    #[test]
    fn the_longest_time_ends_the_runs_request_with_work_ended_and_a_retry_at_once() {
        let (mut w, started, _reports) = with_request(1);
        w.pump();
        assert_eq!(w.sched.counts(), (1, 1));
        let first = started.borrow()[0];
        w.env.now.set(1_000 + RUN_MAX_MS);
        w.pump();
        // Told to end; the run waits for the end event of its request.
        assert_eq!(w.sched.counts(), (1, 1));
        // The end event is delivered in the run; its callback starts a
        // retry, past the longest time: it ends at once with 14 as well.
        w.kernel.native_service_pump();
        assert_eq!(
            w.told(0),
            [due(1, why::TIME, "fetch", true), done(first, 14, true)]
        );
        let second = started.borrow()[1];
        w.kernel.native_service_pump();
        assert_eq!(w.told(0), [done(second, 14, true)]);
        assert_eq!(w.sched.counts(), (0, 0), "the run is over");
    }

    #[test]
    fn a_plugin_that_ignores_its_events_and_a_late_start_ends_at_once() {
        let (mut w, started, _reports) = with_request(1);
        w.pump();
        let first = started.borrow()[0];
        w.env.now.set(1_000 + RUN_MAX_MS);
        w.pump();
        // The plugin takes no event; the run is over after the grace.
        w.env.now.set(1_000 + RUN_MAX_MS + RUN_END_GRACE_MS);
        w.pump();
        assert_eq!(w.sched.counts(), (0, 0), "the run is over");
        assert_eq!(w.told(0), [due(1, why::TIME, "fetch", true)]);
        // The end event comes late, with its run; what its callback starts
        // is kept and ends at once with 14, in the run's callback (quiet).
        w.kernel.native_service_pump();
        assert_eq!(w.told(0), [done(first, 14, true)]);
        let second = started.borrow()[1];
        w.kernel.native_service_pump();
        assert_eq!(w.told(0), [done(second, 14, true)]);
        assert_eq!(started.borrow().len(), 2);
        assert_eq!(w.sched.counts(), (0, 0));
    }

    #[test]
    fn a_socket_started_in_an_end_event_callback_after_the_limit_ends_with_27() {
        // Within the grace the run still exists (counted, ended at once by
        // the next pass); after it, it is over and the start is kept
        // uncounted and ended at once: the same end either way.
        for grace_over in [false, true] {
            let (mut w, started, _reports) = with_retry(1, true);
            w.pump();
            let first = started.borrow()[0];
            w.env.now.set(1_000 + RUN_MAX_MS);
            w.pump();
            if grace_over {
                w.env.now.set(1_000 + RUN_MAX_MS + RUN_END_GRACE_MS);
                w.pump();
            }
            w.kernel.native_service_pump();
            let socket = started.borrow()[1];
            // Their ids are of two spaces: NET's and SOCK's.
            assert_eq!(started.borrow().len(), 2, "grace over: {grace_over}");
            w.kernel.native_service_pump();
            let told = w.told(0);
            let ended = Told {
                tag: wire::EVENT_SOCKET_TAG,
                token: socket,
                code: 27,
                key: String::new(),
                quiet: true,
            };
            assert!(told.contains(&done(first, 14, true)), "{told:?}");
            assert!(told.contains(&ended), "grace over {grace_over}: {told:?}");
            w.kernel.native_service_pump();
            assert_eq!(w.sched.counts(), (0, 0), "grace over {grace_over}");
        }
    }

    #[test]
    fn the_end_event_is_a_one_record_batch_as_plugins_read_it() {
        let (mut w, started, _reports) = with_request(0);
        w.pump();
        w.env.now.set(1_000 + RUN_MAX_MS);
        w.pump();
        let net = w
            .kernel
            .shared
            .native_instances
            .test_resources()
            .unwrap()
            .hosts()
            .net()
            .unwrap();
        let mut buffer = [0u8; 1_280];
        let (len, run) = net.next_event(w.owners[0], &mut buffer).unwrap().unwrap();
        assert_eq!(run, Some(1), "the event names its run");
        let wire::Record::EventHttpDone(event) =
            wire::records_decode_single(&buffer[..len]).unwrap()
        else {
            panic!("an http_done event");
        };
        assert_eq!((event.id, event.reason), (started.borrow()[0], 14));
    }
}
