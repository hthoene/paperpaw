//! The command line: what to check, in which variants, and the commands.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::OnceLock;

use paperpaw_emulator::profiles::{self, Profile};

use crate::AddPlugins;
use crate::report::{Level, Report};
use crate::target::Target;
use crate::variant::{self, Theme, Variant};

const USAGE: &str = "\
paperpaw-dev: checks PaperPaw plugins before they are published

USAGE:
    paperpaw-dev <COMMAND> [PATHS...] [OPTIONS]

COMMANDS:
    check       Everything below, one report (the default)
    gallery     Contact sheets: every screen and widget in every size, device,
                language, light and dark, look
    visual      Text cut or overflowing, parts outside their cell or the screen,
                uneven margins, touch targets too small, focus not shown,
                ink off centre, contrast on black-and-white screens
    lint        Texts: missing translations, pseudo-language overflow, settings
                without help, ids outside the plugin's namespace, dashes
    flows       Scripted flows with assertions (--flow <file>), recorded or written:
                each step's heap and time against the X4's budgets, and the flow
                varied at the edges (repeated steps, Back, Home and sleep in the
                middle, the network failing, slow, empty or huge, the
                pseudo-language, long data, little storage; not with --quick)
    reach       Every screen reachable and closable with buttons alone
    explore     Every target of every screen used once, each from a fresh start
    perf        Time per frame, heap per screen, storage reads while drawing,
                against the X4's budgets
    package     A data plugin as the installer and the website take it, a .ppn
                file's header, fingerprint, signature and sizes, compatibility;
                a .so library (interface 7) is checked to be freestanding
    snapshots   Compares screens with the pictures in --snapshots (--bless writes them)
    check-publish [repo] [--release-dir <dir>] [--tag <tag>] [--rules <file>]
                A plugin's own repository (and its release's files) checked by the
                plugin store's rules at paperpaw.org, saying what to change: run it
                before tagging; the release workflow runs it too
    new --github <id> [--code] [--dir <path>]
                A plugin's own repository for GitHub that check-publish passes: the
                plugin's folder, store/store.ini, the release workflow, README,
                .gitignore; with --code the crate too
    ui-kit      Every building block of the SDK's `ui` module, as the gallery shows
                screens (every device, look, light and dark), checked as `visual`
                checks; a label shortened to fit is an error

