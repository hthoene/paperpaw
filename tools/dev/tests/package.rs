//! Packaging checks on data plugins: the samples pass but for what is
//! known, a fixture wrong in every file is caught, by the files and on
//! the device; releases and `.ppn` files.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::path::{Path, PathBuf};

use paperpaw_dev::cli::Context;
use paperpaw_dev::report::{Level, Report};
use paperpaw_dev::target::Target;

fn context(name: &str, paths: &[PathBuf]) -> Context {
    let out = std::env::temp_dir().join(format!(
        "paperpaw-dev-package-{name}-{}",
        std::process::id()
    ));
    let mut target = Target::default();
    for path in paths {
        target.take_path(path).unwrap();
    }
    let mut context = Context::new(target, out);
    // One device is enough here.
    context.profiles = vec![paperpaw_emulator::profiles::find("xteink-x4").unwrap()];
    context
}

fn run(context: &Context) -> Report {
    let mut report = Report::default();
    paperpaw_dev::package::run(context, &mut report).unwrap();
    let _ = std::fs::remove_dir_all(&context.out);
    report
}

fn has(report: &Report, check: &str, place: &str, level: Level) -> bool {
    report
        .findings
        .iter()
        .any(|f| f.check == check && f.place == place && f.level == level)
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/package")
        .join(name)
}

#[test]
fn a_plugin_wrong_everywhere_is_caught() {
    let report = run(&context("broken", &[fixture("broken-theme")]));
    let text = report.text(Level::Info);
    // As files: a font pack in fonts/ is taken, but this one does not
    // load on the device.
    for pack in ["fonts/cyrillic.ppfp", "fonts/greek.ppfp"] {
        assert!(
            !has(&report, "package.website", pack, Level::Error),
            "{text}"
        );
    }
    assert!(
        has(&report, "package.device", "fonts/greek.ppfp", Level::Error),
        "{text}"
    );
    assert!(
        has(&report, "package.files", "style.css", Level::Warning),
        "{text}"
    );
    // On the device.
    assert!(
        has(&report, "package.settings", "settings.ini:2", Level::Error),
        "{text}"
    );
    assert!(
        has(&report, "package.settings", "settings.ini:5", Level::Error),
        "{text}"
    );
    // A number out of its range, a choice not by its option's whole id,
    // and one place of a widget: each said with what it takes.
    let says = |place: &str, part: &str| {
        report
            .findings
            .iter()
            .any(|f| f.check == "package.settings" && f.place == place && f.message.contains(part))
    };
    assert!(says("settings.ini:8", "a whole number from"), "{text}");
    assert!(says("settings.ini:11", "menu.style.tiles"), "{text}");
    assert!(says("settings.ini:14", "`home.clock.date`"), "{text}");
    assert!(
        has(&report, "package.keymap", "keymap.ini:2", Level::Warning),
        "{text}"
    );
    assert!(
        has(&report, "package.presets", "presets.ini:2", Level::Warning),
        "{text}"
    );
    assert!(
        has(&report, "package.presets", "presets.ini:6", Level::Warning),
        "{text}"
    );
    assert!(
        has(&report, "package.fonts", "plugin.ini:9", Level::Error),
        "{text}"
    );
    // The pack of a font that is not there extends nothing (the error
    // above): the device opens a pack only for a font it has or wants, so
    // it says nothing of this one.
    assert!(
        !has(
            &report,
            "package.device",
            "fonts/cyrillic.ppfp",
            Level::Error
        ),
        "{text}"
    );
    // A preset's name that is a key no catalog has.
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.check == "package.presets" && f.message.contains("broken-theme.x")),
        "{text}"
    );
    assert!(
        report
            .findings
            .iter()
            .all(|f| f.plugin.as_deref() == Some("broken-theme"))
    );
}

