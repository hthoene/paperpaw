//! Host-run stage ordering, transaction and reply-boundary proofs.
#![allow(clippy::unwrap_used, reason = "fixture refusal must fail the test")]
use super::*;
use crate::native_host::{native_instances::NativeInstance, stages_wire::core_widget_v1 as widget};
use alloc::{rc::Rc, vec, vec::Vec};
use core::cell::RefCell;
use paperpaw_graphics::Canvas;
use paperpaw_native::host_glue::ERR_UNSUPPORTED;

pub(crate) fn schema() -> Schema {
    Schema {
        key: "core.widget",
        major: 1,
        max_subscribers: 32,
        max_input: 3840,
        max_reply: 3840,
        reply_rows: 32,
        input: |bytes| widget::input_decode(bytes).map(|_| ()),
        reply: |bytes| widget::reply_decode(bytes).map(|_| ()),
    }
}

pub(crate) fn input() -> Vec<u8> {
    let mut bytes = vec![0; 64];
    let len = widget::WidgetMeasure::new(1, 20)
        .encode(&mut bytes)
        .unwrap();
    bytes.truncate(len);
    bytes
}

#[derive(Clone, Copy)]
pub(crate) enum Behavior {
    Good,
    Refuse,
    Oversize,
    Foreign,
    TooMany,
}
struct Fake {
    calls: Rc<RefCell<Vec<u32>>>,
    behavior: Behavior,
    live: core::cell::Cell<bool>,
}
impl NativeInstance for Fake {
    fn running(&self) -> bool {
        self.live.get()
    }
    fn stop(&self) {
        self.live.set(false);
    }
}
impl Dispatch for Fake {
    fn event(&self, _: u32, _: &[u8], _: &Env<'_>) -> Result<i32, i32> {
        Err(ERR_UNSUPPORTED)
    }
    fn input(&self, _: u32, _: &[u8], _: &Env<'_>) -> Result<i32, i32> {
        Err(ERR_UNSUPPORTED)
    }
    fn draw(
        &self,
        _: u32,
        _: &wire::DrawFrame<'_>,
        _: &Env<'_>,
        _: &mut Canvas<'_>,
    ) -> Result<i32, i32> {
        Err(ERR_UNSUPPORTED)
    }
    fn dropped(&self, _: u32, _: i32) {}
    fn stage(&self, token: u32, bytes: &[u8], out: &mut [u8], _: &Env<'_>) -> Result<i32, i32> {
        let Record::StageCall(call) = wire::records_decode_single(bytes).unwrap() else {
            panic!("stage.call");
        };
        assert_eq!((call.stage, call.schema), ("core.widget", 1));
        widget::input_decode(call.payload).unwrap();
        self.calls.borrow_mut().push(token);
        match self.behavior {
            Behavior::Refuse => Ok(ERR_STATE),
            Behavior::Oversize => Ok(i32::try_from(out.len() + 1).unwrap()),
            Behavior::Foreign => {
                out[..4].copy_from_slice(&[1, 127, 1, 0]);
                Ok(4)
            }
            Behavior::TooMany => {
                out[0] = 33;
                Ok(1)
            }
            Behavior::Good => widget::WidgetSize::new(token, 0)
                .encode(out)
                .map(|len| i32::try_from(len).unwrap()),
        }
    }
}

pub(crate) struct Harness {
    pub(crate) registry: NativeInstances,
    pub(crate) depth: super::super::DispatchDepth,
    pub(crate) calls: Rc<RefCell<Vec<u32>>>,
}
impl Harness {
    pub(crate) fn new() -> Self {
        Self {
            registry: NativeInstances::default(),
            depth: super::super::DispatchDepth::new(),
            calls: Rc::new(RefCell::new(Vec::with_capacity(64))),
        }
    }
    pub(crate) fn add(&mut self, id: &'static str, behavior: Behavior) -> usize {
        let lease = NativeLease::try_new(Fake {
            calls: self.calls.clone(),
            behavior,
            live: core::cell::Cell::new(true),
        })
        .unwrap();
        self.registry
            .insert(Id::new_static(id), Some(lease), true)
            .unwrap()
    }
}
pub(crate) fn row(plugin: usize, token: u32, order: i16, index: u32) -> Subscription<'static> {
    Subscription {
        stage: "core.widget",
        plugin,
        token,
        order,
        index,
        signature: u64::from(token),
        enabled: true,
        style_granted: true,
    }
}
fn copy_target(target: &Vec<u32>) -> Result<Vec<u32>, i32> {
    let mut staged = Vec::new();
    staged
        .try_reserve_exact(target.len() + 32)
        .map_err(|_| ERR_NO_MEMORY)?;
    staged.extend_from_slice(target);
    Ok(staged)
}
fn apply(target: &mut Vec<u32>, reply: Reply<'_>) -> Result<(), i32> {
    let value = widget::WidgetSize::decode(reply.bytes)?;
    assert_eq!(reply.signature, u64::from(value.height));
    assert!(reply.style_granted);
    assert!(!reply.plugin.as_str().is_empty());
    assert!(reply.index < 32);
    target.push(value.height);
    Ok(())
}

