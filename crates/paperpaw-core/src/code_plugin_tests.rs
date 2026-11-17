//! The code plugins of a start: which load, which are refused and why.
use alloc::rc::Rc;
use core::cell::RefCell;

use paperpaw_sdk::{Manifest, MemoryStorage, Version};

use super::*;
use crate::KernelBuilder;
use crate::tests::DEVICE;

/// Loads from a table, noting what it was asked.
#[derive(Clone, Default)]
struct Loader {
    asked: Rc<RefCell<Vec<String>>>,
    /// The ids it was told, with the paths asked.
    ids: Rc<RefCell<Vec<String>>>,
    finished: Rc<RefCell<bool>>,
    /// The paths of code it hosts (named by their plugin, for the
    /// tests' reading) or refuses; any other file is none of its own.
    answers: Vec<(&'static str, Result<&'static str, PluginProblem>)>,
    /// The paths of native libraries, and how their admission goes.
    natives: Vec<(&'static str, Result<(), PluginProblem>)>,
    /// The folders the kernel admitted.
    admitted: Rc<RefCell<Vec<String>>>,
    /// What it says it reads of interface-7 files.
    reads: Option<header5::Host>,
    /// The features the plugins of these paths declare.
    declared: Vec<(&'static str, &'static str)>,
}

/// A hosted native plugin: admitting it notes its folder.
struct Admit {
    result: Result<(), PluginProblem>,
    admitted: Rc<RefCell<Vec<String>>>,
    declared: Vec<&'static str>,
}

impl NativeAdmission for Admit {
    fn admit(self: Box<Self>, kernel: &mut Kernel, folder: &str) -> Result<(), PluginProblem> {
        self.admitted.borrow_mut().push(String::from(folder));
        // As the first native admission makes it.
        let directory = &mut kernel.shared.registry.native_declarations;
        if directory.is_none() {
            *directory = crate::native_host::shared::Shared::try_new(Default::default()).ok();
        }
        self.result
    }

    fn declares(&self, key: &str) -> bool {
        self.declared.contains(&key)
    }
}

impl CodeLoader for Loader {
    fn load_native(
        &mut self,
        _: &dyn Storage,
        code: &CodeFile<'_>,
    ) -> Result<Option<Box<dyn NativeAdmission>>, PluginProblem> {
        let path = code.path;
        let declared = self
            .declared
            .iter()
            .filter(|(p, _)| *p == path)
            .map(|(_, key)| *key)
            .collect();
        let admit = |result| {
            Box::new(Admit {
                result,
                admitted: self.admitted.clone(),
                declared,
            }) as Box<dyn NativeAdmission>
        };
        if let Some((_, result)) = self.natives.iter().find(|(p, _)| *p == path) {
            return Ok(Some(admit(*result)));
        }
        self.asked.borrow_mut().push(String::from(path));
        self.ids.borrow_mut().push(String::from(code.id.as_str()));
        if code
            .sha256
            .is_some_and(|sha256| sha256 != installed(code.id.as_str()))
        {
            return Err(PluginProblem::Changed);
        }
        match self.answers.iter().find(|(p, _)| *p == path) {
            Some((_, Ok(_))) => Ok(Some(admit(Ok(())))),
            Some((_, Err(problem))) => Err(*problem),
            None => Ok(None),
        }
    }

    fn reads(&self) -> Option<header5::Host> {
        self.reads
    }

    fn finish(&mut self, _: &dyn Storage) {
        *self.finished.borrow_mut() = true;
    }
}

/// The digest a test plugin's code was installed with.
fn installed(id: &str) -> [u8; 32] {
    [id.len() as u8; 32]
}

/// The storage's code plugins named as installed.
fn named_installed(storage: MemoryStorage, ids: &[&str]) -> MemoryStorage {
    for id in ids {
        assert!(paperpaw_sdk::native::remember_code(
            &storage,
            id,
            &installed(id)
        ));
    }
    storage
}

fn storage() -> MemoryStorage {
    let storage = MemoryStorage::new()
        .with(
            "plugins/zeta/plugin.ini",
            "[plugin]\nname = Zeta\nversion = 1.0.0\n",
        )
        .with("plugins/zeta/code.ppn", "PPNP")
        .with(
            "plugins/hello/plugin.ini",
            "[plugin]\nname = Hello\nversion = 1.0.0\n",
        )
        .with("plugins/hello/code.ppn", "PPNP")
        .with(
            "plugins/hello/i18n/en.ini",
            "[meta]\nlanguage = en\nname = English\nplural = one-other\n\n[messages]\nhello.greeting = Hi\n",
        )
        .with("plugins/theme/plugin.ini", "[plugin]\nname = Theme\n");
    named_installed(storage, &["hello", "zeta"])
}

fn loader() -> Loader {
    Loader {
        answers: alloc::vec![
            ("plugins/hello/code.ppn", Ok("hello")),
            ("plugins/zeta/code.ppn", Ok("zeta")),
        ],
        ..Loader::default()
    }
}

fn kernel(loader: &Loader, config: &str, safe_mode: Option<SafeMode>) -> Kernel {
    let builder = Kernel::builder(DEVICE)
        .storage(storage())
        .user_config(config)
        .code_plugins(Native::new("code.ppn"), Box::new(loader.clone()));
    match safe_mode {
        Some(safe_mode) => builder.safe_mode(safe_mode),
        None => builder,
    }
    .build()
    .unwrap()
}

fn info<'a>(kernel: &'a Kernel, id: &str) -> &'a paperpaw_sdk::PluginInfo {
    kernel.plugins().get(id).unwrap()
}

