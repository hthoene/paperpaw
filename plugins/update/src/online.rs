//! Updates from the internet: the update server (paperpaw.org unless the
//! setting [`SERVER`] names another) is asked for a newer firmware with
//! the same plugins, and the update file it names is downloaded into
//! `updates`, where it is installed as one copied there would be.
//!
//! Nothing is sent unless the reader asks: on the update page, or through
//! the action `update.check` (e.g. from a rule); and, once an update so
//! found waits for plugins, once a day at a cheap moment until it no
//! longer waits (module `waiting`). The request is a `POST` to
//! `<server>/api/v1/updates` with what the device runs:
//!
//! ```json
//! {"project": "paperpaw-xteink", "chip": 5, "version": "0.4.0",
//!  "plugins": ["paperpaw-plugin-stats 0.4.0", "..."],
//!  "abi": "f1f6cd5dd377bb4d", "compiler": "6a1f03c2",
//!  "code": ["hello 0.1.0", "..."], "trigger": "manual",
//!  "first": ["day", "week"], "age": "new"}
//! ```
//!
//! `abi` is the fingerprint native code plugins must have to run here
//! (ADR 0024), `compiler` the compiler they must name (ADR 0026), `code`
//! the code plugins installed with their versions, so
//! the server can say which have builds for an update; both are left out
//! where the firmware loads no code plugins. `trigger`, `first` and `age`
//! count devices without knowing them (module `count`).
//!
//! The answer names the newest update for it, or none (`version`
//! empty, or no newer than the one running):
//!
//! ```json
//! {"version": "0.5.0", "url": "https://…/paperpaw-x4-0.5.0-update.bin",
//!  "size": 6194784, "plugins": ["paperpaw-plugin-stats 0.5.0", "..."],
//!  "missing": ["paperpaw-plugin-chess not-ready"]}
//! ```
//!
//! `url` may be a path on the server (`/api/v1/updates/…`).
//! `abi` and `compiler` may say what code plugins must have to run on the
//! update: its fingerprint (16 hexadecimal digits) and its compiler
//! (`paperpaw_native::compiler::id`, 8). A build fetched for it is placed
//! only where it has them; where the answer does not say, only where it
//! has the running firmware's, which an update of the same plugin
//! interface shares.
//! `waiting` may name code plugins of the question (`id version`) that
//! have no build for the update yet: with one of them still in use, or
//! with a crate built in now `missing` as `not-ready`, the update waits
//! for its plugins (module `waiting`): a search in the background does not
//! offer it, the update page says so.
//! `plugins` are those the update has built in; what the device has and
//! they lack cannot come along, and `missing` may say why (`unknown`: not
//! in the plugin library; `not-ready`: not built for that version yet).
//! The downloaded file's own list is what counts when it is installed
//! (see [`Update::leaves_out`](paperpaw_sdk::firmware::Update)); its
//! signature is checked then, as always.
//!
//! The answer is kept in the plugin's `offer.txt` until it is downloaded
//! or no longer newer. A `version` that is no version (SemVer) is no
//! answer: it names the file the update is downloaded to.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::event::Event;
use paperpaw_sdk::firmware::{Firmware, Plugins, UPDATE_DIR, compare_versions};
use paperpaw_sdk::native::Native;
use paperpaw_sdk::replies::HttpReply;
use paperpaw_sdk::setting::{Setting, Text};
use paperpaw_sdk::{Arg, Command, Commands, Env, HttpRequest, Params, PluginKind, Storage, files};

use crate::count::{self, Trigger};
use crate::fetch::{self, Build};
use crate::{OFFERED, PAGE, file, read_pair, waiting, write_pair};

