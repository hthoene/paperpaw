//! Same-instance exclusion and shared nesting quota before dispatch is wired.
#[path = "../src/abi7/calls.rs"]
mod calls;
use calls::{Depth, Error, Instance};

#[test]
fn different_tokens_cannot_reenter_the_same_state() {
    let instance = Instance::new();
    let depth = Depth::new();
    let token_one = instance.enter(&depth).unwrap();
    assert!(matches!(instance.enter(&depth), Err(Error::Busy)));
    drop(token_one);
    assert!(instance.enter(&depth).is_ok());
}

#[test]
fn foreign_nesting_obeys_one_shared_depth_limit() {
    let depth = Depth::new();
    let a = Instance::new();
    let b = Instance::new();
    let c = Instance::new();
    let first = a.enter(&depth).unwrap();
    let second = b.enter(&depth).unwrap();
    assert!(matches!(c.enter(&depth), Err(Error::Depth)));
    assert!(matches!(a.enter(&depth), Err(Error::Busy)));
    drop(second);
    assert!(c.enter(&depth).is_ok());
    drop(first);
    assert!(a.enter(&depth).is_ok());
}

#[test]
fn error_exit_releases_admission() {
    let depth = Depth::new();
    let instance = Instance::new();
    let operation = || -> Result<(), Error> {
        let _call = instance.enter(&depth)?;
        Err(Error::Depth)
    };
    assert_eq!(operation(), Err(Error::Depth));
    assert!(instance.enter(&depth).is_ok());
}
