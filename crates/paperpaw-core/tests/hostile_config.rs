//! A broken or hostile `paperpaw.ini` (unknown sections, broken key map
//! lines, values out of range, syntax errors, huge files, bytes that are
//! not UTF-8) is read with defaults and warnings, never a panic, and
//! saving a setting does not lose what the user wrote.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]
// Tests panic on purpose when something is wrong.

mod common;

use common::AddDriver;

use common::{DEVICE, Harness, press, render};
use paperpaw_core::sdk::MemoryStorage;
use paperpaw_core::sdk::builtin::CONFIG_FILE;
use paperpaw_core::{Kernel, Warning, WarningSource};
use paperpaw_platform::Instant;
use proptest::prelude::*;

/// A kernel with the driver plugin and `config` as given by the shell.
fn with_config(harness: &Harness, config: &str) -> (Kernel, Vec<Warning>) {
    let mut kernel = Kernel::builder(DEVICE)
        .driver(harness.plugin())
        .user_config(config)
        .build()
        .unwrap();
    let warnings = kernel.take_warnings();
    kernel.start(Instant(0));
    render(&mut kernel);
    harness.take();
    (kernel, warnings)
}

/// A kernel with the driver plugin and `bytes` as `paperpaw.ini` on the
/// storage.
fn with_stored_config(harness: &Harness, bytes: &[u8]) -> (Kernel, Vec<Warning>) {
    let storage = MemoryStorage::new().with(CONFIG_FILE, bytes.to_vec());
    let mut kernel = Kernel::builder(DEVICE)
        .driver(harness.plugin())
        .storage(storage)
        .build()
        .unwrap();
    let warnings = kernel.take_warnings();
    kernel.start(Instant(0));
    render(&mut kernel);
    harness.take();
    (kernel, warnings)
}

fn user_lines(warnings: &[Warning]) -> Vec<u32> {
    warnings
        .iter()
        .filter(|w| w.source == WarningSource::UserConfig)
        .filter_map(|w| w.line)
        .collect()
}

#[test]
fn unknown_sections_and_settings_are_reported_and_the_rest_applies() {
    let harness = Harness::new();
    let config = "[nonsense]\nfoo = bar\n[core]\nno-such-setting = 1\nui-size = 12\n";
    let (kernel, warnings) = with_config(&harness, config);
    assert_eq!(kernel.settings().int("core.ui-size"), 12);
    assert_eq!(user_lines(&warnings), [2, 4], "{warnings:?}");
    assert_eq!(kernel.active_screen(), Some(&common::BASE));
}

#[test]
fn syntax_errors_are_reported_with_their_line() {
    let harness = Harness::new();
    let config = "just some words\n[unclosed\n[bad section!]\nkey with spaces = 1\n\
                  [core]\nui-size = \"12\nnight-mode = \"\\q\"\nflash-screens = off\n";
    let (kernel, warnings) = with_config(&harness, config);
    assert_eq!(user_lines(&warnings), [1, 2, 3, 4, 6, 7], "{warnings:?}");
    // What could be read is applied.
    assert!(!kernel.settings().bool("core.flash-screens"));
    assert!(!kernel.settings().bool("core.night-mode"));
}

#[test]
fn values_out_of_range_or_of_the_wrong_kind_keep_their_defaults() {
    let harness = Harness::new();
    let config = "[core]\n\
                  ui-size = 99999\n\
                  night-mode = maybe\n\
                  full-refresh-interval = -5\n\
                  long-press-ms = 18446744073709551616\n\
                  notice-seconds = 0x10\n\
                  rotation = core.rotation.sideways\n\
                  language = de de\n\
                  keymap-preset = core.no-such-preset\n";
    let (kernel, warnings) = with_config(&harness, config);
    let settings = kernel.settings();
    assert_eq!(settings.int("core.ui-size"), 8);
    assert!(!settings.bool("core.night-mode"));
    assert_eq!(settings.int("core.full-refresh-interval"), 6);
    assert_eq!(settings.int("core.long-press-ms"), 500);
    assert_eq!(settings.int("core.notice-seconds"), 4);
    assert_eq!(settings.text("core.keymap-preset"), "core.standard");
    assert_eq!(
        user_lines(&warnings),
        [2, 3, 4, 5, 6, 7, 8, 9],
        "{warnings:?}"
    );
}