/// Where updates are asked for.
pub const SERVER: Setting<Text> = Setting::new("update.server");
/// Whether the update server may offer betas too (a device running one
/// gets them anyway): off, betas are for testing.
pub const BETAS: Setting<bool> = Setting::new("update.betas");
/// The update server PaperPaw asks unless [`SERVER`] says otherwise.
pub const DEFAULT_SERVER: &str = "https://paperpaw.org";
/// The answer to the question for an update.
pub(crate) const CHECKED: Event<HttpReply> = Event::new("update.checked");
/// The update file, downloaded.
pub(crate) const DOWNLOADED: Event<HttpReply> = Event::new("update.downloaded");
/// The newest update the server named, kept until it is downloaded.
const OFFER: &str = "offer.txt";
/// The download until it is whole, not seen as an update before.
const PART: &str = "updates/.download";
/// The longest version taken from the server.
const MAX_VERSION: usize = 64;
/// Set on a question asked in the background (by the action): what it
/// finds is offered by opening the page.
const QUIET: &str = "quiet";

/// An update the server named.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Offer {
    pub version: String,
    pub url: String,
    /// Its size in bytes, 0 if not said.
    pub size: u64,
    /// The plugins it has built in.
    pub plugins: Plugins,
    /// Why plugins it lacks are missing: a crate and a word a line.
    pub missing: String,
    /// The code plugins of this device with no build for it yet: an id
    /// and a version a line.
    pub waiting: String,
    /// The builds for it of the code plugins this device has, and of
    /// plugins built in now that it has as files ([`fetch`]).
    pub builds: Vec<Build>,
    /// The fingerprint and the compiler its code plugins must have, where
    /// the server says.
    pub native: Option<(u64, u32)>,
}

/// Whether `version` is a pre-release (`0.5.0-beta.1`, `0.5.0-dev`).
fn is_pre_release(version: &str) -> bool {
    version.split('+').next().is_some_and(|v| v.contains('-'))
}

impl Offer {
    /// The answer's `body`, if it names an update newer than `firmware`;
    /// a pre-release only where the reader asked for `betas` or the
    /// device runs one already, whatever the server sends.
    pub(crate) fn from_answer(body: &str, firmware: &Firmware, betas: bool) -> Option<Self> {
        let members = paperpaw_sdk::json::object(body);
        let version = paperpaw_sdk::json::text(&members, "version")?
            .trim()
            .to_string();
        let url = paperpaw_sdk::json::text(&members, "url")?
            .trim()
            .to_string();
        if !is_version(&version)
            || url.is_empty()
            || !compare_versions(&version, &firmware.version).is_gt()
            || (is_pre_release(&version) && !betas && !is_pre_release(&firmware.version))
        {
            return None;
        }
        let lines = |key: &str| {
            let mut text = String::new();
            for line in paperpaw_sdk::json::raw_member(body, key)
                .map(paperpaw_sdk::json::string_array)
                .unwrap_or_default()
            {
                text.push_str(line.trim());
                text.push('\n');
            }
            text
        };
        Some(Self {
            version,
            url,
            size: paperpaw_sdk::json::number(&members, "size").map_or(0, |n| n.max(0.0) as u64),
            plugins: Plugins::new(lines("plugins")),
            missing: lines("missing"),
            waiting: lines("waiting"),
            builds: fetch::parse(body),
            native: paperpaw_sdk::json::text(&members, "abi")
                .and_then(|abi| u64::from_str_radix(abi.trim(), 16).ok())
                .zip(
                    paperpaw_sdk::json::text(&members, "compiler")
                        .and_then(|c| u32::from_str_radix(c.trim(), 16).ok()),
                ),
        })
    }

    /// What it waits for here: code plugins in use with no build for it,
    /// crates built in now not built for it yet (module `waiting`).
    pub(crate) fn holding(&self, env: &Env<'_>, firmware: &Firmware) -> waiting::Holding {
        waiting::of_offer(self, env, firmware)
    }

