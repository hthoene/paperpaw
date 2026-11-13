//! The confirmation of a plugin install: its words, and the kernel showing it
//! on the guarding screen and taking only the user's answer.
#![allow(clippy::unwrap_used, reason = "fixture errors fail tests")]
use super::*;
use crate::native_host::firmware_confirm::{ANSWERED, NONCE, SCREEN};
use crate::native_host::firmware_driver::plugin_tests::{asking_job, kernel_host};
use crate::native_host::firmware_install::Answer;
use crate::native_host::plugin_job::{
    KIND_INSTALL, USE_KEY_MAX, USES_SHOWN, Uses, reason as job_reason,
};
use alloc::string::ToString;
use paperpaw_platform::firmware::{Text, WRITE_BLOCK};
use paperpaw_platform::firmware_fake::{FakeFirmware, paperpaw_facts};
use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::{Instant, Params};

fn en() -> paperpaw_i18n::Translator {
    let mut translator = paperpaw_i18n::Translator::new();
    translator.add(paperpaw_i18n::Catalog::parse(include_str!("../../i18n/host/en.ini")).unwrap());
    translator
}

fn question() -> PluginQuestion {
    PluginQuestion {
        owner: 1,
        request: 5,
        kind: KIND_INSTALL,
        id: Text::new("dials"),
        name: Text::new("Dials"),
        version: Text::new("1.2.0"),
        signer: Text::new("PaperPaw release"),
        key: Some([0xab; 32]),
        sideload: false,
        relation: Relation::Newer,
        uses: Uses {
            keys: [Text::<USE_KEY_MAX>::new(""); USES_SHOWN],
            count: 0,
            more: false,
        },
    }
}

#[test]
fn the_words_name_the_plugin_its_signer_and_what_it_uses() {
    let i18n = en();
    let mut q = question();
    q.uses.keys[0] = Text::new("storage");
    q.uses.keys[1] = Text::new("network");
    q.uses.count = 2;
    q.uses.more = true;
    let said = texts(&i18n, "update", &q).unwrap();
    assert_eq!(said.title, "Install this plugin?");
    assert!(said.body.contains("Asked by update."));
    assert!(said.body.contains("Dials 1.2.0 (dials)"));
    assert!(said.body.contains("Signed by PaperPaw release."));
    assert!(said.body.contains("It uses: storage, network, …."));
    assert!(!said.body.contains("store: nothing"));
    assert_eq!((said.yes.as_str(), said.no.as_str()), ("Install", "Cancel"));
}

#[test]
fn another_key_a_sideload_and_an_older_version_are_said() {
    let i18n = en();
    let mut q = question();
    q.signer = Text::new("");
    q.sideload = true;
    q.relation = Relation::Older;
    let said = texts(&i18n, "update", &q).unwrap();
    assert!(said.body.contains("abab abab abab abab"));
    assert!(said.body.contains("not PaperPaw's"));
    assert!(said.body.contains("nothing vouches for it"));
    assert!(said.body.contains("older version"));
    q.key = None;
    assert!(
        texts(&i18n, "update", &q)
            .unwrap()
            .body
            .contains("Not signed.")
    );
}

#[test]
fn a_removal_says_remove_and_no_install_facts() {
    let i18n = en();
    let mut q = question();
    q.kind = KIND_REMOVAL;
    let said = texts(&i18n, "update", &q).unwrap();
    assert_eq!(said.title, "Remove this plugin?");
    assert_eq!(said.yes, "Remove, keep data");
    assert_eq!(said.data.as_deref(), Some("Remove with data"));
    assert_eq!(said.no, "Cancel");
    assert!(said.body.contains("data folder is kept"));
    assert!(!said.body.contains("Signed"));
}

#[test]
fn names_from_the_plugin_cannot_forge_lines() {
    let i18n = en();
    let mut q = question();
    q.name = Text::new("Dials\nSigned by PaperPaw.");
    q.version = Text::new("1\u{202E}.0");
    q.signer = Text::new("");
    q.key = None;
    q.uses.keys[0] = Text::new("a\nSigned by PaperPaw.");
    q.uses.count = 1;
    let said = texts(&i18n, "evil\nSigned by PaperPaw.", &q).unwrap();
    assert!(
        said.body.lines().all(|line| !line.starts_with("Signed")),
        "{:?}",
        said.body
    );
    // The host's own lines: asker, plugin, signer, uses.
    assert_eq!(said.body.lines().count(), 4);
}

