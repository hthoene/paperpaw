//! A data plugin installed on an emulated device, as the store installs
//! it: its folder copied into `plugins/.<id>.part`, then
//! `Ask::InstallPlugin`, so the kernel's own loader decides what it
//! takes (`crates/paperpaw-core/src/data_plugin.rs` and the plugin
//! registration). What the kernel warns about is reported; then what the
//! plugin set is compared with the app it joined.
//!
//! The command comes from a small plugin of the tool's
//! (`Installing`), which hears an event of its own and asks for the
//! installation, as the store plugin would.

use std::cell::Cell;

use paperpaw_core::Ask;
use paperpaw_core::{Items, Kernel, KernelBuilder, WarningSource};
use paperpaw_sdk::event::Event;
use paperpaw_sdk::{Id, Manifest, Params, RegisterError, SDK_VERSION, Version};
use paperpaw_sdk::{PluginState, SettingKind};

use super::Release;
use super::folder::{Folder, Line};
use crate::AddPlugins;
use crate::cli::Context;
use crate::report::{Finding, Report};
use crate::session::{self, Session};
use crate::target::{DataPlugin, Target};
use crate::variant::Variant;

/// The tool's installing plugin (its namespace starts with the probe's,
/// so the checks leave it out).
const PLUGIN: &str = "paperpaw-dev-install";
/// Asks it to install: `plugin` (the id) from `folder`.
const INSTALL: Event<Params> = Event::new("paperpaw-dev-install.install");

thread_local! {
    /// What the target adds besides the installing plugin: the thread's,
    /// as the probe's (see [`crate::probe`]).
    static TARGET: Cell<Option<AddPlugins>> = const { Cell::new(None) };
}

/// The installing plugin and the target's plugins.
fn add(builder: KernelBuilder) -> KernelBuilder {
    let builder = builder.items(installing_manifest(), installing());
    match TARGET.get() {
        Some(add) => add(builder),
        None => builder,
    }
}

fn installing() -> impl Fn(&mut Items) -> Result<(), RegisterError> {
    |r| {
        r.on(INSTALL, |k, p| {
            k.with_event(&INSTALL, p, |_, params, out| {
                if let (Some(plugin), Some(folder)) = (params.get("plugin"), params.get("folder"))
                    && let Ok(id) = Id::parse(plugin)
                {
                    out.push(Ask::InstallPlugin(id, folder.to_string()));
                }
            });
        })
    }
}

/// What [`installing`] registers under.
fn installing_manifest() -> Manifest {
    Manifest::new(
        Id::new_static(PLUGIN),
        "paperpaw-dev install",
        Version::new(0, 1, 0),
    )
    .description("Installs data plugins as the store does, for paperpaw-dev.")
    .uses(paperpaw_sdk::Uses::NOTHING)
}

/// A context for sessions without the data plugins looked at (they are
/// installed while running instead), with the installing plugin.
fn context_for(context: &Context) -> Context {
    TARGET.set(context.target.add);
    let mut own = Context::new(
        Target {
            add: Some(add),
            ..Target::default()
        },
        context.out.clone(),
    );
    own.keep = context.keep;
    own.in_memory = context.in_memory;
    own.settings.clone_from(&context.settings);
    own
}

/// The variants a plugin is installed in: one a profile.
fn variants(context: &Context) -> Vec<Variant> {
    context.variants_per_profile()
}

/// Installs `plugin` on a device of every profile and reports what the
/// kernel says and what does not fit the app.
pub fn check(
    context: &Context,
    plugin: &DataPlugin,
    folder: Option<&Folder>,
    report: &mut Report,
) -> Result<(), String> {
    let Some(folder) = folder else {
        return Ok(());
    };
    let id = folder.id(plugin).to_string();
    if Id::parse(&id).is_err() || !paperpaw_core::kit::install::is_plugin_name(&id) {
        report.add(
            Finding::info(
                "package.device",
                "plugin.ini",
                "Not installed on a device: its id is unusable (see above).",
            )
            .plugin(plugin.id.clone()),
        );
        return Ok(());
    }
    let own = context_for(context);
    let start = session::Start {
        settings: context.settings.clone(),
        ..session::Start::default()
    };
    for variant in variants(context) {
        let mut session = Session::start(&own, &variant, &start)?;
        let findings = install(&mut session, context, plugin, folder, &id)?;
        for finding in findings {
            report.add(finding.plugin(plugin.id.clone()).variant(variant.name()));
        }
    }
    Ok(())
}