    /// The plugin crates the running firmware has that this update lacks
    /// and that do not come back as files: among the builds `wanted`
    /// (those this device takes, `fetch::wanted`), or fetched already.
    pub(crate) fn leaves_out<'a>(
        &'a self,
        storage: &dyn Storage,
        firmware: &'a Firmware,
        wanted: &[Build],
    ) -> Vec<&'a str> {
        firmware
            .plugins
            .missing_from(&self.plugins)
            .filter(|c| !fetch::comes_back(storage, wanted, c))
            .collect()
    }

    /// Why the server says `name` is missing, if it says.
    pub(crate) fn why_missing(&self, name: &str) -> Option<&str> {
        self.missing.lines().find_map(|line| {
            let (crate_name, reason) = line.split_once(' ')?;
            (crate_name == name).then_some(reason.trim())
        })
    }

    /// The text of [`OFFER`]: the version, the address and the size a line
    /// each, then a line for each plugin (`plugin name version`) and each
    /// reason (`missing name word`).
    fn to_text(&self) -> String {
        let mut text = format!("{}\n{}\n{}\n", self.version, self.url, self.size);
        for c in self.plugins.iter() {
            text.push_str(&format!("plugin {} {}\n", c.name, c.version));
        }
        for line in self.missing.lines() {
            text.push_str(&format!("missing {line}\n"));
        }
        for line in self.waiting.lines() {
            text.push_str(&format!("waiting {line}\n"));
        }
        if let Some((abi, compiler)) = self.native {
            text.push_str(&format!("native {abi:016x} {compiler:08x}\n"));
        }
        text.push_str(&fetch::to_text(&self.builds));
        text
    }

    fn from_text(text: &str) -> Option<Self> {
        let mut lines = text.lines();
        let version = lines.next()?.trim().to_string();
        let url = lines.next()?.trim().to_string();
        let size = lines.next()?.trim().parse().ok()?;
        if !is_version(&version) {
            return None;
        }
        let (mut plugins, mut missing, mut waiting) = (String::new(), String::new(), String::new());
        let mut native = None;
        for line in lines {
            if let Some(rest) = line.strip_prefix("native ") {
                let mut parts = rest.split_whitespace();
                native = parts
                    .next()
                    .and_then(|a| u64::from_str_radix(a, 16).ok())
                    .zip(parts.next().and_then(|c| u32::from_str_radix(c, 16).ok()));
                continue;
            }
            let (list, rest) = if let Some(rest) = line.strip_prefix("plugin ") {
                (&mut plugins, rest)
            } else if let Some(rest) = line.strip_prefix("missing ") {
                (&mut missing, rest)
            } else if let Some(rest) = line.strip_prefix("waiting ") {
                (&mut waiting, rest)
            } else {
                continue;
            };
            list.push_str(rest.trim());
            list.push('\n');
        }
        Some(Self {
            version,
            url,
            size,
            plugins: Plugins::new(plugins),
            missing,
            waiting,
            builds: fetch::from_lines(text.lines()),
            native,
        })
    }
}

/// Whether `version` reads as a version: three numbers, then a
/// pre-release (`-dev`) or build (`+abc`) of letters, digits, `.` and `-`
/// where it has one. Nothing else may name a file in `updates`.
pub(crate) fn is_version(version: &str) -> bool {
    let (numbers, rest) = version.split_once(['-', '+']).unwrap_or((version, ""));
    let mut count = 0;
    let numbers_read = numbers.split('.').all(|number| {
        count += 1;
        !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit())
    });
    numbers_read
        && count == 3
        && version.len() <= MAX_VERSION
        && rest
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-+.".contains(&b))
}

/// Lets go of the update the server named.
pub(crate) fn clear_offer(storage: &dyn Storage) {
    let _ = storage.remove(&file(OFFER));
}

/// Whether an update the server named is kept.
pub(crate) fn has_offer(storage: &dyn Storage) -> bool {
    storage.exists(&file(OFFER))
}

/// Removes what a download cut off (by a restart, a failure) left in
/// `updates`: the file being downloaded, and the part the shell writes
/// it through.
pub(crate) fn clear_download(storage: &dyn Storage) {
    let _ = storage.remove(PART);
    let _ = storage.remove(&files::part_path(PART));
}

/// The update the server named last, if it is still newer than
/// `firmware`.
pub(crate) fn offer(storage: &dyn Storage, firmware: &Firmware) -> Option<Offer> {
    let text = paperpaw_sdk::read_file(storage, &file(OFFER), 64 * 1024).ok()?;
    let offer = Offer::from_text(core::str::from_utf8(&text).ok()?)?;
    compare_versions(&offer.version, &firmware.version)
        .is_gt()
        .then_some(offer)
}

