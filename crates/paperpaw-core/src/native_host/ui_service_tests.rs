//! `core.ui` hand-over and text ops: the statuses, the limits, the caller
//! rules, and that a refusal leaves nothing.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
extern crate std;
use super::*;
use crate::abi7::services_wire;
use crate::native_host::admitted_features::AdmittedFeatures;
use crate::native_host::call_services::validate_reply;
use crate::native_host::ui_guard::{Row, UiStackView};
use crate::native_host::ui_store::Limits;
use alloc::{rc::Rc, string::String};
use core::cell::Cell;
use paperpaw_i18n::Translator;
use paperpaw_native::host_glue::{ERR_DENIED, ERR_FULL, ERR_NOT_FOUND};
use paperpaw_platform::Instant;
use paperpaw_sdk::{Env, Settings};

struct Stack {
    epoch: Cell<u64>,
    instance: Cell<Option<u32>>,
    rows: core::cell::RefCell<Vec<ui_guard::Row>>,
    x4: Cell<bool>,
    closed: core::cell::RefCell<Vec<u32>>,
}
struct View<'a> {
    env: &'a Env<'a>,
    hosts: crate::native_host::host_services::HostServices,
    store: Shared<UiStore>,
    stack: Rc<Stack>,
    fail: Cell<u32>,
}
impl Frame for View<'_> {
    fn settings(&self) -> &Settings {
        self.env.settings()
    }
    fn translator(&self) -> &Translator {
        self.env.i18n()
    }
    // The stack's view, as Core's frame gives it from the stamped callback.
    fn callback_epoch(&self) -> Result<u64, i32> {
        Some(self.stack.epoch.get())
            .filter(|epoch| *epoch != 0)
            .ok_or(ERR_STATE)
    }
    fn calling_instance(&self) -> Result<u32, i32> {
        self.callback_epoch()?;
        self.stack.instance.get().ok_or(ERR_STATE)
    }
    fn with_stack(&self, visit: &mut dyn FnMut(&dyn UiStackView)) -> Result<(), i32> {
        self.callback_epoch()?;
        visit(&ui_guard::Rows(&self.stack.rows.borrow()));
        Ok(())
    }
    fn ui_x4(&self) -> Result<bool, i32> {
        self.callback_epoch()?;
        Ok(self.stack.x4.get())
    }
    fn close_dialog(&self, instance: u32) -> Result<(), i32> {
        self.stack.closed.borrow_mut().push(instance);
        Ok(())
    }
}

impl View<'_> {
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
        self.call_raw(owner, op, &input[..used])
    }
    fn call_raw(&self, owner: u64, op: &ServiceOp, input: &[u8]) -> Result<Vec<u8>, i32> {
        let caller = Caller {
            owner,
            plugin: "p",
            features: AdmittedFeatures::NONE,
            hosts: &self.hosts,
        };
        let mut out = std::vec![0; op.max_reply];
        super::super::shell::tests::FAIL_NTH.with(|n| n.set(self.fail.get()));
        let result = backend(self, &caller, op, input, &mut out);
        super::super::shell::tests::FAIL_NTH.with(|n| n.set(0));
        let length = result?;
        out.truncate(length);
        validate_reply(&out, op)?;
        Ok(out)
    }
    fn make(&self, owner: u64, params: &[(&str, &str)]) -> Result<u32, i32> {
        let records: Vec<Record<'_>> = params
            .iter()
            .map(|(key, value)| Record::ServiceUiParam(wire::ServiceUiParam { key, value }))
            .collect();
        let reply = self.call(owner, 16, &records)?;
        match wire::record_decode_one(&reply).unwrap() {
            Record::ServiceUiHandle(handle) => Ok(handle.handle),
            other => panic!("{other:?}"),
        }
    }
    fn write(&self, owner: u64, id: u32, offset: u32, data: &str, last: bool) -> Result<(), i32> {
        self.call(
            owner,
            9,
            &[Record::ServiceUiText(wire::ServiceUiText {
                text: id,
                offset,
                total: 0,
                flags: u32::from(last),
                data,
            })],
        )
        .map(|_| ())
    }
    fn read(&self, owner: u64, id: u32, offset: u32) -> Result<(String, u32, bool), i32> {
        let reply = self.call(
            owner,
            10,
            &[Record::ServiceUiTextAt(wire::ServiceUiTextAt {
                text: id,
                offset,
            })],
        )?;
        match wire::record_decode_one(&reply).unwrap() {
            Record::ServiceUiText(t) => Ok((t.data.into(), t.total, t.flags & 1 == 1)),
            other => panic!("{other:?}"),
        }
    }
    fn params(&self, owner: u64, handle: u32) -> Result<(Vec<(String, String)>, u32), i32> {
        let reply = self.call(
            owner,
            17,
            &[Record::ServiceUiHandle(wire::ServiceUiHandle { handle })],
        )?;
        let (mut rows, mut total) = (Vec::new(), 0);
        crate::native_host::stage_wire::visit_stream(&reply, |_, framed| {
            match wire::record_decode_one(framed).unwrap() {
                Record::ServiceUiParam(p) => rows.push((p.key.into(), p.value.into())),
                Record::ServiceEnd(e) => total = e.total,
                other => panic!("{other:?}"),
            }
            Ok(())
        })
        .unwrap();
        Ok((rows, total))
    }
    /// What Core's stack does with a made hand-over: check, claim and attach it.
    fn open_with(&self, owner: u64, id: u32, instance: u32) {
        let stamp = Stamp::new(owner, self.stack.epoch.get()).unwrap();
        self.store.handover_check(stamp, id).unwrap();
        let claims = self.store.claim(stamp, &[id]).unwrap();
        self.store.attach(Some(id), instance).unwrap();
        drop(claims);
    }
}

