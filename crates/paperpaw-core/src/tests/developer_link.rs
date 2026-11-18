//! The developer link over USB (#12): every rule of `developer_link.rs`
//! is refused or kept here, each test failing without the rule.
#![allow(clippy::unwrap_used, reason = "fixture errors fail tests")]

use super::*;
use crate::developer_link::{self as link, Use};
use crate::native_host::firmware_confirm::{ANSWER, ANSWERED, NONCE, SCREEN};
use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::{Canvas, Handled, MemoryStorage, Params, Priority, Version};

const ROW: &str = "core.usb-link";

fn fill(bytes: &mut [u8]) -> bool {
    for (at, byte) in bytes.iter_mut().enumerate() {
        *byte = (at as u8).wrapping_mul(37).wrapping_add(11);
    }
    true
}

struct Page;
impl crate::testing::Fake for Page {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Place<'_>) {}
    fn on_action(&mut self, _: &Id, _: &mut Turn<'_, '_>) -> Handled {
        Handled::No
    }
}

/// A kernel with a home screen (the question lies over it), a card with
/// what a reader keeps on it, and a source of random bytes.
fn kernel_with(card: MemoryStorage) -> Kernel {
    let mut kernel = Kernel::builder(DEVICE)
        .storage(card)
        .random(fill)
        .items(
            Manifest::new(Id::new_static("h"), "Home", Version::new(1, 0, 0))
                .uses(paperpaw_sdk::Uses::NOTHING),
            |r: &mut Staging| {
                r.add_fake(crate::testing::FakeSpec::new(
                    Id::new_static("h.home"),
                    "home",
                    alloc::boxed::Box::new(|_, _| alloc::boxed::Box::new(Page)),
                ))?;
                r.bind_slot(
                    paperpaw_sdk::builtin::slots::HOME,
                    Id::new_static("h.home"),
                    Priority::DEFAULT,
                )
            },
        )
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel
}

fn card() -> MemoryStorage {
    MemoryStorage::new()
        .with("books/a.epub", b"a book".to_vec())
        .with(".paperpaw/secrets.txt", b"hunter2".to_vec())
        .with("wifi.ini", b"[net]\npassword = hunter2\n".to_vec())
        .with("paperpaw.ini", b"[core]\nlanguage = en\n".to_vec())
        .with("plugins/demo/plugin.ini", b"[plugin]\n".to_vec())
        .with("plugins/demo/secrets.txt", b"hunter2".to_vec())
        .with("plugins/code/code.ppn", b"code".to_vec())
        .with("screenshots/s.png", b"png".to_vec())
}

fn kernel() -> Kernel {
    kernel_with(card())
}

/// The reader's own press on the question: `left` moves to *Allow*.
fn press(kernel: &mut Kernel, at: u64, action: Id) {
    let _ = kernel.run_action(Instant(at), action);
}

/// The reader switches the row on and allows (a moment after the question
/// shows, as input counts only then).
fn allow(kernel: &mut Kernel) {
    ask(kernel, 10);
    press(kernel, 710, actions::LEFT);
    press(kernel, 720, actions::CONFIRM);
    assert!(kernel.developer_on());
}

/// The reader switches the row on at `at`; the question shows.
fn ask(kernel: &mut Kernel, at: u64) {
    kernel.shared.reader = true;
    kernel.set_setting(Instant(at), ROW, "on").unwrap();
    let _ = kernel.tick(Instant(at));
    assert_eq!(kernel.stack.last().map(|s| s.id.clone()), Some(SCREEN));
}

fn send(kernel: &mut Kernel, at: u64, line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let _ = kernel.developer_line(Instant(at), line, (1000, 2000), &mut |l| out.push(l.into()));
    out
}

fn said(out: &[String]) -> String {
    out.join("|")
}

#[test]
fn it_is_off_until_the_reader_allows_it_on_the_device() {
    let mut kernel = kernel();
    assert!(!kernel.developer_on());
    for line in ["hello", "ping", "key confirm", "get books/a.epub", "shot"] {
        let out = send(&mut kernel, 1000, &alloc::format!("DEV/1 1 {line}"));
        assert_eq!(said(&out), "DEV/1 1 err off", "{line}");
        // Said once a second at most: no flood of answers.
        assert!(send(&mut kernel, 1100, "DEV/1 2 ping").is_empty());
        kernel.developer.said_off = None;
    }
    // Asking is the row's; a command cannot ask.
    assert!(kernel.stack.iter().all(|s| s.id != SCREEN));
}