/// The server's address, without a slash at the end.
fn server(env: &Env<'_>) -> String {
    let server = env.settings().read(&SERVER);
    let server = server.trim().trim_end_matches('/');
    String::from(if server.is_empty() {
        DEFAULT_SERVER
    } else {
        server
    })
}

/// The server's name as the reader knows it, e.g. `paperpaw.org`.
pub(crate) fn server_name(env: &Env<'_>) -> String {
    let server = server(env);
    let name = server
        .split_once("://")
        .map_or(server.as_str(), |(_, rest)| rest);
    String::from(name.split('/').next().unwrap_or(name))
}

/// The question's body (see the [module](self)): what runs, and the code
/// plugins installed (`code`, with `native` where they load).
fn question(
    firmware: &Firmware,
    native: Option<&Native>,
    code: &[(&str, String)],
    betas: bool,
    counting: &str,
) -> String {
    let plugins: Vec<String> = firmware
        .plugins
        .iter()
        .map(|c| paperpaw_sdk::json::string(&format!("{} {}", c.name, c.version)))
        .collect();
    let mut body = format!(
        "{{\"project\":{},\"chip\":{},\"version\":{},\"plugins\":[{}]",
        paperpaw_sdk::json::string(&firmware.project),
        firmware.chip,
        paperpaw_sdk::json::string(&firmware.version),
        plugins.join(",")
    );
    if let Some(native) = native {
        let code: Vec<String> = code
            .iter()
            .map(|(id, version)| paperpaw_sdk::json::string(&format!("{id} {version}")))
            .collect();
        body.push_str(&format!(
            ",\"abi\":\"{}\",\"compiler\":\"{:08x}\",\"code\":[{}]",
            native.fingerprint,
            native.compiler,
            code.join(",")
        ));
    }
    // What the server reads to offer betas (a device on one gets them
    // anyway).
    if betas {
        body.push_str(",\"betas\":true");
    }
    body.push_str(counting);
    body.push('}');
    body
}

/// Asks the server for an update; `quiet` from the action, which offers
/// what it finds by opening the page and says nothing otherwise.
pub(crate) fn check(env: &Env<'_>, quiet: bool, trigger: Trigger, out: &mut Commands) {
    let Some(firmware) = env.firmware() else {
        return;
    };
    // Beside another firmware nothing is installed: nobody is asked.
    if crate::dual::declared(env.storage()) {
        return;
    }
    let url = format!("{}/api/v1/updates", server(env));
    let request = HttpRequest::new("POST", &url, CHECKED).header(
        String::from("content-type"),
        String::from("application/json"),
    );
    let code: Vec<(&str, String)> = env
        .plugins()
        .iter()
        .filter(|p| p.kind == PluginKind::Code)
        .map(|p| (p.manifest.id.as_str(), p.manifest.version.to_string()))
        .collect();
    let betas = env.settings().read(&BETAS);
    let unix = paperpaw_sdk::time::unix_seconds(env);
    let counting = count::members(env.storage(), trigger, unix);
    let mut request = request.body(question(firmware, env.native(), &code, betas, &counting));
    // Asked: an update waiting is asked for again not before a day.
    waiting::tried(env.storage(), unix);
    if quiet {
        request = request.param(QUIET, "1");
    }
    out.push(Command::http(request));
}