#[test]
fn the_samples_load_and_a_release_is_compared() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples/plugins");
    // A theme as a preset beside them, which PaperPaw 0.3 could not read.
    let themes = std::env::temp_dir().join(format!("paperpaw-dev-themes-{}", std::process::id()));
    std::fs::create_dir_all(&themes).unwrap();
    std::fs::write(
        themes.join("plugin.ini"),
        "[plugin]\nid = themes\nname = Themes\ndescription = A theme.\nversion = 1.0.0\nformats = 1\n",
    )
    .unwrap();
    std::fs::write(
        themes.join("presets.ini"),
        "[tall]\nkind = core.theme\nname = Tall\ndescription = Taller rows.\ncore.ui-row = 180\n",
    )
    .unwrap();
    let mut context = context(
        "samples",
        &[
            root.join("airy-theme"),
            root.join("reading-nook"),
            themes.clone(),
        ],
    );
    context.release = Some("0.3.0".into());
    let report = run(&context);
    let text = report.text(Level::Info);
    assert!(
        !has(&report, "package.device", "plugin.ini", Level::Error),
        "{text}"
    );
    // 0.3 had no presets in data plugins.
    assert!(
        has(&report, "package.release", "presets.ini", Level::Error),
        "{text}"
    );
    let errors: Vec<_> = report
        .findings
        .iter()
        .filter(|f| f.level == Level::Error && f.check != "package.release")
        .collect();
    assert!(errors.is_empty(), "{text}");
    let _ = std::fs::remove_dir_all(&themes);
}

