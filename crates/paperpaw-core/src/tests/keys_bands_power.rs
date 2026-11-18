//! Key maps, frames drawn in bands, turned screens, sleep and speech.

use super::*;

/// Swapped page buttons apply while reading only (the reader's tests
/// show them swapped there): elsewhere they stay as they are.
#[test]
fn page_buttons_stay_outside_the_reader() {
    let log = log();
    let mut kernel = Kernel::builder(DEVICE)
        .test_plugin(TestPlugin::new("a", &log))
        .setting_default(&settings::SWAP_PAGE_BUTTONS, true)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    press(&mut kernel, 100, "page-forward");
    press(&mut kernel, 300, "page-back");
    press(&mut kernel, 500, "confirm");
    assert_eq!(
        *log.borrow(),
        ["a:core.next-page", "a:core.prev-page", "a:core.confirm"]
    );
}

#[test]
fn keymap_presets_follow_device_defaults_and_switch_at_runtime() {
    let log = log();
    let mut kernel = Kernel::builder(DEVICE)
        .test_plugin(TestPlugin::new("a", &log))
        .setting_default(&settings::KEYMAP_PRESET, "core.two-buttons".into())
        .build()
        .unwrap();
    assert!(kernel.take_warnings().is_empty());
    assert_eq!(
        kernel.settings().text("core.keymap-preset"),
        "core.two-buttons"
    );
    kernel.start(Instant(0));

    // Two-button preset: forward is "next", which falls back to next page;
    // a long press on forward confirms.
    press(&mut kernel, 100, "page-forward");
    kernel.handle(
        Instant(300),
        InputEvent::Button {
            name: "page-forward",
            state: ButtonState::Pressed,
        },
    );
    kernel.tick(Instant(900));
    assert_eq!(
        *log.borrow(),
        ["a:core.next", "a:core.next-page", "a:core.confirm"]
    );

    // Back to the standard layout: forward turns pages directly again.
    log.borrow_mut().clear();
    kernel.handle(
        Instant(1000),
        InputEvent::Button {
            name: "page-forward",
            state: ButtonState::Released,
        },
    );
    kernel
        .shared
        .settings
        .set(
            "core.keymap-preset",
            paperpaw_sdk::SettingValue::Text(Cow::Borrowed("core.standard")),
        )
        .unwrap();
    kernel.rebuild_keymap();
    press(&mut kernel, 1100, "page-forward");
    assert_eq!(*log.borrow(), ["a:core.next-page"]);

    // Unknown device defaults are reported.
    let mut kernel = Kernel::builder(DEVICE)
        .setting_default(&settings::KEYMAP_PRESET, "nope.preset".into())
        .build()
        .unwrap();
    assert_eq!(kernel.take_warnings().len(), 1);
}

#[test]
fn banded_frames_match_full_frames() {
    // Renders into a copy of the whole screen, as a panel controller keeps it.
    fn screen(kernel: &mut Kernel, copy: &mut paperpaw_graphics::Framebuffer) -> Vec<u8> {
        let mut bands = 0;
        kernel.render(&mut |top, rows| {
            bands += 1;
            copy.write_rows(top, rows);
        });
        assert!(bands > 0);
        copy.view().data.to_vec()
    }
    let build = |rows: Option<u16>| {
        let builder = Kernel::builder(DEVICE).items(
            overlay_plugin_manifest(),
            overlay_plugin(OverlayPlugin { log: log() }),
        );
        match rows {
            Some(rows) => builder.frame_rows(rows),
            None => builder,
        }
        .build()
        .unwrap()
    };
    let new_copy = || {
        let display = DEVICE.display;
        paperpaw_graphics::Framebuffer::new(display.width, display.height, display.format).unwrap()
    };
    let (mut full, mut banded) = (build(None), build(Some(16)));
    let (mut full_copy, mut banded_copy) = (new_copy(), new_copy());
    full.start(Instant(0));
    banded.start(Instant(0));
    assert_eq!(
        screen(&mut full, &mut full_copy),
        screen(&mut banded, &mut banded_copy)
    );
    // An overlay redraws only part of the screen, across band borders.
    for kernel in [&mut full, &mut banded] {
        for (at, state) in [(100, ButtonState::Pressed), (150, ButtonState::Released)] {
            kernel.handle(
                Instant(at),
                InputEvent::Button {
                    name: "menu",
                    state,
                },
            );
        }
    }
    assert_eq!(
        screen(&mut full, &mut full_copy),
        screen(&mut banded, &mut banded_copy)
    );
    assert!(banded.frame().height <= 16);
}