#[test]
fn the_german_texts_cover_every_english_plugin_key() {
    let en = include_str!("../../i18n/host/en.ini");
    let de = include_str!("../../i18n/host/de.ini");
    for line in en.lines().filter(|l| l.starts_with("core.plugin.")) {
        let key = line.split(" = ").next().unwrap();
        assert!(
            de.lines()
                .any(|l| l.starts_with(&alloc::format!("{key} = "))),
            "{key} missing in de.ini"
        );
    }
}

mod kernel_side {
    use super::*;
    use crate::Kernel;
    use paperpaw_sdk::{
        Canvas, Cx, Env, Handled, Id, Manifest, Plugin, Priority, RegisterError, Registrar, Screen,
        ScreenSpec, Version, builtin::slots,
    };

    fn fill(buf: &mut [u8]) -> bool {
        for (at, byte) in buf.iter_mut().enumerate() {
            *byte = (at as u8).wrapping_mul(53).wrapping_add(7);
        }
        true
    }

    /// A home screen to lie under the confirmation.
    struct Page;
    impl Screen for Page {
        fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}
        fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
            Handled::No
        }
    }
    struct Home;
    impl Plugin for Home {
        fn manifest(&self) -> Manifest {
            Manifest::new(Id::new_static("h"), "Home", Version::new(1, 0, 0))
                .uses(paperpaw_sdk::Uses::NOTHING)
        }
        fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
            const HOME: Id = Id::new_static("h.home");
            r.add_screen(ScreenSpec::new(
                HOME,
                "home",
                alloc::boxed::Box::new(|_, _| alloc::boxed::Box::new(Page)),
            ))?;
            r.bind_slot(slots::HOME, HOME, Priority::DEFAULT)
        }
    }

    fn kernel(random: bool) -> Kernel {
        let fake = FakeFirmware::new(
            "0.5.0",
            "paperpaw-x4",
            5,
            paperpaw_facts("0.6.0", "paperpaw-x4", 2 * WRITE_BLOCK),
        );
        let (loader, offered) = kernel_host();
        let mut builder = Kernel::builder(crate::tests::DEVICE);
        if random {
            builder = builder.random(fill);
        }
        let mut kernel = builder
            .plugin(Home)
            .firmware_host(alloc::boxed::Box::new(fake))
            .plugin_host(loader, offered, &[])
            .build()
            .unwrap();
        kernel.start(Instant(0));
        kernel
    }

    fn submit(kernel: &mut Kernel) {
        let driver = kernel.firmware.as_ref().unwrap().make().unwrap();
        driver.borrow_mut().submit_plugin(asking_job(1, 5)).unwrap();
        for _ in 0..20 {
            kernel.native_firmware_pump();
        }
    }

    /// The install asked; the screen is open: its nonce.
    fn shown(kernel: &mut Kernel) -> u128 {
        submit(kernel);
        let _ = kernel.tick(Instant(0));
        let top = kernel.stack.last().expect("the confirmation is open");
        assert_eq!(top.id, SCREEN);
        top.params.get(NONCE).unwrap().parse().unwrap()
    }

    /// What the install ended with, told or not yet.
    fn ended(kernel: &Kernel) -> Option<u8> {
        let driver = kernel.firmware.as_ref().unwrap().make().unwrap();
        let driver = driver.borrow();
        driver
            .kept(1, KIND_INSTALL)
            .or_else(|| driver.peek_ready())
            .map(|outcome| outcome.reason)
    }

    fn open_count(kernel: &Kernel) -> usize {
        kernel.stack.iter().filter(|s| s.id == SCREEN).count()
    }

    #[test]
    fn the_question_opens_one_guarding_screen_and_the_job_waits_without_work() {
        let mut kernel = kernel(true);
        shown(&mut kernel);
        assert!(kernel.guard().is_some(), "it guards the device");
        for _ in 0..5 {
            kernel.native_firmware_pump();
        }
        assert_eq!(open_count(&kernel), 1, "no second screen for it");
        let driver = kernel.firmware.as_ref().unwrap().make().unwrap();
        // An open question is no work for the loop.
        assert!(!driver.borrow().has_work(0));
        assert!(driver.borrow().peek_plugin_question().is_none());
        assert_eq!(ended(&kernel), None);
    }

    #[test]
    fn only_the_screen_answers_no_is_2_and_an_event_with_the_nonce_changes_nothing() {
        let mut kernel = kernel(true);
        let nonce = shown(&mut kernel);
        for word in ["yes", "no", "dismissed"] {
            let params = Params::new()
                .with(NONCE, nonce.to_string())
                .with("answer", word);
            kernel.emit(&ANSWERED, &params);
        }
        assert_eq!(ended(&kernel), None, "forged answers count for nothing");
        let driver = kernel.firmware.as_ref().unwrap().make().unwrap();
        assert!(
            driver
                .borrow_mut()
                .answer_shown(nonce ^ 1, Answer::Yes)
                .is_none()
        );
        // The user presses on the screen: the focus starts on No.
        let _ = kernel.run_action(Instant(600), actions::CONFIRM);
        assert_eq!(ended(&kernel), Some(2));
        // Once: a late answer from anyone is nobody's.
        let params = Params::new()
            .with(NONCE, nonce.to_string())
            .with("answer", "yes");
        kernel.emit(&ANSWERED, &params);
        assert_eq!(ended(&kernel), Some(2));
    }

    #[test]
    fn a_yes_on_the_screen_ends_the_job_with_0() {
        let mut kernel = kernel(true);
        shown(&mut kernel);
        let _ = kernel.run_action(Instant(600), actions::LEFT);
        let _ = kernel.run_action(Instant(700), actions::CONFIRM);
        for _ in 0..5 {
            kernel.native_firmware_pump();
        }
        assert_eq!(ended(&kernel), Some(job_reason::OK));
        assert_eq!(open_count(&kernel), 0, "the screen closed");
    }

    #[test]
    fn back_and_ten_minutes_are_dismissed_not_a_no() {
        let mut kernel = kernel(true);
        shown(&mut kernel);
        let _ = kernel.run_action(Instant(600), actions::BACK);
        for _ in 0..5 {
            kernel.native_firmware_pump();
        }
        assert_eq!(ended(&kernel), Some(3));
        let mut kernel = self::kernel(true);
        shown(&mut kernel);
        let _ = kernel.tick(Instant(599_999));
        assert_eq!(ended(&kernel), None, "not yet");
        let _ = kernel.tick(Instant(600_000));
        for _ in 0..5 {
            kernel.native_firmware_pump();
        }
        assert_eq!(ended(&kernel), Some(3));
    }

    #[test]
    fn sleep_closes_the_screen_and_the_asking_plugin_stopping_takes_it_with_it() {
        let mut kernel = kernel(true);
        shown(&mut kernel);
        let _ = kernel.suspend(Instant(1_000));
        assert_eq!(open_count(&kernel), 0, "closed though it guards");
        assert!(ended(&kernel).is_some(), "the job ended and was told");
        let mut kernel = self::kernel(true);
        let nonce = shown(&mut kernel);
        kernel.firmware_sender_stopped(1);
        assert_eq!(open_count(&kernel), 0, "closed though it guards");
        let params = Params::new()
            .with(NONCE, nonce.to_string())
            .with("answer", "yes");
        kernel.emit(&ANSWERED, &params);
        let _ = kernel.run_action(Instant(800), actions::CONFIRM);
        assert_eq!(ended(&kernel), None, "nobody is told, nobody answers");
    }

    #[test]
    fn without_a_random_source_nothing_is_asked_and_it_is_dismissed() {
        let mut kernel = kernel(false);
        submit(&mut kernel);
        assert_eq!(open_count(&kernel), 0, "no screen");
        assert_eq!(ended(&kernel), Some(3), "dismissed, nobody can answer");
    }

    /// A kernel over a card that holds the plugin `dials` and its data, and a
    /// removal of it that asked.
    fn removal_shown() -> (Kernel, u128) {
        let fake = FakeFirmware::new(
            "0.5.0",
            "paperpaw-x4",
            5,
            paperpaw_facts("0.6.0", "paperpaw-x4", 2 * WRITE_BLOCK),
        );
        let (loader, offered) = kernel_host();
        let mut kernel = Kernel::builder(crate::tests::DEVICE)
            .random(fill)
            .storage(paperpaw_sdk::MemoryStorage::new())
            .plugin(Home)
            .firmware_host(alloc::boxed::Box::new(fake))
            .plugin_host(loader, offered, &[])
            .build()
            .unwrap();
        kernel.start(Instant(0));
        let card = kernel.storage();
        card.write(
            "plugins/dials/plugin.ini",
            b"[plugin]\nid = dials\nname = Dials\nversion = 1.0.0\n",
        )
        .unwrap();
        card.write(".paperpaw/plugins/dials/notes.txt", b"notes")
            .unwrap();
        let driver = kernel.firmware.as_ref().unwrap().make().unwrap();
        let job = crate::native_host::plugin_remove::PluginRemove::new(1, 5, "dials").unwrap();
        driver
            .borrow_mut()
            .submit_plugin(crate::native_host::plugin_jobs::PluginJobs::Remove(job))
            .unwrap();
        for _ in 0..20 {
            kernel.native_firmware_pump();
        }
        let _ = kernel.tick(Instant(0));
        let top = kernel.stack.last().expect("the confirmation is open");
        assert_eq!(top.id, SCREEN);
        let nonce = top.params.get(NONCE).unwrap().parse().unwrap();
        (kernel, nonce)
    }

    fn removal_ended(kernel: &mut Kernel) -> Option<u8> {
        for _ in 0..40 {
            kernel.native_firmware_pump();
        }
        let driver = kernel.firmware.as_ref().unwrap().make().unwrap();
        let driver = driver.borrow();
        driver
            .kept(1, crate::native_host::plugin_job::KIND_REMOVAL)
            .or_else(|| driver.peek_ready())
            .map(|outcome| outcome.reason)
    }

    #[test]
    fn a_removal_shows_three_buttons_and_the_focus_starts_on_cancel() {
        let (mut kernel, _) = removal_shown();
        let top = kernel.stack.last().unwrap();
        assert!(top.params.get("data").is_some(), "the middle yes");
        // Enter on the focus that starts: Cancel, reason 2; nothing changes.
        let _ = kernel.run_action(Instant(600), actions::CONFIRM);
        assert_eq!(removal_ended(&mut kernel), Some(2));
        let card = kernel.storage();
        assert!(card.exists("plugins/dials/plugin.ini"));
        assert!(card.exists(".paperpaw/plugins/dials/notes.txt"));
    }

    #[test]
    fn the_first_yes_keeps_the_data_and_the_middle_one_removes_it_too() {
        // Two steps left from Cancel: "Remove, keep data".
        let (mut kernel, _) = removal_shown();
        let _ = kernel.run_action(Instant(600), actions::LEFT);
        let _ = kernel.run_action(Instant(650), actions::LEFT);
        let _ = kernel.run_action(Instant(700), actions::CONFIRM);
        assert_eq!(removal_ended(&mut kernel), Some(0));
        let card = kernel.storage();
        assert!(!card.exists("plugins/dials/plugin.ini"));
        assert!(card.exists(".paperpaw/plugins/dials/notes.txt"), "kept");
        // One step left: "Remove with data".
        let (mut kernel, _) = removal_shown();
        let _ = kernel.run_action(Instant(600), actions::LEFT);
        let _ = kernel.run_action(Instant(700), actions::CONFIRM);
        assert_eq!(removal_ended(&mut kernel), Some(0));
        let card = kernel.storage();
        assert!(!card.exists("plugins/dials/plugin.ini"));
        assert!(!card.exists(".paperpaw/plugins/dials/notes.txt"), "removed");
    }

    #[test]
    fn back_dismisses_a_removal_and_nothing_is_removed() {
        let (mut kernel, _) = removal_shown();
        let _ = kernel.run_action(Instant(600), actions::BACK);
        assert_eq!(removal_ended(&mut kernel), Some(3));
        assert!(kernel.storage().exists("plugins/dials/plugin.ini"));
    }

    /// A device that loads no code (Boox, desktop) installs the data plugin
    /// `id` of `version` from `Download/<id>` with a yes on the screen.
    fn data_install(
        kernel: &mut Kernel,
        request: u32,
        (id, version): (&str, &str),
        flags: u32,
    ) -> Option<u8> {
        let card = kernel.storage();
        let ini = alloc::format!("[plugin]\nid = {id}\nname = Dials\nversion = {version}\n");
        let source = alloc::format!("Download/{id}");
        card.write(&alloc::format!("{source}/plugin.ini"), ini.as_bytes())
            .unwrap();
        let job = crate::native_host::plugin_install::PluginInstall::new(
            1,
            request,
            &source,
            flags,
            crate::native_host::plugin_rules::Evidence::NONE,
        )
        .unwrap();
        let driver = kernel.firmware.as_ref().unwrap().make().unwrap();
        driver
            .borrow_mut()
            .submit_plugin(crate::native_host::plugin_jobs::PluginJobs::Install(job))
            .unwrap();
        for _ in 0..40 {
            kernel.native_firmware_pump();
        }
        let _ = kernel.tick(Instant(0));
        assert_eq!(kernel.stack.last().unwrap().id, SCREEN);
        kernel.output.commands.clear();
        let _ = kernel.run_action(Instant(600), actions::LEFT);
        let _ = kernel.run_action(Instant(700), actions::CONFIRM);
        for _ in 0..40 {
            kernel.native_firmware_pump();
        }
        let driver = kernel.firmware.as_ref().unwrap().make().unwrap();
        let driver = driver.borrow();
        driver
            .kept(1, KIND_INSTALL)
            .or_else(|| driver.peek_ready())
            .map(|outcome| outcome.reason)
    }

    fn data_only_kernel() -> Kernel {
        let mut kernel = Kernel::builder(crate::tests::DEVICE)
            .random(fill)
            .storage(paperpaw_sdk::MemoryStorage::new())
            .plugin(Home)
            .firmware_host(alloc::boxed::Box::new(FakeFirmware::new(
                "0.5.0",
                "paperpaw-x4",
                5,
                paperpaw_facts("0.6.0", "paperpaw-x4", 2 * WRITE_BLOCK),
            )))
            .data_plugin_host()
            .build()
            .unwrap();
        kernel.start(Instant(0));
        kernel
    }

    #[test]
    fn an_installed_data_plugin_loads_at_once_and_the_list_changes() {
        let mut kernel = data_only_kernel();
        let changed = |kernel: &Kernel| {
            kernel.output.commands.iter().any(|c| {
                matches!(c, crate::Command::Emit(e, _)
                    if *e == paperpaw_sdk::builtin::events::PLUGINS_CHANGED)
            })
        };
        let version = |kernel: &Kernel| {
            kernel
                .plugins()
                .get("dials")
                .map(|info| info.manifest.version.to_string())
        };
        assert_eq!(data_install(&mut kernel, 5, ("dials", "1.0.0"), 0), Some(0));
        // Listed now, not at the next start (interface 6 did the same).
        assert_eq!(version(&kernel).as_deref(), Some("1.0.0"));
        assert!(changed(&kernel), "event 70");
        // An update takes the old version's place at once.
        let replace = crate::native_host::plugin_install::FLAG_REPLACE;
        assert_eq!(
            data_install(&mut kernel, 6, ("dials", "1.1.0"), replace),
            Some(0)
        );
        assert_eq!(version(&kernel).as_deref(), Some("1.1.0"));
        assert!(changed(&kernel));
        assert!(
            kernel.take_warnings().is_empty(),
            "{:?}",
            kernel.take_warnings()
        );
    }

    #[test]
    fn a_data_plugin_with_the_id_of_one_built_in_never_unloads_it() {
        // No device host wired: the job knows no plugin built in.
        let mut kernel = data_only_kernel();
        let _ = kernel.take_warnings();
        assert_eq!(data_install(&mut kernel, 5, ("h", "9.0.0"), 0), Some(0));
        let home = kernel.plugins().get("h").unwrap();
        assert_eq!(home.manifest.version.to_string(), "1.0.0", "still built in");
        assert_ne!(home.kind, paperpaw_sdk::PluginKind::Data);
        assert!(!kernel.take_warnings().is_empty(), "said");
    }

    #[test]
    fn a_data_plugin_whose_swap_a_sleep_finishes_is_loaded_too() {
        // A sleep after each number of steps: one comes during the swap,
        // which then finishes in the stop's bounded steps, not in a pump.
        let mut in_the_sleep = 0;
        for pumps in 0..40 {
            let mut kernel = data_only_kernel();
            kernel
                .storage()
                .write(
                    "Download/dials/plugin.ini",
                    b"[plugin]\nid = dials\nname = Dials\nversion = 1.0.0\n",
                )
                .unwrap();
            let job = crate::native_host::plugin_install::PluginInstall::new(
                1,
                5,
                "Download/dials",
                0,
                crate::native_host::plugin_rules::Evidence::NONE,
            )
            .unwrap();
            let driver = kernel.firmware.as_ref().unwrap().make().unwrap();
            driver
                .borrow_mut()
                .submit_plugin(crate::native_host::plugin_jobs::PluginJobs::Install(job))
                .unwrap();
            drop(driver);
            for _ in 0..40 {
                kernel.native_firmware_pump();
            }
            let _ = kernel.tick(Instant(0));
            let _ = kernel.run_action(Instant(600), actions::LEFT);
            let _ = kernel.run_action(Instant(700), actions::CONFIRM);
            for _ in 0..pumps {
                kernel.native_firmware_pump();
            }
            let before = kernel.plugins().get("dials").is_some();
            let _ = kernel.suspend(Instant(1_000));
            let on_card = kernel.storage().exists("plugins/dials/plugin.ini");
            let loaded = kernel.plugins().get("dials").is_some();
            assert_eq!(on_card, loaded, "a sleep after {pumps} steps");
            in_the_sleep += usize::from(loaded && !before);
        }
        assert!(in_the_sleep > 0, "no sleep came during a swap");
    }
}
