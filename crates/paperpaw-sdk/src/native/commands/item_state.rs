//! `command.set_item_state` (60): what menus show of one of the plugin's own
//! actions or settings (`docs/abi/screens-input.md` 3.1).
use super::{CommandSender, Error, check_id, wire};
use crate::native::formatting::{Arg, encode_args};

/// What a menu shows of one item: faint, hidden, a switch, another label or a
/// badge. The default state removes the item's state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct ItemState<'a> {
    flags: u32,
    label: &'a str,
    label_args: &'a [Arg<'a>],
    badge: &'a str,
    badge_args: &'a [Arg<'a>],
}

impl<'a> ItemState<'a> {
    /// The default state: nothing changed, none kept.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            flags: 0,
            label: "",
            label_args: &[],
            badge: "",
            badge_args: &[],
        }
    }
    /// Not shown in menus (the item the user listed in a menu of their own stays).
    #[must_use]
    pub const fn hidden(mut self) -> Self {
        self.flags |= wire::COMMAND_ITEM_HIDDEN;
        self
    }
    /// Shown faint: it cannot be chosen.
    #[must_use]
    pub const fn faint(mut self) -> Self {
        self.flags |= wire::COMMAND_ITEM_FAINT;
        self
    }
    /// Shown with a switch, on or off. Not for a setting: a Boolean setting
    /// shows its own value.
    #[must_use]
    pub const fn switch(mut self, on: bool) -> Self {
        self.flags |= wire::COMMAND_ITEM_SWITCH;
        if on {
            self.flags |= wire::COMMAND_ITEM_ON;
        } else {
            self.flags &= !wire::COMMAND_ITEM_ON;
        }
        self
    }
    /// The host keeps this state across restarts until the plugin changes it.
    #[must_use]
    pub const fn keep(mut self) -> Self {
        self.flags |= wire::COMMAND_ITEM_KEEP;
        self
    }
    /// A catalog key of the plugin shown in place of the title, with the
    /// arguments of its message.
    #[must_use]
    pub const fn label(mut self, key: &'a str, args: &'a [Arg<'a>]) -> Self {
        self.label = key;
        self.label_args = args;
        self
    }
    /// A catalog key of a short text at the item's right (a count, "new"),
    /// with the arguments of its message.
    #[must_use]
    pub const fn badge(mut self, key: &'a str, args: &'a [Arg<'a>]) -> Self {
        self.badge = key;
        self.badge_args = args;
        self
    }
}

fn batch(args: &[Arg<'_>], room: &mut [u8]) -> Result<usize, Error> {
    if args.is_empty() {
        Ok(0)
    } else {
        encode_args(args, room)
    }
}

impl CommandSender<'_, '_> {
    /// Stage the state a menu shows for `item`, one of the submitting plugin's
    /// own actions or settings, relative to the plugin. The state replaces the
    /// item's whole state; the default state removes it, the kept copy included.
    /// `arg_scratch` holds the encoded arguments of the label (its first 256
    /// bytes) and of the badge (the next 256): 512 bytes hold both.
    ///
    /// # Errors
    /// Invalid for an empty item, an overlong one or a catalog key over 128
    /// bytes; Invalid for arguments without their key, duplicate argument
    /// names or a second plural selector; NoSpace for short scratch or
    /// arguments over 256 bytes (nothing is sent in these cases); otherwise the
    /// host's refusal: NotFound for an item the plugin did not declare, Invalid
    /// for a switch on a setting, Full beyond the item limits, Unsupported
    /// where no host shows item states.
    pub fn set_item_state(
        &mut self,
        item: &str,
        state: &ItemState<'_>,
        arg_scratch: &mut [u8],
    ) -> Result<(), Error> {
        check_id(item, wire::COMMAND_MAX_ITEM_BYTES)?;
        for (key, args) in [
            (state.label, state.label_args),
            (state.badge, state.badge_args),
        ] {
            if key.len() > wire::COMMAND_MAX_ITEM_KEY_BYTES || (key.is_empty() && !args.is_empty())
            {
                return Err(Error::Invalid);
            }
        }
        let (label_room, badge_room) =
            arg_scratch.split_at_mut(arg_scratch.len().min(wire::COMMAND_MAX_ITEM_ARGS_BYTES));
        // No arguments is no bytes (the wire says "empty for none").
        let label_len = batch(state.label_args, label_room)?;
        let badge_len = batch(state.badge_args, badge_room)?;
        self.submit(wire::Command::SetItemState {
            item,
            flags: state.flags,
            label: state.label,
            label_args: label_room.get(..label_len).ok_or(Error::NoSpace)?,
            badge: state.badge,
            badge_args: badge_room.get(..badge_len).ok_or(Error::NoSpace)?,
        })
    }
}

#[cfg(test)]
#[path = "item_state_tests.rs"]
mod tests;