/// Installs the plugin on the session's device; what was found.
fn install(
    session: &mut Session,
    context: &Context,
    plugin: &DataPlugin,
    folder: &Folder,
    id: &str,
) -> Result<Vec<Finding>, String> {
    let mut out = Vec::new();
    let staging = format!("plugins/.{id}.part");
    for (path, bytes) in &plugin.files {
        let file = format!("{staging}/{path}");
        session
            .storage()
            .write(&file, bytes)
            .map_err(|e| format!("{file}: {e:?}"))?;
    }
    let event = INSTALL.id().clone();
    let params = Params::new()
        .with("plugin", id.to_string())
        .with("folder", staging.clone());
    // The device reports the kernel's warnings after every call (on the
    // console), so they are taken within it.
    let mut warnings = Vec::new();
    session.device.call(|kernel, now| {
        drop(kernel.take_warnings());
        let effects = kernel.send_event(now, event, params);
        warnings = kernel.take_warnings();
        effects
    });
    session.settle();
    let kernel = &mut session.device.kernel;
    let installed = format!("plugins/{id}/");
    for warning in warnings {
        let about_it = match &warning.source {
            WarningSource::Plugin(source) => source.as_str() == id,
            WarningSource::Runtime => {
                warning.message.contains(&staging) || warning.message.contains(&installed)
            }
            _ => false,
        };
        // Defaults for settings are checked below, with their lines.
        if !about_it || warning.message.starts_with("default for `") {
            continue;
        }
        let message = warning
            .message
            .replace(&format!("{staging}/"), "")
            .replace(&installed, "");
        let (place, message) = match message.split_once(": ") {
            Some((file, rest)) if folder.file(file).is_some() => {
                (file.to_string(), rest.to_string())
            }
            _ => (String::from("plugin.ini"), message.clone()),
        };
        let place = match warning.line {
            Some(line) => format!("{place}:{line}"),
            None => place,
        };
        let finding = Finding::error(
            "package.device",
            place,
            format!("The device says: {message}."),
        );
        // A font is loaded more than once (for reading, for the status).
        if !out.contains(&finding) {
            out.push(finding);
        }
    }
    let state = kernel
        .plugins()
        .iter()
        .find(|p| p.manifest.id.as_str() == id)
        .map(|p| p.state);
    match state {
        Some(PluginState::Running) => {}
        Some(PluginState::Off) => out.push(Finding::warning(
            "package.device",
            "plugin.ini",
            "Installed, but switched off.",
        )),
        _ => {
            out.push(Finding::error(
                "package.device",
                "plugin.ini",
                "The device did not load the plugin (see the other findings).",
            ));
            return Ok(out);
        }
    }
    settings(kernel, &folder.settings, &mut out);
    presets(kernel, id, folder, &mut out);
    keymap(kernel, &folder.keymap, &mut out);
    extends(kernel, context, &folder.manifest.extends, &mut out);
    Ok(out)
}

/// `settings.ini`: every key names a setting of the app, every value one
/// it takes.
fn settings(kernel: &Kernel, lines: &[Line], out: &mut Vec<Finding>) {
    for line in lines {
        let place = format!("settings.ini:{}", line.line);
        if line.section.is_empty() {
            out.push(Finding::error(
                "package.settings",
                place,
                format!("`{}` is outside a section; settings go under the first part of their id, e.g. [reader] font-size = 14.", line.key),
            ));
            continue;
        }
        let id = format!("{}.{}", line.section, line.key);
        // A widget's place of its own (`widgets.clock@2.face`): the kernel
        // takes no plugin's default for it.
        if let Some(plain) = Id::parse(&id).ok().and_then(|id| id.unplaced()) {
            out.push(Finding::error(
                "package.settings",
                place,
                format!(
                    "`{id}` is the setting of one place of a widget (`@` and a number): \
                     PaperPaw takes no plugin's default for it, so it does nothing. Give the \
                     default to `{plain}`, which holds for every place without a value of its own."
                ),
            ));
            continue;
        }
        setting(
            kernel,
            &id,
            &line.value,
            &place,
            "package.settings",
            Level::Error,
            out,
        );
    }
}

#[derive(Clone, Copy)]
enum Level {
    Error,
    Warning,
}

