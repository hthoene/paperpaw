//! Op 3 `hints` through the `core.ui` backend: rows as declared, order, scope,
//! paging and the generation.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
extern crate std;
use super::*;
use crate::abi7::{
    context::{
        registry_views::{Asker, RegistryView, Row},
        ui_views::{Declared, HintView, UiDirectory},
    },
    services_wire,
};
use crate::native_host::{
    admitted_features::AdmittedFeatures,
    call_services::validate_reply,
    host_services::HostServices,
    shared::Shared,
    ui_service::{self, UiServices},
    ui_store::{Limits, UiStore},
};
use alloc::{string::String, vec::Vec};
use core::cell::Cell;
use paperpaw_i18n::Translator;
use paperpaw_native::host_glue::ERR_NOT_FOUND;
use paperpaw_platform::Instant;
use paperpaw_sdk::{Env, Settings};

/// One declared hint: (scope, owner, input, zone, text).
type Declared4 = (&'static str, &'static str, u32, [u32; 4], String);

struct Dir(Vec<Declared4>);

impl UiDirectory for Dir {
    fn widget(&self, _: &str) -> Result<Declared, i32> {
        Err(ERR_NOT_FOUND)
    }
    fn status_item(&self, _: &str) -> Result<Declared, i32> {
        Err(ERR_NOT_FOUND)
    }
    fn status_line(&self, _: &str) -> Result<Declared, i32> {
        Err(ERR_NOT_FOUND)
    }
    fn hints(
        &self,
        scope: &str,
        visit: &mut dyn FnMut(HintView<'_>) -> ControlFlow<()>,
    ) -> Result<(), i32> {
        for (at, owner, input, zone, text) in &self.0 {
            if *at != scope {
                continue;
            }
            let flow = visit(HintView {
                input: *input,
                x: zone[0],
                y: zone[1],
                width: zone[2],
                height: zone[3],
                text,
                owner,
            });
            if flow.is_break() {
                break;
            }
        }
        Ok(())
    }
}

struct Generation(Cell<u32>);
impl RegistryView for Generation {
    fn generation(&self) -> u32 {
        self.0.get()
    }
    fn rows(
        &self,
        _: u32,
        _: &str,
        _: u32,
        _: &Asker<'_>,
        _: &mut dyn FnMut(&dyn Row) -> ControlFlow<()>,
    ) -> Result<(), i32> {
        Err(ERR_UNSUPPORTED)
    }
    fn detail(
        &self,
        _: u32,
        _: &str,
        _: &Asker<'_>,
        _: &mut dyn FnMut(&dyn Row) -> Result<(), i32>,
    ) -> Result<(), i32> {
        Err(ERR_UNSUPPORTED)
    }
}

struct World<'a> {
    env: &'a Env<'a>,
    hosts: HostServices,
    dir: Dir,
    generation: Generation,
}

impl Frame for World<'_> {
    fn settings(&self) -> &Settings {
        self.env.settings()
    }
    fn translator(&self) -> &Translator {
        self.env.i18n()
    }
    fn ui_directory(&self) -> Result<&dyn UiDirectory, i32> {
        Ok(&self.dir)
    }
    fn registry(&self) -> Result<&dyn RegistryView, i32> {
        Ok(&self.generation)
    }
}

/// One page: the hints (input, zone, text, owner) and the `end` record.
type Page = (Vec<(u32, [u32; 4], String, String)>, wire::ServiceEnd);

