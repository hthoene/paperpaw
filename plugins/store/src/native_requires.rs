//! What a plugin the store installs requires (`requires` in its `plugin.ini`,
//! data format 4): read from the staged folder before the host takes it, so
//! that the plugins the device lacks are offered once it is installed
//! (`command.request_plugins`; the dialog is `native_asks`, by `store`).
//!
//! The host starts a plugin whatever it requires and drops from a request
//! the plugins it has, so the store asks for all that are named: two plugins
//! that require each other end after the second, a chain is one dialog for
//! each step. A wrong list (an id that is no name, the plugin's own, an id
//! twice, more than 16) is read as none, as the host does not load such a
//! manifest either.

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::Storage;
use paperpaw_sdk::install::is_plugin_name;
use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::{
    mem::push,
    text::{concat, copy},
};

use crate::native_queue::read_text;

/// The most plugins a plugin may require (the SDK's bound, and one request).
pub const MAX_REQUIRED: usize = 16;
/// The most of a `plugin.ini` read.
const MAX_MANIFEST: usize = 16 * 1024;

/// The ids `plugin.ini` of `folder` names in `requires`, in its order; none
/// where there is no such file or key, or the list is wrong. The versions
/// it names are the host's to judge.
///
/// # Errors
/// `NoMemory`.
pub fn required(storage: &dyn Storage, folder: &str, plugin: &str) -> Result<Vec<String>, Error> {
    let path = concat(&[folder, "/plugin.ini"])?;
    let Some(text) = read_text(storage, &path, MAX_MANIFEST)? else {
        return Ok(Vec::new());
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
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let mut ids: Vec<String> = Vec::new();
    for part in value.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let mut words = part.split_whitespace();
        let id = words.next().unwrap_or_default();
        if !is_plugin_name(id)
            || id == plugin
            || words.nth(1).is_some()
            || ids.iter().any(|seen| seen == id)
            || ids.len() == MAX_REQUIRED
        {
            return Ok(Vec::new());
        }
        push(&mut ids, copy(id)?)?;
    }
    Ok(ids)
}
