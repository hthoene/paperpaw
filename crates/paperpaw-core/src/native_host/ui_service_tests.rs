//! `core.ui` hand-over and text ops: the statuses, the limits, the caller
//! rules, and that a refusal leaves nothing.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
extern crate std;
use super::*;
use crate::abi7::services_wire;
use crate::native_host::admitted_features::AdmittedFeatures;
use crate::native_host::call_services::validate_reply;
use crate::native_host::ui_store::Limits;
use alloc::string::String;
use core::cell::Cell;
use paperpaw_i18n::Translator;
use paperpaw_native::host_glue::{ERR_FULL, ERR_NOT_FOUND};
use paperpaw_platform::Instant;
use paperpaw_sdk::{Env, Settings};

struct Stack {
    epoch: Cell<u64>,
    instance: Cell<Option<u32>>,
}
impl UiHost for Stack {
    fn callback_epoch(&self) -> u64 {
        self.epoch.get()
    }
    fn calling_instance(&self, _: u64) -> Option<u32> {
        self.instance.get()
    }
}

struct View<'a> {
    env: &'a Env<'a>,
    hosts: crate::native_host::host_services::HostServices,
    store: Rc<UiStore>,
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
    /// What Core's stack does with a made hand-over: name it and claim it.
    fn open_with(&self, owner: u64, id: u32, instance: u32) {
        let stamp = Stamp::new(owner, self.stack.epoch.get()).unwrap();
        self.store.check(stamp, id).unwrap();
        self.store.claim(id, instance).unwrap();
    }
}

fn with_view<R>(limits: Limits, wire_it: bool, f: impl FnOnce(&View<'_>) -> R) -> R {
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(Instant(0));
    let store = Rc::new(UiStore::new(limits));
    let stack = Rc::new(Stack {
        epoch: Cell::new(1),
        instance: Cell::new(None),
    });
    let hosts = crate::native_host::host_services::HostServices::new();
    if wire_it {
        hosts
            .wire_ui(UiServices {
                store: store.clone(),
                host: stack.clone(),
            })
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
        for op in [1, 2, 3, 4, 5, 6, 7, 8, 11, 12, 13, 14, 15] {
            let op = schema.op(op).unwrap();
            assert_eq!(
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