#[test]
fn code_is_loaded_in_the_order_of_ids_with_its_folder() {
    let loader = loader();
    let kernel = kernel(&loader, "", None);
    assert_eq!(
        *loader.asked.borrow(),
        ["plugins/hello/code.ppn", "plugins/zeta/code.ppn"]
    );
    assert!(*loader.finished.borrow());
    let hello = info(&kernel, "hello");
    assert_eq!(
        (hello.state, hello.kind, hello.problem),
        (PluginState::Running, PluginKind::Code, None)
    );
    assert_eq!(info(&kernel, "theme").kind, PluginKind::Data);
    assert_eq!(
        kernel.translator().get("hello.greeting"),
        Some("Hi"),
        "its folder's"
    );
    let native = kernel.shared.native.as_ref().unwrap();
    assert_eq!(native.safe_mode, None);
}

/// A native library is admitted by the kernel once, before the plugin is
/// registered; a refusal lists the plugin as failed with why, and the
/// others still load.
#[test]
fn a_native_library_is_admitted_once_and_a_refusal_is_listed() {
    let loader = Loader {
        natives: alloc::vec![("plugins/hello/code.ppn", Ok(()))],
        ..loader()
    };
    let first = kernel(&loader, "", None);
    assert_eq!(*loader.admitted.borrow(), ["plugins/hello", "plugins/zeta"]);
    assert_eq!(*loader.asked.borrow(), ["plugins/zeta/code.ppn"]);
    let hello = info(&first, "hello");
    assert_eq!(
        (hello.state, hello.kind, hello.problem),
        (PluginState::Running, PluginKind::Code, None)
    );
    assert_eq!(first.translator().get("hello.greeting"), Some("Hi"));
    assert_eq!(info(&first, "zeta").problem, None, "the other loads");

    let refused = Loader {
        natives: alloc::vec![("plugins/hello/code.ppn", Err(PluginProblem::NoRoom))],
        ..self::loader()
    };
    let second = kernel(&refused, "", None);
    assert_eq!(
        *refused.admitted.borrow(),
        ["plugins/hello", "plugins/zeta"]
    );
    let hello = info(&second, "hello");
    assert_eq!(
        (hello.state, hello.kind, hello.problem),
        (
            PluginState::Failed,
            PluginKind::Code,
            Some(PluginProblem::NoRoom)
        )
    );
    assert_eq!(info(&second, "zeta").state, PluginState::Running);
}

#[test]
fn switched_off_its_code_is_not_loaded() {
    let loader = loader();
    let kernel = kernel(&loader, "[plugins]\nhello = off\n", None);
    assert_eq!(*loader.asked.borrow(), ["plugins/zeta/code.ppn"]);
    let hello = info(&kernel, "hello");
    assert_eq!(
        (hello.state, hello.enabled, hello.kind),
        (PluginState::Off, false, PluginKind::Code)
    );
}

