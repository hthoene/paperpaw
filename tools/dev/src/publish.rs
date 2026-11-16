//! Publishing from GitHub, made hard to get wrong:
//!
//! - `check-publish [repo] [--release-dir <dir>] [--tag <tag>] [--rules
//!   <file>]`: the repository (and its release files) checked by the
//!   plugin store's rules ([`paperpaw_publish`]), with what to change;
//!   the release workflow runs the same check before it releases anything;
//! - `new --github <id> [--dir <path>]`: a repository that passes it from
//!   the start: the plugin's folder with `plugin.ini` and
//!   `store/store.ini`, the release workflow with its switches set, a
//!   README that says what is left to do, `.gitignore`. For a plugin
//!   without code: there is no template for one with code yet (`--code`
//!   says so).

use std::path::{Path, PathBuf};

/// The release workflow template, with `PLUGIN_DIR` and `PAPERPAW` to set.
const WORKFLOW: &str = include_str!("../templates/.github/workflows/paperpaw-release.yml");

/// The PaperPaw release this tool is of, which a new plugin is made for.
const PAPERPAW: &str = env!("CARGO_PKG_VERSION");

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
            "/target/\n/dist/\n/release/\n# The author's signing key never goes into the repository: keep it\n\
             # outside (e.g. ~/.paperpaw/author.key) and in the CI's secrets.\n*.key\n"
                .to_string(),
        ),
    ]
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

/// `new --github <id> [--dir <path>]`: writes a new repository (into
/// `<id>/` unless named), never over files that are there. `--code` is
/// refused: there is no template for a plugin with code yet.
pub fn new(args: &[String]) -> Result<bool, String> {
    let (mut id, mut dir, mut github) = (None, None, false);
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--github" => github = true,
            "--code" => {
                return Err(
                    "this tool does not yet support plugins with code for the new plugin \
                     interface (7); plugins without code (data plugins) are supported: \
                     `new --github <id>`"
                        .into(),
                );
            }
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
    let dir = dir.unwrap_or_else(|| PathBuf::from(&id));
    write(&dir, &files(&id))?;
    println!(
        "made {}: see its README for what is left to do",
        dir.display()
    );
    Ok(true)
}

/// Writes `files` below `dir`, refusing to overwrite one.
fn write(dir: &Path, files: &[(String, String)]) -> Result<(), String> {
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
    fn a_plugin_with_code_has_no_template_and_new_says_so() {
        let args: Vec<String> = ["--github", "night-owl", "--code"]
            .iter()
            .map(ToString::to_string)
            .collect();
        let said = new(&args).unwrap_err();
        assert!(said.contains("does not yet support plugins with code"));
        assert!(said.contains("data plugins) are supported"));
        assert!(!Path::new("night-owl").exists());
    }
}
