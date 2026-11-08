//! Every allocation of the screen lock's rules may be refused: each ends in
//! `NoMemory` or succeeds, never aborts.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the tests")]
#[path = "../../test-support/fail_alloc.rs"]
mod fail_alloc;
use paperpaw_plugin_lock::native_model as model;

#[global_allocator]
static ALLOC: fail_alloc::Failing = fail_alloc::Failing;

#[test]
fn keeping_and_checking_a_code_survive_refused_allocations() {
    fail_alloc::sweep(|| {
        let salt = model::salt(Some("abcdefghijklmnop"), "", 0)?;
        let kept = model::kept_value("pin", &salt, "1234")?;
        assert!(model::check(Some(&kept), "1234")?);
        assert!(!model::check(Some(&kept), "1235")?);
        let old = model::kept_value("pin", "legacy", "1234")?;
        assert_eq!(model::renewal(Some(&old), "1234")?, Some("pin"));
        model::salt(None, "device", 7)?;
        Ok(())
    });
}

#[test]
fn the_keypad_and_the_pattern_survive_refused_allocations() {
    fail_alloc::sweep(|| {
        let mut pin = model::Pin::default();
        for key in [0, 1, 2, 3, 9, 10, 11] {
            pin.press(key)?;
        }
        let mut dots = String::new();
        for dot in [4, 0, 8, 4] {
            model::add_dot(&mut dots, dot)?;
        }
        model::format_tries(5)?;
        Ok(())
    });
}
