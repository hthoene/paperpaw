use super::*;

/// What a plugin asks of the radio, as the kernel gets it (see the
/// module's note).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum BtCommand {
    Scan(Scan),
    StopScan {
        events: Id,
    },
    Connect(Connect),
    Disconnect {
        device: DeviceId,
    },
    Discover(Discover),
    Read {
        device: DeviceId,
        handle: Handle,
    },
    Write {
        device: DeviceId,
        handle: Handle,
        bytes: Vec<u8>,
        response: Response,
    },
    Subscribe {
        device: DeviceId,
        handle: Handle,
    },
    SubscribeTo {
        device: DeviceId,
        service: Uuid,
        characteristic: Uuid,
    },
    Unsubscribe {
        device: DeviceId,
        handle: Handle,
    },
    Pair {
        device: DeviceId,
    },
    Forget {
        device: DeviceId,
        events: Id,
    },
    Give {
        device: DeviceId,
        plugin: Id,
        events: Id,
    },
    TakeBack {
        device: DeviceId,
        plugin: Id,
        events: Id,
    },
    KeysOn {
        device: DeviceId,
        events: Id,
    },
    KeysOff {
        device: DeviceId,
        events: Id,
    },
    HidReports {
        device: DeviceId,
        events: Id,
    },
    Advertise(Advertising),
    StopAdvertising,
    NotifyCentrals {
        service: Uuid,
        characteristic: Uuid,
        bytes: Vec<u8>,
    },
    Respond {
        request: GattRequest,
        bytes: Vec<u8>,
    },
    Refuse {
        request: GattRequest,
        error: AttError,
    },
    Extended {
        key: Id,
        params: Params,
    },
}

/// Whether a write waits for the device's answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Response {
    /// Answered with [`BtEvent::Written`].
    #[default]
    Wanted,
    /// Not answered; [`BtEvent::Sent`] says the next may follow.
    NotWanted,
}
