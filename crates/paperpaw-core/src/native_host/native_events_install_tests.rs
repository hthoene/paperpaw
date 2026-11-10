//! A library that only registers events (no declaration) is admitted and
//! subscribed to them, and the kernel's events reach its real callback in one
//! order: `book_closed` (65-67 are registrable) comes before `suspend`.
use super::*;

/// What the callback of tokens 95-98 was told: (tag, reason) in order.
pub(super) fn told(token: u32, bytes: PpBytes) -> i32 {
    if !(95..=98).contains(&token) {
        return ERR_INVALID;
    }
    // SAFETY: the callback borrows the host's initialized event for this call.
    let bytes = unsafe { core::slice::from_raw_parts(bytes.ptr, bytes.len) };
    let row = match wire::records_decode_single(bytes) {
        Ok(wire::Record::EventBookOpened(event)) => (65, event.reason, 0, 0),
        Ok(wire::Record::EventBookClosed(event)) => (66, event.reason, 0, 0),
        Ok(wire::Record::EventBookFinished(event)) => (67, event.reason, 0, 0),
        Ok(wire::Record::EventSuspend(_)) => (34, 0, 0, 0),
        Ok(_) => return ERR_INVALID,
        Err(error) => return error,
    };
    EVENTS.with(|events| events.borrow_mut().push(row));
    OK
}

#[test]
fn an_events_only_library_is_admitted_and_subscribed() {
    EMPTY.with(|value| value.set(true));
    let (mut kernel, result) = regs_kernel(1);
    EMPTY.with(|value| value.set(false));
    result.unwrap();
    let key = kernel.shared.native_instances.find("factory").unwrap();
    let instances = &kernel.shared.native_instances;
    assert!(instances.subscribed(key, wire::EVENT_STARTED_TAG));
    assert!(
        !kernel
            .shared
            .registry
            .screens
            .contains_key("factory.screen")
    );
    assert!(kernel.unload_plugin(&Id::new_static("factory")));
    assert_eq!(DESTROYED.with(Cell::get), 1);
}

fn location(reason: &str) -> Params {
    let mut params = Params::new();
    for (key, value) in [
        ("section", "2"),
        ("offset", "10"),
        ("progress", "0.5"),
        ("path", "/books/a.epub"),
        ("title", "T"),
        ("chapter", "One"),
        ("reason", reason),
    ] {
        params.set(key, value);
    }
    params
}

#[test]
fn the_book_events_register_and_book_closed_comes_before_suspend() {
    use paperpaw_sdk::builtin::events as builtin;
    EVENTS.with(|events| events.borrow_mut().clear());
    EMPTY.with(|value| value.set(true));
    let (mut kernel, result) = regs_kernel(14);
    EMPTY.with(|value| value.set(false));
    result.unwrap();
    let key = kernel.shared.native_instances.find("factory").unwrap();
    for tag in [65, 66, 67, 34] {
        assert!(
            kernel.shared.native_instances.subscribed(key, tag),
            "tag {tag}"
        );
    }
    assert!(!kernel.shared.native_instances.subscribed(key, 32));
    // Outside a callback the pump tells them in the order they were raised.
    kernel.emit(&builtin::BOOK_OPENED.id().clone(), &location("0"));
    kernel.emit(&builtin::BOOK_FINISHED.id().clone(), &location("0"));
    while kernel.native_setting_pump() {}
    let told: Vec<_> = EVENTS.with(|events| events.borrow_mut().drain(..).collect());
    assert_eq!(told, [(65, 0, 0, 0), (67, 0, 0, 0)]);
    // Raised while a callback is on the stack: it waits, then suspend drains
    // the queue in the order the events were raised.
    kernel.shared.native_instances.test_enter_callback();
    kernel.emit(&builtin::BOOK_CLOSED.id().clone(), &location("1"));
    kernel.shared.native_instances.test_leave_callback();
    kernel.emit(&builtin::SUSPEND.id().clone(), &Params::new());
    let told: Vec<_> = EVENTS.with(|events| events.borrow().clone());
    assert_eq!(told, [(66, 1, 0, 0), (34, 0, 0, 0)]);
}