/// A folder refused for its id (a linked plugin's) loads nothing, not
/// its translations; the linked plugin's row takes its place, and
/// removing the id never reaches the folder (#1424 R1).
#[test]
fn a_folder_refused_for_its_id_gives_its_row_to_the_linked_plugin() {
    let loader = Loader {
        natives: alloc::vec![("plugins/hello/code.ppn", Err(PluginProblem::Mismatch))],
        ..loader()
    };
    let mut kernel = kernel(&loader, "", None);
    assert_eq!(
        info(&kernel, "hello").problem,
        Some(PluginProblem::Mismatch)
    );
    assert_eq!(kernel.shared.translator.get("hello.greeting"), None);
    let manifest = Manifest::new(Id::new_static("hello"), "Linked", Version::new(2, 0, 0));
    let manifest = paperpaw_sdk::ManifestRef::try_new(manifest, None).unwrap();
    let linked = paperpaw_sdk::PluginInfo::new(
        manifest,
        PluginState::Running,
        true,
        paperpaw_sdk::RequiredFor::default(),
        PluginKind::BuiltIn,
        None,
        None,
    );
    assert_eq!(kernel.list_native(linked), Ok(()));
    let hello = info(&kernel, "hello");
    assert_eq!((hello.kind, hello.problem), (PluginKind::BuiltIn, None));
    kernel.apply(paperpaw_sdk::Command::RemovePlugin(Id::new_static("hello")));
    assert!(
        kernel
            .shared
            .storage
            .size("plugins/hello/plugin.ini")
            .is_ok()
    );
    assert_eq!(info(&kernel, "hello").kind, PluginKind::BuiltIn);
}

#[test]
fn code_that_does_not_fit_is_listed_with_why() {
    let loader = Loader {
        answers: alloc::vec![("plugins/hello/code.ppn", Err(PluginProblem::OtherBuild)),],
        ..Loader::default()
    };
    let kernel = kernel(&loader, "", None);
    let hello = info(&kernel, "hello");
    assert_eq!(hello.state, PluginState::Failed);
    assert!(hello.enabled, "it stays switched on, for a build that fits");
    assert_eq!(hello.problem, Some(PluginProblem::OtherBuild));
    // None of the loader's own, and no plugin file by its bytes.
    let zeta = info(&kernel, "zeta");
    assert_eq!(
        (zeta.state, zeta.enabled, zeta.problem),
        (PluginState::Failed, true, Some(PluginProblem::Damaged))
    );

    // Where PaperPaw loads no code.
    let kernel = Kernel::builder(DEVICE).storage(storage()).build().unwrap();
    let hello = info(&kernel, "hello");
    assert_eq!(
        (hello.state, hello.problem),
        (PluginState::Failed, Some(PluginProblem::Unsupported))
    );
    assert!(kernel.shared.native.is_none());
}

/// An installed plugin runs only the file installed; code copied to the
/// card and never installed runs without a question (Decision 205),
/// marked unsigned, unless its id has an author's key or it declares
/// installing plugins or firmware.
#[test]
fn installed_code_runs_as_installed_and_copied_code_unsigned() {
    let mut loader = loader();
    let mut storage = storage();
    for name in ["copied", "pinned", "installer", "beta3"] {
        let ini = format!("[plugin]\nname = {name}\n");
        storage = storage
            .with(&format!("plugins/{name}/plugin.ini"), ini)
            .with(&format!("plugins/{name}/code.ppn"), "PPNP");
    }
    loader.answers.extend([
        ("plugins/copied/code.ppn", Ok("copied")),
        ("plugins/pinned/code.ppn", Ok("pinned")),
        ("plugins/installer/code.ppn", Ok("installer")),
    ]);
    loader.declared = alloc::vec![("plugins/installer/code.ppn", "uses.install")];
    // A Beta 3 plugin whose author's key is pinned is not installed in this
    // version's list: it is told as built for an older PaperPaw.
    storage = storage.with(
        "plugins/beta3/code.ppn",
        include_bytes!("../tests/data/beta3-hyphenation-it-1.0.3-x4.ppn").as_slice(),
    );
    assert!(paperpaw_sdk::native::remember_author(
        &storage, "beta3", &[4; 32]
    ));
    // Hello's file is not the one installed.
    assert!(paperpaw_sdk::native::remember_code(
        &storage, "hello", &[9; 32]
    ));
    assert!(paperpaw_sdk::native::remember_author(
        &storage, "pinned", &[3; 32]
    ));
    let kernel = Kernel::builder(DEVICE)
        .storage(storage)
        .code_plugins(Native::new("code.ppn"), Box::new(loader.clone()))
        .build()
        .unwrap();
    let unsigned = |id: &str| {
        let directory = kernel.shared.registry.native_declarations.as_deref();
        directory.is_some_and(|directory| directory.is_unsigned(id))
    };
    assert_eq!(info(&kernel, "copied").state, PluginState::Running);
    assert!(unsigned("copied"));
    // A pinned id's never-installed code is not even read.
    assert!(!loader.asked.borrow().iter().any(|p| p.contains("pinned")));
    let pinned = info(&kernel, "pinned");
    assert_eq!(
        (pinned.state, pinned.problem),
        (PluginState::Failed, Some(PluginProblem::NotInstalled))
    );
    assert!(pinned.enabled, "it runs once installed");
    let beta3 = info(&kernel, "beta3");
    assert_eq!(
        (beta3.state, beta3.problem),
        (
            PluginState::Failed,
            Some(PluginProblem::OlderInterface { built_for: 6 })
        )
    );
    let installer = info(&kernel, "installer");
    assert_eq!(
        (installer.state, installer.problem),
        (PluginState::Failed, Some(PluginProblem::NotInstalled))
    );
    assert!(
        !loader
            .admitted
            .borrow()
            .iter()
            .any(|f| f.contains("installer"))
    );
    let hello = info(&kernel, "hello");
    assert_eq!(
        (hello.state, hello.problem),
        (PluginState::Failed, Some(PluginProblem::Changed))
    );
    assert_eq!(info(&kernel, "zeta").state, PluginState::Running);
    assert!(!unsigned("zeta") && !unsigned("pinned") && !unsigned("installer"));
}