#[test]
fn extreme_numbers_at_the_edges_of_their_range_are_taken() {
    let harness = Harness::new();
    let config = "[core]\nlong-press-ms = 200\nfull-refresh-interval = 0\nui-size = 24\n";
    let (mut kernel, warnings) = with_config(&harness, config);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(kernel.settings().int("core.long-press-ms"), 200);
    // No periodic full refresh: many page turns stay partial.
    for i in 0..30 {
        press(&mut kernel, 1000 + i * 100, "page-forward");
    }
    assert_eq!(
        harness.lines("base:action core.next-page").len(),
        30,
        "{:?}",
        harness.log.borrow()
    );
}

#[test]
fn colour_codes_that_are_no_colours_show_black_on_white() {
    let harness = Harness::new();
    for (text, paper) in [
        ("#zzzzzz", "\u{e4}\u{e4}\u{e4}"),
        ("#12345", "#1234567"),
        ("", "#"),
        ("\u{e4}\u{e4}", "#\u{e4}\u{e4}\u{e4}"),
    ] {
        let config = format!(
            "[core]\ntext-color = core.color.own\ntext-color-own = \"{text}\"\n\
             paper-color = core.paper.own\npaper-color-own = \"{paper}\"\n"
        );
        let mut kernel = Kernel::builder(DEVICE)
            .driver(harness.plugin())
            .colors(true)
            .user_config(config)
            .build()
            .unwrap();
        assert_eq!(
            kernel.colors(),
            [[0, 0, 0], [255, 255, 255]],
            "{text} {paper}"
        );
        kernel.start(Instant(0));
        render(&mut kernel);
    }
    // Short codes are taken.
    let kernel = Kernel::builder(DEVICE)
        .driver(harness.plugin())
        .colors(true)
        .user_config("[core]\ntext-color = core.color.own\ntext-color-own = #f00\n")
        .build()
        .unwrap();
    assert_eq!(kernel.colors()[0], [255, 0, 0]);
}

#[test]
fn a_rotation_that_is_none_keeps_the_screen_upright() {
    let harness = Harness::new();
    let mut kernel = Kernel::builder(DEVICE)
        .driver(harness.plugin())
        .rotation(true)
        .user_config("[core]\nrotation = core.rotation.45\n")
        .build()
        .unwrap();
    assert_eq!(kernel.rotation(), paperpaw_platform::Rotation::Deg0);
    assert_eq!(user_lines(&kernel.take_warnings()), [2]);
}

#[test]
fn settings_before_any_section_are_reported() {
    let harness = Harness::new();
    let (kernel, warnings) = with_config(&harness, "ui-size = 12\n[core]\nui-size = 11\n");
    assert_eq!(kernel.settings().int("core.ui-size"), 11);
    assert_eq!(user_lines(&warnings), [1]);
}

#[test]
fn a_key_given_twice_takes_the_last_value() {
    let harness = Harness::new();
    let (kernel, warnings) = with_config(
        &harness,
        "[core]\nui-size = 10\nui-size = 13\n[core]\nui-size = 14\n",
    );
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(kernel.settings().int("core.ui-size"), 14);
}

#[test]
fn broken_key_map_lines_are_reported_and_the_others_bind() {
    let harness = Harness::new();
    let config = "[keymap.global]\n\
                  button. = core.menu\n\
                  button.UPPER = core.menu\n\
                  button.page-forward = not an id!\n\
                  tap.nowhere = core.menu\n\
                  swipe.diagonal = core.menu\n\
                  button.confirm = none\n\
                  button.page-back = core.next-page\n\
                  [keymap.]\n\
                  button.up = core.menu\n\
                  [keymap.Bad Layer]\n\
                  button.up = core.menu\n";
    let (mut kernel, warnings) = with_config(&harness, config);
    let lines = user_lines(&warnings);
    for line in [2, 3, 4, 5, 6, 10] {
        assert!(
            lines.contains(&line),
            "line {line} not reported: {warnings:?}"
        );
    }
    // The broken line left the default binding.
    press(&mut kernel, 1000, "page-forward");
    // `none` unbinds.
    press(&mut kernel, 2000, "confirm");
    // A good line among broken ones binds.
    press(&mut kernel, 3000, "page-back");
    assert_eq!(
        harness.lines("base:action"),
        ["base:action core.next-page", "base:action core.next-page"]
    );
}

