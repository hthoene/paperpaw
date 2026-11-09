//! `core.ui` op 1: the stack top first, paged, and what a guard hides.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
use super::*;
use crate::native_host::host_services::HostServices;
use crate::native_host::stack_rows::{GUARDS, OVERLAY, StackRow};
use alloc::{
    string::{String, ToString},
    vec::Vec,
};
use paperpaw_platform::Instant;
use paperpaw_sdk::{Id, ScreenKey, StackEntry};

const LOCK: u64 = 12;
const READER: u64 = 13;

/// A frame over a kernel with the given stack (screen id, slot, instance,
/// owner, flags), bottom to top.
struct View<'a> {
    env: &'a paperpaw_sdk::Env<'a>,
    rows: &'a crate::native_host::stack_rows::StackRows,
}

impl Frame for View<'_> {
    fn settings(&self) -> &paperpaw_sdk::Settings {
        self.env.settings()
    }
    fn translator(&self) -> &paperpaw_i18n::Translator {
        self.env.i18n()
    }
    fn callback_epoch(&self) -> Result<u64, i32> {
        Ok(1)
    }
    fn kernel_env(&self) -> Option<&paperpaw_sdk::Env<'_>> {
        Some(self.env)
    }
    fn with_stack(&self, visit: &mut dyn FnMut(&dyn UiStackView)) -> Result<(), i32> {
        visit(self.rows);
        Ok(())
    }
}

type Row = (&'static str, Option<&'static str>, u32, u64, u8);

fn kernel(rows: &[Row]) -> crate::Kernel {
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    for (screen, slot, instance, _, flags) in rows {
        kernel.shared.stack.push(
            StackEntry::new(
                Id::new_static(screen),
                slot.map(Id::new_static),
                flags & OVERLAY != 0,
                ScreenKey::new(*instance),
            )
            .guarding(flags & GUARDS != 0),
        );
    }
    kernel
        .shared
        .stack_rows
        .update(rows.iter().map(|(_, _, instance, owner, flags)| StackRow {
            instance: *instance,
            owner: *owner,
            flags: *flags,
        }));
    kernel
}

/// One page of op 1 as plugin `viewer` asks: the rows (screen, instance, slot,
/// owner, flags) and the end record.
fn ask(
    kernel: &crate::Kernel,
    viewer: u64,
    cursor: u32,
    generation: u32,
) -> Result<(Vec<(String, u32, String, String, u32)>, wire::ServiceEnd), i32> {
    let env = kernel.shared.env(Instant(0));
    let hosts = HostServices::new();
    let caller = Caller {
        owner: viewer,
        plugin: "reader",
        features: crate::native_host::admitted_features::AdmittedFeatures::NONE,
        hosts: &hosts,
    };
    let mut input = [0; 128];
    let length = wire::records_encode(
        &[Record::ServiceRegQuery(wire::ServiceRegQuery {
            scope: "",
            cursor,
            generation,
            flags: 0,
        })],
        &mut input,
    )
    .unwrap();
    let mut out = [0; 4096];
    let used = stack(
        &View {
            env: &env,
            rows: &kernel.shared.stack_rows,
        },
        &caller,
        &input[..length],
        &mut out,
    )?;
    let mut bytes = &out[..used];
    let mut rows = Vec::new();
    loop {
        let (record, length) = codec::decode_any_prefix(bytes).unwrap();
        bytes = &bytes[length..];
        match record {
            Record::ServiceUiScreen(row) => rows.push((
                row.screen.to_string(),
                row.instance,
                row.slot.to_string(),
                row.owner.to_string(),
                row.flags,
            )),
            Record::ServiceEnd(end) => {
                assert!(bytes.is_empty());
                return Ok((rows, end));
            }
            other => panic!("{other:?}"),
        }
    }
}

