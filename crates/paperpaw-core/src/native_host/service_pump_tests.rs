//! The event pump: order, the per-pass cap, delivery after a failing callback,
//! never inside a callback, and the hooks.
#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]
extern crate std;
use super::*;
use crate::native_host::{
    native_instances::{NativeInstance, NativeInstances, NativeLease},
    native_screen::Dispatch,
};
use alloc::{rc::Rc, vec, vec::Vec};
use core::cell::{Cell, RefCell};
use paperpaw_native::host_glue::{ERR_NO_SPACE, ERR_UNSUPPORTED};
use paperpaw_sdk::Id;

/// A plugin that takes service events: what it was given, and what it
/// answers.
struct Plugin {
    running: Cell<bool>,
    seen: Rc<RefCell<Vec<Vec<u8>>>>,
    answer: Cell<Result<i32, i32>>,
    depth: Cell<u8>,
}
impl NativeInstance for Plugin {
    fn running(&self) -> bool {
        self.running.get()
    }
    fn stop(&self) {
        self.running.set(false);
    }
    fn callback_depth(&self) -> u8 {
        self.depth.get()
    }
    fn service_event(&self, bytes: &[u8], _: &Env<'_>) -> Result<i32, i32> {
        self.seen.borrow_mut().push(bytes.to_vec());
        self.answer.get()
    }
}
impl Dispatch for Plugin {
    fn event(&self, _: u32, _: &[u8], _: &Env<'_>) -> Result<i32, i32> {
        Err(ERR_UNSUPPORTED)
    }
    fn input(&self, _: u32, _: &[u8], _: &Env<'_>) -> Result<i32, i32> {
        Err(ERR_UNSUPPORTED)
    }
    fn draw(
        &self,
        _: u32,
        _: &crate::native_host::registrations::DrawFrame<'_>,
        _: &Env<'_>,
        _: &mut paperpaw_graphics::Canvas<'_>,
    ) -> Result<i32, i32> {
        Err(ERR_UNSUPPORTED)
    }
    fn dropped(&self, _: u32, _: i32) {}
}

/// A service with a queue of events per owner, and a log of what it was asked.
#[derive(Default)]
struct Service {
    queue: RefCell<Vec<(u64, Vec<u8>)>>,
    log: RefCell<Vec<&'static str>>,
    delivered: RefCell<Vec<u64>>,
    busy_next: Cell<bool>,
    ticks: Cell<u32>,
    deadline: Cell<Option<u64>>,
}
impl ServiceEvents for Service {
    fn next_event(&self, owner: u64, out: &mut [u8]) -> Result<Option<(usize, Option<u64>)>, i32> {
        if self.busy_next.replace(false) {
            return Err(ERR_BUSY);
        }
        let mut queue = self.queue.borrow_mut();
        let Some(at) = queue.iter().position(|(o, _)| *o == owner) else {
            return Ok(None);
        };
        let bytes = &queue[at].1;
        let Some(to) = out.get_mut(..bytes.len()) else {
            return Err(ERR_NO_SPACE);
        };
        to.copy_from_slice(bytes);
        let len = bytes.len();
        queue.remove(at);
        Ok(Some((len, None)))
    }
    fn event_delivered(&self, owner: u64) -> Result<(), i32> {
        self.log.borrow_mut().push("delivered");
        self.delivered.borrow_mut().push(owner);
        Ok(())
    }
    fn screen_closed(&self, _: u64, _: u64) -> Result<(), i32> {
        self.log.borrow_mut().push("screen_closed");
        Ok(())
    }
    fn work_ended(&self, _: u64, _: u64) -> Result<(), i32> {
        self.log.borrow_mut().push("work_ended");
        Ok(())
    }
    fn tick(&self) -> Result<(), i32> {
        self.ticks.set(self.ticks.get() + 1);
        Ok(())
    }
    fn next_deadline(&self) -> Option<u64> {
        self.deadline.get()
    }
}

pub(crate) struct Fixture {
    pub(crate) instances: NativeInstances,
    pub(crate) keys: Vec<u64>,
    /// The bytes each plugin's `service_event` was given.
    pub(crate) seen: Vec<Rc<RefCell<Vec<Vec<u8>>>>>,
    plugins: Vec<Rc<Cell<Result<i32, i32>>>>,
}

pub(crate) fn fixture(count: usize) -> Fixture {
    let mut instances = NativeInstances::default();
    let (mut keys, mut seen, mut plugins) = (Vec::new(), Vec::new(), Vec::new());
    for n in 0..count {
        let log = Rc::new(RefCell::new(Vec::new()));
        let plugin = Plugin {
            running: Cell::new(true),
            seen: Rc::clone(&log),
            answer: Cell::new(Ok(0)),
            depth: Cell::new(0),
        };
        let lease = NativeLease::try_new(plugin).unwrap();
        let id = Id::try_parse(["a", "b", "c"][n]).unwrap();
        keys.push(instances.test_insert(id, Some(lease), true).unwrap());
        seen.push(log);
        plugins.push(Rc::new(Cell::new(Ok(0))));
    }
    Fixture {
        instances,
        keys,
        seen,
        plugins,
    }
}

/// The delivery of a pump without a kernel: no stage, the plain callback.
fn plain<'a>(
    env: &'a Env<'a>,
) -> impl Fn(u64, &super::super::native_instances::NativeLease, &[u8]) + 'a {
    move |_, lease, bytes| {
        let _ = lease.service_event(bytes, env);
    }
}

fn run(f: &Fixture, service: &Service) -> bool {
    run_at(f, service, 0)
}

/// One pass of the real pump over `services`, at time 0.
pub(crate) fn run_services(f: &Fixture, services: [Option<&dyn ServiceEvents>; 3]) -> bool {
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(paperpaw_platform::Instant(0));
    pump(&f.instances, &services, &env, &plain(&env))
}