#[test]
fn bindings_to_unknown_actions_are_reported_and_do_nothing() {
    let harness = Harness::new();
    let config = "[keymap.global]\nbutton.page-forward = nobody.nothing\n";
    let (mut kernel, warnings) = with_config(&harness, config);
    assert!(
        warnings
            .iter()
            .any(|w| w.message.contains("nobody.nothing")),
        "{warnings:?}"
    );
    press(&mut kernel, 1000, "page-forward");
    assert_eq!(kernel.active_screen(), Some(&common::BASE));
    // Pressing it again warns at run time, nothing else.
    let runtime: Vec<Warning> = kernel.take_warnings();
    assert!(
        runtime
            .iter()
            .all(|w| w.source == WarningSource::Runtime && w.message.contains("nobody.nothing")),
        "{runtime:?}"
    );
}

#[test]
fn a_broken_device_key_map_is_reported_as_the_devices() {
    let harness = Harness::new();
    let mut kernel = Kernel::builder(DEVICE)
        .driver(harness.plugin())
        .device_keymap("[keymap.global]\nbutton.x = !\n[core]\nui-size = 3\nnot a line\n")
        .build()
        .unwrap();
    let warnings = kernel.take_warnings();
    let device: Vec<Option<u32>> = warnings
        .iter()
        .filter(|w| w.source == WarningSource::DeviceKeymap)
        .map(|w| w.line)
        .collect();
    assert_eq!(device, [Some(2), Some(4), Some(5)], "{warnings:?}");
    // A key map is no place for settings.
    assert_eq!(kernel.settings().int("core.ui-size"), 8);
    kernel.start(Instant(0));
    assert_eq!(kernel.active_screen(), Some(&common::BASE));
}

#[test]
fn invalid_slots_menus_and_fonts_are_reported() {
    let harness = Harness::new();
    let config = "[slots]\ncore.home = Not An Id\ncore.nowhere = drv.full\ncore.home = drv.nothing\n\
                  [menus]\ncore.main = core.back, !!, core.home\n\
                  [fonts]\ncore.ui = nobody.font\nBad Role = core.font\n";
    let (kernel, warnings) = with_config(&harness, config);
    let lines = user_lines(&warnings);
    for line in [2, 6, 8, 9] {
        assert!(
            lines.contains(&line),
            "line {line} not reported: {warnings:?}"
        );
    }
    let runtime: Vec<&str> = warnings
        .iter()
        .filter(|w| w.source == WarningSource::Runtime)
        .map(|w| w.message.as_str())
        .collect();
    assert!(
        runtime
            .iter()
            .any(|m| m.contains("unknown slot `core.nowhere`")),
        "{runtime:?}"
    );
    assert!(
        runtime.iter().any(|m| m.contains("drv.nothing")),
        "{runtime:?}"
    );
    // The home slot falls back to the screen that is there.
    assert_eq!(kernel.active_screen(), Some(&common::BASE));
}

#[test]
fn the_kernel_cannot_be_switched_off_and_unknown_plugins_are_harmless() {
    let harness = Harness::new();
    let config = "[plugins]\ncore = off\nnobody = off\ndrv = maybe\n";
    let (mut kernel, _) = with_config(&harness, config);
    assert!(kernel.plugins().get("core").is_some_and(|p| p.enabled));
    assert!(kernel.plugins().get("drv").is_some_and(|p| p.enabled));
    press(&mut kernel, 1000, "page-forward");
    assert_eq!(harness.lines("base:action"), ["base:action core.next-page"]);
}