/// A list of installed code that cannot be read loads no code: an installed
/// plugin is not taken for one never installed, nor is a copied one run.
#[test]
fn an_unreadable_list_of_installed_code_loads_no_code() {
    let loader = loader();
    let storage = storage()
        .with("plugins/copied/plugin.ini", "[plugin]\nname = Copied\n")
        .with("plugins/copied/code.ppn", "PPNP")
        .with(".paperpaw/plugin-code.txt", &b"\xff\xfe"[..]);
    let kernel = Kernel::builder(DEVICE)
        .storage(storage)
        .code_plugins(Native::new("code.ppn"), Box::new(loader.clone()))
        .build()
        .unwrap();
    assert!(
        loader.asked.borrow().is_empty(),
        "{:?}",
        loader.asked.borrow()
    );
    for id in ["hello", "zeta", "copied"] {
        let plugin = info(&kernel, id);
        assert_eq!(
            (plugin.state, plugin.problem),
            (PluginState::Failed, Some(PluginProblem::NotInstalled)),
            "{id}"
        );
    }
}

/// The code of a plugin whose author's key was revoked for it does not
/// run, and is not even read; another plugin's does, even one signed
/// by the same key (revoked per plugin, review M1 of #277).
#[test]
fn code_of_a_revoked_key_does_not_run() {
    use paperpaw_native::signature::{public_key, sign_message};
    use paperpaw_sdk::native::keys::{self, Revocation};
    let loader = loader();
    let storage = storage();
    let seed = [5; 32];
    let hello = public_key(&seed);
    assert!(paperpaw_sdk::native::remember_author(
        &storage, "hello", &hello
    ));
    // zeta is signed by the same key, revoked for hello alone.
    assert!(paperpaw_sdk::native::remember_author(
        &storage, "zeta", &hello
    ));
    // Revoked by the author's key itself.
    let message = keys::revocation_message("hello", &hello);
    let signature = sign_message(message.as_bytes(), &seed);
    let revocation = Revocation::new("hello", hello, signature);
    assert!(keys::keep_revoked(&storage, &[revocation]).is_ok());
    assert_eq!(keys::revoked(&storage)[0].key, hello);
    let kernel = Kernel::builder(DEVICE)
        .storage(storage)
        .code_plugins(Native::new("code.ppn"), Box::new(loader.clone()))
        .build()
        .unwrap();
    assert!(!loader.asked.borrow().iter().any(|p| p.contains("hello")));
    let hello = info(&kernel, "hello");
    assert_eq!(
        (hello.state, hello.problem),
        (PluginState::Failed, Some(PluginProblem::Revoked))
    );
    assert_eq!(info(&kernel, "zeta").state, PluginState::Running);
}

/// A stolen key revoked, and the author moved to a new key with the
/// recovery key: the code on the card (perhaps the thief's) still
/// does not run, until an update by the new key is installed (#20,
/// review M2).
#[test]
fn code_of_a_revoked_key_stays_off_after_a_rotation() {
    use paperpaw_native::signature::{public_key, sign_message};
    use paperpaw_sdk::native::{self, keys};
    let loader = loader();
    let storage = storage();
    let recovery = [7; 32];
    assert!(keys::remember_first_install(
        &storage,
        "hello",
        &[5; 32],
        &public_key(&recovery)
    ));
    // The stolen key revoked with the recovery key.
    let message = keys::revocation_message("hello", &[5; 32]);
    let signature = sign_message(message.as_bytes(), &recovery);
    let revocation = keys::Revocation::new("hello", [5; 32], signature);
    assert!(keys::keep_revoked(&storage, &[revocation]).is_ok());
    assert!(keys::is_revoked(&storage, "hello"));
    let message = keys::rotation_message("hello", &[5; 32], &[8; 32]);
    let signature = sign_message(message.as_bytes(), &recovery);
    assert_eq!(
        keys::accept_rotation(&storage, "hello", &[8; 32], &signature),
        Ok(())
    );
    assert_eq!(native::known_author(&storage, "hello"), Some([8; 32]));
    let kernel = Kernel::builder(DEVICE)
        .storage(storage)
        .code_plugins(Native::new("code.ppn"), Box::new(loader.clone()))
        .build()
        .unwrap();
    assert!(!loader.asked.borrow().iter().any(|p| p.contains("hello")));
    let hello = info(&kernel, "hello");
    assert_eq!(
        (hello.state, hello.problem),
        (PluginState::Failed, Some(PluginProblem::NotInstalled))
    );
    assert_eq!(info(&kernel, "zeta").state, PluginState::Running);
}

