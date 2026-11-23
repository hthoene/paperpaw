//! What the keyboard, the setup guide, the library and Files draw and read
//! for a key (#486), walked on the X4's buttons: a focus step within the page
//! draws the two places it leaves and reaches, typing draws the text field,
//! Files steps through a folder without asking the memory card, holding Back
//! finishes a password as it finishes any text, and a text the host does not
//! take stays in the keyboard.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::rc::Rc;

use paperpaw_core::sdk::MemoryStorage;
use paperpaw_emulator::{device::Device, headless::run_script, profiles};

const ALICE: &[u8] = include_bytes!("../../../samples/library/books/alice-in-wonderland.epub");

/// A started X4, set up unless `fresh`, with `files` on its card.
fn x4(files: MemoryStorage, fresh: bool) -> Device {
    let files = files.with("paperpaw.ini", "[help]\nshow = false\n");
    let files = if fresh {
        files
    } else {
        files.with(".paperpaw/plugins/setup/done", "")
    };
    paperpaw_emulator::budget::override_limit(None);
    let profile = profiles::find("xteink-x4").unwrap();
    let mut device = Device::in_memory(profile, None, Rc::new(files), true, None).unwrap();
    device.start();
    run_script(&mut device, "wait:3000").unwrap();
    device
}

fn screen(device: &mut Device) -> String {
    let mut screen = String::new();
    device.call(|kernel, _| {
        screen = kernel
            .active_screen()
            .map(ToString::to_string)
            .unwrap_or_default();
        paperpaw_core::Effects::default()
    });
    screen
}

/// What `steps` showed: the areas of the frames, none of them the whole
/// panel where only part was drawn.
fn shown_by(device: &mut Device, steps: &str) -> Vec<paperpaw_platform::Rect> {
    device.shown.clear();
    run_script(device, steps).unwrap();
    device.shown.iter().map(|(area, _)| *area).collect()
}

/// Whether every frame of `areas` drew a small part of the panel (less than
/// half: a screen drawn again whole is all of it but the status bar), and
/// one did.
fn in_part(device: &Device, areas: &[paperpaw_platform::Rect]) -> bool {
    let display = &device.profile.device.display;
    let panel = u32::from(display.width) * u32::from(display.height);
    let size = |area: &paperpaw_platform::Rect| u32::from(area.width) * u32::from(area.height);
    !areas.is_empty() && areas.iter().all(|area| size(area) < panel / 2)
}

#[test]
fn holding_back_finishes_a_password() {
    let mut device = x4(MemoryStorage::new(), false);
    run_script(
        &mut device,
        "open:keyboard.screen:title=Password:secret=true,wait:500,type:secret,wait:300",
    )
    .unwrap();
    assert_eq!(screen(&mut device), "keyboard.screen");
    run_script(&mut device, "long:back,wait:500").unwrap();
    assert_ne!(screen(&mut device), "keyboard.screen");
}

#[test]
fn typing_draws_the_text_field_only() {
    let mut device = x4(MemoryStorage::new(), false);
    run_script(&mut device, "open:keyboard.screen:title=Name,wait:500").unwrap();
    assert_eq!(screen(&mut device), "keyboard.screen");
    // The first letter and the next: only the field shows what changed.
    let typed = shown_by(&mut device, "type:a,wait:300,type:b,wait:300");
    assert!(in_part(&device, &typed), "{typed:?}");
}

#[test]
fn a_focus_step_in_the_setup_guide_draws_its_two_buttons() {
    let mut device = x4(MemoryStorage::new(), true);
    assert_eq!(screen(&mut device), "setup.guide");
    let stepped = shown_by(&mut device, "right,wait:300,right,wait:300");
    assert!(in_part(&device, &stepped), "{stepped:?}");
}

#[test]
fn a_focus_step_in_the_library_draws_its_two_books() {
    let mut files = MemoryStorage::new();
    for n in 0..3 {
        files = files.with(&format!("books/b{n}.epub"), ALICE);
    }
    let mut device = x4(files, false);
    run_script(&mut device, "wait:20000,open:files.browser,wait:1000").unwrap();
    assert_eq!(screen(&mut device), "files.browser");
    let stepped = shown_by(&mut device, "run:core.down,wait:300,run:core.down,wait:300");
    assert!(in_part(&device, &stepped), "{stepped:?}");
}

#[test]
fn files_steps_through_a_folder_without_the_card() {
    let mut files = MemoryStorage::new();
    for n in 0..4 {
        files = files.with(&format!("f{n}.txt"), "x");
    }
    let mut device = x4(files, false);
    run_script(&mut device, "open:file-manager.browser,wait:1000").unwrap();
    assert_eq!(screen(&mut device), "file-manager.browser");
    let before = paperpaw_emulator::trace::counts();
    let stepped = shown_by(
        &mut device,
        "run:core.down,wait:300,run:core.down,wait:300,run:core.up,wait:300",
    );
    let asked = paperpaw_emulator::trace::counts().since(&before);
    assert_eq!(asked.calls(), 0, "{asked}");
    assert!(in_part(&device, &stepped), "{stepped:?}");
}

/// The labels of what the screen shows.
fn labels(device: &mut Device) -> Vec<String> {
    let mut labels = Vec::new();
    device.call(|kernel, _| {
        labels = kernel.describe().into_iter().map(|p| p.label).collect();
        paperpaw_core::Effects::default()
    });
    labels
}

/// A text the host does not take stays in the keyboard, which says so (the
/// X4 keeps 2 KiB of text in memory: a longer note is refused): before, the
/// field was emptied and the keyboard stayed without a word.
#[test]
fn a_text_that_could_not_be_passed_on_stays_in_the_keyboard() {
    let mut device = x4(MemoryStorage::new(), false);
    run_script(&mut device, "open:keyboard.screen:title=Note,wait:500").unwrap();
    // A long note, typed in parts as a keyboard on a cable sends it.
    let part = "ä".repeat(300);
    for _ in 0..6 {
        run_script(&mut device, &format!("type:{part},wait:100")).unwrap();
    }
    run_script(&mut device, "long:back,wait:500").unwrap();
    assert_eq!(screen(&mut device), "keyboard.screen");
    let said = labels(&mut device);
    assert!(
        said.iter()
            .any(|l| l.starts_with("The text could not be passed on")),
        "{said:?}"
    );
    assert!(said.iter().any(|l| l.contains("ääää")), "{said:?}");
    // Back still leaves it.
    run_script(&mut device, "back,wait:500").unwrap();
    assert_ne!(screen(&mut device), "keyboard.screen");
}
