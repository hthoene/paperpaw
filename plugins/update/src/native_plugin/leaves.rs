//! The plugins an update leaves out, on the host (the rules are in
//! `native_leaves`): the device's list for an update on the card (`core.device`
//! op 16, read at once after the check), what the server's answer says for an
//! update not downloaded yet, their names from `core.registry`, and the *New*
//! mark on the update entry in the settings (`set_item_state`).
use super::{MOST_PAGES, NativeUpdate, waiting::list};
use crate::{
    native_leaves::{self, MAX_LEAVES},
    native_online::{OFFER, Offer},
};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::native::{
    Error, Host,
    commands::{CommandSender, ItemState},
    device::DeviceCursor,
    storage::{self, Root},
};
use paperpaw_ui_kit::{
    Messages,
    mem::{SERVICE_BUFFER, buffer, join, push},
    text::{concat, copy},
};

/// The update entry in the settings (the action `update.open`, relative to the
/// plugin).
const ENTRY: &str = "open";
/// The mark on the entry: *New*.
const BADGE: &str = "update.badge";

impl NativeUpdate {
    /// The plugins the device says the update it has just checked leaves out, by
    /// their names, `, ` between; empty where it leaves none out. Read at once
    /// after the check: the list is the last check's only. A cursor of an
    /// earlier check starts the read again (a few times, `native_leaves::read_all`).
    ///
    /// # Errors
    /// [`Error::Busy`] while a check is still running or a newer check replaced
    /// the result (`ERR_NOT_FOUND`; the caller waits for the running check and
    /// submits the file's check again, never reads the old request again); else the host's refusal (no finished check, a foreign
    /// image, a cursor that stays stale), or [`Error::NoMemory`].
    pub(super) fn left_by_check(&mut self, host: &Host<'_>) -> Result<String, Error> {
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let dev = self.device(host)?;
        // TODO: call op 16 only when `FileCheck::leaves_out()` (dev_check.flags bit 5, draft-36 carrier) says so, once it is on integration
        let ids = native_leaves::read_all(DeviceCursor::start(), |cursor, ids| {
            let page = dev.leaves_out(host, cursor, &mut input, &mut out)?;
            for plugin in page.rows() {
                if ids.len() < MAX_LEAVES {
                    push(ids, copy(plugin.id())?)?;
                }
            }
            Ok(page.end().next_cursor())
        })?;
        self.leaves_named(host, &ids, None)
    }

    /// The plugins the update `offer` the server named leaves out, by their
    /// names (each with the server's reason where it gives one and `reasons`);
    /// empty where it leaves none out or the answer does not say. A list that
    /// cannot be read is none: the device's check names them once the update is
    /// downloaded.
    pub(super) fn left_by_offer(
        &mut self,
        host: &Host<'_>,
        offer: &Offer,
        reasons: bool,
    ) -> Result<String, Error> {
        match self.offer_names(host, offer, reasons) {
            Err(Error::NoMemory) => Err(Error::NoMemory),
            Err(_) => Ok(String::new()),
            Ok(names) => Ok(names),
        }
    }

    fn offer_names(
        &mut self,
        host: &Host<'_>,
        offer: &Offer,
        reasons: bool,
    ) -> Result<String, Error> {
        if offer.plugins.is_empty() {
            return Ok(String::new());
        }
        let ids = self.builtin_ids(host)?;
        let left = native_leaves::of_offer(ids.iter().map(String::as_str), offer)?;
        self.leaves_named(host, &left, reasons.then_some(offer))
    }

    /// The ids of the plugins built in now.
    fn builtin_ids(&mut self, host: &Host<'_>) -> Result<Vec<String>, Error> {
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let dev = self.device(host)?;
        let mut ids = Vec::new();
        let mut cursor = DeviceCursor::start();
        for _ in 0..MOST_PAGES {
            let page = match dev.builtins(host, cursor, &mut input, &mut out) {
                Ok(page) => page,
                Err(Error::NotFound) => break,
                Err(error) => return Err(error),
            };
            for plugin in page.rows() {
                push(&mut ids, copy(plugin.id())?)?;
            }
            match page.end().next_cursor() {
                Some(next) => cursor = next,
                None => break,
            }
        }
        Ok(ids)
    }

