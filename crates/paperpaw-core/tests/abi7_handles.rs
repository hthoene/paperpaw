//! Registry tests before the core dispatch is activated.
#[path = "../src/abi7/handles.rs"]
mod handles;
use handles::{Error, Handles, Kind};

#[test]
fn stale_owner_and_wrong_type_never_resolve_or_release() {
    let mut h = Handles::new();
    let first = h.insert(1, Kind::Font, 42).unwrap();
    assert_eq!(h.get(1, Kind::Font, first), Ok(&42));
    assert_eq!(h.get(2, Kind::Font, first), Err(Error::State));
    assert_eq!(h.remove(1, Kind::Blob, first), Err(Error::State));
    assert_eq!(h.remove(1, Kind::Font, first), Ok(42));
    let next = h.insert(1, Kind::Font, 43).unwrap();
    assert_ne!(first, next);
    assert_eq!(h.get(1, Kind::Font, first), Err(Error::State));
    assert_eq!(h.get(1, Kind::Font, next), Ok(&43));
    for invalid in [0, 1, 31, u32::MAX] {
        assert_eq!(h.get(1, Kind::Font, invalid), Err(Error::State));
    }
}

#[test]
fn owner_and_global_limits_return_unconsumed_values() {
    let mut h = Handles::new();
    assert_eq!(h.insert(0, Kind::Service, 90), Err((Error::State, 90)));
    for owner in 1..=8 {
        for value in 0..4 {
            h.insert(owner, Kind::Service, value).unwrap();
        }
        assert_eq!(
            h.insert(owner, Kind::Service, 99),
            Err((Error::Capacity, 99))
        );
    }
    assert_eq!(h.insert(9, Kind::Blob, 98), Err((Error::Capacity, 98)));
    assert!(h.take_owner(1).is_some());
    assert!(h.insert(9, Kind::Blob, 98).is_ok());
}

#[test]
fn teardown_invalidates_before_resources_are_dropped_once() {
    use std::{cell::Cell, rc::Rc};
    #[derive(Debug)]
    struct Resource(Rc<Cell<usize>>);
    impl Drop for Resource {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }
    let count = Rc::new(Cell::new(0));
    let mut h = Handles::new();
    let id = h.insert(1, Kind::Blob, Resource(count.clone())).unwrap();
    let retained = h.insert(2, Kind::Blob, Resource(count.clone())).unwrap();
    let value = h.take_owner(1).unwrap();
    assert!(h.get(1, Kind::Blob, id).is_err());
    assert!(h.get(2, Kind::Blob, retained).is_ok());
    assert_eq!(count.get(), 0);
    drop(value);
    assert_eq!(count.get(), 1);
    assert!(h.take_owner(1).is_none());
    drop(h);
    assert_eq!(count.get(), 2);
}