impl World<'_> {
    fn call(&self, scope: &str, cursor: u32, generation: u32, flags: u32) -> Result<Page, i32> {
        let schema = services_wire::service_schema("core.ui", 1).unwrap();
        let op = schema.op(3).unwrap();
        let mut input = std::vec![0; 512];
        let used = wire::records_encode_counted(
            &[Record::ServiceRegQuery(wire::ServiceRegQuery {
                scope,
                cursor,
                generation,
                flags,
            })],
            usize::from(op.input_rows),
            op.max_input,
            &mut input,
        )?;
        let caller = Caller {
            owner: 1,
            plugin: "help",
            features: AdmittedFeatures::NONE,
            hosts: &self.hosts,
        };
        let mut out = std::vec![0; op.max_reply];
        let length = ui_service::backend(self, &caller, op, &input[..used], &mut out)?;
        out.truncate(length);
        validate_reply(&out, op)?;
        let (mut rows, mut end, mut bytes) = (Vec::new(), None, out.as_slice());
        while !bytes.is_empty() {
            let (record, taken) = codec::decode_any_prefix(bytes).unwrap();
            bytes = &bytes[taken..];
            match record {
                Record::ServiceUiHint(h) => {
                    rows.push((
                        h.input,
                        [h.x, h.y, h.width, h.height],
                        h.text.into(),
                        h.owner.into(),
                    ));
                }
                Record::ServiceEnd(e) => end = Some(e),
                other => panic!("{other:?}"),
            }
        }
        Ok((rows, end.unwrap()))
    }
}

fn world<R>(hints: Vec<Declared4>, f: impl FnOnce(&World<'_>) -> R) -> R {
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
        dir: Dir(hints),
        generation: Generation(Cell::new(7)),
    })
}

fn hint(scope: &'static str, owner: &'static str, text: &str) -> Declared4 {
    (scope, owner, 0, [0; 4], text.into())
}

#[test]
fn a_page_holds_the_hints_of_the_screen_as_declared_in_order() {
    let hints = std::vec![
        (
            "reader.main",
            "keys",
            2,
            [0, 0, 0, 0],
            String::from("keys.next")
        ),
        hint("home.main", "other", "other.text"),
        (
            "reader.main",
            "help",
            1,
            [100, 200, 300, 400],
            String::from("help.zone")
        ),
    ];
    world(hints, |w| {
        let (rows, end) = w.call("reader.main", 0, 0, 0).unwrap();
        assert_eq!(
            rows,
            [
                (2, [0; 4], "keys.next".into(), "keys".into()),
                (1, [100, 200, 300, 400], "help.zone".into(), "help".into()),
            ]
        );
        assert_eq!(
            (end.total, end.next, end.flags, end.generation),
            (2, 2, 1, 7)
        );
    });
}

#[test]
fn a_screen_nobody_declared_a_hint_for_is_an_empty_page() {
    world(std::vec![hint("a.b", "x", "x.t")], |w| {
        let (rows, end) = w.call("c.d", 0, 0, 0).unwrap();
        assert!(rows.is_empty());
        assert_eq!((end.total, end.flags), (0, 1));
        // So is a scope that is no id at all.
        assert!(w.call("not an id", 0, 0, 0).unwrap().0.is_empty());
    });
}

#[test]
fn many_hints_page_by_cursor_in_one_generation() {
    let hints: Vec<Declared4> = (0..40)
        .map(|n| hint("a.b", "x", &std::format!("x.t{n}")))
        .collect();
    world(hints, |w| {
        let (first, end) = w.call("a.b", 0, 0, 0).unwrap();
        assert_eq!((first.len(), end.total, end.next, end.flags), (8, 40, 8, 0));
        let (rest, end) = w.call("a.b", end.next, end.generation, 0).unwrap();
        assert_eq!(rest.len(), 8);
        assert_eq!(rest[0].2, "x.t8");
        assert_eq!((end.next, end.flags), (16, 0));
        // The last page: the rest, and `end` says so.
        let (last, end) = w.call("a.b", 32, end.generation, 0).unwrap();
        assert_eq!((last.len(), end.next, end.flags), (8, 40, 1));
        // A cursor of another generation is stale.
        assert_eq!(w.call("a.b", 8, 6, 0).err(), Some(ERR_STATE));
    });
}

#[test]
fn a_flag_this_build_does_not_know_is_refused() {
    world(std::vec![hint("a.b", "x", "x.t")], |w| {
        assert_eq!(w.call("a.b", 0, 0, 1).err(), Some(ERR_UNSUPPORTED));
    });
}