const HOME: Row = ("home.screen", None, 1, READER, 0);
const BOOK: Row = ("reader.book", Some("core.reader"), 2, READER, 0);
const MENU: Row = ("menu.screen", None, 3, READER, OVERLAY);
const LOCKED: Row = ("lock.screen", None, 4, LOCK, GUARDS);

#[test]
fn the_stack_is_listed_top_first_with_its_flags() {
    let kernel = kernel(&[HOME, BOOK, MENU]);
    let (rows, end) = ask(&kernel, READER, 0, 0).unwrap();
    assert_eq!(
        rows,
        [
            (
                "menu.screen".to_string(),
                3,
                String::new(),
                "menu".to_string(),
                1
            ),
            (
                "reader.book".to_string(),
                2,
                "core.reader".to_string(),
                "reader".to_string(),
                4
            ),
            (
                "home.screen".to_string(),
                1,
                String::new(),
                "home".to_string(),
                4
            ),
        ]
    );
    assert_eq!((end.total, end.next, end.flags), (3, 3, 1));
    assert_ne!(end.generation, 0);
}

#[test]
fn pages_of_eight_and_a_stale_generation() {
    let names = [
        "a.0", "a.1", "a.2", "a.3", "a.4", "a.5", "a.6", "a.7", "a.8", "a.9",
    ];
    let rows: Vec<Row> = names
        .iter()
        .enumerate()
        .map(|(i, name)| (*name, None, i as u32 + 1, READER, 0))
        .collect();
    let mut kernel = kernel(&rows);
    let (first, end) = ask(&kernel, READER, 0, 0).unwrap();
    assert_eq!((first.len(), end.total, end.next, end.flags), (8, 10, 8, 0));
    assert_eq!(first[0].1, 10);
    let (second, last) = ask(&kernel, READER, end.next, end.generation).unwrap();
    assert_eq!((second.len(), last.next, last.flags), (2, 10, 1));
    assert_eq!(second[1].1, 1);
    // The stack changed between the pages: start again.
    kernel
        .shared
        .stack_rows
        .update((1..=9).map(|instance| StackRow {
            instance,
            owner: READER,
            flags: 0,
        }));
    kernel.shared.stack.pop();
    assert_eq!(
        ask(&kernel, READER, end.next, end.generation),
        Err(ERR_STATE)
    );
    // A cursor without a generation; a cursor past the end.
    assert_eq!(ask(&kernel, READER, 4, 0), Err(ERR_STATE));
    let (_, now) = ask(&kernel, READER, 0, 0).unwrap();
    assert_eq!(ask(&kernel, READER, 99, now.generation), Err(ERR_INVALID));
}

/// Under a guard: another plugin sees the topmost guard alone and is told rows
/// were left out; the guard's plugin sees from the guard up; what lies above a
/// guard is secret.
#[test]
fn a_guard_hides_the_stack_from_other_plugins() {
    const KEYBOARD: Row = ("keyboard.screen", None, 5, LOCK, 0);
    let kernel = kernel(&[HOME, BOOK, LOCKED, KEYBOARD]);
    let (rows, end) = ask(&kernel, READER, 0, 0).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(
        (rows[0].0.as_str(), rows[0].1, rows[0].3.as_str()),
        ("lock.screen", 4, "lock")
    );
    // Guards (bit 1), covered (bit 2), secret (bit 4).
    assert_eq!(rows[0].4, 0b10110);
    assert_eq!((end.total, end.flags), (1, 1 | 4));
    let (rows, end) = ask(&kernel, LOCK, 0, 0).unwrap();
    assert_eq!(rows.iter().map(|row| row.1).collect::<Vec<_>>(), [5, 4]);
    assert_eq!(end.flags, 1 | 4);
    // Above the guard: secret, not covered.
    assert_eq!(rows[0].4, 1 << 4);
    // No guard: nothing is left out.
    let open = kernel_open();
    assert_eq!(ask(&open, READER, 0, 0).unwrap().1.flags, 1);
}

fn kernel_open() -> crate::Kernel {
    kernel(&[HOME, BOOK])
}
