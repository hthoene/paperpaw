//! Dedicated painted stack for fixed synchronous test draws, never plugins.
//! Scheduler stack checks refer to the registered task stack, so interrupts
//! remain masked while the callback uses this unregistered test stack.

extern crate alloc;
use alloc::vec::Vec;

pub const STACK_BYTES: usize = 24 * 1024;
const GUARD_BYTES: usize = 64;
pub const ALLOCATION_BYTES: usize = STACK_BYTES + 2 * GUARD_BYTES;
const PAINT: u8 = 0xcd;

#[repr(C, align(16))]
#[derive(Clone)]
struct Chunk([u8; 16]);

pub struct Stack {
    chunks: Vec<Chunk>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sample {
    pub total: usize,
    pub unused: usize,
    pub used: usize,
    pub guard_valid: bool,
}

impl Stack {
    pub fn try_new() -> Result<Self, &'static str> {
        let mut chunks = Vec::new();
        chunks
            .try_reserve_exact(ALLOCATION_BYTES / 16)
            .map_err(|_| "probe-stack-allocation")?;
        chunks.resize(ALLOCATION_BYTES / 16, Chunk([PAINT; 16]));
        let mut stack = Self { chunks };
        stack.paint();
        Ok(stack)
    }

    fn paint(&mut self) {
        for chunk in &mut self.chunks {
            chunk.0.fill(PAINT);
        }
        for chunk in &mut self.chunks[..GUARD_BYTES / 16] {
            chunk.0.fill(0xa7);
        }
        for chunk in &mut self.chunks[(GUARD_BYTES + STACK_BYTES) / 16..] {
            chunk.0.fill(0xd3);
        }
    }

    fn sample(&self) -> Sample {
        let guard_valid = self.chunks[..GUARD_BYTES / 16]
            .iter()
            .all(|c| c.0 == [0xa7; 16])
            && self.chunks[(GUARD_BYTES + STACK_BYTES) / 16..]
                .iter()
                .all(|c| c.0 == [0xd3; 16]);
        let unused = self.chunks[GUARD_BYTES / 16..(GUARD_BYTES + STACK_BYTES) / 16]
            .iter()
            .flat_map(|c| c.0.iter())
            .take_while(|&&b| b == PAINT)
            .count();
        Sample {
            total: STACK_BYTES,
            unused,
            used: STACK_BYTES - unused,
            guard_valid,
        }
    }

    /// Runs exactly once on a fresh stack, with no output or allocation here.
    ///
    /// # Safety
    /// The fixed callback must fit STACK_BYTES, return normally, never enable
    /// interrupts, yield, block, access SD/RTOS, unwind, or retain stack pointers.
    /// Call only from thread mode with an outer critical section held across
    /// admission and this operation. Guards diagnose touched bytes after return;
    /// they are not an overflow barrier. Paint measures touched depth, not exact
    /// minimum SP or unwritten reserved frames. No arbitrary callback is safe.
    #[cfg(target_arch = "riscv32")]
    pub unsafe fn run<R, F: FnOnce() -> R>(&mut self, callback: F) -> (R, Sample) {
        use core::{ffi::c_void, mem::MaybeUninit};
        self.paint();
        struct Call<F, R> {
            callback: Option<F>,
            result: MaybeUninit<R>,
        }
        unsafe extern "C" fn invoke<F: FnOnce() -> R, R>(raw: *mut c_void) {
            // SAFETY: synchronous trampoline receives its live Call.
            let call = unsafe { &mut *raw.cast::<Call<F, R>>() };
            if let Some(callback) = call.callback.take() {
                call.result.write(callback());
            }
        }
        let mut call = Call {
            callback: Some(callback),
            result: MaybeUninit::uninit(),
        };
        // SAFETY: allocation is 16-aligned; top excludes the upper guard.
        let top = unsafe {
            self.chunks
                .as_mut_ptr()
                .cast::<u8>()
                .add(GUARD_BYTES + STACK_BYTES)
        };
        // SAFETY: caller holds the admission critical section and all documented
        // callback restrictions. Trampoline preserves the C ABI and returns once.
        unsafe { switch_stack((&raw mut call).cast(), invoke::<F, R>, top) };
        let sample = self.sample();
        // SAFETY: invoke ran the FnOnce and initialized result before returning.
        (unsafe { call.result.assume_init() }, sample)
    }
}

/// a0=call context, a1=callback, a2=new aligned stack top. Callee-saved registers
/// remain unchanged by this wrapper and are preserved by the callback's C ABI.
/// Save old SP/RA in the new stack's 16-byte frame; restore them before return.
#[cfg(target_arch = "riscv32")]
#[unsafe(naked)]
unsafe extern "C" fn switch_stack(
    _context: *mut core::ffi::c_void,
    _callback: unsafe extern "C" fn(*mut core::ffi::c_void),
    _top: *mut u8,
) {
    core::arch::naked_asm!(
        "mv t0, sp",
        "mv sp, a2",
        "addi sp, sp, -16",
        "sw t0, 0(sp)",
        "sw ra, 4(sp)",
        "jalr ra, a1, 0",
        "lw ra, 4(sp)",
        "lw sp, 0(sp)",
        "ret",
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn layout_and_guards_are_fixed_and_scoped_paint_is_fresh() {
        let mut stack = Stack::try_new().unwrap();
        assert_eq!(core::mem::align_of::<Chunk>(), 16);
        assert_eq!(stack.chunks.as_ptr() as usize % 16, 0);
        assert_eq!(
            stack.sample(),
            Sample {
                total: STACK_BYTES,
                unused: STACK_BYTES,
                used: 0,
                guard_valid: true
            }
        );
        stack.chunks[GUARD_BYTES / 16 + 10].0[0] = 0;
        assert_eq!(stack.sample().unused, 160);
        stack.chunks[0].0[0] = 0;
        assert!(!stack.sample().guard_valid);
        stack.paint();
        assert_eq!(stack.sample().unused, STACK_BYTES);
        assert!(stack.sample().guard_valid);
    }
}
