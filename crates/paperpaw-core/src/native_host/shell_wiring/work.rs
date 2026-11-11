//! What a shell gives `core.work` (`docs/abi/work.md`): its clock, its radio
//! and whether the network is up ([`WorkDevice`]).
//!
//! Core owns the scheduler and every rule of the service (limits, deadlines,
//! moments, the run's time, events 103 to 105). What the scheduler also asks
//! of the device is composed here, not by the shell: the user's power level
//! and its deadline stretch and whether a real-time screen holds the device
//! come from the kernel at each pass; whether a plugin subscribed to
//! `event.work_due` comes from the owner directory; whether a plugin's
//! network is off or held in saving comes from the network policy
//! (`net_policy`, closed until the kernel has filled it: a plugin's network
//! work is then refused, never allowed by a guess).
//!
//! Wire before `Kernel::start`, in the same call as the other families. The
//! kernel runs the pass at the end of each call and `Kernel::tick`, and its
//! next wake time already counts the next time work comes due: a shell that
//! sleeps until the kernel's deadline needs nothing more.
use super::*;
use crate::native_host::{
    net_policy::NetPolicy,
    net_service::reason,
    owner_directory::OwnerDirectory,
    work_sched::{Limits, Power, WorkEnv, WorkSched},
};
use core::cell::Cell;

/// What the device tells the scheduler (`work.md` section 2). The shell
/// answers from its own state; every method is cheap, never blocks and never
/// calls the kernel.
pub trait WorkDevice {
    /// The monotonic clock, in milliseconds, valid within one start. It must
    /// be the clock the shell gives `Kernel::tick` (the kernel's next wake
    /// time is read against it).
    fn now_ms(&self) -> u64;
    /// The network is up now (an `online` moment comes when it turns true).
    fn network_up(&self) -> bool;
    /// The radio is on anyway (`dev_power.flags` bit 1): work that runs only
    /// when the radio is on for another reason may run.
    fn radio_on(&self) -> bool;
}

/// What a shell names for `core.work`: the device's view and the table sizes
/// of its memory class.
#[derive(Clone)]
pub struct WorkInputs {
    device: Rc<dyn WorkDevice>,
    small: bool,
}

impl core::fmt::Debug for WorkInputs {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("WorkInputs")
            .field("small", &self.small)
            .finish_non_exhaustive()
    }
}

impl WorkInputs {
    /// Whether the device is of the small memory class (the X4's).
    pub(crate) fn small(&self) -> bool {
        self.small
    }

    /// The device's view, for the clocks of other services.
    pub(crate) fn device(&self) -> Rc<dyn WorkDevice> {
        self.device.clone()
    }

    /// `device` is the shell's view; `memory` picks the table sizes (`Small`:
    /// 8 works per plugin and 32 on the device; elsewhere 32 and 256).
    #[must_use]
    pub fn new(device: Rc<dyn WorkDevice>, memory: paperpaw_platform::MemoryClass) -> Self {
        Self {
            device,
            small: crate::native_host::ui_store::is_x4(memory),
        }
    }

    pub(crate) fn limits(&self) -> Limits {
        if self.small {
            Limits::X4
        } else {
            Limits::OTHER
        }
    }

    pub(crate) fn env(self, owners: Shared<OwnerDirectory>, policy: Rc<NetPolicy>) -> WorkAdapter {
        WorkAdapter {
            device: self.device,
            owners,
            policy,
            level: Cell::new(0),
            stretch: Cell::new(1),
            hold: Cell::new(false),
        }
    }
}

/// The scheduler's view of the device, composed of the shell's
/// ([`WorkDevice`]), the kernel's and the owner directory's.
pub(crate) struct WorkAdapter {
    device: Rc<dyn WorkDevice>,
    owners: Shared<OwnerDirectory>,
    policy: Rc<NetPolicy>,
    level: Cell<u32>,
    stretch: Cell<u32>,
    hold: Cell<bool>,
}

impl WorkAdapter {
    /// The policy's end for `owner`'s network use, if any. An owner that is
    /// unknown or stopped is one whose network is off (the safe answer).
    fn end_reason(&self, owner: u64, background: bool) -> Option<u32> {
        let mut found = Some(reason::NETWORK_OFF);
        let seen = self.owners.with_id(owner, &mut |id| {
            found = self.policy.end_for(id, background).map(|end| end.reason);
        });
        if seen.is_err() {
            return Some(reason::NETWORK_OFF);
        }
        found
    }
}

impl WorkEnv for WorkAdapter {
    fn now_ms(&self) -> u64 {
        self.device.now_ms()
    }
    fn power(&self) -> Power {
        Power {
            level: self.level.get(),
            stretch: self.stretch.get(),
            radio_on: self.device.radio_on(),
        }
    }
    fn screen_hold(&self) -> bool {
        self.hold.get()
    }
    fn network_up(&self) -> bool {
        self.device.network_up()
    }
    fn network_allowed(&self, owner: u64) -> bool {
        self.end_reason(owner, false) != Some(reason::NETWORK_OFF)
    }
    fn saving_manual(&self, owner: u64) -> bool {
        self.end_reason(owner, true) == Some(reason::SAVING)
    }
    fn subscribed(&self, owner: u64, tag: u32) -> bool {
        self.owners.subscribed(owner, tag)
    }
    fn kernel_state(&self, level: u32, stretch: u32, screen_hold: bool) {
        self.level.set(level);
        self.stretch.set(stretch);
        self.hold.set(screen_hold);
    }
}

/// The scheduler of a device, made once.
pub(crate) fn scheduler(inputs: &WorkInputs) -> Rc<WorkSched> {
    Rc::new(WorkSched::new(inputs.limits()))
}
