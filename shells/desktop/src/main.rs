//! PaperPaw desktop emulator.
//!
//! Runs the real kernel and plugins against an emulated e-ink device,
//! interactively in a window, or headless for screenshots and CI.

mod gui;
mod record;

use std::path::PathBuf;
use std::process::ExitCode;

use paperpaw_emulator::device::Device;
use paperpaw_emulator::{budget, cable, device, headless, profiles, trace};

const USAGE: &str = "\
PaperPaw emulator

USAGE:
    paperpaw-emu [OPTIONS]

OPTIONS:
    --profile <id>        Device to emulate (default: xteink-x4)
    --list-profiles       List available device profiles
    --check <dir> [--profile <id>]...
                          Check a plugin with paperpaw-dev (beside this program),
                          its findings as JSON on standard output
    --trust-code          Load code plugins built for this computer from the
                          library (plugins/<id>/code-host.dll, .so or .dylib),
                          those there installed first; without it, none
    --stdio               Be driven by another program over standard input and
                          output, a JSON object a line (see the `stdio` module)
    --registry <file>     Write what the app offers data plugins (settings,
                          widgets, actions, device profiles) as JSON
    --config <file>       User configuration file (paperpaw.ini)
    --set <key>=<value>   Override configuration, e.g. --set core.language=de
                          or --set slots.core.home=core.placeholder
    --headless            Run without a window
    --script <steps>      Input steps, e.g. \"page-forward,long:confirm,tap:100:200\"
    --screenshot <file>   Save a PNG of the panel after the script (headless)
    --library <dir>       Folder emulating the device storage
                          (default: samples/library)
    --card <image>        A card image (FAT or exFAT, as `dd` copies a card)
                          as the device storage, mounted as the X4 mounts
                          its card; written in place
    --screenshot-dir <d>  Where F12 saves screenshots (default: out)
    --record <file>       Write what is pressed, tapped and swiped in the window
                          as a flow's steps, for `paperpaw-dev flows`
    --heap-budget <KiB>   Override the profile's heap budget (`none` = unlimited)
    --trace-alloc <bytes> Print every kernel allocation of at least this size, with where
    --heap-report         After a headless run, list the live heap by call site
    --network <how>       Where plugins' requests go: `real` (the default), or
                          `fixtures`, answered offline from the emulator's own
                          (a small plugin library and the servers PaperPaw's
                          plugins ask; others fail), alike everywhere
    --wifi <state>        `online`: start online, on devices where PaperPaw
                          switches Wi-Fi (the X4s): on USB, the radio on and in
                          the network `Workshop Wi-Fi`, known as joined before,
                          so that plugins' requests go out at once; Wi-Fi
                          switches off and on in the settings as on a device.
                          `off` (the default): the radio starts off, as on a
                          device. Before --stdio, for the devices it starts
    --free-space <bytes>  Say the storage has this much free, not the computer's
                          disk (screenshots alike everywhere)
    --trace               Print what each frame's drawing asks the storage, band by band,
                          and how long it takes
    --cable               Speak the cable transfer (USB) on standard input and output,
                          as a device does over its cable (docs/usb-transfer.md)
    -h, --help            Show this help

KEYS (if the device has the button):
    PageDown/Space  page-forward     PageUp      page-back
    Arrow keys      up/down/left/right
    Enter           confirm          Esc/Backspace back
    M               menu             H           home        P  power
    Hold a key for a long press. Mouse = touch screen.
    F2  show refreshed regions   F3  toggle flash animation   F12  screenshot
";

#[derive(Debug, Default)]
struct Options {
    profile: Option<String>,
    config: Option<PathBuf>,
    sets: Vec<(String, String)>,
    headless: bool,
    script: Option<String>,
    screenshot: Option<PathBuf>,
    screenshot_dir: Option<PathBuf>,
    record: Option<PathBuf>,
    library: Option<PathBuf>,
    /// A card image in place of the library folder (`--card`).
    card: Option<PathBuf>,
    /// `Some(None)` removes the budget.
    heap_budget: Option<Option<usize>>,
    heap_report: bool,
    /// The cable transfer on standard input and output.
    cable: bool,
    /// Install the code plugins built for this computer on the card.
    trust_code: bool,
    /// Requests answered from the emulator's fixtures (`--network`).
    fixtures: bool,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let Some(options) = parse_args(std::env::args().skip(1))? else {
        return Ok(());
    };
    let profile_id = options.profile.as_deref().unwrap_or("xteink-x4");
    let profile = profiles::find(profile_id)
        .ok_or_else(|| format!("unknown profile `{profile_id}` (see --list-profiles)"))?;

