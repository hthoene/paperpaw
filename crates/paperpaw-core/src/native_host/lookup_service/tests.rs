#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
//! `core.lookup`: input rules, one provider per page, checks of its answer.
extern crate std;
use super::*;
use crate::abi7::services_wire;
use crate::native_host::call_services::validate_reply;
use alloc::vec::Vec;
use core::cell::{Cell, RefCell};
use paperpaw_i18n::Translator;
use paperpaw_native::host_glue::{ERR_NO_SPACE, ERR_NOT_FOUND};
use paperpaw_platform::Instant;
use paperpaw_sdk::{Env, Settings};
use wire::{ServiceLookupAsk as Ask, ServiceLookupEntry as Entry};

/// Three providers; `answer` overrides what the one at the cursor says.
struct Fake {
    asked: RefCell<Vec<(u64, u32, std::string::String, std::string::String)>>,
    answer: RefCell<Option<Vec<u8>>>,
    fail: Cell<Option<i32>>,
}
fn entry_bytes(provider: &str, flags: u32, text: &str) -> Vec<u8> {
    let mut out = std::vec![0; 4096];
    let used = wire::record_encode_one(
        &Record::ServiceLookupEntry(Entry {
            provider,
            title: "Word",
            text,
            flags,
            opens: "",
        }),
        &mut out,
    )
    .unwrap();
    out.truncate(used);
    out
}
impl LookupHost for Fake {
    fn ask(
        &self,
        _frame: &dyn Frame,
        owner: u64,
        ask: &Ask<'_>,
        out: &mut [u8],
    ) -> Result<usize, i32> {
        self.asked
            .borrow_mut()
            .push((owner, ask.cursor, ask.text.into(), ask.language.into()));
        if let Some(error) = self.fail.get() {
            return Err(error);
        }
        if ask.cursor >= 3 {
            return Err(ERR_NOT_FOUND);
        }
        let bytes = self.answer.borrow().clone().unwrap_or_else(|| {
            let more = u32::from(ask.cursor < 2);
            entry_bytes(&std::format!("dict{}", ask.cursor), more, "definition")
        });
        out[..bytes.len()].copy_from_slice(&bytes);
        Ok(bytes.len())
    }
}

struct View<'a> {
    env: &'a Env<'a>,
    hosts: crate::native_host::host_services::HostServices,
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
    fn run(&self, op: &ServiceOp, input: &[u8], out: &mut [u8]) -> Result<usize, i32> {
        let caller = Caller {
            owner: 7,
            plugin: "reader",
            features: crate::native_host::admitted_features::AdmittedFeatures::NONE,
            hosts: &self.hosts,
        };
        backend(self, &caller, op, input, out)
    }
}

fn fake() -> Rc<Fake> {
    Rc::new(Fake {
        asked: RefCell::new(Vec::new()),
        answer: RefCell::new(None),
        fail: Cell::new(None),
    })
}
fn ask(fake: &Rc<Fake>, record: Ask<'_>) -> Result<Vec<u8>, i32> {
    let op = services_wire::service_schema("core.lookup", 1)
        .unwrap()
        .op(1)
        .unwrap();
    let mut input = std::vec![0; 4096];
    let used = wire::records_encode_counted(
        &[Record::ServiceLookupAsk(record)],
        1,
        op.max_input,
        &mut input,
    )?;
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(Instant(0));
    let host: Rc<dyn LookupHost> = fake.clone();
    let hosts = crate::native_host::host_services::HostServices::new();
    hosts.wire_lookup(host, Default::default()).unwrap();
    let view = View { env: &env, hosts };
    let mut out = std::vec![0; op.max_reply];
    let length = view.run(op, &input[..used], &mut out)?;
    out.truncate(length);
    validate_reply(&out, op)?;
    Ok(out)
}
fn plain(cursor: u32) -> Ask<'static> {
    Ask {
        text: "word",
        language: "en",
        context: "",
        cursor,
    }
}

