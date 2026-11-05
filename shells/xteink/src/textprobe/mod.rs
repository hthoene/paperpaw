//! Opt-in, synchronous text memory probe. No loader or plugin activation.

mod config;
mod stack_scope;
mod workload;

use alloc::vec::Vec;
use config::Config;
use esp_println::println;
use paperpaw_sdk::digest::{hex, sha256};

struct Snapshot {
    used: usize,
    free: usize,
    largest: usize,
    stack_left: usize,
    stack_size: usize,
    boot_peak: usize,
}

impl Snapshot {
    fn take() -> Self {
        let (stack_left, stack_size) = crate::stack::left();
        let largest = crate::largest_block();
        let (used, free) =
            critical_section::with(|_| (esp_alloc::HEAP.used(), esp_alloc::HEAP.free()));
        Self {
            used,
            free,
            largest,
            stack_left,
            stack_size,
            boot_peak: crate::peak::peak(),
        }
    }

    fn print(&self, phase: &str) {
        println!(
            "PP:TEXTPROBE_{phase} used={} free={} largest={} stack_left={} stack_size={} total={} boot_peak={}",
            self.used,
            self.free,
            self.largest,
            self.stack_left,
            self.stack_size,
            self.used + self.free,
            self.boot_peak
        );
    }
}

/// Controlled fragmentation, not a model of every reader heap history.
/// Sixteen 1-KiB requests are interleaved; eight remain live during the draw.
fn fragmentation(enabled: bool) -> Result<[Option<Vec<u8>>; 16], &'static str> {
    let mut blocks = core::array::from_fn(|_| None);
    if enabled {
        for slot in &mut blocks {
            let mut block = Vec::new();
            block
                .try_reserve_exact(1024)
                .map_err(|_| "fragment-allocation")?;
            block.resize(1024, 0x5a);
            *slot = Some(block);
        }
        for i in (0..16).step_by(2) {
            blocks[i] = None;
        }
    }
    Ok(blocks)
}

fn run(config: Config<'_>) -> Result<(), &'static str> {
    if !config.mode_qualified() {
        println!(
            "PP:TEXTPROBE_UNSUPPORTED case={} mode={} reason=stored-churn-admission-unqualified",
            config.case, config.mode
        );
        return Err("mode-unqualified");
    }
    if !matches!(
        esp_hal::interrupt::RunLevel::current(),
        esp_hal::interrupt::RunLevel::ThreadMode
    ) {
        return Err("thread-mode");
    }
    let stack_before = esp_alloc::HEAP.used();
    let mut stack = stack_scope::Stack::try_new()?;
    let stack_charged = esp_alloc::HEAP.used().saturating_sub(stack_before);
    let text = workload::fixture(config.case)?;
    let fonts = workload::fonts(config.case)?;
    fonts.set_describing(config.mode == "describing");
    let font = workload::handle(&fonts, config.mode)?;
    let mut frame = workload::frame(config.mode)?;
    let fixture = hex::encode(&sha256::digest(text.as_bytes()));
    println!(
        "PP:TEXTPROBE_BEGIN version=1 case={} path={} cache={} heap={} mode={} fixture={} font={} repeats={}",
        config.case,
        config.path,
        config.cache,
        config.heap,
        config.mode,
        fixture,
        config.font(),
        config.repeats
    );
    println!(
        "PP:TEXTPROBE_STACK_SETUP requested={} charged={stack_charged} size={}",
        stack_scope::ALLOCATION_BYTES,
        stack_scope::STACK_BYTES
    );
    if config.cache == "warm" {
        for index in 0..2 {
            let warmup = execute(&mut stack, &font, &mut frame, &text, "direct", false)?;
            println!(
                "PP:TEXTPROBE_WARMUP index={index} irq_body_us={} stack_used={} guard_valid={}",
                warmup.irq_body_us,
                warmup.stack.used,
                u8::from(warmup.stack.guard_valid)
            );
            if !warmup.stack.guard_valid || warmup.stack.unused < 10 * 1024 {
                return Err("warmup-stack");
            }
            warmup.draw?;
        }
    }
    let _fragmentation = fragmentation(config.heap == "fragmented")?;
    for repeat in 0..config.repeats {
        measured(&mut stack, &config, &font, &mut frame, &text, repeat)?;
    }
    Ok(())
}

// Reserve and release a real aligned block while interrupts remain masked.
// The same critical section extends through the draw; no other task/IRQ may
// consume this space between the fallible proof and the fixed-asset workload.
fn reserve() -> Result<usize, &'static str> {
    crate::peak::probing(|| {
        let before = esp_alloc::HEAP.used();
        let mut block = Vec::<u32>::new();
        block
            .try_reserve_exact(96 * 1024 / 4)
            .map_err(|_| "headroom")?;
        let charged = esp_alloc::HEAP.used().saturating_sub(before);
        drop(block);
        if esp_alloc::HEAP.used() != before {
            return Err("reservation-accounting");
        }
        Ok(charged)
    })
}

struct Execution {
    draw: Result<f32, &'static str>,
    heap: Option<crate::peak::ScopeSample>,
    stack: stack_scope::Sample,
    elapsed_us: u64,
    irq_body_us: u64,
    reservation_charged: usize,
}

