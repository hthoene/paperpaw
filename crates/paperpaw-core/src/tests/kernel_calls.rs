//! Calls into the kernel from the shell: what they count, what they say
//! back, readings that make no sense.

use super::*;
use paperpaw_platform::{SystemStatus, WallClock};
use paperpaw_sdk::semantics::Role;

/// Rows of which one has the focus, moved by *Down*.
struct RowsPlugin;

struct Rows {
    focus: usize,
}

impl Screen for Rows {
    fn draw(&mut self, canvas: &mut Canvas<'_>, _: &Env<'_>) {
        for row in 0..3u16 {
            let label = alloc::format!("Row {row}");
            let area = Rect::new(0, row * 20, 90, 20);
            canvas
                .describe(Role::Row, &label, area)
                .focused(usize::from(row) == self.focus);
        }
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        if *action == actions::DOWN {
            self.focus = (self.focus + 1) % 3;
            cx.request_redraw();
            return Handled::Yes;
        }
        Handled::No
    }
}

impl Plugin for RowsPlugin {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("rows"), "Rows", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_screen(ScreenSpec::new(
            Id::new_static("rows.screen"),
            "rows",
            Box::new(|_, _| Box::new(Rows { focus: 0 })),
        ))?;
        r.bind_slot(
            slots::HOME,
            Id::new_static("rows.screen"),
            Priority::DEFAULT,
        )?;
        Ok(())
    }
}

/// What the shell is asked to say.
fn said(kernel: &mut Kernel) -> Vec<String> {
    kernel
        .take_speech()
        .into_iter()
        .filter_map(|call| match call {
            SpeechCall::Say { text, .. } => Some(text),
            SpeechCall::Stop => None,
        })
        .collect()
}

/// Moving the focus quickly, each row is said in place of the one before
/// (not left out while that is still spoken); while a plugin reads
/// aloud, the focus is not said over it.
#[test]
fn the_focus_is_said_in_place_of_what_was_said_of_it() {
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(RowsPlugin)
        .speech(true)
        .setting_default(&settings::SPEAK_FOCUS, true)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    render(&mut kernel);
    // Neither is done being said when the next comes.
    press(&mut kernel, 10, "down");
    press(&mut kernel, 100, "down");
    assert_eq!(said(&mut kernel), ["Row 1", "Row 2"]);
    // A plugin reads aloud: the focus waits.
    kernel.apply(Command::speak(paperpaw_sdk::Speech::new(
        "A page",
        Id::new_static("rows.read"),
    )));
    kernel.finish();
    press(&mut kernel, 200, "down");
    assert_eq!(said(&mut kernel), ["A page"]);
}

/// Every call from the shell is counted as the kernel's work, also those
/// that only redraw or pass on what happened.
#[test]
fn every_call_counts_as_the_kernels_work() {
    let mut kernel = Kernel::builder(DEVICE).build().unwrap();
    kernel.start(Instant(0));
    let calls = |kernel: &Kernel| {
        let usage = kernel.plugin_usage();
        usage
            .iter()
            .find(|p| p.plugin.as_str() == "core")
            .unwrap()
            .calls
    };
    let mut before = calls(&kernel);
    let steps: [&dyn Fn(&mut Kernel) -> crate::Effects; 5] = [
        &|k| k.draw_again(Instant(10)),
        &|k| k.resize(Instant(20), 80, 90),
        &|k| k.speech_done(Instant(30), 7, true),
        &|k| k.clipboard_text(Instant(40), None),
        &|k| {
            let event = crate::ServerEvent::Failed {
                server: 9,
                reason: String::new(),
            };
            k.server_event(Instant(50), event)
        },
    ];
    for (i, step) in steps.iter().enumerate() {
        step(&mut kernel);
        let now = calls(&kernel);
        assert!(now > before, "call {i} not counted");
        before = now;
    }
}

/// What the shell does after a call lists everything it is to do.
#[test]
fn the_effects_list_all_there_is_to_do() {
    let effects = crate::Effects {
        grays: true,
        proceed: true,
        sleep: true,
        ..crate::Effects::default()
    };
    let all: Vec<crate::Effect> = effects.iter().collect();
    assert_eq!(
        all,
        [
            crate::Effect::Sleep,
            crate::Effect::Grays,
            crate::Effect::Proceed
        ]
    );
}

/// Wall clock readings far apart, at the ends of the clock, do not
/// overflow when compared.
#[test]
fn wall_clock_readings_that_make_no_sense_do_not_panic() {
    let mut kernel = Kernel::builder(DEVICE).build().unwrap();
    kernel.start(Instant(0));
    let clock = |at: u64, unix_millis: i64, offset: Option<i32>| {
        Some(WallClock::new(Instant(at), unix_millis, offset))
    };
    for (now, reading) in [
        (10, clock(0, i64::MAX, None)),
        (20, clock(0, i64::MIN, None)),
        (u64::MAX, clock(0, 0, Some(i32::MAX))),
        (30, clock(u64::MAX, i64::MAX, Some(i32::MIN))),
    ] {
        let status = SystemStatus::new(reading, None);
        kernel.set_status(Instant(now), status);
    }
}

/// Switches the device off as soon as PaperPaw started.
struct OffAtStart;

impl Plugin for OffAtStart {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("off"), "Off", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.on(paperpaw_sdk::builtin::events::STARTED, |_, _, out| {
            out.push(Command::PowerOff);
        })
    }
}

#[test]
fn plugins_switch_the_device_off_where_the_shell_can() {
    // Where the system decides (Android), nothing happens.
    let mut kernel = Kernel::builder(DEVICE).plugin(OffAtStart).build().unwrap();
    assert!(!kernel.start(Instant(0)).power_off());
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(OffAtStart)
        .power_off(true)
        .build()
        .unwrap();
    assert!(kernel.shared.env(Instant(0)).caps().power_off);
    let effects = kernel.start(Instant(0));
    assert!(effects.power_off() && !effects.sleep());
    assert!(effects.iter().any(|e| e == crate::Effect::PowerOff));
}

#[test]
fn plugins_hear_how_long_the_device_slept() {
    let mut kernel = Kernel::builder(DEVICE).build().unwrap();
    kernel.start(Instant(0));
    assert_eq!(kernel.shared.env(Instant(0)).slept_for(), None);
    kernel.suspend(Instant(1_000));
    kernel.resume(Instant(61_000));
    let slept = kernel.shared.env(Instant(61_000)).slept_for();
    assert_eq!(slept, Some(core::time::Duration::from_secs(60)));
    // A start after a sleep with the device switched off, where the shell
    // knows how long.
    let kernel = Kernel::builder(DEVICE)
        .after_sleep(true)
        .slept_for(core::time::Duration::from_secs(3_600))
        .build()
        .unwrap();
    let slept = kernel.shared.env(Instant(0)).slept_for();
    assert_eq!(slept, Some(core::time::Duration::from_secs(3_600)));
}
