//! Literata's bold and italic, which the lean firmware (the X4 family's)
//! leaves out for room: they come as the data plugin `literata-styles`.
//! Until they are on the card, bold and italic words are set in the
//! regular face, so nothing is lost but the look.
//!
//! A firmware without them (a first start after an update, a card set up
//! from CrossPoint's) asks the store's quiet queue for the plugin once the
//! first-run setup is over: it is fetched by itself, unasked, as soon as
//! the network allows (also after later restarts, as the setup's offer
//! is), and said once at its end. The only download the store makes
//! unasked, 0.5 MB, so it is not made in Offline mode. It is tried at most
//! [`TRIES`] times, counted where a page of the store answered (see
//! [`answered`]), not at the starts: a start without a network costs no
//! try, and says once, in a hint, to connect once. A library without it
//! after that is left to the reader, who finds it in the Plugin store.
//!
//! A web flasher's bundle brings it with the firmware, and there is
//! nothing to fetch.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::Commands;
use paperpaw_sdk::Env;

use crate::catalog::Device;
use crate::{PLUGIN, queue};

/// The plugin with the faces.
pub(crate) const PACK: &str = "literata-styles";
/// The faces a firmware with them has built in, and the pack's.
const BUILT_IN: &str = "fonts-default.literata-bold";
const FROM_PACK: &str = "literata-styles.bold";
/// The firmware's regular face: where it is, Literata is the default.
const REGULAR: &str = "fonts-default.literata";
/// How often the store may answer for it at most (it came, or it did not
/// install), over all starts.
const TRIES: u32 = 3;

/// How often the store answered for it.
fn tries_path() -> String {
    format!("{}/styles-tries.txt", Env::plugin_dir(PLUGIN.as_str()))
}

fn tries(env: &Env<'_>) -> u32 {
    paperpaw_sdk::read_file(env.storage(), &tries_path(), 16)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .and_then(|text| text.trim().parse().ok())
        .unwrap_or(0)
}

/// Whether this firmware has Literata but not its bold, and the plugin is
/// not on the card.
fn lacks_styles(env: &Env<'_>) -> bool {
    let fonts = env.fonts();
    fonts.get(REGULAR).is_some()
        && fonts.get(BUILT_IN).is_none()
        && fonts.get(FROM_PACK).is_none()
        && env.plugins().get(PACK).is_none()
}

/// Puts the plugin in the queue, if it is wanted at this start.
pub(crate) fn offer(env: &Env<'_>, out: &mut Commands) {
    // After the first-run setup, which may be setting up the network.
    let setup_done = format!("{}/done", Env::plugin_dir("setup"));
    if !lacks_styles(env)
        || !env.storage().exists(&setup_done)
        || !env.power().online()
        || Device::of(env).is_none()
        || queue::waiting(env).iter().any(|id| id == PACK)
    {
        return;
    }
    if tries(env) >= TRIES {
        return;
    }
    queue::add_quietly(env, &[PACK.to_string()], out);
}

/// Keeps that the store answered for plugin `id` (with `status`, 0 where
/// there was no connection at all): only an answer is a try.
pub(crate) fn answered(env: &Env<'_>, id: &str, status: u16) {
    if id != PACK || status == 0 {
        return;
    }
    let _ = env.storage().create_dir(&Env::plugin_dir(PLUGIN.as_str()));
    let _ = env
        .storage()
        .write(&tries_path(), (tries(env) + 1).to_string().as_bytes());
}

/// Where the hint was said is kept: it is said once.
fn hinted_path() -> String {
    format!("{}/styles-hinted.txt", Env::plugin_dir(PLUGIN.as_str()))
}

/// Whether the hint to connect once is to be said now: the first time
/// only, as it is kept.
pub(crate) fn hint_due(env: &Env<'_>) -> bool {
    if env.storage().exists(&hinted_path()) {
        return false;
    }
    let _ = env.storage().create_dir(&Env::plugin_dir(PLUGIN.as_str()));
    let _ = env.storage().write(&hinted_path(), b"1");
    true
}

/// The names of `list` (joined with `, `) without `name`, if it is one.
pub(crate) fn without(list: &str, name: &str) -> Option<String> {
    let mut found = false;
    let rest: Vec<&str> = list
        .split(", ")
        .filter(|item| {
            let is = *item == name;
            found |= is;
            !is
        })
        .collect();
    found.then(|| rest.join(", "))
}