#[test]
fn a_huge_config_from_the_shell_is_read() {
    let harness = Harness::new();
    let mut config = String::from("[core]\n");
    for i in 0..20_000 {
        config.push_str(&format!("# comment {i}\nui-size = {}\n", 7 + i % 18));
    }
    config.push_str("[keymap.global]\n");
    for _ in 0..5_000 {
        config.push_str("button.page-back = core.next-page\n");
    }
    config.push_str(&"x".repeat(200_000));
    config.push('\n');
    config.push_str("[core]\nui-size = 16\n");
    let (mut kernel, warnings) = with_config(&harness, &config);
    assert_eq!(kernel.settings().int("core.ui-size"), 16);
    // The one line of `x` is the only problem.
    assert_eq!(user_lines(&warnings).len(), 1, "{warnings:?}");
    press(&mut kernel, 1000, "page-back");
    assert_eq!(harness.lines("base:action"), ["base:action core.next-page"]);
}

#[test]
fn a_stored_config_with_crlf_lines_is_read() {
    let harness = Harness::new();
    let (kernel, warnings) =
        with_stored_config(&harness, b"[core]\r\nui-size = 12\r\nnight-mode = on\r\n");
    assert_eq!(kernel.settings().int("core.ui-size"), 12);
    assert!(kernel.settings().bool("core.night-mode"));
    assert!(warnings.is_empty(), "{warnings:?}");
}

/// Editors on Windows (Notepad before 2019, and others) start UTF-8 files
/// with a byte order mark.
#[test]
fn a_stored_config_with_a_byte_order_mark_is_read() {
    let harness = Harness::new();
    let (kernel, warnings) = with_stored_config(
        &harness,
        b"\xEF\xBB\xBF[core]\nui-size = 12\nnight-mode = on\n",
    );
    assert_eq!(kernel.settings().int("core.ui-size"), 12, "{warnings:?}");
    assert!(kernel.settings().bool("core.night-mode"));
    assert!(warnings.is_empty(), "{warnings:?}");
}

#[test]
fn a_stored_config_that_is_not_utf8_starts_with_defaults() {
    let harness = Harness::new();
    // Latin-1 in a comment, as an editor on another system might save it.
    let (kernel, _) = with_stored_config(&harness, b"# Gr\xFC\xDFe\n[core]\nui-size = 12\n");
    assert_eq!(kernel.active_screen(), Some(&common::BASE));
}

#[test]
fn a_stored_config_that_is_not_utf8_is_reported() {
    let harness = Harness::new();
    let (_, warnings) = with_stored_config(&harness, b"# Gr\xFC\xDFe\n[core]\nui-size = 12\n");
    assert!(
        warnings.iter().any(|w| w.message.contains(CONFIG_FILE)),
        "nothing tells the user that the whole file was ignored: {warnings:?}"
    );
}

#[test]
fn saving_a_setting_keeps_the_rest_of_a_config_that_is_not_utf8() {
    let harness = Harness::new();
    let original = b"# Gr\xFC\xDFe\n[core]\nui-size = 12\n[keymap.global]\nbutton.up = core.menu\n";
    let (mut kernel, _) = with_stored_config(&harness, original);
    kernel
        .set_setting(Instant(10), "core.night-mode", "on")
        .unwrap();
    let saved = paperpaw_core::sdk::read_file(kernel.storage(), CONFIG_FILE, 1 << 20).unwrap();
    let text = String::from_utf8_lossy(&saved);
    assert!(
        text.contains("ui-size = 12") && text.contains("button.up = core.menu"),
        "the user's other lines are gone: {text:?}"
    );
}

/// More than the 64 KiB the kernel reads of `paperpaw.ini`.
fn oversized_config() -> Vec<u8> {
    let mut config = String::from("[core]\nui-size = 12\n");
    while config.len() <= 70 * 1024 {
        config.push_str("# a long comment that pads the file out to more than it reads\n");
    }
    config.into_bytes()
}