#[test]
fn one_provider_per_page_until_none_is_left() {
    let fake = fake();
    for (cursor, provider, more) in [(0, "dict0", 1), (1, "dict1", 1), (2, "dict2", 0)] {
        let reply = ask(&fake, plain(cursor)).unwrap();
        let Record::ServiceLookupEntry(entry) = wire::record_decode_one(&reply).unwrap() else {
            panic!()
        };
        assert_eq!((entry.provider, entry.flags), (provider, more));
    }
    assert_eq!(ask(&fake, plain(3)), Err(ERR_NOT_FOUND));
    assert_eq!(fake.asked.borrow()[0], (7, 0, "word".into(), "en".into()));
}

#[test]
fn input_is_checked_before_any_provider_is_asked() {
    let fake = fake();
    assert_eq!(
        ask(
            &fake,
            Ask {
                text: "",
                ..plain(0)
            }
        ),
        Err(ERR_INVALID)
    );
    for language in [
        "e",
        "en_US",
        "en-",
        "-en",
        "en--US",
        "toolongsubtag-x",
        "en us",
        "é-fr",
    ] {
        assert_eq!(
            ask(
                &fake,
                Ask {
                    language,
                    ..plain(0)
                }
            ),
            Err(ERR_INVALID),
            "{language:?}"
        );
    }
    for language in ["", "en", "pt-BR", "zh-Hant-TW", "de-1996"] {
        assert!(
            ask(
                &fake,
                Ask {
                    language,
                    ..plain(0)
                }
            )
            .is_ok(),
            "{language:?}"
        );
    }
    let long = "w".repeat(1025);
    assert_eq!(
        ask(
            &fake,
            Ask {
                text: &long,
                ..plain(0)
            }
        )
        .map(drop),
        Err(ERR_NO_SPACE)
    );
    let long = "w".repeat(1024);
    assert!(
        ask(
            &fake,
            Ask {
                text: &long,
                context: &long,
                ..plain(0)
            }
        )
        .is_ok()
    );
    assert_eq!(
        fake.asked.borrow().len(),
        6,
        "only the valid asks reached a provider"
    );
}

#[test]
fn a_malformed_provider_answer_is_invalid_and_a_providers_status_passes() {
    let fake = fake();
    let bad = [
        Vec::new(),
        [entry_bytes("a.b", 0, "x"), entry_bytes("a.b", 0, "x")].concat(),
        entry_bytes("a.b", 4, "x"),
        entry_bytes("", 0, "x"),
        std::vec![0xff, 0xff],
    ];
    for answer in bad {
        *fake.answer.borrow_mut() = Some(answer);
        // The fake writes an empty answer as zero bytes.
        assert_eq!(ask(&fake, plain(0)), Err(ERR_INVALID));
    }
    *fake.answer.borrow_mut() = Some(entry_bytes("a.b", 2, ""));
    assert!(ask(&fake, plain(0)).is_ok(), "pending with no text yet");
    let longest = entry_bytes(&"p".repeat(64), 1, &"t".repeat(2048));
    *fake.answer.borrow_mut() = Some(longest);
    assert!(ask(&fake, plain(0)).is_ok());
    fake.fail.set(Some(ERR_UNSUPPORTED));
    assert_eq!(
        ask(&fake, plain(0)),
        Err(ERR_UNSUPPORTED),
        "a network provider before ASY"
    );
}

#[test]
fn a_frame_without_the_host_refuses_the_family() {
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(Instant(0));
    let view = View {
        env: &env,
        hosts: crate::native_host::host_services::HostServices::new(),
    };
    let op = services_wire::service_schema("core.lookup", 1)
        .unwrap()
        .op(1)
        .unwrap();
    let mut out = std::vec![0; op.max_reply];
    assert_eq!(view.run(op, &[0], &mut out), Err(ERR_UNSUPPORTED));
}