/// One setting and its value, as `settings.ini` and presets give them.
fn setting(
    kernel: &Kernel,
    id: &str,
    value: &str,
    place: &str,
    check: &'static str,
    unknown: Level,
    out: &mut Vec<Finding>,
) {
    let Some(spec) = kernel.settings().spec(id) else {
        let message = format!(
            "`{id}` is no setting of the app; check its id in Settings (the help names it) or in the plugin's documentation."
        );
        out.push(match unknown {
            Level::Error => Finding::error(check, place, message),
            Level::Warning => Finding::warning(check, place, message),
        });
        return;
    };
    // As the kernel takes a default (`Settings::set_default_from_text`).
    if spec.parse(value.trim()).is_none() {
        let value = value.trim();
        let takes = match &spec.kind {
            SettingKind::Int { min, max } => {
                format!("it takes a whole number from {min} to {max}, not made to fit")
            }
            SettingKind::Choice(options) => {
                let options: Vec<&str> = options.iter().collect();
                format!(
                    "it takes one of its options, each by its whole id: {}",
                    options.join(", ")
                )
            }
            SettingKind::Bool => String::from("it takes true or false"),
            _ => String::from("it takes another kind of value"),
        };
        out.push(Finding::error(
            check,
            place,
            format!("`{value}` is no value of `{id}` ({takes}), so PaperPaw passes over it."),
        ));
    }
}

/// `presets.ini`: a kind some screen offers, a name and a sentence that
/// are shown, settings that exist with values they take.
fn presets(kernel: &Kernel, id: &str, folder: &Folder, out: &mut Vec<Finding>) {
    // The kinds other plugins' presets have: those some screen offers.
    let kinds: Vec<String> = kernel
        .settings()
        .presets()
        .iter()
        .filter(|p| p.id.namespace() != id)
        .map(|p| p.kind.as_str().to_string())
        .collect();
    let mut sections: Vec<(&str, u32)> = Vec::new();
    for line in &folder.presets {
        if !line.section.is_empty() && !sections.iter().any(|(s, _)| *s == line.section) {
            sections.push((&line.section, line.line));
        }
    }
    for (section, first) in sections {
        let lines: Vec<&Line> = folder
            .presets
            .iter()
            .filter(|l| l.section == section)
            .collect();
        let value = |key: &str| lines.iter().find(|l| l.key == key).map(|l| l.value.trim());
        let place = format!("presets.ini:{first}");
        match value("kind") {
            None => out.push(Finding::warning(
                "package.presets",
                place.clone(),
                format!("[{section}] has no `kind`, so no screen offers it; home screen layouts are `kind = home.layout`."),
            )),
            Some(kind) if !kinds.iter().any(|k| k == kind) => out.push(Finding::warning(
                "package.presets",
                place.clone(),
                format!("No plugin of the app offers presets of the kind `{kind}`; home screen layouts are `home.layout`."),
            )),
            Some(_) => {}
        }
        for key in ["name", "description"] {
            match value(key) {
                None | Some("") => out.push(Finding::warning(
                    "package.presets",
                    place.clone(),
                    format!("[{section}] has no `{key}`, so it is shown without one."),
                )),
                Some(text)
                    if looks_like_key(text, id) && kernel.translator().get(text).is_none() =>
                {
                    out.push(Finding::warning(
                        "package.presets",
                        place.clone(),
                        format!("`{text}` looks like a translation key, but no catalog has it, so it is shown as it is; add it to i18n/en.ini."),
                    ));
                }
                Some(_) => {}
            }
        }
        for line in &lines {
            let at = format!("presets.ini:{}", line.line);
            match line.key.as_str() {
                "kind" | "name" | "description" => {}
                key if key.contains('.') => setting(
                    kernel,
                    key,
                    &line.value,
                    &at,
                    "package.presets",
                    Level::Warning,
                    out,
                ),
                key => out.push(Finding::warning(
                    "package.presets",
                    at,
                    format!("`{key}` is not read: a preset has kind, name and description, then settings by their whole id (`home.rows = 6`)."),
                )),
            }
        }
    }
}

/// Whether `text` is meant as a translation key of plugin `id`.
fn looks_like_key(text: &str, id: &str) -> bool {
    !text.contains(' ')
        && text
            .strip_prefix(id)
            .is_some_and(|rest| rest.starts_with('.'))
}

/// `keymap.ini`: actions that exist.
fn keymap(kernel: &Kernel, lines: &[Line], out: &mut Vec<Finding>) {
    for line in lines {
        let action = line.value.trim();
        if action == "none" || line.key == "gestures.policy" {
            continue;
        }
        if kernel.actions().get(action).is_none() {
            out.push(Finding::warning(
                "package.keymap",
                format!("keymap.ini:{}", line.line),
                format!("`{action}` is no action of the app, so the key does nothing."),
            ));
        }
    }
}

