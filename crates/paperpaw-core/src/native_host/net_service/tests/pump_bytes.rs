//! The bytes a plugin's `service_event` gets from the real pump: the batch of
//! one the SDK frames with `Incoming::new`, byte for byte.
use super::*;
use crate::native_host::{codec, net_service::NetServices, service_pump::tests as pump};
use alloc::rc::Rc;
use paperpaw_sdk::native::events::{Event, Incoming};

#[test]
fn a_plugin_is_given_event_99_as_a_batch_of_one_it_decodes() {
    let f = pump::fixture(1);
    let fake = Rc::new(Fake::x4());
    let id = request(&fake, f.keys[0], &get(7)).unwrap();
    finish(&fake, id, 200, b"x");
    let services = NetServices(super::super::NetRef::Handed(fake.clone()));
    pump::run_services(&f, [Some(&services), None, None, None]);
    let seen = f.seen[0].borrow();
    assert_eq!(seen.len(), 1, "one event, once");
    let bytes = &seen[0];
    let Ok(Event::HttpDone(done)) = Incoming::new(bytes).unwrap().decode() else {
        panic!("the plugin decodes event 99")
    };
    assert_eq!((done.token(), done.status()), (7, 200));
    // Byte for byte: a count of one, then the record, nothing after.
    let record = Record::EventHttpDone(wire::EventHttpDone {
        now_ms: done.now_ms(),
        id,
        token: 7,
        status: 200,
        reason: 0,
        flags: 0,
        body_len: 1,
    });
    let mut expected = std::vec![0; 128];
    let used = codec::encode_batch(&[record], &mut expected).unwrap();
    assert_eq!(bytes.as_slice(), &expected[..used]);
    assert_eq!(bytes.first(), Some(&1), "the count is the first byte");
}
