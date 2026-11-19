//! The heap's inventory for the owner's test images (`heap-inventory`, #400):
//! every live allocation with its size and the code addresses found on the
//! stack when it was made, kept in a table of its own (in RAM the heap gives
//! up, `main.rs` `INVENTORY_BYTES`) and written to the serial log on the
//! command `inventory`. The addresses are conservative (the stack is
//! scanned, not unwound: RISC-V code without frame pointers): they are read
//! against the ELF off the device (`tools/heap-inventory.py`), which skips
//! the allocator's own frames. Never in a release.
//!
//! One entry is 16 bytes: the block's address (17 bits, in 4-byte steps from
//! the start of the data RAM), its requested size (17 bits, clamped), and
//! four code addresses of the flash (22 bits each, in 2-byte steps).

use core::cell::UnsafeCell;

use esp_println::println;

/// Entries of the table (a block each); 56 KB (the start's peak was 175 KB of a 262 KB heap).
const ENTRIES: usize = 3584;
/// The RAM the table takes.
pub const BYTES: usize = ENTRIES * 16;
/// The data RAM's start: the unit of the stored block addresses.
const RAM: usize = 0x3fc8_0000;
/// The flash's code: where the stored code addresses start, and its end.
const CODE: usize = 0x4200_0000;
const CODE_END: usize = 0x4280_0000;
/// Words of the stack looked at after the hook's own frame.
const SCAN: usize = 96;
/// Code addresses kept per allocation, after the first found (the hook's
/// caller inside the allocator).
const KEEP: usize = 4;
/// How far the hook's code reaches from its start, at most.
const HOOK_LEN: usize = 0x300;
/// A probe longer than this gives the allocation up (counted).
const PROBE: usize = 400;

struct Table(UnsafeCell<Inner>);

struct Inner {
    slots: [[u32; 4]; ENTRIES],
    live: u32,
    dropped: u32,
    dropped_bytes: u32,
}

// SAFETY: touched only inside a critical section (the hooks, `dump`).
unsafe impl Sync for Table {}

static TABLE: Table = Table(UnsafeCell::new(Inner {
    slots: [[0; 4]; ENTRIES],
    live: 0,
    dropped: 0,
    dropped_bytes: 0,
}));

fn key(ptr: usize) -> Option<u32> {
    let offset = ptr.checked_sub(RAM)? >> 2;
    (offset < 1 << 17).then_some(offset as u32 + 1)
}

fn home(key: u32) -> usize {
    (key as usize).wrapping_mul(2_654_435_761) % ENTRIES
}

/// The block key of a slot: bits 0..17 (`0`: empty).
fn slot_key(slot: &[u32; 4]) -> u32 {
    slot[0] & 0x1_ffff
}

#[inline(always)]
fn stack_pointer() -> usize {
    let sp: usize;
    // SAFETY: reads a register.
    unsafe { core::arch::asm!("mv {}, sp", out(reg) sp) };
    sp
}

/// The first code addresses on the stack above the hook's frame (`hook`: its
/// start).
fn callers(hook: usize) -> [u32; KEEP] {
    let mut found = [0u32; KEEP];
    let (mut skipped, mut n) = (false, 0);
    let sp = stack_pointer() & !3;
    for i in 0..SCAN {
        let at = sp + i * 4;
        if at + 4 > RAM + 0x6_0000 {
            break;
        }
        // SAFETY: the data RAM's stack; a word read.
        let word = unsafe { (at as *const usize).read_volatile() };
        if (CODE..CODE_END).contains(&word) && word & 1 == 0 {
            // The hook's own code (the callee of this scan) is not a caller.
            if (hook..hook + HOOK_LEN).contains(&word) {
                continue;
            }
            if !skipped {
                skipped = true;
                continue;
            }
            found[n] = ((word - CODE) >> 1) as u32;
            n += 1;
            if n == KEEP {
                break;
            }
        }
    }
    found
}