#[test]
fn only_the_screen_answers_the_question_not_an_event_nor_a_key_over_the_link() {
    let mut kernel = kernel();
    ask(&mut kernel, 10);
    let nonce = kernel.developer.asked.expect("a question is shown");
    assert!(!kernel.developer_on());
    // The channel is off while the question shows: no key answers it.
    assert_eq!(
        said(&send(&mut kernel, 2000, "DEV/1 1 key left")),
        "DEV/1 1 err off"
    );
    // Anyone can send the event; not even with the right nonce.
    let forged = Params::new()
        .with(NONCE, nonce.to_string())
        .with(ANSWER, "yes");
    kernel.emit(&ANSWERED, &forged);
    assert!(!kernel.developer_on());
    // The reader answers on the screen: Allow.
    press(&mut kernel, 710, actions::LEFT);
    press(&mut kernel, 720, actions::CONFIRM);
    assert!(kernel.developer_on());
    assert!(kernel.settings().bool(ROW), "the row shows it is on");
    assert_eq!(said(&send(&mut kernel, 3000, "DEV/1 1 ping")), "DEV/1 1 ok");
}

#[test]
fn no_and_back_leave_it_off() {
    let mut kernel = kernel();
    ask(&mut kernel, 10);
    // The focus starts on *Don't allow*.
    press(&mut kernel, 710, actions::CONFIRM);
    assert!(!kernel.developer_on());
    assert!(!kernel.settings().bool(ROW));
    ask(&mut kernel, 1000);
    press(&mut kernel, 1700, actions::BACK);
    assert!(!kernel.developer_on());
}

#[test]
fn a_plugin_cannot_ask_and_a_file_cannot_switch_it_on() {
    // Not the reader's press: no question.
    let mut kernel = kernel();
    kernel.shared.reader = false;
    kernel.output.commands.push(Ask::SetSetting(
        Id::new_static(ROW),
        paperpaw_sdk::SettingValue::Bool(true),
    ));
    kernel.run_commands();
    assert!(kernel.stack.iter().all(|s| s.id != SCREEN));
    // A card that says it is on: off after the restart, row and all.
    let on = MemoryStorage::new().with("paperpaw.ini", b"[core]\nusb-link = on\n".to_vec());
    let mut kernel = kernel_with(on);
    assert!(!kernel.developer_on());
    assert!(!kernel.settings().bool(ROW));
    assert_eq!(
        said(&send(&mut kernel, 5, "DEV/1 1 hello")),
        "DEV/1 1 err off"
    );
}

#[test]
fn it_is_off_after_a_restart_and_after_sleep() {
    let mut kernel = kernel();
    allow(&mut kernel);
    // A restart is a new kernel: the state is RAM only.
    let mut fresh = kernel_with(card());
    assert!(!fresh.developer_on());
    assert_eq!(
        said(&send(&mut fresh, 5, "DEV/1 1 ping")),
        "DEV/1 1 err off"
    );
    // Sleep ends it and says why, once.
    let _ = kernel.suspend(Instant(9000));
    assert!(!kernel.developer_on());
    assert!(!kernel.settings().bool(ROW));
    assert_eq!(kernel.take_developer_notice(), Some("sleep"));
    assert_eq!(kernel.take_developer_notice(), None);
}

#[test]
fn it_ends_when_the_row_is_switched_off_the_cable_is_pulled_or_it_is_quiet() {
    let mut kernel = kernel();
    allow(&mut kernel);
    kernel.shared.reader = true;
    kernel.set_setting(Instant(300), ROW, "off").unwrap();
    assert!(!kernel.developer_on());
    allow(&mut kernel);
    let _ = kernel.developer_close(Instant(400), "unplugged");
    assert!(!kernel.developer_on());
    assert_eq!(kernel.take_developer_notice(), Some("unplugged"));
    allow(&mut kernel);
    let _ = kernel.developer_poll(Instant(720 + link::IDLE_MS - 1));
    assert!(kernel.developer_on());
    let _ = kernel.developer_poll(Instant(720 + link::IDLE_MS));
    assert!(!kernel.developer_on());
    assert_eq!(kernel.take_developer_notice(), Some("idle"));
    // A command keeps it from being quiet.
    allow(&mut kernel);
    let _ = send(&mut kernel, 500_000, "DEV/1 1 ping");
    let _ = kernel.developer_poll(Instant(500_000 + link::IDLE_MS - 1));
    assert!(kernel.developer_on());
}