fn run_at(f: &Fixture, service: &Service, ms: u64) -> bool {
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(paperpaw_platform::Instant(ms));
    pump(
        &f.instances,
        &[Some(service), None, None],
        &env,
        &plain(&env),
    )
}

fn queue(service: &Service, owner: u64, tags: &[u8]) {
    for tag in tags {
        service.queue.borrow_mut().push((owner, vec![*tag]));
    }
}

#[test]
fn each_event_goes_to_its_plugin_in_order_and_is_delivered_after_its_callback() {
    let f = fixture(2);
    let service = Service::default();
    queue(&service, f.keys[0], &[1, 2]);
    queue(&service, f.keys[1], &[9]);
    assert!(!run(&f, &service), "nothing remains");
    assert_eq!(*f.seen[0].borrow(), [vec![1], vec![2]]);
    assert_eq!(*f.seen[1].borrow(), [vec![9]]);
    assert_eq!(
        *service.delivered.borrow(),
        [f.keys[0], f.keys[0], f.keys[1]]
    );
    assert_eq!(service.ticks.get(), 1, "the clock runs once a pass");
    let _ = &f.plugins;
}

#[test]
fn a_plugin_gets_a_capped_number_of_events_and_another_pass_is_asked_for() {
    let f = fixture(2);
    let service = Service::default();
    queue(&service, f.keys[0], &[0; PER_OWNER + 3]);
    queue(&service, f.keys[1], &[7]);
    assert!(run(&f, &service), "events remain");
    assert_eq!(f.seen[0].borrow().len(), PER_OWNER);
    assert_eq!(
        f.seen[1].borrow().len(),
        1,
        "the other plugin is not starved"
    );
    assert!(!run(&f, &service), "the second pass empties the queue");
    assert_eq!(f.seen[0].borrow().len(), PER_OWNER + 3);
}

#[test]
fn a_busy_table_asks_for_another_pass_and_loses_nothing() {
    let f = fixture(1);
    let service = Service::default();
    queue(&service, f.keys[0], &[5]);
    service.busy_next.set(true);
    assert!(run(&f, &service));
    assert!(f.seen[0].borrow().is_empty());
    assert!(!run(&f, &service));
    assert_eq!(*f.seen[0].borrow(), [vec![5]]);
}

#[test]
fn a_failing_callback_still_frees_the_event_and_does_not_get_it_again() {
    let mut f = fixture(1);
    let service = Service::default();
    let failing = NativeLease::try_new(Plugin {
        running: Cell::new(true),
        seen: Rc::clone(&f.seen[0]),
        answer: Cell::new(Err(ERR_UNSUPPORTED)),
        depth: Cell::new(0),
    })
    .unwrap();
    // Replace the plugin by one that fails.
    let key = f
        .instances
        .test_insert(Id::try_parse("d").unwrap(), Some(failing), true)
        .unwrap();
    queue(&service, key, &[3]);
    assert!(!run(&f, &service));
    assert_eq!(f.seen[0].borrow().len(), 1, "called once");
    assert_eq!(*service.delivered.borrow(), [key]);
    assert!(service.queue.borrow().is_empty());
}

#[test]
fn nothing_is_delivered_inside_a_callback_and_the_pass_stays_wanted() {
    let f = fixture(1);
    let service = Service::default();
    queue(&service, f.keys[0], &[1]);
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(paperpaw_platform::Instant(0));
    f.instances.test_enter_callback();
    assert!(pump(
        &f.instances,
        &[Some(&service), None, None],
        &env,
        &plain(&env)
    ));
    assert!(f.seen[0].borrow().is_empty());
    assert_eq!(service.queue.borrow().len(), 1);
}

#[test]
fn a_stopped_plugin_gets_no_events() {
    let mut f = fixture(1);
    let service = Service::default();
    queue(&service, f.keys[0], &[1]);
    f.instances.stop(f.keys[0]).unwrap();
    assert!(!run(&f, &service));
    assert!(f.seen[0].borrow().is_empty());
}

#[test]
fn no_wired_service_means_no_pass_and_no_buffer() {
    let f = fixture(1);
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(paperpaw_platform::Instant(0));
    assert!(!pump(&f.instances, &[None, None, None], &env, &plain(&env)));
}

#[test]
fn a_refused_buffer_waits_for_its_retry_time_and_never_asks_for_an_immediate_pass() {
    let f = fixture(1);
    let service = Service::default();
    queue(&service, f.keys[0], &[1]);
    crate::native_host::net_service::fail_nth(1);
    assert!(!run_at(&f, &service, 1_000), "no pass is asked for at once");
    crate::native_host::net_service::fail_nth(0);
    assert_eq!(f.instances.service_retry(), Some(1_000 + RETRY_MS));
    assert!(
        f.seen[0].borrow().is_empty(),
        "the event waits, it is not lost"
    );
    // Before the retry time: nothing is tried, nothing delivered.
    assert!(!run_at(&f, &service, 1_000 + RETRY_MS - 1));
    assert!(f.seen[0].borrow().is_empty());
    // After it the buffer is made, once, and the event comes.
    assert!(!run_at(&f, &service, 1_000 + RETRY_MS));
    assert_eq!(*f.seen[0].borrow(), [vec![1]]);
    assert_eq!(f.instances.service_retry(), None);
    // The buffer is kept: no second allocation.
    queue(&service, f.keys[0], &[2]);
    crate::native_host::net_service::fail_nth(1);
    assert!(!run_at(&f, &service, 10_000));
    crate::native_host::net_service::fail_nth(0);
    assert_eq!(f.seen[0].borrow().len(), 2);
}
