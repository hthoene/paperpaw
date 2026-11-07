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

#[path = "tests/query.rs"]
mod query;
#[path = "tests/write.rs"]
mod write;

pub(super) const OWNER: u64 = 7;
pub(super) const PLUGIN: &str = "files";

/// A frame whose caller is plugin `files` (owner 7) and that grants
/// `uses.library` as `feature` says.
pub(super) struct View<'a> {
    env: &'a Env<'a>,
    services: Option<LibServices>,
    pub(super) granted: bool,
    pub(super) storage: bool,
}
impl Frame for View<'_> {
    fn settings(&self) -> &Settings {
        self.env.settings()
    }
    fn translator(&self) -> &Translator {
        self.env.i18n()
    }
    fn caller(&self) -> Result<u64, i32> {
        Ok(OWNER)
    }
    fn caller_plugin(&self) -> Result<&str, i32> {
        Ok(PLUGIN)
    }
    fn feature(&self, key: &str) -> Result<(), i32> {
        let granted = match key {
            "uses.library" => self.granted,
            "uses.storage" => self.storage,
            other => panic!("unexpected feature {other}"),
        };
        if granted { Ok(()) } else { Err(ERR_DENIED) }
    }
    fn host_service(&self, key: &str) -> Option<&dyn core::any::Any> {
        (key == "core.library")
            .then_some(self.services.as_ref())
            .flatten()
            .map(|services| services as &dyn core::any::Any)
    }
}

std::thread_local! {
    /// Whether the caller declared `uses.storage` (each test has its thread).
    pub(super) static STORAGE: core::cell::Cell<bool> = const { core::cell::Cell::new(true) };
}

/// Run `body` with a frame over `host`.
pub(super) fn with<R>(host: Rc<dyn LibHost>, body: impl FnOnce(&View<'_>) -> R) -> R {
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(Instant(0));
    body(&View {
        env: &env,
        services: Some(LibServices(host)),
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
    let length = with(host.clone(), |frame| backend(frame, op, bytes, &mut out))?;
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
        services: None,
        granted: true,
        storage: true,
    };
    let op = schema_op(1);
    let mut out = std::vec![0; op.max_reply];
    assert_eq!(backend(&view, op, &[0], &mut out), Err(ERR_UNSUPPORTED));
}
