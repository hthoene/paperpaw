//! The emulator as a library: the emulated device, its profiles and what
//! measures it (heap budget, storage trace), for the `paperpaw-emu`
//! window and for tools that drive the device themselves, such as
//! `paperpaw-dev` (`tools/dev`) and the browser's emulator
//! (`shells/web`). Without the feature `window` it has no window and
//! builds for `wasm32` too.
//!
//! Linking it puts its allocator in charge ([`budget::BudgetAllocator`]),
//! so that the kernel's heap is counted against the device's budget.

pub mod bluetooth;
pub mod budget;
#[cfg(not(target_arch = "wasm32"))]
pub mod cable;
#[cfg(not(target_arch = "wasm32"))]
pub mod card_image;
pub mod device;
pub mod headless;
pub mod host;
pub mod http;
pub mod light;
#[cfg(not(target_arch = "wasm32"))]
pub mod native;
pub mod offline;
pub mod panel;
#[cfg(not(target_arch = "wasm32"))]
pub mod private;
pub mod profiles;
#[cfg(not(target_arch = "wasm32"))]
pub mod registry;
#[cfg(not(target_arch = "wasm32"))]
pub mod stdio;
pub mod trace;
pub mod wifi;

#[global_allocator]
static ALLOCATOR: budget::BudgetAllocator = budget::BudgetAllocator;

/// One emulated device at a time in this crate's tests, and none while
/// the budget's own are run: the heap budget counts every thread's (one
/// device's is the whole budget of an X4).
#[cfg(test)]
pub(crate) fn one_device_at_a_time() -> std::sync::MutexGuard<'static, ()> {
    budget::accounting_tests::LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