    let library = options
        .library
        .clone()
        .unwrap_or_else(|| PathBuf::from("samples/library"));
    if options.trust_code {
        device::set_code_plugins(true);
        let installed = paperpaw_emulator::native::trust(&library)
            .map_err(|e| format!("library {}: {e}", library.display()))?;
        eprintln!("code plugins installed: {}", installed.join(", "));
    }
    let storage =
        device::library(&library).map_err(|e| format!("library {}: {e}", library.display()))?;
    if options.record.is_some() && options.headless {
        return Err("--record records the window: not with --headless".into());
    }
    if options.heap_report {
        if !options.headless {
            return Err("--heap-report requires --headless".into());
        }
        budget::profile();
    }
    if let Some(limit) = options.heap_budget {
        budget::override_limit(limit);
    }
    let mut device = match &options.card {
        Some(image) => {
            let blocks = paperpaw_emulator::card_image::open(image)
                .map_err(|e| format!("card {}: {e}", image.display()))?;
            Device::on_card_image(
                profile,
                user_config(&options)?,
                blocks,
                options.headless,
                None,
            )?
        }
        None => Device::new(profile, user_config(&options)?, storage, options.headless)
            .map_err(|e| format!("{}: {e}", profile.device.id))?,
    };
    if options.fixtures {
        device.use_fixtures();
    }

    if options.headless {
        device.start();
        // What is due at once is done first, as on a device (e.g. a book
        // opened after the screen says it is being prepared).
        device.tick_if_due();
        if let Some(script) = &options.script {
            headless::run_script(&mut device, script)?;
        }
        if options.cable {
            // Line by line until standard input ends, time passing as it
            // does.
            let lines = cable::lines();
            loop {
                match lines.recv_timeout(std::time::Duration::from_millis(50)) {
                    Ok(line) => device.cable_line(&line),
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                }
                device.tick_if_due();
                cable::send(&device.take_cable_lines());
            }
            // Standard output is the cable's: nothing more on it.
            return Ok(());
        }
        if let Some(path) = &options.screenshot {
            headless::screenshot(&device, path)?;
            println!("screenshot saved to {}", path.display());
        }
        println!(
            "{}: screen {}, {}",
            profile.device.id,
            device
                .kernel
                .active_screen()
                .map(ToString::to_string)
                .unwrap_or_default(),
            budget::stats()
        );
        if options.heap_report {
            budget::report(2);
        }
        return Ok(());
    }

    if options.script.is_some() || options.screenshot.is_some() {
        return Err("--script and --screenshot require --headless".into());
    }
    let recorder = match &options.record {
        Some(path) => Some(record::Recorder::create(path, profile.device.id)?),
        None => None,
    };
    let dir = options
        .screenshot_dir
        .unwrap_or_else(|| PathBuf::from("out"));
    gui::run(device, dir, recorder, options.cable)
}

/// `--wifi`'s value: whether the device starts online.
fn wifi_online(value: &str) -> Result<bool, String> {
    match value {
        "online" => Ok(true),
        "off" => Ok(false),
        other => Err(format!("--wifi expects online or off, got `{other}`")),
    }
}

/// Reads `--config` and appends `--set` overrides as config sections.
fn user_config(options: &Options) -> Result<Option<String>, String> {
    let mut config = match &options.config {
        Some(path) => std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?,
        None if options.sets.is_empty() => return Ok(None),
        None => String::new(),
    };
    for (key, value) in &options.sets {
        let (section, name) = key
            .split_once('.')
            .ok_or_else(|| format!("--set expects <section>.<key>=<value>, got `{key}`"))?;
        config.push_str(&format!("\n[{section}]\n{name} = {value}\n"));
    }
    Ok(Some(config))
}

/// `--check <plugin folder> [--profile <id>]...`: paperpaw-dev's checks
/// of a plugin (`paperpaw-dev check`, which the release puts beside this
/// program), their findings as JSON on standard output (its
/// `report.json`), what it tells on the way on standard error; the exit
/// code its (0: passed, 1: findings, 2: it could not check).
fn check(args: &[String]) -> Result<i32, String> {
    let exe = std::env::current_exe().map_err(|e| format!("--check: {e}"))?;
    let dev = beside(&exe, "paperpaw-dev");
    if !dev.exists() {
        return Err(format!(
            "--check needs {} beside this program (both are in the release's zip)",
            dev.display()
        ));
    }
    let out = paperpaw_emulator::private::dir("paperpaw-check")
        .map_err(|e| format!("--check: a folder for its report: {e}"))?;
    let ran = std::process::Command::new(&dev)
        .arg("check")
        .args(args)
        .arg("--out")
        .arg(&out)
        .output()
        .map_err(|e| format!("{}: {e}", dev.display()))?;
    eprint!("{}", String::from_utf8_lossy(&ran.stdout));
    eprint!("{}", String::from_utf8_lossy(&ran.stderr));
    let report = std::fs::read_to_string(out.join("report.json"));
    let _ = std::fs::remove_dir_all(&out);
    if let Ok(report) = report {
        print!("{report}");
    }
    Ok(ran.status.code().unwrap_or(2))
}