PATHS:
    A data plugin folder (with plugin.ini), a .ppn file or a .so library
    (interface 7). A plugin with code
    is added from its own repository (see docs/plugins.md, \"Checking your plugin\").

OPTIONS:
    --plugin <id>         Look at this plugin of the app (repeatable); none named
                          and no paths: every plugin
    --profile <id>|all    Device profiles (repeatable; default: the X4s and the Boox)
    --language <code>|all Languages (repeatable; default: every one the app has,
                          `pseudo` for the pseudo-language)
    --dark | --light      Only dark or only light (default: both)
    --theme <name>|all    Looks (repeatable; default: all; see --list)
    --quick               One variant per profile: English, light, default look
    --set <id>=<value>    A setting for every session (repeatable)
    --flow <file>|<dir>   A flow to run, or every .flow in a folder (repeatable;
                          `flows` and `check`)
    --snapshots <dir>     Reference pictures (default: tests/snapshots if it exists)
    --bless               Write the reference pictures anew
    --release <version>   Check compatibility with this PaperPaw release
    --out <dir>           Output folder (default: target/paperpaw-dev)
    --network <how>       Where plugins' requests go: `real` (the default), or
                          `fixtures`, answered offline from the emulator's own
                          (PaperPaw's plugins' servers; others fail), alike
                          everywhere; then a screen that stays unreachable
                          while requests went unanswered is an error
    --strict              Warnings fail the run too
    --labels-whole        A label shortened to fit is an error, wherever it ends
                          (`visual`; `ui-kit` checks its strict pages so by itself)
    --verbose             Notes too on the console
    --keep                Keep the emulated libraries (in <out>/work)
    --list                List profiles and looks
    -h, --help            Show this help
";

/// The commands, in the order `check` runs them.
pub const COMMANDS: &[&str] = &[
    "lint",
    "package",
    "gallery",
    "visual",
    "reach",
    "explore",
    "perf",
    "flows",
    "snapshots",
];

/// What every command is given.
#[derive(Debug)]
pub struct Context {
    pub target: Target,
    pub out: PathBuf,
    pub profiles: Vec<&'static Profile>,
    /// Asked for; `None`: every language the app has.
    pub languages: Option<Vec<String>>,
    pub nights: Vec<bool>,
    pub themes: Vec<&'static Theme>,
    pub quick: bool,
    /// Settings for every session, as `(id, value)`.
    pub settings: Vec<(String, String)>,
    pub flows: Vec<PathBuf>,
    pub snapshots: Option<PathBuf>,
    pub bless: bool,
    pub release: Option<String>,
    pub strict: bool,
    pub verbose: bool,
    pub keep: bool,
    /// The emulated libraries kept in memory and no pictures written:
    /// where there is no file system (the browser's emulator).
    pub in_memory: bool,
    /// A label shortened to fit (`visual.ellipsis`) is an error, not a
    /// warning, and reported wherever it ends: labels are never cut (the
    /// UI kit's blocks always so checked).
    pub labels_whole: bool,
    /// Plugins' requests answered from the emulator's fixtures, offline
    /// (`--network fixtures`), not sent to the network.
    pub fixtures: bool,
    app_languages: OnceLock<Vec<String>>,
}

impl Context {
    /// A context for `target` with the defaults of the command line, the
    /// output in `out`: for tests and plugin repositories that drive the
    /// checks themselves.
    pub fn new(target: Target, out: PathBuf) -> Self {
        Context {
            target,
            out,
            profiles: default_profiles(),
            languages: None,
            nights: vec![false, true],
            themes: variant::THEMES.iter().collect(),
            quick: false,
            settings: Vec::new(),
            flows: Vec::new(),
            snapshots: None,
            bless: false,
            release: None,
            strict: false,
            verbose: false,
            keep: false,
            in_memory: false,
            labels_whole: false,
            fixtures: false,
            app_languages: OnceLock::new(),
        }
    }

    /// The languages looked at: those asked for, else every one the app
    /// has (its catalogs, as the X4 loads them).
    pub fn languages(&self) -> Vec<String> {
        if let Some(languages) = &self.languages {
            return languages.clone();
        }
        self.app_languages().to_vec()
    }

    /// Every language the app has catalogs for.
    pub fn app_languages(&self) -> &[String] {
        self.app_languages.get_or_init(|| {
            match crate::session::Session::new(self, &Variant::x4()) {
                Ok(session) => session
                    .device
                    .kernel
                    .translator()
                    .languages()
                    .into_iter()
                    .map(|(code, _)| code.to_string())
                    .collect(),
                Err(_) => vec![String::from("en")],
            }
        })
    }

    /// Every variant asked for (or one a profile with `--quick`).
    pub fn variants(&self) -> Vec<Variant> {
        if self.quick {
            return self.profiles.iter().map(|p| Variant::of(p)).collect();
        }
        variant::matrix(
            &self.profiles,
            &self.languages(),
            &self.nights,
            &self.themes,
        )
    }

    /// One variant a profile asked for: English (or the first language
    /// asked for), light, the default look; for checks whose findings do
    /// not depend on language and look.
    pub fn variants_per_profile(&self) -> Vec<Variant> {
        let language = self
            .languages
            .as_ref()
            .and_then(|l| l.first().cloned())
            .unwrap_or_else(|| String::from("en"));
        self.profiles
            .iter()
            .map(|p| Variant {
                language: language.clone(),
                ..Variant::of(p)
            })
            .collect()
    }

    /// A path in the output folder.
    pub fn path(&self, relative: &str) -> PathBuf {
        self.out.join(relative)
    }
}

fn default_profiles() -> Vec<&'static Profile> {
    variant::DEFAULT_PROFILES
        .iter()
        .filter_map(|id| profiles::find(id))
        .collect()
}

/// Runs the command line `args` (without the program's name).
pub fn main(args: impl Iterator<Item = String>, add: Option<AddPlugins>) -> ExitCode {
    match run(args, add) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(2)
        }
    }
}

