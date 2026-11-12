//! Parking again leaves nothing behind (#248, D3): with every plugin the app
//! lets sleep (`paperpaw_app::SLEEPS`) flagged, 50 cycles of waking each of
//! them and going to standby. Each standby must really park each plugin
//! again, and each next use must make it again; the live heap and the
//! handles all native plugins hold must be the same after the 50th cycle as
//! after the first (a remade plugin opens its font and service handles
//! anew: the old ones must have gone with the old instance).
//!
//! The first cycle grows lasting state once: the storage recall cache's
//! table (`recall::Recall`, about 1.1 KiB, filled by the catalog check after
//! the first standby), the same with every plugin awake; the heap is compared
//! from the end of cycle 2 on. Its own
//! binary without the test harness (`harness = false`): the allocator counts
//! every allocation, and captured output would grow the heap it measures.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

#[path = "support/card.rs"]
mod card;
mod native_activation;

use std::alloc::{GlobalAlloc, Layout, System};
use std::rc::Rc;
use std::sync::atomic::{AtomicIsize, Ordering};

use card::Card;
use paperpaw_core::{Effects, Kernel};
use paperpaw_platform::{
    DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, PixelFormat, Rotation,
};
use paperpaw_sdk::builtin::{actions, slots};
use paperpaw_sdk::{Action, Id, Params};

/// The bytes allocated and not freed, all threads.
static LIVE: AtomicIsize = AtomicIsize::new(0);

struct Counting;

// SAFETY: forwards to `System` unchanged; only counts the sizes.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: the caller's layout, as `GlobalAlloc::alloc` asks.
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            LIVE.fetch_add(layout.size().cast_signed(), Ordering::Relaxed);
        }
        ptr
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size().cast_signed(), Ordering::Relaxed);
        // SAFETY: `ptr` came from `alloc` above with this layout.
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: as `GlobalAlloc::realloc` asks, passed on unchanged.
        let new = unsafe { System.realloc(ptr, layout, new_size) };
        if !new.is_null() {
            LIVE.fetch_add(
                new_size.cast_signed() - layout.size().cast_signed(),
                Ordering::Relaxed,
            );
        }
        new
    }
}

#[global_allocator]
static COUNTING: Counting = Counting;

const CYCLES: usize = 50;

/// An X4 as the emulator's profile has it: buttons, no touch.
const BUTTONS: DeviceInfo = DeviceInfo::new(
    "test-x4",
    "Test X4",
    DisplayInfo::new(480, 800, 220, PixelFormat::Mono1, Rotation::Deg0, None),
    InputCaps::new(
        true,
        false,
        false,
        &["back", "confirm", "left", "right", "up", "down", "power"],
    ),
    MemoryClass::Small,
);

fn fonts() -> paperpaw_app::Fonts {
    #[cfg(any(feature = "packs", not(feature = "outline")))]
    return paperpaw_app::Fonts::Packs;
    #[cfg(all(feature = "outline", not(feature = "packs")))]
    return paperpaw_app::Fonts::Outline;
}

/// How the cycle reaches `plugin`: its slot where the kernel has one, else
/// its first screen, else its first own action.
enum Use {
    Slot(Id),
    Screen(Id),
    Action(Id),
}

struct Run {
    kernel: Kernel,
    now: u64,
}

impl Run {
    /// Ticks until nothing is due within a second; drops the warnings.
    fn settle(&mut self, mut effects: Effects) {
        for _ in 0..10_000 {
            let Some(at) = effects.wake_at() else {
                break;
            };
            if at.0 > self.now + 1000 {
                break;
            }
            self.now = self.now.max(at.0);
            effects = self.kernel.tick(Instant(self.now));
        }
        drop(self.kernel.take_warnings());
    }

    fn run(&mut self, action: &Id) {
        self.now += 100;
        let effects = self.kernel.run_action(Instant(self.now), action.clone());
        self.settle(effects);
    }