#[test]
fn a_stored_config_too_large_to_read_starts_with_defaults() {
    let harness = Harness::new();
    let (kernel, _) = with_stored_config(&harness, &oversized_config());
    assert_eq!(kernel.active_screen(), Some(&common::BASE));
}

#[test]
fn a_stored_config_too_large_to_read_is_reported() {
    let harness = Harness::new();
    let (_, warnings) = with_stored_config(&harness, &oversized_config());
    assert!(
        warnings.iter().any(|w| w.message.contains(CONFIG_FILE)),
        "nothing tells the user that the whole file was ignored: {warnings:?}"
    );
}

#[test]
fn saving_a_setting_keeps_the_rest_of_a_config_too_large_to_read() {
    let harness = Harness::new();
    let (mut kernel, _) = with_stored_config(&harness, &oversized_config());
    kernel
        .set_setting(Instant(10), "core.night-mode", "on")
        .unwrap();
    let saved = paperpaw_core::sdk::read_file(kernel.storage(), CONFIG_FILE, 1 << 20).unwrap();
    let text = String::from_utf8_lossy(&saved);
    assert!(
        text.contains("ui-size = 12"),
        "the user's other lines are gone: {} bytes left",
        saved.len()
    );
}

/// Lines a config might have, good and broken, to build configs from.
fn config_line() -> impl Strategy<Value = String> {
    prop_oneof![
        Just(String::from("[core]")),
        Just(String::from("[keymap.global]")),
        Just(String::from("[keymap.drv.base]")),
        Just(String::from("[plugins]")),
        Just(String::from("[slots]")),
        Just(String::from("[menus]")),
        Just(String::from("[fonts]")),
        Just(String::from("[drv]")),
        Just(String::from("[")),
        Just(String::from("]")),
        Just(String::from("=")),
        Just(String::from("\"")),
        Just(String::from("ui-size = 30")),
        Just(String::from("long-press-ms = 0")),
        Just(String::from("long-press-ms = -9223372036854775808")),
        Just(String::from("full-refresh-interval = 1")),
        Just(String::from("edge-ends = 25")),
        Just(String::from("night-mode = on")),
        Just(String::from("language = \"\"")),
        Just(String::from("keymap-preset = core.two-buttons")),
        Just(String::from("rotation = core.rotation.90")),
        Just(String::from("button.page-forward = none")),
        Just(String::from("button.back = core.menu")),
        Just(String::from("long.page-forward = core.home")),
        Just(String::from("tap.center = core.back")),
        Just(String::from("edge.left = core.nothing")),
        Just(String::from("gestures.policy = core.gestures.off")),
        Just(String::from("core.home = drv.overlay")),
        Just(String::from("core.menu = drv.full")),
        Just(String::from("core.main = core.back, core.home")),
        Just(String::from("drv = off")),
        Just(String::from("core = off")),
        "[a-z.\\-@]{0,12} ?= ?[ -~]{0,20}",
        "\"(\\\\[nt\"\\\\u{]|[^\"\\\\]){0,10}\"?",
        any::<String>(),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig { failure_persistence: None, ..ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(96)) })]

    /// Any config built of such lines starts a kernel with a screen.
    #[test]
    fn any_config_starts_a_kernel_with_a_screen(lines in prop::collection::vec(config_line(), 0..40)) {
        let harness = Harness::new();
        let config = lines.join("\n");
        let (mut kernel, _) = with_config(&harness, &config);
        prop_assert!(kernel.active_screen().is_some());
        // And it takes input.
        press(&mut kernel, 1000, "page-forward");
        press(&mut kernel, 2000, "menu");
        press(&mut kernel, 3000, "back");
        prop_assert!(kernel.active_screen().is_some());
    }

    /// Any bytes as the stored config start a kernel with a screen.
    #[test]
    fn any_stored_bytes_start_a_kernel_with_a_screen(bytes in prop::collection::vec(any::<u8>(), 0..2048)) {
        let harness = Harness::new();
        let (kernel, _) = with_stored_config(&harness, &bytes);
        prop_assert!(kernel.active_screen().is_some());
    }
}
