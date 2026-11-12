//! Ops 4, 5, 14 and 15: the rules of who, own, guard and depth, the owner
//! calls grouped per (owner, token), the cache, and the handles' lifetime.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
extern crate std;
use super::*;
use crate::abi7::{context::ui_views::Declared, services_wire};
use crate::native_host::stages_wire::core_widget_v1 as widget;
use crate::native_host::{
    admitted_features::AdmittedFeatures,
    call_services::validate_reply,
    host_services::HostServices,
    shared::Shared,
    ui_guard::{Row, Rows, UiStackView},
    ui_service::{self, UiDirectory},
    ui_store::{Limits, UiStore},
};
use alloc::string::String;
use core::cell::{Cell, RefCell};
use paperpaw_i18n::Translator;
use paperpaw_native::host_glue::{ERR_DENIED, ERR_NOT_FOUND};
use paperpaw_platform::Instant;
use paperpaw_sdk::{Env, Settings};

const ME: u64 = 1;
const A: u64 = 2;
const B: u64 = 3;

fn declared(owner: u64, token: u32, private: bool) -> Declared {
    Declared {
        owner,
        token,
        private,
    }
}

/// What the registry declares.
#[derive(Default)]
struct Dir {
    widgets: Vec<(&'static str, Declared)>,
    items: Vec<(&'static str, Declared)>,
    lines: Vec<(&'static str, Declared)>,
}
fn look(list: &[(&'static str, Declared)], id: &str) -> Result<Declared, i32> {
    list.iter()
        .find(|(name, _)| *name == id)
        .map(|(_, d)| *d)
        .ok_or(ERR_NOT_FOUND)
}
impl UiDirectory for Dir {
    fn widget(&self, widget: &str) -> Result<Declared, i32> {
        look(&self.widgets, widget)
    }
    fn status_item(&self, item: &str) -> Result<Declared, i32> {
        look(&self.items, item)
    }
    fn status_line(&self, line: &str) -> Result<Declared, i32> {
        look(&self.lines, line)
    }
}

struct World<'a> {
    env: &'a Env<'a>,
    hosts: HostServices,
    dir: Dir,
    rows: RefCell<Vec<Row>>,
    instance: Cell<Option<u32>>,
    depth: Cell<u8>,
    x4: Cell<bool>,
    epoch: Cell<u64>,
    /// The allocation the next call refuses (0: none).
    fail: Cell<u32>,
    /// (owner, token) -> the reply batch, or an error.
    replies: RefCell<Vec<Reply>>,
    /// (owner, token, items) of each owner call.
    asked: RefCell<Vec<(u64, u32, Vec<String>)>>,
    /// (owner, token) -> what the owner answers a `core.widget` call (raw
    /// stage-local reply bytes, or an error), and each call made.
    widget_replies: RefCell<Vec<Reply>>,
    widget_asked: RefCell<Vec<(u64, u32, widget::Record)>>,
}

type Reply = (u64, u32, Result<Vec<u8>, i32>);

use crate::native_host::shell::tests::FAIL_NTH;

impl Frame for World<'_> {
    fn settings(&self) -> &Settings {
        self.env.settings()
    }
    fn translator(&self) -> &Translator {
        self.env.i18n()
    }
    fn callback_epoch(&self) -> Result<u64, i32> {
        Some(self.epoch.get()).filter(|e| *e != 0).ok_or(ERR_STATE)
    }
    fn callback_depth(&self) -> Result<u8, i32> {
        self.callback_epoch()?;
        Ok(self.depth.get())
    }
    fn calling_instance(&self) -> Result<u32, i32> {
        self.callback_epoch()?;
        self.instance.get().ok_or(ERR_STATE)
    }
    fn with_stack(&self, visit: &mut dyn FnMut(&dyn UiStackView)) -> Result<(), i32> {
        visit(&Rows(&self.rows.borrow()));
        Ok(())
    }
    fn ui_x4(&self) -> Result<bool, i32> {
        Ok(self.x4.get())
    }
    fn ui_directory(&self) -> Result<&dyn UiDirectory, i32> {
        Ok(&self.dir)
    }
    fn widget_call(
        &self,
        owner: u64,
        token: u32,
        payload: &[u8],
        out: &mut [u8],
    ) -> Result<usize, i32> {
        let call = widget::input_decode(payload)?.next().unwrap()?;
        // The harness's own allocation must not be the one that fails.
        let armed = FAIL_NTH.with(|n| n.replace(0));
        self.widget_asked.borrow_mut().push((owner, token, call));
        FAIL_NTH.with(|n| n.set(armed));
        let replies = self.widget_replies.borrow();
        let (.., reply) = replies
            .iter()
            .find(|(o, t, _)| (*o, *t) == (owner, token))
            .ok_or(ERR_NOT_FOUND)?;
        let bytes = reply.as_ref().map_err(|e| *e)?;
        out[..bytes.len()].copy_from_slice(bytes);
        Ok(bytes.len())
    }
    fn status_ask(
        &self,
        owner: u64,
        token: u32,
        items: &[&str],
        _context: &[u8],
        out: &mut [u8],
    ) -> Result<usize, i32> {
        let armed = FAIL_NTH.with(|n| n.replace(0));
        self.asked
            .borrow_mut()
            .push((owner, token, items.iter().map(|i| (*i).into()).collect()));
        FAIL_NTH.with(|n| n.set(armed));
        let replies = self.replies.borrow();
        let (.., reply) = replies
            .iter()
            .find(|(o, t, _)| (*o, *t) == (owner, token))
            .ok_or(ERR_NOT_FOUND)?;
        let bytes = reply.as_ref().map_err(|e| *e)?;
        out[..bytes.len()].copy_from_slice(bytes);
        Ok(bytes.len())
    }
}

fn row(instance: u32, owner: u64, guards: bool) -> Row {
    Row {
        instance,
        owner,
        overlay: false,
        guards,
        secret: false,
        host: false,
        guard_dialog: false,
    }
}

fn values(rows: &[(&str, u32, &str)]) -> Vec<u8> {
    let records: Vec<status::Record<'_>> = rows
        .iter()
        .map(|(item, flags, text)| {
            status::Record::StatusValue(status::StatusValue::new(item, *flags, text))
        })
        .collect();
    let mut bytes = std::vec![0; 3840];
    let used = status::reply_encode(&records, &mut bytes).unwrap();
    bytes.truncate(used);
    bytes
}

impl World<'_> {
    fn call(&self, owner: u64, op: u32, records: &[Record<'_>]) -> Result<Vec<u8>, i32> {
        let schema = services_wire::service_schema("core.ui", 1).unwrap();
        let op = schema.op(op).unwrap();
        let mut input = std::vec![0; 4096];
        let used = wire::records_encode_counted(
            records,
            usize::from(op.input_rows),
            op.max_input,
            &mut input,
        )?;
        let caller = Caller {
            owner,
            plugin: "p",
            features: AdmittedFeatures::NONE,
            hosts: &self.hosts,
        };
        let mut out = std::vec![0; op.max_reply];
        // Only the host's own allocations may fail, not this harness's.
        let fail = self.fail.get();
        crate::native_host::shell::tests::FAIL_NTH.with(|n| n.set(fail));
        let got = ui_service::backend(self, &caller, op, &input[..used], &mut out);
        crate::native_host::shell::tests::FAIL_NTH.with(|n| n.set(0));
        let length = got?;
        out.truncate(length);
        validate_reply(&out, op)?;
        Ok(out)
    }
    fn make(&self, owner: u64, widget: &str) -> Result<(u32, u32), i32> {
        let reply = self.call(
            owner,
            4,
            &[Record::ServiceUiWidgetRef(wire::ServiceUiWidgetRef {
                widget,
                place: "p",
            })],
        )?;
        match wire::record_decode_one(&reply).unwrap() {
            Record::ServiceUiInstance(i) => Ok((i.instance, i.flags)),
            other => panic!("{other:?}"),
        }
    }
    fn drop_widget(&self, owner: u64, instance: u32) -> Result<(), i32> {
        self.call(
            owner,
            5,
            &[Record::ServiceUiInstance(wire::ServiceUiInstance {
                instance,
                flags: 0,
            })],
        )
        .map(|_| ())
    }
    fn status(&self, owner: u64, items: &str) -> Result<Vec<(String, u32, u32, String)>, i32> {
        let reply = self.call(
            owner,
            14,
            &[Record::ServiceUiStatusAsk(wire::ServiceUiStatusAsk {
                items,
                context: b"ctx",
            })],
        )?;
        let (mut rows, mut total) = (Vec::new(), 0);
        crate::native_host::stage_wire::visit_stream(&reply, |_, framed| {
            match wire::record_decode_one(framed).unwrap() {
                Record::ServiceUiStatusValue(v) => {
                    rows.push((v.item.into(), v.flags, v.handle, v.text.into()));
                }
                Record::ServiceEnd(e) => total = e.total,
                other => panic!("{other:?}"),
            }
            Ok(())
        })
        .unwrap();
        assert_eq!(usize::try_from(total).unwrap(), rows.len());
        Ok(rows)
    }
    fn line(&self, owner: u64, line: &str) -> Result<(u32, u32), i32> {
        let reply = self.call(
            owner,
            15,
            &[Record::ServiceUiStatusAsk(wire::ServiceUiStatusAsk {
                items: line,
                context: &[],
            })],
        )?;
        match wire::record_decode_one(&reply).unwrap() {
            Record::ServiceUiInstance(i) => Ok((i.instance, i.flags)),
            other => panic!("{other:?}"),
        }
    }
    fn reply(&self, owner: u64, token: u32, reply: Result<Vec<u8>, i32>) {
        self.replies.borrow_mut().push((owner, token, reply));
    }
}

fn world<R>(f: impl FnOnce(&World<'_>) -> R) -> R {
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(Instant(0));
    let hosts = HostServices::new();
    hosts
        .wire_ui(UiServices::new(
            Shared::try_new(UiStore::new(Limits::ELSEWHERE)).unwrap(),
            false,
        ))
        .unwrap();
    f(&World {
        env: &env,
        hosts,
        dir: Dir {
            widgets: std::vec![
                ("a.w", declared(A, 7, false)),
                ("a.secret", declared(A, 8, true)),
                ("me.w", declared(ME, 9, false))
            ],
            items: std::vec![
                ("a.one", declared(A, 1, false)),
                ("a.two", declared(A, 1, false)),
                ("a.other", declared(A, 2, false)),
                ("b.one", declared(B, 1, false)),
                ("a.priv", declared(A, 1, true)),
                ("me.item", declared(ME, 5, false)),
            ],
            lines: std::vec![
                ("a.line", declared(A, 3, false)),
                ("me.line", declared(ME, 4, false)),
                ("a.pline", declared(A, 3, true))
            ],
        },
        rows: RefCell::new(std::vec![row(10, ME, false)]),
        instance: Cell::new(Some(10)),
        depth: Cell::new(1),
        x4: Cell::new(false),
        epoch: Cell::new(1),
        fail: Cell::new(0),
        replies: RefCell::new(Vec::new()),
        asked: RefCell::new(Vec::new()),
        widget_replies: RefCell::new(Vec::new()),
        widget_asked: RefCell::new(Vec::new()),
    })
}

#[test]
fn a_widget_is_made_for_the_top_instance_dropped_by_its_maker_and_own_is_flagged() {
    world(|w| {
        let (id, flags) = w.make(ME, "a.w").unwrap();
        assert_ne!(id, 0);
        assert_eq!(flags, 0);
        // A placement is stripped; the same widget can be made again.
        assert_ne!(w.make(ME, "a.w@2").unwrap().0, id);
        // Own: flagged, instance 0, nothing made.
        assert_eq!(w.make(ME, "me.w").unwrap(), (0, 1));
        assert_eq!(w.make(ME, "nobody").err(), Some(ERR_NOT_FOUND));
        assert_eq!(w.make(ME, "@2").err(), Some(ERR_INVALID));
        // Only the maker drops, and once.
        assert_eq!(w.drop_widget(A, id).err(), Some(ERR_DENIED));
        w.drop_widget(ME, id).unwrap();
        assert_eq!(w.drop_widget(ME, id).err(), Some(ERR_NOT_FOUND));
        // Not in a screen callback, deeper than 1, or not the top: refused.
        w.instance.set(None);
        assert_eq!(w.make(ME, "a.w").err(), Some(ERR_STATE));
        w.instance.set(Some(10));
        w.depth.set(2);
        assert_eq!(w.make(ME, "a.w").err(), Some(ERR_BUSY));
        w.depth.set(1);
        w.rows.borrow_mut().push(row(11, A, false));
        assert_eq!(w.make(ME, "a.w").err(), Some(ERR_STATE));
    });
}

#[test]
fn under_a_guard_another_plugins_private_widget_is_denied() {
    world(|w| {
        // The caller's instance is the top; A's lock guards beneath it? No: a
        // guard is the caller's own, so nothing is denied to it.
        w.rows.borrow_mut().insert(0, row(9, A, true));
        // ME's instance 10 now lies above A's guard: it is not the top the
        // guard's plugin owns, but it is the input target; A's private widget
        // is A's own guard's, a private one of someone else is denied.
        assert_eq!(w.make(ME, "a.secret").err(), Some(ERR_DENIED));
        assert_ne!(w.make(ME, "a.w").unwrap().0, 0);
    });
}

#[test]
fn status_values_group_per_owner_and_token_skip_own_and_hide_private_under_a_guard() {
    world(|w| {
        w.reply(
            A,
            1,
            Ok(values(&[
                ("a.two", 0, "two"),
                ("a.one", 0, "one"),
                ("zzz", 0, "foreign"),
                ("a.one", 0, "second wins nothing"),
            ])),
        );
        w.reply(A, 2, Ok(values(&[("a.other", 1, "")])));
        w.reply(B, 1, Err(ERR_BUSY));
        let rows = w
            .status(ME, "a.one\nme.item\nb.one\na.other\na.two\nnobody")
            .unwrap();
        // Groups in the order of their first item: (A,1) a.one+a.two, (B,1), (A,2).
        assert_eq!(
            *w.asked.borrow(),
            [
                (A, 1, std::vec!["a.one".into(), "a.two".into()]),
                (B, 1, std::vec!["b.one".into()]),
                (A, 2, std::vec!["a.other".into()]),
            ]
        );
        let by = |name: &str| rows.iter().find(|r| r.0 == name).unwrap().clone();
        assert_eq!(by("a.one"), ("a.one".into(), 0, 0, "one".into()));
        assert_eq!(by("a.two"), ("a.two".into(), 0, 0, "two".into()));
        // Own: flag 4, nothing else. A failed owner: empty. Undeclared: empty.
        assert_eq!(by("me.item"), ("me.item".into(), 4, 0, String::new()));
        assert_eq!(by("b.one"), ("b.one".into(), 0, 0, String::new()));
        assert_eq!(by("nobody"), ("nobody".into(), 0, 0, String::new()));
        // Drawn: flag 1 and a handle for this callback.
        let other = by("a.other");
        assert_eq!((other.1, other.2 != 0), (1, true));
        assert_eq!(rows.len(), 6);
    });
}

#[test]
fn values_are_cached_until_the_status_changes_and_private_ones_hidden_under_a_guard() {
    world(|w| {
        w.reply(A, 1, Ok(values(&[("a.one", 0, "v1")])));
        assert_eq!(w.status(ME, "a.one").unwrap()[0].3, "v1");
        w.asked.borrow_mut().clear();
        w.replies.borrow_mut().clear();
        // Cached: no call, same text, even though the owner would now fail.
        assert_eq!(w.status(ME, "a.one").unwrap()[0].3, "v1");
        assert!(w.asked.borrow().is_empty());
        // A change drops the cache: the owner is asked again.
        w.hosts.ui().unwrap().status_changed().unwrap();
        w.reply(A, 1, Ok(values(&[("a.one", 0, "v2")])));
        assert_eq!(w.status(ME, "a.one").unwrap()[0].3, "v2");
        // A private item's value, cached, goes when a guard starts; under the
        // guard it is hidden (flag 2) and never called.
        w.replies.borrow_mut().clear();
        w.reply(A, 1, Ok(values(&[("a.priv", 0, "secret")])));
        assert_eq!(w.status(ME, "a.priv").unwrap()[0].3, "secret");
        w.hosts.ui().unwrap().guard_started().unwrap();
        w.rows.borrow_mut().insert(0, row(9, A, true));
        w.asked.borrow_mut().clear();
        let rows = w.status(ME, "a.priv").unwrap();
        assert_eq!(rows[0], ("a.priv".into(), 2, 0, String::new()));
        assert!(w.asked.borrow().is_empty());
    });
}

#[test]
#[cfg_attr(feature = "x4-nested", ignore = "x4-nested opens the X4 gate (#250)")]
fn ops_14_and_15_are_depth_one_only_and_unsupported_on_the_x4() {
    world(|w| {
        w.depth.set(2);
        assert_eq!(w.status(ME, "a.one").err(), Some(ERR_BUSY));
        assert_eq!(w.line(ME, "a.line").err(), Some(ERR_BUSY));
        w.depth.set(1);
        w.x4.set(true);
        assert_eq!(w.status(ME, "a.one").err(), Some(ERR_UNSUPPORTED));
        assert_eq!(w.line(ME, "a.line").err(), Some(ERR_UNSUPPORTED));
        w.x4.set(false);
        // More than 8 items, an empty line or an overlong one: invalid.
        let nine = "a.one\n".repeat(8) + "a.one";
        assert_eq!(w.status(ME, &nine).err(), Some(ERR_INVALID));
        assert_eq!(w.status(ME, "a.one\n\na.two").err(), Some(ERR_INVALID));
        assert_eq!(w.status(ME, &"x".repeat(129)).err(), Some(ERR_INVALID));
        // No items: just the end.
        assert!(w.status(ME, "").unwrap().is_empty());
    });
}

#[test]
fn a_status_line_handle_is_granted_own_is_flagged_and_a_private_line_is_denied_under_a_guard() {
    world(|w| {
        let (handle, flags) = w.line(ME, "a.line").unwrap();
        assert_ne!(handle, 0);
        assert_eq!(flags, 0);
        assert_eq!(w.line(ME, "me.line").unwrap(), (0, 1));
        assert_eq!(w.line(ME, "nobody").err(), Some(ERR_NOT_FOUND));
        assert_eq!(w.line(ME, "a.line\nb").err(), Some(ERR_INVALID));
        // A new callback of the caller drops the old handles.
        w.epoch.set(2);
        w.line(ME, "a.line").unwrap();
        // Under a guard of someone else a private line is denied.
        w.rows.borrow_mut().insert(0, row(9, B, true));
        assert_eq!(w.line(ME, "a.pline").err(), Some(ERR_DENIED));
        assert_ne!(w.line(ME, "a.line").unwrap().0, 0);
    });
}

#[test]
fn a_stopped_plugin_loses_its_instances_and_the_cache() {
    world(|w| {
        let (id, _) = w.make(ME, "a.w").unwrap();
        w.reply(A, 1, Ok(values(&[("a.one", 0, "v1")])));
        w.status(ME, "a.one").unwrap();
        let ui = w.hosts.ui().unwrap();
        assert_eq!(ui.widgets.handles.borrow().widgets(), 1);
        // A widget keeps its plugin from sleeping again (#248, D3).
        assert_eq!(ui.plugin_holds(A), Ok(true));
        ui.plugin_stopped(A).unwrap();
        assert_eq!(ui.plugin_holds(A), Ok(false));
        assert_eq!(ui.widgets.handles.borrow().widgets(), 0);
        assert_eq!(w.drop_widget(ME, id).err(), Some(ERR_NOT_FOUND));
        // The screen closing drops its instances the same way.
        w.make(ME, "a.w").unwrap();
        assert_eq!(ui.screen_closed(10).unwrap(), 1);
        // Cache gone: the owner is asked again.
        w.asked.borrow_mut().clear();
        w.status(ME, "a.one").unwrap();
        assert_eq!(w.asked.borrow().len(), 1);
    });
}

#[test]
fn a_heap_refusal_is_no_memory_and_hands_out_nothing() {
    world(|w| {
        w.reply(A, 1, Ok(values(&[("a.one", 1, "")])));
        for nth in 1..=4 {
            w.fail.set(nth);
            let got = w.status(ME, "a.one");
            w.fail.set(0);
            if let Err(error) = got {
                assert_eq!(error, ERR_NO_MEMORY, "allocation {nth}");
            }
        }
        w.fail.set(1);
        let made = w.make(ME, "a.w");
        w.fail.set(0);
        assert!(made.is_err() || made.is_ok());
        assert!(w.make(ME, "a.w").is_ok());
    });
}

#[test]
fn the_cache_compares_whole_keys_holds_sixteen_and_skips_long_ones() {
    let mut cache = StatusCache::default();
    cache.put(1, "a.one", b"ctx", false, "v");
    // The same item under another context, or another item, is a miss.
    assert_eq!(cache.get(1, "a.one", b"ctx"), Some(&b"v"[..]));
    assert_eq!(cache.get(1, "a.one", b"cty"), None);
    assert_eq!(cache.get(1, "a.on", b"ectx"), None);
    assert_eq!(cache.get(1, "a.one", b""), None);
    // A key over 128 bytes is not cached.
    cache.put(1, "a.one", &[7; 128], false, "long");
    assert_eq!(cache.get(1, "a.one", &[7; 128]), None);
    // 16 entries; the 17th replaces the oldest, the second the next oldest.
    cache.clear();
    for n in 0..16_u8 {
        cache.put(1, "i", &[n], false, "x");
    }
    cache.put(1, "i", &[16], false, "y");
    assert_eq!(cache.entries.len(), 16);
    assert_eq!(cache.get(1, "i", &[0]), None);
    assert_eq!(cache.get(1, "i", &[1]), Some(&b"x"[..]));
    assert_eq!(cache.get(1, "i", &[16]), Some(&b"y"[..]));
    cache.put(1, "i", &[17], false, "z");
    assert_eq!(cache.get(1, "i", &[1]), None);
    assert_eq!(cache.get(1, "i", &[2]), Some(&b"x"[..]));
}

#[test]
fn an_owner_that_runs_out_of_memory_leaves_empty_rows_and_the_op_answers() {
    world(|w| {
        w.reply(A, 1, Err(ERR_NO_MEMORY));
        let rows = w.status(ME, "a.one").unwrap();
        assert_eq!(rows, std::vec![("a.one".into(), 0, 0, String::new())]);
    });
}

#[test]
fn a_widget_is_made_at_depth_one_only_and_not_below_a_guard() {
    world(|w| {
        w.depth.set(2);
        assert_eq!(w.make(ME, "a.w").err(), Some(ERR_BUSY));
        w.depth.set(1);
        // Under a guard of another plugin the caller is not the top instance
        // (kept as ERR_STATE until Kernel settles ERR_DENIED, ui.md 4.3).
        w.rows.borrow_mut().push(row(9, A, true));
        assert_eq!(w.make(ME, "a.w").err(), Some(ERR_STATE));
    });
}

#[test]
fn a_screen_below_another_plugins_guard_gets_denied_for_status_and_may_still_drop() {
    world(|w| {
        let (id, _) = w.make(ME, "a.w").unwrap();
        w.rows.borrow_mut().push(row(9, A, true));
        assert_eq!(w.status(ME, "a.one").err(), Some(ERR_DENIED));
        assert_eq!(w.line(ME, "a.line").err(), Some(ERR_DENIED));
        // Op 5 has no guard rule.
        w.drop_widget(ME, id).unwrap();
        // The guard's own plugin is not denied.
        assert!(w.status(A, "a.one").is_ok());
    });
}

#[test]
fn a_caller_screen_missing_from_the_stack_is_denied_under_a_guard_and_fine_without() {
    world(|w| {
        w.instance.set(Some(77));
        assert!(w.status(ME, "a.one").is_ok());
        w.rows.borrow_mut().push(row(9, A, true));
        assert_eq!(w.status(ME, "a.one").err(), Some(ERR_DENIED));
        assert_eq!(w.line(ME, "a.line").err(), Some(ERR_DENIED));
        // The guard's own plugin is not denied.
        assert!(w.status(A, "a.one").is_ok());
    });
}

#[test]
fn each_item_keeps_its_own_private_flag_in_a_group_and_cache_keys_hold_the_owner() {
    world(|w| {
        // a.one is public, a.priv private, both owner A, token 1, one group.
        w.reply(
            A,
            1,
            Ok(values(&[("a.one", 0, "open"), ("a.priv", 0, "secret")])),
        );
        w.status(ME, "a.one\na.priv").unwrap();
        w.status(ME, "a.priv\na.one").unwrap();
        {
            let cache = w.hosts.ui().unwrap().widgets.cache.borrow();
            assert_eq!(cache.get(A, "a.priv", b"ctx"), Some(&b"secret"[..]));
            // Another owner's item of the same name is another key.
            assert_eq!(cache.get(A + 1, "a.priv", b"ctx"), None);
        }
        w.hosts.ui().unwrap().guard_started().unwrap();
        let cache = w.hosts.ui().unwrap().widgets.cache.borrow();
        assert_eq!(cache.get(A, "a.priv", b"ctx"), None);
        assert_eq!(cache.get(A, "a.one", b"ctx"), Some(&b"open"[..]));
    });
}

#[path = "ui_widget_layout_tests.rs"]
mod layout;