/// After an allocation (called in esp-alloc's hook, in a critical section).
pub fn allocated(ptr: usize, size: usize, hook: usize) {
    let Some(key) = key(ptr) else {
        return;
    };
    let [a, b, c, d] = callers(hook);
    let size = size.min((1 << 17) - 1) as u32;
    let entry = pack(key, size, [a, b, c, d]);
    // SAFETY: in a critical section (one core), the only user.
    let table = unsafe { &mut *TABLE.0.get() };
    let mut at = home(key);
    for _ in 0..PROBE {
        if slot_key(&table.slots[at]) == 0 {
            table.slots[at] = entry;
            table.live += 1;
            return;
        }
        at = (at + 1) % ENTRIES;
    }
    table.dropped += 1;
    table.dropped_bytes += size;
}

/// Packs 17 + 17 + 4 x 22 bits into 4 words (bit 127 stays 0).
fn pack(key: u32, size: u32, at: [u32; 4]) -> [u32; 4] {
    let mut bits = u128::from(key);
    bits |= u128::from(size) << 17;
    for (i, a) in at.iter().enumerate() {
        bits |= u128::from(*a & 0x3f_ffff) << (34 + 22 * i);
    }
    [
        bits as u32,
        (bits >> 32) as u32,
        (bits >> 64) as u32,
        (bits >> 96) as u32,
    ]
}

fn unpack(slot: &[u32; 4]) -> (u32, u32, [u32; 4]) {
    let bits = u128::from(slot[0])
        | u128::from(slot[1]) << 32
        | u128::from(slot[2]) << 64
        | u128::from(slot[3]) << 96;
    let at = |i: usize| ((bits >> (34 + 22 * i)) & 0x3f_ffff) as u32;
    (
        (bits & 0x1_ffff) as u32,
        ((bits >> 17) & 0x1_ffff) as u32,
        [at(0), at(1), at(2), at(3)],
    )
}

/// Before a free (called in esp-alloc's hook, in a critical section).
pub fn freed(ptr: usize) {
    let Some(key) = key(ptr) else {
        return;
    };
    // SAFETY: as `allocated`.
    let table = unsafe { &mut *TABLE.0.get() };
    let mut at = home(key);
    for _ in 0..PROBE {
        match slot_key(&table.slots[at]) {
            0 => return,
            k if k == key => break,
            _ => at = (at + 1) % ENTRIES,
        }
    }
    if slot_key(&table.slots[at]) != key {
        return;
    }
    table.live -= 1;
    // Backward shift: the run after the hole closes up, so there are no
    // gravestones to fill the table over a long run.
    let mut hole = at;
    let mut next = at;
    loop {
        next = (next + 1) % ENTRIES;
        let k = slot_key(&table.slots[next]);
        if k == 0 {
            break;
        }
        let wanted = home(k);
        // The entry stays where it is if its home lies cyclically in (hole, next].
        let stays = if hole <= next {
            hole < wanted && wanted <= next
        } else {
            hole < wanted || wanted <= next
        };
        if !stays {
            table.slots[hole] = table.slots[next];
            hole = next;
        }
    }
    table.slots[hole] = [0; 4];
}

/// The command `inventory`: one line for each live block, then the totals.
pub fn dump() {
    let (mut blocks, mut bytes) = (0u32, 0u32);
    for i in 0..ENTRIES {
        let slot = critical_section::with(|_| {
            // SAFETY: in a critical section.
            unsafe { (*TABLE.0.get()).slots[i] }
        });
        if slot_key(&slot) == 0 {
            continue;
        }
        let (key, size, at) = unpack(&slot);
        blocks += 1;
        bytes += size;
        let ptr = RAM + ((key as usize - 1) << 2);
        let code = |a: u32| {
            if a == 0 {
                0
            } else {
                CODE + ((a as usize) << 1)
            }
        };
        println!(
            "PP:INV {ptr:x} {size} {:x} {:x} {:x} {:x}",
            code(at[0]),
            code(at[1]),
            code(at[2]),
            code(at[3])
        );
    }
    let (live, dropped, dropped_bytes) = critical_section::with(|_| {
        // SAFETY: in a critical section.
        let t = unsafe { &*TABLE.0.get() };
        (t.live, t.dropped, t.dropped_bytes)
    });
    println!(
        "PP:INV-END blocks {blocks} bytes {bytes} live {live} dropped {dropped} dropped_bytes {dropped_bytes} used {} free {} table {BYTES}",
        esp_alloc::HEAP.used(),
        esp_alloc::HEAP.free()
    );
    println!("PP:END");
}