#[test]
fn code_is_loaded_by_its_plugins_id_in_any_folder() {
    let loader = Loader {
        answers: alloc::vec![("plugins/my-clock/code.ppn", Ok("clock"))],
        ..Loader::default()
    };
    let storage = MemoryStorage::new()
        .with(
            "plugins/my-clock/plugin.ini",
            "[plugin]\nid = clock\nname = Clock\n",
        )
        .with("plugins/my-clock/code.ppn", "PPNP");
    let storage = named_installed(storage, &["clock"]);
    let kernel = Kernel::builder(DEVICE)
        .storage(storage)
        .code_plugins(Native::new("code.ppn"), Box::new(loader.clone()))
        .build()
        .unwrap();
    // Told by its id, what safe mode and a crash name it by.
    assert_eq!(*loader.ids.borrow(), ["clock"]);
    assert_eq!(*loader.asked.borrow(), ["plugins/my-clock/code.ppn"]);
    assert_eq!(info(&kernel, "clock").state, PluginState::Running);
}

#[test]
fn safe_mode_switches_off_the_plugin_the_crash_was_in() {
    let loader = loader();
    let mut safe_mode = SafeMode::new(SafeModeReason::FailedStarts);
    safe_mode.crashed = Some(String::from("zeta"));
    let kernel = kernel(&loader, "", Some(safe_mode));
    assert!(loader.asked.borrow().is_empty(), "no code at all");
    // Told that loading is over: it says how much room is left.
    assert!(*loader.finished.borrow());
    let zeta = info(&kernel, "zeta");
    assert_eq!(
        (zeta.enabled, zeta.problem),
        (false, Some(PluginProblem::Crashed))
    );
    // Not loaded at this start, but on for the next.
    let hello = info(&kernel, "hello");
    assert_eq!((hello.state, hello.enabled), (PluginState::Off, true));
    assert_eq!(hello.problem, Some(PluginProblem::SafeMode));
    assert!(hello.restart_needed());
    let config = paperpaw_sdk::read_file(kernel.storage(), "paperpaw.ini", 4096).unwrap();
    let config = String::from_utf8(config).unwrap();
    assert!(config.contains("zeta = off"), "{config}");
    assert!(!config.contains("hello"), "{config}");
    let native = kernel.shared.native.as_ref().unwrap();
    let safe = native.safe_mode.as_ref().unwrap();
    assert_eq!(safe.switched_off, [Id::new_static("zeta")]);
}

#[test]
fn safe_mode_without_a_culprit_switches_every_code_plugin_off() {
    for reason in [SafeModeReason::FailedStarts, SafeModeReason::KeyHeld] {
        let loader = loader();
        let mut safe_mode = SafeMode::new(reason);
        // Held at power-on, a crash before is not what it is about.
        safe_mode.crashed = (reason == SafeModeReason::KeyHeld).then(|| String::from("zeta"));
        let kernel = kernel(&loader, "[plugins]\nzeta = off\n", Some(safe_mode));
        let native = kernel.shared.native.as_ref().unwrap();
        let safe = native.safe_mode.as_ref().unwrap();
        assert_eq!(
            safe.switched_off,
            [Id::new_static("hello")],
            "{reason:?}: zeta was off"
        );
        let hello = info(&kernel, "hello");
        assert_eq!(
            (hello.enabled, hello.problem),
            (false, Some(PluginProblem::SafeMode))
        );
    }
}

#[test]
fn what_the_firmware_takes_is_told_once_the_update_on_trial_is_kept() {
    // An update on trial loads no code and offers none (as without
    // `code_plugins`); once its mark is written for good the kernel is
    // told, and plugins (the store) see what the firmware takes (#164).
    let mut kernel = Kernel::builder(DEVICE)
        .storage(storage())
        .native_after_trial(Native::new("code.ppn"))
        .build()
        .unwrap();
    assert!(kernel.shared.native.is_none());
    let hello = info(&kernel, "hello");
    assert_eq!(
        (hello.state, hello.enabled, hello.problem),
        (PluginState::Failed, true, Some(PluginProblem::Unsupported))
    );
    // A mark that could not be written: the shell does not call it.
    assert!(kernel.shared.native.is_none());
    kernel.trial_kept();
    assert_eq!(
        kernel.shared.native.as_ref().map(|n| n.fingerprint),
        Some(Native::new("code.ppn").fingerprint)
    );
    // Again: nothing changes.
    kernel.trial_kept();
    assert!(kernel.shared.native.is_some());
}