/// The answer: the update it names kept (or the one kept before let go),
/// and offered if the question was asked in the background.
pub(crate) fn checked(env: &Env<'_>, reply: &HttpReply, out: &mut Commands) {
    let Some(firmware) = env.firmware() else {
        return;
    };
    let storage = env.storage();
    if !(200..300).contains(&reply.status) {
        // An update waiting: tried (see `check`), the next day again,
        // also where the device stays on that long.
        if waiting::read(storage).is_some() {
            waiting::later(out);
        }
        return;
    }
    let text = reply.text();
    let Some(offer) = Offer::from_answer(&text, firmware, env.settings().read(&BETAS)) else {
        let _ = storage.remove(&file(OFFER));
        waiting::clear(storage);
        return;
    };
    let _ = storage.create_dir(&Env::plugin_dir(crate::PLUGIN.as_str()));
    let _ = storage.write(&file(OFFER), offer.to_text().as_bytes());
    // The update is downloaded already and plugins failed for it: the
    // answer's builds are fetched again (the update page stays ready).
    if storage.exists(&path_for(firmware, &offer.version))
        && fetch::retry(env, &offer.version, &offer.builds, offer.native, out)
    {
        let _ = storage.remove(&file(OFFER));
        return;
    }
    // Waiting for plugins: kept and asked for again, offered once ready.
    let holding = offer.holding(env, firmware);
    if !holding.is_empty() {
        let now = paperpaw_sdk::time::unix_seconds(env);
        waiting::answered(storage, &offer.version, holding, now);
        waiting::later(out);
        // Asked for by the reader (the action): shown where it waits.
        if reply.context.get(QUIET).is_some() && env.reader() {
            out.push(Command::OpenScreen(PAGE, Params::new()));
        }
        return;
    }
    waiting::clear(storage);
    let quiet = reply.context.get(QUIET).is_some();
    let key = (String::from("online"), offer.version.clone());
    if quiet && read_pair(storage, OFFERED) != Some(key.clone()) {
        write_pair(storage, OFFERED, &key.0, &key.1);
        if env.reader() {
            out.push(Command::OpenScreen(PAGE, Params::new()));
        } else {
            // Nothing over reading: the offer waits on the update's page.
            let text = env.i18n().format(
                "update.offer.title",
                &[(
                    "version",
                    Arg::Str(&crate::shown_version(env.i18n(), &offer.version)),
                )],
            );
            out.push(Command::Notify(text));
        }
    }
}

/// Where a downloaded update of `version` is kept.
pub(crate) fn path_for(firmware: &Firmware, version: &str) -> String {
    format!("{UPDATE_DIR}/{}-{version}.bin", firmware.project)
}

/// Downloads `offer` into `updates`.
pub(crate) fn download(env: &Env<'_>, offer: &Offer, out: &mut Commands) {
    let _ = env.storage().create_dir(UPDATE_DIR);
    clear_download(env.storage());
    // The plugins of an update before are not this one's.
    fetch::reset(env.storage());
    // A path on the server asked.
    let url = if offer.url.starts_with('/') {
        format!("{}{}", server(env), offer.url)
    } else {
        offer.url.clone()
    };
    let request = HttpRequest::new("GET", &url, DOWNLOADED)
        .save_to(String::from(PART))
        .param("version", offer.version.as_str())
        .param("size", offer.size.to_string());
    // The size the server named is the most the file may be (none named: the
    // largest file the kernel takes).
    let request = if offer.size > 0 {
        request.max_file(offer.size)
    } else {
        request
    };
    out.push(Command::http(request));
}

/// The download done: whole, it takes its name in `updates` and the
/// offer is let go; otherwise it is removed. Either is said, also while
/// the page is closed (downloads go on meanwhile).
pub(crate) fn downloaded(env: &Env<'_>, reply: &HttpReply, out: &mut Commands) {
    let Some(firmware) = env.firmware() else {
        return;
    };
    let storage = env.storage();
    let version = reply.context.get("version").unwrap_or_default();
    let size: u64 = reply
        .context
        .get("size")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let whole = (200..300).contains(&reply.status)
        && storage
            .size(PART)
            .is_ok_and(|got| got > 0 && (size == 0 || got == size));
    let path = path_for(firmware, version);
    if whole && is_version(version) {
        let _ = storage.remove(&path);
        if storage.rename(PART, &path).is_ok() {
            // The code plugins for it next; it is ready once they are here.
            let offer = offer(storage, firmware);
            let builds = offer
                .as_ref()
                .map(|o| fetch::wanted(env, &o.builds))
                .unwrap_or_default();
            let native = offer.and_then(|o| o.native);
            let _ = storage.remove(&file(OFFER));
            if !fetch::start(env, version, &builds, native, out) {
                out.push(Command::Notify(env.i18n().format(
                    "update.downloaded",
                    &[(
                        "version",
                        Arg::Str(&crate::shown_version(env.i18n(), version)),
                    )],
                )));
            }
            return;
        }
    }
    clear_download(storage);
    out.push(Command::Notify(String::from(
        env.i18n().tr("update.download-failed.title"),
    )));
}

