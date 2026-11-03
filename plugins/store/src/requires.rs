//! What a plugin the store just installed needs (`requires` in its
//! `plugin.ini`, data formats 4: e.g. the Arabic menus need the Arabic
//! letters): the plugins it names that the device lacks are offered in the
//! dialog of [`request`](crate::request), asked as the store's own
//! request, so that the reader decides (*Download* or *Not now*) and sees
//! their sizes. The plugin stays installed either way: the kernel starts a
//! plugin whatever it requires, and the plugin manager says what is
//! missing.
//!
//! Nothing here loops: only a plugin that was installed just now is
//! looked at, a required plugin already on the device is not asked for
//! (so two that require each other end after the second), one the library
//! does not offer for this device is passed over by the dialog, and a
//! plugin that fails to install asks for nothing more. A chain (a
//! required plugin that requires another) is a dialog for each step, as
//! the plugin installed in it is looked at in turn.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::builtin::PLUGIN_DIR;
use paperpaw_sdk::{Command, Commands, Env, Id, requires};

use crate::PLUGIN;

/// Whether plugin `id` is on the device: known to the kernel, or its
/// folder is on the card already (a plugin installed meanwhile).
fn present(env: &Env<'_>, id: &str) -> bool {
    env.plugins().get(id).is_some()
        || env
            .storage()
            .exists(&format!("{PLUGIN_DIR}/{id}/plugin.ini"))
}

/// The plugins `plugin` requires that are not on the device, in the order
/// its `plugin.ini` in `folder` (where it waits to be installed) names
/// them; none where it names none or names them wrongly.
pub(crate) fn missing(env: &Env<'_>, folder: &str, plugin: &str) -> Vec<Id> {
    let path = format!("{folder}/plugin.ini");
    let Ok(text) = paperpaw_sdk::read_file(env.storage(), &path, 16 * 1024) else {
        return Vec::new();
    };
    let Ok(text) = String::from_utf8(text) else {
        return Vec::new();
    };
    let mut section = "";
    let mut value = None;
    for line in text.lines().map(str::trim) {
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            section = name.trim();
        } else if section == "plugin"
            && let Some((key, v)) = line.split_once('=')
            && key.trim() == "requires"
        {
            value = Some(v.trim());
        }
    }
    let Some(Ok(required)) = value.map(|v| requires::parse(v, plugin)) else {
        return Vec::new();
    };
    required
        .into_iter()
        .filter(|r| !present(env, r.id.as_str()))
        .map(|r| r.id)
        .collect()
}

/// Offers `missing`, which a plugin just installed requires: asks the
/// store's own request for it ([`Command::request_plugins`]), whose
/// dialog says what is offered and what it takes.
pub(crate) fn offer(missing: Vec<Id>, out: &mut Commands) {
    if !missing.is_empty() {
        out.push(Command::request_plugins(PLUGIN.clone(), missing));
    }
}