#[test]
fn turned_screens_are_made_anew_for_the_new_size() {
    let log = log();
    let mut kernel = Kernel::builder(DEVICE)
        .items(
            overlay_plugin_manifest(),
            overlay_plugin(OverlayPlugin { log: log.clone() }),
        )
        .build()
        .unwrap();
    kernel.start(Instant(0));
    for (at, state) in [(100, ButtonState::Pressed), (150, ButtonState::Released)] {
        kernel.handle(
            Instant(at),
            InputEvent::Button {
                name: "menu",
                state,
            },
        );
    }
    let open: Vec<Id> = kernel.stack.iter().map(|s| s.id.clone()).collect();
    let effects = kernel.resize(Instant(200), 120, 60);
    assert_eq!(
        effects.refresh().map(|(_, mode)| mode),
        Some(RefreshMode::Full)
    );
    assert_eq!(kernel.device().display.width, 120);
    assert_eq!(
        kernel.shared.place(Instant(200)).device().display.height,
        60
    );
    let mut size = (0, 0);
    kernel.render(&mut |_, rows| size = (rows.width, size.1 + rows.height));
    assert_eq!(size, (120, 60));
    // The same screens, in the same order.
    let reopened: Vec<Id> = kernel.stack.iter().map(|s| s.id.clone()).collect();
    assert_eq!(reopened, open);
    // The same size again (turned half way round): drawn whole.
    let effects = kernel.resize(Instant(300), 120, 60);
    assert_eq!(
        effects.refresh().map(|(_, mode)| mode),
        Some(RefreshMode::Full)
    );
}

#[test]
fn turned_bands_keep_their_memory() {
    let mut kernel = Kernel::builder(DEVICE)
        .items(
            overlay_plugin_manifest(),
            overlay_plugin(OverlayPlugin { log: log() }),
        )
        .frame_rows(32)
        .row_align(8)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    // Wider: fewer rows a band, still whole multiples of 8.
    kernel.resize(Instant(100), 150, 54);
    assert_eq!((kernel.frame().width, kernel.frame().height), (150, 16));
    let mut tops = Vec::new();
    kernel.render(&mut |top, rows| tops.push((top, rows.height)));
    assert_eq!(tops, [(0, 16), (16, 16), (32, 16), (48, 6)]);
    // Back: the rows it had.
    kernel.resize(Instant(200), 90, 90);
    assert_eq!(kernel.frame().height, 32);
}

#[test]
fn bands_pass_whole_aligned_rows_and_again() {
    let mut kernel = Kernel::builder(DEVICE)
        .items(
            overlay_plugin_manifest(),
            overlay_plugin(OverlayPlugin { log: log() }),
        )
        .frame_rows(16)
        .row_align(8)
        .build()
        .unwrap();
    let passed = |kernel: &mut Kernel, again: bool| {
        let mut parts = Vec::new();
        let mut out = |top: u16, rows: paperpaw_platform::FrameView<'_>| {
            parts.push((top, rows.height, rows.width))
        };
        if again {
            kernel.rerender(&mut out);
        } else {
            kernel.render(&mut out);
        }
        parts
    };
    kernel.start(Instant(0));
    passed(&mut kernel, false);
    // The overlay covers rows 45 to 90: passed from row 40, in bands of 16.
    for (at, state) in [(100, ButtonState::Pressed), (150, ButtonState::Released)] {
        kernel.handle(
            Instant(at),
            InputEvent::Button {
                name: "menu",
                state,
            },
        );
    }
    let parts = passed(&mut kernel, false);
    assert_eq!(
        parts,
        [(40, 8, 90), (48, 16, 90), (64, 16, 90), (80, 10, 90)]
    );
    // Drawn again the same, e.g. for the controller's memory of the frame shown.
    assert_eq!(passed(&mut kernel, true), parts);

    // Captured: the whole screen in bands, and what was rendered last
    // stays what is drawn again.
    let mut captured = Vec::new();
    assert!(kernel.capture(&mut |top, rows| captured.push((top, rows.height))));
    assert_eq!(captured.first(), Some(&(0, 16)));
    let height = captured.iter().map(|(_, h)| *h).sum::<u16>();
    assert_eq!(height, kernel.framebuffer.bounds().height);
    assert_eq!(passed(&mut kernel, true), parts);
    // With changes waiting, nothing is captured.
    kernel.frame.unrendered = Some(Rect::new(0, 0, 10, 10));
    assert!(!kernel.capture(&mut |_, _| panic!("captured with changes waiting")));
}

#[test]
fn the_power_button_puts_the_device_to_sleep_where_the_shell_can() {
    // Where the system sleeps on its own (Android), nothing happens.
    let mut kernel = Kernel::builder(DEVICE).build().unwrap();
    kernel.start(Instant(0));
    assert!(!kernel.shared.place(Instant(0)).caps().sleep);
    assert!(!press(&mut kernel, 100, "power").sleep());
    // Where the shell sleeps, it is told to.
    let mut kernel = Kernel::builder(DEVICE).sleep(true).build().unwrap();
    kernel.start(Instant(0));
    assert!(kernel.shared.place(Instant(0)).caps().sleep);
    assert!(press(&mut kernel, 100, "power").sleep());
    assert!(!press(&mut kernel, 200, "confirm").sleep());
}

