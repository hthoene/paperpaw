//! The bytes a plugin's `service_event` gets from the real pump: the batch of
//! one the SDK frames with `Incoming::new`, byte for byte.
use super::*;
use crate::native_host::{codec, service_pump::tests as pump, sock_service::SockServices};
use alloc::rc::Rc;
use paperpaw_sdk::native::events::{Event, Incoming};

#[test]
fn a_plugin_is_given_event_106_as_a_batch_of_one_it_decodes() {
    let f = pump::fixture(1);
    let fake = Rc::new(Fake::x4());
    let id = connect(&fake, f.keys[0], 5).unwrap();
    connected(&*fake, id).unwrap();
    let services = SockServices(fake.clone());
    pump::run_services(&f, [None, Some(&services), None]);
    let seen = f.seen[0].borrow();
    assert_eq!(seen.len(), 1, "one event, once");
    let bytes = &seen[0];
    let Ok(Event::Socket(socket)) = Incoming::new(bytes).unwrap().decode() else {
        panic!("the plugin decodes event 106")
    };
    // Byte for byte: a count of one, then the record, nothing after.
    let record = Record::EventSocket(wire::EventSocket {
        now_ms: socket.now_ms(),
        socket: id,
        token: 5,
        kind: kind::CONNECTED,
        reason: 0,
        len: 0,
    });
    let mut expected = std::vec![0; 128];
    let used = codec::encode_batch(&[record], &mut expected).unwrap();
    assert_eq!(bytes.as_slice(), &expected[..used]);
    assert_eq!(bytes.first(), Some(&1), "the count is the first byte");
}