#[test]
fn secrets_and_everything_outside_the_folders_are_refused() {
    let mut kernel = kernel();
    allow(&mut kernel);
    let refused = [
        ".paperpaw/secrets.txt",
        "/.paperpaw/secrets.txt",
        ".PaperPaw/secrets.txt",
        ".paperpaw",
        "wifi.ini",
        "paperpaw.ini",
        "books/../wifi.ini",
        "books/../.paperpaw/secrets.txt",
        "books//a.epub",
        "books/./a.epub",
        "books/a.epub.",
        "PAPERP~1/SECRETS.TXT",
        "books/PAPERP~1",
        "plugins/demo/secrets.txt",
        "plugins/demo/Secrets.TXT",
        "plugins/demo/wifi.ini",
        "plugins/demo/token.key",
        "plugins/demo/ſecret.txt",
        "books\\..\\wifi.ini",
        "screenshots/../wifi.ini",
        "elsewhere/file",
    ];
    for path in refused {
        for verb in ["get", "md5", "ls"] {
            let out = send(
                &mut kernel,
                10_000,
                &alloc::format!("DEV/1 7 {verb} {path}"),
            );
            assert_eq!(said(&out), "DEV/1 7 err denied", "{verb} {path:?}");
            // Not too many refusals in a minute here: start over.
            kernel.developer.switch_on(10_000);
        }
    }
    // What the folders hold is there.
    let out = send(&mut kernel, 20_000, "DEV/1 8 get books/a.epub");
    assert_eq!(
        said(&out),
        "DEV/1 8 size 6|DEV/1 8 data 612062 6f6f6b|DEV/1 8 ok"
            .replace("612062 6f6f6b", "6120626f6f6b")
    );
    let out = send(&mut kernel, 20_100, "DEV/1 9 get /screenshots/s.png");
    assert!(said(&out).ends_with("ok"), "{out:?}");
    let out = send(&mut kernel, 20_200, "DEV/1 9 md5 books/a.epub");
    assert!(said(&out).starts_with("DEV/1 9 ok "), "{out:?}");
}

#[test]
fn a_listing_leaves_secrets_out_and_the_top_names_the_folders() {
    let mut kernel = kernel();
    allow(&mut kernel);
    let out = send(&mut kernel, 1000, "DEV/1 1 ls plugins/demo");
    let text = said(&out);
    assert!(text.contains("plugin.ini"), "{text}");
    assert!(!text.contains("secrets"), "{text}");
    let out = send(&mut kernel, 1100, "DEV/1 2 ls /");
    assert_eq!(
        said(&out),
        "DEV/1 2 entry d 0 books|DEV/1 2 entry d 0 plugins|DEV/1 2 entry d 0 screenshots|DEV/1 2 ok"
    );
}

#[test]
fn writes_go_to_the_folders_never_over_a_book_never_code() {
    let mut kernel = kernel();
    allow(&mut kernel);
    // A new book, whole.
    assert_eq!(
        said(&send(&mut kernel, 1000, "DEV/1 1 put 4 books/new.epub")),
        "DEV/1 1 ready"
    );
    assert_eq!(
        said(&send(&mut kernel, 1010, "DEV/1 1 data 6e657721")),
        "DEV/1 1 ok 4"
    );
    assert_eq!(kernel.storage().size("books/new.epub").unwrap(), 4);
    assert!(!kernel.storage().exists("books/new.epub.pp-put"));
    // Over a book: no.
    assert_eq!(
        said(&send(&mut kernel, 1100, "DEV/1 2 put 1 books/a.epub")),
        "DEV/1 2 err exists"
    );
    // Over a file of a data plugin's folder: yes, and only whole.
    let _ = send(&mut kernel, 1200, "DEV/1 3 put 3 plugins/demo/plugin.ini");
    let _ = send(&mut kernel, 1210, "DEV/1 3 data 616263");
    assert_eq!(kernel.storage().size("plugins/demo/plugin.ini").unwrap(), 3);
    // Code, firmware, a code plugin's folder, the screenshots, the folder itself.
    for path in [
        "plugins/demo/code.ppn",
        "plugins/demo/lib.so",
        "plugins/demo/plugin-code.txt",
        "books/update.bin",
        "plugins/code/plugin.ini",
        "screenshots/new.png",
        "plugins",
        "plugins/demo/secrets.txt",
        ".paperpaw/new.txt",
        "wifi.ini",
    ] {
        kernel.developer.switch_on(2000);
        let out = send(&mut kernel, 2000, &alloc::format!("DEV/1 4 put 1 {path}"));
        assert_eq!(said(&out), "DEV/1 4 err denied", "{path}");
    }
    assert_eq!(kernel.storage().size("plugins/code/code.ppn").unwrap(), 4);
    assert!(!kernel.storage().exists("plugins/demo/code.ppn"));
}