/// `[extends]`: fonts that exist.
///
/// A font of a plugin that is no part of the app (`literata-styles.bold`
/// of the store's data plugin) is there once that plugin is installed: it
/// is found among the plugins checked with this one, else told as a note
/// (a typo in the name of a font of the app's own plugins is an error).
fn extends(kernel: &Kernel, context: &Context, lines: &[Line], out: &mut Vec<Finding>) {
    for line in lines {
        let font = line.value.trim();
        let Ok(id) = Id::parse(font) else { continue };
        if kernel.fonts().contains(font) || family_face_of_sibling(context, &id) {
            continue;
        }
        let place = format!("plugin.ini:{}", line.line);
        let namespace = id.namespace();
        let of_the_app = kernel
            .plugins()
            .iter()
            .any(|p| p.manifest.id.as_str() == namespace);
        out.push(if of_the_app {
            Finding::error(
                "package.fonts",
                place,
                format!("`{font}` is no font of the app, so the pack extends nothing; e.g. fonts-default.literata."),
            )
        } else {
            Finding::warning(
                "package.fonts",
                place,
                format!("`{font}` is a font of the plugin `{namespace}`, which is not part of the app: the pack extends nothing until that plugin is installed too."),
            )
        });
    }
}

/// Whether `font` (`<plugin>.<face>`) is a face of the `[family]` of a data
/// plugin checked together with this one.
fn family_face_of_sibling(context: &Context, font: &Id) -> bool {
    let face = font
        .as_str()
        .strip_prefix(font.namespace())
        .and_then(|r| r.strip_prefix('.'));
    let Some(face) = face else { return false };
    context
        .target
        .data
        .iter()
        .filter(|p| p.id == font.namespace())
        .filter_map(|p| p.files.get("plugin.ini"))
        .filter_map(|ini| core::str::from_utf8(ini).ok())
        .any(|ini| {
            paperpaw_config::parse(ini).filter_map(Result::ok).any(|e| {
                e.section == "family" && e.key == face && !e.value.as_str().trim().is_empty()
            })
        })
}

/// Plugins with code named with `--plugin`: the SDK each was built for,
/// against this PaperPaw and the release.
/// Code plugins of the app with their folders (`plugin.ini`'s version):
/// the code's manifest must name the same version (the device goes by
/// `plugin.ini`'s).
pub fn code_versions(
    context: &Context,
    folders: &[(String, Version)],
    report: &mut Report,
) -> Result<(), String> {
    let mut own = Context::new(
        Target {
            add: context.target.add,
            ..Target::default()
        },
        context.out.clone(),
    );
    own.in_memory = context.in_memory;
    let session = Session::new(&own, &Variant::x4())?;
    for (id, declared) in folders {
        let plugins = session.device.kernel.plugins();
        let Some(info) = plugins.iter().find(|p| p.manifest.id.as_str() == id) else {
            continue;
        };
        // Whole versions, a beta's too (0.5.0-beta.1): the store offers
        // a newer one as an update by them.
        let built = info.manifest.version;
        if built != *declared {
            report.add(
                Finding::error(
                    "package.version",
                    "plugin.ini",
                    format!(
                        "plugin.ini says version {declared}, its code's manifest {built}: the device and the store go by plugin.ini's. Give both the same."
                    ),
                )
                .plugin(id.as_str()),
            );
        }
    }
    Ok(())
}

pub fn code_plugins(
    context: &Context,
    ids: &[&String],
    release: Option<&Release>,
    report: &mut Report,
) -> Result<(), String> {
    let mut own = Context::new(
        Target {
            add: context.target.add,
            ..Target::default()
        },
        context.out.clone(),
    );
    own.in_memory = context.in_memory;
    let session = Session::new(&own, &Variant::x4())?;
    for id in ids {
        let info = session
            .device
            .kernel
            .plugins()
            .iter()
            .find(|p| p.manifest.id.as_str() == id.as_str())
            .cloned();
        let Some(info) = info else {
            report.add(
                Finding::error(
                    "package.release",
                    id.as_str(),
                    "No such plugin in the app: add it in the harness (paperpaw_dev::main), or name it by its id.",
                )
                .plugin(id.as_str()),
            );
            continue;
        };
        report.add(
            Finding::info(
                "package.release",
                "manifest",
                format!(
                    "Version {}; this PaperPaw is SDK {SDK_VERSION}.",
                    info.manifest.version
                ),
            )
            .plugin(id.as_str()),
        );
        if let Some(release) = release {
            let line = format!("{}.{}", release.version.major, release.version.minor);
            // A plugin with code is loaded by its file's fingerprint
            // (`paperpaw-sign plugin`), not by a version it names.
            report.add(
                Finding::info(
                    "package.release",
                    "manifest",
                    format!(
                        "PaperPaw {line} loads a plugin with code by its file's fingerprint: build it against paperpaw-sdk {line} with that release's compiler."
                    ),
                )
                .plugin(id.as_str()),
            );
        }
    }
    Ok(())
}
