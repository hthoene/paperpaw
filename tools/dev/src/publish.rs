//! Publishing from GitHub, made hard to get wrong:
//!
//! - `check-publish [repo] [--release-dir <dir>] [--tag <tag>] [--rules
//!   <file>]`: the repository (and its release files) checked by the
//!   plugin store's rules ([`paperpaw_publish`]), with what to change;
//!   the release workflow runs the same check before it releases anything;
//! - `new --github <id> [--code] [--sdk <path>] [--dir <path>]`: a
//!   repository that passes it from the start: the plugin's folder with
//!   `plugin.ini` and `store/store.ini`, the release workflow with its
//!   switches set, a README that says what is left to do, `.gitignore`.
//!   With `--code` also a plugin with code for plugin interface 7
//!   ([`code_files`]): a crate with `native_plugin!`, its declarations,
//!   the build's configuration for the X4 and the record of what it calls.

use std::path::{Path, PathBuf};

/// The release workflow template, with `PLUGIN_DIR` and `PAPERPAW` to set.
const WORKFLOW: &str = include_str!("../templates/.github/workflows/paperpaw-release.yml");

/// The PaperPaw release this tool is of, which a new plugin is made for.
const PAPERPAW: &str = env!("CARGO_PKG_VERSION");

/// The template of a plugin with code (`templates/code/`), `@ID@`, `@NAME@`,
/// `@TYPE@` and `@SDK@` to set.
const CODE_CARGO: &str = include_str!("../templates/code/Cargo.toml.in");
const CODE_MAIN: &str = include_str!("../templates/code/main.rs.in");
const CODE_CONFIG: &str = include_str!("../templates/code/cargo-config.toml");
const CODE_TOOLCHAIN: &str = include_str!("../templates/code/rust-toolchain.toml");
/// What the template's code calls of the device, and the record made of it
/// (`tools/abi-metadata.py`; `tools/tests` holds the two together).
const CODE_METADATA: &str = include_str!("../templates/code/metadata.json");
const CODE_METADATA_BYTES: &[u8] = include_bytes!("../templates/code/metadata.bin");
/// The linker script of a plugin file, the release's own.
const CODE_LINKER: &str = include_str!("../../../crates/paperpaw-native/paperpaw-plugin7.x");

/// The template's records beside the X4's (for the X4 Pro and X4 Classic, and
/// for the emulator), made of its list.
fn other_records() -> Vec<(String, Vec<u8>)> {
    crate::metadata::records(CODE_METADATA)
        .map(|(made, _)| made)
        .unwrap_or_default()
        .into_iter()
        .skip(1)
        .map(|(file, bytes)| (file.to_string(), bytes))
        .collect()
}

/// `check-publish`: whether the plugin store takes it.
pub fn check(args: &[String]) -> Result<bool, String> {
    let problems = paperpaw_publish::run(args, None)?;
    for problem in &problems {
        println!("{problem}");
    }
    let passes = paperpaw_publish::passes(&problems);
    if passes {
        println!("check-publish: the plugin store takes it");
    } else {
        println!("check-publish: the plugin store would refuse it; change what the errors say");
    }
    Ok(passes)
}

