//! X4 device-test plugin (interface 7): registers `event.started`, counts the
//! events it gets, and, built with a `crash-*` feature, faults on purpose.
#![no_std]
#![no_main]

use paperpaw_sdk::native::events::Incoming;
use paperpaw_sdk::native::reply::Reply;
use paperpaw_sdk::native::{Error, Host, Plugin, registrations};

/// One registration batch: `event.started`, token 1, exactly as the host's
/// wire codec writes it (`registrations_encode`, `abi/`); the host rejects any
/// other bytes.
const STARTED: [u8; 12] = [0x01, 0x01, 0x09, 0x00, 0x20, 0, 0, 0, 0x01, 0, 0, 0];
const TOKEN: u32 = 1;

/// Faults the CPU (an illegal instruction): the device resets, and safe mode
/// must name this plugin afterwards.
#[cfg(any(feature = "crash-make", feature = "crash-event"))]
#[inline(never)]
fn fault() -> ! {
    // SAFETY: deliberate: the instruction is illegal and never returns.
    unsafe { core::arch::asm!("unimp", options(noreturn)) }
}

struct DeviceTest {
    events: u32,
}

impl Plugin for DeviceTest {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        #[cfg(feature = "crash-make")]
        fault();
        registrations::submit(host, &STARTED)?;
        Ok(Self { events: 0 })
    }

    fn event(
        &mut self,
        _host: &Host<'_>,
        token: u32,
        _input: Incoming<'_>,
        _reply: &mut Reply<'_>,
    ) -> Result<(), Error> {
        if token != TOKEN {
            return Err(Error::Unsupported);
        }
        #[cfg(feature = "crash-event")]
        fault();
        self.events = self.events.wrapping_add(1);
        Ok(())
    }
}
paperpaw_sdk::native_plugin!(DeviceTest, metadata = include_bytes!("../metadata.bin"));