#[test]
fn ordering_uses_order_plugin_id_then_registration_index() {
    let mut h = Harness::new();
    let beta = h.add("beta", Behavior::Good);
    let alpha = h.add("alpha", Behavior::Good);
    let schema = schema();
    let input = input();
    let rows = [
        row(beta, 4, 1, 0),
        row(alpha, 2, 0, 3),
        row(alpha, 1, 0, 2),
        row(beta, 3, 0, 0),
    ];
    let run = Run::prepare(&schema, &h.depth, &input, &h.registry, rows).unwrap();
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let mut target = Vec::new();
    let result = run
        .apply(
            &mut target,
            &kernel.shared.env(paperpaw_platform::Instant(0)),
            copy_target,
            apply,
        )
        .unwrap();
    assert_eq!(target, [1, 2, 3, 4]);
    assert_eq!(*h.calls.borrow(), [1, 2, 3, 4]);
    assert_eq!(
        result,
        Outcome {
            applied: 4,
            refused: 0
        }
    );
    assert_eq!(h.registry.depth(), 0);
}

#[test]
fn disabled_rows_are_removed_before_the_active_subscriber_cut() {
    let mut h = Harness::new();
    let plugin = h.add("probe", Behavior::Good);
    let mut schema = schema();
    schema.max_subscribers = 2;
    let input = input();
    let mut disabled = row(plugin, 99, i16::MIN, 0);
    disabled.enabled = false;
    let rows = [
        disabled,
        row(plugin, 3, 3, 3),
        row(plugin, 2, 2, 2),
        row(plugin, 1, 1, 1),
    ];
    let run = Run::prepare(&schema, &h.depth, &input, &h.registry, rows).unwrap();
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let mut target = Vec::new();
    run.apply(
        &mut target,
        &kernel.shared.env(paperpaw_platform::Instant(0)),
        copy_target,
        apply,
    )
    .unwrap();
    assert_eq!(target, [1, 2]);
}

#[test]
fn bad_replies_and_refusal_isolate_the_subscriber() {
    let mut h = Harness::new();
    let mut rows = Vec::new();
    for (index, (id, behavior)) in [
        ("refuse", Behavior::Refuse),
        ("oversize", Behavior::Oversize),
        ("foreign", Behavior::Foreign),
        ("rows", Behavior::TooMany),
        ("good", Behavior::Good),
    ]
    .into_iter()
    .enumerate()
    {
        let plugin = h.add(id, behavior);
        rows.push(row(
            plugin,
            u32::try_from(index).unwrap(),
            i16::try_from(index).unwrap(),
            0,
        ));
    }
    let schema = schema();
    let input = input();
    let run = Run::prepare(&schema, &h.depth, &input, &h.registry, rows).unwrap();
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let mut target = vec![77];
    let result = run
        .apply(
            &mut target,
            &kernel.shared.env(paperpaw_platform::Instant(0)),
            copy_target,
            apply,
        )
        .unwrap();
    assert_eq!(
        result,
        Outcome {
            applied: 1,
            refused: 4
        }
    );
    assert_eq!(target, [77, 4]);
}

#[test]
fn failed_transaction_after_an_earlier_reply_never_changes_the_target() {
    let mut h = Harness::new();
    let plugin = h.add("probe", Behavior::Good);
    let schema = schema();
    let input = input();
    let run = Run::prepare(
        &schema,
        &h.depth,
        &input,
        &h.registry,
        [row(plugin, 1, 0, 0), row(plugin, 2, 0, 1)],
    )
    .unwrap();
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let mut target = vec![77];
    let result = run.apply(
        &mut target,
        &kernel.shared.env(paperpaw_platform::Instant(0)),
        copy_target,
        |target, reply| {
            apply(target, reply)?;
            if target.len() > 2 {
                Err(ERR_NO_MEMORY)
            } else {
                Ok(())
            }
        },
    );
    assert_eq!(result, Err(ERR_NO_MEMORY));
    assert_eq!(target, [77]);
    assert_eq!(h.depth.0.current(), 0);
    assert_eq!(h.registry.depth(), 0);
}

#[test]
fn revoked_frozen_owner_is_not_called_and_lazy_start_waits_for_run_drop() {
    let mut h = Harness::new();
    let plugin = h.add("probe", Behavior::Good);
    let lazy = h
        .registry
        .insert(Id::new_static("lazy"), None, true)
        .unwrap();
    let InstanceState::Running(lease) = h.registry.state(plugin).unwrap() else {
        panic!("running");
    };
    let schema = schema();
    let input = input();
    let run = Run::prepare(
        &schema,
        &h.depth,
        &input,
        &h.registry,
        [row(plugin, 1, 0, 0), row(lazy, 2, 0, 0)],
    )
    .unwrap();
    assert_eq!(h.registry.take_start(), None);
    assert_eq!(
        Run::prepare(&schema, &h.depth, &input, &h.registry, []).err(),
        Some(ERR_STATE)
    );
    lease.stop();
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let mut target = Vec::new();
    let outcome = run
        .apply(
            &mut target,
            &kernel.shared.env(paperpaw_platform::Instant(0)),
            copy_target,
            apply,
        )
        .unwrap();
    assert_eq!(
        outcome,
        Outcome {
            applied: 0,
            refused: 2
        }
    );
    assert!(h.calls.borrow().is_empty());
    assert_eq!(h.registry.take_start(), Some(lazy));
}