/// Runs the command line; whether the checks passed.
pub fn run(args: impl Iterator<Item = String>, add: Option<AddPlugins>) -> Result<bool, String> {
    // Publishing from GitHub: arguments of their own.
    let args: Vec<String> = args.collect();
    match args.first().map(String::as_str) {
        Some("check-publish") => return crate::publish::check(&args[1..]),
        Some("new") => return crate::publish::new(&args[1..]),
        _ => {}
    }
    let Some((command, mut context)) = parse(args.into_iter())? else {
        return Ok(true);
    };
    context.target.add = add;
    // The kit's pages, drawn as the gallery draws a plugin's screens.
    // The kit's pages, drawn as the gallery draws a plugin's screens and
    // checked as `visual` checks them, a label shortened an error.
    let kit = command == "ui-kit";
    if kit {
        context.target.plugins = vec![crate::ui_kit::PLUGIN.to_string()];
        context.target.add = Some(crate::ui_kit::add);
        // Its blocks never cut a label.
        context.labels_whole = true;
    }
    let mut report = Report::default();
    let commands: Vec<&str> = if command == "check" {
        COMMANDS.to_vec()
    } else if kit {
        vec!["gallery", "visual"]
    } else {
        vec![command.as_str()]
    };
    let all = command == "check";
    for command in commands {
        // `check` runs only what applies.
        if all && !applies(command, &context) {
            continue;
        }
        eprintln!("paperpaw-dev: {command}");
        report.ran.push(command.to_string());
        run_command(command, &context, &mut report)?;
    }
    report.write(&context.out)?;
    let level = if context.verbose {
        Level::Info
    } else {
        Level::Warning
    };
    print!("{}", report.text(level));
    println!("report: {}", context.out.join("report.html").display());
    Ok(!report.fails(context.strict))
}

/// Whether `command` has something to look at.
fn applies(command: &str, context: &Context) -> bool {
    match command {
        "package" => {
            !context.target.data.is_empty()
                || !context.target.ppn.is_empty()
                || !context.target.libraries.is_empty()
                || !context.target.sources.is_empty()
        }
        "flows" => !context.flows.is_empty(),
        "snapshots" => context.snapshots.is_some(),
        _ => context.target.runs(),
    }
}

/// Runs one command into `report`.
pub fn run_command(command: &str, context: &Context, report: &mut Report) -> Result<(), String> {
    match command {
        "gallery" => crate::gallery::run(context, report),
        "visual" => crate::visual::run(context, report),
        "lint" => crate::lint::run(context, report),
        "flows" => crate::flows::run(context, report),
        "reach" => crate::reach::run(context, report),
        "explore" => crate::explore::run(context, report),
        "perf" => crate::perf::run(context, report),
        "package" => crate::package::run(context, report),
        "snapshots" => crate::snapshot::run(context, report),
        other => Err(format!("unknown command `{other}` (see --help)")),
    }
}

