//! Each allocation of a format stage call refuses before the format is
//! entered, and the opener's picture mapping never loses a call half way.
#![allow(clippy::unwrap_used, reason = "fixture refusal must fail the test")]
use super::{NORMAL, arm_failure, reset};
use crate::native_host::doc_service::{FormatCall, FormatRef, Options};
use crate::native_host::fmt_call::{ImageMap, Opener, Registry, Stage};
use crate::native_host::format_call::Target;
use crate::native_host::format_call::tests::{Behavior, Harness};
use crate::native_host::registrations as wire;
use paperpaw_native::host_glue::ERR_NO_MEMORY;
extern crate std;

struct None_;
impl ImageMap for None_ {
    fn map(
        &self,
        _: u64,
        _: u32,
        _: &crate::native_host::stages_wire::core_format_v1::FmtImage<'_>,
        _: bool,
    ) -> Result<Option<wire::ServiceDocImage>, i32> {
        Ok(None)
    }
}

#[test]
fn a_stage_call_allocation_failure_never_enters_the_format() {
    reset(NORMAL);
    // A fixed reply: `fmt.state` of document 9 (count 1, tag 5, 5 bytes body).
    let mut h = Harness::new();
    let reply: &'static [u8] = {
        use crate::native_host::stages_wire::core_format_v1 as local;
        let mut bytes = [0_u8; 64];
        let n = local::reply_encode(
            &[local::Record::FmtState(local::FmtState::new(
                9, 0, 0, 0, 0, 5, "x",
            ))],
            &mut bytes,
        )
        .unwrap();
        std::boxed::Box::leak(bytes[..n].to_vec().into_boxed_slice())
    };
    let plugin = h.add("format-test", Behavior::Raw(reply));
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(paperpaw_platform::Instant(0));
    let stage = Stage {
        route: &Registry {
            registry: &h.registry,
            env: &env,
        },
        images: &None_,
    };
    let call = FormatCall {
        format: FormatRef(1),
        op: 2,
        document: 9,
        section: 0,
        offset: 0,
        index: 0,
        cursor: 0,
        generation: 0,
        flags: 0,
        image_side: 0,
        cover_side: 0,
        conversion: 0,
        text: "",
        options: Options::NONE,
        css_root: 0,
        css_path: "",
    };
    let who = Opener {
        owner: 5,
        sibling: "",
    };
    let target = Target { plugin, token: 7 };
    // The call's rows, its encoded payload, the payload's exact copy, the
    // framed call and the reply buffer: the 1st to 5th allocations, all before
    // the format is entered (the fake format allocates infallibly from the 6th
    // on).
    for nth in 1..=5 {
        let mut out = [0_u8; 4096];
        arm_failure(nth);
        let r = stage.call(who, target, &call, &mut out);
        arm_failure(0);
        assert_eq!(r, Err(ERR_NO_MEMORY), "allocation {nth}");
        assert!(h.seen.borrow().is_empty(), "allocation {nth}");
        assert_eq!(h.registry.depth(), 0);
    }
    let mut out = [0_u8; 4096];
    assert!(stage.call(who, target, &call, &mut out).is_ok());
}

/// What the stage holds while the format runs: the encoded call alone, never
/// the room for the longest call (#248: the format's own peak comes on top).
#[test]
fn while_the_format_runs_the_stage_holds_only_the_encoded_call() {
    use crate::native_host::fmt_call::Route;
    use core::cell::Cell;
    struct Measure {
        before: Cell<usize>,
        held: Cell<usize>,
        payload: Cell<usize>,
    }
    impl Route for Measure {
        fn call(&self, _: Target, payload: &[u8], _: &mut [u8]) -> Result<usize, i32> {
            let live = super::HEAP_LIVE.with(Cell::get);
            self.held.set(live.saturating_sub(self.before.get()));
            self.payload.set(payload.len());
            Err(ERR_NO_MEMORY)
        }
    }
    reset(NORMAL);
    let route = Measure {
        before: Cell::new(0),
        held: Cell::new(0),
        payload: Cell::new(0),
    };
    let stage = Stage {
        route: &route,
        images: &None_,
    };
    let call = FormatCall::new(FormatRef(1), 8, 9);
    let who = Opener {
        owner: 5,
        sibling: "",
    };
    let mut out = [0_u8; 64];
    route.before.set(super::HEAP_LIVE.with(Cell::get));
    let _ = stage.call(
        who,
        Target {
            plugin: 1,
            token: 7,
        },
        &call,
        &mut out,
    );
    assert!(route.payload.get() > 0);
    assert_eq!(
        route.held.get(),
        route.payload.get(),
        "only the encoded call is held"
    );
}