#[test]
fn code_installed_while_running_starts_after_a_restart() {
    let loader = loader();
    let mut kernel = Kernel::builder(DEVICE)
        .storage(MemoryStorage::new())
        .code_plugins(Native::new("code.ppn"), Box::new(loader.clone()))
        .build()
        .unwrap();
    let _ = kernel.storage().create_dir("plugins/hello");
    for (path, data) in [
        ("plugins/hello/plugin.ini", "[plugin]\nname = Hello\n"),
        ("plugins/hello/code.ppn", "PPNP"),
    ] {
        kernel.storage().write(path, data.as_bytes()).unwrap();
    }
    kernel.load_plugin("plugins/hello");
    assert!(loader.asked.borrow().is_empty());
    let hello = info(&kernel, "hello");
    assert_eq!(
        (hello.state, hello.enabled, hello.kind),
        (PluginState::Off, true, PluginKind::Code)
    );
    assert!(hello.restart_needed());
}

#[test]
fn plugins_the_shell_put_in_folders_install_before_the_start() {
    let loader = loader();
    let storage = MemoryStorage::new()
        .with(
            "plugins/hello/plugin.ini",
            "[plugin]\nname = Hello\nversion = 0.1.0\n",
        )
        .with(
            "plugins/.hello.part/plugin.ini",
            "[plugin]\nname = Hello\nversion = 1.0.0\n",
        )
        .with("plugins/.hello.part/code.ppn", "PPNP")
        .with("plugins/.other.part/plugin.ini", "[plugin]\nid = zeta\n");
    // As the shell's installer names it.
    let storage = named_installed(storage, &["hello"]);
    let mut kernel = Kernel::builder(DEVICE)
        .storage(storage)
        .code_plugins(Native::new("code.ppn"), Box::new(loader.clone()))
        .install_plugin(Id::new_static("hello"), "plugins/.hello.part")
        .install_plugin(Id::new_static("other"), "plugins/.other.part")
        .build()
        .unwrap();
    // In place of the version before, its code loaded at this start.
    assert_eq!(*loader.asked.borrow(), ["plugins/hello/code.ppn"]);
    let hello = info(&kernel, "hello");
    assert_eq!(
        (hello.state, hello.kind),
        (PluginState::Running, PluginKind::Code)
    );
    let storage = kernel.storage();
    assert!(!storage.exists("plugins/.hello.part/plugin.ini"));
    let ini = paperpaw_sdk::read_file(storage, "plugins/hello/plugin.ini", 512).unwrap();
    assert!(String::from_utf8(ini).unwrap().contains("1.0.0"));
    // Not what it says it is: refused, its folder gone.
    assert!(!storage.exists("plugins/.other.part/plugin.ini"));
    assert!(kernel.plugins().get("other").is_none());
    let warnings = kernel.take_warnings();
    assert!(
        warnings
            .iter()
            .any(|w| w.message.contains("not the plugin `other`")),
        "{warnings:?}"
    );
}

#[test]
fn a_code_plugin_updated_while_it_runs_takes_over_at_the_restart() {
    let loader = loader();
    let mut kernel = kernel(&loader, "", None);
    assert_eq!(info(&kernel, "hello").state, PluginState::Running);
    for (path, data) in [
        (
            "plugins/.hello.part/plugin.ini",
            "[plugin]
id = hello
name = Hello
version = 2.0.0
",
        ),
        ("plugins/.hello.part/code.ppn", "PPNP"),
    ] {
        kernel.storage().write(path, data.as_bytes()).unwrap();
    }
    kernel.install_plugin(&Id::new_static("hello"), "plugins/.hello.part");
    assert!(kernel.take_warnings().is_empty());
    // The code before runs on, listed with the update it waits for.
    let hello = info(&kernel, "hello");
    assert_eq!((hello.state, hello.enabled), (PluginState::Running, true));
    assert_eq!(hello.update, Some(Version::new(2, 0, 0)));
    assert!(hello.restart_needed());
    let storage = kernel.storage();
    assert!(storage.exists("plugins/hello/plugin.ini"));
    assert!(!storage.exists("plugins/.hello.part/code.ppn"));
    assert!(!storage.exists("plugins/.hello.old/code.ppn"));
}