/// A plugin's name from its id: `night-owl` is Night owl.
fn name_of(id: &str) -> String {
    let words = id.replace('-', " ");
    let mut chars = words.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// The files of a new repository for plugin `id`, by their paths.
pub fn files(id: &str) -> Vec<(String, String)> {
    let name = name_of(id);
    vec![
        (
            "plugin/plugin.ini".to_string(),
            format!(
                "[plugin]\nid = {id}\nname = {name}\n\
                 # One sentence on what it does, at most 160 characters: the store's summary.\n\
                 description = Say in one sentence what {name} does.\nversion = 0.1.0\n"
            ),
        ),
        ("plugin/store/store.ini".to_string(), store_ini()),
        (
            ".github/workflows/paperpaw-release.yml".to_string(),
            workflow(),
        ),
        ("README.md".to_string(), readme(id, &name)),
        (
            ".gitignore".to_string(),
            "/target/\n/dist/\n/release/\n# A link to a PaperPaw checkout on this computer (`new --code --sdk`).\n\
             /.paperpaw\n# The author's signing key never goes into the repository: keep it\n\
             # outside (e.g. ~/.paperpaw/author.key) and in the CI's secrets.\n*.key\n"
                .to_string(),
        ),
    ]
}

/// A Rust type's name from a plugin's id: `night-owl` is `NightOwl`, `7-up`
/// is `Plugin7Up`.
fn type_of(id: &str) -> String {
    let mut name: String = id
        .split('-')
        .filter(|word| !word.is_empty())
        .map(|word| {
            let mut chars = word.chars();
            chars
                .next()
                .map(|c| c.to_ascii_uppercase().to_string() + chars.as_str())
                .unwrap_or_default()
        })
        .collect();
    if !name.starts_with(|c: char| c.is_ascii_alphabetic()) {
        name.insert_str(0, "Plugin");
    }
    name
}

/// The link in a plugin's folder to the PaperPaw checkout its SDK is taken
/// from (`new --code --sdk`), and the SDK's crate through it.
pub const SDK_LINK: &str = ".paperpaw";
const SDK_IN_LINK: &str = ".paperpaw/crates/paperpaw-sdk";

/// The PaperPaw checkout whose `crates/paperpaw-sdk` is `sdk`, if it is one.
fn checkout_of(sdk: &Path) -> Option<&Path> {
    sdk.ends_with("crates/paperpaw-sdk")
        .then(|| sdk.parent()?.parent())
        .flatten()
}

/// Makes `link` a link to the folder `to`; whether it could (not on a
/// Windows that keeps links to folders for administrators).
fn link_folder(to: &Path, link: &Path) -> bool {
    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(to, link);
    #[cfg(windows)]
    let made = std::os::windows::fs::symlink_dir(to, link);
    #[cfg(not(any(unix, windows)))]
    let made: std::io::Result<()> = Err(std::io::ErrorKind::Unsupported.into());
    made.is_ok()
}

/// The files a plugin with code has beside those of [`files`]: the crate
/// (`Cargo.toml`, `src/main.rs`), how it is built for the X4
/// (`.cargo/config.toml`, the linker script, the compiler), the record of
/// the device's functions it calls (`native/`), and its words. The SDK is
/// the one of this tool's release, or the checkout at `sdk`, named as it is;
/// `new` names it through the plugin's own link ([`SDK_LINK`]) where it can.
pub fn code_files(id: &str, sdk: Option<&Path>) -> Vec<(String, Vec<u8>)> {
    let name = name_of(id);
    let sdk = match sdk {
        // TOML's literal string: a path is taken as it is.
        Some(path) => format!("{{ path = '{}' }}", path.display()),
        None => format!("\"={PAPERPAW}\""),
    };
    let set = |text: &str| {
        text.replace("@ID@", id)
            .replace("@NAME@", &name)
            .replace("@TYPE@", &type_of(id))
            .replace("@SDK@", &sdk)
            .into_bytes()
    };
    let mut files = vec![
        ("Cargo.toml".to_string(), set(CODE_CARGO)),
        ("src/main.rs".to_string(), set(CODE_MAIN)),
        (".cargo/config.toml".to_string(), set(CODE_CONFIG)),
        ("rust-toolchain.toml".to_string(), set(CODE_TOOLCHAIN)),
        ("paperpaw-plugin7.x".to_string(), set(CODE_LINKER)),
        ("native/metadata.json".to_string(), set(CODE_METADATA)),
        (
            "native/metadata.bin".to_string(),
            CODE_METADATA_BYTES.to_vec(),
        ),
        (
            "plugin/i18n/en.ini".to_string(),
            format!(
                "[meta]\nlanguage = en\nname = English\nplural = one-other\n\n[messages]\n\
                 {id}.plugin.name = {name}\n{id}.title = {name}\n\
                 {id}.setting.name = Name to greet\n\
                 {id}.setting.name.help = Who the greeting on the plugin's screen is for.\n"
            )
            .into_bytes(),
        ),
        (
            "CODE.md".to_string(),
            format!(
                "# {name}: the code\n\n\
                 `src/main.rs` is the plugin: an entry in the tools menu that opens a screen. It is \
                 written for PaperPaw's plugin interface 7 and built without the standard library.\n\n\
                 - **Build it:** `paperpaw-dev build` for the Xteink X4 and X3, \
                 `paperpaw-dev build --device x4-pro` for the X4 Pro and X4 Classic. It says what is \
                 missing of the compiler and how to install it, builds the program, packs it \
                 (the packer `paperpaw-sign` comes with PaperPaw) and leaves the plugin as it \
                 goes onto a card in `dist/<device>/{id}`. \
                 `rust-toolchain.toml` names the X4's compiler, `.cargo/config.toml` and \
                 `paperpaw-plugin7.x` how the program is linked; leave `RUSTFLAGS` unset. Commit \
                 the `Cargo.lock` the first build writes: `check-publish` asks for it.\n\
                 - **What the build leaves:** `dist/x4/{id}` (or `dist/x4-pro/{id}`) is the plugin's \
                 folder as it lies in the folder `plugins` of a card, with the code in it. The \
                 code is not signed.\n\
                 - **Try it in the emulator** (Linux, x86-64): `paperpaw-dev build --device emulator` \
                 builds the plugin for the computer and leaves `dist/emulator`, a library for the \
                 emulator with the plugin in it; `paperpaw-emu --library dist/emulator --trust-code` \
                 starts it as an X4, with the X4's memory. Build again and start again to see a \
                 change. The emulator comes with PaperPaw and must be built by the compiler \
                 `rust-toolchain.toml` names: the build says how to install it.\n\
                 - **If the packer refuses the program:** it says which function or data it is \
                 about. A plugin keeps no data it writes outside what `make` returns (no `static \
                 mut`, no cell in a `static`), and no function of it may take 2,048 bytes of stack \
                 or more: put a large buffer on the heap.\n\
                 - **What it may call:** `native/metadata.json` lists the device's functions the code \
                 calls. `paperpaw-dev metadata` (and every build) makes the records of it: \
                 `native/metadata.bin` and `native/metadata.s3.bin`, one for each chip, and \
                 `native/metadata.host.bin` for the emulator. A call that is not listed is \
                 refused: add it to the list with the code.\n\
                 - **Its words:** `plugin/i18n/en.ini`; add a file for each language.\n"
            )
            .into_bytes(),
        ),
    ];
    files.extend(other_records());
    files
}

/// `store/store.ini`, with a group of the store's and room for the rest.
fn store_ini() -> String {
    let rules = paperpaw_publish::Rules::built_in();
    format!(
        "# What the plugin store at paperpaw.org shows of the plugin. Only the store\n\
         # reads store/; it is taken out of what readers install.\n\
         # Comments go on lines of their own, never after a value.\n\n\
         [store]\n\
         # One of: {}.\n\
         group = tools\n\
         # At most {} tags, lower-case words with hyphens, separated by commas.\n\
         tags = \n\
         # An SPDX identifier.\n\
         licence = MIT\n\
         sdk = {}\n\n\
         [screenshots]\n\
         # A picture of the plugin at work in store/, and what it shows, e.g.\n\
         # home.png = The plugin on the home screen\n",
        rules.groups.join(", "),
        rules.max_tags,
        PAPERPAW.split('.').take(2).collect::<Vec<_>>().join("."),
    )
}

/// The release workflow with its PaperPaw release set.
fn workflow() -> String {
    WORKFLOW
        .lines()
        .map(|line| {
            if line.trim_start().starts_with("PAPERPAW: \"") {
                format!("  PAPERPAW: \"{PAPERPAW}\"")
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

/// What is left to do, in the new repository's README.
fn readme(id: &str, name: &str) -> String {
    format!(
        "# {name}\n\n\
         A plugin for [PaperPaw](https://paperpaw.org). Its folder is `plugin/`; \
         `plugin/store/store.ini` says what the plugin store shows of it.\n\n\
         ## Publishing\n\n\
         1. Make the repository public on GitHub and give it the topic `paperpaw-plugin` \
         (About, the gear, Topics): the store finds it by that.\n\
         2. Write `description` in `plugin/plugin.ini`, choose the `group` and `tags` in \
         `plugin/store/store.ini`, and add a screenshot to `plugin/store/` with what it shows \
         under `[screenshots]`.\n\
         3. Check it as the store will: `paperpaw-dev check-publish` (or `paperpaw-sign \
         check-publish`).\n\
         4. Tag the version of `plugin.ini` and push the tag: `git tag v0.1.0`, \
         `git push --tags`. The workflow checks it again, then makes the release; within the \
         hour the store lists `{id}` under Community plugins, unreviewed. The reviewed badge you \
         can ask for later, in the store's form (From GitHub).\n"
    )
}

/// `new --github <id> [--code] [--sdk <path>] [--dir <path>]`: writes a new
/// repository (into `<id>/` unless named), never over files that are there.
/// `--code` adds a plugin with code ([`code_files`]), which passes
/// `check-publish` once its first build's `Cargo.lock` is there; `--sdk` names a
/// PaperPaw checkout's `crates/paperpaw-sdk` to build it against instead of
/// the published SDK of this tool's release.
pub fn new(args: &[String]) -> Result<bool, String> {
    let (mut id, mut dir, mut github) = (None, None, false);
    let (mut code, mut sdk) = (false, None);
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--github" => github = true,
            "--code" => code = true,
            "--sdk" => sdk = Some(PathBuf::from(args.next().ok_or("--sdk needs a path")?)),
            "--dir" => dir = Some(PathBuf::from(args.next().ok_or("--dir needs a path")?)),
            other if other.starts_with("--") => return Err(format!("unknown option {other}")),
            other => id = Some(other.to_string()),
        }
    }
    if !github {
        return Err("new makes a plugin's own repository for GitHub: `new --github <id>`".into());
    }
    let id = id.ok_or("name the plugin's id: `new --github night-owl`")?;
    let rules = paperpaw_publish::Rules::built_in();
    if !rules.valid_id(&id) {
        return Err(format!(
            "`{id}` is no plugin id: lower-case letters, digits and hyphens, at most {}",
            rules.max_id
        ));
    }
    if sdk.is_some() && !code {
        return Err("--sdk goes with --code: only a plugin with code is built with the SDK".into());
    }
    if code && !id.starts_with(|c: char| c.is_ascii_lowercase()) {
        return Err(format!(
            "`{id}` cannot have code: the id is the package's name, which Cargo wants to begin \
             with a letter"
        ));
    }
    // Cargo reads the path from the new repository's folder, not from here.
    let sdk = match sdk {
        Some(path) => Some(
            std::path::absolute(&path)
                .map_err(|error| format!("--sdk {}: {error}", path.display()))?,
        ),
        None => None,
    };
    let dir = dir.unwrap_or_else(|| PathBuf::from(&id));
    let mut all: Vec<(String, Vec<u8>)> = files(&id)
        .into_iter()
        .map(|(path, text)| (path, text.into_bytes()))
        .collect();
    // A checkout's SDK is reached through a link in the plugin's own folder,
    // and named by that: Cargo makes a path outside the folder a part of
    // the build, so that the same source would build to another file on
    // another computer, and the plugin store could not rebuild it. Named
    // through the link, the build is the same wherever the checkout lies.
    let mut linked = false;
    if let Some(checkout) = sdk.as_deref().and_then(checkout_of) {
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let link = dir.join(SDK_LINK);
        if link.exists() || link.is_symlink() {
            return Err(format!("{} is there already", link.display()));
        }
        linked = link_folder(checkout, &link);
    }
    if code {
        let named = if linked {
            Some(Path::new(SDK_IN_LINK))
        } else {
            sdk.as_deref()
        };
        all.extend(code_files(&id, named));
    }
    if sdk.is_some() && !linked {
        println!(
            "note: the SDK is named by its path on this computer (it is no PaperPaw checkout's \
             crates/paperpaw-sdk, or a link to it could not be made here): the plugin builds, \
             but the plugin store cannot rebuild it to the same file"
        );
    }
    write(&dir, &all)?;
    println!(
        "made {}: see its README for what is left to do",
        dir.display()
    );
    Ok(true)
}

/// Writes `files` below `dir`, refusing to overwrite one.
fn write(dir: &Path, files: &[(String, Vec<u8>)]) -> Result<(), String> {
    for (path, _) in files {
        if dir.join(path).exists() {
            return Err(format!(
                "{} is there already: new writes into an empty folder",
                dir.join(path).display()
            ));
        }
    }
    for (path, text) in files {
        let path = dir.join(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn made(name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join("paperpaw-dev-new")
            .join(format!("{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let args: Vec<String> = ["--github", "night-owl", "--dir", &dir.display().to_string()]
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(new(&args), Ok(true));
        dir
    }

    fn errors(dir: &Path) -> Vec<String> {
        paperpaw_publish::run(&[dir.display().to_string()], None)
            .unwrap()
            .iter()
            .filter(|p| p.level == paperpaw_publish::Level::Error)
            .map(ToString::to_string)
            .collect()
    }

    #[test]
    fn the_rules_interface_is_the_sdks() {
        assert_eq!(
            paperpaw_publish::interface(),
            paperpaw_sdk::native::INTERFACE
        );
    }

    #[test]
    fn a_new_data_plugins_repository_passes_check_publish() {
        let dir = made("data");
        assert_eq!(errors(&dir), Vec::<String>::new());
        let workflow =
            std::fs::read_to_string(dir.join(".github/workflows/paperpaw-release.yml")).unwrap();
        assert!(workflow.contains(&format!("  PAPERPAW: \"{PAPERPAW}\"")));
        // Never over what is there.
        let again: Vec<String> = ["--github", "night-owl", "--dir", &dir.display().to_string()]
            .iter()
            .map(ToString::to_string)
            .collect();
        assert!(new(&again).unwrap_err().contains("is there already"));
    }

    #[test]
    fn a_new_plugin_with_code_passes_check_publish_and_has_its_crate() {
        let dir = std::env::temp_dir()
            .join("paperpaw-dev-new")
            .join(format!("code-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let args: Vec<String> = [
            "--github",
            "night-owl",
            "--code",
            "--dir",
            &dir.display().to_string(),
        ]
        .iter()
        .map(ToString::to_string)
        .collect();
        assert_eq!(new(&args), Ok(true));
        // All the store asks for beyond what `new` can write: the lock file of
        // the first build.
        let errors = errors(&dir);
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].contains("no Cargo.lock"), "{errors:?}");
        let read = |path: &str| std::fs::read_to_string(dir.join(path)).unwrap();
        let main = read("src/main.rs");
        assert!(main.contains("native_plugin!(NightOwl,"));
        assert!(main.contains("ActionDecl::new(\"open\", \"night-owl.title\", OPEN)"));
        assert!(read("Cargo.toml").contains(&format!("paperpaw-sdk = \"={PAPERPAW}\"")));
        assert!(read("plugin/i18n/en.ini").contains("night-owl.title = Night owl"));
        for file in [
            "Cargo.toml",
            "src/main.rs",
            "native/metadata.json",
            "CODE.md",
        ] {
            assert!(!read(file).contains('@'), "{file} has a placeholder left");
        }
        assert_eq!(
            std::fs::read(dir.join("native/metadata.bin")).unwrap(),
            CODE_METADATA_BYTES
        );
        assert_eq!(read("paperpaw-plugin7.x"), CODE_LINKER);
        // The records are what the list gives.
        assert_eq!(crate::metadata::write(&dir, true), Ok(true));
        // `--sdk` without `--code` builds nothing.
        let alone: Vec<String> = ["--github", "x", "--sdk", "."]
            .iter()
            .map(ToString::to_string)
            .collect();
        assert!(new(&alone).unwrap_err().contains("--sdk goes with --code"));
    }

    /// A checkout's SDK is named through a link in the plugin's own folder,
    /// so that the same source builds to the same file wherever the checkout
    /// lies (and the plugin store can rebuild it); an SDK that is no
    /// checkout's is named by its path, as before.
    #[cfg(unix)]
    #[test]
    fn a_checkouts_sdk_is_named_through_a_link_in_the_plugins_folder() {
        let top = std::env::temp_dir()
            .join("paperpaw-dev-new")
            .join(format!("sdk-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&top);
        let (checkout, alone) = (top.join("checkout"), top.join("alone"));
        std::fs::create_dir_all(checkout.join("crates/paperpaw-sdk")).unwrap();
        std::fs::create_dir_all(&alone).unwrap();
        let made = |sdk: &Path, name: &str| {
            let dir = top.join(name);
            let args: Vec<String> = ["--github", "night-owl", "--code", "--sdk"]
                .iter()
                .map(ToString::to_string)
                .chain([sdk.display().to_string(), "--dir".into()])
                .chain([dir.display().to_string()])
                .collect();
            assert_eq!(new(&args), Ok(true));
            (
                std::fs::read_to_string(dir.join("Cargo.toml")).unwrap(),
                dir,
            )
        };
        let (manifest, dir) = made(&checkout.join("crates/paperpaw-sdk"), "linked");
        assert!(
            manifest.contains("paperpaw-sdk = { path = '.paperpaw/crates/paperpaw-sdk' }"),
            "{manifest}"
        );
        assert!(manifest.contains("[workspace]\nexclude = [\".paperpaw\"]"));
        assert_eq!(std::fs::read_link(dir.join(SDK_LINK)).unwrap(), checkout);
        let ignored = std::fs::read_to_string(dir.join(".gitignore")).unwrap();
        assert!(ignored.lines().any(|line| line == "/.paperpaw"));
        let (manifest, dir) = made(&alone, "by-path");
        assert!(manifest.contains(&format!(
            "paperpaw-sdk = {{ path = '{}' }}",
            alone.display()
        )));
        assert!(!dir.join(SDK_LINK).exists());
        let _ = std::fs::remove_dir_all(&top);
    }

    /// The template of a community plugin source (`templates/plugin-source`,
    /// #318) holds the plugin with code that `new --code` writes, and its
    /// workflow names this release and the compilers the release is built
    /// with: a change here that the template does not follow fails.
    #[test]
    fn the_plugin_source_template_is_the_plugin_new_writes() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let template = root.join("templates/plugin-source");
        for (path, bytes) in code_files("hello-paw", None) {
            let kept = std::fs::read(template.join(&path)).unwrap_or_default();
            assert!(
                kept == bytes,
                "templates/plugin-source/{path} is not what `new --github hello-paw --code` writes"
            );
        }
        let read = |path: &str| std::fs::read_to_string(template.join(path)).unwrap();
        let sdk = PAPERPAW.split('.').take(2).collect::<Vec<_>>().join(".");
        assert!(read("plugin/store/store.ini").contains(&format!("\nsdk = {sdk}\n")));
        let workflow = read(".github/workflows/plugin-source.yml");
        assert!(workflow.contains(&format!("  PAPERPAW: \"{PAPERPAW}\"\n")));
        assert!(read("README.md").contains(&format!("--tag v{PAPERPAW} paperpaw-sign")));
        let versions = std::fs::read_to_string(root.join("shells/xteink/versions.env")).unwrap();
        let version = |name: &str| {
            let line = versions.lines().find_map(|l| l.strip_prefix(name));
            line.and_then(|l| l.strip_prefix('=')).unwrap().to_string()
        };
        for (switch, name) in [
            ("ESP_RUST", "ESP_RUST_VERSION"),
            ("ESP_GCC", "ESP_GCC_VERSION"),
            ("ESPUP", "ESPUP_VERSION"),
            ("ESPUP_SHA256", "ESPUP_SHA256_X86_64"),
        ] {
            let line = format!("  {switch}: \"{}\"\n", version(name));
            assert!(workflow.contains(&line), "the workflow has no {line}");
        }
    }
}
