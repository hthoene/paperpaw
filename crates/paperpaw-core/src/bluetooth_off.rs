//! The kernel without a Bluetooth radio model (the `bluetooth` feature off,
//! as on the X4 and the X4 Classic, which have no Bluetooth): what the
//! kernel calls where it has one answers as a kernel built without
//! [`KernelBuilder::bluetooth`] does. The SDK's types stay: plugins use
//! them whether or not the device has the radio. #306, row 11.

#[allow(
    unused_imports,
    reason = "a part of the kernel, which sees all of it; not every part uses every name"
)]
use super::*;

use crate::io::bluetooth::BtReport;
use paperpaw_sdk::bluetooth::{BtError, BtFailure, BtOp, BtRequest};

impl Kernel {
    /// `None`: this kernel is built without Bluetooth.
    pub fn bluetooth_model(&self) -> Option<&paperpaw_sdk::bluetooth::Bluetooth> {
        None
    }

    /// A plugin's Bluetooth command: refused, as `off` (no radio).
    pub(crate) fn apply_bluetooth(&mut self, request: BtRequest) {
        let (_, from) = request.into_parts();
        let plugin = from.as_ref().map_or("", |f| f.as_str());
        let failure = BtFailure::new(BtOp::Extended, BtError::Off);
        self.warn_runtime(format!(
            "{plugin}: Bluetooth {}: {}",
            failure.op.as_str(),
            failure.error
        ));
    }

    /// A native plugin's Bluetooth command: refused as any other.
    pub(crate) fn apply_admitted_bluetooth(&mut self, request: BtRequest) {
        self.apply_bluetooth(request);
    }

    /// Nothing reports Bluetooth here; the call only keeps the kernel's time.
    pub fn bluetooth_event(&mut self, now: Instant, _report: BtReport) -> Effects {
        let running = self.shared.meter.enter(meter::CORE);
        self.shared.now = now;
        drop(running);
        self.finish()
    }

    pub(crate) fn bluetooth_deadline(&self) -> Option<Instant> {
        None
    }

    pub(crate) fn bluetooth_tick(&mut self) {}

    pub(crate) fn bluetooth_screens_closed(&mut self) {}

    pub(crate) fn bluetooth_suspend(&mut self) {}

    pub(crate) fn bluetooth_answer(&mut self, _event: &Id, _params: &Params) -> bool {
        false
    }

    pub(crate) fn sync_bluetooth(&mut self) {}
}