#[test]
fn pending_install_marks_settle_before_the_startup_snapshot() {
    use paperpaw_sdk::install::marks::{self, From, Marks};
    let storage = storage();
    let native = Native::new("code.ppn");
    assert!(paperpaw_sdk::native::remember_code(
        &storage, "hello", &[9; 32]
    ));
    storage
        .write(
            "plugins/hello/code.ppn",
            &code_file(native.fingerprint.0, installed("hello")),
        )
        .unwrap();
    marks::write(
        &storage,
        "plugins/hello",
        &Marks::new(From::Store(Version::new(1, 0, 0))).code(installed("hello")),
    )
    .unwrap();
    let loader = loader();
    let kernel = Kernel::builder(DEVICE)
        .storage(storage)
        .code_plugins(native, Box::new(loader.clone()))
        .build()
        .unwrap();
    assert_eq!(info(&kernel, "hello").state, PluginState::Running);
    assert_eq!(loader.asked.borrow().len(), 2);
    assert!(!kernel.storage().exists("plugins/hello/.install-marks"));
}

/// The file of a code plugin with `fingerprint`, whose header's
/// SHA-256 is `sha256` (the test loader checks only that).
fn code_file(fingerprint: u64, sha256: [u8; 32]) -> alloc::vec::Vec<u8> {
    let header = paperpaw_sdk::native::Header {
        chip: paperpaw_sdk::native::Chip::from_id(5).unwrap(),
        fingerprint,
        base: 0x3c00_0000,
        image_len: 128,
        code_at: 96,
        entry: 0,
        api: 32,
        relocs: 0,
        align: 16,
        compiler: 1,
        hot_at: 0,
        hot_len: 0,
        patches: 0,
        needs_api: 0,
        families: [0; 8],
        interface: crate::sdk::native::INTERFACE,
        sha256,
    };
    let mut file = header.to_bytes().to_vec();
    file.extend_from_slice(&[0; 128]);
    file
}

/// An interface-7 file of the X4 (ESP32-C3), as `paperpaw-sign pack5`
/// made it, and what reads it: this firmware's loader, and another's.
const FILE: &[u8] = include_bytes!("../../paperpaw-native/tests/data/abi7-x4-min.ppn");
const X4: header5::Host = header5::Host::x4(paperpaw_native::host_glue::MINOR);
const X4_PRO: header5::Host = header5::Host::x4_pro(paperpaw_native::host_glue::MINOR);

/// The SHA-256 the file's header carries (its bytes 64 to 96).
fn file_sha256() -> [u8; 32] {
    FILE[64..96].try_into().unwrap()
}

/// A storage with `stats` fetched for a firmware update: its folder,
/// its code an interface-7 file of the X4, named as fetched if `named`.
fn fetched(named: bool) -> MemoryStorage {
    let storage = storage()
        .with(
            "plugins/.stats.next/plugin.ini",
            "[plugin]\nid = stats\nname = Statistics\nversion = 1.0.0\n",
        )
        .with(".paperpaw/plugins/stats/statistics.sqlite3", "kept");
    storage.write("plugins/.stats.next/code.ppn", FILE).unwrap();
    if named {
        assert!(paperpaw_sdk::native::remember_fetched(
            &storage,
            "stats",
            &file_sha256()
        ));
    }
    storage
}

/// A kernel of a firmware whose loader reads what `reads` says, built
/// as the X4's shell builds it: the loader alone says so (no plugin
/// host, which that shell does not set).
fn builder_on(storage: MemoryStorage, loader: &Loader, reads: header5::Host) -> KernelBuilder {
    let loader = Loader {
        reads: Some(reads),
        ..loader.clone()
    };
    Kernel::builder(DEVICE)
        .storage(storage)
        .code_plugins(Native::new("code.ppn"), Box::new(loader))
}

fn stats_loader() -> Loader {
    let mut loader = loader();
    loader.natives.push(("plugins/stats/code.ppn", Ok(())));
    loader
}

#[test]
fn code_fetched_for_this_firmware_is_installed_at_its_start() {
    let loader = stats_loader();
    let kernel = builder_on(fetched(true), &loader, X4).build().unwrap();
    let storage = kernel.storage();
    assert_eq!(info(&kernel, "stats").state, PluginState::Running);
    assert_eq!(info(&kernel, "stats").kind, PluginKind::Code);
    assert!(
        loader
            .admitted
            .borrow()
            .iter()
            .any(|p| p == "plugins/stats")
    );
    // Named as installed: the file's own SHA-256.
    assert_eq!(
        paperpaw_sdk::native::installed_code(storage, "stats"),
        Some(file_sha256())
    );
    assert!(!storage.exists("plugins/.stats.next/code.ppn"));
    // Its data and settings belong to its id: they stay.
    assert!(storage.exists(".paperpaw/plugins/stats/statistics.sqlite3"));
}