#[test]
fn a_put_that_is_wrong_or_cut_off_leaves_nothing() {
    let mut kernel = kernel();
    allow(&mut kernel);
    let _ = send(&mut kernel, 1000, "DEV/1 1 put 2 books/x.epub");
    assert_eq!(
        said(&send(&mut kernel, 1010, "DEV/1 1 data 616263")),
        "DEV/1 1 err too much"
    );
    assert!(!kernel.storage().exists("books/x.epub"));
    assert!(!kernel.storage().exists("books/x.epub.pp-put"));
    // Another command in between ends it.
    let _ = send(&mut kernel, 1100, "DEV/1 2 put 9 books/y.epub");
    assert_eq!(
        said(&send(&mut kernel, 1110, "DEV/1 2 ping")),
        "DEV/1 2 err put aborted"
    );
    assert!(!kernel.storage().exists("books/y.epub.pp-put"));
    // Silence gives it up.
    let _ = send(&mut kernel, 1200, "DEV/1 3 put 9 books/z.epub");
    assert!(kernel.storage().exists("books/z.epub.pp-put"));
    let _ = kernel.developer_poll(Instant(1200 + link::QUIET_MS));
    assert!(!kernel.storage().exists("books/z.epub.pp-put"));
    // Too long a file, bad numbers.
    assert_eq!(
        said(&send(
            &mut kernel,
            1300,
            "DEV/1 4 put 99999999 books/big.epub"
        )),
        "DEV/1 4 err too big"
    );
    assert_eq!(
        said(&send(&mut kernel, 1310, "DEV/1 5 put x books/a")),
        "DEV/1 5 err bad size"
    );
}

#[test]
fn only_what_the_link_wrote_in_a_plugin_folder_is_removed() {
    let mut kernel = kernel();
    allow(&mut kernel);
    for path in [
        "plugins/demo/plugin.ini",
        "books/a.epub",
        "plugins/demo/secrets.txt",
        "plugins/code/code.ppn",
    ] {
        kernel.developer.switch_on(1000);
        assert_eq!(
            said(&send(
                &mut kernel,
                1000,
                &alloc::format!("DEV/1 1 rm {path}")
            )),
            "DEV/1 1 err denied",
            "{path}"
        );
        assert!(kernel.storage().exists(path), "{path}");
    }
    let _ = send(&mut kernel, 2000, "DEV/1 2 put 1 plugins/demo/new.txt");
    let _ = send(&mut kernel, 2010, "DEV/1 2 data 61");
    assert_eq!(
        said(&send(&mut kernel, 2100, "DEV/1 3 rm plugins/demo/new.txt")),
        "DEV/1 3 ok"
    );
    assert!(!kernel.storage().exists("plugins/demo/new.txt"));
}

#[test]
fn commands_are_rate_limited_and_probing_ends_the_link() {
    let mut kernel = kernel();
    allow(&mut kernel);
    let busy = (0..30)
        .filter(|n| said(&send(&mut kernel, 5000 + n, "DEV/1 1 ping")) == "DEV/1 1 err busy")
        .count();
    assert_eq!(busy, 10, "20 in a second");
    // Ten refusals in a minute: a program that probes.
    let mut kernel = self::kernel();
    allow(&mut kernel);
    for n in 0..10u64 {
        let _ = send(&mut kernel, 10_000 + n * 100, "DEV/1 1 get wifi.ini");
    }
    assert!(!kernel.developer_on());
    assert_eq!(kernel.take_developer_notice(), Some("probing"));
    assert!(!kernel.settings().bool(ROW));
}

