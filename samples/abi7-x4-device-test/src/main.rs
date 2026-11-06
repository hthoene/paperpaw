//! X4 device-test plugin (interface 7): registers `event.started`, counts the
//! events it gets, and, built with a `crash-*` feature, faults on purpose.
#![no_std]
#![no_main]

use core::alloc::{GlobalAlloc, Layout};
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

    fn event(&mut self, _host: &Host<'_>, token: u32, _input: &[u8]) -> Result<(), Error> {
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

// The plugin allocates no data of its own: its state lives in the host's heap
// (the macro); a null allocator catches accidental use of a plugin-owned heap.
struct NoHeap;
// SAFETY: allocations always fail; no non-null allocation can be deallocated.
unsafe impl GlobalAlloc for NoHeap {
    unsafe fn alloc(&self, _: Layout) -> *mut u8 {
        core::ptr::null_mut()
    }
    unsafe fn dealloc(&self, _: *mut u8, _: Layout) {}
}
#[global_allocator]
static HEAP: NoHeap = NoHeap;
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {}
}
