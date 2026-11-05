//! Opt-in, synchronous text memory probe. No loader or plugin activation.

mod config;
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
        }
    }

    fn print(&self, phase: &str) {
        println!(
            "PP:TEXTPROBE_{phase} used={} free={} largest={} stack_left={} stack_size={}",
            self.used, self.free, self.largest, self.stack_left, self.stack_size
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
    let text = workload::fixture(config.case)?;
    let fonts = workload::fonts(config.case)?;
    fonts.set_describing(config.mode == "describing");
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
    admit(config.case, &Snapshot::take())?;
    if config.cache == "warm" {
        for _ in 0..2 {
            workload::draw(&fonts, &mut frame, &text, config.mode, false)?;
        }
    }
    let _fragmentation = fragmentation(config.heap == "fragmented")?;
    for repeat in 0..config.repeats {
        measured(&config, &fonts, &mut frame, &text, repeat)?;
    }
    Ok(())
}

fn admit(_case: &str, before: &Snapshot) -> Result<(), &'static str> {
    // Infallible renderer allocations remain unchanged. This test admission
    // reserve is conservative, not an arbitrary-font memory bound. A refusal
    // is evidence of insufficient entry headroom, never a passing draw.
    // One contiguous reserve must cover the whole fixed-asset transient demand;
    // a largest block sized only for Part cannot admit the later order buffer.
    let (free_needed, largest_needed) = (96 * 1024, 96 * 1024);
    if before.free < free_needed || before.largest < largest_needed {
        return Err("headroom");
    }
    Ok(())
}

fn measured(
    config: &Config<'_>,
    fonts: &paperpaw_sdk::Fonts,
    frame: &mut paperpaw_graphics::Framebuffer,
    text: &str,
    repeat: u8,
) -> Result<(), &'static str> {
    let before = Snapshot::take();
    println!("PP:TEXTPROBE_SAMPLE repeat={repeat}");
    before.print("BEFORE");
    admit(config.case, &before)?;
    let scope = crate::peak::begin_scope().ok_or("nested-scope")?;
    let started = esp_hal::time::Instant::now();
    let result = if config.path == "control" {
        Ok(0.0)
    } else {
        workload::draw(fonts, frame, text, config.mode, config.path == "adapter")
    };
    let elapsed = (esp_hal::time::Instant::now() - started).as_micros();
    let sample = scope.finish();
    let after = Snapshot::take();
    after.print("AFTER");
    let pixels = hex::encode(&sha256::digest(frame.view().data));
    let (status, advance) = match result {
        Ok(width) => ("ok", alloc::format!("{}", width.to_bits())),
        Err(_) => ("draw-error", alloc::string::String::from("unsupported")),
    };
    println!(
        "PP:TEXTPROBE_RESULT version=1 repeat={repeat} status={status} advance_bits={advance} pixels={pixels} baseline_used={} end_used={} peak_used={} allocations={} requested_bytes={} largest_request={} deallocations={} freed_requested_bytes={} failed_allocations={} elapsed_us={elapsed} overflow={}",
        sample.baseline_used,
        sample.end_used,
        sample.peak_used,
        sample.allocations,
        sample.requested_bytes,
        sample.largest_requested_bytes,
        sample.deallocations,
        sample.freed_requested_bytes,
        sample.failed_allocations,
        u8::from(sample.overflowed)
    );
    println!(
        "PP:TEXTPROBE_LIMITS stack_scope_supported=0 request_alignment=unsupported semantic_hash=unsupported probe_resolution=64 heap_profile={} warmups={}",
        if config.heap == "fragmented" {
            "artificial-16x1024-held8"
        } else {
            "clean"
        },
        if config.cache == "warm" { 2 } else { 0 }
    );
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