#[test]
fn code_fetched_for_another_firmware_waits() {
    // The file is the X4's: the X4 Pro's loader does not read it.
    let loader = stats_loader();
    let kernel = builder_on(fetched(true), &loader, X4_PRO).build().unwrap();
    let storage = kernel.storage();
    assert!(kernel.plugins().get("stats").is_none());
    assert!(storage.exists("plugins/.stats.next/code.ppn"));
}

#[test]
fn a_folder_the_update_did_not_fetch_is_not_taken() {
    // Put there otherwise (e.g. over the Wi-Fi transfer).
    let loader = stats_loader();
    let kernel = builder_on(fetched(false), &loader, X4).build().unwrap();
    assert!(kernel.plugins().get("stats").is_none());
    let storage = kernel.storage();
    assert!(paperpaw_sdk::native::installed_code(storage, "stats").is_none());
}

#[test]
fn safe_mode_leaves_code_fetched_where_it_is() {
    let loader = stats_loader();
    let kernel = builder_on(fetched(true), &loader, X4)
        .safe_mode(SafeMode::new(SafeModeReason::KeyHeld))
        .build()
        .unwrap();
    assert!(kernel.storage().exists("plugins/.stats.next/code.ppn"));
}

#[test]
fn a_start_without_code_plugins_leaves_code_fetched_for_the_next() {
    // An update's trial start loads no code plugins: what was fetched
    // for it waits, and the firmware, seeing it, starts once more
    // when the update is kept.
    let native = Native::new("code.ppn");
    let kernel = Kernel::builder(DEVICE)
        .storage(fetched(true))
        .build()
        .unwrap();
    let storage = kernel.storage();
    assert!(storage.exists("plugins/.stats.next/code.ppn"));
    let waiting = super::fetched_for(storage, &native, Some(&X4));
    assert_eq!(waiting, [("stats".into(), file_sha256())]);
    // For another firmware, nothing waits.
    assert!(super::fetched_for(storage, &native, Some(&X4_PRO)).is_empty());
    assert!(super::code_to_load(storage, &native, Some(&X4)));
    assert!(!super::code_to_load(storage, &native, Some(&X4_PRO)));
}

/// An update whose loader still reads the code installed fetches
/// nothing: a start once the update is kept loads it (the trial's start
/// went without).
#[test]
fn code_installed_that_fits_is_code_to_load() {
    let native = Native::new("code.ppn");
    let plugins = storage().with(
        "plugins/stats/plugin.ini",
        "[plugin]\nid = stats\nname = Statistics\nversion = 1.0.0\n",
    );
    assert!(!super::code_to_load(&plugins, &native, Some(&X4)));
    plugins.write("plugins/stats/code.ppn", FILE).unwrap();
    assert!(super::code_to_load(&plugins, &native, Some(&X4)));
    assert!(!super::code_to_load(&plugins, &native, Some(&X4_PRO)));
    // A folder being installed or put aside is not one to load.
    let aside = storage();
    aside.write("plugins/.stats.old/code.ppn", FILE).unwrap();
    assert!(!super::code_to_load(&aside, &native, Some(&X4)));
}

/// A file of the format before interface 7 is no code to load, even one
/// that says this firmware's fingerprint: no loader runs it, so nothing
/// starts again for it and nothing fetched takes its place.
#[test]
fn a_file_of_the_format_before_is_no_code_to_load() {
    let native = Native::new("code.ppn");
    let old = code_file(native.fingerprint.0, installed("stats"));
    let plugins = storage().with(
        "plugins/stats/plugin.ini",
        "[plugin]\nid = stats\nname = Statistics\nversion = 1.0.0\n",
    );
    plugins.write("plugins/stats/code.ppn", &old).unwrap();
    assert!(!super::code_to_load(&plugins, &native, Some(&X4)));
    let waiting = storage().with(
        "plugins/.stats.next/plugin.ini",
        "[plugin]\nid = stats\nname = Statistics\nversion = 1.0.0\n",
    );
    waiting.write("plugins/.stats.next/code.ppn", &old).unwrap();
    assert!(paperpaw_sdk::native::remember_fetched(
        &waiting,
        "stats",
        &installed("stats")
    ));
    assert!(super::fetched_for(&waiting, &native, Some(&X4)).is_empty());
    assert!(!super::code_to_load(&waiting, &native, Some(&X4)));
}