#[cfg(test)]
mod tests {
    use alloc::string::String;

    use paperpaw_sdk::firmware::{Firmware, Plugins};
    use paperpaw_sdk::native::{Fingerprint, Native};

    use super::{Offer, is_version, question};

    fn firmware() -> Firmware {
        Firmware::new("0.4.0", "paperpaw-xteink", 5).plugins(Plugins::new(
            "paperpaw-plugin-stats 0.4.0\npaperpaw-plugin-chess 1.0.0\n",
        ))
    }

    #[test]
    fn the_question_says_what_runs() {
        assert_eq!(
            question(&firmware(), None, &[], false, ""),
            "{\"project\":\"paperpaw-xteink\",\"chip\":5,\"version\":\"0.4.0\",\
             \"plugins\":[\"paperpaw-plugin-stats 0.4.0\",\"paperpaw-plugin-chess 1.0.0\"]}"
        );
        // Where code plugins load: the fingerprint and those installed.
        let mut native = Native::new("code.ppn");
        native.fingerprint = Fingerprint(0xf1f6_cd5d_d377_bb4d);
        native.compiler = 0x6a1f_03c2;
        let code = [("hello", String::from("0.1.0"))];
        assert_eq!(
            question(&firmware(), Some(&native), &code, false, ""),
            "{\"project\":\"paperpaw-xteink\",\"chip\":5,\"version\":\"0.4.0\",\
             \"plugins\":[\"paperpaw-plugin-stats 0.4.0\",\"paperpaw-plugin-chess 1.0.0\"],\
             \"abi\":\"f1f6cd5dd377bb4d\",\"compiler\":\"6a1f03c2\",\"code\":[\"hello 0.1.0\"]}"
        );
    }