fn execute(
    stack: &mut stack_scope::Stack,
    font: &paperpaw_sdk::FontHandle<'_>,
    frame: &mut paperpaw_graphics::Framebuffer,
    text: &str,
    path: &str,
    measured: bool,
) -> Result<Execution, &'static str> {
    critical_section::with(|_| {
        let irq_started = esp_hal::time::Instant::now();
        let reservation_charged = reserve()?;
        let heap = if measured {
            Some(crate::peak::begin_scope().ok_or("nested-scope")?)
        } else {
            None
        };
        // SAFETY: fixed synthetic/MemoryStorage callback; no SDK services, SD,
        // scheduler, enabled interrupts, unwinding or escaped stack references.
        // Target stack/layout review remains a device-execution prerequisite.
        let ((draw, elapsed_us), sample) = unsafe {
            stack.run(|| {
                let started = esp_hal::time::Instant::now();
                let draw = if path == "control" {
                    Ok(0.0)
                } else {
                    workload::draw_handle(font, frame, text, path == "adapter")
                };
                (draw, (esp_hal::time::Instant::now() - started).as_micros())
            })
        };
        let heap = heap.map(crate::peak::Scope::finish);
        let irq_body_us = (esp_hal::time::Instant::now() - irq_started).as_micros();
        Ok(Execution {
            draw,
            heap,
            stack: sample,
            elapsed_us,
            irq_body_us,
            reservation_charged,
        })
    })
}

#[allow(
    clippy::too_many_arguments,
    reason = "explicit fixed workload and sample identity"
)]
fn measured(
    stack: &mut stack_scope::Stack,
    config: &Config<'_>,
    font: &paperpaw_sdk::FontHandle<'_>,
    frame: &mut paperpaw_graphics::Framebuffer,
    text: &str,
    repeat: u8,
) -> Result<(), &'static str> {
    let before = Snapshot::take();
    println!("PP:TEXTPROBE_SAMPLE repeat={repeat}");
    before.print("BEFORE");
    let execution = execute(stack, font, frame, text, config.path, true)?;
    let sample = execution.heap.ok_or("scope-missing")?;
    let result = execution.draw;
    let elapsed = execution.elapsed_us;
    let after = Snapshot::take();
    after.print("AFTER");
    let pixels = hex::encode(&sha256::digest(frame.view().data));
    let (status, advance) = match result {
        Ok(width) => ("ok", alloc::format!("{}", width.to_bits())),
        Err(_) => ("draw-error", alloc::string::String::from("unsupported")),
    };
    let scope_valid = result.is_ok()
        && !sample.overflowed
        && sample.failed_allocations == 0
        && execution.stack.guard_valid;
    let min_free = (before.used + before.free).saturating_sub(sample.peak_used);
    let retained = sample.end_used as i64 - sample.baseline_used as i64;
    println!(
        "PP:TEXTPROBE_RESULT version=1 repeat={repeat} status={status} advance_bits={advance} pixels={pixels} baseline_used={} end_used={} peak_used={} allocations={} requested_bytes={} largest_request={} deallocations={} freed_requested_bytes={} failed_allocations={} elapsed_us={elapsed} overflow={} scope_valid={} min_free={min_free} retained_delta={retained}",
        sample.baseline_used,
        sample.end_used,
        sample.peak_used,
        sample.allocations,
        sample.requested_bytes,
        sample.largest_requested_bytes,
        sample.deallocations,
        sample.freed_requested_bytes,
        sample.failed_allocations,
        u8::from(sample.overflowed),
        u8::from(scope_valid)
    );
    println!(
        "PP:TEXTPROBE_STACK repeat={repeat} method=paint-dedicated total={} unused={} used={} guard_valid={} irq_masked=1 irq_body_us={} admission_requested=98304 admission_align=4 admission_charged={} admission_released=1",
        execution.stack.total,
        execution.stack.unused,
        execution.stack.used,
        u8::from(execution.stack.guard_valid),
        execution.irq_body_us,
        execution.reservation_charged
    );
    println!(
        "PP:TEXTPROBE_LIMITS stack_scope_supported=painted-depth-only request_alignment=unsupported semantic_hash=unsupported probe_resolution=64 heap_profile={} warmups={}",
        if config.heap == "fragmented" {
            "synthetic-holes-v1"
        } else {
            "clean"
        },
        if config.cache == "warm" { 2 } else { 0 }
    );
    if !execution.stack.guard_valid {
        return Err("probe-stack-guard");
    }
    if execution.stack.unused < 10 * 1024 {
        return Err("probe-stack-headroom");
    }
    result.map(|_| ())
}

/// Intercepts only the test command; all production remote parsing is untouched.
pub fn command(line: &str) -> bool {
    if line.split_ascii_whitespace().next() != Some("textprobe") {
        return false;
    }
    let result = Config::parse(line).ok_or("arguments").and_then(run);
    if let Err(reason) = result {
        println!("PP:TEXTPROBE_ERROR reason={reason}");
    }
    println!("PP:TEXTPROBE_END");
    println!("PP:END");
    true
}
