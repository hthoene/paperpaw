//! `check-publish` on repositories made here: a data plugin and a plugin
//! with code as the template makes them pass, with their release files;
//! each way a release would be refused is found, with what to change.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::path::{Path, PathBuf};

use paperpaw_native::header5::Header5;
use paperpaw_native::signature::sign;
use paperpaw_native::{Chip, LINK_BASE};
use paperpaw_publish::{Level, Problem, run};
use paperpaw_zip::testing::ZipBuilder;

/// A folder of its own for each test.
fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join("paperpaw-check-publish")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(dir: &Path, path: &str, bytes: impl AsRef<[u8]>) {
    let path = dir.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

/// A 1 x 1 PNG's start, enough to be taken for one.
const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x01\0\0\0\x01\x08\x06\0\0\0";

const STORE_INI: &str = "[store]\ngroup = look\ntags = theme, night\nlicence = MIT\n\n\
                         [screenshots]\nhome.png = The library at night\n";

const WORKFLOW: &str = "env:\n  CODE: \"false\"\n  PLUGIN_DIR: plugin\n";

/// A data plugin's repository as `paperpaw-dev new --github` makes it.
fn data_repo(name: &str) -> PathBuf {
    let dir = folder(name);
    write(
        &dir,
        "plugin/plugin.ini",
        "[plugin]\nid = night-owl\nname = Night owl\ndescription = A dark theme for reading at night.\nversion = 1.0.0\n",
    );
    write(&dir, "plugin/theme.css", "body {}\n");
    write(&dir, "plugin/store/store.ini", STORE_INI);
    write(&dir, "plugin/store/home.png", PNG);
    write(&dir, ".github/workflows/paperpaw-release.yml", WORKFLOW);
    write(
        &dir,
        "README.md",
        "Give the repository the topic paperpaw-plugin.\n",
    );
    write(&dir, "LICENSE", "MIT\n");
    dir
}

/// A plugin file for `chip`, of plugin interface `interface`, signed with
/// the key of `seed` (or not).
fn ppn(chip: Chip, interface: u16, seed: Option<[u8; 32]>) -> Vec<u8> {
    let image = [0u8; 128];
    let mut header = Header5 {
        target: chip,
        abi_major: interface,
        needs_minor: 0,
        base: LINK_BASE,
        image_len: image.len() as u32,
        code_at: 96,
        entry: 0,
        meta_at: 32,
        meta_len: 4,
        relocs: 0,
        align: 16,
        hot_at: 0,
        hot_len: 0,
        patches: 0,
        heap_at: None,
        sha256: [0; 32],
        signed: false,
    };
    let mut sha = header.digest();
    sha.update(&image);
    header.sha256 = sha.finish();
    let mut file = header.to_bytes().to_vec();
    file.extend_from_slice(&image);
    if let Some(seed) = seed {
        file.extend_from_slice(&sign(&file, &seed).to_bytes());
    }
    file
}

const AUTHOR: [u8; 32] = [7; 32];

/// A plugin with code's repository, as the template makes it, and its
/// release files.
fn code_repo(name: &str) -> PathBuf {
    let dir = folder(name);
    write(
        &dir,
        "plugin/plugin.ini",
        "[plugin]\nid = clock\nname = Clock\ndescription = A clock on the home screen.\nversion = 1.0.0\n",
    );
    write(&dir, "plugin/store/store.ini", STORE_INI);
    write(&dir, "plugin/store/home.png", PNG);
    write(
        &dir,
        "plugin/how/en.txt",
        "Shows the time on the home screen, in the corner, and updates it once a minute with a \
         partial refresh. It needs nothing else: no network, no card.",
    );
    write(
        &dir,
        ".github/workflows/paperpaw-release.yml",
        "env:\n  CODE: \"true\"\n  PLUGIN_DIR: plugin\n",
    );
    write(
        &dir,
        "Cargo.toml",
        "[package]\nname = \"clock\"\n\n[dependencies]\npaperpaw-sdk = \"0.5.0-beta.8\"\n",
    );
    write(&dir, "Cargo.lock", "version = 4\n");
    write(&dir, ".gitignore", "/target/\n*.key\n");
    write(&dir, "README.md", "Topic: paperpaw-plugin.\n");
    write(&dir, "LICENSE", "MIT\n");
    let zip = ZipBuilder::new()
        .stored(
            "plugin/plugin.ini",
            std::fs::read(dir.join("plugin/plugin.ini")).unwrap(),
        )
        .stored("plugin/store/store.ini", STORE_INI)
        .build();
    write(&dir, "release/clock-1.0.0.zip", zip);
    write(
        &dir,
        "release/clock-1.0.0-x4.ppn",
        ppn(Chip::Esp32C3, 2, Some(AUTHOR)),
    );
    write(
        &dir,
        "release/clock-1.0.0-x4-pro.ppn",
        ppn(Chip::Esp32S3, 2, Some(AUTHOR)),
    );
    dir
}

fn check(dir: &Path, more: &[&str]) -> Vec<Problem> {
    let mut args = vec![dir.display().to_string()];
    args.extend(more.iter().map(ToString::to_string));
    run(&args, Some(2)).unwrap()
}

fn errors(problems: &[Problem]) -> Vec<String> {
    problems
        .iter()
        .filter(|p| p.level == Level::Error)
        .map(ToString::to_string)
        .collect()
}

/// That an error says `text`.
fn says(problems: &[Problem], text: &str) {
    let errors = errors(problems);
    assert!(
        errors.iter().any(|e| e.contains(text)),
        "{text}: {errors:#?}"
    );
}

#[test]
fn a_data_plugin_as_the_template_makes_it_passes() {
    let dir = data_repo("data-good");
    let problems = check(&dir, &["--tag", "v1.0.0"]);
    assert!(errors(&problems).is_empty(), "{problems:#?}");
}

#[test]
fn a_plugin_with_code_and_its_release_pass() {
    let dir = code_repo("code-good");
    let release = dir.join("release").display().to_string();
    let problems = check(&dir, &["--release-dir", &release, "--tag", "v1.0.0"]);
    assert!(errors(&problems).is_empty(), "{problems:#?}");
}

#[test]
fn store_entries_the_site_refuses_are_found() {
    let dir = data_repo("data-entry");
    // Without a licence file GitHub names none either.
    std::fs::remove_file(dir.join("LICENSE")).unwrap();
    write(
        &dir,
        "plugin/store/store.ini",
        "[store]\ngroup = themes\ntags = a, b, c, d, e, f, Bad_Tag\n\n[i18n.en]\nname = x\n",
    );
    let problems = check(&dir, &[]);
    says(
        &problems,
        "the group `themes` is none of the store's: use one of reading,",
    );
    says(&problems, "7 tags: at most 5");
    says(&problems, "the tag `Bad_Tag`");
    says(&problems, "not en");
    says(&problems, "a translation needs a name and a summary");
    says(&problems, "say what it shows: `home.png = ...`");
    says(&problems, "no licence");
    write(&dir, "plugin/store/store.ini", "");
    std::fs::remove_file(dir.join("plugin/store/store.ini")).unwrap();
    says(&check(&dir, &[]), "no store/store.ini");
}

#[test]
fn manifests_and_folders_the_site_refuses_are_found() {
    let dir = data_repo("data-manifest");
    write(
        &dir,
        "plugin/plugin.ini",
        "[plugin]\nid = Night_Owl\nname = Night owl\nversion = 1.0\n",
    );
    write(&dir, "plugin/tool.exe", "x");
    write(&dir, "plugin/notes.key", "secret");
    let problems = check(&dir, &["--tag", "v1.0.1"]);
    says(&problems, "the id `Night_Owl`");
    says(&problems, "the version `1.0` is no SemVer version");
    says(&problems, "no summary");
    says(&problems, "plugin/tool.exe");
    says(&problems, "a key file in the repository");
    // The folder at the top holds the repository's own files: refused.
    let top = data_repo("data-top");
    for f in ["plugin.ini", "theme.css"] {
        std::fs::copy(top.join("plugin").join(f), top.join(f)).unwrap();
    }
    write(
        &top,
        ".github/workflows/paperpaw-release.yml",
        "env:\n  CODE: \"false\"\n  PLUGIN_DIR: .\n",
    );
    says(&check(&top, &[]), "LICENSE");
}

#[test]
fn a_tag_that_is_not_the_version_is_refused() {
    let dir = data_repo("data-tag");
    says(
        &check(&dir, &["--tag", "v1.0.1"]),
        "plugin.ini says version 1.0.0: tag it v1.0.0",
    );
}

#[test]
fn release_files_the_site_refuses_are_found() {
    let dir = code_repo("code-release");
    let release = dir.join("release");
    let r = release.display().to_string();
    // Unsigned, another key, the wrong reader's name, another interface.
    write(&release, "clock-1.0.0-x4.ppn", ppn(Chip::Esp32C3, 2, None));
    write(
        &release,
        "clock-1.0.0-x4-pro.ppn",
        ppn(Chip::Esp32S3, 3, Some([9; 32])),
    );
    write(
        &release,
        "clock-1.0.0-x3.ppn",
        ppn(Chip::Esp32S3, 2, Some(AUTHOR)),
    );
    write(
        &release,
        "clock-1.0.0-kindle.ppn",
        ppn(Chip::Esp32C3, 2, Some(AUTHOR)),
    );
    let problems = check(&dir, &["--release-dir", &r]);
    says(&problems, "not signed: sign it with your author key");
    says(
        &problems,
        "built for plugin interface 3, and the PaperPaw it is for has 2",
    );
    says(&problems, "which is not the x3's chip");
    says(&problems, "names no reader");
    says(&problems, "signed by different keys");
    // No folder's ZIP, and no builds at all.
    for name in std::fs::read_dir(&release).unwrap() {
        std::fs::remove_file(name.unwrap().path()).unwrap();
    }
    let problems = check(&dir, &["--release-dir", &r]);
    says(&problems, "no clock-1.0.0.zip");
    // A ZIP of the files inside, without store/.
    write(
        &release,
        "clock-1.0.0.zip",
        ZipBuilder::new().stored("theme.css", "x").build(),
    );
    let problems = check(&dir, &["--release-dir", &r]);
    says(&problems, "no plugin.ini at its top");
    says(
        &problems,
        "a plugin with code needs a signed build for each reader",
    );
}

#[test]
fn a_build_no_reader_takes_is_refused() {
    let dir = code_repo("code-refused");
    let release = dir.join("release");
    let r = release.display().to_string();
    let good = ppn(Chip::Esp32C3, 2, Some(AUTHOR));
    // A file of the format before 5.
    let mut old = good.clone();
    old[4..6].copy_from_slice(&4u16.to_le_bytes());
    write(&release, "clock-1.0.0-x4.ppn", old);
    says(
        &check(&dir, &["--release-dir", &r]),
        "a plugin file of format 4",
    );
    // One byte of the image changed: the digest no longer holds.
    let mut changed = good.clone();
    changed[100] ^= 1;
    write(&release, "clock-1.0.0-x4.ppn", changed);
    says(
        &check(&dir, &["--release-dir", &r]),
        "readers refuse it: Damaged",
    );
    // Cut short.
    write(&release, "clock-1.0.0-x4.ppn", &good[..good.len() - 1]);
    says(
        &check(&dir, &["--release-dir", &r]),
        "readers refuse it: Length",
    );
}

#[test]
fn a_plugin_with_code_needs_its_lock_its_sdk_by_version_and_the_right_switch() {
    let dir = code_repo("code-repo");
    std::fs::remove_file(dir.join("Cargo.lock")).unwrap();
    write(
        &dir,
        "Cargo.toml",
        "[dependencies]\npaperpaw-sdk = { path = \"../paperpaw/crates/paperpaw-sdk\" }\n",
    );
    write(
        &dir,
        ".github/workflows/paperpaw-release.yml",
        "env:\n  CODE: \"false\"\n  PLUGIN_DIR: plugin\n",
    );
    let problems = check(&dir, &[]);
    says(&problems, "no Cargo.lock");
    says(
        &problems,
        "name the SDK of a PaperPaw release by its version",
    );
    says(
        &problems,
        "CODE is \"false\", but the repository has a Cargo.toml",
    );
}

#[test]
fn the_sites_rules_are_taken_where_given() {
    let dir = data_repo("data-rules");
    let mut rules = paperpaw_publish::rules::BUILT_IN.to_string();
    rules = rules.replace("\"look\", ", "");
    write(&dir, "rules.json", rules);
    let file = dir.join("rules.json").display().to_string();
    let problems = check(&dir, &["--rules", &file]);
    says(&problems, "the group `look` is none of the store's");
    assert!(problems.iter().any(|p| p.code == "rules.newer"));
}

/// Makes `dir` a plugin whose code depends on a stand-in SDK (`kernel` is
/// a feature of it) and on `helper` (which names it as `helper_sdk`), with
/// the lock Cargo wants for that.
fn with_kernel_graph(dir: &Path, plugin_sdk: &str, helper_sdk: Option<&str>) {
    write(
        dir,
        "sdk/Cargo.toml",
        "[package]\nname = \"paperpaw-sdk\"\nversion = \"0.5.0-beta.8\"\nedition = \"2021\"\n\n\
         [features]\nkernel = []\n",
    );
    write(dir, "sdk/src/lib.rs", "");
    let mut lock =
        String::from("version = 4\n\n[[package]]\nname = \"clock\"\nversion = \"0.1.0\"\n");
    let mut toml = format!(
        "[package]\nname = \"clock\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\npaperpaw-sdk = {plugin_sdk}\n"
    );
    let mut deps = String::from("dependencies = [\n \"paperpaw-sdk\",\n");
    if let Some(sdk) = helper_sdk {
        write(
            dir,
            "helper/Cargo.toml",
            format!(
                "[package]\nname = \"fonts-default\"\nversion = \"1.0.0\"\nedition = \"2021\"\n\n[dependencies]\npaperpaw-sdk = {sdk}\n"
            ),
        );
        write(dir, "helper/src/lib.rs", "");
        toml.push_str("fonts-default = { path = \"helper\" }\n");
        deps.push_str(" \"fonts-default\",\n");
    }
    deps.push_str("]\n");
    lock.push_str(&deps);
    if helper_sdk.is_some() {
        lock.push_str("\n[[package]]\nname = \"fonts-default\"\nversion = \"1.0.0\"\ndependencies = [\n \"paperpaw-sdk\",\n]\n");
    }
    lock.push_str("\n[[package]]\nname = \"paperpaw-sdk\"\nversion = \"0.5.0-beta.8\"\n");
    write(dir, "Cargo.toml", toml);
    write(dir, "src/lib.rs", "");
    write(dir, "Cargo.lock", lock);
}

const SDK_PLAIN: &str = "{ path = \"sdk\" }";
const HELPER_PLAIN: &str = "{ path = \"../sdk\" }";
const HELPER_KERNEL: &str = "{ path = \"../sdk\", features = [\"kernel\"] }";
const SDK_KERNEL: &str = "{ path = \"sdk\", features = [\"kernel\"] }";

#[test]
fn a_plugin_that_turns_on_the_sdks_kernel_feature_is_refused() {
    let dir = code_repo("kernel-direct");
    with_kernel_graph(&dir, SDK_KERNEL, None);
    says(
        &check(&dir, &[]),
        "plugin clock turns on paperpaw-sdk/kernel itself",
    );
}

#[test]
fn a_plugin_whose_dependency_turns_on_the_kernel_feature_is_refused() {
    let dir = code_repo("kernel-via");
    with_kernel_graph(&dir, SDK_PLAIN, Some(HELPER_KERNEL));
    says(
        &check(&dir, &[]),
        "plugin clock turns on paperpaw-sdk/kernel via fonts-default",
    );
}

#[test]
fn a_plugin_without_the_kernel_feature_passes_the_check() {
    let dir = code_repo("kernel-none");
    with_kernel_graph(&dir, SDK_PLAIN, Some(HELPER_PLAIN));
    let problems = check(&dir, &[]);
    assert!(
        problems.iter().all(|p| p.code != "source.kernel"),
        "{problems:#?}"
    );
}