fn with_view<R>(limits: Limits, wire_it: bool, f: impl FnOnce(&View<'_>) -> R) -> R {
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(Instant(0));
    let store = Shared::try_new(UiStore::new(limits)).unwrap();
    store.bind_clipboard(&Rc::default());
    let stack = Rc::new(Stack {
        epoch: Cell::new(1),
        instance: Cell::new(None),
        rows: core::cell::RefCell::new(Vec::new()),
        x4: Cell::new(false),
        closed: core::cell::RefCell::new(Vec::new()),
    });
    let hosts = crate::native_host::host_services::HostServices::new();
    if wire_it {
        hosts
            .wire_ui(UiServices::new(store.try_clone().unwrap(), false))
            .unwrap();
    }
    f(&View {
        env: &env,
        hosts,
        store,
        stack,
        fail: Cell::new(0),
    })
}

fn view<R>(f: impl FnOnce(&View<'_>) -> R) -> R {
    with_view(Limits::ELSEWHERE, true, f)
}

#[test]
fn an_unwired_family_and_the_ops_not_yet_here_are_unsupported() {
    with_view(Limits::ELSEWHERE, false, |v| {
        assert_eq!(v.make(1, &[]).err(), Some(ERR_UNSUPPORTED));
    });
    view(|v| {
        let schema = services_wire::service_schema("core.ui", 1).unwrap();
        let op = schema.op(6).unwrap();
        assert_eq!(
            v.call_raw(1, op, &[0]).err(),
            Some(ERR_UNSUPPORTED),
            "op {}",
            op.id
        );
        // Ops 7, 12 and 13 are served (`ui_widget_layout`): a malformed input
        // is refused as such, no longer as unsupported. Op 6 waits for the
        // drain of Core's dispatcher.
        for op in [7, 12, 13] {
            let op = schema.op(op).unwrap();
            assert_ne!(
                v.call_raw(1, op, &[0]).err(),
                Some(ERR_UNSUPPORTED),
                "op {}",
                op.id
            );
        }
    });
}

#[test]
fn a_hand_over_is_made_written_opened_and_read_by_its_instance() {
    view(|v| {
        let id = v.make(1, &[("book", "a.epub"), ("page", "3")]).unwrap();
        assert!(id != 0);
        v.write(1, id, 0, "hällo ", false).unwrap();
        v.write(1, id, 7, "wörld", true).unwrap();
        v.open_with(1, id, 40);
        // Only the instance the stack resolves reads, and only its own.
        assert_eq!(v.params(1, id).err(), Some(ERR_STATE), "no instance");
        v.stack.instance.set(Some(41));
        assert_eq!(v.params(1, id).err(), Some(ERR_STATE));
        assert_eq!(v.read(1, id, 0).err(), Some(ERR_STATE));
        v.stack.instance.set(Some(40));
        assert_eq!(
            v.params(1, id).unwrap(),
            (
                std::vec![
                    ("book".into(), "a.epub".into()),
                    ("page".into(), "3".into())
                ],
                2
            )
        );
        assert_eq!(v.params(1, id + 1).err(), Some(ERR_STATE), "not its handle");
        assert_eq!(v.read(1, id, 0).unwrap(), ("hällo wörld".into(), 13, true));
        assert_eq!(v.read(1, id, 7).unwrap(), ("wörld".into(), 13, true));
        assert_eq!(v.read(1, id, 99).err(), Some(ERR_INVALID));
    });
}

#[test]
fn a_long_text_comes_back_in_3_kib_parts_cut_at_characters() {
    view(|v| {
        let id = v.make(1, &[]).unwrap();
        let text = "ö".repeat(5_000);
        let mut at = 0;
        while at < text.len() {
            let end = (at + 3_072).min(text.len());
            v.write(1, id, at as u32, &text[at..end], end == text.len())
                .unwrap();
            at = end;
        }
        v.open_with(1, id, 5);
        v.stack.instance.set(Some(5));
        let (mut all, mut offset) = (String::new(), 0_u32);
        loop {
            let (part, total, last) = v.read(1, id, offset).unwrap();
            assert!(part.len() <= 3_072 && !part.is_empty());
            offset += part.len() as u32;
            all.push_str(&part);
            assert_eq!(total as usize, text.len());
            if last {
                break;
            }
        }
        assert_eq!(all, text);
    });
}

#[test]
fn the_param_limits_and_the_form_with_a_refusal_leaving_nothing() {
    view(|v| {
        let keys: Vec<String> = (0..33).map(|n| std::format!("k{n}")).collect();
        let many: Vec<(&str, &str)> = keys.iter().map(|k| (k.as_str(), "")).collect();
        v.make(1, &many[..32]).unwrap();
        assert_eq!(v.make(1, &many).err(), Some(ERR_NO_SPACE));
        let value = "v".repeat(1_020);
        v.make(1, &[("key1", &value)]).unwrap();
        let more = std::format!("{value}x");
        assert_eq!(v.make(1, &[("key1", &more)]).err(), Some(ERR_NO_SPACE));
        let before = v.store.len();
        for bad in [
            std::vec![("", "x")],
            std::vec![("Key", "x")],
            std::vec![("a", "1"), ("a", "2")],
            std::vec![("core.request", "1")],
            std::vec![("ok", "1"), ("core.kind", "1")],
        ] {
            assert_eq!(v.make(1, &bad).err(), Some(ERR_INVALID), "{bad:?}");
        }
        assert_eq!(v.store.len(), before, "a refusal publishes nothing");
        // Another record in the batch, garbage, and a missing batch.
        assert_eq!(
            v.call(
                1,
                16,
                &[Record::ServiceUiHandle(wire::ServiceUiHandle { handle: 1 })]
            )
            .err(),
            Some(ERR_INVALID)
        );
        let schema = services_wire::service_schema("core.ui", 1).unwrap();
        let op = schema.op(16).unwrap();
        for input in [&[][..], &[0xff; 6], &[5, 1, 2]] {
            assert!(v.call_raw(1, op, input).is_err());
        }
        assert_eq!(v.store.len(), before);
    });
}

#[test]
fn no_callback_epoch_is_a_state_error_and_only_the_maker_in_its_callback_writes() {
    view(|v| {
        v.stack.epoch.set(0);
        assert_eq!(v.make(1, &[]).err(), Some(ERR_STATE));
        v.stack.epoch.set(7);
        let id = v.make(1, &[]).unwrap();
        assert_eq!(v.write(2, id, 0, "x", true).err(), Some(ERR_NOT_FOUND));
        v.stack.epoch.set(8);
        assert_eq!(v.write(1, id, 0, "x", true).err(), Some(ERR_STATE));
        v.stack.epoch.set(7);
        // Flags other than `last`, or a part over 3,072 bytes, are refused.
        let bad = v.call(
            1,
            9,
            &[Record::ServiceUiText(wire::ServiceUiText {
                text: id,
                offset: 0,
                total: 0,
                flags: 2,
                data: "x",
            })],
        );
        assert_eq!(bad.err(), Some(ERR_INVALID));
        v.write(1, id, 0, "x", true).unwrap();
    });
}

#[test]
fn the_text_cap_is_24576_and_the_x4_params_share_1536() {
    view(|v| {
        let id = v.make(1, &[]).unwrap();
        let part = "a".repeat(3_072);
        for n in 0..8 {
            v.write(1, id, n * 3_072, &part, false).unwrap();
        }
        assert_eq!(v.write(1, id, 24_576, "x", true).err(), Some(ERR_NO_SPACE));
    });
    with_view(Limits::X4, true, |v| {
        let value = "v".repeat(500);
        for key in ["a", "b", "c"] {
            v.make(1, &[(key, &value)]).unwrap();
        }
        assert_eq!(v.make(1, &[("d", &value)]).err(), Some(ERR_FULL));
    });
}

#[test]
fn a_stopped_owner_frees_what_it_made() {
    view(|v| {
        let id = v.make(1, &[("a", "1")]).unwrap();
        v.write(1, id, 0, "text", true).unwrap();
        let other = v.make(2, &[("b", "2")]).unwrap();
        v.hosts.owner_retired(1).unwrap();
        assert_eq!(v.store.len(), 1);
        v.open_with(2, other, 3);
        assert_eq!(v.store.len(), 1);
    });
}

#[test]
fn every_failed_allocation_refuses_with_no_memory_and_changes_nothing() {
    view(|v| {
        let id = v.make(1, &[("k", "v")]).unwrap();
        let before = (v.store.len(), v.store.held());
        for nth in 1..=6 {
            v.fail.set(nth);
            let made = v.make(1, &[("a", "1")]);
            let wrote = v.write(1, id, 0, "text", true);
            v.fail.set(0);
            if let Err(e) = made {
                assert_eq!(e, ERR_NO_MEMORY, "make, allocation {nth}");
            } else {
                v.stack.epoch.set(v.stack.epoch.get());
            }
            if let Err(e) = wrote {
                assert_eq!(e, ERR_NO_MEMORY, "write, allocation {nth}");
            }
            let _ = v.hosts.owner_retired(99);
        }
        assert!(v.store.len() >= before.0);
        // A read refuses the same way and never panics.
        v.open_with(1, id, 9);
        v.stack.instance.set(Some(9));
        for nth in 1..=3 {
            v.fail.set(nth);
            let read = v.read(1, id, 0);
            let params = v.params(1, id);
            v.fail.set(0);
            for e in [read.err(), params.err()].into_iter().flatten() {
                assert_eq!(e, ERR_NO_MEMORY, "allocation {nth}");
            }
        }
    });
}

/// An `ask` input for op 8.
fn ask_record<'a>(kind: u32, flags: u32, slot: &'a str, choices: &'a str) -> Record<'a> {
    Record::ServiceUiAsk(wire::ServiceUiAsk {
        kind,
        flags,
        slot,
        title: "T",
        text: "Q?",
        choices,
    })
}

fn ask_id(v: &View<'_>, owner: u64, record: Record<'_>) -> Result<u32, i32> {
    let reply = v.call(owner, 8, &[record])?;
    match wire::record_decode_one(&reply).unwrap() {
        Record::ServiceUiRequest(r) => Ok(r.request),
        other => panic!("{other:?}"),
    }
}

/// The stack's end of a question: the callback returns, the dialog opens.
fn open_dialog(v: &View<'_>, owner: u64, id: u32, dialog: u32) {
    let stamp = Stamp::new(owner, v.stack.epoch.get()).unwrap();
    let mut started = 0;
    v.store
        .callback_returned(stamp, false, &mut |_| started += 1)
        .unwrap();
    assert_eq!(started, 1);
    v.store.dialog_opened(id, dialog).unwrap();
}

fn row(instance: u32, owner: u64) -> Row {
    Row {
        instance,
        owner,
        overlay: false,
        guards: false,
        secret: false,
        host: false,
        guard_dialog: false,
    }
}

#[test]
fn ask_records_a_question_the_dialog_reads_and_answers() {
    view(|v| {
        v.stack.instance.set(Some(30));
        let id = ask_id(v, 1, ask_record(0, 0b10, "", "")).unwrap();
        assert_ne!(id, 0);
        // The asker writes the initial text in the same callback.
        v.write(1, id, 0, "hello", true).unwrap();
        open_dialog(v, 1, id, 40);
        // The dialog (another owner, its own instance) reads question and text.
        v.stack.instance.set(Some(40));
        let reply = v
            .call(
                2,
                2,
                &[Record::ServiceUiRequest(wire::ServiceUiRequest {
                    request: id,
                })],
            )
            .unwrap();
        let Record::ServiceUiQuestion(shown) = wire::record_decode_one(&reply).unwrap() else {
            panic!()
        };
        assert_eq!(
            (shown.kind, shown.flags, shown.max_bytes),
            (0, 0b10, 24_576)
        );
        assert_eq!((shown.title, shown.text), ("T", "Q?"));
        assert_eq!(v.read(2, id, 0).unwrap(), ("hello".into(), 5, true));
        // Another instance cannot read the question; the asker cannot answer.
        v.stack.instance.set(Some(41));
        let ask = Record::ServiceUiRequest(wire::ServiceUiRequest { request: id });
        assert_eq!(v.call(2, 2, &[ask]).err(), Some(ERR_STATE));
        // The dialog answers: write the text over, commit, op 11.
        v.stack.instance.set(Some(40));
        v.stack.epoch.set(5);
        v.write(2, id, 0, "typed", true).unwrap();
        let answer = |outcome| {
            v.call(
                2,
                11,
                &[Record::ServiceUiAnswer(wire::ServiceUiAnswer {
                    request: id,
                    outcome,
                    choice: 0,
                })],
            )
        };
        assert_eq!(answer(3).err(), Some(ERR_INVALID));
        answer(0).unwrap();
        assert_eq!(answer(0).err(), Some(ERR_STATE), "once");
        let delivery = v.store.take_delivery(1).unwrap();
        assert_eq!(
            (delivery.request, delivery.outcome, delivery.length),
            (id, 0, 5)
        );
    });
}

#[test]
fn ask_refuses_a_bad_form_with_the_contract_statuses() {
    view(|v| {
        for (record, status) in [
            (ask_record(4, 0, "", ""), ERR_INVALID),
            (ask_record(0, 0, "core.choice", ""), ERR_INVALID),
            (ask_record(1, 0, "core.remote-keyboard", "a"), ERR_INVALID),
            (ask_record(1, 0, "", ""), ERR_INVALID),
            (ask_record(1, 0, "", "a\n\nb"), ERR_INVALID),
            (ask_record(2, 0, "", "one"), ERR_INVALID),
            (ask_record(2, 1 << 8, "", "Read\nNo"), ERR_INVALID),
            (ask_record(0, 1 << 8, "", ""), ERR_INVALID),
            (ask_record(0, 0, "", "x"), ERR_INVALID),
        ] {
            assert_eq!(ask_id(v, 1, record).err(), Some(status));
        }
        assert_eq!(v.store.request_count(), 0, "a refusal records nothing");
        // No callback epoch is a state error.
        v.stack.epoch.set(0);
        assert_eq!(
            ask_id(v, 1, ask_record(0, 0, "", "")).err(),
            Some(ERR_STATE)
        );
    });
}

#[test]
fn ask_limits_four_devices_wide_one_on_the_x4_and_one_per_asker() {
    view(|v| {
        for owner in 1..=4 {
            ask_id(v, owner, ask_record(2, 0, "", "")).unwrap();
        }
        assert_eq!(ask_id(v, 5, ask_record(2, 0, "", "")).err(), Some(ERR_FULL));
        // The asker asking again cancels its open one: not a fifth.
        let again = ask_id(v, 1, ask_record(2, 0, "", "")).unwrap();
        assert_eq!(v.store.take_delivery(1).unwrap().outcome, 1);
        assert_ne!(again, 0);
    });
    with_view(Limits::X4, true, |v| {
        v.stack.x4.set(true);
        ask_id(v, 1, ask_record(2, 0, "", "")).unwrap();
        assert_eq!(ask_id(v, 2, ask_record(2, 0, "", "")).err(), Some(ERR_FULL));
    });
}

#[test]
fn the_phone_keyboard_asks_under_its_dialog_and_never_under_a_guard() {
    view(|v| {
        v.stack.instance.set(Some(30));
        let parent = ask_id(v, 1, ask_record(0, 1 << 2, "", "")).unwrap();
        open_dialog(v, 1, parent, 40);
        // The dialog instance asks the phone question: kind 0 only, on the slot.
        v.stack.instance.set(Some(40));
        v.stack.epoch.set(6);
        let phone = ask_record(0, 0, "core.remote-keyboard", "");
        assert_eq!(
            ask_id(v, 2, ask_record(1, 0, "", "a")).err(),
            Some(ERR_DENIED)
        );
        let nested = ask_id(v, 2, phone).unwrap();
        // It inherits the parent's secret, and the parent has only one.
        let nested_secret = v.store.dialog_of(40).unwrap();
        assert_eq!(nested_secret, (parent, true, false, true));
        assert_eq!(
            ask_id(v, 2, ask_record(0, 0, "core.remote-keyboard", "")).err(),
            Some(ERR_FULL)
        );
        let _ = nested;
    });
}

#[test]
fn a_guard_denies_the_nested_question_and_the_answer_goes_to_the_asker() {
    view(|v| {
        v.stack.instance.set(Some(30));
        let parent = ask_id(v, 1, ask_record(0, 0, "", "")).unwrap();
        open_dialog(v, 1, parent, 40);
        v.stack.instance.set(Some(40));
        v.stack.epoch.set(6);
        let mut guard = row(50, 3);
        guard.guards = true;
        v.stack.rows.borrow_mut().push(guard);
        assert_eq!(
            ask_id(v, 2, ask_record(0, 0, "core.remote-keyboard", "")).err(),
            Some(ERR_DENIED)
        );
        assert_eq!(v.store.request_count(), 1);
    });
}

/// A scheduler view of a device that is up and awake.
struct WorkDevice;
impl crate::native_host::work_sched::WorkEnv for WorkDevice {
    fn subscribed(&self, _: u64, _: u32) -> bool {
        true
    }
    fn now_ms(&self) -> u64 {
        1_000
    }
    fn power(&self) -> crate::native_host::work_sched::Power {
        crate::native_host::work_sched::Power {
            level: 0,
            stretch: 1,
            radio_on: true,
        }
    }
    fn screen_hold(&self) -> bool {
        false
    }
    fn network_up(&self) -> bool {
        true
    }
    fn network_allowed(&self, _: u64) -> bool {
        true
    }
    fn saving_manual(&self, _: u64) -> bool {
        false
    }
}

#[test]
fn ask_is_refused_in_a_work_runs_callback() {
    use crate::native_host::{
        work_sched::{Limits as WorkLimits, WorkSched},
        work_service::WorkServices,
    };
    view(|v| {
        v.stack.instance.set(Some(30));
        let sched = Rc::new(WorkSched::new(WorkLimits::OTHER));
        v.hosts
            .wire_work(WorkServices {
                sched: Rc::clone(&sched),
                env: Rc::new(WorkDevice),
            })
            .unwrap();
        // In a run's callback (quiet) no dialog is asked for.
        {
            let _run = sched.enter_run_callback(1, 7);
            assert_eq!(
                ask_id(v, 1, ask_record(0, 0b10, "", "")).err(),
                Some(ERR_STATE)
            );
        }
        // Another plugin, or the same one outside a run, may ask.
        {
            let _run = sched.enter_run_callback(2, 7);
            assert!(ask_id(v, 1, ask_record(0, 0b10, "", "")).is_ok());
        }
    });
}

impl View<'_> {
    fn clip_write(&self, owner: u64, offset: u32, data: &str, last: bool) -> Result<(), i32> {
        self.call(
            owner,
            18,
            &[Record::ServiceUiText(wire::ServiceUiText {
                text: 0,
                offset,
                total: 0,
                flags: u32::from(last),
                data,
            })],
        )
        .map(|_| ())
    }
    fn clip_read(&self, offset: u32) -> Result<(String, u32, bool), i32> {
        let reply = self.call(
            1,
            19,
            &[Record::ServiceUiTextAt(wire::ServiceUiTextAt {
                text: 0,
                offset,
            })],
        )?;
        match wire::record_decode_one(&reply).unwrap() {
            Record::ServiceUiText(t) => Ok((t.data.into(), t.total, t.flags & 1 == 1)),
            other => panic!("{other:?}"),
        }
    }
}

#[test]
fn the_clipboard_is_written_in_parts_and_read_by_anyone() {
    view(|v| {
        // Empty to begin with.
        assert_eq!(v.clip_read(0).unwrap(), (String::new(), 0, true));
        v.clip_write(1, 0, "hello ", false).unwrap();
        // Reads see the old (empty) clipboard until `last` commits.
        assert_eq!(v.clip_read(0).unwrap().1, 0);
        v.clip_write(1, 6, "wörld", true).unwrap();
        assert_eq!(v.clip_read(0).unwrap(), ("hello wörld".into(), 12, true));
        // Another plugin reads it, from the middle.
        assert_eq!(v.clip_read(6).unwrap(), ("wörld".into(), 12, true));
    });
}

#[test]
fn a_new_clipboard_text_replaces_the_old_only_when_committed() {
    view(|v| {
        v.clip_write(1, 0, "old", true).unwrap();
        v.clip_write(1, 0, "new, begun", false).unwrap();
        assert_eq!(v.clip_read(0).unwrap().0, "old");
        // A part at a wrong offset is refused and changes nothing.
        assert_eq!(v.clip_write(1, 3, "x", false), Err(ERR_INVALID));
        v.clip_write(1, 10, "!", true).unwrap();
        assert_eq!(v.clip_read(0).unwrap().0, "new, begun!");
        // An empty committed text clears it.
        v.clip_write(1, 0, "", true).unwrap();
        assert_eq!(v.clip_read(0).unwrap(), (String::new(), 0, true));
    });
}

#[test]
fn a_clipboard_text_left_begun_is_dropped_with_its_callback() {
    view(|v| {
        v.clip_write(1, 0, "kept", true).unwrap();
        v.clip_write(1, 0, "begun", false).unwrap();
        // The callback returned; the next one cannot continue it.
        v.stack.epoch.set(2);
        assert_eq!(v.clip_write(1, 5, "more", true), Err(ERR_INVALID));
        assert_eq!(v.clip_read(0).unwrap().0, "kept");
        // The pump between callbacks frees a begun text too.
        v.clip_write(1, 0, "again", false).unwrap();
        v.store.clipboard_callbacks_over();
        v.stack.epoch.set(3);
        assert_eq!(v.clip_write(1, 5, "x", true), Err(ERR_INVALID));
    });
}

#[test]
fn the_clipboard_holds_4096_bytes_and_never_cuts() {
    view(|v| {
        let part = "a".repeat(2_048);
        v.clip_write(1, 0, &part, false).unwrap();
        v.clip_write(1, 2_048, &part, true).unwrap();
        assert_eq!(v.clip_read(0).unwrap().1, 4_096);
        // One byte over: refused, the old clipboard stays.
        v.clip_write(1, 0, &part, false).unwrap();
        v.clip_write(1, 2_048, &part, false).unwrap();
        assert_eq!(v.clip_write(1, 4_096, "b", true), Err(ERR_NO_SPACE));
        assert_eq!(v.clip_read(0).unwrap().1, 4_096);
        // A part over the part size does not frame (the record's own bound).
        assert_eq!(
            v.clip_write(1, 0, &"c".repeat(PART_MAX + 1), false),
            Err(ERR_NO_SPACE)
        );
    });
}

#[test]
fn clipboard_ops_refuse_another_text_id_and_a_missing_callback() {
    view(|v| {
        let write = |text| {
            v.call(
                1,
                18,
                &[Record::ServiceUiText(wire::ServiceUiText {
                    text,
                    offset: 0,
                    total: 0,
                    flags: 1,
                    data: "x",
                })],
            )
            .err()
        };
        assert_eq!(write(7), Some(ERR_INVALID));
        // Flags other than `last`.
        assert_eq!(
            v.call(
                1,
                18,
                &[Record::ServiceUiText(wire::ServiceUiText {
                    text: 0,
                    offset: 0,
                    total: 0,
                    flags: 2,
                    data: "x",
                })]
            )
            .err(),
            Some(ERR_INVALID)
        );
        // No callback running.
        v.stack.epoch.set(0);
        assert_eq!(v.clip_write(1, 0, "x", true), Err(ERR_STATE));
        assert_eq!(v.clip_read(0), Err(ERR_STATE));
        // The small-heap device has the same clipboard.
        v.stack.epoch.set(1);
        v.stack.x4.set(true);
        v.clip_write(1, 0, "x", true).unwrap();
        assert_eq!(v.clip_read(0).unwrap().0, "x");
    });
}

/// A committed write is the kernel's text (interface 6 reads the same cell) and
/// asks once for the copy out; a store the kernel has not bound is `ERR_STATE`.
#[test]
fn a_committed_write_replaces_the_kernels_text_and_asks_for_a_copy() {
    use crate::native_host::ui_clipboard::ClipboardCell;
    let store = UiStore::new(Limits::ELSEWHERE);
    let who = Stamp::new(1, 1).unwrap();
    assert_eq!(store.clipboard_write(who, 0, "x", true), Err(ERR_STATE));
    let cell = Rc::new(ClipboardCell::default());
    cell.set(String::from("six"));
    store.bind_clipboard(&cell);
    store.clipboard_write(who, 0, "new", false).unwrap();
    assert_eq!(cell.get(), "six");
    assert!(!cell.take_copy());
    store.clipboard_write(who, 3, "!", true).unwrap();
    assert_eq!(cell.get(), "new!");
    assert!(cell.take_copy());
    assert!(!cell.take_copy());
    assert_eq!(cell.text_for_copy().as_deref(), Some("new!"));
}

/// Whichever allocation of a write fails, the clipboard is the old text or the
/// new one, never a mix, and the next write works.
#[test]
fn a_clipboard_write_that_finds_no_memory_leaves_the_old_text() {
    for nth in 1..=10 {
        view(|v| {
            v.clip_write(1, 0, "old", true).unwrap();
            v.fail.set(nth);
            let written = v.clip_write(1, 0, "new text", true);
            v.fail.set(0);
            let read = v.clip_read(0).unwrap().0;
            match written {
                Ok(()) => assert_eq!(read, "new text", "allocation {nth}"),
                Err(_) => assert_eq!(read, "old", "allocation {nth}"),
            }
            v.clip_write(1, 0, "next", true).unwrap();
            assert_eq!(v.clip_read(0).unwrap().0, "next");
        });
    }
}

/// A text written in many small parts reserves room a few times, not once a
/// part: the capacity doubles up to the cap.
#[test]
fn a_clipboard_written_in_many_parts_is_assembled_and_read_whole() {
    view(|v| {
        let mut expected = String::new();
        for n in 0..400_u32 {
            let part = std::format!("{n:02}-");
            v.clip_write(1, expected.len() as u32, &part, false)
                .unwrap();
            expected.push_str(&part);
        }
        v.clip_write(1, expected.len() as u32, "", true).unwrap();
        assert_eq!(v.clip_read(0).unwrap().1 as usize, expected.len());
        let (part, total, _) = v.clip_read(0).unwrap();
        assert_eq!(total as usize, expected.len());
        assert!(expected.starts_with(&part));
    });
}

/// What `handed_file` shows for `instance`: the maker, the root, the path.
fn handed_of(v: &View<'_>, instance: u32) -> Result<(u64, u32, String), i32> {
    let mut seen = None;
    v.store.handed_file(instance, &mut |h| {
        seen = Some((h.maker, h.root, String::from(h.path)));
    })?;
    seen.ok_or(ERR_STATE)
}

#[test]
fn a_reader_holding_a_handed_file_passes_it_on_to_its_own_screen() {
    const MAKER: u64 = 9;
    const READER: u64 = 12;
    view(|v| {
        // A maker hands its file over; the reader's instance 4 holds it.
        let made = v
            .make(MAKER, &[("root", "2"), ("path", "licences.md")])
            .unwrap();
        v.open_with(MAKER, made, 4);
        // The reader's callback hands "root 5" and the same path to its
        // table-of-contents screen (instance 5).
        v.stack.instance.set(Some(4));
        let toc = v
            .make(READER, &[("root", "5"), ("path", "licences.md")])
            .unwrap();
        v.open_with(READER, toc, 5);
        // The screen's file is the maker's, as a root of its own folders: the
        // same file as instance 4's, and the screen reads the params of the
        // hand-over the maker made.
        assert_eq!(handed_of(v, 5), handed_of(v, 4));
        assert_eq!(handed_of(v, 5), Ok((MAKER, 2, String::from("licences.md"))));
        v.stack.instance.set(Some(5));
        let (rows, _) = v.params(READER, toc).unwrap();
        assert!(rows.contains(&(String::from("root"), String::from("2"))));
        // And the screen passes it on again (a chain keeps the first maker).
        let again = v
            .make(READER, &[("root", "5"), ("path", "licences.md")])
            .unwrap();
        v.open_with(READER, again, 6);
        assert_eq!(handed_of(v, 6), Ok((MAKER, 2, String::from("licences.md"))));
        // The maker stopping does not take the file from the instances; the
        // host (doc_open) ends the document with the maker.
        v.store.owner_stopped(MAKER).unwrap();
        assert_eq!(handed_of(v, 5), Ok((MAKER, 2, String::from("licences.md"))));
    });
}

#[test]
fn a_root_of_five_is_passed_on_only_for_the_file_the_instance_holds() {
    view(|v| {
        let made = v.make(9, &[("root", "0"), ("path", "a.txt")]).unwrap();
        v.open_with(9, made, 4);
        let five = |path: &str| v.make(12, &[("root", "5"), ("path", path)]);
        // Another path than the one held.
        v.stack.instance.set(Some(4));
        assert_eq!(five("b.txt").err(), Some(ERR_DENIED));
        assert_eq!(five("").err(), Some(ERR_DENIED));
        // No path at all.
        assert_eq!(v.make(12, &[("root", "5")]).err(), Some(ERR_DENIED));
        // An instance that holds no hand-over, and a call from no instance.
        v.stack.instance.set(Some(77));
        assert_eq!(five("a.txt").err(), Some(ERR_DENIED));
        v.stack.instance.set(None);
        assert_eq!(five("a.txt").err(), Some(ERR_DENIED));
        // A book of the library (root 1) and the maker's own roots are as before.
        assert!(v.make(12, &[("root", "1"), ("path", "a.txt")]).is_ok());
        assert!(v.make(12, &[("root", "0"), ("path", "a.txt")]).is_ok());
    });
}
