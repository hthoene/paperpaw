//! Going to sleep waits for `core.net` as for interface 6's requests and
//! servers (#458): a request the transport has holds the sleep for a while
//! (`Kernel::holds_sleep`) and puts a timed sleep off, as a server on the
//! network does (`Kernel::busy_at_sleep`). A request that waits for a network,
//! and the cable's server, do neither.
use super::*;
use crate::Kernel;
use paperpaw_platform::Instant;
use paperpaw_sdk::Id;

/// A started kernel whose `core.net` is `fake`.
fn kernel_with(fake: &Rc<Fake>) -> Kernel {
    let mut kernel = Kernel::builder(crate::tests::DEVICE).build().unwrap();
    kernel.start(Instant(0));
    let instances = &mut kernel.shared.native_instances;
    instances
        .test_insert(Id::new_static("net"), None, true)
        .unwrap();
    let host: Rc<dyn NetHost> = fake.clone();
    instances.hosts().unwrap().wire_net(host).unwrap();
    kernel
}

fn serve_on(fake: &Fake, token: u32, port: u32) {
    let hosts = HostServices::new();
    let serve = wire::ServiceNetServe {
        token,
        port,
        flags: 0,
        max_body: 0,
    };
    serving::serve(fake, &caller(OWNER, &hosts), &serve).unwrap();
}

#[test]
fn a_running_request_holds_the_sleep_and_puts_a_timed_one_off() {
    let fake = Rc::new(Fake::x4());
    let mut kernel = kernel_with(&fake);
    assert!(!kernel.holds_sleep());
    assert!(!kernel.busy_at_sleep(Instant(1_000)));
    let id = request(&fake, OWNER, &get(1)).unwrap();
    assert_eq!(*fake.started.borrow(), [id], "the transport has it");
    assert!(kernel.holds_sleep());
    assert!(kernel.busy_at_sleep(Instant(2_000)));
    finish(&fake, id, 200, b"");
    assert!(!kernel.holds_sleep());
    assert!(!kernel.busy_at_sleep(Instant(3_000)));
}

#[test]
fn a_request_waiting_for_a_network_does_not_keep_it_awake() {
    let fake = Rc::new(Fake::x4());
    fake.network.set(false);
    let mut kernel = kernel_with(&fake);
    request(&fake, OWNER, &get(1)).unwrap();
    assert!(fake.started.borrow().is_empty(), "queued");
    assert!(!kernel.holds_sleep());
    assert!(!kernel.busy_at_sleep(Instant(1_000)));
}

#[test]
fn a_server_on_the_network_puts_a_timed_sleep_off_and_the_cables_does_not() {
    let fake = Rc::new(Fake::x4());
    let mut kernel = kernel_with(&fake);
    *fake.listen_result.borrow_mut() = Some(Ok(""));
    serve_on(&fake, 1, 0);
    assert!(!kernel.busy_at_sleep(Instant(1_000)), "the cable's server");
    *fake.listen_result.borrow_mut() = None;
    serve_on(&fake, 2, 8080);
    assert!(kernel.busy_at_sleep(Instant(2_000)));
    // A server is no request: the sleep it was asked for does not wait.
    assert!(!kernel.holds_sleep());
}
