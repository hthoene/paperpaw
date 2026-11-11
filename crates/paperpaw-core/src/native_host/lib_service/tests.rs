#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
//! Service-level tests of `core.library` over fake hosts. This file is the
//! shared harness; the ops' tests are in `tests/query.rs` and `tests/write.rs`.
extern crate std;
use super::*;
use crate::abi7::services_wire::{self, ServiceOp};
use crate::native_host::call_services::validate_reply;
use alloc::vec::Vec;
use paperpaw_i18n::Translator;
use paperpaw_native::host_glue::ERR_DENIED;
use paperpaw_platform::Instant;
use paperpaw_sdk::{Env, Settings};

#[path = "tests/catalog_reads.rs"]
mod catalog_reads;
#[path = "tests/edit_host.rs"]
mod edit_host;
#[path = "tests/query.rs"]
mod query;
#[path = "tests/write.rs"]
mod write;

pub(super) const OWNER: u64 = 7;
pub(super) const PLUGIN: &str = "files";

/// The features the fixture's host wires: `uses.library` and `uses.storage`.
const WIRED: &[paperpaw_native::metadata::Feature<'static>] = &[
    paperpaw_native::metadata::Feature {
        key: "uses.library",
        revision: 1,
    },
    paperpaw_native::metadata::Feature {
        key: "uses.storage",
        revision: 1,
    },
];

/// A frame, and the caller plugin `files` (owner 7) with the library host
/// wired (or not) and `uses.library` / `uses.storage` as the fields say.
pub(super) struct View<'a> {
    env: &'a Env<'a>,
    /// The settings the frame shows: `files.archive` as the test set it.
    settings: Settings,
    hosts: crate::native_host::host_services::HostServices,
    pub(super) granted: bool,
    pub(super) storage: bool,
}
impl Frame for View<'_> {
    fn settings(&self) -> &Settings {
        &self.settings
    }
    fn translator(&self) -> &Translator {
        self.env.i18n()
    }
}
impl View<'_> {
    /// The backend as the dispatcher calls it.
    pub(super) fn run(&self, op: &ServiceOp, input: &[u8], out: &mut [u8]) -> Result<usize, i32> {
        let bits = u32::from(self.granted) | (u32::from(self.storage) << 1);
        let caller = crate::native_host::host_services::Caller {
            owner: OWNER,
            plugin: PLUGIN,
            features: crate::native_host::admitted_features::AdmittedFeatures::test(bits, WIRED),
            hosts: &self.hosts,
        };
        backend(self, &caller, op, input, out)
    }
}

std::thread_local! {
    /// Whether the caller declared `uses.storage` (each test has its thread).
    pub(super) static STORAGE: core::cell::Cell<bool> = const { core::cell::Cell::new(true) };
    /// The value of the `files.archive` setting the frame has ("" for none).
    pub(super) static ARCHIVE: core::cell::Cell<&'static str> = const { core::cell::Cell::new("") };
}

/// Run `body` with a frame over `host`.
pub(super) fn with<R>(host: Rc<dyn LibHost>, body: impl FnOnce(&View<'_>) -> R) -> R {
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let mut settings = Settings::new();
    settings
        .define(paperpaw_sdk::SettingSpec::new(
            paperpaw_sdk::Id::new_static("files.archive"),
            "archive",
            paperpaw_sdk::SettingKind::Text,
            paperpaw_sdk::SettingValue::Text(ARCHIVE.with(core::cell::Cell::get).into()),
            paperpaw_sdk::builtin::groups::OTHER,
        ))
        .unwrap();
    let env = kernel.shared.env(Instant(0));
    let hosts = crate::native_host::host_services::HostServices::new();
    hosts.wire_library(host).unwrap();
    body(&View {
        env: &env,
        settings,
        hosts,
        granted: true,
        storage: STORAGE.with(core::cell::Cell::get),
    })
}

pub(super) fn schema_op(id: u32) -> &'static ServiceOp {
    services_wire::service_schema("core.library", 1)
        .unwrap()
        .op(id)
        .unwrap()
}

/// A counted input batch of the records, within the op's row and byte caps.
pub(super) fn input(op: u32, records: &[Record<'_>]) -> Result<Vec<u8>, i32> {
    let op = schema_op(op);
    let mut buffer = std::vec![0; 4096];
    let used = wire::records_encode_counted(
        records,
        usize::from(op.input_rows),
        op.max_input,
        &mut buffer,
    )?;
    buffer.truncate(used);
    Ok(buffer)
}

/// One call as the dispatcher makes it: the reply is checked against the op's
/// schema (`validate_reply`) before the caller sees it.
pub(super) fn call(
    host: &Rc<dyn LibHost>,
    op: u32,
    records: &[Record<'_>],
) -> Result<Vec<u8>, i32> {
    call_raw(host, op, &input(op, records)?)
}
pub(super) fn call_raw(host: &Rc<dyn LibHost>, id: u32, bytes: &[u8]) -> Result<Vec<u8>, i32> {
    let op = schema_op(id);
    let mut out = std::vec![0; op.max_reply];
    let length = with(host.clone(), |frame| frame.run(op, bytes, &mut out))?;
    out.truncate(length);
    validate_reply(&out, op)?;
    Ok(out)
}

#[test]
fn a_frame_without_the_host_refuses_the_family() {
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(Instant(0));
    let view = View {
        env: &env,
        settings: Settings::new(),
        hosts: crate::native_host::host_services::HostServices::new(),
        granted: true,
        storage: true,
    };
    let op = schema_op(1);
    let mut out = std::vec![0; op.max_reply];
    assert_eq!(view.run(op, &[0], &mut out), Err(ERR_UNSUPPORTED));
}