    /// The names of the plugins `ids` (a plugin's own name where the registry
    /// knows it, else its id), `, ` between; with the reason the answer `offer`
    /// gives where there is one.
    fn leaves_named(
        &mut self,
        host: &Host<'_>,
        ids: &[String],
        offer: Option<&Offer>,
    ) -> Result<String, Error> {
        let mut wanted: Vec<(&str, &str)> = Vec::new();
        wanted
            .try_reserve_exact(ids.len())
            .map_err(|_| Error::NoMemory)?;
        wanted.extend(ids.iter().map(|id| (id.as_str(), id.as_str())));
        let mut names = self.shown_each(host, &wanted)?;
        if let Some(offer) = offer {
            let mut messages = Messages::new()?;
            for (name, id) in names.iter_mut().zip(ids) {
                if let Some(why) = native_leaves::why(offer, id) {
                    let reason = messages.copy(host, why.key())?;
                    *name = native_leaves::with_reason(name, &reason)?;
                }
            }
        }
        join(&names, ", ")
    }

    /// Shows the *New* mark on the update entry while an update the server named
    /// is kept (`offer.txt`), and takes it off when there is none. The host keeps
    /// it across restarts. Callers tolerate a refusal (a host that shows no item
    /// states is no failure of the update).
    pub(super) fn mark_new(host: &Host<'_>) -> Result<(), Error> {
        let waiting = storage::size(host, Root::Data, OFFER).is_ok();
        let state = if waiting {
            ItemState::new().keep().badge(BADGE, &[])
        } else {
            ItemState::new()
        };
        let mut commands = [0; 256];
        let mut args = [0; 512];
        CommandSender::new(host, &mut commands).set_item_state(ENTRY, &state, &mut args)
    }
}

/// `help` followed by the sentence `key` that names the plugins `names`.
pub(super) fn with_leaves(
    host: &Host<'_>,
    messages: &mut Messages,
    help: &str,
    key: &str,
    names: &str,
) -> Result<String, Error> {
    if names.is_empty() {
        return copy(help);
    }
    let text = messages.format(host, key, &list(names))?;
    if help.is_empty() {
        Ok(text)
    } else {
        concat(&[help, " ", &text])
    }
}

/// The sentence `key` that names the plugins `names`, then `help`: where the
/// footer is cut at six lines it is the end that goes.
pub(super) fn leaves_first(
    host: &Host<'_>,
    messages: &mut Messages,
    help: &str,
    key: &str,
    names: &str,
) -> Result<String, Error> {
    if names.is_empty() {
        return copy(help);
    }
    let text = messages.format(host, key, &list(names))?;
    concat(&[&text, " ", help])
}

#[cfg(test)]
mod tests {
    use super::super::install_label;
    use crate::native_check::{Image, Signer};
    use alloc::string::ToString;
    use core::cmp::Ordering;

    fn image(signer: Signer, order: Ordering, leaves: &str) -> Image {
        Image {
            path: "updates/a.bin".to_string(),
            version: "0.5.0".to_string(),
            signer,
            order,
            leaves: leaves.to_string(),
        }
    }

    #[test]
    fn an_update_that_leaves_plugins_out_says_so_on_its_button() {
        let label = |signer, order, leaves| install_label(&image(signer, order, leaves));
        assert_eq!(
            label(Signer::PaperPaw, Ordering::Greater, ""),
            "update.install"
        );
        assert_eq!(
            label(Signer::PaperPaw, Ordering::Greater, "Stats"),
            "update.install-without"
        );
        // Going back, the same again and an own build keep their words: the
        // sentence under the button names the plugins.
        assert_eq!(
            label(Signer::PaperPaw, Ordering::Less, "Stats"),
            "update.install-older"
        );
        assert_eq!(
            label(Signer::PaperPaw, Ordering::Equal, "Stats"),
            "update.reinstall"
        );
        assert_eq!(
            label(Signer::Own([1; 32]), Ordering::Greater, "Stats"),
            "update.install-with-key"
        );
    }
}