/// The flow at `path`, or every flow in the folder `path` (by name).
fn flows_at(path: PathBuf) -> Result<Vec<PathBuf>, String> {
    if !path.is_dir() {
        return Ok(vec![path]);
    }
    let mut flows: Vec<PathBuf> = std::fs::read_dir(&path)
        .map_err(|e| format!("{}: {e}", path.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|file| file.extension().is_some_and(|ext| ext == "flow"))
        .collect();
    flows.sort();
    if flows.is_empty() {
        return Err(format!("{}: no .flow files in it", path.display()));
    }
    Ok(flows)
}

#[expect(
    clippy::too_many_lines,
    reason = "longer than the limit from before it was set; split it when it is next changed"
)]
fn parse(mut args: impl Iterator<Item = String>) -> Result<Option<(String, Context)>, String> {
    let mut command = None;
    let mut target = Target::default();
    let mut out = None;
    let mut profiles_asked: Vec<&'static Profile> = Vec::new();
    let mut languages: Vec<String> = Vec::new();
    let mut themes: Vec<&'static Theme> = Vec::new();
    let mut nights = vec![false, true];
    let mut context = Context::new(Target::default(), PathBuf::new());
    while let Some(arg) = args.next() {
        let mut value = |name: &str| {
            args.next()
                .ok_or_else(|| format!("{name} requires a value"))
        };
        match arg.as_str() {
            "-h" | "--help" => {
                print!("{USAGE}");
                return Ok(None);
            }
            "--list" => {
                println!("profiles:");
                for profile in profiles::PROFILES {
                    println!("    {:<22} {}", profile.device.id, profile.summary);
                }
                println!("looks:");
                for theme in variant::THEMES {
                    let settings: Vec<String> = theme
                        .settings
                        .iter()
                        .map(|(id, value)| format!("{id} = {value}"))
                        .collect();
                    println!("    {:<22} {}", theme.name, settings.join(", "));
                }
                return Ok(None);
            }
            "--plugin" => target.plugins.push(value("--plugin")?),
            "--profile" => {
                let id = value("--profile")?;
                if id == "all" {
                    profiles_asked.extend(profiles::PROFILES.iter());
                } else {
                    profiles_asked.push(
                        profiles::find(&id)
                            .ok_or_else(|| format!("unknown profile `{id}` (see --list)"))?,
                    );
                }
            }
            "--language" => {
                let code = value("--language")?;
                if code != "all" {
                    languages.push(code);
                }
            }
            "--dark" => nights = vec![true],
            "--light" => nights = vec![false],
            "--theme" => {
                let name = value("--theme")?;
                if name == "all" {
                    themes.extend(variant::THEMES.iter());
                } else {
                    themes.push(
                        variant::theme(&name)
                            .ok_or_else(|| format!("unknown look `{name}` (see --list)"))?,
                    );
                }
            }
            "--quick" => context.quick = true,
            "--set" => {
                let set = value("--set")?;
                let (id, v) = set
                    .split_once('=')
                    .ok_or_else(|| format!("--set expects <id>=<value>, got `{set}`"))?;
                context.settings.push((id.to_string(), v.to_string()));
            }
            "--flow" => context
                .flows
                .extend(flows_at(PathBuf::from(value("--flow")?))?),
            "--snapshots" => context.snapshots = Some(PathBuf::from(value("--snapshots")?)),
            "--bless" => context.bless = true,
            "--release" => context.release = Some(value("--release")?),
            "--out" => out = Some(PathBuf::from(value("--out")?)),
            "--strict" => context.strict = true,
            "--network" => {
                context.fixtures = match value("--network")?.as_str() {
                    "real" => false,
                    "fixtures" => true,
                    other => {
                        return Err(format!("--network expects real or fixtures, got `{other}`"));
                    }
                };
            }
            "--labels-whole" => context.labels_whole = true,
            "--verbose" => context.verbose = true,
            "--keep" => context.keep = true,
            flag if flag.starts_with('-') => {
                return Err(format!("unknown option `{flag}` (see --help)"));
            }
            word if command.is_none() && is_command(word) => command = Some(word.to_string()),
            path => target.take_path(std::path::Path::new(path))?,
        }
    }
    context.target = target;
    context.out = out.unwrap_or_else(|| PathBuf::from("target/paperpaw-dev"));
    if !profiles_asked.is_empty() {
        profiles_asked.dedup_by_key(|p| p.device.id);
        context.profiles = profiles_asked;
    }
    if !languages.is_empty() {
        context.languages = Some(languages);
    }
    if !themes.is_empty() {
        context.themes = themes;
    }
    context.nights = nights;
    if context.snapshots.is_none() && std::path::Path::new("tests/snapshots").is_dir() {
        context.snapshots = Some(PathBuf::from("tests/snapshots"));
    }
    Ok(Some((command.unwrap_or_else(|| "check".into()), context)))
}

fn is_command(word: &str) -> bool {
    word == "check" || word == "ui-kit" || COMMANDS.contains(&word)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(args: &[&str]) -> (String, Context) {
        parse(args.iter().map(|s| s.to_string())).unwrap().unwrap()
    }

    #[test]
    fn a_command_with_options() {
        let (command, context) = parsed(&[
            "visual",
            "--plugin",
            "widgets",
            "--profile",
            "xteink-x4",
            "--language",
            "de",
            "--dark",
            "--theme",
            "square",
        ]);
        assert_eq!(command, "visual");
        assert_eq!(context.target.plugins, ["widgets"]);
        let variants = context.variants();
        assert_eq!(variants.len(), 1);
        assert_eq!(variants[0].name(), "xteink-x4 de dark square");
    }

    #[test]
    fn check_is_the_default_and_paths_are_targets() {
        let folder = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../samples/plugins/airy-theme"
        );
        let (command, context) = parsed(&[folder, "--quick"]);
        assert_eq!(command, "check");
        assert_eq!(context.target.data.len(), 1);
        assert_eq!(context.variants().len(), variant::DEFAULT_PROFILES.len());
    }

    #[test]
    fn a_folder_of_flows_gives_each_flow() {
        let folder = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/flows");
        let (_, context) = parsed(&["flows", "--flow", folder]);
        assert!(!context.flows.is_empty());
        assert!(
            context
                .flows
                .iter()
                .all(|f| f.extension().is_some_and(|e| e == "flow"))
        );
        assert!(context.flows.is_sorted());
    }

    #[test]
    fn unknown_options_are_refused() {
        assert!(parse(["--nonsense".to_string()].into_iter()).is_err());
        assert!(parse(["--profile".to_string(), "x".to_string()].into_iter()).is_err());
    }
}