    fn reach(&mut self, way: &Use) {
        self.run(&actions::HOME);
        self.now += 100;
        let effects = match way {
            Use::Slot(slot) => {
                self.kernel
                    .open_slot(Instant(self.now), slot.clone(), Params::new())
            }
            Use::Screen(screen) => {
                self.kernel
                    .open_screen(Instant(self.now), screen.clone(), Params::new())
            }
            Use::Action(action) => self.kernel.run_action(Instant(self.now), action.clone()),
        };
        self.settle(effects);
        self.run(&actions::BACK);
    }

    fn standby(&mut self) {
        self.now += 100;
        let effects = self.kernel.suspend(Instant(self.now));
        self.settle(effects);
    }

    fn resume(&mut self) {
        self.now += 100;
        let effects = self.kernel.resume(Instant(self.now));
        self.settle(effects);
    }

    fn way_to(&self, plugin: &str) -> Use {
        match plugin {
            "menu" => return Use::Slot(slots::MENU),
            "keyboard" => return Use::Slot(slots::KEYBOARD),
            "settings" => return Use::Slot(slots::SETTINGS),
            _ => {}
        }
        let mut screens: Vec<Id> = self
            .kernel
            .known_screens()
            .filter(|screen| screen.namespace() == plugin)
            .cloned()
            .collect();
        screens.sort();
        if let Some(screen) = screens.into_iter().next() {
            return Use::Screen(screen);
        }
        let mut own: Vec<Id> = self
            .kernel
            .actions()
            .iter()
            .filter(|action| matches!(action, Action::Native(_)))
            .map(Action::id)
            .filter(|id| id.namespace() == plugin)
            .cloned()
            .collect();
        own.sort();
        Use::Action(own.into_iter().next().expect("a way to reach the plugin"))
    }
}

fn main() {
    let card = Card::new(true);
    let builder = Kernel::builder(BUTTONS)
        .storage(card.clone())
        .user_config("[core]\nlanguage = en\n[help]\nshow = false\n");
    let mut kernel = native_activation::build_on_sleeping(
        paperpaw_app::install(builder, fonts()),
        Rc::new(card),
        paperpaw_app::sleeps,
    );
    let effects = kernel.start(Instant(0));
    let mut run = Run { kernel, now: 0 };
    run.settle(effects);
    let ways: Vec<(&str, Use)> = paperpaw_app::SLEEPS
        .iter()
        .map(|plugin| (*plugin, run.way_to(plugin)))
        .collect();
    // An empty list would pass with nothing tested (CR on #1365).
    assert!(!ways.is_empty(), "no plugin may sleep: nothing to cycle");
    let mut first = None;
    for cycle in 1..=CYCLES {
        for (plugin, way) in &ways {
            run.reach(way);
            assert_eq!(
                run.kernel.native_is_parked(plugin),
                Some(false),
                "{plugin}: not made in cycle {cycle}, the cycle tests nothing"
            );
        }
        run.run(&actions::HOME);
        run.standby();
        for (plugin, _) in &ways {
            assert_eq!(
                run.kernel.native_is_parked(plugin),
                Some(true),
                "{plugin}: not parked again on standby in cycle {cycle}"
            );
        }
        run.resume();
        let now = (
            LIVE.load(Ordering::Relaxed),
            run.kernel.native_handle_count().expect("the handle table"),
        );
        match first {
            None if cycle == 2 => first = Some(now),
            None => {}
            Some(_) if cycle < CYCLES => {}
            Some((heap, handles)) => {
                assert_eq!(now.1, handles, "handles after cycle {cycle} vs cycle 2");
                assert_eq!(
                    now.0, heap,
                    "live heap (bytes) after cycle {cycle} vs cycle 2"
                );
            }
        }
    }
    let (heap, handles) = first.expect("cycle 2 measured");
    println!(
        "sleep_cycles: {CYCLES} cycles of {} plugins: live heap {heap} B and {handles} handles \
         after cycle 2 and cycle {CYCLES}",
        ways.len()
    );
}