/// The program `name` in the folder of `exe` (with `.exe` on Windows).
fn beside(exe: &std::path::Path, name: &str) -> PathBuf {
    exe.with_file_name(format!("{name}{}", std::env::consts::EXE_SUFFIX))
}

fn parse_args(mut args: impl Iterator<Item = String>) -> Result<Option<Options>, String> {
    let mut options = Options::default();
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
            "--check" => {
                let rest: Vec<String> = args.by_ref().collect();
                std::process::exit(check(&rest)?);
            }
            "--stdio" => {
                let stdin = std::io::BufReader::new(std::io::stdin());
                paperpaw_emulator::stdio::run(stdin, std::io::stdout().lock())
                    .map_err(|e| format!("--stdio: {e}"))?;
                return Ok(None);
            }
            "--registry" => {
                let out = PathBuf::from(value("--registry")?);
                let json = paperpaw_emulator::registry::registry()?;
                std::fs::write(&out, json).map_err(|e| format!("{}: {e}", out.display()))?;
                return Ok(None);
            }
            "--list-profiles" => {
                for profile in profiles::PROFILES {
                    println!("{:<22} {}", profile.device.id, profile.summary);
                }
                return Ok(None);
            }
            "--profile" => options.profile = Some(value("--profile")?),
            "--config" => options.config = Some(PathBuf::from(value("--config")?)),
            "--set" => {
                let set = value("--set")?;
                let (key, val) = set
                    .split_once('=')
                    .ok_or_else(|| format!("--set expects <key>=<value>, got `{set}`"))?;
                options.sets.push((key.to_string(), val.to_string()));
            }
            "--headless" => options.headless = true,
            "--cable" => options.cable = true,
            "--script" => options.script = Some(value("--script")?),
            "--screenshot" => options.screenshot = Some(PathBuf::from(value("--screenshot")?)),
            "--library" => options.library = Some(PathBuf::from(value("--library")?)),
            "--card" => options.card = Some(PathBuf::from(value("--card")?)),
            "--record" => options.record = Some(PathBuf::from(value("--record")?)),
            "--screenshot-dir" => {
                options.screenshot_dir = Some(PathBuf::from(value("--screenshot-dir")?));
            }
            "--heap-report" => options.heap_report = true,
            "--trust-code" => options.trust_code = true,
            "--network" => {
                options.fixtures = match value("--network")?.as_str() {
                    "real" => false,
                    "fixtures" => true,
                    other => {
                        return Err(format!("--network expects real or fixtures, got `{other}`"));
                    }
                };
            }
            // A global: also for the devices --stdio starts, which runs
            // as it is read.
            "--wifi" => device::set_wifi_online(wifi_online(&value("--wifi")?)?),
            "--free-space" => {
                let bytes = value("--free-space")?;
                device::set_free_space(
                    bytes
                        .parse()
                        .map_err(|_| format!("--free-space expects bytes, got `{bytes}`"))?,
                );
            }
            "--trace" => trace::start(),
            "--trace-alloc" => {
                let bytes = value("--trace-alloc")?;
                budget::trace(
                    bytes
                        .parse()
                        .map_err(|_| format!("--trace-alloc expects bytes, got `{bytes}`"))?,
                );
            }
            "--heap-budget" => {
                let budget = value("--heap-budget")?;
                options.heap_budget = Some(if budget == "none" {
                    None
                } else {
                    let kib: usize = budget.parse().map_err(|_| {
                        format!("--heap-budget expects KiB or `none`, got `{budget}`")
                    })?;
                    Some(kib * 1024)
                });
            }
            other => return Err(format!("unknown argument `{other}` (see --help)")),
        }
    }
    Ok(Some(options))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// paperpaw-dev for `--check` is looked for in this program's folder.
    #[test]
    fn a_program_beside_is_in_the_same_folder() {
        let suffix = std::env::consts::EXE_SUFFIX;
        let exe = std::path::Path::new("dist").join(format!("paperpaw-emu{suffix}"));
        assert_eq!(
            beside(&exe, "paperpaw-dev"),
            std::path::Path::new("dist").join(format!("paperpaw-dev{suffix}"))
        );
    }
}