#[test]
fn keys_do_not_reach_a_question_or_a_lock() {
    let mut kernel = kernel();
    allow(&mut kernel);
    assert_eq!(
        said(&send(&mut kernel, 1000, "DEV/1 1 key down")),
        "DEV/1 1 ok"
    );
    assert_eq!(
        said(&send(&mut kernel, 1010, "DEV/1 2 key power")),
        "DEV/1 2 err not a key"
    );
    // A question of the device's own (an install, a restart) is the reader's.
    let said_texts = crate::native_host::firmware_confirm::Texts {
        title: "Install?".into(),
        body: "Body".into(),
        yes: "Yes".into(),
        no: "No".into(),
        data: None,
    };
    let params = crate::native_host::firmware_confirm::open_params(&said_texts, 5);
    kernel.open(SCREEN, None, &params);
    assert_eq!(kernel.stack.last().map(|s| s.id.clone()), Some(SCREEN));
    for line in ["key confirm", "key left", "run core.confirm"] {
        let out = send(&mut kernel, 1100, &alloc::format!("DEV/1 3 {line}"));
        assert_eq!(said(&out), "DEV/1 3 err question", "{line}");
    }
    assert_eq!(kernel.stack.last().map(|s| s.id.clone()), Some(SCREEN));
}

#[test]
fn the_screen_the_heap_and_the_picture_are_told() {
    let mut kernel = kernel();
    allow(&mut kernel);
    assert_eq!(
        said(&send(&mut kernel, 1000, "DEV/1 1 heap")),
        "DEV/1 1 ok 1000 2000"
    );
    assert!(said(&send(&mut kernel, 1100, "DEV/1 2 screen")).starts_with("DEV/1 2 ok "));
    assert!(said(&send(&mut kernel, 1200, "DEV/1 3 describe")).ends_with("ok"));
    let mut frame = Vec::new();
    kernel.render(&mut |_, _| {});
    let out = send(&mut kernel, 1300, "DEV/1 4 shot");
    frame.extend(out);
    assert!(frame[0].starts_with("DEV/1 4 frame "), "{frame:?}");
    assert!(frame.last().unwrap().ends_with("ok"));
    assert_eq!(
        said(&send(&mut kernel, 1400, "DEV/1 5 settings")),
        "DEV/1 5 err no such command"
    );
    assert_eq!(
        said(&send(&mut kernel, 1500, "DEV/1 6 set core.language de")),
        "DEV/1 6 err no such command"
    );
}

#[test]
fn a_mark_is_drawn_over_the_screen_while_it_is_on() {
    fn top_rows(kernel: &mut Kernel) -> Vec<u8> {
        let mut rows = Vec::new();
        kernel.invalidate(kernel.framebuffer.bounds());
        kernel.render(&mut |top, view| {
            if top == 0 {
                rows.extend_from_slice(view.data);
            }
        });
        rows
    }
    let mut kernel = kernel();
    let before = top_rows(&mut kernel);
    allow(&mut kernel);
    let on = top_rows(&mut kernel);
    assert_ne!(before, on, "DEV is drawn");
    let _ = kernel.suspend(Instant(5000));
    assert_eq!(top_rows(&mut kernel), before, "and gone when it ends");
}

#[test]
fn the_path_function_names_the_rules() {
    let p = |raw, usage| link::path(raw, "books", usage);
    assert_eq!(
        p("/books/x/a.epub", Use::Read),
        Ok((link::Root::Library, "books/x/a.epub".into()))
    );
    assert_eq!(
        p("BOOKS/a.epub", Use::Read),
        Ok((link::Root::Library, "books/a.epub".into()))
    );
    assert_eq!(
        p("Plugins/d/x.ini", Use::Write),
        Ok((link::Root::Plugins, "plugins/d/x.ini".into()))
    );
    assert!(p("books/a.epub", Use::Remove).is_err());
    // A library folder of the reader's choosing, one deep.
    assert_eq!(
        link::path("Novels/Deep/a.epub", "Novels/Deep", Use::Read),
        Ok((link::Root::Library, "Novels/Deep/a.epub".into()))
    );
    // A library folder that names a secret is no folder.
    assert!(link::path(".paperpaw/a", ".paperpaw", Use::Read).is_err());
    assert!(link::path("x/a", "../x", Use::Read).is_err());
    assert_eq!(link::plugin_folder("plugins/demo/a"), Some("demo"));
}

#[test]
fn lines_that_are_not_the_links_are_passed_over() {
    let mut kernel = kernel();
    allow(&mut kernel);
    assert!(send(&mut kernel, 1000, "PPT/1 3 GET / 0").is_empty());
    assert!(send(&mut kernel, 1010, "DEV/1 x ping").is_empty());
    assert_eq!(
        said(&send(&mut kernel, 1020, "DEV/1 4 frobnicate")),
        "DEV/1 4 err no such command"
    );
}