#[test]
fn ppn_files_are_checked_and_releases_must_be_versions() {
    let dir = std::env::temp_dir().join(format!("paperpaw-dev-ppn-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("hello-x4.ppn");
    std::fs::write(&file, b"PPNP").unwrap();
    let place = file.display().to_string();
    let mut context = context("ppn", &[file]);
    context.release = Some("not-a-version".into());
    let report = run(&context);
    // Cut short: the device refuses it.
    assert!(has(&report, "package.ppn", &place, Level::Error));
    assert!(has(
        &report,
        "package.release",
        "not-a-version",
        Level::Error
    ));
    assert_eq!(report.findings.len(), 2, "{}", report.text(Level::Info));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A code plugin's folder and its code name the same version: the
/// device loads no code of another.
#[test]
fn a_code_plugins_folder_names_its_codes_version() {
    // The widgets plugin's own version, as its crate names it: plugins
    // have versions of their own, not PaperPaw's.
    let manifest = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/widgets/Cargo.toml"),
    )
    .unwrap();
    let version = manifest
        .lines()
        .find_map(|l| l.strip_prefix("version = \""))
        .and_then(|v| v.strip_suffix('"'))
        .unwrap()
        .to_string();
    let folder = |name: &str, version: &str| {
        let ini = format!(
            "[plugin]\nid = widgets\nname = Widgets\ndescription = The widgets.\nversion = {version}\n"
        );
        let mut context = context(name, &[]);
        context.target.plugins = vec!["widgets".into()];
        context.target.data = vec![paperpaw_dev::target::DataPlugin::new(
            "widgets",
            [(String::from("plugin.ini"), ini.into_bytes())],
        )];
        run(&context)
    };
    // As the release writes it: the whole version, a beta's too.
    let report = folder("version-same", &version);
    assert!(
        !has(&report, "package.version", "plugin.ini", Level::Error),
        "{}",
        report.text(Level::Info)
    );
    // A beta's plugin.ini naming the release's numbers only is another
    // version: the store would not offer the next beta as an update.
    if version.contains('-') {
        let release = version.clone();
        let release = release.split('-').next().unwrap_or_default();
        let report = folder("version-release", release);
        assert!(has(&report, "package.version", "plugin.ini", Level::Error));
    }
    let report = folder("version-other", "9.9.9");
    assert!(
        has(&report, "package.version", "plugin.ini", Level::Error),
        "{}",
        report.text(Level::Info)
    );
}

/// A data plugin folder in a folder of its own: `plugin.ini` and the
/// other files (`path`, content).
fn folder_of(name: &str, ini: &str, files: &[(&str, &[u8])]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "paperpaw-dev-package-folder-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("plugin.ini"), ini).unwrap();
    for (path, bytes) in files {
        let file = dir.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, bytes).unwrap();
    }
    dir
}

/// Findings of `check` on a folder of `ini` and `files`, as (place, level).
fn found(name: &str, check: &str, ini: &str, files: &[(&str, &[u8])]) -> Vec<(String, Level)> {
    let dir = folder_of(name, ini, files);
    let report = run(&context(name, std::slice::from_ref(&dir)));
    let _ = std::fs::remove_dir_all(&dir);
    report
        .findings
        .iter()
        .filter(|f| f.check == check)
        .map(|f| (f.place.clone(), f.level))
        .collect()
}

#[test]
fn the_files_of_later_formats_need_the_formats_that_read_them() {
    let layouts: &[(&str, &[u8])] = &[("points/keyboard.layouts.txt", b"keyboard.layout.xx\n")];
    let ini = |formats: &str| format!("[plugin]\nid = cosy\nversion = 1.0.0\n{formats}");
    // Keyboards a plugin brings are read from formats 3 on; for formats 1
    // PaperPaw leaves them out.
    let none = found("points-1", "package.formats", &ini(""), layouts);
    assert!(
        none.contains(&("points/keyboard.layouts.txt".to_string(), Level::Error)),
        "{none:?}"
    );
    let three = found(
        "points-3",
        "package.formats",
        &ini("formats = 3\n"),
        layouts,
    );
    assert!(three.is_empty(), "{three:?}");
    // A compiled translation is read from formats 4 on.
    let catalog: &[(&str, &[u8])] = &[("i18n/ar.ppcat", b"PPCT")];
    let three = found("ppcat-3", "package.formats", &ini("formats = 3\n"), catalog);
    assert!(
        three.contains(&("i18n/ar.ppcat".to_string(), Level::Error)),
        "{three:?}"
    );
    let four = found("ppcat-4", "package.formats", &ini("formats = 4\n"), catalog);
    assert!(four.is_empty(), "{four:?}");
    let read = found(
        "ppcat-read",
        "package.files",
        &ini("formats = 4\n"),
        catalog,
    );
    assert!(read.is_empty(), "{read:?}");
    // [fonts] binds a role to a font, from formats 3 on too.
    let fonts = "\n[fonts]\ncore.font-reading-bold = cosy.bold\n";
    let old = found("fonts-1", "package.formats", &ini(fonts), &[]);
    assert!(
        old.contains(&("plugin.ini:6".to_string(), Level::Error)),
        "{old:?}"
    );
    let new = found(
        "fonts-3",
        "package.formats",
        &ini(&format!("formats = 3\n{fonts}")),
        &[],
    );
    assert!(new.is_empty(), "{new:?}");
}

#[test]
fn a_family_may_bring_faces_without_the_regular_one() {
    let pack: &[(&str, &[u8])] = &[("fonts/bold.ppfp", b"PPFP")];
    let ini = |extra: &str| {
        format!(
            "[plugin]\nid = cosy\nversion = 1.0.0\nformats = 3\n\n[family]\nname = Cosy\nbold = bold.ppfp\n{extra}"
        )
    };
    // Bound to a role with [fonts]: nothing to say.
    let bound = found(
        "family-bound",
        "package.fonts",
        &ini("\n[fonts]\ncore.font-reading-bold = cosy.bold\n"),
        pack,
    );
    assert!(!bound.iter().any(|(p, _)| p == "plugin.ini"), "{bound:?}");
    // Not bound: the faces are there and nothing uses them.
    let loose = found("family-loose", "package.fonts", &ini(""), pack);
    assert!(
        loose.contains(&("plugin.ini".to_string(), Level::Warning)),
        "{loose:?}"
    );
    assert!(
        !loose.contains(&("plugin.ini".to_string(), Level::Error)),
        "{loose:?}"
    );
}

#[test]
fn a_font_of_a_plugin_checked_with_it_is_a_font_to_extend() {
    let styles = folder_of(
        "styles",
        "[plugin]\nid = styles\nversion = 1.0.0\nformats = 3\n\n[family]\nname = Cosy\nbold = bold.ppfp\n",
        &[("fonts/bold.ppfp", b"PPFP")],
    );
    let pack = |font: &str| {
        folder_of(
            "extender",
            &format!("[plugin]\nid = extender\nversion = 1.0.0\n\n[extends]\nx.ppfp = {font}\n"),
            &[("fonts/x.ppfp", b"PPFP")],
        )
    };
    let fonts = |font: &str, together: bool| -> Vec<(String, Level)> {
        let extender = pack(font);
        let mut paths = vec![extender.clone()];
        if together {
            paths.push(styles.clone());
        }
        let report = run(&context("extends", &paths));
        let _ = std::fs::remove_dir_all(&extender);
        report
            .findings
            .iter()
            .filter(|f| f.check == "package.fonts" && f.place == "plugin.ini:6")
            .map(|f| (f.place.clone(), f.level))
            .collect()
    };
    // Together with the plugin that brings the font: found.
    assert!(fonts("styles.bold", true).is_empty());
    // Alone, a font of a plugin that is no part of the app: a note, not
    // an error (the plugin is installed beside it).
    assert_eq!(
        fonts("styles.bold", false),
        [("plugin.ini:6".to_string(), Level::Warning)]
    );
    // A face the plugin does not bring, or one of the app's own plugins
    // that is not there: the pack extends nothing.
    assert_eq!(
        fonts("styles.italic", true),
        [("plugin.ini:6".to_string(), Level::Warning)]
    );
    assert_eq!(
        fonts("fonts-default.nothing", false),
        [("plugin.ini:6".to_string(), Level::Error)]
    );
    let _ = std::fs::remove_dir_all(&styles);
}

#[test]
fn requires_needs_formats_4_and_plugin_ids_the_kernel_reads() {
    let ini = |extra: &str| format!("[plugin]\nid = cosy\nversion = 1.0.0\n{extra}");
    let none: &[(&str, &[u8])] = &[];
    let ok = found(
        "requires-ok",
        "package.requires",
        &ini("formats = 4\nrequires = script-arabic\n"),
        none,
    );
    assert!(ok.is_empty(), "{ok:?}");
    // Not read as a typo either: the same findings as without it.
    let typo = found(
        "requires-typo",
        "package.manifest",
        &ini("formats = 4\nrequires = script-arabic\n"),
        none,
    );
    let without = found(
        "requires-none",
        "package.manifest",
        &ini("formats = 4\n"),
        none,
    );
    assert_eq!(typo, without);
    let old = found(
        "requires-3",
        "package.requires",
        &ini("formats = 3\nrequires = script-arabic\n"),
        none,
    );
    assert_eq!(old, [("plugin.ini".to_string(), Level::Error)]);
    // A plugin that needs itself is refused by the kernel.
    let own = found(
        "requires-own",
        "package.requires",
        &ini("formats = 4\nrequires = cosy\n"),
        none,
    );
    assert_eq!(own, [("plugin.ini".to_string(), Level::Error)]);
}

/// The interface-7 sample library, built as its `verify.sh` does (into a
/// folder of its own, with nothing of the test run's cargo settings).
fn built_sample(dir: &Path) -> PathBuf {
    let sample = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../samples/abi7-tick-draw")
        .canonicalize()
        .unwrap();
    let target = dir.join("target");
    let mut build = std::process::Command::new("cargo");
    build.env_clear();
    for name in [
        "PATH",
        "HOME",
        "RUSTUP_HOME",
        "CARGO_HOME",
        "RUSTUP_TOOLCHAIN",
    ] {
        if let Some(value) = std::env::var_os(name) {
            build.env(name, value);
        }
    }
    let output = build
        .current_dir(sample)
        .env("CARGO_TARGET_DIR", &target)
        .args([
            "rustc",
            "--release",
            "--lib",
            "--locked",
            "--offline",
            "-j4",
            "--",
            "--crate-type=cdylib",
            "-Cpanic=abort",
            "-Clink-arg=-nostartfiles",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    std::fs::read_dir(target.join("release/deps"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    name.starts_with("libpaperpaw_sample_abi7_tick_draw-") && name.ends_with(".so")
                })
        })
        .expect("the sample's cdylib")
}

/// A freestanding library passes with the loader's rule; the same file cut
/// short, and a file that is no library, are refused (the loader's refusals
/// themselves are tested in `paperpaw-native`).
#[test]
fn so_libraries_are_checked_with_the_loaders_freestanding_rule() {
    let dir = std::env::temp_dir().join(format!("paperpaw-dev-so-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let built = built_sample(&dir);
    let good = dir.join("good.so");
    std::fs::copy(&built, &good).unwrap();
    let cut = dir.join("cut.so");
    let bytes = std::fs::read(&good).unwrap();
    std::fs::write(&cut, &bytes[..bytes.len() / 2]).unwrap();
    let junk = dir.join("junk.so");
    std::fs::write(&junk, b"not a library").unwrap();
    let report = run(&context("so", &[good.clone(), cut.clone(), junk.clone()]));
    let text = report.text(Level::Info);
    assert!(
        has(
            &report,
            "package.library",
            &good.display().to_string(),
            Level::Info
        ),
        "{text}"
    );
    for refused in [&cut, &junk] {
        assert!(
            has(
                &report,
                "package.library",
                &refused.display().to_string(),
                Level::Error
            ),
            "{text}"
        );
    }
    assert_eq!(report.findings.len(), 3, "{text}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Builds a tiny `no_std` library from `source` with `rustc` alone (no
/// cargo, no C library), linked with `extra` more arguments.
fn tiny_library(dir: &Path, name: &str, source: &str, extra: &[&str]) -> PathBuf {
    let prelude =
        "#![no_std]\n#[panic_handler]\nfn panic(_: &core::panic::PanicInfo) -> ! { loop {} }\n";
    let file = dir.join(format!("{name}.rs"));
    std::fs::write(&file, format!("{prelude}{source}")).unwrap();
    let out = dir.join(format!("{name}.so"));
    let output = std::process::Command::new("rustc")
        .args([
            "--edition",
            "2024",
            "--crate-type",
            "cdylib",
            "-Cpanic=abort",
            "-Clink-arg=-nostartfiles",
            "-Clink-arg=-nostdlib",
        ])
        .args(extra)
        .arg("-o")
        .arg(&out)
        .arg(&file)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    out
}

/// One library per way the loader refuses a freestanding plugin, each said
/// with what to look at (`nm -D`, `readelf -d`), and a machine it does not
/// read.
#[test]
fn so_libraries_are_refused_for_imports_exports_and_needed_libraries() {
    let dir = std::env::temp_dir().join(format!("paperpaw-dev-so-refused-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let entry = "#[unsafe(no_mangle)]\npub extern \"C\" fn PAPERPAW_ENTRY() {}\n";
    let export = tiny_library(
        &dir,
        "export",
        &format!("{entry}#[unsafe(no_mangle)]\npub extern \"C\" fn extra() {{}}\n"),
        &[],
    );
    let import = tiny_library(
        &dir,
        "import",
        "unsafe extern \"C\" { fn host_missing() -> i32; }\n\
         #[unsafe(no_mangle)]\npub extern \"C\" fn PAPERPAW_ENTRY() -> i32 { unsafe { host_missing() } }\n",
        &[],
    );
    let needed = tiny_library(
        &dir,
        "needed",
        entry,
        &["-Clink-arg=-Wl,--no-as-needed", "-Clink-arg=-lm"],
    );
    let other = dir.join("other.so");
    let mut bytes = std::fs::read(&export).unwrap();
    // e_machine: the 32-bit x86 (3), a machine the loaders do not read.
    bytes[18] = 3;
    bytes[19] = 0;
    std::fs::write(&other, bytes).unwrap();
    let all = [&export, &import, &needed, &other];
    let paths: Vec<PathBuf> = all.iter().map(|p| (*p).clone()).collect();
    let report = run(&context("so-refused", &paths));
    let text = report.text(Level::Info);
    assert_eq!(report.findings.len(), 4, "{text}");
    for (path, hint) in [
        (&export, "nm -D --defined-only"),
        (&import, "nm -D --undefined-only"),
        (&needed, "readelf -d"),
        (&other, "machine 3"),
    ] {
        let place = path.display().to_string();
        let finding = report
            .findings
            .iter()
            .find(|f| f.place == place)
            .unwrap_or_else(|| panic!("no finding for {place}: {text}"));
        assert_eq!(finding.check, "package.library", "{text}");
        assert_eq!(finding.level, Level::Error, "{text}");
        assert!(
            finding.message.contains(hint),
            "{place}: {}",
            finding.message
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}