    /// The fingerprint and compiler an update's code plugins must have,
    /// where the server says, kept with the offer.
    #[test]
    fn an_update_names_what_its_code_plugins_must_have() {
        let body = r#"{"version": "0.5.0", "url": "u", "abi": "0002aabbccddeeff", "compiler": "6a1f03c2"}"#;
        let offer = Offer::from_answer(body, &firmware(), false).unwrap();
        assert_eq!(offer.native, Some((0x0002_aabb_ccdd_eeff, 0x6a1f_03c2)));
        assert_eq!(Offer::from_text(&offer.to_text()), Some(offer));
        let unsaid = Offer::from_answer(r#"{"version": "0.5.0", "url": "u"}"#, &firmware(), false);
        assert_eq!(unsaid.unwrap().native, None);
    }

    /// Betas asked for where the reader wants them; offered only when
    /// newer, by beta number, and never one back.
    #[test]
    fn betas_are_asked_for_and_offered_only_newer() {
        assert!(question(&firmware(), None, &[], true, "").ends_with(",\"betas\":true}"));
        assert!(!question(&firmware(), None, &[], false, "").contains("betas"));
        let on = |version: &str| {
            let mut firmware = firmware();
            firmware.version = version.into();
            firmware
        };
        let offer = |answer: &str, running: &str| {
            let body = alloc::format!(r#"{{"version": "{answer}", "url": "u"}}"#);
            Offer::from_answer(&body, &on(running), true).map(|o| o.version)
        };
        assert_eq!(
            offer("0.5.0-beta.2", "0.5.0-beta.1").as_deref(),
            Some("0.5.0-beta.2")
        );
        assert_eq!(
            offer("0.5.0-beta.10", "0.5.0-beta.2").as_deref(),
            Some("0.5.0-beta.10")
        );
        assert_eq!(offer("0.5.0", "0.5.0-beta.10").as_deref(), Some("0.5.0"));
        assert_eq!(
            offer("0.5.0-beta.1", "0.4.0").as_deref(),
            Some("0.5.0-beta.1")
        );
        for (answer, running) in [
            ("0.5.0-beta.1", "0.5.0-beta.2"),
            ("0.5.0-beta.2", "0.5.0-beta.10"),
            ("0.5.0-beta.3", "0.5.0"),
            ("0.5.0-beta.1", "0.5.0-beta.1"),
        ] {
            assert_eq!(offer(answer, running), None, "{answer} on {running}");
        }
    }

    /// A pre-release a server offers where the reader did not ask for
    /// betas is not taken, unless the device runs a pre-release already.
    #[test]
    fn betas_are_left_where_the_reader_did_not_ask() {
        let on = |version: &str| {
            let mut firmware = firmware();
            firmware.version = version.into();
            firmware
        };
        let offer = |answer: &str, running: &str| {
            let body = alloc::format!(r#"{{"version": "{answer}", "url": "u"}}"#);
            Offer::from_answer(&body, &on(running), false).map(|o| o.version)
        };
        assert_eq!(offer("0.5.0-beta.1", "0.4.0"), None);
        assert_eq!(offer("0.5.0-rc.1", "0.4.0"), None);
        assert_eq!(offer("0.5.0", "0.4.0").as_deref(), Some("0.5.0"));
        assert_eq!(
            offer("0.5.0-beta.2", "0.5.0-beta.1").as_deref(),
            Some("0.5.0-beta.2")
        );
        assert_eq!(
            offer("0.5.0-beta.1", "0.4.0-dev").as_deref(),
            Some("0.5.0-beta.1")
        );
    }

    #[test]
    fn an_answer_with_an_update() {
        let body = r#"{"version": "0.5.0", "url": "https://paperpaw.org/u.bin", "size": 6194784,
            "plugins": ["paperpaw-plugin-stats 0.5.0"],
            "missing": ["paperpaw-plugin-chess not-ready"]}"#;
        let firmware = firmware();
        let offer = Offer::from_answer(body, &firmware, false).unwrap();
        assert_eq!(offer.version, "0.5.0");
        assert_eq!(offer.size, 6_194_784);
        let storage = paperpaw_sdk::MemoryStorage::new();
        assert_eq!(
            offer.leaves_out(&storage, &firmware, &offer.builds),
            ["paperpaw-plugin-chess"]
        );
        assert_eq!(
            offer.why_missing("paperpaw-plugin-chess"),
            Some("not-ready")
        );
        assert_eq!(Offer::from_text(&offer.to_text()), Some(offer));
    }

    #[test]
    fn answers_without_a_newer_one() {
        let firmware = firmware();
        for body in [
            r#"{"version": ""}"#,
            r#"{"version": "0.4.0", "url": "https://paperpaw.org/u.bin"}"#,
            r#"{"version": "0.3.0", "url": "https://paperpaw.org/u.bin"}"#,
            r#"{"version": "0.5.0"}"#,
            // No versions: they would name the file downloaded to.
            r#"{"version": "0.5.0/../../x", "url": "u"}"#,
            r#"{"version": "1.0.0-a/b", "url": "u"}"#,
            r#"{"version": "9", "url": "u"}"#,
            "not json",
        ] {
            assert_eq!(Offer::from_answer(body, &firmware, false), None, "{body}");
        }
        let all = Offer::from_answer(
            r#"{"version": "0.5.0", "url": "u", "plugins": ["paperpaw-plugin-stats 0.5.0", "paperpaw-plugin-chess 1.1.0"]}"#,
            &firmware,
            false,
        )
        .unwrap();
        let storage = paperpaw_sdk::MemoryStorage::new();
        assert!(all.leaves_out(&storage, &firmware, &[]).is_empty());
    }

    #[test]
    fn versions_are_read_as_semver() {
        for good in ["0.5.0", "10.20.30", "0.5.0-dev", "1.0.0-rc.1+build-7"] {
            assert!(is_version(good), "{good}");
        }
        for bad in [
            "",
            "0.5",
            "0.5.0.1",
            "a.b.c",
            "0..5",
            "0.5.0/x",
            "0.5.0-a b",
            "0.5.0-a\\b",
        ] {
            assert!(!is_version(bad), "{bad}");
        }
        assert!(!is_version(&alloc::format!("1.0.0-{}", "a".repeat(64))));
    }
}