#[test]
fn the_device_sleeps_by_itself_after_a_while_unused() {
    let mut kernel = Kernel::builder(DEVICE)
        .sleep(true)
        .setting_default(&settings::SLEEP_AFTER, 1)
        .build()
        .unwrap();
    let effects = kernel.start(Instant(0));
    // A wake-up is asked for when the minute is over.
    assert_eq!(effects.wake_at(), Some(Instant(60_000)));
    // Use puts it off.
    press(&mut kernel, 30_000, "confirm");
    assert!(!kernel.tick(Instant(60_000)).sleep());
    assert!(kernel.tick(Instant(90_100)).sleep());
    // Never, where the setting says so.
    let mut kernel = Kernel::builder(DEVICE)
        .sleep(true)
        .setting_default(&settings::SLEEP_AFTER, 0)
        .build()
        .unwrap();
    assert_eq!(kernel.start(Instant(0)).wake_at(), None);
    assert!(!kernel.tick(Instant(10_000_000)).sleep());
}

#[test]
fn speech_goes_to_the_shell_and_stops_with_its_screen() {
    let speech = |id: &str, queued: bool| {
        let params = paperpaw_sdk::Params::new().with("id", String::from(id));
        let speech = paperpaw_sdk::Speech::new("Hello", Id::new_static("ov.reply"));
        Ask::speak(speech.with(params).queued(queued))
    };
    // Without speech, the reply comes at once.
    let log = log();
    let mut kernel = Kernel::builder(DEVICE)
        .items(
            overlay_plugin_manifest(),
            overlay_plugin(OverlayPlugin { log: log.clone() }),
        )
        .build()
        .unwrap();
    kernel.start(Instant(0));
    assert!(!kernel.shared.place(Instant(0)).caps().speech);
    kernel.output.commands.push(speech("a", false));
    kernel.run_commands();
    assert!(kernel.take_speech().is_empty());
    assert!(log.borrow().contains(&String::from("event ov.reply a")));

    // With it: the shell speaks and says when it is done.
    let log = self::log();
    let mut kernel = Kernel::builder(DEVICE)
        .items(
            overlay_plugin_manifest(),
            overlay_plugin(OverlayPlugin { log: log.clone() }),
        )
        .speech(true)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel.output.commands.push(speech("b", false));
    kernel.run_commands();
    let calls = kernel.take_speech();
    let [SpeechCall::Say { id, text, rate, .. }] = calls.as_slice() else {
        panic!("{calls:?}");
    };
    assert_eq!((text.as_str(), *rate), ("Hello", 100));
    assert!(kernel.speaking());
    assert!(!log.borrow().contains(&String::from("event ov.reply b")));
    kernel.speech_done(Instant(5), *id, true);
    assert!(!kernel.speaking());
    assert!(log.borrow().contains(&String::from("event ov.reply b")));

    // Queued texts follow; each is answered when done.
    kernel
        .output
        .commands
        .extend([speech("q1", false), speech("q2", true)]);
    kernel.run_commands();
    let calls = kernel.take_speech();
    let ids: Vec<(u32, bool)> = calls
        .iter()
        .filter_map(|c| match c {
            SpeechCall::Say { id, queue, .. } => Some((*id, *queue)),
            SpeechCall::Stop => None,
        })
        .collect();
    assert_eq!(ids.iter().map(|i| i.1).collect::<Vec<_>>(), [false, true]);
    kernel.speech_done(Instant(6), ids[0].0, true);
    assert!(kernel.speaking());
    assert!(log.borrow().contains(&String::from("event ov.reply q1")));
    kernel.speech_done(Instant(7), ids[1].0, true);
    assert!(!kernel.speaking());
    assert!(log.borrow().contains(&String::from("event ov.reply q2")));

    // Started over the menu, it stops when the menu closes.
    kernel.output.commands.push(Ask::open_slot(slots::MENU));
    kernel.run_commands();
    kernel.output.commands.push(speech("c", false));
    kernel.run_commands();
    let Some(SpeechCall::Say { id, .. }) = kernel.take_speech().pop() else {
        panic!("no speech");
    };
    kernel.output.commands.push(Ask::Back);
    kernel.run_commands();
    assert_eq!(kernel.take_speech(), [SpeechCall::Stop]);
    assert!(!kernel.speaking());
    // An end reported late is not passed on.
    kernel.speech_done(Instant(9), id, true);
    assert!(!log.borrow().contains(&String::from("event ov.reply c")));
}
