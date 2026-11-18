//! How much faster code runs from internal RAM than from flash (and, on
//! the X4 Pro, than from PSRAM, where code plugins run), measured once at
//! start in test builds (feature `remote`) and logged: what hot code in
//! internal RAM gains on this chip (ADR 0030). The work is a small
//! interpreter over its memory, fetching, decoding and branching as an
//! emulator's core does.

use alloc::vec;
use alloc::vec::Vec;

use log::info;

/// Steps a run, as the frame test does a frame.
const STEPS: u32 = 20_000;
const MEMORY: usize = 8192;
const PROGRAM: usize = 256;

struct Machine {
    registers: [u8; 8],
    pc: usize,
    program: Vec<u8>,
    memory: Vec<u8>,
}

impl Machine {
    fn new() -> Self {
        let mut seed = 0x1234_5678u32;
        let program = (0..PROGRAM)
            .map(|_| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (seed >> 24) as u8
            })
            .collect();
        Self {
            registers: [0; 8],
            pc: 0,
            program,
            memory: vec![0; MEMORY],
        }
    }
}

#[inline(always)]
fn step(m: &mut Machine) {
    let op = m.program[m.pc % PROGRAM];
    let a = usize::from(op & 7);
    let b = usize::from((op >> 3) & 7);
    let at = (usize::from(m.registers[a]) << 5 | usize::from(m.registers[b])) % MEMORY;
    match op >> 6 {
        0 => m.registers[a] = m.registers[a].wrapping_add(m.registers[b] ^ op),
        1 => m.registers[a] = m.memory[at],
        2 => m.memory[at] = m.registers[a].rotate_left(u32::from(op & 3)),
        _ if m.registers[a] & 1 == 0 => m.pc = m.pc.wrapping_add(usize::from(m.registers[b])),
        _ => {}
    }
    m.pc = m.pc.wrapping_add(1);
}

/// From flash, through the cache.
#[inline(never)]
fn from_flash(m: &mut Machine, steps: u32) -> u8 {
    for _ in 0..steps {
        step(m);
    }
    m.registers[0]
}

/// From internal RAM (the attribute keeps it from being inlined).
#[esp_hal::ram]
fn from_ram(m: &mut Machine, steps: u32) -> u8 {
    for _ in 0..steps {
        step(m);
    }
    m.registers[0]
}

/// Runs both a few times, the cache cold and warm, and logs the
/// microseconds of each.
pub fn run() {
    let mut machine = Machine::new();
    let time = |f: fn(&mut Machine, u32) -> u8, m: &mut Machine| {
        let started = crate::micros();
        let result = f(m, STEPS);
        (crate::micros() - started, result)
    };
    let mut flash = [0u64; 3];
    let mut ram = [0u64; 3];
    let mut check = 0u8;
    for i in 0..3 {
        let (us, r) = time(from_flash, &mut machine);
        flash[i] = us;
        check ^= r;
        let (us, r) = time(from_ram, &mut machine);
        ram[i] = us;
        check ^= r;
    }
    info!(
        "bench: {STEPS} interpreter steps: flash {flash:?} us, internal RAM {ram:?} us ({check})"
    );
}
